use foundation::diagnostics::{Diagnostic, Severity};

pub fn render(path: &str, source: &str, diagnostic: &Diagnostic) -> String {
    let sev = severity_label(diagnostic.severity);
    let start = diagnostic.span.start() as usize;
    let end = diagnostic.span.end() as usize;
    let mut out = String::new();

    out.push_str(&format!("{sev}: {}\n", diagnostic.message));

    if let Some(loc) = locate(source, start, end) {
        out.push_str(&format!(
            "  --> {}:{}:{} [{}..{}]\n",
            path, loc.line, loc.column, diagnostic.span.start(), diagnostic.span.end()
        ));
        out.push_str("   |\n");
        out.push_str(&format!("{:>3} | {}\n", loc.line, loc.line_text));
        out.push_str(&format!("   | {}\n", caret_line(&loc)));
    } else {
        out.push_str(&format!(
            "  --> {}:? [?] [{}..{}]\n",
            path,
            diagnostic.span.start(),
            diagnostic.span.end()
        ));
    }

    if let Some(help) = suggest_fix(&diagnostic.message) {
        out.push_str(&format!("   = help: {help}\n"));
    }

    out
}

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

struct Location {
    line: usize,
    column: usize,
    line_text: String,
    highlight_len: usize,
}

fn locate(source: &str, start: usize, end: usize) -> Option<Location> {
    if start > source.len() || end > source.len() || start > end {
        return None;
    }

    let mut line = 1usize;
    let mut col = 1usize;
    let mut line_start = 0usize;
    for (idx, ch) in source.char_indices() {
        if idx >= start {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
            line_start = idx + ch.len_utf8();
        } else {
            col += 1;
        }
    }

    let line_end = source[line_start..]
        .find('\n')
        .map(|off| line_start + off)
        .unwrap_or(source.len());
    let line_text = source[line_start..line_end].to_string();

    let safe_end = end.max(start + 1);
    let highlight_len = if safe_end <= line_end {
        safe_end.saturating_sub(start).max(1)
    } else {
        1
    };

    Some(Location {
        line,
        column: col,
        line_text,
        highlight_len,
    })
}

fn caret_line(loc: &Location) -> String {
    let padding = " ".repeat(loc.column.saturating_sub(1));
    let marks = "^".repeat(loc.highlight_len);
    format!("{padding}{marks}")
}

fn suggest_fix(message: &str) -> Option<&'static str> {
    if message.contains("expected ')'") {
        Some("close the missing ')' at the end of the expression or call.")
    } else if message.contains("expected '{' after if condition") {
        Some("add a block after the condition, for example: if condition { ... }")
    } else if message.contains("expected '{' or 'if' after 'else'") {
        Some("use `else if condition { ... }` or `else { ... }`.")
    } else if message.contains("unexpected 'else' without matching 'if'") {
        Some("remove this `else` or add the corresponding `if` before it.")
    } else if message.contains("if condition is not truthy/falsy-compatible") {
        Some("use a truthy/falsy-compatible value in the condition, such as bool, number, string, or char.")
    } else if message.contains("expected '}'") {
        Some("close the block with '}' to end its local scope.")
    } else if message.contains("undefined symbol") {
        Some("declare the variable before use with ':=' or ': type = value'; symbols from a block are not visible outside it.")
    } else if message.contains("cannot assign to constant") {
        Some("use ':' instead of '::' for mutable bindings, or assign to a new name.")
    } else if message.contains("invalid argument type") || message.contains("cannot assign value of type") {
        Some("adjust the declared type to match the value, or convert the value to the expected type.")
    } else if message.contains("unterminated string") {
        Some("close the string with a double quote and escape quotes/newlines when needed.")
    } else if message.contains("invalid char literal") {
        Some("use exactly one character in single quotes, for example 'a' or '\\n'.")
    } else if message.contains("division by zero") {
        Some("ensure the divisor is never zero before this operation.")
    } else if message.contains("invalid numeric literal") || message.contains("invalid integer literal") {
        Some("check base prefixes (0b/0o/0x), place '_' only between digits, and use valid exponent form like 1.2e-3.")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use foundation::{diagnostics::Diagnostic, ids::FileId, span::Span};

    use super::render;

    #[test]
    fn renders_single_column_span() {
        let source = "abc\nx := 1\n";
        let span = Span::new_unchecked(FileId::from_u32(0), 4, 5);
        let d = Diagnostic::new("oops", span, foundation::diagnostics::Severity::Error);
        let text = render("main.pand", source, &d);
        assert!(text.contains("--> main.pand:2:1 [4..5]"));
        assert!(text.contains("  2 | x := 1"));
        assert!(text.contains("   | ^"));
    }

    #[test]
    fn renders_multi_column_same_line_span() {
        let source = "name := 123\n";
        let span = Span::new_unchecked(FileId::from_u32(0), 8, 11);
        let d = Diagnostic::new("bad", span, foundation::diagnostics::Severity::Error);
        let text = render("main.pand", source, &d);
        assert!(text.contains("^^^"));
    }

    #[test]
    fn multi_line_span_marks_start_only() {
        let source = "a := (\n1 + 2\n";
        let span = Span::new_unchecked(FileId::from_u32(0), 5, 11);
        let d = Diagnostic::new("expected ')'", span, foundation::diagnostics::Severity::Error);
        let text = render("main.pand", source, &d);
        assert!(text.contains("  1 | a := ("));
        assert!(text.contains("   = help:"));
    }

    #[test]
    fn out_of_bounds_span_falls_back() {
        let source = "x := 1";
        let span = Span::new_unchecked(FileId::from_u32(0), 99, 100);
        let d = Diagnostic::new("bad", span, foundation::diagnostics::Severity::Error);
        let text = render("main.pand", source, &d);
        assert!(text.contains("--> main.pand:? [?] [99..100]"));
    }

    #[test]
    fn help_is_optional() {
        let source = "x := 1";
        let span = Span::new_unchecked(FileId::from_u32(0), 0, 1);
        let d = Diagnostic::new("custom unknown message", span, foundation::diagnostics::Severity::Error);
        let text = render("main.pand", source, &d);
        assert!(!text.contains("= help:"));
    }

    #[test]
    fn suggests_fix_for_missing_right_brace() {
        let source = "{ x := 1";
        let span = Span::new_unchecked(FileId::from_u32(0), 0, source.len() as u32);
        let d = Diagnostic::new("expected '}'", span, foundation::diagnostics::Severity::Error);
        let text = render("main.pand", source, &d);
        assert!(text.contains("close the block with '}'"));
    }
}
