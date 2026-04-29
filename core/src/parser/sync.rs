use crate::lexer::TokenKind;

use super::parser::Parser;

impl Parser {
    pub(super) fn synchronize(&mut self) {
        while let Some(token) = self.current() {
            if token.kind == TokenKind::Semicolon {
                self.bump();
                return;
            }
            if self.looks_like_statement_start() {
                return;
            }
            self.bump();
        }
    }

    fn looks_like_statement_start(&self) -> bool {
        let Some(current) = self.current() else {
            return false;
        };

        if current.kind == TokenKind::Identifier {
            return true;
        }

        matches!(
            current.kind,
            TokenKind::If
                | TokenKind::Else
                | TokenKind::Integer
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::Bool
                | TokenKind::Minus
                | TokenKind::Bang
                | TokenKind::Tilde
                | TokenKind::LeftParen
                | TokenKind::LeftBrace
                | TokenKind::RightBrace
        )
    }
}
