#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    Unknown,
    Any,
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
    },
}
