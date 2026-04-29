use foundation::{ids::ArenaId, span::Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AstNode {
    Invalid {
        span: Span,
    },
    Identifier {
        name: String,
        span: Span,
    },
    TypeName {
        name: String,
        span: Span,
    },
    IntegerLiteral {
        value: String,
        span: Span,
    },
    FloatLiteral {
        value: String,
        span: Span,
    },
    StringLiteral {
        value: String,
        span: Span,
    },
    CharLiteral {
        value: char,
        span: Span,
    },
    BoolLiteral {
        value: bool,
        span: Span,
    },
    NullLiteral {
        span: Span,
    },
    UnaryExpr {
        op: UnaryOp,
        operand: ArenaId,
        span: Span,
    },
    BinaryExpr {
        op: BinaryOp,
        left: ArenaId,
        right: ArenaId,
        span: Span,
    },
    CallExpr {
        callee: ArenaId,
        args: Vec<ArenaId>,
        span: Span,
    },
    TupleLiteral {
        items: Vec<ArenaId>,
        span: Span,
    },
    TupleAccess {
        tuple: ArenaId,
        index: usize,
        span: Span,
    },
    IncDecExpr {
        target: ArenaId,
        op: IncDecOp,
        position: IncDecPosition,
        span: Span,
    },
    LetDecl {
        name: ArenaId,
        ty: Option<ArenaId>,
        value: ArenaId,
        is_const: bool,
        span: Span,
    },
    TupleDestructureDecl {
        names: Vec<ArenaId>,
        ty: Option<ArenaId>,
        value: ArenaId,
        span: Span,
    },
    FnDecl {
        name: ArenaId,
        params: Vec<(ArenaId, ArenaId)>,
        return_ty: ArenaId,
        body: ArenaId,
        span: Span,
    },
    AssignStmt {
        target: ArenaId,
        value: ArenaId,
        span: Span,
    },
    CompoundAssignStmt {
        target: ArenaId,
        op: CompoundOp,
        value: ArenaId,
        span: Span,
    },
    IfStmt {
        condition: ArenaId,
        then_branch: ArenaId,
        else_branch: Option<ArenaId>,
        span: Span,
    },
    WhileStmt {
        condition: ArenaId,
        body: ArenaId,
        span: Span,
    },
    ForStmt {
        init: Option<ArenaId>,
        condition: Option<ArenaId>,
        step: Option<ArenaId>,
        body: ArenaId,
        span: Span,
    },
    BreakStmt {
        span: Span,
    },
    ContinueStmt {
        span: Span,
    },
    ReturnStmt {
        value: Option<ArenaId>,
        span: Span,
    },
    BlockStmt {
        statements: Vec<ArenaId>,
        span: Span,
    },
    ExprStmt {
        expr: ArenaId,
        span: Span,
    },
}

impl AstNode {
    pub fn span(&self) -> Span {
        match self {
            Self::Invalid { span }
            | Self::Identifier { span, .. }
            | Self::TypeName { span, .. }
            | Self::IntegerLiteral { span, .. }
            | Self::FloatLiteral { span, .. }
            | Self::StringLiteral { span, .. }
            | Self::CharLiteral { span, .. }
            | Self::BoolLiteral { span, .. }
            | Self::NullLiteral { span, .. }
            | Self::UnaryExpr { span, .. }
            | Self::BinaryExpr { span, .. }
            | Self::CallExpr { span, .. }
            | Self::TupleLiteral { span, .. }
            | Self::TupleAccess { span, .. }
            | Self::IncDecExpr { span, .. }
            | Self::LetDecl { span, .. }
            | Self::TupleDestructureDecl { span, .. }
            | Self::FnDecl { span, .. }
            | Self::AssignStmt { span, .. }
            | Self::CompoundAssignStmt { span, .. }
            | Self::IfStmt { span, .. }
            | Self::WhileStmt { span, .. }
            | Self::ForStmt { span, .. }
            | Self::BreakStmt { span, .. }
            | Self::ContinueStmt { span, .. }
            | Self::ReturnStmt { span, .. }
            | Self::BlockStmt { span, .. }
            | Self::ExprStmt { span, .. } => *span,
        }
    }
}
