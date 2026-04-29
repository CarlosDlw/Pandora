//! Linear VM execution: builtins use [`SymbolOrigin::Builtin`];
//! user-defined callees are rejected until closures/functions exist.

use std::collections::HashMap;

use foundation::{
    diagnostics::{Diagnostic, Diagnostics, Severity},
    span::Span,
};

use crate::hir::symbols::{SymbolId, SymbolOrigin, SymbolTable};

use super::{
    bytecode::Op,
    chunk::Chunk,
    value::Value,
};

#[derive(Debug, Clone)]
struct CallFrame {
    chunk: Chunk,
    ip: usize,
    env: HashMap<SymbolId, Value>,
    scope_frames: Vec<Vec<SymbolId>>,
}

pub struct Vm<'a> {
    chunk: Chunk,
    ip: usize,
    stack: Vec<Value>,
    env: HashMap<SymbolId, Value>,
    scope_frames: Vec<Vec<SymbolId>>,
    symbols: &'a SymbolTable,
    call_stack: Vec<CallFrame>,
}

impl<'a> Vm<'a> {
    pub fn new(chunk: &'a Chunk, symbols: &'a SymbolTable, initial_env: HashMap<SymbolId, Value>) -> Self {
        let mut env = initial_env;
        for idx in 0..u32::MAX {
            let id = SymbolId(idx);
            let Some(symbol) = symbols.symbol(id) else {
                break;
            };
            if symbol.origin == SymbolOrigin::Builtin {
                env.entry(id).or_insert(Value::Builtin(id));
            }
        }
        Self {
            chunk: chunk.clone(),
            ip: 0,
            stack: Vec::new(),
            env,
            scope_frames: vec![Vec::new()],
            symbols,
            call_stack: Vec::new(),
        }
    }

    /// Executes until [`Op::Return`] or end of chunk. On failure returns accumulated diagnostics (no panics).
    pub fn run(&mut self) -> Result<(), Diagnostics> {
        let mut diagnostics = Diagnostics::new();

        loop {
            if self.ip >= self.chunk.code.len() {
                match self.return_from_call_frame() {
                    Ok(true) => continue,
                    Ok(false) => break,
                    Err(d) => {
                        diagnostics.push(d);
                        return Err(diagnostics);
                    }
                }
            }
            let op = self.chunk.code[self.ip].clone();
            let span = self.chunk.spans[self.ip];
            self.ip += 1;

            if let Err(d) = self.step_op(&op, span) {
                diagnostics.push(d);
                return Err(diagnostics);
            }

        }

        if !self.stack.is_empty() {
            diagnostics.push(Diagnostic::new(
                format!(
                    "internal error: stack not empty after execution ({} value(s) left)",
                    self.stack.len()
                ),
                self.last_chunk_span(),
                Severity::Error,
            ));
            return Err(diagnostics);
        }
        Ok(())
    }

    fn last_chunk_span(&self) -> Span {
        self.chunk
            .spans
            .last()
            .copied()
            .unwrap_or_else(|| Span::new_unchecked(foundation::ids::FileId::from_u32(0), 0, 0))
    }

