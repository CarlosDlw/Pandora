use rustc_hash::FxHashMap as HashMap;
use std::sync::Arc;

use crate::hir::symbols::SymbolId;
use crate::vm::chunk::FunctionChunk;
use crate::vm::int::{IntPayload, IntTag, TypedInt};

/// Runtime values for the stack machine.

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Transitional legacy variant while engine migration is in progress.
    Int128(i128),
    /// Transitional legacy variant while engine migration is in progress.
    UInt128(u128),
    /// Integer value with exact runtime tag (signed + bits).
    Int(TypedInt),
    Bool(bool),
    Str(String),
    Float(f64),
    Char(char),
    Unit,
    Null,
    Builtin(SymbolId),
    Function {
        function: Arc<FunctionChunk>,
        captured: Arc<HashMap<SymbolId, Value>>,
        self_symbol: Option<SymbolId>,
    },
    Tuple(Vec<Value>),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Set(Vec<Value>),
    Err {
        message: String,
        code: i32,
        origin: String,
        cause: Option<Box<Value>>,
    },
    StructInstance {
        type_name: String,
        fields: Vec<Value>,
    },
}

impl Value {
    pub fn int_i128(value: i128) -> Self {
        Value::Int(TypedInt::try_from_signed(IntTag::I128, value).expect("i128 in range"))
    }

    pub fn int_u128(value: u128) -> Self {
        Value::Int(TypedInt::try_from_unsigned(IntTag::U128, value).expect("u128 in range"))
    }

    pub fn as_i128(&self) -> Option<i128> {
        match self {
            Value::Int128(i) => Some(*i),
            Value::UInt128(u) => i128::try_from(*u).ok(),
            Value::Int(v) => match v.payload() {
                IntPayload::Signed(i) => Some(i),
                IntPayload::Unsigned(u) => i128::try_from(u).ok(),
            },
            _ => None,
        }
    }

    pub fn as_u128(&self) -> Option<u128> {
        match self {
            Value::UInt128(u) => Some(*u),
            Value::Int128(i) if *i >= 0 => Some(*i as u128),
            Value::Int(v) => match v.payload() {
                IntPayload::Unsigned(u) => Some(u),
                IntPayload::Signed(i) if i >= 0 => Some(i as u128),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn is_signed_int(&self) -> bool {
        match self {
            Value::Int128(_) => true,
            Value::Int(v) => matches!(v.payload(), IntPayload::Signed(_)),
            _ => false,
        }
    }

    pub fn is_unsigned_int(&self) -> bool {
        match self {
            Value::UInt128(_) => true,
            Value::Int(v) => matches!(v.payload(), IntPayload::Unsigned(_)),
            _ => false,
        }
    }

    pub fn display_for_print(&self) -> String {
        match self {
            Value::Int128(i) => i.to_string(),
            Value::UInt128(u) => u.to_string(),
            Value::Int(v) => v.display_value(),
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => s.clone(),
            Value::Float(f) => f.to_string(),
            Value::Char(c) => c.to_string(),
            Value::Unit => String::new(),
            Value::Null => "null".to_string(),
            Value::Builtin(_) => "<builtin fn>".to_string(),
            Value::Function { .. } => "<fn>".to_string(),
            Value::Tuple(items) => {
                let rendered = items
                    .iter()
                    .map(Value::display_for_print)
                    .collect::<Vec<_>>();
                format!("({})", rendered.join(", "))
            }
            Value::Array(items) => {
                let rendered = items
                    .iter()
                    .map(Value::display_for_print)
                    .collect::<Vec<_>>();
                format!("[{}]", rendered.join(", "))
            }
            Value::Map(entries) => {
                let rendered = entries
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.display_for_print(), v.display_for_print()))
                    .collect::<Vec<_>>();
                format!("{{{}}}", rendered.join(", "))
            }
            Value::Set(items) => {
                let rendered = items
                    .iter()
                    .map(Value::display_for_print)
                    .collect::<Vec<_>>();
                format!("set{{{}}}", rendered.join(", "))
            }
            Value::Err {
                message,
                code,
                origin,
                cause,
            } => {
                let mut rendered = format!(
                    "err(message=\"{}\", code={}, origin=\"{}\")",
                    message, code, origin
                );
                if let Some(cause) = cause {
                    rendered.push_str(&format!(" <- {}", cause.display_for_print()));
                }
                rendered
            }
            Value::StructInstance { type_name, .. } => format!("<{}>", type_name),
        }
    }
}
