//! Runtime values for the stack machine.

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
        }
    }
}
