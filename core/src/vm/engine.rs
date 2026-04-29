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

pub struct Vm<'a> {
    chunk: &'a Chunk,
    ip: usize,
    stack: Vec<Value>,
    env: HashMap<SymbolId, Value>,
    scope_frames: Vec<Vec<SymbolId>>,
    symbols: &'a SymbolTable,
}

impl<'a> Vm<'a> {
    pub fn new(chunk: &'a Chunk, symbols: &'a SymbolTable, initial_env: HashMap<SymbolId, Value>) -> Self {
        Self {
            chunk,
            ip: 0,
            stack: Vec::new(),
            env: initial_env,
            scope_frames: vec![Vec::new()],
            symbols,
        }
    }

    /// Executes until [`Op::Return`] or end of chunk. On failure returns accumulated diagnostics (no panics).
    pub fn run(&mut self) -> Result<(), Diagnostics> {
        let mut diagnostics = Diagnostics::new();

        while self.ip < self.chunk.code.len() {
            let op = self.chunk.code[self.ip].clone();
            let span = self.chunk.spans[self.ip];
            self.ip += 1;

            if let Err(d) = self.step_op(&op, span) {
                diagnostics.push(d);
                return Err(diagnostics);
            }

            if matches!(op, Op::Return) {
                break;
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

            Op::Add => self.apply_bin_checked(
                span,
                |a, b| a.checked_add(b),
                |a, b| a.checked_add(b),
                |a, b| Some(a + b),
            )?,
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

            Op::Pop => {
                let _ = self.pop_one(span)?;
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

            Op::Return => {}
        }
        Ok(())
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
}
