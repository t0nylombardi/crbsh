use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenizeError {
    UnterminatedSingleQuote,
    UnterminatedDoubleQuote,
    TrailingEscape,
}

impl fmt::Display for TokenizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnterminatedSingleQuote => write!(formatter, "unterminated single quote"),
            Self::UnterminatedDoubleQuote => write!(formatter, "unterminated double quote"),
            Self::TrailingEscape => write!(formatter, "trailing escape"),
        }
    }
}
