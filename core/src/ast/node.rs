use foundation::{ids::ArenaId, span::Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
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
    BoolLiteral {
        value: bool,
        span: Span,
    },
    BinaryExpr {
        op: BinaryOp,
        left: ArenaId,
        right: ArenaId,
        span: Span,
    },
    LetDecl {
        name: ArenaId,
        ty: Option<ArenaId>,
        value: ArenaId,
        is_const: bool,
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
            | Self::BoolLiteral { span, .. }
            | Self::BinaryExpr { span, .. }
            | Self::LetDecl { span, .. }
            | Self::ExprStmt { span, .. } => *span,
        }
    }
}
