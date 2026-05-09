#![allow(clippy::module_inception)]

mod parser;
mod pratt;
mod sync;

pub use parser::{Parser, parse};
