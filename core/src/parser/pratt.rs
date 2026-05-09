use crate::{
    ast::{ArrayItem, AstNode, BinaryOp, IncDecOp, IncDecPosition, UnaryOp},
    lexer::TokenKind,
};
use foundation::ids::ArenaId;

use super::parser::Parser;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Precedence {
    Lowest,
    Range,
    LogicalOr,
    LogicalAnd,
    BitOr,
    BitXor,
    BitAnd,
    Equality,
    Comparison,
    Shift,
    Sum,
    Product,
    Power,
    Highest,
}

impl Parser {
    pub(super) fn parse_expression(&mut self) -> ArenaId {
        self.parse_expression_bp(Precedence::Lowest)
    }

    fn parse_expression_bp(&mut self, min_prec: Precedence) -> ArenaId {
        let mut left = self.parse_prefix();

        loop {
            if self
                .current()
                .is_some_and(|t| t.kind == TokenKind::LeftParen)
                && Precedence::Highest >= min_prec
            {
                left = self.parse_call_suffix(left);
                continue;
            }
            if self.current().is_some_and(|t| t.kind == TokenKind::Dot)
                && Precedence::Highest >= min_prec
            {
                left = self.parse_dot_suffix(left);
                continue;
            }
            if self
                .current()
                .is_some_and(|t| t.kind == TokenKind::LeftBracket)
                && Precedence::Highest >= min_prec
            {
                left = self.parse_array_access_bracket_suffix(left);
                continue;
            }

            if self
                .current()
                .is_some_and(|t| t.kind == TokenKind::PlusPlus || t.kind == TokenKind::MinusMinus)
                && Precedence::Highest >= min_prec
            {
                let token = self.current().expect("checked above").clone();
                self.bump();
                let op = if token.kind == TokenKind::PlusPlus {
                    IncDecOp::Increment
                } else {
                    IncDecOp::Decrement
                };
                let span = merge_pair(self.node_span(left), token.span);
                left = self.insert_node(AstNode::IncDecExpr {
                    target: left,
                    op,
                    position: IncDecPosition::Postfix,
                    span,
                });
                continue;
            }
            if self
                .current()
                .is_some_and(|t| t.kind == TokenKind::Question)
                && Precedence::Highest >= min_prec
            {
                let token = self.current().expect("checked above").clone();
                self.bump();
                let span = merge_pair(self.node_span(left), token.span);
                left = self.insert_node(AstNode::PropagateExpr { expr: left, span });
                continue;
            }
            if self.current().is_some_and(|t| {
                t.kind == TokenKind::DoubleDot || t.kind == TokenKind::DoubleDotEqual
            }) && Precedence::Range >= min_prec
            {
                let token = self.current().expect("checked above").clone();
                let inclusive = token.kind == TokenKind::DoubleDotEqual;
                self.bump();
                let right = self.parse_expression_bp(next_precedence(Precedence::Range));
                let span = self.merge_spans(left, right);
                left = self.insert_node(AstNode::RangeExpr {
                    start: left,
                    end: right,
                    inclusive,
                    span,
                });
                continue;
            }

            if let Some((op, prec, right_assoc)) = self.current_binary_op() {
                if prec < min_prec {
                    break;
                }

                self.bump();
                let right = if right_assoc {
                    self.parse_expression_bp(prec)
                } else {
                    self.parse_expression_bp(next_precedence(prec))
                };
                let span = self.merge_spans(left, right);
                left = self.insert_node(AstNode::BinaryExpr {
                    op,
                    left,
                    right,
                    span,
                });
                continue;
            }

            break;
        }

        left
    }

