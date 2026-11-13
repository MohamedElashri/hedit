//! Logical line offsets shared by the editor and status bar. Cursor coordinates
//! count Unicode scalars, just like TextEdit; byte offsets always stay on UTF-8 boundaries.
#[derive(Clone, Default)]
pub(in crate::app) struct TextIndex {
    lines: Vec<(usize, usize)>,
    pub chars: usize,
}
impl TextIndex {
    pub fn new(text: &str) -> Self {
        let mut this = Self {
            lines: vec![(0, 0)],
            chars: 0,
        };
        this.append(text, 0, 0);
        this
    }
    fn append(&mut self, text: &str, byte: usize, chars: usize) {
        let mut count = chars;
        for (at, ch) in text.char_indices() {
            count += 1;
            if ch == '\n' {
                self.lines.push((byte + at + 1, count));
            }
        }
        self.chars = count;
    }
    fn char_at_byte(&self, text: &str, byte: usize) -> usize {
        let i = self
            .lines
            .partition_point(|line| line.0 <= byte)
            .saturating_sub(1);
        let (start, chars) = self.lines[i];
        chars + text[start..byte].chars().count()
    }
    pub fn update(&mut self, old: &str, new: &str) {
        let (start, end, inserted) = hedit::workspace::splice(old, new);
        let first_char = self.char_at_byte(old, start);
        let last_char = self.char_at_byte(old, end);
        self.replace(start, end, first_char, last_char, inserted);
    }
    pub fn byte(&self, text: &str, cursor: usize) -> usize {
        let cursor = cursor.min(self.chars);
        let line = self
            .lines
            .partition_point(|line| line.1 <= cursor)
            .saturating_sub(1);
        let (byte, chars) = self.lines[line];
        byte + text[byte..]
            .char_indices()
            .nth(cursor - chars)
            .map_or(text.len() - byte, |(at, _)| at)
    }
    pub fn replace(
        &mut self,
        start: usize,
        end: usize,
        first_char: usize,
        last_char: usize,
        inserted: &str,
    ) {
        let added = inserted.chars().count();
        let byte_delta = inserted.len() as isize - (end - start) as isize;
        let char_delta = added as isize - (last_char - first_char) as isize;
        let first = self.lines.partition_point(|line| line.0 <= start);
        let last = self.lines.partition_point(|line| line.0 <= end);
        let mut tail = self.lines.split_off(last);
        self.lines.truncate(first);
        for line in &mut tail {
            line.0 = (line.0 as isize + byte_delta) as usize;
            line.1 = (line.1 as isize + char_delta) as usize;
        }
        let total = (self.chars as isize + char_delta) as usize;
        self.append(inserted, start, first_char);
        self.lines.extend(tail);
        self.chars = total;
    }
    pub fn lines(&self) -> usize {
        self.lines.len()
    }
    pub fn line_column(&self, text: &str, cursor: usize, width: usize) -> (usize, usize) {
        let cursor = cursor.min(self.chars);
        let line = self
            .lines
            .partition_point(|line| line.1 <= cursor)
            .saturating_sub(1);
        let (byte, chars) = self.lines[line];
        let col = text[byte..]
            .chars()
            .take(cursor - chars)
            .fold(0, |col, ch| {
                col + match ch {
                    '\r' => 0,
                    '\t' => width.max(1) - col % width.max(1),
                    _ => 1,
                }
            });
        (line, col)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incremental_edits_match_unicode_reference() {
        let mut text = "a🦀\tב\r\n\nlast\n".to_owned();
        let mut index = TextIndex::new(&text);
        let mut random = 17u64;
        for _ in 0..300 {
            for cursor in 0..=text.chars().count() + 1 {
                assert_eq!(
                    index.byte(&text, cursor),
                    hedit::advanced::byte(&text, cursor)
                );
                for width in [1, 4, 8] {
                    assert_eq!(
                        index.line_column(&text, cursor, width),
                        hedit::advanced::line_column(&text, cursor, width)
                    );
                }
            }
            assert_eq!(index.chars, text.chars().count());
            assert_eq!(
                index.lines(),
                text.bytes().filter(|b| *b == b'\n').count() + 1
            );
            let positions: Vec<_> = text
                .char_indices()
                .map(|(at, _)| at)
                .chain([text.len()])
                .collect();
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let a = random as usize % positions.len();
            let b = (random >> 32) as usize % positions.len();
            let insert = ["", "🦀\n", "\tשלום\r\n", "x", "\n\n"][random as usize % 5];
            let mut next = text.clone();
            next.replace_range(positions[a.min(b)]..positions[a.max(b)], insert);
            index.update(&text, &next);
            text = next;
        }
    }
}
