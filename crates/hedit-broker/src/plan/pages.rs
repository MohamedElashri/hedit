//! Index complete previews during plan preparation, never while drawing frames.
pub(crate) struct Pages(Vec<usize>);
pub(crate) const INDEX_BYTES: usize =
    2 * super::MAX_FILE_BYTES.div_ceil(1024) * std::mem::size_of::<usize>();
impl Pages {
    pub(crate) fn new(text: &str) -> Self {
        let mut offsets = Vec::with_capacity(text.len().div_ceil(1024).max(1));
        offsets.push(0);
        let mut count = 0;
        let mut previous = None;
        for (at, ch) in text.char_indices() {
            if count >= 1024 && !(previous == Some('\r') && ch == '\n') {
                offsets.push(at);
                count = 0;
            }
            count += 1;
            previous = Some(ch);
        }
        Self(offsets)
    }
    pub(crate) fn get<'a>(&self, text: &'a str, page: usize) -> (&'a str, usize) {
        let page = page.min(self.0.len() - 1);
        let start = self.0[page];
        let end = self.0.get(page + 1).copied().unwrap_or(text.len());
        (&text[start..end], self.0.len())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn previews_reconstruct_every_byte_without_splitting_scalars_or_crlf() {
        for text in [
            String::new(),
            "x".repeat(super::super::MAX_FILE_BYTES),
            format!(
                "{}\r\n{}",
                "x".repeat(1023),
                "café / 東京 / 🦀\r\n".repeat(4000)
            ),
        ] {
            let pages = Pages::new(&text);
            let mut joined = String::new();
            for index in 0..pages.0.len() {
                let (part, count) = pages.get(&text, index);
                assert_eq!(count, pages.0.len());
                assert!(part.chars().count() <= 1025);
                assert!(!part.ends_with('\r') || joined.len() + part.len() == text.len());
                joined.push_str(part);
            }
            assert_eq!(joined, text);
            assert_eq!(
                pages.get(&text, usize::MAX).0,
                pages.get(&text, pages.0.len() - 1).0
            );
        }
    }
}