    fn parse_prefix(&mut self) -> ArenaId {
        let token = match self.current() {
            Some(token) => token.clone(),
            None => {
                let span = self.eof_span();
                self.push_error("expected expression", span);
                return self.invalid_node(span);
            }
        };

        match token.kind {
            TokenKind::Identifier => {
                self.bump();
                let ident = self.insert_node(AstNode::Identifier {
                    name: token.lexeme,
                    span: token.span,
                });
                if self.identifier_name_from_node(ident) == "set"
                    && self
                        .current()
                        .is_some_and(|t| t.kind == TokenKind::LeftBrace)
                {
                    return self.parse_set_literal_suffix(token.span);
                }
                if self
                    .current()
                    .is_some_and(|t| t.kind == TokenKind::LeftBrace)
                    && self.looks_like_struct_literal_suffix()
                {
                    return self.parse_struct_literal_suffix(ident);
                }
                if self
                    .current()
                    .is_some_and(|t| t.kind == TokenKind::DoubleColon)
                {
                    return self.parse_static_method_call_suffix(ident);
                }
                ident
            }
            TokenKind::SelfKw => {
                self.bump();
                self.insert_node(AstNode::Identifier {
                    name: "self".to_string(),
                    span: token.span,
                })
            }
            TokenKind::TypeName => {
                self.bump();
                self.insert_node(AstNode::TypeName {
                    name: token.lexeme,
                    span: token.span,
                })
            }
            TokenKind::Integer => {
                self.bump();
                self.insert_node(AstNode::IntegerLiteral {
                    value: token.lexeme,
                    span: token.span,
                })
            }
            TokenKind::Float => {
                self.bump();
                self.insert_node(AstNode::FloatLiteral {
                    value: token.lexeme,
                    span: token.span,
                })
            }
            TokenKind::String => {
                self.bump();
                self.insert_node(AstNode::StringLiteral {
                    value: token.lexeme,
                    span: token.span,
                })
            }
            TokenKind::Char => {
                self.bump();
                let parsed = parse_char_lexeme(&token.lexeme);
                match parsed {
                    Some(ch) => self.insert_node(AstNode::CharLiteral {
                        value: ch,
                        span: token.span,
                    }),
                    None => {
                        self.push_error("invalid char literal", token.span);
                        self.invalid_node(token.span)
                    }
                }
            }
            TokenKind::Bool => {
                self.bump();
                self.insert_node(AstNode::BoolLiteral {
                    value: token.lexeme == "true",
                    span: token.span,
                })
            }
            TokenKind::Null => {
                self.bump();
                self.insert_node(AstNode::NullLiteral { span: token.span })
            }
            TokenKind::Try => self.parse_try_catch_expr(),
            TokenKind::Minus => {
                let op_span = token.span;
                self.bump();
                let operand = self.parse_expression_bp(Precedence::Highest);
                let span = merge_pair(op_span, self.node_span(operand));
                self.insert_node(AstNode::UnaryExpr {
                    op: UnaryOp::Neg,
                    operand,
                    span,
                })
            }
            TokenKind::PlusPlus | TokenKind::MinusMinus => {
                let op_span = token.span;
                let op = if token.kind == TokenKind::PlusPlus {
                    IncDecOp::Increment
                } else {
                    IncDecOp::Decrement
                };
                self.bump();
                let target = self.parse_expression_bp(Precedence::Highest);
                let span = merge_pair(op_span, self.node_span(target));
                self.insert_node(AstNode::IncDecExpr {
                    target,
                    op,
                    position: IncDecPosition::Prefix,
                    span,
                })
            }
            TokenKind::Bang => {
                let op_span = token.span;
                self.bump();
                let operand = self.parse_expression_bp(Precedence::Highest);
                let span = merge_pair(op_span, self.node_span(operand));
                self.insert_node(AstNode::UnaryExpr {
                    op: UnaryOp::Not,
                    operand,
                    span,
                })
            }
            TokenKind::Tilde => {
                let op_span = token.span;
                self.bump();
                let operand = self.parse_expression_bp(Precedence::Highest);
                let span = merge_pair(op_span, self.node_span(operand));
                self.insert_node(AstNode::UnaryExpr {
                    op: UnaryOp::BitNot,
                    operand,
                    span,
                })
            }
            TokenKind::LeftParen => {
                let open_span = token.span;
                self.bump();
                let first = self.parse_expression();
                if self.consume_if(TokenKind::Comma) {
                    let mut items = vec![first, self.parse_expression()];
                    while self.consume_if(TokenKind::Comma) {
                        items.push(self.parse_expression());
                    }
                    if !self.consume_if(TokenKind::RightParen) {
                        let err_span = merge_pair(
                            open_span,
                            self.node_span(*items.last().expect("non-empty")),
                        );
                        self.push_error("expected ')' after tuple literal", err_span);
                    }
                    let span = merge_pair(open_span, self.previous_span_or(open_span));
                    self.insert_node(AstNode::TupleLiteral { items, span })
                } else {
                    if !self.consume_if(TokenKind::RightParen) {
                        let err_span = merge_pair(open_span, self.node_span(first));
                        self.push_error("expected ')'", err_span);
                    }
                    first
                }
            }
            TokenKind::LeftBracket => self.parse_array_literal(),
            TokenKind::LeftBrace => self.parse_map_literal(),
            _ => {
                self.bump();
                self.push_error("expected expression", token.span);
                self.invalid_node(token.span)
            }
        }
    }

