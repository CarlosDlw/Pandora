use crate::{
    ast::{Ast, AstNode, CompoundOp},
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
    loop_depth: usize,
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
            loop_depth: 0,
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
        if self.current().is_some_and(|token| token.kind == TokenKind::While) {
            return self.parse_while_stmt();
        }
        if self.current().is_some_and(|token| token.kind == TokenKind::For) {
            return self.parse_for_stmt();
        }
        if self.current().is_some_and(|token| token.kind == TokenKind::Break) {
            return self.parse_break_stmt();
        }
        if self.current().is_some_and(|token| token.kind == TokenKind::Continue) {
            return self.parse_continue_stmt();
        }
        if self.current().is_some_and(|token| token.kind == TokenKind::If) {
            return self.parse_if_stmt();
        }
        if self.current().is_some_and(|token| token.kind == TokenKind::Else) {
            let span = self.current_span_or_eof();
            self.push_error("unexpected 'else' without matching 'if'", span);
            self.bump();
            return self.invalid_node(span);
        }
        if self.current().is_some_and(|token| token.kind == TokenKind::LeftBrace) {
            return self.parse_block_stmt();
        }
        if self.is_declaration_start() {
            return self.parse_let_decl();
        }
        if self.is_compound_assign_start() {
            return self.parse_compound_assign_stmt();
        }
        if self.is_assignment_start() {
            return self.parse_assign_stmt();
        }

        let expr = self.parse_expression();
        let span = self.node_span(expr);
        self.insert_node(AstNode::ExprStmt { expr, span })
    }

    fn parse_while_stmt(&mut self) -> ArenaId {
        let while_span = self.current_span_or_eof();
        self.bump();
        if self.current().is_none() || self.current().is_some_and(|t| t.kind == TokenKind::LeftBrace) {
            self.push_error("expected condition expression after 'while'", self.current_span_or_eof());
        }
        let condition = self.parse_expression();
        let prev_depth = self.loop_depth;
        self.loop_depth += 1;
        let body = self.parse_if_branch_block("expected '{' after while condition");
        self.loop_depth = prev_depth;
        let span = merge_span(while_span, self.node_span(body));
        self.insert_node(AstNode::WhileStmt {
            condition,
            body,
            span,
        })
    }

    fn parse_for_stmt(&mut self) -> ArenaId {
        let for_span = self.current_span_or_eof();
        self.bump();

        let init = if self.current().is_some_and(|t| t.kind == TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_for_init_decl())
        };
        if !self.consume_if(TokenKind::Semicolon) {
            self.push_error("expected ';' after for init", self.current_span_or_eof());
        }

        let condition = if self.current().is_some_and(|t| t.kind == TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expression())
        };
        if !self.consume_if(TokenKind::Semicolon) {
            self.push_error("expected ';' after for condition", self.current_span_or_eof());
        }

        let step = if self.current().is_some_and(|t| t.kind == TokenKind::LeftBrace) {
            None
        } else {
            Some(self.parse_expression())
        };

        let prev_depth = self.loop_depth;
        self.loop_depth += 1;
        let body = self.parse_if_branch_block("expected '{' after for header");
        self.loop_depth = prev_depth;

        let span = merge_span(for_span, self.node_span(body));
        self.insert_node(AstNode::ForStmt {
            init,
            condition,
            step,
            body,
            span,
        })
    }

    fn parse_for_init_decl(&mut self) -> ArenaId {
        let name_token = match self.current() {
            Some(token) if token.kind == TokenKind::Identifier => token.clone(),
            _ => {
                let span = self.current_span_or_eof();
                self.push_error("expected identifier in for init declaration", span);
                return self.invalid_node(span);
            }
        };
        self.bump();

        let name = self.insert_node(AstNode::Identifier {
            name: name_token.lexeme,
            span: name_token.span,
        });

        if !self.consume_if(TokenKind::Colon) {
            self.push_error("for init must use typed declaration 'name: type = value'", self.current_span_or_eof());
            return self.invalid_node(name_token.span);
        }
        let ty = self.parse_type_name();

        if !self.consume_if(TokenKind::Assign) {
            self.push_error("expected '=' in for init declaration", self.current_span_or_eof());
            return self.invalid_node(name_token.span);
        }
        let value = self.parse_expression();
        let span = merge_span(name_token.span, self.node_span(value));
        self.insert_node(AstNode::LetDecl {
            name,
            ty: Some(ty),
            value,
            is_const: false,
            span,
        })
    }

    fn parse_break_stmt(&mut self) -> ArenaId {
        let span = self.current_span_or_eof();
        self.bump();
        if self.loop_depth == 0 {
            self.push_error("break used outside of loop", span);
        }
        self.insert_node(AstNode::BreakStmt { span })
    }

    fn parse_continue_stmt(&mut self) -> ArenaId {
        let span = self.current_span_or_eof();
        self.bump();
        if self.loop_depth == 0 {
            self.push_error("continue used outside of loop", span);
        }
        self.insert_node(AstNode::ContinueStmt { span })
    }

    fn parse_if_stmt(&mut self) -> ArenaId {
        let if_span = self.current_span_or_eof();
        self.bump();

        if self.current().is_none() || self.current().is_some_and(|t| t.kind == TokenKind::LeftBrace) {
            self.push_error("expected condition expression after 'if'", self.current_span_or_eof());
        }
        let condition = self.parse_expression();
        let then_branch = self.parse_if_branch_block("expected '{' after if condition");

        let mut else_branch = None;
        if self.consume_if(TokenKind::Else) {
            if self.current().is_some_and(|t| t.kind == TokenKind::If) {
                else_branch = Some(self.parse_if_stmt());
            } else {
                else_branch = Some(self.parse_if_branch_block("expected '{' or 'if' after 'else'"));
            }
        }

        let end_span = else_branch
            .map(|id| self.node_span(id))
            .unwrap_or_else(|| self.node_span(then_branch));
        let span = merge_span(if_span, end_span);
        self.insert_node(AstNode::IfStmt {
            condition,
            then_branch,
            else_branch,
            span,
        })
    }

    fn parse_if_branch_block(&mut self, message: &str) -> ArenaId {
        if self.current().is_some_and(|t| t.kind == TokenKind::LeftBrace) {
            return self.parse_block_stmt();
        }
        let span = self.current_span_or_eof();
        self.push_error(message, span);
        self.invalid_node(span)
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

    fn parse_compound_assign_stmt(&mut self) -> ArenaId {
        let name_token = match self.current() {
            Some(token) if token.kind == TokenKind::Identifier => token.clone(),
            _ => {
                let span = self.current_span_or_eof();
                self.push_error("expected identifier in compound assignment", span);
                self.synchronize();
                return self.invalid_node(span);
            }
        };
        self.bump();

        let target = self.insert_node(AstNode::Identifier {
            name: name_token.lexeme,
            span: name_token.span,
        });

        let op = match self.current().map(|t| t.kind.clone()) {
            Some(TokenKind::PlusAssign) => CompoundOp::Add,
            Some(TokenKind::MinusAssign) => CompoundOp::Subtract,
            Some(TokenKind::StarAssign) => CompoundOp::Multiply,
            Some(TokenKind::SlashAssign) => CompoundOp::Divide,
            Some(TokenKind::PercentAssign) => CompoundOp::Modulo,
            Some(TokenKind::DoubleStarAssign) => CompoundOp::Power,
            Some(TokenKind::AmpersandAssign) => CompoundOp::BitAnd,
            Some(TokenKind::PipeAssign) => CompoundOp::BitOr,
            Some(TokenKind::CaretAssign) => CompoundOp::BitXor,
            Some(TokenKind::ShiftLeftAssign) => CompoundOp::ShiftLeft,
            Some(TokenKind::ShiftRightAssign) => CompoundOp::ShiftRight,
            _ => {
                let span = self.current_span_or_eof();
                self.push_error("expected compound assignment operator", span);
                self.synchronize();
                return self.invalid_node(name_token.span);
            }
        };
        self.bump();

        let value = self.parse_expression();
        let span = merge_span(name_token.span, self.node_span(value));
        self.insert_node(AstNode::CompoundAssignStmt {
            target,
            op,
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

    fn is_compound_assign_start(&self) -> bool {
        matches!(
            (self.peek_kind(0), self.peek_kind(1)),
            (
                Some(TokenKind::Identifier),
                Some(
                    TokenKind::PlusAssign
                        | TokenKind::MinusAssign
                        | TokenKind::StarAssign
                        | TokenKind::DoubleStarAssign
                        | TokenKind::SlashAssign
                        | TokenKind::PercentAssign
                        | TokenKind::AmpersandAssign
                        | TokenKind::PipeAssign
                        | TokenKind::CaretAssign
                        | TokenKind::ShiftLeftAssign
                        | TokenKind::ShiftRightAssign
                )
            )
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

    #[test]
    fn parser_parses_logical_precedence() {
        let source = "x := a + b * c < d && e || f";
        let lex_out = lex(FileId::from_u32(10), source);
        let (ast, diagnostics) = parse(FileId::from_u32(10), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let AstNode::LetDecl { value, .. } = ast.get(ast.roots[0]).expect("let") else {
            panic!("expected let");
        };
        let AstNode::BinaryExpr {
            op: BinaryOp::LogicalOr,
            ..
        } = ast.get(*value).expect("logical or root") else {
            panic!("expected logical or at root");
        };
    }

    #[test]
    fn parser_makes_power_right_associative() {
        let source = "x := 2 ** 3 ** 2";
        let lex_out = lex(FileId::from_u32(11), source);
        let (ast, diagnostics) = parse(FileId::from_u32(11), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let AstNode::LetDecl { value, .. } = ast.get(ast.roots[0]).expect("let") else {
            panic!("expected let");
        };
        let AstNode::BinaryExpr {
            op: BinaryOp::Power,
            right,
            ..
        } = ast.get(*value).expect("outer pow") else {
            panic!("expected power expression");
        };
        assert!(matches!(
            ast.get(*right),
            Some(AstNode::BinaryExpr {
                op: BinaryOp::Power,
                ..
            })
        ));
    }

    #[test]
    fn parser_parses_unary_not_and_bit_not() {
        let source = "x := !flag; y := ~mask";
        let lex_out = lex(FileId::from_u32(12), source);
        let (ast, diagnostics) = parse(FileId::from_u32(12), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let AstNode::LetDecl { value, .. } = ast.get(ast.roots[0]).expect("first let") else {
            panic!("expected let");
        };
        assert!(matches!(
            ast.get(*value),
            Some(AstNode::UnaryExpr {
                op: crate::ast::UnaryOp::Not,
                ..
            })
        ));
        let AstNode::LetDecl { value, .. } = ast.get(ast.roots[1]).expect("second let") else {
            panic!("expected let");
        };
        assert!(matches!(
            ast.get(*value),
            Some(AstNode::UnaryExpr {
                op: crate::ast::UnaryOp::BitNot,
                ..
            })
        ));
    }

    #[test]
    fn parses_if_else_chain_as_statements() {
        let source = "if true { x := 1 } else if false { x := 2 } else { x := 3 }";
        let lex_out = lex(FileId::from_u32(13), source);
        let (ast, diagnostics) = parse(FileId::from_u32(13), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let root = ast.roots[0];
        let AstNode::IfStmt {
            then_branch,
            else_branch,
            ..
        } = ast.get(root).expect("if stmt") else {
            panic!("expected if statement");
        };
        assert!(matches!(ast.get(*then_branch), Some(AstNode::BlockStmt { .. })));
        let else_id = else_branch.expect("else branch");
        assert!(matches!(
            ast.get(else_id),
            Some(AstNode::IfStmt { .. }) | Some(AstNode::BlockStmt { .. })
        ));
    }

    #[test]
    fn reports_missing_if_block() {
        let source = "if true x := 1";
        let lex_out = lex(FileId::from_u32(14), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(14), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("expected '{' after if condition")));
    }

    #[test]
    fn reports_unexpected_else_without_if() {
        let source = "else { x := 1 }";
        let lex_out = lex(FileId::from_u32(15), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(15), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("unexpected 'else' without matching 'if'")));
    }

    #[test]
    fn parses_while_break_continue_statements() {
        let source = "while 1 { if true { continue }; break }";
        let lex_out = lex(FileId::from_u32(16), source);
        let (ast, diagnostics) = parse(FileId::from_u32(16), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        assert!(matches!(ast.get(ast.roots[0]), Some(AstNode::WhileStmt { .. })));
    }

    #[test]
    fn reports_missing_while_block() {
        let source = "while true x := 1";
        let lex_out = lex(FileId::from_u32(17), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(17), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
        assert!(diagnostics.iter().any(|d| d.message.contains("expected '{' after while condition")));
    }

    #[test]
    fn reports_break_outside_loop() {
        let source = "break";
        let lex_out = lex(FileId::from_u32(18), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(18), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
        assert!(diagnostics.iter().any(|d| d.message.contains("break used outside of loop")));
    }

    #[test]
    fn reports_continue_outside_loop() {
        let source = "continue";
        let lex_out = lex(FileId::from_u32(19), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(19), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("continue used outside of loop")));
    }

    #[test]
    fn parses_for_stmt_with_all_fields() {
        let source = "for i: i32 = 0; i < 3; i++ { print(i) }";
        let lex_out = lex(FileId::from_u32(43), source);
        let (ast, diagnostics) = parse(FileId::from_u32(43), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        assert!(matches!(ast.get(ast.roots[0]), Some(AstNode::ForStmt { .. })));
    }

    #[test]
    fn parses_for_stmt_with_empty_fields() {
        let source = "for ; ; { break }";
        let lex_out = lex(FileId::from_u32(44), source);
        let (ast, diagnostics) = parse(FileId::from_u32(44), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let Some(AstNode::ForStmt { init, condition, step, .. }) = ast.get(ast.roots[0]) else {
            panic!("expected for stmt");
        };
        assert!(init.is_none());
        assert!(condition.is_none());
        assert!(step.is_none());
    }

    #[test]
    fn for_init_requires_typed_declaration() {
        let source = "for i := 0; i < 3; i++ { }";
        let lex_out = lex(FileId::from_u32(45), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(45), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("for init must use typed declaration")));
    }

    #[test]
    fn parses_prefix_and_postfix_incdec() {
        let source = "x: i32 = 1; y := ++x; z := x--";
        let lex_out = lex(FileId::from_u32(46), source);
        let (ast, diagnostics) = parse(FileId::from_u32(46), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let AstNode::LetDecl { value: y_value, .. } = ast.get(ast.roots[1]).expect("y let") else {
            panic!("expected let decl");
        };
        assert!(matches!(
            ast.get(*y_value),
            Some(AstNode::IncDecExpr {
                position: crate::ast::IncDecPosition::Prefix,
                ..
            })
        ));
        let AstNode::LetDecl { value: z_value, .. } = ast.get(ast.roots[2]).expect("z let") else {
            panic!("expected let decl");
        };
        assert!(matches!(
            ast.get(*z_value),
            Some(AstNode::IncDecExpr {
                position: crate::ast::IncDecPosition::Postfix,
                ..
            })
        ));
    }

    #[test]
    fn parses_compound_assign_stmt() {
        let source = "x += 1";
        let lex_out = lex(FileId::from_u32(40), source);
        let (ast, diagnostics) = parse(FileId::from_u32(40), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        assert!(matches!(
            ast.get(ast.roots[0]),
            Some(AstNode::CompoundAssignStmt {
                op: crate::ast::CompoundOp::Add,
                ..
            })
        ));
    }

    #[test]
    fn parses_all_compound_operators() {
        let source = "a += 1; b -= 1; c *= 1; d /= 1; e %= 1; f **= 1; g &= 1; h |= 1; i ^= 1; j <<= 1; k >>= 1";
        let lex_out = lex(FileId::from_u32(41), source);
        let (ast, diagnostics) = parse(FileId::from_u32(41), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        assert_eq!(ast.roots.len(), 11);
        assert!(ast
            .roots
            .iter()
            .all(|id| matches!(ast.get(*id), Some(AstNode::CompoundAssignStmt { .. }))));
    }

    #[test]
    fn compound_assign_requires_identifier() {
        let source = "1 += 2";
        let lex_out = lex(FileId::from_u32(42), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(42), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
    }
}
