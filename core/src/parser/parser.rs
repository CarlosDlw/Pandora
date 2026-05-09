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
        if self
            .current()
            .is_some_and(|token| token.kind == TokenKind::Struct)
        {
            return self.parse_struct_decl();
        }
        if self
            .current()
            .is_some_and(|token| token.kind == TokenKind::Trait)
        {
            return self.parse_trait_decl();
        }
        if self
            .current()
            .is_some_and(|token| token.kind == TokenKind::Impl)
        {
            return self.parse_impl_block();
        }
        if self
            .current()
            .is_some_and(|token| token.kind == TokenKind::Fn)
        {
            return self.parse_fn_decl();
        }
        if self
            .current()
            .is_some_and(|token| token.kind == TokenKind::Return)
        {
            return self.parse_return_stmt();
        }
        if self
            .current()
            .is_some_and(|token| token.kind == TokenKind::While)
        {
            return self.parse_while_stmt();
        }
        if self
            .current()
            .is_some_and(|token| token.kind == TokenKind::For)
        {
            return self.parse_for_stmt();
        }
        if self
            .current()
            .is_some_and(|token| token.kind == TokenKind::Break)
        {
            return self.parse_break_stmt();
        }
        if self
            .current()
            .is_some_and(|token| token.kind == TokenKind::Continue)
        {
            return self.parse_continue_stmt();
        }
        if self
            .current()
            .is_some_and(|token| token.kind == TokenKind::If)
        {
            return self.parse_if_stmt();
        }
        if self
            .current()
            .is_some_and(|token| token.kind == TokenKind::Import)
        {
            return self.parse_import_stmt();
        }
        if self
            .current()
            .is_some_and(|token| token.kind == TokenKind::From)
        {
            return self.parse_from_import_stmt();
        }
        if self
            .current()
            .is_some_and(|token| token.kind == TokenKind::Else)
        {
            let span = self.current_span_or_eof();
            self.push_error("unexpected 'else' without matching 'if'", span);
            self.bump();
            return self.invalid_node(span);
        }
        if self
            .current()
            .is_some_and(|token| token.kind == TokenKind::LeftBrace)
        {
            return self.parse_block_stmt();
        }
        if self.is_declaration_start() {
            return self.parse_let_decl();
        }
        if self.is_tuple_destructure_decl_start() {
            return self.parse_tuple_destructure_decl();
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
        if self.current().is_none()
            || self
                .current()
                .is_some_and(|t| t.kind == TokenKind::LeftBrace)
        {
            self.push_error(
                "expected condition expression after 'while'",
                self.current_span_or_eof(),
            );
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

        if self.is_for_in_header() {
            return self.parse_for_in_stmt(for_span);
        }

        let init = if self
            .current()
            .is_some_and(|t| t.kind == TokenKind::Semicolon)
        {
            None
        } else {
            Some(self.parse_for_init_decl())
        };
        if !self.consume_if(TokenKind::Semicolon) {
            self.push_error("expected ';' after for init", self.current_span_or_eof());
        }

        let condition = if self
            .current()
            .is_some_and(|t| t.kind == TokenKind::Semicolon)
        {
            None
        } else {
            Some(self.parse_expression())
        };
        if !self.consume_if(TokenKind::Semicolon) {
            self.push_error(
                "expected ';' after for condition",
                self.current_span_or_eof(),
            );
        }

        let step = if self
            .current()
            .is_some_and(|t| t.kind == TokenKind::LeftBrace)
        {
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

    fn parse_import_stmt(&mut self) -> ArenaId {
        let start = self.current_span_or_eof();
        self.bump();
        let path_tok = match self.current() {
            Some(t) if t.kind == TokenKind::String => t.clone(),
            _ => {
                self.push_error(
                    "expected string path after import",
                    self.current_span_or_eof(),
                );
                return self.invalid_node(start);
            }
        };
        self.bump();
        if !self.consume_if(TokenKind::As) {
            self.push_error(
                "import requires alias: use `import \"...\" as name`",
                self.current_span_or_eof(),
            );
            return self.invalid_node(start);
        }
        let alias = self.parse_required_identifier("expected alias identifier after 'as'");
        let span = merge_span(start, self.node_span(alias));
        self.insert_node(AstNode::ImportStmt {
            path: path_tok.lexeme,
            alias,
            span,
        })
    }

    fn parse_from_import_stmt(&mut self) -> ArenaId {
        let start = self.current_span_or_eof();
        self.bump();
        let path_tok = match self.current() {
            Some(t) if t.kind == TokenKind::String => t.clone(),
            _ => {
                self.push_error(
                    "expected string path after from",
                    self.current_span_or_eof(),
                );
                return self.invalid_node(start);
            }
        };
        self.bump();
        if !self.consume_if(TokenKind::Import) {
            self.push_error(
                "expected `import` after module path",
                self.current_span_or_eof(),
            );
            return self.invalid_node(start);
        }
        let mut names = Vec::new();
        names.push(self.parse_required_identifier("expected imported symbol name"));
        while self.consume_if(TokenKind::Comma) {
            names.push(self.parse_required_identifier("expected imported symbol name"));
        }
        let span = merge_span(start, self.previous_span_or(start));
        self.insert_node(AstNode::FromImportStmt {
            path: path_tok.lexeme,
            names,
            span,
        })
    }

    fn parse_for_in_stmt(&mut self, for_span: Span) -> ArenaId {
        let name = self.parse_required_identifier("expected iteration variable name after 'for'");
        let ty = if self.consume_if(TokenKind::Colon) {
            Some(self.parse_type_name())
        } else {
            None
        };
        if !self.consume_if(TokenKind::In) {
            self.push_error("expected 'in' in for-in loop", self.current_span_or_eof());
            return self.invalid_node(for_span);
        }
        let iterable = if matches!(self.peek_kind(0), Some(TokenKind::Identifier))
            && matches!(self.peek_kind(1), Some(TokenKind::LeftBrace))
        {
            self.parse_required_identifier("expected iterable expression in for-in loop")
        } else {
            self.parse_expression()
        };
        let prev_depth = self.loop_depth;
        self.loop_depth += 1;
        let body = self.parse_if_branch_block("expected '{' after for-in iterable");
        self.loop_depth = prev_depth;
        let span = merge_span(for_span, self.node_span(body));
        self.insert_node(AstNode::ForInStmt {
            name,
            ty,
            iterable,
            body,
            span,
        })
    }

    fn is_for_in_header(&self) -> bool {
        if self.peek_kind(0) != Some(TokenKind::Identifier) {
            return false;
        }
        let mut idx = 1usize;
        if self.peek_kind(idx) == Some(TokenKind::Colon) {
            idx += 1;
            let mut depth_paren = 0usize;
            let mut depth_bracket = 0usize;
            loop {
                match self.peek_kind(idx) {
                    Some(TokenKind::In) if depth_paren == 0 && depth_bracket == 0 => return true,
                    Some(TokenKind::Semicolon) => return false,
                    Some(TokenKind::LeftBrace) => return false,
                    Some(TokenKind::LeftParen) => depth_paren += 1,
                    Some(TokenKind::RightParen) => depth_paren = depth_paren.saturating_sub(1),
                    Some(TokenKind::LeftBracket) => depth_bracket += 1,
                    Some(TokenKind::RightBracket) => {
                        depth_bracket = depth_bracket.saturating_sub(1)
                    }
                    Some(_) => {}
                    None => return false,
                }
                idx += 1;
            }
        }
        self.peek_kind(idx) == Some(TokenKind::In)
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
            self.push_error(
                "for init must use typed declaration 'name: type = value'",
                self.current_span_or_eof(),
            );
            return self.invalid_node(name_token.span);
        }
        let ty = self.parse_type_name();

        if !self.consume_if(TokenKind::Assign) {
            self.push_error(
                "expected '=' in for init declaration",
                self.current_span_or_eof(),
            );
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

    fn parse_return_stmt(&mut self) -> ArenaId {
        let return_span = self.current_span_or_eof();
        self.bump();
        if self.current().is_none()
            || self
                .current()
                .is_some_and(|t| t.kind == TokenKind::Semicolon || t.kind == TokenKind::RightBrace)
        {
            return self.insert_node(AstNode::ReturnStmt {
                values: Vec::new(),
                span: return_span,
            });
        }
        let first = self.parse_expression();
        let mut values = vec![first];
        while self.consume_if(TokenKind::Comma) {
            if self.current().is_none()
                || self.current().is_some_and(|t| {
                    t.kind == TokenKind::Semicolon || t.kind == TokenKind::RightBrace
                })
            {
                self.push_error(
                    "expected return value after ','",
                    self.current_span_or_eof(),
                );
                break;
            }
            values.push(self.parse_expression());
        }
        let end_span = values
            .last()
            .map(|id| self.node_span(*id))
            .unwrap_or(return_span);
        let span = merge_span(return_span, end_span);
        self.insert_node(AstNode::ReturnStmt { values, span })
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

        if self.current().is_none()
            || self
                .current()
                .is_some_and(|t| t.kind == TokenKind::LeftBrace)
        {
            self.push_error(
                "expected condition expression after 'if'",
                self.current_span_or_eof(),
            );
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
        if self
            .current()
            .is_some_and(|t| t.kind == TokenKind::LeftBrace)
        {
            return self.parse_block_stmt();
        }
        let span = self.current_span_or_eof();
        self.push_error(message, span);
        self.invalid_node(span)
    }

    pub(super) fn parse_block_stmt(&mut self) -> ArenaId {
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
                ty = Some(self.parse_type_ref());
            }
            Some(TokenKind::Colon) => {
                self.bump();
                ty = Some(self.parse_type_ref());
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

        if !matches!(self.previous_kind(), Some(TokenKind::InferAssign))
            && !self.consume_if(TokenKind::Assign)
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

    fn parse_tuple_destructure_decl(&mut self) -> ArenaId {
        let start = self.current_span_or_eof();
        let names = self.parse_destructure_name_list();
        let mut ty = None;
        let used_infer = if self.consume_if(TokenKind::InferAssign) {
            true
        } else if self.consume_if(TokenKind::Colon) {
            ty = Some(self.parse_type_ref());
            false
        } else {
            self.push_error(
                "expected ':=' or ':' after tuple destructuring pattern",
                self.current_span_or_eof(),
            );
            return self.invalid_node(start);
        };

        if !used_infer && !self.consume_if(TokenKind::Assign) {
            self.push_error(
                "expected '=' in tuple destructuring declaration",
                self.current_span_or_eof(),
            );
            return self.invalid_node(start);
        }

        let value = self.parse_expression();
        let span = merge_span(start, self.node_span(value));
        self.insert_node(AstNode::TupleDestructureDecl {
            names,
            ty,
            value,
            span,
        })
    }

    fn parse_type_name(&mut self) -> ArenaId {
        self.parse_type_ref()
    }

    pub(super) fn parse_type_ref(&mut self) -> ArenaId {
        if self.current().is_some_and(|t| t.kind == TokenKind::Fn) {
            return self.parse_fn_type_ref();
        }
        if self
            .current()
            .is_some_and(|t| t.kind == TokenKind::SelfType)
        {
            let span = self.current_span_or_eof();
            self.bump();
            return self.insert_node(AstNode::SelfTypeRef { span });
        }
        if self
            .current()
            .is_some_and(|t| t.kind == TokenKind::LeftParen)
        {
            return self.parse_tuple_type_ref();
        }
        if self
            .current()
            .is_some_and(|t| t.kind == TokenKind::LeftBracket)
        {
            return self.parse_array_type_ref();
        }
        if self
            .current()
            .is_some_and(|t| t.kind == TokenKind::Identifier && t.lexeme == "map")
        {
            return self.parse_map_type_ref();
        }
        if self
            .current()
            .is_some_and(|t| t.kind == TokenKind::Identifier && t.lexeme == "set")
        {
            return self.parse_set_type_ref();
        }
        let token = match self.current() {
            Some(token) if token.kind == TokenKind::TypeName => token.clone(),
            Some(token) if token.kind == TokenKind::Identifier => token.clone(),
            Some(token) if token.kind == TokenKind::Null => token.clone(),
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

    fn parse_fn_type_ref(&mut self) -> ArenaId {
        let start = self.current_span_or_eof();
        self.bump();
        if !self.consume_if(TokenKind::LeftParen) {
            let span = self.current_span_or_eof();
            self.push_error("expected '(' after 'fn' in function type", span);
            return self.invalid_node(span);
        }
        let mut param_types = Vec::new();
        if !self
            .current()
            .is_some_and(|t| t.kind == TokenKind::RightParen)
        {
            loop {
                let ty_id = self.parse_type_ref();
                let ty_name = self.type_name_from_node(ty_id);
                param_types.push(ty_name);
                if self.consume_if(TokenKind::Comma) {
                    continue;
                }
                break;
            }
        }
        if !self.consume_if(TokenKind::RightParen) {
            self.push_error("expected ')' in function type", self.current_span_or_eof());
        }
        if !self.consume_if(TokenKind::Arrow) {
            self.push_error("expected '->' in function type", self.current_span_or_eof());
        }
        let ret_ty_id = self.parse_type_ref();
        let ret_name = self.type_name_from_node(ret_ty_id);
        let name = format!("fn({}) -> {}", param_types.join(", "), ret_name);
        let span = merge_span(start, self.node_span(ret_ty_id));
        self.insert_node(AstNode::TypeName { name, span })
    }

    fn parse_tuple_type_ref(&mut self) -> ArenaId {
        let start = self.current_span_or_eof();
        self.bump();
        let mut item_types = Vec::new();
        let first = self.parse_type_ref();
        item_types.push(self.type_name_from_node(first));
        if !self.consume_if(TokenKind::Comma) {
            self.push_error(
                "tuple type must contain at least two elements",
                self.current_span_or_eof(),
            );
            return self.invalid_node(start);
        }
        let second = self.parse_type_ref();
        item_types.push(self.type_name_from_node(second));
        while self.consume_if(TokenKind::Comma) {
            let next = self.parse_type_ref();
            item_types.push(self.type_name_from_node(next));
        }
        if !self.consume_if(TokenKind::RightParen) {
            self.push_error(
                "expected ')' to close tuple type",
                self.current_span_or_eof(),
            );
        }
        let name = format!("({})", item_types.join(", "));
        let span = merge_span(start, self.previous_span_or(start));
        self.insert_node(AstNode::TypeName { name, span })
    }

    fn parse_array_type_ref(&mut self) -> ArenaId {
        let start = self.current_span_or_eof();
        self.bump();
        let item = self.parse_type_ref();
        if !self.consume_if(TokenKind::RightBracket) {
            self.push_error(
                "expected ']' to close array type",
                self.current_span_or_eof(),
            );
        }
        let item_name = self.type_name_from_node(item);
        let name = format!("[{}]", item_name);
        let span = merge_span(start, self.previous_span_or(start));
        self.insert_node(AstNode::TypeName { name, span })
    }

    fn parse_map_type_ref(&mut self) -> ArenaId {
        let start = self.current_span_or_eof();
        self.bump();
        if !self.consume_if(TokenKind::LeftBracket) {
            self.push_error(
                "expected '[' after 'map' in map type",
                self.current_span_or_eof(),
            );
            return self.invalid_node(start);
        }
        let key = self.parse_type_ref();
        if !self.consume_if(TokenKind::RightBracket) {
            self.push_error(
                "expected ']' after map key type",
                self.current_span_or_eof(),
            );
        }
        let value = self.parse_type_ref();
        let name = format!(
            "map[{}]{}",
            self.type_name_from_node(key),
            self.type_name_from_node(value)
        );
        let span = merge_span(start, self.node_span(value));
        self.insert_node(AstNode::TypeName { name, span })
    }

    fn parse_set_type_ref(&mut self) -> ArenaId {
        let start = self.current_span_or_eof();
        self.bump();
        if !self.consume_if(TokenKind::LeftBracket) {
            self.push_error(
                "expected '[' after 'set' in set type",
                self.current_span_or_eof(),
            );
            return self.invalid_node(start);
        }
        let item = self.parse_type_ref();
        if !self.consume_if(TokenKind::RightBracket) {
            self.push_error(
                "expected ']' after set item type",
                self.current_span_or_eof(),
            );
        }
        let name = format!("set[{}]", self.type_name_from_node(item));
        let span = merge_span(start, self.previous_span_or(start));
        self.insert_node(AstNode::TypeName { name, span })
    }

    pub(super) fn type_name_from_node(&self, id: ArenaId) -> String {
        match self.arena.get(id) {
            Some(AstNode::TypeName { name, .. }) => name.clone(),
            _ => "<invalid>".to_string(),
        }
    }

    pub(super) fn identifier_name_from_node(&self, id: ArenaId) -> String {
        match self.arena.get(id) {
            Some(AstNode::Identifier { name, .. }) => name.clone(),
            _ => "<invalid>".to_string(),
        }
    }

    fn parse_fn_decl(&mut self) -> ArenaId {
        let fn_span = self.current_span_or_eof();
        self.bump();
        let name_token = match self.current() {
            Some(token) if token.kind == TokenKind::Identifier => token.clone(),
            _ => {
                let span = self.current_span_or_eof();
                self.push_error("expected function name after 'fn'", span);
                return self.invalid_node(span);
            }
        };
        self.bump();
        let name = self.insert_node(AstNode::Identifier {
            name: name_token.lexeme,
            span: name_token.span,
        });

        if !self.consume_if(TokenKind::LeftParen) {
            let span = self.current_span_or_eof();
            self.push_error("expected '(' after function name", span);
            return self.invalid_node(span);
        }
        let (params, param_defaults) = self.parse_fn_params(true);
        if !self.consume_if(TokenKind::RightParen) {
            self.push_error(
                "expected ')' after function parameters",
                self.current_span_or_eof(),
            );
            return self.invalid_node(name_token.span);
        }
        if !self.consume_if(TokenKind::Arrow) {
            self.push_error(
                "expected '->' after function parameters",
                self.current_span_or_eof(),
            );
            return self.invalid_node(name_token.span);
        }
        let return_ty = self.parse_type_ref();
        let body = self.parse_if_branch_block("expected '{' after function signature");
        let span = merge_span(fn_span, self.node_span(body));
        self.insert_node(AstNode::FnDecl {
            name,
            params,
            param_defaults,
            return_ty,
            body,
            span,
        })
    }

    fn parse_struct_decl(&mut self) -> ArenaId {
        let start = self.current_span_or_eof();
        self.bump();
        let name = self.parse_required_identifier("expected struct name after 'struct'");
        if !self.consume_if(TokenKind::LeftBrace) {
            self.push_error("expected '{' after struct name", self.current_span_or_eof());
            return self.invalid_node(start);
        }
        let mut fields = Vec::new();
        while self.current().is_some()
            && !self
                .current()
                .is_some_and(|t| t.kind == TokenKind::RightBrace)
        {
            let field_name = self.parse_required_identifier("expected field name in struct");
            if !self.consume_if(TokenKind::Colon) {
                self.push_error("expected ':' after field name", self.current_span_or_eof());
                return self.invalid_node(start);
            }
            let field_ty = self.parse_type_ref();
            fields.push((field_name, field_ty));
            if self.consume_if(TokenKind::Comma) {
                continue;
            }
            break;
        }
        if !self.consume_if(TokenKind::RightBrace) {
            self.push_error(
                "expected '}' after struct fields",
                self.current_span_or_eof(),
            );
        }
        let span = merge_span(start, self.previous_span_or(start));
        self.insert_node(AstNode::StructDecl { name, fields, span })
    }

    fn parse_trait_decl(&mut self) -> ArenaId {
        let start = self.current_span_or_eof();
        self.bump();
        let name = self.parse_required_identifier("expected trait name after 'trait'");
        if !self.consume_if(TokenKind::LeftBrace) {
            self.push_error("expected '{' after trait name", self.current_span_or_eof());
            return self.invalid_node(start);
        }
        let mut methods = Vec::new();
        while self.current().is_some()
            && !self
                .current()
                .is_some_and(|t| t.kind == TokenKind::RightBrace)
        {
            if !self.current().is_some_and(|t| t.kind == TokenKind::Fn) {
                self.push_error(
                    "expected 'fn' in trait declaration",
                    self.current_span_or_eof(),
                );
                self.bump();
                continue;
            }
            let method = self.parse_trait_method_signature();
            methods.push(method);
            self.consume_if(TokenKind::Semicolon);
        }
        if !self.consume_if(TokenKind::RightBrace) {
            self.push_error("expected '}' after trait body", self.current_span_or_eof());
        }
        let span = merge_span(start, self.previous_span_or(start));
        self.insert_node(AstNode::TraitDecl {
            name,
            methods,
            span,
        })
    }

    fn parse_impl_block(&mut self) -> ArenaId {
        let start = self.current_span_or_eof();
        self.bump();
        let first_ty = self.parse_type_ref();
        let (trait_ty, target_ty) = if self.consume_if(TokenKind::For) {
            (Some(first_ty), self.parse_type_ref())
        } else {
            (None, first_ty)
        };
        if !self.consume_if(TokenKind::LeftBrace) {
            self.push_error("expected '{' after impl header", self.current_span_or_eof());
            return self.invalid_node(start);
        }
        let mut methods = Vec::new();
        while self.current().is_some()
            && !self
                .current()
                .is_some_and(|t| t.kind == TokenKind::RightBrace)
        {
            if !self.current().is_some_and(|t| t.kind == TokenKind::Fn) {
                self.push_error("expected 'fn' in impl block", self.current_span_or_eof());
                self.bump();
                continue;
            }
            methods.push(self.parse_fn_decl());
            self.consume_if(TokenKind::Semicolon);
        }
        if !self.consume_if(TokenKind::RightBrace) {
            self.push_error("expected '}' after impl body", self.current_span_or_eof());
        }
        let span = merge_span(start, self.previous_span_or(start));
        self.insert_node(AstNode::ImplBlock {
            target_ty,
            trait_ty,
            methods,
            span,
        })
    }

    fn parse_trait_method_signature(&mut self) -> ArenaId {
        let fn_span = self.current_span_or_eof();
        self.bump();
        let name = self.parse_required_identifier("expected method name after 'fn'");
        if !self.consume_if(TokenKind::LeftParen) {
            self.push_error("expected '(' after method name", self.current_span_or_eof());
            return self.invalid_node(fn_span);
        }
        let (params, param_defaults) = self.parse_fn_params(true);
        if !self.consume_if(TokenKind::RightParen) {
            self.push_error(
                "expected ')' after method parameters",
                self.current_span_or_eof(),
            );
        }
        if !self.consume_if(TokenKind::Arrow) {
            self.push_error(
                "expected '->' after method parameters",
                self.current_span_or_eof(),
            );
        }
        let return_ty = self.parse_type_ref();
        let empty_body = self.insert_node(AstNode::BlockStmt {
            statements: Vec::new(),
            span: self.previous_span_or(fn_span),
        });
        self.insert_node(AstNode::FnDecl {
            name,
            params,
            param_defaults,
            return_ty,
            body: empty_body,
            span: merge_span(fn_span, self.node_span(return_ty)),
        })
    }

    fn parse_fn_params(
        &mut self,
        allow_self: bool,
    ) -> (Vec<(ArenaId, ArenaId)>, Vec<Option<ArenaId>>) {
        let mut params = Vec::new();
        let mut defaults = Vec::new();
        let mut saw_optional = false;
        if self
            .current()
            .is_some_and(|t| t.kind == TokenKind::RightParen)
        {
            return (params, defaults);
        }
        loop {
            if allow_self && self.current().is_some_and(|t| t.kind == TokenKind::SelfKw) {
                let self_span = self.current_span_or_eof();
                self.bump();
                let self_name = self.insert_node(AstNode::Identifier {
                    name: "self".to_string(),
                    span: self_span,
                });
                let self_ty = self.insert_node(AstNode::SelfTypeRef { span: self_span });
                params.push((self_name, self_ty));
                defaults.push(None);
            } else {
                let param_name = self.parse_required_identifier("expected parameter name");
                if !self.consume_if(TokenKind::Colon) {
                    self.push_error(
                        "expected ':' after parameter name",
                        self.current_span_or_eof(),
                    );
                    break;
                }
                let param_ty = self.parse_type_ref();
                let default_value = if self.consume_if(TokenKind::Assign) {
                    saw_optional = true;
                    Some(self.parse_expression())
                } else {
                    if saw_optional {
                        self.push_error(
                            "optional parameters must be trailing (all required params must come first)",
                            self.current_span_or_eof(),
                        );
                    }
                    None
                };
                params.push((param_name, param_ty));
                defaults.push(default_value);
            }
            if self.consume_if(TokenKind::Comma) {
                continue;
            }
            break;
        }
        (params, defaults)
    }

    fn parse_required_identifier(&mut self, message: &str) -> ArenaId {
        let token = match self.current() {
            Some(token) if token.kind == TokenKind::Identifier => token.clone(),
            _ => {
                let span = self.current_span_or_eof();
                self.push_error(message, span);
                return self.invalid_node(span);
            }
        };
        self.bump();
        self.insert_node(AstNode::Identifier {
            name: token.lexeme,
            span: token.span,
        })
    }

    fn parse_assign_stmt(&mut self) -> ArenaId {
        let target_start = self.current_span_or_eof();
        let target = self.parse_assignment_target();

        if !self.consume_if(TokenKind::Assign) {
            let span = self.current_span_or_eof();
            self.push_error("expected '=' in assignment", span);
            self.synchronize();
            return self.invalid_node(target_start);
        }

        let value = self.parse_expression();
        let span = merge_span(self.node_span(target), self.node_span(value));
        self.insert_node(AstNode::AssignStmt {
            target,
            value,
            span,
        })
    }

    fn parse_assignment_target(&mut self) -> ArenaId {
        let ident = match self.current() {
            Some(token) if token.kind == TokenKind::Identifier => token.clone(),
            _ => {
                let span = self.current_span_or_eof();
                self.push_error("expected identifier in assignment", span);
                return self.invalid_node(span);
            }
        };
        self.bump();
        let mut target = self.insert_node(AstNode::Identifier {
            name: ident.lexeme,
            span: ident.span,
        });
        while self
            .current()
            .is_some_and(|t| matches!(t.kind, TokenKind::LeftBracket | TokenKind::Dot))
        {
            if self
                .current()
                .is_some_and(|t| t.kind == TokenKind::LeftBracket)
            {
                let open = self.current_span_or_eof();
                self.bump();
                if self
                    .current()
                    .is_some_and(|t| t.kind == TokenKind::RightBracket)
                {
                    self.push_error("expected index expression inside brackets", open);
                    return self.invalid_node(open);
                }
                let index = self.parse_expression();
                if !self.consume_if(TokenKind::RightBracket) {
                    self.push_error(
                        "expected ']' after index expression",
                        self.current_span_or_eof(),
                    );
                    return self.invalid_node(open);
                }
                let span = merge_span(self.node_span(target), self.node_span(index));
                target = self.insert_node(AstNode::ArrayAccessExpr {
                    base: target,
                    index,
                    span,
                });
                continue;
            }

            let dot_span = self.current_span_or_eof();
            self.bump();
            let field_token = match self.current() {
                Some(token) if token.kind == TokenKind::Identifier => token.clone(),
                _ => {
                    self.push_error("expected field name after '.' in assignment", dot_span);
                    return self.invalid_node(dot_span);
                }
            };
            self.bump();
            let span = merge_span(self.node_span(target), field_token.span);
            target = self.insert_node(AstNode::FieldAccessExpr {
                base: target,
                field: field_token.lexeme,
                span,
            });
        }
        target
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

    fn is_tuple_destructure_decl_start(&self) -> bool {
        let mut idx = 0usize;
        let mut has_parens = false;
        if self.peek_kind(idx) == Some(TokenKind::LeftParen) {
            has_parens = true;
            idx += 1;
        }
        if self.peek_kind(idx) != Some(TokenKind::Identifier) {
            return false;
        }
        idx += 1;
        if self.peek_kind(idx) != Some(TokenKind::Comma) {
            return false;
        }
        loop {
            if self.peek_kind(idx) != Some(TokenKind::Comma) {
                break;
            }
            idx += 1;
            if self.peek_kind(idx) != Some(TokenKind::Identifier) {
                return false;
            }
            idx += 1;
        }
        if has_parens {
            if self.peek_kind(idx) != Some(TokenKind::RightParen) {
                return false;
            }
            idx += 1;
        }
        matches!(
            self.peek_kind(idx),
            Some(TokenKind::InferAssign | TokenKind::Colon)
        )
    }

    fn parse_destructure_name_list(&mut self) -> Vec<ArenaId> {
        let mut names = Vec::new();
        let has_parens = self.consume_if(TokenKind::LeftParen);
        loop {
            let token = match self.current() {
                Some(token) if token.kind == TokenKind::Identifier => token.clone(),
                _ => {
                    self.push_error(
                        "expected identifier in tuple destructuring pattern",
                        self.current_span_or_eof(),
                    );
                    break;
                }
            };
            self.bump();
            names.push(self.insert_node(AstNode::Identifier {
                name: token.lexeme,
                span: token.span,
            }));
            if !self.consume_if(TokenKind::Comma) {
                break;
            }
            if has_parens
                && self
                    .current()
                    .is_some_and(|t| t.kind == TokenKind::RightParen)
            {
                break;
            }
        }
        if names.len() < 2 {
            self.push_error(
                "tuple destructuring requires at least two names",
                self.current_span_or_eof(),
            );
        }
        if has_parens && !self.consume_if(TokenKind::RightParen) {
            self.push_error(
                "expected ')' in tuple destructuring declaration",
                self.current_span_or_eof(),
            );
        }
        names
    }

    fn is_assignment_start(&self) -> bool {
        if self.peek_kind(0) != Some(TokenKind::Identifier) {
            return false;
        }
        let mut idx = 1usize;
        loop {
            if self.peek_kind(idx) == Some(TokenKind::Assign) {
                return true;
            }
            match self.peek_kind(idx) {
                Some(TokenKind::LeftBracket) => {
                    idx += 1;
                    let mut depth = 1usize;
                    while depth > 0 {
                        match self.peek_kind(idx) {
                            Some(TokenKind::LeftBracket) => depth += 1,
                            Some(TokenKind::RightBracket) => depth = depth.saturating_sub(1),
                            Some(_) => {}
                            None => return false,
                        }
                        idx += 1;
                    }
                }
                Some(TokenKind::Dot) => {
                    idx += 1;
                    if self.peek_kind(idx) != Some(TokenKind::Identifier) {
                        return false;
                    }
                    idx += 1;
                }
                _ => return false,
            }
        }
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
        self.tokens
            .get(self.pos + lookahead)
            .map(|t| t.kind.clone())
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
            self.tokens
                .get(self.pos - 1)
                .map(|t| t.span)
                .unwrap_or(fallback)
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
        self.current()
            .map(|t| t.span)
            .unwrap_or_else(|| self.eof_span())
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
        assert!(matches!(
            ast.get(statements[0]),
            Some(AstNode::LetDecl { .. })
        ));
        assert!(matches!(
            ast.get(statements[1]),
            Some(AstNode::ExprStmt { .. })
        ));
    }

    #[test]
    fn reports_missing_block_closing_brace() {
        let source = "{ x := 1";
        let lex_out = lex(FileId::from_u32(8), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(8), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("expected '}'"))
        );
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
        } = ast.get(*value).expect("logical or root")
        else {
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
        } = ast.get(*value).expect("outer pow")
        else {
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
        } = ast.get(root).expect("if stmt")
        else {
            panic!("expected if statement");
        };
        assert!(matches!(
            ast.get(*then_branch),
            Some(AstNode::BlockStmt { .. })
        ));
        let else_id = else_branch.expect("else branch");
        assert!(matches!(
            ast.get(else_id),
            Some(AstNode::IfStmt { .. }) | Some(AstNode::BlockStmt { .. })
        ));
    }

    #[test]
    fn parses_if_identifier_comparison_condition_with_block() {
        let source = "x := 1; y := 2; if x == y { print(\"eq\") }";
        let lex_out = lex(FileId::from_u32(311), source);
        let (ast, diagnostics) = parse(FileId::from_u32(311), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        assert!(matches!(ast.get(ast.roots[2]), Some(AstNode::IfStmt { .. })));
    }

    #[test]
    fn reports_missing_if_block() {
        let source = "if true x := 1";
        let lex_out = lex(FileId::from_u32(14), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(14), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("expected '{' after if condition"))
        );
    }

    #[test]
    fn reports_unexpected_else_without_if() {
        let source = "else { x := 1 }";
        let lex_out = lex(FileId::from_u32(15), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(15), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
        assert!(diagnostics.iter().any(|d| {
            d.message
                .contains("unexpected 'else' without matching 'if'")
        }));
    }

    #[test]
    fn parses_while_break_continue_statements() {
        let source = "while 1 { if true { continue }; break }";
        let lex_out = lex(FileId::from_u32(16), source);
        let (ast, diagnostics) = parse(FileId::from_u32(16), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        assert!(matches!(
            ast.get(ast.roots[0]),
            Some(AstNode::WhileStmt { .. })
        ));
    }

    #[test]
    fn reports_missing_while_block() {
        let source = "while true x := 1";
        let lex_out = lex(FileId::from_u32(17), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(17), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("expected '{' after while condition"))
        );
    }

    #[test]
    fn reports_break_outside_loop() {
        let source = "break";
        let lex_out = lex(FileId::from_u32(18), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(18), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("break used outside of loop"))
        );
    }

    #[test]
    fn reports_continue_outside_loop() {
        let source = "continue";
        let lex_out = lex(FileId::from_u32(19), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(19), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("continue used outside of loop"))
        );
    }

    #[test]
    fn parses_for_stmt_with_all_fields() {
        let source = "for i: i32 = 0; i < 3; i++ { print(i) }";
        let lex_out = lex(FileId::from_u32(43), source);
        let (ast, diagnostics) = parse(FileId::from_u32(43), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        assert!(matches!(
            ast.get(ast.roots[0]),
            Some(AstNode::ForStmt { .. })
        ));
    }

    #[test]
    fn parses_for_stmt_with_empty_fields() {
        let source = "for ; ; { break }";
        let lex_out = lex(FileId::from_u32(44), source);
        let (ast, diagnostics) = parse(FileId::from_u32(44), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let Some(AstNode::ForStmt {
            init,
            condition,
            step,
            ..
        }) = ast.get(ast.roots[0])
        else {
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
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("for init must use typed declaration"))
        );
    }

    #[test]
    fn parses_function_declaration_and_return() {
        let source = "fn add(a: i32, b: i32) -> i32 { return a + b }";
        let lex_out = lex(FileId::from_u32(47), source);
        let (ast, diagnostics) = parse(FileId::from_u32(47), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        assert!(matches!(
            ast.get(ast.roots[0]),
            Some(AstNode::FnDecl { .. })
        ));
    }

    #[test]
    fn parses_function_type_annotation() {
        let source = "f: fn(i32, i32) -> i32 = add";
        let lex_out = lex(FileId::from_u32(48), source);
        let (ast, diagnostics) = parse(FileId::from_u32(48), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let Some(AstNode::LetDecl {
            ty: Some(ty_id), ..
        }) = ast.get(ast.roots[0])
        else {
            panic!("expected typed let");
        };
        assert!(
            matches!(ast.get(*ty_id), Some(AstNode::TypeName { name, .. }) if name.starts_with("fn("))
        );
    }

    #[test]
    fn parses_nested_function_declaration() {
        let source = "fn outer(a: i32) -> fn(i32) -> i32 { fn inner(x: i32) -> i32 { return a + x }; return inner }";
        let lex_out = lex(FileId::from_u32(51), source);
        let (ast, diagnostics) = parse(FileId::from_u32(51), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let Some(AstNode::FnDecl { body, .. }) = ast.get(ast.roots[0]) else {
            panic!("expected outer fn");
        };
        let Some(AstNode::BlockStmt { statements, .. }) = ast.get(*body) else {
            panic!("expected block");
        };
        assert!(
            statements
                .iter()
                .any(|id| matches!(ast.get(*id), Some(AstNode::FnDecl { .. })))
        );
    }

    #[test]
    fn parses_bare_return_statement() {
        let source = "fn noop() -> unit { return }";
        let lex_out = lex(FileId::from_u32(52), source);
        let (ast, diagnostics) = parse(FileId::from_u32(52), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let Some(AstNode::FnDecl { body, .. }) = ast.get(ast.roots[0]) else {
            panic!("expected fn decl");
        };
        let Some(AstNode::BlockStmt { statements, .. }) = ast.get(*body) else {
            panic!("expected block");
        };
        assert!(statements.iter().any(|id| matches!(
            ast.get(*id),
            Some(AstNode::ReturnStmt { values, .. }) if values.is_empty()
        )));
    }

    #[test]
    fn parses_multi_return_values() {
        let source = "fn pair(a: i32, b: i32) -> (i32, i32) { return a, b }";
        let lex_out = lex(FileId::from_u32(58), source);
        let (ast, diagnostics) = parse(FileId::from_u32(58), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let Some(AstNode::FnDecl { body, .. }) = ast.get(ast.roots[0]) else {
            panic!("expected fn");
        };
        let Some(AstNode::BlockStmt { statements, .. }) = ast.get(*body) else {
            panic!("expected block");
        };
        assert!(statements.iter().any(|id| matches!(
            ast.get(*id),
            Some(AstNode::ReturnStmt { values, .. }) if values.len() == 2
        )));
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
        assert!(
            ast.roots
                .iter()
                .all(|id| matches!(ast.get(*id), Some(AstNode::CompoundAssignStmt { .. })))
        );
    }

    #[test]
    fn compound_assign_requires_identifier() {
        let source = "1 += 2";
        let lex_out = lex(FileId::from_u32(42), source);
        let (_ast, diagnostics) = parse(FileId::from_u32(42), source.len() as u32, lex_out.tokens);
        assert!(diagnostics.has_errors());
    }

    #[test]
    fn parses_tuple_literal_and_access_forms() {
        let source = r#"t := (1, "a"); x := t.0; y := t[1]"#;
        let lex_out = lex(FileId::from_u32(53), source);
        let (ast, diagnostics) = parse(FileId::from_u32(53), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let AstNode::LetDecl {
            value: tuple_value, ..
        } = ast.get(ast.roots[0]).expect("let t")
        else {
            panic!("expected tuple let");
        };
        assert!(
            matches!(ast.get(*tuple_value), Some(AstNode::TupleLiteral { items, .. }) if items.len() == 2)
        );
        let AstNode::LetDecl { value: x_value, .. } = ast.get(ast.roots[1]).expect("let x") else {
            panic!("expected let x");
        };
        assert!(matches!(
            ast.get(*x_value),
            Some(AstNode::TupleAccess { index: 0, .. })
        ));
        let AstNode::LetDecl { value: y_value, .. } = ast.get(ast.roots[2]).expect("let y") else {
            panic!("expected let y");
        };
        assert!(matches!(
            ast.get(*y_value),
            Some(AstNode::ArrayAccessExpr { .. })
        ));
    }

    #[test]
    fn parses_array_type_literal_and_index_assignment() {
        let source = "arr: [i32] = [1, 2, 3]; arr[1] = 9; x := arr[1]";
        let lex_out = lex(FileId::from_u32(531), source);
        let (ast, diagnostics) = parse(FileId::from_u32(531), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        assert!(matches!(
            ast.get(ast.roots[0]),
            Some(AstNode::LetDecl { ty: Some(_), value, .. })
                if matches!(ast.get(*value), Some(AstNode::ArrayLiteral { items, .. }) if items.len() == 3)
        ));
        assert!(matches!(
            ast.get(ast.roots[1]),
            Some(AstNode::AssignStmt { target, .. })
                if matches!(ast.get(*target), Some(AstNode::ArrayAccessExpr { .. }))
        ));
        assert!(matches!(
            ast.get(ast.roots[2]),
            Some(AstNode::LetDecl { value, .. })
                if matches!(ast.get(*value), Some(AstNode::ArrayAccessExpr { .. }))
        ));
    }

    #[test]
    fn parses_struct_field_assignment() {
        let source = "struct VMState { debug: bool }; vm: VMState = VMState { debug: true }; vm.debug = false";
        let lex_out = lex(FileId::from_u32(534), source);
        let (ast, diagnostics) = parse(FileId::from_u32(534), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        assert!(
            matches!(ast.get(ast.roots[2]), Some(AstNode::AssignStmt { target, .. })
            if matches!(ast.get(*target), Some(AstNode::FieldAccessExpr { field, .. }) if field == "debug"))
        );
    }

    #[test]
    fn parses_array_spread_and_optional_param_defaults() {
        let source =
            "fn f(a: i32, b: i32 = 2) -> i32 { return a + b }\narr: [i32] = [1, ...[2, 3]]";
        let lex_out = lex(FileId::from_u32(532), source);
        let (ast, diagnostics) = parse(FileId::from_u32(532), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let Some(AstNode::FnDecl { param_defaults, .. }) = ast.get(ast.roots[0]) else {
            panic!("expected fn");
        };
        assert_eq!(param_defaults.len(), 2);
        assert!(param_defaults[0].is_none());
        assert!(param_defaults[1].is_some());
        let Some(AstNode::LetDecl { value, .. }) = ast.get(ast.roots[1]) else {
            panic!("expected let");
        };
        let Some(AstNode::ArrayLiteral { items, .. }) = ast.get(*value) else {
            panic!("expected array literal");
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(items[1], crate::ast::ArrayItem::SpreadExpr(_)));
    }

    #[test]
    fn parses_range_and_for_in_forms() {
        let source =
            "a := 0..10; b := 0..=10; for i: i32 in a { print(i) }; for j in b { print(j) }";
        let lex_out = lex(FileId::from_u32(533), source);
        let (ast, diagnostics) = parse(FileId::from_u32(533), source.len() as u32, lex_out.tokens);
        assert!(
            !diagnostics.has_errors(),
            "{:?}",
            diagnostics
                .iter()
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
        );
        assert!(matches!(
            ast.get(ast.roots[0]),
            Some(AstNode::LetDecl { value, .. }) if matches!(ast.get(*value), Some(AstNode::RangeExpr { inclusive: false, .. }))
        ));
        assert!(matches!(
            ast.get(ast.roots[1]),
            Some(AstNode::LetDecl { value, .. }) if matches!(ast.get(*value), Some(AstNode::RangeExpr { inclusive: true, .. }))
        ));
        assert!(matches!(
            ast.get(ast.roots[2]),
            Some(AstNode::ForInStmt { ty: Some(_), .. })
        ));
        assert!(matches!(
            ast.get(ast.roots[3]),
            Some(AstNode::ForInStmt { ty: None, .. })
        ));
    }

    #[test]
    fn parses_tuple_destructuring_declarations() {
        let source = "(a, b) := pair; c, d: (i32, i32) = pair";
        let lex_out = lex(FileId::from_u32(54), source);
        let (ast, diagnostics) = parse(FileId::from_u32(54), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        assert!(matches!(
            ast.get(ast.roots[0]),
            Some(AstNode::TupleDestructureDecl { names, ty: None, .. }) if names.len() == 2
        ));
        assert!(matches!(
            ast.get(ast.roots[1]),
            Some(AstNode::TupleDestructureDecl { names, ty: Some(_), .. }) if names.len() == 2
        ));
    }

    #[test]
    fn parses_null_literal() {
        let source = "x: i32 = null";
        let lex_out = lex(FileId::from_u32(55), source);
        let (ast, diagnostics) = parse(FileId::from_u32(55), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let Some(AstNode::LetDecl { value, .. }) = ast.get(ast.roots[0]) else {
            panic!("expected let");
        };
        assert!(matches!(ast.get(*value), Some(AstNode::NullLiteral { .. })));
    }

    #[test]
    fn parses_struct_trait_impl_and_method_calls() {
        let source = r#"
            struct Point { x: i32, y: i32 }
            trait Show { fn show(self) -> str }
            impl Point {
                fn sum(self) -> i32 { return self.x + self.y }
                fn origin() -> Point { return Point { x: 0, y: 0 } }
            }
            impl Show for Point {
                fn show(self) -> str { return "ok" }
            }
            p: Point = Point { x: 1, y: 2 }
            s := p.sum()
            o := Point::origin()
        "#;
        let lex_out = lex(FileId::from_u32(57), source);
        let (ast, diagnostics) = parse(FileId::from_u32(57), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        assert!(
            ast.roots
                .iter()
                .any(|id| matches!(ast.get(*id), Some(AstNode::StructDecl { .. })))
        );
        assert!(
            ast.roots
                .iter()
                .any(|id| matches!(ast.get(*id), Some(AstNode::TraitDecl { .. })))
        );
        assert!(
            ast.roots
                .iter()
                .any(|id| matches!(ast.get(*id), Some(AstNode::ImplBlock { .. })))
        );
    }

    #[test]
    fn parses_propagate_and_try_catch_expressions() {
        let source = r#"
            fn div(a: i32, b: i32) -> (i32, err) { return a / b, null }
            fn f() -> (i32, err) { x := div(4, 2)?; return x, null }
            y := try div(1, 0) catch(e: err) { print(e.message); return 0 }
        "#;
        let lex_out = lex(FileId::from_u32(59), source);
        let (ast, diagnostics) = parse(FileId::from_u32(59), source.len() as u32, lex_out.tokens);
        assert!(!diagnostics.has_errors());
        let Some(AstNode::FnDecl { body, .. }) = ast.get(ast.roots[1]) else {
            panic!("expected second fn");
        };
        let Some(AstNode::BlockStmt { statements, .. }) = ast.get(*body) else {
            panic!("expected block");
        };
        let Some(AstNode::LetDecl { value, .. }) = ast.get(statements[0]) else {
            panic!("expected let");
        };
        assert!(matches!(
            ast.get(*value),
            Some(AstNode::PropagateExpr { .. })
        ));
        let Some(AstNode::LetDecl { value, .. }) = ast.get(ast.roots[2]) else {
            panic!("expected let");
        };
        assert!(matches!(
            ast.get(*value),
            Some(AstNode::TryCatchExpr { .. })
        ));
    }
}
