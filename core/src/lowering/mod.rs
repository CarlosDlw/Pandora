#![allow(clippy::module_inception)]

mod lowering;

pub use lowering::{lower, lower_with_registry};
