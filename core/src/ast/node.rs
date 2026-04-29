use foundation::{ids::ArenaId, span::Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
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
    LetDecl {
        name: ArenaId,
        ty: Option<ArenaId>,
        value: ArenaId,
        is_const: bool,
        span: Span,
    },
    AssignStmt {
        target: ArenaId,
        value: ArenaId,
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
            | Self::UnaryExpr { span, .. }
            | Self::BinaryExpr { span, .. }
            | Self::CallExpr { span, .. }
            | Self::LetDecl { span, .. }
            | Self::AssignStmt { span, .. }
            | Self::ExprStmt { span, .. } => *span,
        }
    }
}
