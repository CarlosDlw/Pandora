use crate::{
    ast::{Ast, AstNode},
    lexer::{Token, TokenKind},
};
use foundation::{
    arena::Arena,
    diagnostics::{Diagnostic, Diagnostics, Severity},
    ids::{ArenaId, FileId},
    span::Span,
};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Diagnostics,
    arena: Arena<AstNode>,
    file_id: FileId,
    source_len: u32,
    invalid_node_id: ArenaId,
}

pub fn parse(file_id: FileId, source_len: u32, tokens: Vec<Token>) -> (Ast, Diagnostics) {
    let mut parser = Parser::new(file_id, source_len, tokens);
    let ast = parser.parse_program();
    (ast, parser.diagnostics)
}

impl Parser {
    pub fn new(file_id: FileId, source_len: u32, tokens: Vec<Token>) -> Self {
        let mut arena = Arena::new();
        let invalid_span = Span::new_unchecked(file_id, 0, 0);
        let invalid_node_id = arena
            .insert(AstNode::Invalid { span: invalid_span })
            .expect("arena must accept first node");

        Self {
            tokens,
            pos: 0,
            diagnostics: Diagnostics::new(),
            arena,
            file_id,
            source_len,
            invalid_node_id,
        }
    }

    fn parse_program(&mut self) -> Ast {
        let mut roots = Vec::new();
        while self.current().is_some() {
            let node = self.parse_statement();
            roots.push(node);
            self.consume_if(TokenKind::Semicolon);
        }
        Ast {
            file_id: self.file_id,
            roots,
            arena: std::mem::replace(&mut self.arena, Arena::new()),
        }
    }

    fn parse_statement(&mut self) -> ArenaId {
        if self.current().is_some_and(|token| token.kind == TokenKind::LeftBrace) {
            return self.parse_block_stmt();
        }
        if self.is_declaration_start() {
            return self.parse_let_decl();
        }
        if self.is_assignment_start() {
            return self.parse_assign_stmt();
        }

        let expr = self.parse_expression();
        let span = self.node_span(expr);
        self.insert_node(AstNode::ExprStmt { expr, span })
    }

    fn parse_block_stmt(&mut self) -> ArenaId {
        let open = self.current_span_or_eof();
        self.bump();

        let mut statements = Vec::new();
        while self.current().is_some() {
            if self.consume_if(TokenKind::RightBrace) {
                let span = merge_span(open, self.previous_span_or(open));
                return self.insert_node(AstNode::BlockStmt { statements, span });
            }
            let stmt = self.parse_statement();
            statements.push(stmt);
            self.consume_if(TokenKind::Semicolon);
        }

        let eof = self.eof_span();
        let span = merge_span(open, eof);
        self.push_error("expected '}'", span);
        self.insert_node(AstNode::BlockStmt { statements, span })
    }

    fn parse_let_decl(&mut self) -> ArenaId {
        let name_token = match self.current() {
            Some(token) if token.kind == TokenKind::Identifier => token.clone(),
            _ => {
                let span = self.current_span_or_eof();
                self.push_error("expected identifier in declaration", span);
                self.synchronize();
                return self.invalid_node(span);
            }
        };
        self.bump();

        let name = self.insert_node(AstNode::Identifier {
            name: name_token.lexeme,
            span: name_token.span,
        });

        let mut is_const = false;
        let mut ty = None;

        match self.current().map(|t| t.kind.clone()) {
            Some(TokenKind::DoubleColon) => {
                is_const = true;
                self.bump();
                ty = Some(self.parse_type_name());
            }
            Some(TokenKind::Colon) => {
                self.bump();
                ty = Some(self.parse_type_name());
            }
            Some(TokenKind::InferAssign) => {
                self.bump();
            }
            _ => {
                let span = self.current_span_or_eof();
                self.push_error("expected ':', '::' or ':=' after identifier", span);
                self.synchronize();
                return self.invalid_node(name_token.span);
            }
        }

        if !matches!(self.previous_kind(), Some(TokenKind::InferAssign)) && !self.consume_if(TokenKind::Assign)
        {
            let span = self.current_span_or_eof();
            self.push_error("expected '=' in declaration", span);
            self.synchronize();
            return self.invalid_node(name_token.span);
        }

        let value = self.parse_expression();
        let span = merge_span(name_token.span, self.node_span(value));
        self.insert_node(AstNode::LetDecl {
            name,
            ty,
            value,
            is_const,
            span,
        })
    }

