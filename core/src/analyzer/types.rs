#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int { signed: bool, bits: u16 },
    Float { bits: u16 },
    Bool,
    Str,
    Char,
    Unit,
    Unknown,
    Any,
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
    },
}
