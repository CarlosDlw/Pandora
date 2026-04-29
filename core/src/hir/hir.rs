use std::collections::HashMap;

use foundation::{arena::Arena, ids::ArenaId, ids::FileId, span::Span};

use super::symbols::SymbolId;

pub type HirId = ArenaId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    LogicalAnd,
    LogicalOr,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirExpr {
    Int(String),
    Float(String),
    Bool(bool),
    Str(String),
    Char(char),
    Var(SymbolId),
    Unary { op: UnaryOp, operand: HirId },
    Binary { op: BinOp, lhs: HirId, rhs: HirId },
    Call { callee: SymbolId, args: Vec<HirId> },
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirStmt {
    Let {
        symbol: SymbolId,
        value: HirId,
        is_const: bool,
        span: Span,
    },
    Assign {
        symbol: SymbolId,
        value: HirId,
        span: Span,
    },
    Expr {
        expr: HirId,
        span: Span,
    },
    Block {
        stmts: Vec<HirStmt>,
        span: Span,
    },
    Invalid {
        span: Span,
    },
}

#[derive(Debug)]
pub struct Hir {
    pub file_id: FileId,
    pub exprs: Arena<HirExpr>,
    pub stmts: Vec<HirStmt>,
    /// Source span for each expression id (lowering fills this; used by bytecode / diagnostics).
    pub expr_spans: HashMap<HirId, Span>,
}