    fn step_op(&mut self, op: &Op, span: Span) -> Result<(), Diagnostic> {
        match op {
            Op::ConstI128(n) => self.stack.push(Value::Int128(*n)),
            Op::ConstU128(n) => self.stack.push(Value::UInt128(*n)),
            Op::ConstBool(b) => self.stack.push(Value::Bool(*b)),
            Op::ConstStr(s) => self.stack.push(Value::Str(s.clone())),
            Op::ConstFloat(f) => self.stack.push(Value::Float(*f)),
            Op::ConstChar(c) => self.stack.push(Value::Char(*c)),
            Op::ConstUnit => self.stack.push(Value::Unit),
            Op::ConstNull => self.stack.push(Value::Null),
            Op::MakeTuple(count) => {
                let count = *count as usize;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(self.pop_one(span)?);
                }
                items.reverse();
                self.stack.push(Value::Tuple(items));
            }
            Op::TupleGet(index) => {
                let tuple = self.pop_one(span)?;
                match tuple {
                    Value::Tuple(items) => {
                        let value = items.get(*index).cloned().ok_or_else(|| {
                            Diagnostic::new("tuple index out of range", span, Severity::Error)
                        })?;
                        self.stack.push(value);
                    }
                    other => {
                        return Err(Diagnostic::new(
                            format!("tuple access on non-tuple value: {:?}", other),
                            span,
                            Severity::Error,
                        ));
                    }
                }
            }
            Op::MakeStruct(type_name, field_names) => {
                let mut fields = HashMap::new();
                for field in field_names.iter().rev() {
                    fields.insert(field.clone(), self.pop_one(span)?);
                }
                self.stack.push(Value::StructInstance {
                    type_name: type_name.clone(),
                    fields,
                });
            }
            Op::StructGet(field) => {
                let base = self.pop_one(span)?;
                match base {
                    Value::Err { message, code } => {
                        let value = match field.as_str() {
                            "message" => Value::Str(message),
                            "code" => Value::Int128(i128::from(code)),
                            _ => {
                                return Err(Diagnostic::new(
                                    format!("unknown err field '{field}'"),
                                    span,
                                    Severity::Error,
                                ))
                            }
                        };
                        self.stack.push(value);
                    }
                    Value::StructInstance { fields, .. } => {
                        let value = fields.get(field).cloned().ok_or_else(|| {
                            Diagnostic::new(
                                format!("unknown struct field '{field}'"),
                                span,
                                Severity::Error,
                            )
                        })?;
                        self.stack.push(value);
                    }
                    other => {
                        return Err(Diagnostic::new(
                            format!("field access on non-struct value: {:?}", other),
                            span,
                            Severity::Error,
                        ));
                    }
                }
            }

            Op::Load(sym) => {
                let v = self
                    .env
                    .get(sym)
                    .cloned()
                    .ok_or_else(|| {
                        Diagnostic::new(
                            "load of uninitialized or missing symbol",
                            span,
                            Severity::Error,
                        )
                    })?;
                self.stack.push(v);
            }

            Op::Bind(sym) => {
                let v = self.pop_one(span)?;
                self.env.insert(*sym, v);
                if let Some(frame) = self.scope_frames.last_mut() {
                    frame.push(*sym);
                }
            }
            Op::MakeClosure(sym) => {
                let function = self.chunk.functions.get(sym).cloned().ok_or_else(|| {
                    Diagnostic::new("function body not found", span, Severity::Error)
                })?;
                self.stack.push(Value::Function {
                    function: Box::new(function),
                    captured: self.env.clone(),
                    self_symbol: Some(*sym),
                });
            }

            Op::Assign(sym) => {
                let sym_info = self.symbols.symbol(*sym).ok_or_else(|| {
                    Diagnostic::new("unknown symbol id in assignment", span, Severity::Error)
                })?;
                if sym_info.origin == SymbolOrigin::Builtin {
                    return Err(Diagnostic::new(
                        "cannot assign to a builtin",
                        span,
                        Severity::Error,
                    ));
                }
                if sym_info.is_const {
                    return Err(Diagnostic::new(
                        "cannot assign to a constant declared with '::'",
                        span,
                        Severity::Error,
                    ));
                }
                let v = self.pop_one(span)?;
                self.env.insert(*sym, v);
            }

            Op::Neg => self.apply_neg(span)?,
            Op::Not => self.apply_not(span)?,
            Op::BitNot => self.apply_bit_not(span)?,

            Op::Add => {
                let rhs = self.pop_one(span)?;
                let lhs = self.pop_one(span)?;
                match (&lhs, &rhs) {
                    (Value::Str(_), _) | (_, Value::Str(_)) => {
                        let result = format!("{}{}", lhs.display_for_print(), rhs.display_for_print());
                        self.stack.push(Value::Str(result));
                    }
                    _ => {
                        self.stack.push(lhs);
                        self.stack.push(rhs);
                        self.apply_bin_checked(
                            span,
                            |a, b| a.checked_add(b),
                            |a, b| a.checked_add(b),
                            |a, b| Some(a + b),
                        )?;
                    }
                }
            }
            Op::Sub => self.apply_bin_checked(
                span,
                |a, b| a.checked_sub(b),
                |a, b| a.checked_sub(b),
                |a, b| Some(a - b),
            )?,
            Op::Mul => self.apply_bin_checked(
                span,
                |a, b| a.checked_mul(b),
                |a, b| a.checked_mul(b),
                |a, b| Some(a * b),
            )?,
            Op::Div => self.apply_div(span)?,
            Op::Mod => self.apply_mod(span)?,
            Op::Pow => self.apply_pow(span)?,
            Op::Eq => self.apply_eq(span)?,
            Op::Ne => self.apply_ne(span)?,
            Op::Lt => self.apply_cmp(span, |ord| ord == std::cmp::Ordering::Less)?,
            Op::Le => self.apply_cmp(span, |ord| ord != std::cmp::Ordering::Greater)?,
            Op::Gt => self.apply_cmp(span, |ord| ord == std::cmp::Ordering::Greater)?,
            Op::Ge => self.apply_cmp(span, |ord| ord != std::cmp::Ordering::Less)?,
            Op::LogicalAnd => self.apply_logical(span, |a, b| a && b)?,
            Op::LogicalOr => self.apply_logical(span, |a, b| a || b)?,
            Op::BitAnd => self.apply_bitwise(span, |a, b| a & b, |a, b| a & b)?,
            Op::BitOr => self.apply_bitwise(span, |a, b| a | b, |a, b| a | b)?,
            Op::BitXor => self.apply_bitwise(span, |a, b| a ^ b, |a, b| a ^ b)?,
            Op::Shl => self.apply_shift(span, true)?,
            Op::Shr => self.apply_shift(span, false)?,
            Op::JumpIfFalse(target) => {
                if *target > self.chunk.code.len() {
                    return Err(Diagnostic::new("invalid jump target", span, Severity::Error));
                }
                let value = self.pop_one(span)?;
                if !is_truthy(&value) {
                    self.ip = *target;
                }
            }
            Op::Jump(target) => {
                if *target > self.chunk.code.len() {
                    return Err(Diagnostic::new("invalid jump target", span, Severity::Error));
                }
                self.ip = *target;
            }

            Op::Call(sym, argc) => {
                let argc = *argc as usize;
                let mut args = Vec::with_capacity(argc);
                for _ in 0..argc {
                    args.push(self.pop_one(span)?);
                }
                args.reverse();

                let symbol = self.symbols.symbol(*sym).ok_or_else(|| {
                    Diagnostic::new("unknown symbol id in call", span, Severity::Error)
                })?;

                if symbol.origin != SymbolOrigin::Builtin {
                    return Err(Diagnostic::new(
                        "calls to user-defined functions are not implemented yet",
                        span,
                        Severity::Error,
                    ));
                }

                let ret = dispatch_builtin(symbol.name.as_str(), &args, span)?;
                self.stack.push(ret);
            }
            Op::CallValue(argc) => {
                let argc = *argc as usize;
                let mut args = Vec::with_capacity(argc);
                for _ in 0..argc {
                    args.push(self.pop_one(span)?);
                }
                args.reverse();
                let callee = self.pop_one(span)?;
                match callee {
                    Value::Builtin(sym) => {
                        let symbol = self.symbols.symbol(sym).ok_or_else(|| {
                            Diagnostic::new("unknown builtin symbol", span, Severity::Error)
                        })?;
                        let ret = dispatch_builtin(symbol.name.as_str(), &args, span)?;
                        self.stack.push(ret);
                    }
                    Value::Function {
                        function,
                        mut captured,
                        self_symbol,
                    } => {
                        for (sym, value) in &self.env {
                            if captured.contains_key(sym) {
                                continue;
                            }
                            let should_link = self
                                .symbols
                                .symbol(*sym)
                                .map(|s| {
                                    s.origin == SymbolOrigin::Builtin
                                        || matches!(s.ty, crate::analyzer::Type::Function { .. })
                                })
                                .unwrap_or(false);
                            if should_link {
                                captured.insert(*sym, value.clone());
                            }
                        }
                        if let Some(sym) = self_symbol {
                            captured.insert(
                                sym,
                                Value::Function {
                                    function: function.clone(),
                                    captured: captured.clone(),
                                    self_symbol: Some(sym),
                                },
                            );
                        }
                        let fn_chunk = *function;
                        if fn_chunk.params.len() != argc {
                            return Err(Diagnostic::new(
                                format!(
                                    "invalid argument count: expected {}, got {}",
                                    fn_chunk.params.len(),
                                    argc
                                ),
                                span,
                                Severity::Error,
                            ));
                        }
                        for (param, arg) in fn_chunk.params.iter().zip(args.into_iter()) {
                            captured.insert(*param, arg);
                        }
                        let previous = CallFrame {
                            chunk: self.chunk.clone(),
                            ip: self.ip,
                            env: std::mem::take(&mut self.env),
                            scope_frames: std::mem::take(&mut self.scope_frames),
                        };
                        self.call_stack.push(previous);
                        self.chunk = fn_chunk.chunk;
                        self.ip = 0;
                        self.env = captured;
                        self.scope_frames = vec![Vec::new()];
                    }
                    other => {
                        return Err(Diagnostic::new(
                            format!("attempted call on non-function value: {:?}", other),
                            span,
                            Severity::Error,
                        ));
                    }
                }
            }

            Op::Pop => {
                let _ = self.pop_one(span)?;
            }
            Op::Dup => {
                let value = self.stack.last().cloned().ok_or_else(|| {
                    Diagnostic::new("stack underflow (dup)", span, Severity::Error)
                })?;
                self.stack.push(value);
            }

            Op::EnterScope => {
                self.scope_frames.push(Vec::new());
            }

            Op::ExitScope => {
                if self.scope_frames.len() <= 1 {
                    return Err(Diagnostic::new(
                        "scope stack underflow (internal bytecode error)",
                        span,
                        Severity::Error,
                    ));
                }
                let frame = self.scope_frames.pop().expect("checked len");
                for sym in frame.into_iter().rev() {
                    self.env.remove(&sym);
                }
            }

            Op::Return => {
                self.return_from_call_frame()?;
            }
        }
        Ok(())
    }

    fn return_from_call_frame(&mut self) -> Result<bool, Diagnostic> {
        if let Some(frame) = self.call_stack.pop() {
            let ret = self.stack.pop().unwrap_or(Value::Unit);
            self.chunk = frame.chunk;
            self.ip = frame.ip;
            self.env = frame.env;
            self.scope_frames = frame.scope_frames;
            self.stack.push(ret);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn apply_neg(&mut self, span: Span) -> Result<(), Diagnostic> {
        let v = self.pop_one(span)?;
        let out = match v {
            Value::Int128(i) => Value::Int128(i.checked_neg().ok_or_else(|| {
                Diagnostic::new("integer overflow in unary negation", span, Severity::Error)
            })?),
            Value::Float(f) => Value::Float(-f),
            Value::UInt128(_) => {
                return Err(Diagnostic::new(
                    "invalid operand for unary '-' (unsigned)",
                    span,
                    Severity::Error,
                ));
            }
            other => {
                return Err(Diagnostic::new(
                    format!("invalid operand for unary '-' (got {})", builtin_type_name(&other)),
                    span,
                    Severity::Error,
                ));
            }
        };
        self.stack.push(out);
        Ok(())
    }

    fn apply_not(&mut self, span: Span) -> Result<(), Diagnostic> {
        let v = self.pop_one(span)?;
        match v {
            Value::Bool(b) => {
                self.stack.push(Value::Bool(!b));
                Ok(())
            }
            other => Err(Diagnostic::new(
                format!("invalid operand for logical '!': {}", builtin_type_name(&other)),
                span,
                Severity::Error,
            )),
        }
    }

    fn apply_bit_not(&mut self, span: Span) -> Result<(), Diagnostic> {
        let v = self.pop_one(span)?;
        match v {
            Value::Int128(i) => {
                self.stack.push(Value::Int128(!i));
                Ok(())
            }
            Value::UInt128(u) => {
                self.stack.push(Value::UInt128(!u));
                Ok(())
            }
            other => Err(Diagnostic::new(
                format!("invalid operand for bitwise '~': {}", builtin_type_name(&other)),
                span,
                Severity::Error,
            )),
        }
    }

    fn apply_bin_checked(
        &mut self,
        span: Span,
        signed_int: fn(i128, i128) -> Option<i128>,
        unsigned_int: fn(u128, u128) -> Option<u128>,
        float_op: fn(f64, f64) -> Option<f64>,
    ) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        let out = match (lhs, rhs) {
            (Value::Int128(a), Value::Int128(b)) => {
                let r = signed_int(a, b).ok_or_else(|| {
                    Diagnostic::new("integer overflow or invalid operation", span, Severity::Error)
                })?;
                Value::Int128(r)
            }
            (Value::UInt128(a), Value::UInt128(b)) => {
                let r = unsigned_int(a, b).ok_or_else(|| {
                    Diagnostic::new("integer overflow or invalid operation", span, Severity::Error)
                })?;
                Value::UInt128(r)
            }
            (Value::Float(a), Value::Float(b)) => {
                let r = float_op(a, b).ok_or_else(|| {
                    Diagnostic::new("floating-point operation invalid", span, Severity::Error)
                })?;
                Value::Float(r)
            }
            (l, r) => {
                return Err(Diagnostic::new(
                    format!("invalid operands for arithmetic: {:?} and {:?}", l, r),
                    span,
                    Severity::Error,
                ));
            }
        };
        self.stack.push(out);
        Ok(())
    }

    fn apply_div(&mut self, span: Span) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        let out = match (lhs, rhs) {
            (Value::Int128(a), Value::Int128(b)) => {
                if b == 0 {
                    return Err(Diagnostic::new("division by zero", span, Severity::Error));
                }
                let r = a.checked_div(b).ok_or_else(|| {
                    Diagnostic::new("integer division overflow", span, Severity::Error)
                })?;
                Value::Int128(r)
            }
            (Value::UInt128(a), Value::UInt128(b)) => {
                if b == 0 {
                    return Err(Diagnostic::new("division by zero", span, Severity::Error));
                }
                Value::UInt128(a / b)
            }
            (Value::Float(a), Value::Float(b)) => {
                if b == 0.0 {
                    return Err(Diagnostic::new("division by zero", span, Severity::Error));
                }
                Value::Float(a / b)
            }
            (l, r) => {
                return Err(Diagnostic::new(
                    format!("invalid operands for division: {:?} and {:?}", l, r),
                    span,
                    Severity::Error,
                ));
            }
        };
        self.stack.push(out);
        Ok(())
    }

    fn apply_mod(&mut self, span: Span) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        let out = match (lhs, rhs) {
            (Value::Int128(a), Value::Int128(b)) => {
                if b == 0 {
                    return Err(Diagnostic::new("modulo by zero", span, Severity::Error));
                }
                Value::Int128(a % b)
            }
            (Value::UInt128(a), Value::UInt128(b)) => {
                if b == 0 {
                    return Err(Diagnostic::new("modulo by zero", span, Severity::Error));
                }
                Value::UInt128(a % b)
            }
            (l, r) => {
                return Err(Diagnostic::new(
                    format!("invalid operands for modulo: {:?} and {:?}", l, r),
                    span,
                    Severity::Error,
                ));
            }
        };
        self.stack.push(out);
        Ok(())
    }

    fn apply_pow(&mut self, span: Span) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        let out = match (lhs, rhs) {
            (Value::Int128(base), Value::Int128(exp)) => {
                let exp_u32 = u32::try_from(exp).map_err(|_| {
                    Diagnostic::new("integer exponent out of range", span, Severity::Error)
                })?;
                Value::Int128(base.checked_pow(exp_u32).ok_or_else(|| {
                    Diagnostic::new("integer overflow in pow", span, Severity::Error)
                })?)
            }
            (Value::UInt128(base), Value::UInt128(exp)) => {
                let exp_u32 = u32::try_from(exp).map_err(|_| {
                    Diagnostic::new("integer exponent out of range", span, Severity::Error)
                })?;
                Value::UInt128(base.checked_pow(exp_u32).ok_or_else(|| {
                    Diagnostic::new("integer overflow in pow", span, Severity::Error)
                })?)
            }
            (Value::Float(base), Value::Float(exp)) => Value::Float(base.powf(exp)),
            (l, r) => {
                return Err(Diagnostic::new(
                    format!("invalid operands for power: {:?} and {:?}", l, r),
                    span,
                    Severity::Error,
                ));
            }
        };
        self.stack.push(out);
        Ok(())
    }

    fn apply_cmp(
        &mut self,
        span: Span,
        predicate: fn(std::cmp::Ordering) -> bool,
    ) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        let ord = match (lhs, rhs) {
            (Value::Int128(a), Value::Int128(b)) => a.cmp(&b),
            (Value::UInt128(a), Value::UInt128(b)) => a.cmp(&b),
            (Value::Float(a), Value::Float(b)) => a
                .partial_cmp(&b)
                .ok_or_else(|| Diagnostic::new("float comparison is invalid", span, Severity::Error))?,
            (Value::Bool(a), Value::Bool(b)) => a.cmp(&b),
            (Value::Char(a), Value::Char(b)) => a.cmp(&b),
            (Value::Str(a), Value::Str(b)) => a.cmp(&b),
            (l, r) => {
                return Err(Diagnostic::new(
                    format!("invalid operands for comparison: {:?} and {:?}", l, r),
                    span,
                    Severity::Error,
                ));
            }
        };
        self.stack.push(Value::Bool(predicate(ord)));
        Ok(())
    }

    fn apply_eq(&mut self, span: Span) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        self.stack.push(Value::Bool(lhs == rhs));
        Ok(())
    }

    fn apply_ne(&mut self, span: Span) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        self.stack.push(Value::Bool(lhs != rhs));
        Ok(())
    }

    fn apply_logical(
        &mut self,
        span: Span,
        op: fn(bool, bool) -> bool,
    ) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        match (lhs, rhs) {
            (Value::Bool(a), Value::Bool(b)) => {
                self.stack.push(Value::Bool(op(a, b)));
                Ok(())
            }
            (l, r) => Err(Diagnostic::new(
                format!("invalid operands for logical op: {:?} and {:?}", l, r),
                span,
                Severity::Error,
            )),
        }
    }

    fn apply_bitwise(
        &mut self,
        span: Span,
        signed_op: fn(i128, i128) -> i128,
        unsigned_op: fn(u128, u128) -> u128,
    ) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        match (lhs, rhs) {
            (Value::Int128(a), Value::Int128(b)) => {
                self.stack.push(Value::Int128(signed_op(a, b)));
                Ok(())
            }
            (Value::UInt128(a), Value::UInt128(b)) => {
                self.stack.push(Value::UInt128(unsigned_op(a, b)));
                Ok(())
            }
            (l, r) => Err(Diagnostic::new(
                format!("invalid operands for bitwise op: {:?} and {:?}", l, r),
                span,
                Severity::Error,
            )),
        }
    }

    fn apply_shift(&mut self, span: Span, is_left: bool) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        match (lhs, rhs) {
            (Value::Int128(a), Value::Int128(b)) => {
                let shift = u32::try_from(b).map_err(|_| {
                    Diagnostic::new("shift amount must be non-negative", span, Severity::Error)
                })?;
                let result = if is_left {
                    a.checked_shl(shift)
                } else {
                    a.checked_shr(shift)
                }
                .ok_or_else(|| Diagnostic::new("shift amount out of range", span, Severity::Error))?;
                self.stack.push(Value::Int128(result));
                Ok(())
            }
            (Value::UInt128(a), Value::UInt128(b)) => {
                let shift = u32::try_from(b).map_err(|_| {
                    Diagnostic::new("shift amount out of range", span, Severity::Error)
                })?;
                let result = if is_left {
                    a.checked_shl(shift)
                } else {
                    a.checked_shr(shift)
                }
                .ok_or_else(|| Diagnostic::new("shift amount out of range", span, Severity::Error))?;
                self.stack.push(Value::UInt128(result));
                Ok(())
            }
            (l, r) => Err(Diagnostic::new(
                format!("invalid operands for shift: {:?} and {:?}", l, r),
                span,
                Severity::Error,
            )),
        }
    }

    fn pop_one(&mut self, span: Span) -> Result<Value, Diagnostic> {
        self.stack.pop().ok_or_else(|| {
            Diagnostic::new("stack underflow (internal bytecode error)", span, Severity::Error)
        })
    }
}

