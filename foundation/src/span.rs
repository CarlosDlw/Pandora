use crate::{error::FoundationError, ids::FileId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    file_id: FileId,
    start: u32,
    end: u32,
}

impl Span {
    pub fn try_new(file_id: FileId, start: u32, end: u32) -> Result<Self, FoundationError> {
        if start > end {
            return Err(FoundationError::InvalidSpan { start, end });
        }

        Ok(Self {
            file_id,
            start,
            end,
        })
    }

    pub const fn new_unchecked(file_id: FileId, start: u32, end: u32) -> Self {
        Self { file_id, start, end }
    }

    pub const fn file_id(&self) -> FileId {
        self.file_id
    }

    pub const fn start(&self) -> u32 {
        self.start
    }

    pub const fn end(&self) -> u32 {
        self.end
    }
}

#[cfg(test)]
mod tests {
    use crate::ids::FileId;

    use super::Span;

    #[test]
    fn creates_valid_span() {
        let span = Span::try_new(FileId::from_u32(1), 5, 10).expect("valid span");
        assert_eq!(span.start(), 5);
        assert_eq!(span.end(), 10);
    }

    #[test]
    fn rejects_invalid_span_range() {
        let err = Span::try_new(FileId::from_u32(1), 10, 5).expect_err("must fail");
        assert!(matches!(
            err,
            crate::error::FoundationError::InvalidSpan { start: 10, end: 5 }
        ));
    }
}