    fn parse_type_name(&mut self) -> ArenaId {
        let token = match self.current() {
            Some(token) if token.kind == TokenKind::TypeName => token.clone(),
            Some(token) if token.kind == TokenKind::Identifier => token.clone(),
            _ => {
                let span = self.current_span_or_eof();
                self.push_error("expected type name", span);
                return self.invalid_node(span);
            }
        };
        self.bump();
        self.insert_node(AstNode::TypeName {
            name: token.lexeme,
            span: token.span,
        })
    }

    fn parse_assign_stmt(&mut self) -> ArenaId {
        let name_token = match self.current() {
            Some(token) if token.kind == TokenKind::Identifier => token.clone(),
            _ => {
                let span = self.current_span_or_eof();
                self.push_error("expected identifier in assignment", span);
                self.synchronize();
                return self.invalid_node(span);
            }
        };
        self.bump();

        let target = self.insert_node(AstNode::Identifier {
            name: name_token.lexeme,
            span: name_token.span,
        });

        if !self.consume_if(TokenKind::Assign) {
            let span = self.current_span_or_eof();
            self.push_error("expected '=' in assignment", span);
            self.synchronize();
            return self.invalid_node(name_token.span);
        }

        let value = self.parse_expression();
        let span = merge_span(name_token.span, self.node_span(value));
        self.insert_node(AstNode::AssignStmt {
            target,
            value,
            span,
        })
    }

    fn is_declaration_start(&self) -> bool {
        matches!(
            (self.peek_kind(0), self.peek_kind(1)),
            (
                Some(TokenKind::Identifier),
                Some(TokenKind::Colon | TokenKind::DoubleColon | TokenKind::InferAssign)
            )
        )
    }

    fn is_assignment_start(&self) -> bool {
        matches!(
            (self.peek_kind(0), self.peek_kind(1)),
            (Some(TokenKind::Identifier), Some(TokenKind::Assign))
        )
    }

