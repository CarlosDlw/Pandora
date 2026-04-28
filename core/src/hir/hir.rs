use foundation::{arena::Arena, ids::ArenaId, ids::FileId, span::Span};

use super::symbols::SymbolId;

pub type HirId = ArenaId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirExpr {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Var(SymbolId),
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
    Expr {
        expr: HirId,
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
}
