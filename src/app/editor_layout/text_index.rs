//! Logical line offsets shared by the editor and status bar. Cursor coordinates
//! count Unicode scalars, just like TextEdit; byte offsets always stay on UTF-8 boundaries.
#[derive(Clone, Default)]
pub(in crate::app) struct TextIndex {
    lines: Vec<(usize, usize)>,
    pub chars: usize,
    chunks: Vec<Chunk>,
}
impl TextIndex {
    pub fn new(text: &str) -> Self {
        let mut this = Self {
            lines: vec![(0, 0)],
            chars: 0,
            chunks: chunks(text, 0, 0),
        };
        this.append(text, 0, 0);
        this.recount_columns(0);
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
        let checkpoint = self.chunks.partition_point(|c| c.byte <= byte);
        let (start, chars) = self.point(checkpoint);
        chars + text[start..byte].chars().count()
    }
    pub fn update(&mut self, old: &str, new: &str) {
        let (start, end, inserted) = hedit::workspace::splice(old, new);
        let first_char = self.char_at_byte(old, start);
        let last_char = self.char_at_byte(old, end);
        self.replace(old, start..end, first_char..last_char, inserted);
    }
    pub fn byte(&self, text: &str, cursor: usize) -> usize {
        let cursor = cursor.min(self.chars);
        let (byte, chars) = self.checkpoint(cursor);
        byte + text[byte..]
            .char_indices()
            .nth(cursor - chars)
            .map_or(text.len() - byte, |(at, _)| at)
    }
    /// One-based file coordinates, with UTF-16 columns as used by file navigation.
    pub fn navigation_position(&self, text: &str, cursor: usize) -> (usize, usize) {
        let cursor = cursor.min(self.chars);
        let line = self
            .lines
            .partition_point(|line| line.1 <= cursor)
            .saturating_sub(1);
        let prefix = &text[self.lines[line].0..self.byte(text, cursor)];
        (
            line + 1,
            prefix.trim_end_matches('\r').encode_utf16().count() + 1,
        )
    }

