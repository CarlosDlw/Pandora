use std::collections::HashMap;

use crate::hir::symbols::SymbolId;
use crate::vm::chunk::FunctionChunk;

/// Runtime values for the stack machine.

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Signed integers at full static-analysis width (see [`crate::analyzer::Type::Int`] signed).
    Int128(i128),
    /// Unsigned integers at full static-analysis width.
    UInt128(u128),
    Bool(bool),
    Str(String),
    Float(f64),
    Char(char),
    Unit,
    Null,
    Builtin(SymbolId),
    Function {
        function: Box<FunctionChunk>,
        captured: HashMap<SymbolId, Value>,
        self_symbol: Option<SymbolId>,
    },
    Tuple(Vec<Value>),
    Err {
        message: String,
        code: i32,
    },
    StructInstance {
        type_name: String,
        fields: HashMap<String, Value>,
    },
}

impl Value {
    pub fn display_for_print(&self) -> String {
        match self {
            Value::Int128(i) => i.to_string(),
            Value::UInt128(u) => u.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => s.clone(),
            Value::Float(f) => f.to_string(),
            Value::Char(c) => c.to_string(),
            Value::Unit => String::new(),
            Value::Null => "null".to_string(),
            Value::Builtin(_) => "<builtin fn>".to_string(),
            Value::Function { .. } => "<fn>".to_string(),
            Value::Tuple(items) => {
                let rendered = items.iter().map(Value::display_for_print).collect::<Vec<_>>();
                format!("({})", rendered.join(", "))
            }
            Value::Err { message, code } => format!("err(message=\"{}\", code={})", message, code),
            Value::StructInstance { type_name, .. } => format!("<{}>", type_name),
        }
    }
}
