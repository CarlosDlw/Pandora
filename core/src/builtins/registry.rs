use crate::hir::Type;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Builtin {
    pub name: &'static str,
    pub ty: Type,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuiltinRegistry {
    pub items: Vec<Builtin>,
}
