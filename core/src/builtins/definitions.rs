use crate::hir::Type;

use super::registry::{Builtin, BuiltinRegistry};

pub fn default_registry() -> BuiltinRegistry {
    BuiltinRegistry {
        items: vec![Builtin {
            name: "print",
            ty: Type::Function {
                params: vec![Type::Unknown],
                ret: Box::new(Type::Unit),
            },
        }],
    }
}
