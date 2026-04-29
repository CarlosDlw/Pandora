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
    Range { start: HirId, end: HirId, inclusive: bool },
    IncDec {
        symbol: SymbolId,
        op: IncDecOp,
        position: IncDecPosition,
    },
    Call { callee: HirId, args: Vec<HirId> },
    MethodCall {
        receiver: HirId,
        method: String,
        args: Vec<HirId>,
    },
    StaticMethodCall {
        type_name: String,
        method: String,
        args: Vec<HirId>,
    },
    StructLiteral {
        type_name: String,
        fields: Vec<(String, HirId)>,
    },
    FieldAccess {
        base: HirId,
        field: String,
    },
    Tuple(Vec<HirId>),
    TupleAccess { tuple: HirId, index: usize },
    Array(Vec<HirArrayItem>),
    Map(Vec<(HirId, HirId)>),
    Set(Vec<HirId>),
    ArrayAccess { array: HirId, index: HirId },
    Propagate { expr: HirId },
    TryCatch {
        try_expr: HirId,
        err_symbol: SymbolId,
        catch_stmts: Vec<HirStmt>,
        catch_value: HirId,
    },
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirArrayItem {
    Expr(HirId),
    SpreadExpr(HirId),
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
    StructDecl {
        symbol: SymbolId,
        name: String,
        fields: Vec<(String, crate::analyzer::Type)>,
        span: Span,
    },
    TraitDecl {
        symbol: SymbolId,
        name: String,
        methods: Vec<(String, Vec<crate::analyzer::Type>, crate::analyzer::Type, bool)>,
        span: Span,
    },
    ImplBlock {
        target: crate::analyzer::Type,
        trait_target: Option<crate::analyzer::Type>,
        methods: Vec<HirStmt>,
        span: Span,
    },
    FnDecl {
        symbol: SymbolId,
        name: String,
        is_instance: bool,
        params: Vec<SymbolId>,
        param_defaults: Vec<Option<HirId>>,
        return_ty: crate::analyzer::Type,
        body: Vec<HirStmt>,
        span: Span,
    },
    Assign {
        symbol: SymbolId,
        value: HirId,
        span: Span,
    },
    ArrayAssign {
        symbol: SymbolId,
        index: HirId,
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
    ForIn {
        symbol: SymbolId,
        iterable_symbol: SymbolId,
        index_symbol: SymbolId,
        iterable: HirId,
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
        values: Vec<HirId>,
        span: Span,
    },
    Import {
        path: String,
        alias: SymbolId,
        span: Span,
    },
    FromImport {
        path: String,
        names: Vec<SymbolId>,
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
