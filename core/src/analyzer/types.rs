use crate::hir::SymbolId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int { signed: bool, bits: u16 },
    Float { bits: u16 },
    Bool,
    Str,
    Char,
    Unit,
    Null,
    Err,
    Unknown,
    Any,
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
    },
    Tuple(Vec<Type>),
    Array(Box<Type>),
    Map(Box<Type>, Box<Type>),
    Set(Box<Type>),
    Struct(SymbolId),
    Trait(SymbolId),
    SelfType,
}
