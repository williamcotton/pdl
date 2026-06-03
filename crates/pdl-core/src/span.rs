use serde::{Deserialize, Serialize};

pub type ByteOffset = usize;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Span {
    pub start: ByteOffset,
    pub end: ByteOffset,
}

impl Span {
    pub const fn new(start: ByteOffset, end: ByteOffset) -> Self {
        debug_assert!(start <= end, "span start must not exceed end");
        Self { start, end }
    }

    pub const fn empty(offset: ByteOffset) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    pub const fn zero() -> Self {
        Self::empty(0)
    }

    pub const fn len(self) -> usize {
        self.end - self.start
    }

    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub const fn contains(self, offset: ByteOffset) -> bool {
        self.start <= offset && offset < self.end
    }

    pub fn cover(self, other: Span) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn join(self, other: Span) -> Self {
        self.cover(other)
    }
}

impl From<std::ops::Range<usize>> for Span {
    fn from(range: std::ops::Range<usize>) -> Self {
        Self::new(range.start, range.end)
    }
}

impl From<Span> for std::ops::Range<usize> {
    fn from(span: Span) -> Self {
        span.start..span.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_are_half_open_byte_ranges() {
        let span = Span::new(2, 5);

        assert_eq!(span.len(), 3);
        assert!(!span.is_empty());
        assert!(span.contains(2));
        assert!(span.contains(4));
        assert!(!span.contains(5));
        assert_eq!(span.cover(Span::new(0, 3)), Span::new(0, 5));
    }
}
