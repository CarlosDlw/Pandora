//! Bytecode IR: linear stack-machine ops tied to [`foundation::span::Span`] for source mapping,
//! compilation from [`crate::hir::Hir`], and a small linear interpreter.

mod bytecode;
mod chunk;
mod emit;
mod engine;
mod int;
mod value;

pub use bytecode::Op;
pub use chunk::{Chunk, ChunkBuilder};
pub use emit::compile_program;
pub use engine::execute;
pub use int::{IntPayload, IntTag, TypedInt};
pub use value::Value;