    fn parse_try_catch_expr(&mut self) -> ArenaId {
        let start = self.current_span_or_eof();
        self.bump();
        let try_expr = self.parse_expression_bp(Precedence::Lowest);
        if !self.consume_if(TokenKind::Catch) {
            self.push_error(
                "expected 'catch' after try expression",
                self.current_span_or_eof(),
            );
            return self.invalid_node(start);
        }
        if !self.consume_if(TokenKind::LeftParen) {
            self.push_error("expected '(' after 'catch'", self.current_span_or_eof());
            return self.invalid_node(start);
        }
        let err_name = match self.current() {
            Some(token) if token.kind == TokenKind::Identifier => {
                let token = token.clone();
                self.bump();
                self.insert_node(AstNode::Identifier {
                    name: token.lexeme,
                    span: token.span,
                })
            }
            _ => {
                self.push_error("expected catch binding name", self.current_span_or_eof());
                return self.invalid_node(start);
            }
        };
        if !self.consume_if(TokenKind::Colon) {
            self.push_error("expected ':' in catch binding", self.current_span_or_eof());
            return self.invalid_node(start);
        }
        let err_ty = self.parse_type_ref();
        if !self.consume_if(TokenKind::RightParen) {
            self.push_error(
                "expected ')' after catch binding",
                self.current_span_or_eof(),
            );
            return self.invalid_node(start);
        }
        if !self
            .current()
            .is_some_and(|t| t.kind == TokenKind::LeftBrace)
        {
            self.push_error(
                "expected '{' before catch block",
                self.current_span_or_eof(),
            );
            return self.invalid_node(start);
        }
        let catch_block = self.parse_block_stmt();
        let span = merge_pair(start, self.node_span(catch_block));
        self.insert_node(AstNode::TryCatchExpr {
            try_expr,
            err_name,
            err_ty,
            catch_block,
            span,
        })
    }

    fn current_binary_op(&self) -> Option<(BinaryOp, Precedence, bool)> {
        let token = self.current()?;
        match token.kind {
            TokenKind::OrOr => Some((BinaryOp::LogicalOr, Precedence::LogicalOr, false)),
            TokenKind::AndAnd => Some((BinaryOp::LogicalAnd, Precedence::LogicalAnd, false)),
            TokenKind::Pipe => Some((BinaryOp::BitOr, Precedence::BitOr, false)),
            TokenKind::Caret => Some((BinaryOp::BitXor, Precedence::BitXor, false)),
            TokenKind::Ampersand => Some((BinaryOp::BitAnd, Precedence::BitAnd, false)),
            TokenKind::EqualEqual => Some((BinaryOp::Equal, Precedence::Equality, false)),
            TokenKind::BangEqual => Some((BinaryOp::NotEqual, Precedence::Equality, false)),
            TokenKind::Less => Some((BinaryOp::Less, Precedence::Comparison, false)),
            TokenKind::LessEqual => Some((BinaryOp::LessEqual, Precedence::Comparison, false)),
            TokenKind::Greater => Some((BinaryOp::Greater, Precedence::Comparison, false)),
            TokenKind::GreaterEqual => {
                Some((BinaryOp::GreaterEqual, Precedence::Comparison, false))
            }
            TokenKind::ShiftLeft => Some((BinaryOp::ShiftLeft, Precedence::Shift, false)),
            TokenKind::ShiftRight => Some((BinaryOp::ShiftRight, Precedence::Shift, false)),
            TokenKind::Plus => Some((BinaryOp::Add, Precedence::Sum, false)),
            TokenKind::Minus => Some((BinaryOp::Subtract, Precedence::Sum, false)),
            TokenKind::Star => Some((BinaryOp::Multiply, Precedence::Product, false)),
            TokenKind::Slash => Some((BinaryOp::Divide, Precedence::Product, false)),
            TokenKind::Percent => Some((BinaryOp::Modulo, Precedence::Product, false)),
            TokenKind::DoubleStar => Some((BinaryOp::Power, Precedence::Power, true)),
            _ => None,
        }
    }

