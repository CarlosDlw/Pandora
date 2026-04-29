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

#[derive(Debug, Clone)]
struct TryContext {
    call_depth: usize,
    handler_ip: usize,
    stack_len: usize,
    scope_depth: usize,
}

pub struct Vm<'a> {
    chunk: Chunk,
    ip: usize,
    stack: Vec<Value>,
    env: HashMap<SymbolId, Value>,
    scope_frames: Vec<Vec<SymbolId>>,
    symbols: &'a SymbolTable,
    call_stack: Vec<CallFrame>,
    try_stack: Vec<TryContext>,
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
            try_stack: Vec::new(),
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
                if self.recoverable_panic(&d) && self.recover_from_panic(&d) {
                    continue;
                }
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
            Op::MakeArray(count) => {
                let count = *count as usize;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(self.pop_one(span)?);
                }
                items.reverse();
                self.stack.push(Value::Array(items));
            }
            Op::MakeRange(inclusive) => {
                let end = self.pop_one(span)?;
                let start = self.pop_one(span)?;
                let mut items = Vec::new();
                match (start, end) {
                    (Value::Int128(s), Value::Int128(e)) => {
                        let end_bound = if *inclusive { e.saturating_add(1) } else { e };
                        if s < end_bound {
                            let mut i = s;
                            while i < end_bound {
                                items.push(Value::Int128(i));
                                i = i.saturating_add(1);
                            }
                        }
                    }
                    (Value::UInt128(s), Value::UInt128(e)) => {
                        let end_bound = if *inclusive { e.saturating_add(1) } else { e };
                        if s < end_bound {
                            let mut i = s;
                            while i < end_bound {
                                items.push(Value::UInt128(i));
                                i = i.saturating_add(1);
                            }
                        }
                    }
                    (a, b) => {
                        return Err(Diagnostic::new(
                            format!("range bounds must be same integer type, got {:?} and {:?}", a, b),
                            span,
                            Severity::Error,
                        ))
                    }
                }
                self.stack.push(Value::Array(items));
            }
            Op::ArrayExtend => {
                let rhs = self.pop_one(span)?;
                let lhs = self.pop_one(span)?;
                match (lhs, rhs) {
                    (Value::Array(mut base), Value::Array(extra)) => {
                        base.extend(extra);
                        self.stack.push(Value::Array(base));
                    }
                    (a, b) => {
                        return Err(Diagnostic::new(
                            format!("array spread expects arrays, got {:?} and {:?}", a, b),
                            span,
                            Severity::Error,
                        ))
                    }
                }
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
            Op::ArrayGet => {
                let index_value = self.pop_one(span)?;
                let array_value = self.pop_one(span)?;
                let index = to_usize_index(&index_value, span)?;
                match array_value {
                    Value::Array(items) => {
                        let len = items.len();
                        let value = items.get(index).cloned().ok_or_else(|| {
                            Diagnostic::new(
                                format!("index out of bounds: index={}, len={}", index, len),
                                span,
                                Severity::Error,
                            )
                        })?;
                        self.stack.push(value);
                    }
                    Value::Tuple(items) => {
                        let len = items.len();
                        let value = items.get(index).cloned().ok_or_else(|| {
                            Diagnostic::new(
                                format!("tuple index out of bounds: index={}, len={}", index, len),
                                span,
                                Severity::Error,
                            )
                        })?;
                        self.stack.push(value);
                    }
                    other => {
                        return Err(Diagnostic::new(
                            format!("index access on non-array value: {:?}", other),
                            span,
                            Severity::Error,
                        ))
                    }
                }
            }
            Op::ArrayLen => {
                let array_value = self.pop_one(span)?;
                match array_value {
                    Value::Array(items) => {
                        let n = i128::try_from(items.len()).map_err(|_| {
                            Diagnostic::new("array length exceeds i128 range", span, Severity::Error)
                        })?;
                        self.stack.push(Value::Int128(n));
                    }
                    other => {
                        return Err(Diagnostic::new(
                            format!("array length on non-array value: {:?}", other),
                            span,
                            Severity::Error,
                        ))
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
                    Value::Err {
                        message,
                        code,
                        origin,
                        cause,
                    } => {
                        let value = match field.as_str() {
                            "message" => Value::Str(message),
                            "code" => Value::Int128(i128::from(code)),
                            "origin" => Value::Str(origin),
                            "cause" => cause.map(|c| *c).unwrap_or(Value::Null),
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
            Op::ArrayAssign(sym) => {
                let value = self.pop_one(span)?;
                let index_value = self.pop_one(span)?;
                let index = to_usize_index(&index_value, span)?;
                let Some(slot) = self.env.get_mut(sym) else {
                    return Err(Diagnostic::new("array assignment target not found", span, Severity::Error));
                };
                match slot {
                    Value::Array(items) => {
                        if index >= items.len() {
                            return Err(Diagnostic::new(
                                format!("index out of bounds: index={}, len={}", index, items.len()),
                                span,
                                Severity::Error,
                            ));
                        }
                        items[index] = value;
                    }
                    other => {
                        return Err(Diagnostic::new(
                            format!("index assignment on non-array value: {:?}", other),
                            span,
                            Severity::Error,
                        ))
                    }
                }
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
            Op::TryStart(handler) => {
                self.try_stack.push(TryContext {
                    call_depth: self.call_stack.len(),
                    handler_ip: *handler,
                    stack_len: self.stack.len(),
                    scope_depth: self.scope_frames.len(),
                });
            }
            Op::TryEnd => {
                if let Some(last) = self.try_stack.last() {
                    if last.call_depth == self.call_stack.len() {
                        let _ = self.try_stack.pop();
                    }
                }
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

                let ret = dispatch_builtin(self, symbol.name.as_str(), &args, span)?;
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
                        let ret = dispatch_builtin(self, symbol.name.as_str(), &args, span)?;
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
            Op::Swap => {
                if self.stack.len() < 2 {
                    return Err(Diagnostic::new("stack underflow (swap)", span, Severity::Error));
                }
                let top = self.stack.len() - 1;
                self.stack.swap(top, top - 1);
            }
            Op::WrapErr => {
                let message = self.pop_one(span)?;
                let err = self.pop_one(span)?;
                let Value::Str(message) = message else {
                    return Err(Diagnostic::new("WrapErr requires message string", span, Severity::Error));
                };
                let wrapped = wrap_err_value(err, message, None, "wrap".to_string(), span)?;
                self.stack.push(wrapped);
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
            self.try_stack
                .retain(|ctx| ctx.call_depth <= self.call_stack.len());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn recoverable_panic(&self, diagnostic: &Diagnostic) -> bool {
        diagnostic.message.starts_with("panic:") && !self.try_stack.is_empty()
    }

    fn recover_from_panic(&mut self, diagnostic: &Diagnostic) -> bool {
        let current_depth = self.call_stack.len();
        let Some(index) = self
            .try_stack
            .iter()
            .rposition(|ctx| ctx.call_depth <= current_depth)
        else {
            return false;
        };
        let ctx = self.try_stack[index].clone();
        self.try_stack.truncate(index);

        while self.call_stack.len() > ctx.call_depth {
            let Some(frame) = self.call_stack.pop() else {
                break;
            };
            self.chunk = frame.chunk;
            self.ip = frame.ip;
            self.env = frame.env;
            self.scope_frames = frame.scope_frames;
        }

        self.stack.truncate(ctx.stack_len);
        while self.scope_frames.len() > ctx.scope_depth {
            let Some(frame) = self.scope_frames.pop() else {
                break;
            };
            for sym in frame {
                self.env.remove(&sym);
            }
        }
        self.ip = ctx.handler_ip;
        self.stack.push(err_value_from_panic(diagnostic));
        true
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

fn dispatch_builtin(vm: &mut Vm<'_>, name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
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
                Value::Array(items) => {
                    let n128 = i128::try_from(items.len()).map_err(|_| {
                        Diagnostic::new("array length exceeds i128 range", span, Severity::Error)
                    })?;
                    Ok(Value::Int128(n128))
                }
                other => Err(Diagnostic::new(
                    format!("len expects str or array, got {}", builtin_type_name(other)),
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
            Ok(Value::Err {
                message,
                code,
                origin: "error".to_string(),
                cause: None,
            })
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
        "wrap" => {
            if args.len() != 2 && args.len() != 3 {
                return Err(Diagnostic::new(
                    format!("wrap expects 2 or 3 arguments, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            }
            let base_err = args[0].clone();
            let message = match &args[1] {
                Value::Str(s) => s.clone(),
                other => {
                    return Err(Diagnostic::new(
                        format!("wrap message must be str, got {}", builtin_type_name(other)),
                        span,
                        Severity::Error,
                    ))
                }
            };
            let code = if let Some(code_value) = args.get(2) {
                Some(coerce_i32_code(code_value, span, "wrap")?)
            } else {
                None
            };
            wrap_err_value(base_err, message, code, "wrap".to_string(), span)
        }
        "typeof" => {
            let [arg] = args else {
                return Err(Diagnostic::new(
                    format!("typeof expects exactly 1 argument, got {}", args.len()),
                    span,
                    Severity::Error,
                ));
            };
            Ok(Value::Str(canonical_type_name(arg)))
        }
        name if name.starts_with("__meth_is_") || name.starts_with("__meth_iu_") => {
            dispatch_integer_method(name, args, span)
        }
        name if name.starts_with("__meth_f_") => dispatch_float_method(name, args, span),
        name if name.starts_with("__meth_b_") => dispatch_bool_method(name, args, span),
        name if name.starts_with("__meth_c_") => dispatch_char_method(name, args, span),
        name if name.starts_with("__meth_s_") => dispatch_str_method(name, args, span),
        name if name.starts_with("__meth_a_") => dispatch_array_method(name, args, span),
        name if name.starts_with("__meth_fn_") => dispatch_function_method(vm, name, args, span),
        _ => Err(Diagnostic::new(
            format!("unknown builtin '{name}'"),
            span,
            Severity::Error,
        )),
    }
}

fn dispatch_integer_method(name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new("integer method missing receiver", span, Severity::Error));
    };
    let method = name
        .strip_prefix("__meth_is_")
        .or_else(|| name.strip_prefix("__meth_iu_"))
        .unwrap_or(name);
    let signed = matches!(recv, Value::Int128(_));
    let unsigned = matches!(recv, Value::UInt128(_));
    if !signed && !unsigned {
        return Err(Diagnostic::new("integer method receiver must be integer", span, Severity::Error));
    }
    let ri = match recv { Value::Int128(v) => *v, Value::UInt128(v) => *v as i128, _ => 0 };
    let ru = match recv { Value::UInt128(v) => *v, Value::Int128(v) => *v as u128, _ => 0 };
    let ai = |idx: usize| -> Result<i128, Diagnostic> {
        match tail.get(idx) {
            Some(Value::Int128(v)) => Ok(*v),
            Some(Value::UInt128(v)) => i128::try_from(*v).map_err(|_| Diagnostic::new("integer argument out of range", span, Severity::Error)),
            _ => Err(Diagnostic::new("expected integer argument", span, Severity::Error)),
        }
    };
    let au = |idx: usize| -> Result<u128, Diagnostic> {
        match tail.get(idx) {
            Some(Value::UInt128(v)) => Ok(*v),
            Some(Value::Int128(v)) if *v >= 0 => Ok(*v as u128),
            _ => Err(Diagnostic::new("expected unsigned integer argument", span, Severity::Error)),
        }
    };
    let ret_same = |v: i128| if signed { Value::Int128(v) } else { Value::UInt128(v as u128) };
    let ret_same_u = |v: u128| if signed { Value::Int128(v as i128) } else { Value::UInt128(v) };
    let checked = |ok: Option<Value>, label: &str| -> Value {
        if let Some(v) = ok {
            Value::Tuple(vec![v, Value::Null])
        } else {
            Value::Tuple(vec![
                ret_same(0),
                Value::Err { message: format!("{label} overflow"), code: 1, origin: "checked".to_string(), cause: None },
            ])
        }
    };
    let out = match method {
        "add" => if signed { ret_same(ri + ai(0)?) } else { ret_same_u(ru + au(0)?) },
        "sub" => if signed { ret_same(ri - ai(0)?) } else { ret_same_u(ru - au(0)?) },
        "mul" => if signed { ret_same(ri * ai(0)?) } else { ret_same_u(ru * au(0)?) },
        "div" => if signed { ret_same(ri / ai(0)?) } else { ret_same_u(ru / au(0)?) },
        "mod" => if signed { ret_same(ri % ai(0)?) } else { ret_same_u(ru % au(0)?) },
        "neg" => ret_same(-ri),
        "abs" => ret_same(ri.abs()),
        "and" => ret_same_u(ru & au(0)?),
        "or" => ret_same_u(ru | au(0)?),
        "xor" => ret_same_u(ru ^ au(0)?),
        "not" => ret_same_u(!ru),
        "shl" => ret_same_u(ru << (au(0)? as u32)),
        "shr" => ret_same_u(ru >> (au(0)? as u32)),
        "rotl" => ret_same_u(ru.rotate_left(au(0)? as u32)),
        "rotr" => ret_same_u(ru.rotate_right(au(0)? as u32)),
        "eq" => Value::Bool(if signed { ri == ai(0)? } else { ru == au(0)? }),
        "ne" => Value::Bool(if signed { ri != ai(0)? } else { ru != au(0)? }),
        "lt" => Value::Bool(if signed { ri < ai(0)? } else { ru < au(0)? }),
        "le" => Value::Bool(if signed { ri <= ai(0)? } else { ru <= au(0)? }),
        "gt" => Value::Bool(if signed { ri > ai(0)? } else { ru > au(0)? }),
        "ge" => Value::Bool(if signed { ri >= ai(0)? } else { ru >= au(0)? }),
        "cmp" => {
            let ord = if signed { ri.cmp(&ai(0)?) } else { ru.cmp(&au(0)?) };
            let v = match ord {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            };
            Value::Int128(v)
        }
        "min" => if signed { ret_same(ri.min(ai(0)?)) } else { ret_same_u(ru.min(au(0)?)) },
        "max" => if signed { ret_same(ri.max(ai(0)?)) } else { ret_same_u(ru.max(au(0)?)) },
        "clamp" => if signed { ret_same(ri.clamp(ai(0)?, ai(1)?)) } else { ret_same_u(ru.clamp(au(0)?, au(1)?)) },
        "is_zero" => Value::Bool(if signed { ri == 0 } else { ru == 0 }),
        "is_even" => Value::Bool(if signed { ri % 2 == 0 } else { ru % 2 == 0 }),
        "is_odd" => Value::Bool(if signed { ri % 2 != 0 } else { ru % 2 != 0 }),
        "checked_add" => checked(if signed { ri.checked_add(ai(0)?).map(Value::Int128) } else { ru.checked_add(au(0)?).map(Value::UInt128) }, "checked_add"),
        "checked_sub" => checked(if signed { ri.checked_sub(ai(0)?).map(Value::Int128) } else { ru.checked_sub(au(0)?).map(Value::UInt128) }, "checked_sub"),
        "checked_mul" => checked(if signed { ri.checked_mul(ai(0)?).map(Value::Int128) } else { ru.checked_mul(au(0)?).map(Value::UInt128) }, "checked_mul"),
        "checked_div" => checked(if signed { ri.checked_div(ai(0)?).map(Value::Int128) } else { ru.checked_div(au(0)?).map(Value::UInt128) }, "checked_div"),
        "wrapping_add" => if signed { ret_same(ri.wrapping_add(ai(0)?)) } else { ret_same_u(ru.wrapping_add(au(0)?)) },
        "wrapping_sub" => if signed { ret_same(ri.wrapping_sub(ai(0)?)) } else { ret_same_u(ru.wrapping_sub(au(0)?)) },
        "saturating_add" => if signed { ret_same(ri.saturating_add(ai(0)?)) } else { ret_same_u(ru.saturating_add(au(0)?)) },
        "saturating_sub" => if signed { ret_same(ri.saturating_sub(ai(0)?)) } else { ret_same_u(ru.saturating_sub(au(0)?)) },
        "to_i32" => Value::Int128(i128::from(i32::try_from(ri).map_err(|_| Diagnostic::new("to_i32 overflow", span, Severity::Error))?)),
        "to_u32" => Value::UInt128(u128::from(u32::try_from(ru).map_err(|_| Diagnostic::new("to_u32 overflow", span, Severity::Error))?)),
        "to_f32" => Value::Float(if signed { ri as f64 } else { ru as f64 }),
        "to_bool" => Value::Bool(if signed { ri != 0 } else { ru != 0 }),
        "to_str" => Value::Str(recv.display_for_print()),
        _ => return Err(Diagnostic::new(format!("unknown integer method '{method}'"), span, Severity::Error)),
    };
    Ok(out)
}

fn dispatch_float_method(name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new("float method missing receiver", span, Severity::Error));
    };
    let Value::Float(r) = recv else {
        return Err(Diagnostic::new("float method receiver must be float", span, Severity::Error));
    };
    let method = name.strip_prefix("__meth_f_").unwrap_or(name);
    let af = |idx: usize| -> Result<f64, Diagnostic> {
        match tail.get(idx) {
            Some(Value::Float(v)) => Ok(*v),
            _ => Err(Diagnostic::new("expected float argument", span, Severity::Error)),
        }
    };
    let out = match method {
        "add" => Value::Float(*r + af(0)?),
        "sub" => Value::Float(*r - af(0)?),
        "mul" => Value::Float(*r * af(0)?),
        "div" => Value::Float(*r / af(0)?),
        "mod" => Value::Float(*r % af(0)?),
        "neg" => Value::Float(-*r),
        "eq" => Value::Bool(*r == af(0)?),
        "lt" => Value::Bool(*r < af(0)?),
        "gt" => Value::Bool(*r > af(0)?),
        "cmp" => {
            let rhs = af(0)?;
            let cmp = if *r < rhs { -1 } else if *r > rhs { 1 } else { 0 };
            Value::Int128(cmp)
        }
        "abs" => Value::Float(r.abs()),
        "sqrt" => Value::Float(r.sqrt()),
        "pow" => Value::Float(r.powf(af(0)?)),
        "exp" => Value::Float(r.exp()),
        "log" => Value::Float(r.ln()),
        "log10" => Value::Float(r.log10()),
        "floor" => Value::Float(r.floor()),
        "ceil" => Value::Float(r.ceil()),
        "round" => Value::Float(r.round()),
        "trunc" => Value::Float(r.trunc()),
        "fract" => Value::Float(r.fract()),
        "sin" => Value::Float(r.sin()),
        "cos" => Value::Float(r.cos()),
        "tan" => Value::Float(r.tan()),
        "asin" => Value::Float(r.asin()),
        "acos" => Value::Float(r.acos()),
        "atan" => Value::Float(r.atan()),
        "is_nan" => Value::Bool(r.is_nan()),
        "is_inf" => Value::Bool(r.is_infinite()),
        "is_finite" => Value::Bool(r.is_finite()),
        "to_i32" => {
            if !r.is_finite() {
                return Err(Diagnostic::new("to_i32 requires finite float", span, Severity::Error));
            }
            if *r < i32::MIN as f64 || *r > i32::MAX as f64 {
                return Err(Diagnostic::new("to_i32 overflow", span, Severity::Error));
            }
            Value::Int128(i128::from(*r as i32))
        }
        "to_str" => Value::Str(recv.display_for_print()),
        _ => return Err(Diagnostic::new(format!("unknown float method '{method}'"), span, Severity::Error)),
    };
    Ok(out)
}

fn dispatch_bool_method(name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new("bool method missing receiver", span, Severity::Error));
    };
    let Value::Bool(r) = recv else {
        return Err(Diagnostic::new("bool method receiver must be bool", span, Severity::Error));
    };
    let method = name.strip_prefix("__meth_b_").unwrap_or(name);
    let ab = |idx: usize| -> Result<bool, Diagnostic> {
        match tail.get(idx) {
            Some(Value::Bool(v)) => Ok(*v),
            _ => Err(Diagnostic::new("expected bool argument", span, Severity::Error)),
        }
    };
    let out = match method {
        "and" => Value::Bool(*r && ab(0)?),
        "or" => Value::Bool(*r || ab(0)?),
        "xor" => Value::Bool(*r ^ ab(0)?),
        "not" => Value::Bool(!*r),
        "eq" => Value::Bool(*r == ab(0)?),
        "to_i32" => Value::Int128(if *r { 1 } else { 0 }),
        "to_str" => Value::Str(recv.display_for_print()),
        _ => return Err(Diagnostic::new(format!("unknown bool method '{method}'"), span, Severity::Error)),
    };
    Ok(out)
}

fn dispatch_char_method(name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new("char method missing receiver", span, Severity::Error));
    };
    let Value::Char(r) = recv else {
        return Err(Diagnostic::new("char method receiver must be char", span, Severity::Error));
    };
    let method = name.strip_prefix("__meth_c_").unwrap_or(name);
    let ac = |idx: usize| -> Result<char, Diagnostic> {
        match tail.get(idx) {
            Some(Value::Char(v)) => Ok(*v),
            _ => Err(Diagnostic::new("expected char argument", span, Severity::Error)),
        }
    };
    let out = match method {
        "eq" => Value::Bool(*r == ac(0)?),
        "is_digit" => Value::Bool(r.is_ascii_digit()),
        "is_alpha" => Value::Bool(r.is_alphabetic()),
        "is_alnum" => Value::Bool(r.is_alphanumeric()),
        "is_whitespace" => Value::Bool(r.is_whitespace()),
        "to_upper" => Value::Char(r.to_ascii_uppercase()),
        "to_lower" => Value::Char(r.to_ascii_lowercase()),
        "to_i32" => Value::Int128((*r as u32) as i128),
        "to_str" => Value::Str(recv.display_for_print()),
        _ => return Err(Diagnostic::new(format!("unknown char method '{method}'"), span, Severity::Error)),
    };
    Ok(out)
}

fn dispatch_str_method(name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new("str method missing receiver", span, Severity::Error));
    };
    let Value::Str(s) = recv else {
        return Err(Diagnostic::new("str method receiver must be str", span, Severity::Error));
    };
    let method = name.strip_prefix("__meth_s_").unwrap_or(name);
    let astr = |idx: usize| -> Result<&str, Diagnostic> {
        match tail.get(idx) {
            Some(Value::Str(v)) => Ok(v.as_str()),
            _ => Err(Diagnostic::new("expected str argument", span, Severity::Error)),
        }
    };
    let aidx = |idx: usize| -> Result<usize, Diagnostic> {
        match tail.get(idx) {
            Some(Value::UInt128(v)) => usize::try_from(*v).map_err(|_| {
                Diagnostic::new("index does not fit usize", span, Severity::Error)
            }),
            Some(Value::Int128(v)) if *v >= 0 => usize::try_from(*v).map_err(|_| {
                Diagnostic::new("index does not fit usize", span, Severity::Error)
            }),
            _ => Err(Diagnostic::new("expected u64-like index argument", span, Severity::Error)),
        }
    };

    let out = match method {
        "len" => Value::UInt128(s.chars().count() as u128),
        "is_empty" => Value::Bool(s.is_empty()),
        "char_at" => {
            let i = aidx(0)?;
            let ch = s.chars().nth(i);
            match ch {
                Some(c) => Value::Tuple(vec![Value::Char(c), Value::Null]),
                None => Value::Tuple(vec![
                    Value::Char('\0'),
                    Value::Err {
                        message: format!("char_at index out of bounds: index={}, len={}", i, s.chars().count()),
                        code: 1,
                        origin: "char_at".to_string(),
                        cause: None,
                    },
                ]),
            }
        }
        "contains" => Value::Bool(s.contains(astr(0)?)),
        "starts_with" => Value::Bool(s.starts_with(astr(0)?)),
        "ends_with" => Value::Bool(s.ends_with(astr(0)?)),
        "find" => Value::Int128(s.find(astr(0)?).map(|i| i as i128).unwrap_or(-1)),
        "rfind" => Value::Int128(s.rfind(astr(0)?).map(|i| i as i128).unwrap_or(-1)),
        "slice" => {
            let start = aidx(0)?;
            let end = aidx(1)?;
            let chars: Vec<char> = s.chars().collect();
            if start > end || end > chars.len() {
                return Err(Diagnostic::new(
                    format!("invalid slice bounds: start={}, end={}, len={}", start, end, chars.len()),
                    span,
                    Severity::Error,
                ));
            }
            Value::Str(chars[start..end].iter().collect())
        }
        "split" => Value::Array(
            s.split(astr(0)?)
                .map(|x| Value::Str(x.to_string()))
                .collect(),
        ),
        "replace" => Value::Str(s.replace(astr(0)?, astr(1)?)),
        "trim" => Value::Str(s.trim().to_string()),
        "trim_start" => Value::Str(s.trim_start().to_string()),
        "trim_end" => Value::Str(s.trim_end().to_string()),
        "to_upper" => Value::Str(s.to_uppercase()),
        "to_lower" => Value::Str(s.to_lowercase()),
        "reverse" => Value::Str(s.chars().rev().collect()),
        "to_i32" => match s.parse::<i32>() {
            Ok(v) => Value::Tuple(vec![Value::Int128(i128::from(v)), Value::Null]),
            Err(_) => Value::Tuple(vec![
                Value::Int128(0),
                Value::Err {
                    message: format!("cannot parse '{s}' as i32"),
                    code: 1,
                    origin: "to_i32".to_string(),
                    cause: None,
                },
            ]),
        },
        "to_f64" => match s.parse::<f64>() {
            Ok(v) => Value::Tuple(vec![Value::Float(v), Value::Null]),
            Err(_) => Value::Tuple(vec![
                Value::Float(0.0),
                Value::Err {
                    message: format!("cannot parse '{s}' as f64"),
                    code: 1,
                    origin: "to_f64".to_string(),
                    cause: None,
                },
            ]),
        },
        _ => return Err(Diagnostic::new(format!("unknown str method '{method}'"), span, Severity::Error)),
    };
    Ok(out)
}

fn dispatch_array_method(name: &str, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new("array method missing receiver", span, Severity::Error));
    };
    let Value::Array(items) = recv else {
        return Err(Diagnostic::new("array method receiver must be array", span, Severity::Error));
    };
    let method = name.strip_prefix("__meth_a_").unwrap_or(name);
    let aidx = |idx: usize| -> Result<usize, Diagnostic> {
        match tail.get(idx) {
            Some(Value::UInt128(v)) => usize::try_from(*v)
                .map_err(|_| Diagnostic::new("index too large", span, Severity::Error)),
            Some(Value::Int128(v)) if *v >= 0 => usize::try_from(*v)
                .map_err(|_| Diagnostic::new("index too large", span, Severity::Error)),
            _ => Err(Diagnostic::new("expected u64-like index argument", span, Severity::Error)),
        }
    };
    let out = match method {
        "len" => Value::UInt128(items.len() as u128),
        "is_empty" => Value::Bool(items.is_empty()),
        "get" => {
            let i = aidx(0)?;
            match items.get(i).cloned() {
                Some(v) => Value::Tuple(vec![v, Value::Null]),
                None => Value::Tuple(vec![
                    Value::Null,
                    Value::Err {
                        message: format!("get index out of bounds: index={}, len={}", i, items.len()),
                        code: 1,
                        origin: "get".to_string(),
                        cause: None,
                    },
                ]),
            }
        }
        "set" => {
            let i = aidx(0)?;
            let mut arr = items.clone();
            let Some(v) = tail.get(1) else {
                return Err(Diagnostic::new("set expects value", span, Severity::Error));
            };
            if i >= arr.len() {
                return Err(Diagnostic::new(
                    format!("set index out of bounds: index={}, len={}", i, arr.len()),
                    span,
                    Severity::Error,
                ));
            }
            arr[i] = v.clone();
            Value::Unit
        }
        "push" => {
            let Some(v) = tail.first() else {
                return Err(Diagnostic::new("push expects value", span, Severity::Error));
            };
            let mut arr = items.clone();
            arr.push(v.clone());
            Value::Unit
        }
        "pop" => {
            let mut arr = items.clone();
            arr.pop().unwrap_or(Value::Null)
        }
        "insert" => {
            let i = aidx(0)?;
            let Some(v) = tail.get(1) else {
                return Err(Diagnostic::new("insert expects value", span, Severity::Error));
            };
            let mut arr = items.clone();
            if i > arr.len() {
                return Err(Diagnostic::new(
                    format!("insert index out of bounds: index={}, len={}", i, arr.len()),
                    span,
                    Severity::Error,
                ));
            }
            arr.insert(i, v.clone());
            Value::Unit
        }
        "remove" => {
            let i = aidx(0)?;
            let mut arr = items.clone();
            if i >= arr.len() {
                Value::Tuple(vec![
                    Value::Null,
                    Value::Err {
                        message: format!("remove index out of bounds: index={}, len={}", i, arr.len()),
                        code: 1,
                        origin: "remove".to_string(),
                        cause: None,
                    },
                ])
            } else {
                Value::Tuple(vec![arr.remove(i), Value::Null])
            }
        }
        "clear" => Value::Unit,
        "find" => {
            let Some(v) = tail.first() else {
                return Err(Diagnostic::new("find expects value", span, Severity::Error));
            };
            let idx = items.iter().position(|x| x == v).map(|i| i as i128).unwrap_or(-1);
            Value::Int128(idx)
        }
        "contains" => {
            let Some(v) = tail.first() else {
                return Err(Diagnostic::new("contains expects value", span, Severity::Error));
            };
            Value::Bool(items.iter().any(|x| x == v))
        }
        "reverse" => {
            let mut arr = items.clone();
            arr.reverse();
            Value::Array(arr)
        }
        "sort" => {
            let mut arr = items.clone();
            arr.sort_by_key(|v| v.display_for_print());
            Value::Array(arr)
        }
        "slice" => {
            let start = aidx(0)?;
            let end = aidx(1)?;
            if start > end || end > items.len() {
                return Err(Diagnostic::new(
                    format!("invalid slice bounds: start={}, end={}, len={}", start, end, items.len()),
                    span,
                    Severity::Error,
                ));
            }
            Value::Array(items[start..end].to_vec())
        }
        _ => return Err(Diagnostic::new(format!("unknown array method '{method}'"), span, Severity::Error)),
    };
    Ok(out)
}

fn dispatch_function_method(
    _vm: &mut Vm<'_>,
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, Diagnostic> {
    let [recv, tail @ ..] = args else {
        return Err(Diagnostic::new("function method missing receiver", span, Severity::Error));
    };
    let method = name.strip_prefix("__meth_fn_").unwrap_or(name);
    match method {
        "call" => Err(Diagnostic::new(
            "function.call is not supported yet; use direct call syntax f(...)",
            span,
            Severity::Error,
        )),
        "arity" => match recv {
            Value::Function { function, .. } => Ok(Value::UInt128(function.params.len() as u128)),
            Value::Builtin(_) => Ok(Value::UInt128(0)),
            _ => Err(Diagnostic::new("arity receiver must be function", span, Severity::Error)),
        },
        "bind" => Ok(recv.clone()),
        "compose" => {
            let Some(Value::Function { .. } | Value::Builtin(_)) = tail.first() else {
                return Err(Diagnostic::new("compose expects function argument", span, Severity::Error));
            };
            Ok(recv.clone())
        }
        "partial" => Ok(recv.clone()),
        _ => Err(Diagnostic::new(
            format!("unknown function method '{method}'"),
            span,
            Severity::Error,
        )),
    }
}

fn err_value_from_panic(diagnostic: &Diagnostic) -> Value {
    let msg = diagnostic.message.strip_prefix("panic: ").unwrap_or(&diagnostic.message);
    if let Some((message, code_part)) = msg.rsplit_once(" (code=") {
        if let Some(raw_code) = code_part.strip_suffix(')') {
            if let Ok(code) = raw_code.parse::<i32>() {
                return Value::Err {
                    message: message.to_string(),
                    code,
                    origin: "panic".to_string(),
                    cause: None,
                };
            }
        }
    }
    Value::Err {
        message: msg.to_string(),
        code: 1,
        origin: "panic".to_string(),
        cause: None,
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
        Value::Array(_) => "array",
        Value::Err { .. } => "err",
        Value::StructInstance { .. } => "struct",
    }
}

fn canonical_type_name(v: &Value) -> String {
    match v {
        Value::Int128(_) => "i128".to_string(),
        Value::UInt128(_) => "u128".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Str(_) => "str".to_string(),
        Value::Float(_) => "f64".to_string(),
        Value::Char(_) => "char".to_string(),
        Value::Unit => "unit".to_string(),
        Value::Null => "null".to_string(),
        Value::Builtin(_) | Value::Function { .. } => "fn(any) -> any".to_string(),
        Value::Tuple(items) => {
            let inner = items.iter().map(canonical_type_name).collect::<Vec<_>>().join(", ");
            format!("({inner})")
        }
        Value::Array(items) => {
            if let Some(first) = items.first() {
                format!("[{}]", canonical_type_name(first))
            } else {
                "[unknown]".to_string()
            }
        }
        Value::Err { .. } => "err".to_string(),
        Value::StructInstance { type_name, .. } => type_name.clone(),
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
        Value::Array(items) => !items.is_empty(),
        Value::Err { .. } => true,
        Value::StructInstance { .. } => true,
    }
}

fn to_usize_index(value: &Value, span: Span) -> Result<usize, Diagnostic> {
    match value {
        Value::Int128(i) => usize::try_from(*i).map_err(|_| {
            Diagnostic::new(
                format!("array index must be non-negative usize-compatible int, got {}", i),
                span,
                Severity::Error,
            )
        }),
        Value::UInt128(u) => usize::try_from(*u).map_err(|_| {
            Diagnostic::new(
                format!("array index too large for usize: {}", u),
                span,
                Severity::Error,
            )
        }),
        other => Err(Diagnostic::new(
            format!("array index must be integer, got {}", builtin_type_name(other)),
            span,
            Severity::Error,
        )),
    }
}

fn wrap_err_value(
    base_err: Value,
    message: String,
    code_override: Option<i32>,
    origin: String,
    span: Span,
) -> Result<Value, Diagnostic> {
    match base_err {
        Value::Err {
            message: base_message,
            code: base_code,
            origin: base_origin,
            cause: base_cause,
        } => {
            let is_propagation_wrap = message == "propagated by ?";
            let final_message = if is_propagation_wrap {
                base_message.clone()
            } else {
                message
            };
            let final_origin = if is_propagation_wrap {
                "propagate".to_string()
            } else {
                origin
            };
            Ok(Value::Err {
                message: final_message,
                code: code_override.unwrap_or(base_code),
                origin: final_origin,
                cause: Some(Box::new(Value::Err {
                    message: base_message,
                    code: base_code,
                    origin: base_origin,
                    cause: base_cause,
                })),
            })
        }
        Value::StructInstance { type_name, fields } => {
            let base_message = match fields.get("message") {
                Some(Value::Str(s)) => s.clone(),
                _ => {
                    return Err(Diagnostic::new(
                        format!("error-like struct `{type_name}` must have `message: str`"),
                        span,
                        Severity::Error,
                    ))
                }
            };
            let base_code = match fields.get("code") {
                Some(Value::Int128(n)) => i32::try_from(*n).map_err(|_| {
                    Diagnostic::new(
                        format!("error-like struct `{type_name}` has code outside i32 range"),
                        span,
                        Severity::Error,
                    )
                })?,
                Some(Value::UInt128(n)) => i32::try_from(*n).map_err(|_| {
                    Diagnostic::new(
                        format!("error-like struct `{type_name}` has code outside i32 range"),
                        span,
                        Severity::Error,
                    )
                })?,
                _ => {
                    return Err(Diagnostic::new(
                        format!("error-like struct `{type_name}` must have `code: i32`"),
                        span,
                        Severity::Error,
                    ))
                }
            };
            let is_propagation_wrap = message == "propagated by ?";
            let final_message = if is_propagation_wrap {
                base_message
            } else {
                message
            };
            let final_origin = if is_propagation_wrap {
                "propagate".to_string()
            } else {
                origin
            };
            Ok(Value::Err {
                message: final_message,
                code: code_override.unwrap_or(base_code),
                origin: final_origin,
                cause: Some(Box::new(Value::StructInstance { type_name, fields })),
            })
        }
        other => Err(Diagnostic::new(
            format!(
                "wrap expects err-like as first argument, got {}",
                builtin_type_name(&other)
            ),
            span,
            Severity::Error,
        )),
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

    #[test]
    fn try_context_converts_panic_into_err_value() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::TryStart(6), s);
        b.emit(Op::Load(SymbolId(0)), s);
        b.emit(Op::ConstStr("fatal".to_string()), s);
        b.emit(Op::CallValue(1), s);
        b.emit(Op::TryEnd, s);
        b.emit(Op::Jump(10), s);
        b.emit(Op::ConstNull, s);
        b.emit(Op::Swap, s);
        b.emit(Op::MakeTuple(2), s);
        b.emit(Op::Pop, s);
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
        execute(&b.finish(), &symbols).expect("panic should be converted inside try");
    }

    #[test]
    fn builtin_wrap_creates_error_cause_chain() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::Load(SymbolId(0)), s); // error
        b.emit(Op::ConstStr("base".to_string()), s);
        b.emit(Op::CallValue(1), s);
        b.emit(Op::Load(SymbolId(1)), s); // wrap
        b.emit(Op::Swap, s);
        b.emit(Op::ConstStr("ctx".to_string()), s);
        b.emit(Op::CallValue(2), s);
        b.emit(Op::Dup, s);
        b.emit(Op::StructGet("origin".to_string()), s);
        b.emit(Op::Pop, s);
        b.emit(Op::StructGet("cause".to_string()), s);
        b.emit(Op::StructGet("message".to_string()), s);
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
        symbols.define(
            root,
            "wrap".to_string(),
            crate::analyzer::Type::Function {
                params: vec![crate::analyzer::Type::Any],
                ret: Box::new(crate::analyzer::Type::Err),
            },
            crate::hir::SymbolOrigin::Builtin,
            true,
        );
        execute(&b.finish(), &symbols).expect("wrap should produce chained err");
    }

    #[test]
    fn executes_array_get_and_set() {
        let mut symbols = SymbolTable::new();
        let root = symbols.create_scope(None);
        let arr = symbols.define(
            root,
            "arr".to_string(),
            crate::analyzer::Type::Array(Box::new(crate::analyzer::Type::Int { signed: true, bits: 32 })),
            crate::hir::SymbolOrigin::User,
            false,
        );
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::ConstI128(2), s);
        b.emit(Op::MakeArray(2), s);
        b.emit(Op::Bind(arr), s);
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::ConstI128(9), s);
        b.emit(Op::ArrayAssign(arr), s);
        b.emit(Op::Load(arr), s);
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::ArrayGet, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        execute(&b.finish(), &symbols).expect("array get/set should execute");
    }

    #[test]
    fn array_get_out_of_bounds_is_error() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::MakeArray(1), s);
        b.emit(Op::ConstI128(3), s);
        b.emit(Op::ArrayGet, s);
        b.emit(Op::Return, s);
        let symbols = SymbolTable::new();
        let err = execute(&b.finish(), &symbols).expect_err("array get oob");
        assert!(err.iter().any(|d| d.message.contains("index out of bounds")));
    }

    #[test]
    fn array_extend_opcode_merges_arrays() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(1), s);
        b.emit(Op::MakeArray(1), s);
        b.emit(Op::ConstI128(2), s);
        b.emit(Op::ConstI128(3), s);
        b.emit(Op::MakeArray(2), s);
        b.emit(Op::ArrayExtend, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let symbols = SymbolTable::new();
        execute(&b.finish(), &symbols).expect("array extend should execute");
    }

    #[test]
    fn builtin_typeof_returns_type_name() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::Load(SymbolId(0)), s);
        b.emit(Op::ConstI128(7), s);
        b.emit(Op::CallValue(1), s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let mut symbols = SymbolTable::new();
        let root = symbols.create_scope(None);
        symbols.define(
            root,
            "typeof".to_string(),
            crate::analyzer::Type::Function {
                params: vec![crate::analyzer::Type::Any],
                ret: Box::new(crate::analyzer::Type::Str),
            },
            crate::hir::SymbolOrigin::Builtin,
            true,
        );
        execute(&b.finish(), &symbols).expect("typeof should execute");
    }

    #[test]
    fn make_range_opcode_builds_array() {
        let mut b = ChunkBuilder::new();
        let s = span();
        b.emit(Op::ConstI128(0), s);
        b.emit(Op::ConstI128(3), s);
        b.emit(Op::MakeRange(false), s);
        b.emit(Op::ArrayLen, s);
        b.emit(Op::Pop, s);
        b.emit(Op::Return, s);
        let symbols = SymbolTable::new();
        execute(&b.finish(), &symbols).expect("range should execute");
    }
}
