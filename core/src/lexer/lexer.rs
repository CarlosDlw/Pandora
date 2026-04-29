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
            if ch == '\'' {
                self.lex_char();
                continue;
            }

            let start = self.cursor;
            match ch {
                '+' => {
                    self.bump();
                    if self.peek() == Some('+') {
                        self.bump();
                        self.push_token(TokenKind::PlusPlus, start, self.cursor);
                    } else if self.peek() == Some('=') {
                        self.bump();
                        self.push_token(TokenKind::PlusAssign, start, self.cursor);
                    } else {
                        self.push_token(TokenKind::Plus, start, self.cursor);
                    }
                }
                '-' => {
                    self.bump();
                    if self.peek() == Some('-') {
                        self.bump();
                        self.push_token(TokenKind::MinusMinus, start, self.cursor);
                    } else if self.peek() == Some('=') {
                        self.bump();
                        self.push_token(TokenKind::MinusAssign, start, self.cursor);
                    } else {
                        self.push_token(TokenKind::Minus, start, self.cursor);
                    }
                }
                '*' => {
                    self.bump();
                    if self.peek() == Some('*') {
                        self.bump();
                        if self.peek() == Some('=') {
                            self.bump();
                            self.push_token(TokenKind::DoubleStarAssign, start, self.cursor);
                        } else {
                            self.push_token(TokenKind::DoubleStar, start, self.cursor);
                        }
                    } else if self.peek() == Some('=') {
                        self.bump();
                        self.push_token(TokenKind::StarAssign, start, self.cursor);
                    } else {
                        self.push_token(TokenKind::Star, start, self.cursor);
                    }
                }
                '/' => {
                    self.bump();
                    if self.peek() == Some('=') {
                        self.bump();
                        self.push_token(TokenKind::SlashAssign, start, self.cursor);
                    } else {
                        self.push_token(TokenKind::Slash, start, self.cursor);
                    }
                }
                '%' => {
                    self.bump();
                    if self.peek() == Some('=') {
                        self.bump();
                        self.push_token(TokenKind::PercentAssign, start, self.cursor);
                    } else {
                        self.push_token(TokenKind::Percent, start, self.cursor);
                    }
                }
                '!' => {
                    self.bump();
                    if self.peek() == Some('=') {
                        self.bump();
                        self.push_token(TokenKind::BangEqual, start, self.cursor);
                    } else {
                        self.push_token(TokenKind::Bang, start, self.cursor);
                    }
                }
                '~' => {
                    self.bump();
                    self.push_token(TokenKind::Tilde, start, self.cursor);
                }
                '&' => {
                    self.bump();
                    if self.peek() == Some('&') {
                        self.bump();
                        self.push_token(TokenKind::AndAnd, start, self.cursor);
                    } else if self.peek() == Some('=') {
                        self.bump();
                        self.push_token(TokenKind::AmpersandAssign, start, self.cursor);
                    } else {
                        self.push_token(TokenKind::Ampersand, start, self.cursor);
                    }
                }
                '|' => {
                    self.bump();
                    if self.peek() == Some('|') {
                        self.bump();
                        self.push_token(TokenKind::OrOr, start, self.cursor);
                    } else if self.peek() == Some('=') {
                        self.bump();
                        self.push_token(TokenKind::PipeAssign, start, self.cursor);
                    } else {
                        self.push_token(TokenKind::Pipe, start, self.cursor);
                    }
                }
                '^' => {
                    self.bump();
                    if self.peek() == Some('=') {
                        self.bump();
                        self.push_token(TokenKind::CaretAssign, start, self.cursor);
                    } else {
                        self.push_token(TokenKind::Caret, start, self.cursor);
                    }
                }
                '<' => {
                    self.bump();
                    if self.peek() == Some('<') {
                        self.bump();
                        if self.peek() == Some('=') {
                            self.bump();
                            self.push_token(TokenKind::ShiftLeftAssign, start, self.cursor);
                        } else {
                            self.push_token(TokenKind::ShiftLeft, start, self.cursor);
                        }
                    } else if self.peek() == Some('=') {
                        self.bump();
                        self.push_token(TokenKind::LessEqual, start, self.cursor);
                    } else {
                        self.push_token(TokenKind::Less, start, self.cursor);
                    }
                }
                '>' => {
                    self.bump();
                    if self.peek() == Some('>') {
                        self.bump();
                        if self.peek() == Some('=') {
                            self.bump();
                            self.push_token(TokenKind::ShiftRightAssign, start, self.cursor);
                        } else {
                            self.push_token(TokenKind::ShiftRight, start, self.cursor);
                        }
                    } else if self.peek() == Some('=') {
                        self.bump();
                        self.push_token(TokenKind::GreaterEqual, start, self.cursor);
                    } else {
                        self.push_token(TokenKind::Greater, start, self.cursor);
                    }
                }
                '(' => {
                    self.bump();
                    self.push_token(TokenKind::LeftParen, start, self.cursor);
                }
                ')' => {
                    self.bump();
                    self.push_token(TokenKind::RightParen, start, self.cursor);
                }
                '{' => {
                    self.bump();
                    self.push_token(TokenKind::LeftBrace, start, self.cursor);
                }
                '}' => {
                    self.bump();
                    self.push_token(TokenKind::RightBrace, start, self.cursor);
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
                    if self.peek() == Some('=') {
                        self.bump();
                        self.push_token(TokenKind::EqualEqual, start, self.cursor);
                    } else {
                        self.push_token(TokenKind::Assign, start, self.cursor);
                    }
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
        let kind = match text {
            "true" | "false" => TokenKind::Bool,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "for" => TokenKind::For,
            _ if is_known_type(text) => TokenKind::TypeName,
            _ => TokenKind::Identifier,
        };
        self.push_token(kind, start, self.cursor);
    }

    fn lex_number(&mut self) {
        let start = self.cursor;
        let mut is_float = false;
        let mut had_error = false;

        if self.peek() == Some('0') {
            match self.peek_next() {
                Some('x' | 'X') => {
                    self.bump();
                    self.bump();
                    let digits_start = self.cursor;
                    if !self.lex_based_digits(is_hex_digit) {
                        had_error = true;
                    }
                    if self.peek().is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
                        had_error = true;
                        self.consume_number_tail();
                    }
                    if self.cursor == digits_start {
                        had_error = true;
                    }
                    self.push_token(TokenKind::Integer, start, self.cursor);
                    if had_error {
                        self.push_invalid_number_diagnostic(start, self.cursor);
                    }
                    return;
                }
                Some('o' | 'O') => {
                    self.bump();
                    self.bump();
                    let digits_start = self.cursor;
                    if !self.lex_based_digits(is_octal_digit) {
                        had_error = true;
                    }
                    if self.peek().is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
                        had_error = true;
                        self.consume_number_tail();
                    }
                    if self.cursor == digits_start {
                        had_error = true;
                    }
                    self.push_token(TokenKind::Integer, start, self.cursor);
                    if had_error {
                        self.push_invalid_number_diagnostic(start, self.cursor);
                    }
                    return;
                }
                Some('b' | 'B') => {
                    self.bump();
                    self.bump();
                    let digits_start = self.cursor;
                    if !self.lex_based_digits(is_binary_digit) {
                        had_error = true;
                    }
                    if self.peek().is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
                        had_error = true;
                        self.consume_number_tail();
                    }
                    if self.cursor == digits_start {
                        had_error = true;
                    }
                    self.push_token(TokenKind::Integer, start, self.cursor);
                    if had_error {
                        self.push_invalid_number_diagnostic(start, self.cursor);
                    }
                    return;
                }
                _ => {}
            }
        }

        if !self.lex_decimal_digits() {
            had_error = true;
        }

        if self.peek() == Some('.') {
            let dot_start = self.cursor;
            self.bump();
            if self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                is_float = true;
                if !self.lex_decimal_digits() {
                    had_error = true;
                }
            } else {
                self.push_token(TokenKind::Integer, start, dot_start);
                self.push_invalid_number_diagnostic(start, self.cursor);
                return;
            }
        }

        if matches!(self.peek(), Some('e' | 'E')) {
            is_float = true;
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            let exp_start = self.cursor;
            if !self.lex_decimal_digits() {
                had_error = true;
            }
            if self.cursor == exp_start {
                had_error = true;
            }
        }

        if is_float {
            self.push_token(TokenKind::Float, start, self.cursor);
        } else {
            self.push_token(TokenKind::Integer, start, self.cursor);
        }
        if had_error {
            self.push_invalid_number_diagnostic(start, self.cursor);
        }
    }

    fn lex_decimal_digits(&mut self) -> bool {
        self.lex_based_digits(|ch| ch.is_ascii_digit())
    }

    fn lex_based_digits(&mut self, is_digit: fn(char) -> bool) -> bool {
        let mut had_digits = false;
        let mut last_was_underscore = false;
        let mut valid = true;
        while let Some(ch) = self.peek() {
            if is_digit(ch) {
                had_digits = true;
                last_was_underscore = false;
                self.bump();
                continue;
            }
            if ch == '_' {
                if !had_digits || last_was_underscore {
                    valid = false;
                }
                last_was_underscore = true;
                self.bump();
                continue;
            }
            break;
        }
        if last_was_underscore {
            valid = false;
        }
        valid && had_digits
    }

    fn consume_number_tail(&mut self) {
        while self.peek().is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            self.bump();
        }
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

    fn lex_char(&mut self) {
        let start = self.cursor;
        self.bump();

        let Some(first) = self.peek() else {
            self.push_invalid_char_literal_diagnostic(start, self.cursor);
            return;
        };

        if first == '\\' {
            self.bump();
            if self.peek().is_some() {
                self.bump();
            } else {
                self.push_invalid_char_literal_diagnostic(start, self.cursor);
                return;
            }
        } else if first == '\'' || first == '\n' {
            self.push_invalid_char_literal_diagnostic(start, self.cursor);
            return;
        } else {
            self.bump();
        }

        if self.peek() == Some('\'') {
            self.bump();
            self.push_token(TokenKind::Char, start, self.cursor);
        } else {
            self.push_invalid_char_literal_diagnostic(start, self.cursor);
        }
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
            format!("invalid numeric literal: '{}'", &self.source[start..end]),
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

    fn push_invalid_char_literal_diagnostic(&mut self, start: usize, end: usize) {
        let span = Span::new_unchecked(self.file_id, start as u32, end as u32);
        self.diagnostics.push(Diagnostic::new(
            "invalid char literal",
            span,
            Severity::Error,
        ));
    }

    fn peek(&self) -> Option<char> {
        self.source[self.cursor..].chars().next()
    }

    fn peek_next(&self) -> Option<char> {
        let mut chars = self.source[self.cursor..].chars();
        let _ = chars.next();
        chars.next()
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

fn is_hex_digit(ch: char) -> bool {
    ch.is_ascii_hexdigit()
}

fn is_octal_digit(ch: char) -> bool {
    ('0'..='7').contains(&ch)
}

fn is_binary_digit(ch: char) -> bool {
    ch == '0' || ch == '1'
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
    fn lexes_block_braces() {
        let output = lex(FileId::from_u32(6), "{ x := 1 }");
        assert!(!output.diagnostics.has_errors());
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::LeftBrace));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::RightBrace));
    }

    #[test]
    fn lexes_char_literal() {
        let output = lex(FileId::from_u32(5), "c: char = 'x'");
        assert!(!output.diagnostics.has_errors());
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Char));
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

    #[test]
    fn lexes_new_operator_tokens() {
        let output = lex(
            FileId::from_u32(7),
            "a == b != c <= d >= e < f > g && h || i & j | k ^ l << m >> n !o ~p % q ** r",
        );
        assert!(!output.diagnostics.has_errors());
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::EqualEqual));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::BangEqual));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::LessEqual));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::GreaterEqual));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::AndAnd));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::OrOr));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Ampersand));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Pipe));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Caret));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::ShiftLeft));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::ShiftRight));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Bang));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Tilde));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Percent));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::DoubleStar));
    }

    #[test]
    fn lexes_based_and_scientific_literals() {
        let output = lex(FileId::from_u32(8), "a := 0xFF; b := 0o755; c := 0b1010; d := 1_000_000; e := 6.02e23");
        assert!(!output.diagnostics.has_errors());
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Integer && t.lexeme == "0xFF"));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Integer && t.lexeme == "0o755"));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Integer && t.lexeme == "0b1010"));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Integer && t.lexeme == "1_000_000"));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Float && t.lexeme == "6.02e23"));
    }

    #[test]
    fn invalid_based_and_scientific_literals_report_errors() {
        let output = lex(FileId::from_u32(9), "a := 0x; b := 0b102; c := 1__0; d := 1e+");
        assert!(output.diagnostics.has_errors());
        assert!(output
            .diagnostics
            .iter()
            .all(|d| d.message.contains("invalid numeric literal")));
    }

    #[test]
    fn lexes_if_else_keywords() {
        let output = lex(FileId::from_u32(10), "if true { x := 1 } else { x := 2 }");
        assert!(!output.diagnostics.has_errors());
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::If));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Else));
    }

    #[test]
    fn keeps_partial_keyword_as_identifier() {
        let output = lex(FileId::from_u32(11), "gift := 1");
        assert!(!output.diagnostics.has_errors());
        assert!(output
            .tokens
            .iter()
            .any(|t| t.kind == TokenKind::Identifier && t.lexeme == "gift"));
    }

    #[test]
    fn lexes_while_break_continue_keywords() {
        let output = lex(FileId::from_u32(12), "while true { break; continue }");
        assert!(!output.diagnostics.has_errors());
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::While));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Break));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::Continue));
    }

    #[test]
    fn lexes_for_and_incdec_tokens() {
        let output = lex(FileId::from_u32(17), "for i: i32 = 0; i < 10; i++ { --i }");
        assert!(!output.diagnostics.has_errors());
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::For));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::PlusPlus));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::MinusMinus));
    }

    #[test]
    fn distinguishes_incdec_from_plus_minus_variants() {
        let output = lex(FileId::from_u32(18), "x++ + ++x; y-- - --y; z += 1; w -= 1");
        assert!(!output.diagnostics.has_errors());
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::PlusPlus));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::MinusMinus));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::PlusAssign));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::MinusAssign));
    }

    #[test]
    fn keeps_loop_keyword_prefixes_as_identifiers() {
        let output = lex(FileId::from_u32(13), "breakfast := 1; continued := 2; meanwhile := 3");
        assert!(!output.diagnostics.has_errors());
        assert!(output
            .tokens
            .iter()
            .any(|t| t.kind == TokenKind::Identifier && t.lexeme == "breakfast"));
        assert!(output
            .tokens
            .iter()
            .any(|t| t.kind == TokenKind::Identifier && t.lexeme == "continued"));
        assert!(output
            .tokens
            .iter()
            .any(|t| t.kind == TokenKind::Identifier && t.lexeme == "meanwhile"));
    }

    #[test]
    fn lexes_compound_assignment_tokens() {
        let output = lex(
            FileId::from_u32(14),
            "a += 1; b -= 1; c *= 1; d /= 1; e %= 1; f **= 2; g &= 1; h |= 1; i ^= 1; j <<= 1; k >>= 1",
        );
        assert!(!output.diagnostics.has_errors());
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::PlusAssign));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::MinusAssign));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::StarAssign));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::SlashAssign));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::PercentAssign));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::DoubleStarAssign));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::AmpersandAssign));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::PipeAssign));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::CaretAssign));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::ShiftLeftAssign));
        assert!(output.tokens.iter().any(|t| t.kind == TokenKind::ShiftRightAssign));
    }

    #[test]
    fn distinguishes_compound_from_simple() {
        let compound = lex(FileId::from_u32(15), "x += 1");
        assert!(compound.tokens.iter().any(|t| t.kind == TokenKind::PlusAssign));

        let split = lex(FileId::from_u32(16), "x + = 1");
        assert!(split.tokens.iter().any(|t| t.kind == TokenKind::Plus));
        assert!(split.tokens.iter().any(|t| t.kind == TokenKind::Assign));
        assert!(!split.tokens.iter().any(|t| t.kind == TokenKind::PlusAssign));
    }
}
