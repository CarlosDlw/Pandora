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
//! | [`Op::EnterScope`] / [`Op::ExitScope`] | open/close lexical runtime frame for block locals |
//! | [`Op::Neg`]      | pop one numeric value → push unary negation (`-`) |
//! | [`Op::Not`] [`Op::BitNot`] | unary logical / bitwise negation |
//! | [`Op::Add`] [`Op::Sub`] [`Op::Mul`] [`Op::Div`] [`Op::Mod`] [`Op::Pow`] | pop b, pop a → push result |
//! | [`Op::Eq`] [`Op::Ne`] [`Op::Lt`] [`Op::Le`] [`Op::Gt`] [`Op::Ge`] | comparisons (result bool) |
//! | [`Op::LogicalAnd`] [`Op::LogicalOr`] | boolean binary operators |
//! | [`Op::BitAnd`] [`Op::BitOr`] [`Op::BitXor`] [`Op::Shl`] [`Op::Shr`] | integer bitwise/shift ops |
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
    EnterScope,
    ExitScope,

    Neg,
    Not,
    BitNot,

    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    LogicalAnd,
    LogicalOr,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,

    Call(SymbolId, u8),

    Pop,
    Return,
}