    fn parse_call_suffix(&mut self, callee: ArenaId) -> ArenaId {
        let open = self.current_span_or_eof();
        self.bump();
        let mut args = Vec::new();

        if self.consume_if(TokenKind::RightParen) {
            let span = merge_pair(self.node_span(callee), open);
            return self.insert_node(AstNode::CallExpr { callee, args, span });
        }

        loop {
            let arg = self.parse_expression();
            args.push(arg);

            if self.consume_if(TokenKind::Comma) {
                continue;
            }

            if self.consume_if(TokenKind::RightParen) {
                break;
            }

            self.push_error(
                "expected ',' or ')' in argument list",
                self.current_span_or_eof(),
            );
            break;
        }

        let end = args.last().map(|id| self.node_span(*id)).unwrap_or(open);
        let span = merge_pair(self.node_span(callee), end);
        self.insert_node(AstNode::CallExpr { callee, args, span })
    }

    fn parse_dot_suffix(&mut self, base: ArenaId) -> ArenaId {
        let dot_span = self.current_span_or_eof();
        self.bump();
        let Some(token) = self.current().cloned() else {
            self.push_error("expected tuple index after '.'", dot_span);
            return self.invalid_node(dot_span);
        };
        match token.kind {
            TokenKind::Integer => {
                self.bump();
                let Ok(index) = token.lexeme.parse::<usize>() else {
                    self.push_error("invalid tuple index literal", token.span);
                    return self.invalid_node(token.span);
                };
                let span = merge_pair(self.node_span(base), token.span);
                self.insert_node(AstNode::TupleAccess {
                    tuple: base,
                    index,
                    span,
                })
            }
            TokenKind::Identifier => {
                self.bump();
                if self
                    .current()
                    .is_some_and(|t| t.kind == TokenKind::LeftParen)
                {
                    return self.parse_method_call_suffix(base, token.lexeme, token.span);
                }
                let span = merge_pair(self.node_span(base), token.span);
                self.insert_node(AstNode::FieldAccessExpr {
                    base,
                    field: token.lexeme,
                    span,
                })
            }
            _ => {
                self.push_error("expected field name or tuple index after '.'", token.span);
                self.invalid_node(token.span)
            }
        }
    }

    fn parse_array_access_bracket_suffix(&mut self, base: ArenaId) -> ArenaId {
        let open_span = self.current_span_or_eof();
        self.bump();
        if self
            .current()
            .is_some_and(|t| t.kind == TokenKind::RightBracket)
        {
            self.push_error("expected index expression inside brackets", open_span);
            return self.invalid_node(open_span);
        }
        let index = self.parse_expression();
        if !self.consume_if(TokenKind::RightBracket) {
            self.push_error(
                "expected ']' after index expression",
                self.current_span_or_eof(),
            );
            return self.invalid_node(open_span);
        }
        let span = merge_pair(self.node_span(base), self.node_span(index));
        self.insert_node(AstNode::ArrayAccessExpr { base, index, span })
    }

    fn parse_array_literal(&mut self) -> ArenaId {
        let open_span = self.current_span_or_eof();
        self.bump();
        let mut items: Vec<ArrayItem> = Vec::new();
        if self.consume_if(TokenKind::RightBracket) {
            return self.insert_node(AstNode::ArrayLiteral {
                items,
                span: merge_pair(open_span, open_span),
            });
        }
        loop {
            if self.consume_if(TokenKind::Ellipsis) {
                let expr = self.parse_expression();
                items.push(ArrayItem::SpreadExpr(expr));
            } else {
                items.push(ArrayItem::Expr(self.parse_expression()));
            }
            if self.consume_if(TokenKind::Comma) {
                if self
                    .current()
                    .is_some_and(|t| t.kind == TokenKind::RightBracket)
                {
                    break;
                }
                continue;
            }
            break;
        }
        if !self.consume_if(TokenKind::RightBracket) {
            self.push_error(
                "expected ']' after array literal",
                self.current_span_or_eof(),
            );
        }
        let span = merge_pair(open_span, self.previous_span_or(open_span));
        self.insert_node(AstNode::ArrayLiteral { items, span })
    }