    /// A UTF-8 boundary within one small chunk before the requested character.
    pub fn checkpoint(&self, cursor: usize) -> (usize, usize) {
        self.point(
            self.chunks
                .partition_point(|c| c.chars <= cursor.min(self.chars)),
        )
    }
    fn point(&self, count: usize) -> (usize, usize) {
        count
            .checked_sub(1)
            .map_or((0, 0), |i| (self.chunks[i].byte, self.chunks[i].chars))
    }
    fn recount_columns(&mut self, first: usize) {
        let mut columns = first
            .checked_sub(1)
            .map_or([0; 8], |i| self.chunks[i].columns);
        for chunk in &mut self.chunks[first..] {
            for (i, col) in columns.iter_mut().enumerate() {
                *col = chunk.summary.advance(*col, i + 1);
            }
            chunk.columns = columns;
        }
    }
    pub fn replace(
        &mut self,
        old: &str,
        bytes: std::ops::Range<usize>,
        chars: std::ops::Range<usize>,
        inserted: &str,
    ) {
        let (start, end) = (bytes.start, bytes.end);
        let (first_char, last_char) = (chars.start, chars.end);
        let added = inserted.chars().count();
        let byte_delta = inserted.len() as isize - (end - start) as isize;
        let char_delta = added as isize - (last_char - first_char) as isize;
        // Rebalance the affected chunks and one neighbor on either side. All
        // surviving interior chunks stay at least half full, even after many edits.
        let first_chunk = self
            .chunks
            .partition_point(|c| c.byte < start)
            .saturating_sub(1);
        let last_chunk = (self.chunks.partition_point(|c| c.byte < end) + 2).min(self.chunks.len());
        let (from, before) = self.point(first_chunk);
        let (to, _) = self.point(last_chunk);
        let mut changed = String::with_capacity(start - from + inserted.len() + to - end);
        changed.push_str(&old[from..start]);
        changed.push_str(inserted);
        changed.push_str(&old[end..to]);
        let replacement = chunks(&changed, from, before);
        for chunk in &mut self.chunks[last_chunk..] {
            chunk.byte = chunk.byte.checked_add_signed(byte_delta).unwrap();
            chunk.chars = chunk.chars.checked_add_signed(char_delta).unwrap();
        }
        self.chunks.splice(first_chunk..last_chunk, replacement);
        if self.chunks.capacity() > (self.chunks.len() * 2).max(16) {
            self.chunks.shrink_to_fit();
        }
        self.recount_columns(first_chunk);
        let first = self.lines.partition_point(|line| line.0 <= start);
        let last = self.lines.partition_point(|line| line.0 <= end);
        let mut tail = self.lines.split_off(last);
        self.lines.truncate(first);
        for line in &mut tail {
            line.0 = line.0.checked_add_signed(byte_delta).unwrap();
            line.1 = line.1.checked_add_signed(char_delta).unwrap();
        }
        let total = self.chars.checked_add_signed(char_delta).unwrap();
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
        let (mut byte, mut chars) = self.lines[line];
        let mut col = 0;
        if (1..=8).contains(&width) {
            let point = self.chunks.partition_point(|c| c.chars <= cursor);
            if let Some(chunk) = point
                .checked_sub(1)
                .map(|i| &self.chunks[i])
                .filter(|c| c.byte >= byte)
            {
                byte = chunk.byte;
                chars = chunk.chars;
                col = chunk.columns[width - 1];
            }
        }
        col = text[byte..]
            .chars()
            .take(cursor - chars)
            .fold(col, |col, ch| {
                col + match ch {
                    '\r' => 0,
                    '\t' => width.max(1) - col % width.max(1),
                    _ => 1,
                }
            });
        (line, col)
    }
}
// Only derived scalar/byte/column metadata: no text, disk cache, or per-character table.
const CHUNK_CHARS: usize = 256;
#[derive(Clone)]
struct Chunk {
    byte: usize,
    chars: usize,
    columns: [usize; 8],
    summary: Columns,
}
#[derive(Clone, Default)]
struct Columns {
    zero: [u16; 8],
    prefix: u16,
    tab: bool,
    newline: bool,
}
impl Columns {
    fn new(text: &str) -> Self {
        let mut this = Self::default();
        let mut plain = 0;
        for ch in text.chars() {
            match ch {
                '\r' => {}
                '\n' => {
                    this.newline = true;
                    this.zero = [0; 8];
                    plain = 0;
                }
                '\t' => {
                    if !this.tab {
                        this.prefix = plain;
                    }
                    this.tab = true;
                    for (i, col) in this.zero.iter_mut().enumerate() {
                        let width = i as u16 + 1;
                        *col += plain;
                        *col += width - *col % width;
                    }
                    plain = 0;
                }
                _ => plain += 1,
            }
        }
        for col in &mut this.zero {
            *col += plain;
        }
        this
    }
    fn advance(&self, column: usize, width: usize) -> usize {
        let zero = self.zero[width - 1] as usize;
        if self.newline {
            zero
        } else if self.tab {
            // The first tab rounds the incoming column to a tab stop.
            // Later tabs depend only on this aligned base; a newline resets it.
            let prefix = self.prefix as usize;
            zero + ((column + prefix) / width - prefix / width) * width
        } else {
            column + zero
        }
    }
}
fn chunks(text: &str, byte: usize, chars: usize) -> Vec<Chunk> {
    let count = text.chars().count();
    if count == 0 {
        return vec![];
    }
    let n = count.div_ceil(CHUNK_CHARS);
    let (size, extra) = (count / n, count % n);
    let mut result = Vec::with_capacity(n);
    let (mut start, mut in_chunk, mut consumed) = (0, 0, 0);
    for (at, ch) in text.char_indices() {
        in_chunk += 1;
        if in_chunk == size + usize::from(result.len() < extra) {
            let end = at + ch.len_utf8();
            consumed += in_chunk;
            result.push(Chunk {
                byte: byte + end,
                chars: chars + consumed,
                columns: [0; 8],
                summary: Columns::new(&text[start..end]),
            });
            start = end;
            in_chunk = 0;
        }
    }
    result
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

#[cfg(test)]
mod long_tests {
    use super::*;
    fn check(index: &TextIndex, text: &str, cursors: &[usize]) {
        assert_eq!(index.chars, text.chars().count());
        assert_eq!(
            index.lines(),
            text.bytes().filter(|&b| b == b'\n').count() + 1
        );
        let mut previous = 0;
        for (i, chunk) in index.chunks.iter().enumerate() {
            let size = chunk.chars - previous;
            assert!((1..=CHUNK_CHARS).contains(&size));
            if i + 1 < index.chunks.len() {
                assert!(
                    size >= CHUNK_CHARS / 2,
                    "interior chunk must remain half full: {size}"
                );
            }
            assert!(text.is_char_boundary(chunk.byte));
            previous = chunk.chars;
        }
        assert!(index.chunks.len() <= index.chars.div_ceil(CHUNK_CHARS / 2));
        assert!(index.chunks.capacity() <= (index.chunks.len() * 2).max(16));
        for &cursor in cursors {
            assert_eq!(
                index.byte(text, cursor),
                hedit::advanced::byte(text, cursor)
            );
            let (byte, chars) = index.checkpoint(cursor);
            assert!(text.is_char_boundary(byte));
            assert!(cursor.min(index.chars) - chars <= CHUNK_CHARS);
            for width in [0, 1, 2, 3, 4, 5, 6, 7, 8, 16] {
                assert_eq!(
                    index.line_column(text, cursor, width),
                    hedit::advanced::line_column(text, cursor, width.max(1)),
                    "cursor {cursor}, width {width}"
                );
            }
        }
    }
    #[test]
    fn sparse_checkpoints_match_scalar_walks_across_tabs_crlf_and_chunk_edges() {
        let text = format!(
            "head\r\n{}\n{}",
            "α🦀\tאב\r".repeat(200),
            "\tplain text\t".repeat(100)
        );
        let index = TextIndex::new(&text);
        let mut cursors: Vec<_> = index
            .chunks
            .iter()
            .flat_map(|c| [c.chars.saturating_sub(1), c.chars, c.chars + 1])
            .collect();
        cursors.extend([0, 1, index.chars + 10]);
        check(&index, &text, &cursors);
    }
    #[test]
    fn incremental_chunk_rebalancing_preserves_unicode_columns_and_snapshot_isolation() {
        let mut text = format!("prefix\n{}\r\ntail", "α🦀\tאב\r".repeat(800));
        let mut index = TextIndex::new(&text);
        let mut random = 41u64;
        for step in 0..200 {
            let original = index.clone();
            let old = text.clone();
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let a = random as usize % (index.chars + 1);
            let b = (a + (random >> 32) as usize % 400).min(index.chars);
            let start = hedit::advanced::byte(&text, a);
            let end = hedit::advanced::byte(&text, b);
            let inserted = ["", "🦀\r\n", "\tשלום\r", "x", "ab\n\t"][step % 5];
            text.replace_range(start..end, inserted);
            // Exercise the direct replacement API used by TextBuffer, then
            // the independently calculated minimal splice used by projections.
            if step % 2 == 0 {
                index.replace(&old, start..end, a..b, inserted);
            } else {
                index.update(&old, &text);
            }
            check(
                &index,
                &text,
                &[
                    0,
                    127,
                    128,
                    255,
                    256,
                    a,
                    a + 1,
                    index.chars / 2,
                    index.chars,
                    index.chars + 1,
                ],
            );
            check(&original, &old, &[a, b]);
        }
        index.replace(&text, 0..text.len(), 0..index.chars, "");
        text.clear();
        check(&index, &text, &[0, 1]);
        for _ in 0..1000 {
            index.replace(
                &text,
                text.len()..text.len(),
                index.chars..index.chars,
                "🦀\t",
            );
            text.push_str("🦀\t");
        }
        check(
            &index,
            &text,
            &[1, 255, 256, 1025, index.chars - 1, index.chars],
        );
    }
}
