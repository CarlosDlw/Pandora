use crate::{
    ast::{AstNode, BinaryOp, IncDecOp, IncDecPosition, UnaryOp},
    lexer::TokenKind,
};
use foundation::ids::ArenaId;

use super::parser::Parser;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Precedence {
    Lowest,
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
            if self.current().is_some_and(|t| t.kind == TokenKind::LeftParen)
                && Precedence::Highest >= min_prec
            {
                left = self.parse_call_suffix(left);
                continue;
            }

            if self.current().is_some_and(|t| t.kind == TokenKind::PlusPlus || t.kind == TokenKind::MinusMinus)
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
                self.insert_node(AstNode::Identifier {
                    name: token.lexeme,
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
                let expr = self.parse_expression();
                if !self.consume_if(TokenKind::RightParen) {
                    let err_span = merge_pair(open_span, self.node_span(expr));
                    self.push_error("expected ')'", err_span);
                }
                expr
            }
            _ => {
                self.bump();
                self.push_error("expected expression", token.span);
                self.invalid_node(token.span)
            }
        }
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
            TokenKind::GreaterEqual => Some((BinaryOp::GreaterEqual, Precedence::Comparison, false)),
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

            self.push_error("expected ',' or ')' in argument list", self.current_span_or_eof());
            break;
        }

        let end = args
            .last()
            .map(|id| self.node_span(*id))
            .unwrap_or(open);
        let span = merge_pair(self.node_span(callee), end);
        self.insert_node(AstNode::CallExpr { callee, args, span })
    }
}

fn merge_pair(left: foundation::span::Span, right: foundation::span::Span) -> foundation::span::Span {
    foundation::span::Span::new_unchecked(left.file_id(), left.start(), right.end())
}

fn next_precedence(prec: Precedence) -> Precedence {
    match prec {
        Precedence::Lowest => Precedence::LogicalOr,
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
