//! Raw bytecode ops (stack machine). Ordered linearly in a [`crate::vm::chunk::Chunk`].
//!
//! # Stack semantics (explicit)
//!
//! | Op               | Effect |
//! |------------------|--------|
//! | [`Op::ConstI128`] / [`Op::ConstU128`] / floats / bool / str / char | push literal |
//! | [`Op::Load`]     | push value bound to [`crate::hir::symbols::SymbolId`] |
//! | [`Op::Bind`]     | pop one value → initialize binding (let) |
//! | [`Op::Assign`]   | pop one value → update an existing binding (`id = expr`) |
//! | [`Op::Neg`]      | pop one numeric value → push unary negation (`-`) |
//! | [`Op::Add`] [`Op::Sub`] [`Op::Mul`] [`Op::Div`] | pop b, pop a → push result (see docs on [`Op::Div`] for integers) |
//! | [`Op::Call`]     | pop `n` arguments, push return value |
//! | [`Op::Pop`]      | discard one stack top |
//! | [`Op::Return`]   | stop executing this chunk (`ip` advances, then VM exits; stack must be empty afterwards) |
//!
//! **Integer division [`Op::Div`]**: truncates toward zero (same as Rust integer `/`), with `i128::MIN / -1`
//! reported as overflow instead of wrapping.
//!
//! **String length** (builtin `len`): counts Unicode scalar values (Rust `str::chars`), not bytes or extended
//! grapheme clusters — see [`crate::vm::engine`] `dispatch_builtin`.

use crate::hir::symbols::SymbolId;

/// Stack-machine instruction. Execution order equals vector order in [`crate::vm::chunk::Chunk::code`].
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    ConstI128(i128),
    ConstU128(u128),
    ConstBool(bool),
    ConstStr(String),
    ConstFloat(f64),
    ConstChar(char),

    Load(SymbolId),
    /// First store for `let` / `:=` / `: name = ...`.
    Bind(SymbolId),
    /// Reassignment: rejects builtin names and `::` declarations.
    Assign(SymbolId),

    Neg,

    Add,
    Sub,
    Mul,
    Div,

    Call(SymbolId, u8),

    Pop,
    Return,
}
