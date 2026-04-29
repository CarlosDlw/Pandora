use crate::analyzer::Type;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinFunction {
    pub name: &'static str,
    pub ty: Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReceiverMatcher {
    IntSignedAny,
    IntUnsignedAny,
    FloatAny,
    Bool,
    Char,
    Str,
    ArrayAny,
    FunctionAny,
    Exact(TypeTag),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeTag {
    Err,
    Unit,
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinMethodKind {
    Instance,
    Static,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinMethod {
    pub receiver: ReceiverMatcher,
    pub name: &'static str,
    /// Canonical function symbol name used by lowering/emitter/VM.
    pub symbol_name: &'static str,
    pub kind: BuiltinMethodKind,
    pub params: Vec<Type>,
    pub ret: Type,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuiltinRegistry {
    pub functions: Vec<BuiltinFunction>,
    pub methods: Vec<BuiltinMethod>,
}

impl BuiltinRegistry {
    pub fn function_by_name(&self, name: &str) -> Option<&BuiltinFunction> {
        self.functions.iter().find(|b| b.name == name)
    }

    pub fn method_for_type(&self, receiver_ty: &Type, method: &str) -> Option<&BuiltinMethod> {
        self.methods.iter().find(|m| {
            m.name == method
                && match_receiver(receiver_ty, m.receiver)
        })
    }
}

pub fn match_receiver(ty: &Type, matcher: ReceiverMatcher) -> bool {
    match matcher {
        ReceiverMatcher::IntSignedAny => matches!(ty, Type::Int { signed: true, .. }),
        ReceiverMatcher::IntUnsignedAny => matches!(ty, Type::Int { signed: false, .. }),
        ReceiverMatcher::FloatAny => matches!(ty, Type::Float { .. }),
        ReceiverMatcher::Bool => matches!(ty, Type::Bool),
        ReceiverMatcher::Char => matches!(ty, Type::Char),
        ReceiverMatcher::Str => matches!(ty, Type::Str),
        ReceiverMatcher::ArrayAny => matches!(ty, Type::Array(_)),
        ReceiverMatcher::FunctionAny => matches!(ty, Type::Function { .. }),
        ReceiverMatcher::Exact(TypeTag::Err) => matches!(ty, Type::Err),
        ReceiverMatcher::Exact(TypeTag::Unit) => matches!(ty, Type::Unit),
        ReceiverMatcher::Exact(TypeTag::Null) => matches!(ty, Type::Null),
    }
}
