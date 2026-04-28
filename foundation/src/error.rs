use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoundationError {
    IdExhausted { kind: &'static str },
    InvalidSpan { start: u32, end: u32 },
    FileNotFound,
    InconsistentState(&'static str),
}

impl fmt::Display for FoundationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdExhausted { kind } => write!(f, "id space exhausted for {kind}"),
            Self::InvalidSpan { start, end } => {
                write!(f, "invalid span: start ({start}) is greater than end ({end})")
            }
            Self::FileNotFound => write!(f, "file not found"),
            Self::InconsistentState(message) => write!(f, "inconsistent internal state: {message}"),
        }
    }
}

impl Error for FoundationError {}
