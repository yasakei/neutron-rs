use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// A source location: byte range in the source text plus the 1-based
/// line/column of the range start.
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub column: u32,
}

impl Span {
    pub fn new(start: usize, end: usize, line: u32, column: u32) -> Self {
        Self {
            start,
            end,
            line,
            column,
        }
    }

    /// A zero-value placeholder span (0..0 at line 1, column 1).
    pub fn dummy() -> Self {
        Self {
            start: 0,
            end: 0,
            line: 1,
            column: 1,
        }
    }

    /// Merge two spans to cover `self.start`..`other.end` (used to span
    /// whole expressions from their first to last token).
    pub fn to(self, other: Self) -> Self {
        Self {
            start: self.start,
            end: other.end,
            line: self.line,
            column: self.column,
        }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}
