use foundation::{
    diagnostics::{Diagnostic, Diagnostics, Severity},
    ids::FileId,
    span::Span,
};

use super::token::{Token, TokenKind};

pub struct LexOutput {
    pub tokens: Vec<Token>,
    pub diagnostics: Diagnostics,
}

pub fn lex(file_id: FileId, source: &str) -> LexOutput {
    let mut lexer = Lexer::new(file_id, source);
    lexer.lex_all();
    LexOutput {
        tokens: lexer.tokens,
        diagnostics: lexer.diagnostics,
    }
}

struct Lexer<'a> {
    file_id: FileId,
    source: &'a str,
    cursor: usize,
    tokens: Vec<Token>,
    diagnostics: Diagnostics,
}

impl<'a> Lexer<'a> {
    fn new(file_id: FileId, source: &'a str) -> Self {
        Self {
            file_id,
            source,
            cursor: 0,
            tokens: Vec::new(),
            diagnostics: Diagnostics::new(),
        }
    }

    fn lex_all(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == '\n' {
                self.bump();
                continue;
            }

            if ch.is_whitespace() {
                self.bump();
                continue;
            }

            if ch == '#' {
                self.lex_comment();
                continue;
            }

            if ch.is_ascii_alphabetic() || ch == '_' {
                self.lex_identifier_or_keyword();
                continue;
            }

            if ch.is_ascii_digit() {
                self.lex_number();
                continue;
            }

            if ch == '"' {
                self.lex_string();
                continue;
            }

            let start = self.cursor;
            match ch {
                '+' => {
                    self.bump();
                    self.push_token(TokenKind::Plus, start, self.cursor);
                }
                '-' => {
                    self.bump();
                    self.push_token(TokenKind::Minus, start, self.cursor);
                }
                '*' => {
                    self.bump();
                    self.push_token(TokenKind::Star, start, self.cursor);
                }
                '/' => {
                    self.bump();
                    self.push_token(TokenKind::Slash, start, self.cursor);
                }
                '(' => {
                    self.bump();
                    self.push_token(TokenKind::LeftParen, start, self.cursor);
                }
                ')' => {
                    self.bump();
                    self.push_token(TokenKind::RightParen, start, self.cursor);
                }
                ',' => {
                    self.bump();
                    self.push_token(TokenKind::Comma, start, self.cursor);
                }
                ';' => {
                    self.bump();
                    self.push_token(TokenKind::Semicolon, start, self.cursor);
                }
                ':' => {
                    self.bump();
                    if self.peek() == Some(':') {
                        self.bump();
                        self.push_token(TokenKind::DoubleColon, start, self.cursor);
                    } else if self.peek() == Some('=') {
                        self.bump();
                        self.push_token(TokenKind::InferAssign, start, self.cursor);
                    } else {
                        self.push_token(TokenKind::Colon, start, self.cursor);
                    }
                }
                '=' => {
                    self.bump();
                    self.push_token(TokenKind::Assign, start, self.cursor);
                }
                _ => {
                    self.bump();
                    self.push_invalid_char_diagnostic(start, self.cursor);
                }
            }
        }
    }

    fn lex_comment(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == '\n' {
                break;
            }
            self.bump();
        }
    }

    fn lex_identifier_or_keyword(&mut self) {
        let start = self.cursor;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                self.bump();
            } else {
                break;
            }
        }
        let text = &self.source[start..self.cursor];
        let kind = if text == "true" || text == "false" {
            TokenKind::Bool
        } else if is_known_type(text) {
            TokenKind::TypeName
        } else {
            TokenKind::Identifier
        };
        self.push_token(kind, start, self.cursor);
    }

    fn lex_number(&mut self) {
        let start = self.cursor;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                self.bump();
            } else {
                break;
            }
        }

        if self.peek() == Some('.') {
            let dot_start = self.cursor;
            self.bump();
            if self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                while let Some(ch) = self.peek() {
                    if ch.is_ascii_digit() {
                        self.bump();
                    } else {
                        break;
                    }
                }
                self.push_token(TokenKind::Float, start, self.cursor);
                return;
            }

            self.push_token(TokenKind::Integer, start, dot_start);
            self.push_invalid_number_diagnostic(start, self.cursor);
            return;
        }

        self.push_token(TokenKind::Integer, start, self.cursor);
    }

    fn lex_string(&mut self) {
        let start = self.cursor;
        self.bump();
        while let Some(ch) = self.peek() {
            self.bump();
            if ch == '"' {
                self.push_token(TokenKind::String, start, self.cursor);
                return;
            }
            if ch == '\n' {
                break;
            }
        }
        self.push_token(TokenKind::String, start, self.cursor);
        self.push_unterminated_string_diagnostic(start, self.cursor);
    }

    fn push_token(&mut self, kind: TokenKind, start: usize, end: usize) {
        let lexeme = self.source[start..end].to_string();
        let span = Span::new_unchecked(self.file_id, start as u32, end as u32);
        self.tokens.push(Token { kind, lexeme, span });
    }

    fn push_invalid_char_diagnostic(&mut self, start: usize, end: usize) {
        let span = Span::new_unchecked(self.file_id, start as u32, end as u32);
        self.diagnostics.push(Diagnostic::new(
            format!("invalid character: '{}'", &self.source[start..end]),
            span,
            Severity::Error,
        ));
    }

    fn push_invalid_number_diagnostic(&mut self, start: usize, end: usize) {
        let span = Span::new_unchecked(self.file_id, start as u32, end as u32);
        self.diagnostics.push(Diagnostic::new(
            format!("invalid float literal: '{}'", &self.source[start..end]),
            span,
            Severity::Error,
        ));
    }

    fn push_unterminated_string_diagnostic(&mut self, start: usize, end: usize) {
        let span = Span::new_unchecked(self.file_id, start as u32, end as u32);
        self.diagnostics.push(Diagnostic::new(
            "unterminated string literal",
            span,
            Severity::Error,
        ));
    }

    fn peek(&self) -> Option<char> {
        self.source[self.cursor..].chars().next()
    }

    fn bump(&mut self) {
        if let Some(ch) = self.peek() {
            self.cursor += ch.len_utf8();
        }
    }
}

