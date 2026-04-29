pub mod definitions;
pub mod registry;

pub use definitions::default_registry;
pub use registry::{
    match_receiver, BuiltinFunction, BuiltinMethod, BuiltinMethodKind, BuiltinRegistry,
    ReceiverMatcher, TypeTag,
};
