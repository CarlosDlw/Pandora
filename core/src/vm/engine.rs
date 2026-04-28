//! Linear VM execution: one [`crate::hir::symbols::SymbolTable`] lookup for builtins (`SymbolOrigin::Builtin`),
//! user-defined callees are rejected until closures/functions exist.

use std::collections::HashMap;

use foundation::{
    diagnostics::{Diagnostic, Diagnostics, Severity},
    span::Span,
};

use crate::hir::{
    symbols::{SymbolId, SymbolOrigin, SymbolTable},
};

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
    symbols: &'a SymbolTable,
}

impl<'a> Vm<'a> {
    pub fn new(chunk: &'a Chunk, symbols: &'a SymbolTable, initial_env: HashMap<SymbolId, Value>) -> Self {
        Self {
            chunk,
            ip: 0,
            stack: Vec::new(),
            env: initial_env,
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
            Op::ConstInt(n) => self.stack.push(Value::Int(*n)),
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

            Op::Store(sym) => {
                let v = self.pop_one(span)?;
                self.env.insert(*sym, v);
            }

            Op::Add => self.apply_bin_checked(span, |a, b| a.checked_add(b), |a, b| Some(a + b))?,
            Op::Sub => self.apply_bin_checked(span, |a, b| a.checked_sub(b), |a, b| Some(a - b))?,
            Op::Mul => self.apply_bin_checked(span, |a, b| a.checked_mul(b), |a, b| Some(a * b))?,
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

            Op::Return => {}
        }
        Ok(())
    }

    fn apply_bin_checked(
        &mut self,
        span: Span,
        int_op: fn(i64, i64) -> Option<i64>,
        float_op: fn(f64, f64) -> Option<f64>,
    ) -> Result<(), Diagnostic> {
        let rhs = self.pop_one(span)?;
        let lhs = self.pop_one(span)?;
        let out = match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => {
                let r = int_op(a, b).ok_or_else(|| {
                    Diagnostic::new("integer overflow or invalid operation", span, Severity::Error)
                })?;
                Value::Int(r)
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
            (Value::Int(a), Value::Int(b)) => {
                if b == 0 {
                    return Err(Diagnostic::new("division by zero", span, Severity::Error));
                }
                Value::Int(a / b)
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
                Value::Str(s) => Ok(Value::Int(s.chars().count() as i64)),
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
        Value::Int(_) => "int",
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
        b.emit(Op::ConstInt(1), s);
        b.emit(Op::ConstInt(2), s);
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
        b.emit(Op::ConstInt(1), s);
        b.emit(Op::ConstInt(0), s);
        b.emit(Op::Div, s);
        b.emit(Op::Return, s);
        let chunk = b.finish();
        let symbols = SymbolTable::new();
        let err = execute(&chunk, &symbols).expect_err("div0");
        assert!(err.iter().any(|d| d.message.contains("division by zero")));
    }
}