    pub(super) fn current(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    pub(super) fn peek_kind(&self, lookahead: usize) -> Option<TokenKind> {
        self.tokens.get(self.pos + lookahead).map(|t| t.kind.clone())
    }

    pub(super) fn bump(&mut self) {
        self.pos += 1;
    }

    pub(super) fn consume_if(&mut self, kind: TokenKind) -> bool {
        if self.current().is_some_and(|t| t.kind == kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(super) fn previous_kind(&self) -> Option<TokenKind> {
        if self.pos == 0 {
            None
        } else {
            self.tokens.get(self.pos - 1).map(|t| t.kind.clone())
        }
    }

    pub(super) fn previous_span_or(&self, fallback: Span) -> Span {
        if self.pos == 0 {
            fallback
        } else {
            self.tokens.get(self.pos - 1).map(|t| t.span).unwrap_or(fallback)
        }
    }

    pub(super) fn insert_node(&mut self, node: AstNode) -> ArenaId {
        match self.arena.insert(node) {
            Ok(id) => id,
            Err(_) => self.invalid_node_id,
        }
    }

    pub(super) fn invalid_node(&mut self, span: Span) -> ArenaId {
        self.insert_node(AstNode::Invalid { span })
    }

    pub(super) fn node_span(&self, id: ArenaId) -> Span {
        self.arena
            .get(id)
            .map(AstNode::span)
            .unwrap_or_else(|| self.eof_span())
    }

    pub(super) fn merge_spans(&self, left: ArenaId, right: ArenaId) -> Span {
        merge_span(self.node_span(left), self.node_span(right))
    }

    pub(super) fn eof_span(&self) -> Span {
        Span::new_unchecked(self.file_id, self.source_len, self.source_len)
    }

    pub(super) fn current_span_or_eof(&self) -> Span {
        self.current().map(|t| t.span).unwrap_or_else(|| self.eof_span())
    }

    pub(super) fn push_error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics
            .push(Diagnostic::new(message, span, Severity::Error));
    }
}

fn merge_span(start: Span, end: Span) -> Span {
    Span::new_unchecked(start.file_id(), start.start(), end.end())
}

#[cfg(test)]
mod tests {
    use foundation::ids::FileId;

    use crate::{
        ast::{AstNode, BinaryOp},
        lexer::lex,
    };

    use super::parse;

    #[test]
    fn parser_respects_operator_precedence() {
        let source = "a := 1 + 2 * 3";
        let lex_out = lex(FileId::from_u32(1), source);
        let (ast, diagnostics) = parse(FileId::from_u32(1), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.is_empty());
        let root = ast.roots[0];
        let decl = ast.get(root).expect("root node");
        let value_id = match decl {
            AstNode::LetDecl { value, .. } => *value,
            _ => panic!("expected let declaration"),
        };
        let value = ast.get(value_id).expect("value node");
        match value {
            AstNode::BinaryExpr { op, right, .. } => {
                assert_eq!(*op, BinaryOp::Add);
                let rhs = ast.get(*right).expect("right node");
                assert!(matches!(
                    rhs,
                    AstNode::BinaryExpr {
                        op: BinaryOp::Multiply,
                        ..
                    }
                ));
            }
            _ => panic!("expected binary expression"),
        }
    }

    #[test]
    fn parser_keeps_ast_on_error_and_continues() {
        let source = "a := (1 +\nb := 2";
        let lex_out = lex(FileId::from_u32(2), source);
        let (ast, diagnostics) = parse(FileId::from_u32(2), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.is_empty());
        assert!(!ast.roots.is_empty());
    }

    #[test]
    fn parser_uses_left_associativity_for_product_ops() {
        let source = "a := 8 / 4 / 2";
        let lex_out = lex(FileId::from_u32(3), source);
        let (ast, diagnostics) = parse(FileId::from_u32(3), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.is_empty());
        let root = ast.roots[0];
        let AstNode::LetDecl { value, .. } = ast.get(root).expect("let decl") else {
            panic!("expected let declaration");
        };

        let AstNode::BinaryExpr { left, right, .. } = ast.get(*value).expect("outer binary") else {
            panic!("expected outer binary expr");
        };
        assert!(matches!(ast.get(*left), Some(AstNode::BinaryExpr { .. })));
        assert!(matches!(
            ast.get(*right),
            Some(AstNode::IntegerLiteral { value, .. }) if value == "2"
        ));
    }

    #[test]
    fn missing_right_paren_reports_wide_span() {
        let source = "a := (1 + 2";
        let lex_out = lex(FileId::from_u32(4), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(4), source.len() as u32, lex_out.tokens);
        let diag = diagnostics.iter().next().expect("expected diagnostic");
        assert_eq!(diag.span.start(), 5);
        assert_eq!(diag.span.end(), source.len() as u32);
    }

    #[test]
    fn parser_recovers_after_invalid_declaration() {
        let source = "a: = 1; b := 2";
        let lex_out = lex(FileId::from_u32(5), source);
        let (ast, diagnostics) = parse(FileId::from_u32(5), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
        assert!(ast.roots.len() >= 2);
    }

    #[test]
    fn parses_call_expression_with_multiple_args() {
        let source = "print(name, age)";
        let lex_out = lex(FileId::from_u32(6), source);
        let (ast, diagnostics) = parse(FileId::from_u32(6), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let root = ast.roots[0];
        let expr_stmt = ast.get(root).expect("expr stmt");
        let expr_id = match expr_stmt {
            AstNode::ExprStmt { expr, .. } => *expr,
            _ => panic!("expected expression statement"),
        };
        assert!(matches!(
            ast.get(expr_id),
            Some(AstNode::CallExpr { args, .. }) if args.len() == 2
        ));
    }

    #[test]
    fn parses_block_statement_and_inner_declaration() {
        let source = "{ x := 1; print(x) }";
        let lex_out = lex(FileId::from_u32(7), source);
        let (ast, diagnostics) = parse(FileId::from_u32(7), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        assert_eq!(ast.roots.len(), 1);
        let root = ast.roots[0];
        let AstNode::BlockStmt { statements, .. } = ast.get(root).expect("block stmt") else {
            panic!("expected block statement");
        };
        assert_eq!(statements.len(), 2);
        assert!(matches!(ast.get(statements[0]), Some(AstNode::LetDecl { .. })));
        assert!(matches!(ast.get(statements[1]), Some(AstNode::ExprStmt { .. })));
    }

    #[test]
    fn reports_missing_block_closing_brace() {
        let source = "{ x := 1";
        let lex_out = lex(FileId::from_u32(8), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(8), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
        assert!(diagnostics.iter().any(|d| d.message.contains("expected '}'")));
    }

    #[test]
    fn block_supports_optional_semicolons() {
        let source = "{ x := 1 y := 2 print(x, y) }";
        let lex_out = lex(FileId::from_u32(9), source);
        let (ast, diagnostics) = parse(FileId::from_u32(9), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let root = ast.roots[0];
        let AstNode::BlockStmt { statements, .. } = ast.get(root).expect("block stmt") else {
            panic!("expected block statement");
        };
        assert_eq!(statements.len(), 3);
    }
}
