use crate::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub span: Span,
    pub severity: Severity,
}

impl Diagnostic {
    pub fn new(message: impl Into<String>, span: Span, severity: Severity) -> Self {
        Self {
            message: message.into(),
            span,
            severity,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    pub fn extend(&mut self, other: Diagnostics) {
        self.items.extend(other.items);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| matches!(d.severity, Severity::Error))
    }
}

#[cfg(test)]
mod tests {
    use crate::{ids::FileId, span::Span};

    use super::{Diagnostic, Diagnostics, Severity};

    #[test]
    fn diagnostics_accumulate_items() {
        let span = Span::try_new(FileId::from_u32(1), 0, 1).expect("valid span");
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(Diagnostic::new("test", span, Severity::Warning));
        assert_eq!(diagnostics.len(), 1);
        assert!(!diagnostics.has_errors());
    }
}
