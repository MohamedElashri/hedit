//! Indexed UTF-8/UTF-16 conversion with no surrogate or CRLF splits.
use crate::{sdk, Error};
pub(crate) struct Positions {
    points: Vec<(usize, usize, usize)>,
}
impl Positions {
    pub(crate) fn new(text: &str) -> Self {
        let mut points = Vec::with_capacity(text.chars().count() + 1);
        let (mut line, mut column) = (0, 0);
        for (byte, ch) in text.char_indices() {
            if sdk::boundary(text, byte) {
                points.push((byte, line, column));
            }
            if ch == '\n' {
                line += 1;
                column = 0;
            } else {
                column += ch.len_utf16();
            }
        }
        points.push((text.len(), line, column));
        Self { points }
    }
    pub(crate) fn position(&self, byte: usize) -> Result<(usize, usize), Error> {
        self.points
            .binary_search_by_key(&byte, |p| p.0)
            .map(|at| (self.points[at].1, self.points[at].2))
            .map_err(|_| Error::Request)
    }
    pub(crate) fn offset(&self, line: usize, column: usize) -> Result<usize, Error> {
        self.points
            .binary_search_by_key(&(line, column), |p| (p.1, p.2))
            .map(|at| self.points[at].0)
            .map_err(|_| Error::Request)
    }
}
pub fn utf16_position(text: &str, byte: usize) -> Result<(usize, usize), Error> {
    Positions::new(text).position(byte)
}
pub fn utf16_offset(text: &str, line: usize, column: usize) -> Result<usize, Error> {
    Positions::new(text).offset(line, column)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexed_units_match_independent_slices_and_reject_split_pairs() {
        for text in ["", "λ\r\n東京🦀\n", "e\u{301}\r", "🦀\r\n\n"] {
            let index = Positions::new(text);
            for (byte, _) in text
                .char_indices()
                .chain(std::iter::once((text.len(), '\0')))
            {
                if !sdk::boundary(text, byte) {
                    assert!(index.position(byte).is_err());
                    continue;
                }
                let before = &text[..byte];
                let line = before.bytes().filter(|b| *b == b'\n').count();
                let start = before.rfind('\n').map_or(0, |n| n + 1);
                let column = text[start..byte].encode_utf16().count();
                assert_eq!(index.position(byte).unwrap(), (line, column));
                assert_eq!(index.offset(line, column).unwrap(), byte);
            }
        }
        assert!(utf16_offset("🦀", 0, 1).is_err());
        assert!(utf16_position("\r\n", 1).is_err());
        assert!(utf16_offset("x\r\n", 0, 2).is_err());
    }
}
