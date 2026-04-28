//! Runtime values for the stack machine (no complex types yet).

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Str(String),
    Float(f64),
    Char(char),
    Unit,
}

impl Value {
    pub fn display_for_print(&self) -> String {
        match self {
            Value::Int(i) => i.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => s.clone(),
            Value::Float(f) => f.to_string(),
            Value::Char(c) => c.to_string(),
            Value::Unit => String::new(),
        }
    }
}
