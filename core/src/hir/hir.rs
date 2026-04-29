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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncDecOp {
    Increment,
    Decrement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncDecPosition {
    Prefix,
    Postfix,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirExpr {
    Int(String),
    Float(String),
    Bool(bool),
    Null,
    Str(String),
    Char(char),
    Var(SymbolId),
    Unary { op: UnaryOp, operand: HirId },
    Binary { op: BinOp, lhs: HirId, rhs: HirId },
    IncDec {
        symbol: SymbolId,
        op: IncDecOp,
        position: IncDecPosition,
    },
    Call { callee: HirId, args: Vec<HirId> },
    Tuple(Vec<HirId>),
    TupleAccess { tuple: HirId, index: usize },
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
    TupleDestructure {
        names: Vec<SymbolId>,
        ty: Option<crate::analyzer::Type>,
        value: HirId,
        span: Span,
    },
    FnDecl {
        symbol: SymbolId,
        params: Vec<SymbolId>,
        return_ty: crate::analyzer::Type,
        body: Vec<HirStmt>,
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
    If {
        condition: HirId,
        then_branch: Vec<HirStmt>,
        else_branch: Option<Vec<HirStmt>>,
        span: Span,
    },
    While {
        condition: HirId,
        body: Vec<HirStmt>,
        span: Span,
    },
    For {
        init: Option<Box<HirStmt>>,
        condition: Option<HirId>,
        step: Option<HirId>,
        body: Vec<HirStmt>,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    Return {
        value: Option<HirId>,
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