    fn parse_map_literal(&mut self) -> ArenaId {
        let open_span = self.current_span_or_eof();
        self.bump();
        let mut entries: Vec<(ArenaId, ArenaId)> = Vec::new();
        if self.consume_if(TokenKind::RightBrace) {
            return self.insert_node(AstNode::MapLiteral {
                entries,
                span: merge_pair(open_span, open_span),
            });
        }
        loop {
            let key = self.parse_expression();
            if !self.consume_if(TokenKind::Colon) {
                self.push_error(
                    "expected ':' between map key and value",
                    self.current_span_or_eof(),
                );
                return self.invalid_node(open_span);
            }
            let value = self.parse_expression();
            entries.push((key, value));
            if self.consume_if(TokenKind::Comma) {
                if self
                    .current()
                    .is_some_and(|t| t.kind == TokenKind::RightBrace)
                {
                    break;
                }
                continue;
            }
            break;
        }
        if !self.consume_if(TokenKind::RightBrace) {
            self.push_error("expected '}' after map literal", self.current_span_or_eof());
        }
        let span = merge_pair(open_span, self.previous_span_or(open_span));
        self.insert_node(AstNode::MapLiteral { entries, span })
    }

    fn parse_set_literal_suffix(&mut self, set_kw_span: foundation::span::Span) -> ArenaId {
        let open_span = self.current_span_or_eof();
        self.bump();
        let mut items = Vec::new();
        if self.consume_if(TokenKind::RightBrace) {
            return self.insert_node(AstNode::SetLiteral {
                items,
                span: merge_pair(set_kw_span, open_span),
            });
        }
        loop {
            items.push(self.parse_expression());
            if self.consume_if(TokenKind::Comma) {
                if self
                    .current()
                    .is_some_and(|t| t.kind == TokenKind::RightBrace)
                {
                    break;
                }
                continue;
            }
            break;
        }
        if !self.consume_if(TokenKind::RightBrace) {
            self.push_error("expected '}' after set literal", self.current_span_or_eof());
        }
        let span = merge_pair(set_kw_span, self.previous_span_or(open_span));
        self.insert_node(AstNode::SetLiteral { items, span })
    }

    fn parse_struct_literal_suffix(&mut self, type_ident: ArenaId) -> ArenaId {
        let type_name = self.identifier_name_from_node(type_ident);
        let open = self.current_span_or_eof();
        self.bump();
        let mut fields = Vec::new();
        while self.current().is_some()
            && !self
                .current()
                .is_some_and(|t| t.kind == TokenKind::RightBrace)
        {
            let field_tok = match self.current() {
                Some(t) if t.kind == TokenKind::Identifier => t.clone(),
                _ => {
                    self.push_error(
                        "expected field name in struct literal",
                        self.current_span_or_eof(),
                    );
                    return self.invalid_node(open);
                }
            };
            self.bump();
            if !self.consume_if(TokenKind::Colon) {
                self.push_error(
                    "expected ':' after field name in struct literal",
                    self.current_span_or_eof(),
                );
                return self.invalid_node(open);
            }
            let expr = self.parse_expression();
            fields.push((field_tok.lexeme, expr));
            if self.consume_if(TokenKind::Comma) {
                continue;
            }
            break;
        }
        if !self.consume_if(TokenKind::RightBrace) {
            self.push_error(
                "expected '}' after struct literal fields",
                self.current_span_or_eof(),
            );
        }
        let span = merge_pair(self.node_span(type_ident), self.previous_span_or(open));
        self.insert_node(AstNode::StructLiteralExpr {
            type_name,
            fields,
            span,
        })
    }