fn is_known_type(text: &str) -> bool {
    matches!(
        text,
        "i1"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "u1"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "f32"
            | "f64"
            | "str"
            | "bool"
            | "char"
    )
}

#[cfg(test)]
mod tests {
    use foundation::ids::FileId;

    use super::{lex, TokenKind};

    #[test]
    fn lexes_tokens_used_by_example_file() {
        let src = r#"age: i32 = 20
name: str = "John"
is_student: bool = true
PI:: f32 = 3.14159
print(name, age)
"#;
        let output = lex(FileId::from_u32(1), src);
        assert!(output.diagnostics.is_empty());
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::DoubleColon));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::TypeName));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Float));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::String));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Comma));
    }

    #[test]
    fn malformed_float_does_not_poison_integer_token() {
        let output = lex(FileId::from_u32(2), "x := 1.");
        let integer = output
            .tokens
            .iter()
            .find(|t| t.kind == TokenKind::Integer)
            .expect("integer token");
        assert_eq!(integer.lexeme, "1");
        assert!(output.diagnostics.has_errors());
    }

    #[test]
    fn malformed_dot_prefix_reports_error() {
        let output = lex(FileId::from_u32(3), "x := .5");
        assert!(output.diagnostics.has_errors());
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Integer && t.lexeme == "5"));
    }

    #[test]
    fn malformed_float_with_double_dot_reports_errors() {
        let output = lex(FileId::from_u32(4), "x := 1..2");
        let integers: Vec<_> = output
            .tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Integer)
            .map(|t| t.lexeme.as_str())
            .collect();
        assert_eq!(integers, vec!["1", "2"]);
        assert!(output.diagnostics.has_errors());
    }
}
