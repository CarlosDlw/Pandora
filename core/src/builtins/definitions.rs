use crate::analyzer::Type;

use super::registry::{Builtin, BuiltinRegistry};

pub fn default_registry() -> BuiltinRegistry {
    BuiltinRegistry {
        items: vec![Builtin {
            name: "print",
            ty: Type::Function {
                params: vec![Type::Any],
                ret: Box::new(Type::Unit),
            },
        },
        Builtin {
            name: "len",
            ty: Type::Function {
                params: vec![Type::Str],
                ret: Box::new(Type::Int),
            },
        }],
    }
}
