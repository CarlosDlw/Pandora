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
                ret: Box::new(Type::Int {
                    signed: false,
                    bits: 64,
                }),
            },
        },
        Builtin {
            name: "error",
            ty: Type::Function {
                // Contract enforced in checker/runtime: (str) or (str, i32)
                params: vec![Type::Any],
                ret: Box::new(Type::Err),
            },
        },
        Builtin {
            name: "panic",
            ty: Type::Function {
                // Contract enforced in checker/runtime: (str) or (str, i32), runtime aborts.
                params: vec![Type::Any],
                ret: Box::new(Type::Unit),
            },
        }],
    }
}