fn dispatch_builtin(name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    match name {
        "print" => {
            let line = args
                .iter()
                .map(Value::display_for_print)
                .collect::<Vec<_>>()
                .join(" ");
            println!("{line}");
            Ok(Value::Unit)
        }
        "len" => {
            let [arg] = args else {
                return Err(Diagnostic::new(
                    format!("len expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            match arg {
                Value::Str(s) => {
                    let n = s.chars().count();
                    let n128 = i128::try_from(n).map_err(|_| {
                        Diagnostic::new("string length exceeds i128 range", span, Severity::Error)
                    })?;
                    Ok(Value::Int128(n128))
                }
                other => Err(Diagnostic::new(
                    format!("len expects str, got {}", builtin_type_name(other)),
                    span,
                    Severity::Error,
                )),
            }
        }
        "error" => {
            if args.len() != 1 && args.len() != 2 {
                return Err(Diagnostic::new(
                    format!("error expects 1 or 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            }
            let message = match &args[0] {
                Value::Str(s) => s.clone(),
                other => {
                    return Err(Diagnostic::new(
                        format!("error message must be str, got {}", builtin_type_name(other)),
                        span,
                        Severity::Error,
                    ))
                }
            };
            let code = if let Some(code_value) = args.get(1) {
                coerce_i32_code(code_value, span, "error")?
            } else {
                1
            };
            Ok(Value::Err { message, code })
        }
        "panic" => {
            if args.len() != 1 && args.len() != 2 {
                return Err(Diagnostic::new(
                    format!("panic expects 1 or 2 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            }
            let message = match &args[0] {
                Value::Str(s) => s.clone(),
                other => {
                    return Err(Diagnostic::new(
                        format!("panic message must be str, got {}", builtin_type_name(other)),
                        span,
                        Severity::Error,
                    ))
                }
            };
            let code = if let Some(code_value) = args.get(1) {
                coerce_i32_code(code_value, span, "panic")?
            } else {
                1
            };
            Err(Diagnostic::new(
                format!("panic: {} (code={})", message, code),
                span,
                Severity::Error,
            ))
        }
        _ => Err(Diagnostic::new(
            format!("unknown builtin '{name}'"),
            span,
            Severity::Error,
        )),
    }
}

fn builtin_type_name(v: &Value) -> &'static str {
    match v {
        Value::Int128(_) => "int",
        Value::UInt128(_) => "uint",
        Value::Bool(_) => "bool",
        Value::Str(_) => "str",
        Value::Float(_) => "float",
        Value::Char(_) => "char",
        Value::Unit => "unit",
        Value::Null => "null",
        Value::Builtin(_) => "function",
        Value::Function { .. } => "function",
        Value::Tuple(_) => "tuple",
        Value::Err { .. } => "err",
        Value::StructInstance { .. } => "struct",
    }
}

fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Int128(i) => *i != 0,
        Value::UInt128(u) => *u != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => !s.is_empty(),
        Value::Char(c) => *c != '\0',
        Value::Unit => false,
        Value::Null => false,
        Value::Builtin(_) | Value::Function { .. } => true,
        Value::Tuple(items) => !items.is_empty(),
        Value::Err { .. } => true,
        Value::StructInstance { .. } => true,
    }
}

fn coerce_i32_code(value: &Value, span: Span, fn_name: &str) -> Result<i32, Diagnostic> {
    match value {
        Value::Int128(n) => i32::try_from(*n).map_err(|_| {
            Diagnostic::new(
                format!("{fn_name} code is out of i32 range"),
                span,
                Severity::Error,
            )
        }),
        Value::UInt128(n) => i32::try_from(*n).map_err(|_| {
            Diagnostic::new(
                format!("{fn_name} code is out of i32 range"),
                span,
                Severity::Error,
            )
        }),
        other => Err(Diagnostic::new(
            format!("{fn_name} code must be i32-compatible, got {}", builtin_type_name(other)),
            span,
            Severity::Error,
        )),
    }
}

/// Run a full program chunk. Requires `chunk.invariant_holds()` and terminates with [`Op::Return`].
pub fn execute(chunk: &Chunk, symbols: &SymbolTable) -> Result<(), Diagnostics> {
    debug_assert!(chunk.invariant_holds());
    let mut vm = Vm::new(chunk, symbols, HashMap::new());
    vm.run()
}

#[cfg(test)]
mod tests {
    use foundation::ids::FileId;

    use crate::hir::symbols::SymbolTable;
    use crate::vm::bytecode::Op;
    use crate::vm::chunk::ChunkBuilder;

    use super::*;

    fn span() -> Span {
        Span::new_unchecked(FileId::from_u32(0), 0, 1)
    }

    #[test]
    fn execute_int_add_returns_empty_stack() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::ConstI128(2), s);
        b.emit(Op::Add, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols).expect("ok");
    }

    #[test]
    fn div_by_zero_is_error() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::ConstI128(0), s);
        b.emit(Op::Div, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        let err = execute(&chunk, &symbols).expect_err("div0");
        assert!(err.iter().any(|d| d.message.contains("division by zero")));
    }

    #[test]
    fn exit_scope_removes_local_binding() {
        let mut symbols = SymbolTable::new();
        let root = symbols.create_scope(None);
        let local = symbols.define(
            root,
            "local".to_string(),
            crate::analyzer::Type::Int { signed: true, bits: 32 },
            crate::hir::SymbolOrigin::User,
            false,
        );

        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::EnterScope, s);
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::Bind(local), s);
        b.emit(Op::ExitScope, s);
        b.emit(Op::Load(local), s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let err = execute(&chunk, &symbols).expect_err("missing local after exit");
        assert!(err.iter().any(|d| d.message.contains("load of uninitialized or missing symbol")));
    }

    #[test]
    fn exit_scope_keeps_outer_binding_alive() {
        let mut symbols = SymbolTable::new();
        let root = symbols.create_scope(None);
        let outer = symbols.define(
            root,
            "outer".to_string(),
            crate::analyzer::Type::Int { signed: true, bits: 32 },
            crate::hir::SymbolOrigin::User,
            false,
        );
        let inner_scope = symbols.create_scope(Some(root));
        let inner = symbols.define(
            inner_scope,
            "outer".to_string(),
            crate::analyzer::Type::Int { signed: true, bits: 32 },
            crate::hir::SymbolOrigin::User,
            false,
        );

        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(10), s);
        b.emit(Op::Bind(outer), s);
        b.emit(Op::EnterScope, s);
        b.emit(Op::ConstI128(20), s);
        b.emit(Op::Bind(inner), s);
        b.emit(Op::ExitScope, s);
        b.emit(Op::Load(outer), s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        execute(&chunk, &symbols).expect("outer binding should remain");
    }

    #[test]
    fn executes_mod_and_comparison_ops() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(5), s);
        b.emit(Op::ConstI128(2), s);
        b.emit(Op::Mod, s);
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::Eq, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols).expect("mod/comparison should execute");
    }

    #[test]
    fn executes_bitwise_and_shift_ops() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstU128(0b1010), s);
        b.emit(Op::ConstU128(0b1100), s);
        b.emit(Op::BitAnd, s);
        b.emit(Op::ConstU128(1), s);
        b.emit(Op::Shl, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols).expect("bitwise/shift should execute");
    }

    #[test]
    fn executes_logical_and_not_ops() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstBool(true), s);
        b.emit(Op::ConstBool(false), s);
        b.emit(Op::LogicalOr, s);
        b.emit(Op::Not, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols).expect("logical ops should execute");
    }

    #[test]
    fn modulo_by_zero_is_error() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(4), s);
        b.emit(Op::ConstI128(0), s);
        b.emit(Op::Mod, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        let err = execute(&chunk, &symbols).expect_err("mod zero");
        assert!(err.iter().any(|d| d.message.contains("modulo by zero")));
    }

    #[test]
    fn jump_if_false_uses_truthy_semantics() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(0), s);
        b.emit(Op::JumpIfFalse(4), s);
        b.emit(Op::ConstI128(99), s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols).expect("should jump and finish");
    }

    #[test]
    fn invalid_jump_target_is_error() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::Jump(999), s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        let err = execute(&chunk, &symbols).expect_err("invalid jump");
        assert!(err.iter().any(|d| d.message.contains("invalid jump target")));
    }

    #[test]
    fn string_concat_two_strings() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstStr("hello".to_string()), s);
        b.emit(Op::ConstStr(" world".to_string()), s);
        b.emit(Op::Add, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols).expect("concat should execute");
    }

    #[test]
    fn string_concat_string_and_int() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstStr("count: ".to_string()), s);
        b.emit(Op::ConstI128(42), s);
        b.emit(Op::Add, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols).expect("concat should execute");
    }

    #[test]
    fn string_concat_int_and_string() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(42), s);
        b.emit(Op::ConstStr(" items".to_string()), s);
        b.emit(Op::Add, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols).expect("concat should execute");
    }

    #[test]
    fn string_concat_string_and_bool() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstStr("flag: ".to_string()), s);
        b.emit(Op::ConstBool(true), s);
        b.emit(Op::Add, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols).expect("concat should execute");
    }

    #[test]
    fn executes_tuple_build_access_and_eq() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::ConstI128(2), s);
        b.emit(Op::MakeTuple(2), s);
        b.emit(Op::Dup, s);
        b.emit(Op::TupleGet(1), s);
        b.emit(Op::Pop, s);
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::ConstI128(2), s);
        b.emit(Op::MakeTuple(2), s);
        b.emit(Op::Eq, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        execute(&chunk, &symbols).expect("tuple ops should execute");
    }

    #[test]
    fn builtin_error_produces_err_value_and_field_access() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::Load(SymbolId(0)), s);
        b.emit(Op::ConstStr("boom".to_string()), s);
        b.emit(Op::CallValue(1), s);
        b.emit(Op::Dup, s);
        b.emit(Op::StructGet("message".to_string()), s);
        b.emit(Op::Pop, s);
        b.emit(Op::StructGet("code".to_string()), s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let mut symbols = SymbolTable::new();
        let root = symbols.create_scope(None);
        symbols.define(
            root,
            "error".to_string(),
            crate::analyzer::Type::Function {
                params: vec![crate::analyzer::Type::Any],
                ret: Box::new(crate::analyzer::Type::Err),
            },
            crate::hir::SymbolOrigin::Builtin,
            true,
        );
        execute(&b.finish(), &symbols).expect("error builtin should execute");
    }

    #[test]
    fn builtin_panic_aborts_execution() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::Load(SymbolId(0)), s);
        b.emit(Op::ConstStr("fatal".to_string()), s);
        b.emit(Op::CallValue(1), s);
        b.emit(Op::Return, s);
        let mut symbols = SymbolTable::new();
        let root = symbols.create_scope(None);
        symbols.define(
            root,
            "panic".to_string(),
            crate::analyzer::Type::Function {
                params: vec![crate::analyzer::Type::Any],
                ret: Box::new(crate::analyzer::Type::Unit),
            },
            crate::hir::SymbolOrigin::Builtin,
            true,
        );
        let err = execute(&b.finish(), &symbols).expect_err("panic must abort");
        assert!(err.iter().any(|d| d.message.contains("panic: fatal")));
    }
}