    fn looks_like_struct_literal_suffix(&self) -> bool {
        if self.peek_kind(0) != Some(TokenKind::LeftBrace) {
            return false;
        }
        if self.peek_kind(1) == Some(TokenKind::RightBrace) {
            return true;
        }
        self.peek_kind(1) == Some(TokenKind::Identifier)
            && self.peek_kind(2) == Some(TokenKind::Colon)
    }

    fn parse_method_call_suffix(
        &mut self,
        receiver: ArenaId,
        method: String,
        method_span: foundation::span::Span,
    ) -> ArenaId {
        let _open = self.current_span_or_eof();
        self.bump();
        let mut args = Vec::new();
        if !self
            .current()
            .is_some_and(|t| t.kind == TokenKind::RightParen)
        {
            loop {
                args.push(self.parse_expression());
                if self.consume_if(TokenKind::Comma) {
                    continue;
                }
                break;
            }
        }
        if !self.consume_if(TokenKind::RightParen) {
            self.push_error(
                "expected ')' after method arguments",
                self.current_span_or_eof(),
            );
        }
        let span = merge_pair(self.node_span(receiver), self.previous_span_or(method_span));
        self.insert_node(AstNode::MethodCallExpr {
            receiver,
            method,
            args,
            span,
        })
    }

    fn parse_static_method_call_suffix(&mut self, type_ident: ArenaId) -> ArenaId {
        self.bump();
        let method_tok = match self.current() {
            Some(t) if t.kind == TokenKind::Identifier => t.clone(),
            _ => {
                let span = self.current_span_or_eof();
                self.push_error("expected method name after '::'", span);
                return self.invalid_node(span);
            }
        };
        self.bump();
        if !self.consume_if(TokenKind::LeftParen) {
            self.push_error(
                "expected '(' after static method name",
                self.current_span_or_eof(),
            );
            return self.invalid_node(method_tok.span);
        }
        let mut args = Vec::new();
        if !self
            .current()
            .is_some_and(|t| t.kind == TokenKind::RightParen)
        {
            loop {
                args.push(self.parse_expression());
                if self.consume_if(TokenKind::Comma) {
                    continue;
                }
                break;
            }
        }
        if !self.consume_if(TokenKind::RightParen) {
            self.push_error(
                "expected ')' after static method arguments",
                self.current_span_or_eof(),
            );
        }
        let type_name = self.identifier_name_from_node(type_ident);
        let span = merge_pair(
            self.node_span(type_ident),
            self.previous_span_or(method_tok.span),
        );
        self.insert_node(AstNode::StaticMethodCallExpr {
            type_name,
            method: method_tok.lexeme,
            args,
            span,
        })
    }
}

fn merge_pair(
    left: foundation::span::Span,
    right: foundation::span::Span,
) -> foundation::span::Span {
    foundation::span::Span::new_unchecked(left.file_id(), left.start(), right.end())
}

fn next_precedence(prec: Precedence) -> Precedence {
    match prec {
        Precedence::Lowest => Precedence::Range,
        Precedence::Range => Precedence::LogicalOr,
        Precedence::LogicalOr => Precedence::LogicalAnd,
        Precedence::LogicalAnd => Precedence::BitOr,
        Precedence::BitOr => Precedence::BitXor,
        Precedence::BitXor => Precedence::BitAnd,
        Precedence::BitAnd => Precedence::Equality,
        Precedence::Equality => Precedence::Comparison,
        Precedence::Comparison => Precedence::Shift,
        Precedence::Shift => Precedence::Sum,
        Precedence::Sum => Precedence::Product,
        Precedence::Product => Precedence::Power,
        Precedence::Power => Precedence::Highest,
        Precedence::Highest => Precedence::Highest,
    }
}

fn parse_char_lexeme(lexeme: &str) -> Option<char> {
    if !(lexeme.starts_with('\'') && lexeme.ends_with('\'')) {
        return None;
    }
    let inner = &lexeme[1..lexeme.len() - 1];
    if inner.starts_with('\\') {
        return match inner {
            "\\n" => Some('\n'),
            "\\t" => Some('\t'),
            "\\r" => Some('\r'),
            "\\'" => Some('\''),
            "\\\\" => Some('\\'),
            _ => None,
        };
    }
    let mut chars = inner.chars();
    let ch = chars.next()?;
    if chars.next().is_none() {
        Some(ch)
    } else {
        None
    }
}
