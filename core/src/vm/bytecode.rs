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
//! | [`Op::JumpIfFalse`] / [`Op::Jump`] | control-flow jumps |
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
use crate::vm::int::TypedInt;

/// Stack-machine instruction. Execution order equals vector order in [`crate::vm::chunk::Chunk::code`].
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    ConstI128(i128),
    ConstU128(u128),
    ConstInt(TypedInt),
    ConstBool(bool),
    ConstStr(String),
    ConstFloat(f64),
    ConstChar(char),
    ConstUnit,
    ConstNull,

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
    AddInt,
    AddFloat,
    StrConcat,
    Sub,
    SubInt,
    Mul,
    MulInt,
    Div,
    DivInt,
    Mod,
    ModInt,
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
    JumpIfFalse(usize),
    Jump(usize),
    TryStart(usize),
    TryEnd,

    Call(SymbolId, u8),
    CallDirect(SymbolId, u8),
    CallValue(u8),
    MakeClosure(SymbolId),
    MakeTuple(u8),
    TupleGet(usize),
    MakeArray(u8),
    MakeMap(u8),
    MakeSet(u8),
    ArrayExtend,
    MakeRange(bool),
    ArrayLen,
    ArrayGet,
    ArrayAssign(SymbolId),
    MakeStruct(String, u8),
    StructLoadSlot(SymbolId, usize),
    StructGetSlot(usize),
    StructSetSlot(usize),
    StructAssignSlot(SymbolId, usize),
    StructGet(String),
    StructSet(String),
    StructAssign(SymbolId, String),
    WrapErr,

    Swap,
    Dup,
    Pop,
    Return,
}
