//! Raw bytecode ops (stack machine). Ordered linearly in a [`crate::vm::chunk::Chunk`].
//!
//! # Stack semantics (explicit)
//!
//! | Op              | Effect |
//! |-----------------|--------|
//! | [`Op::ConstInt`] [`Op::ConstFloat`] [`Op::ConstBool`] [`Op::ConstStr`] [`Op::ConstChar`] | push literal |
//! | [`Op::Load`]     | push value bound to [`crate::hir::symbols::SymbolId`] |
//! | [`Op::Store`]    | pop one value → store under symbol |
//! | [`Op::Add`]      | pop b, pop a → push \(a \, \text{op}\, b\) |
//! | [`Op::Sub`]      | pop b, pop a → push \(a - b\) |
//! | [`Op::Mul`]      | pop b, pop a → push \(a \times b\) |
//! | [`Op::Div`]      | pop b, pop a → push \(a / b\) |
//! | [`Op::Call`]     | pop `n` arguments (arity), push return value slots (here: one) |
//! | [`Op::Pop`]      | discard one stack top |
//! | [`Op::Return`]   | stop executing this chunk (`ip` advances, then VM exits; stack must be empty afterwards) |
//!
//! **`Call(SymbolId, n)`**: pops `n` values (first arg topmost after callee convention TBD later;
//! emitter should document order; typical convention: last arg popped first.)
//!
//! Bytecode intentionally does **not** reference AST/HIR — only [`crate::hir::symbols::SymbolId`] and spans.

use crate::hir::symbols::SymbolId;

/// Stack-machine instruction. Execution order equals vector order in [`crate::vm::chunk::Chunk::code`].
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    ConstInt(i64),
    ConstBool(bool),
    ConstStr(String),
    ConstFloat(f64),
    ConstChar(char),

    Load(SymbolId),
    Store(SymbolId),

    Add,
    Sub,
    Mul,
    Div,

    Call(SymbolId, u8),

    Pop,
    Return,
}
