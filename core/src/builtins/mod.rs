pub mod definitions;
pub mod registry;

pub use definitions::default_registry;
pub use registry::{
    BuiltinFunction, BuiltinMethod, BuiltinMethodKind, BuiltinRegistry, ReceiverMatcher, TypeTag,
    match_receiver,
};
