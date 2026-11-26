//! Clipboard behavior for lines: with nothing selected, Copy and Cut take the
//! whole line, and pasting such a line puts it above the cursor's line. Paste
//! and indent (Ctrl+Shift+V) fits pasted lines to the indentation where they land.
use super::*;
use egui::text::{ByteIndex, CharIndex};
use egui::TextBuffer as _;
use hedit::advanced::{self, Selection};

/// The line containing each cursor, once each, as character ranges including
/// the line break.
fn cursor_lines(text: &str, selections: &[Selection]) -> Vec<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let mut lines: Vec<(usize, usize)> = vec![];
    for s in selections {
        let mut start = s.head.min(chars.len());
        while start > 0 && chars[start - 1] != '\n' {
            start -= 1;
        }
        let mut end = s.head.min(chars.len());
        while end < chars.len() && chars[end] != '\n' {
            end += 1;
        }
        let end = (end + 1).min(chars.len());
        if lines.last() != Some(&(start, end)) {
            lines.push((start, end));
        }
    }
    lines
}

/// Indentation as a string of leading spaces and tabs.
fn indentation(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// Clipboard text may come from a document with different line endings.
/// Normalize only line separators, preserving lone carriage returns as text.
pub(super) fn line_endings(text: &str, crlf: bool) -> String {
    let text = text.replace("\r\n", "\n");
    if crlf {
        text.replace('\n', "\r\n")
    } else {
        text
    }
}

/// egui's End key can leave a cursor between CR and LF. Treat that as
/// the end of the line's content, so inserting text cannot split the pair.
pub(super) fn paste_selections(text: &str, mut selections: Vec<Selection>) -> Vec<Selection> {
    let chars: Vec<char> = text.chars().collect();
    for selection in &mut selections {
        for at in [&mut selection.anchor, &mut selection.head] {
            if *at > 0 && chars.get(*at - 1) == Some(&'\r') && chars.get(*at) == Some(&'\n') {
                *at -= 1;
            }
        }
    }
    selections
}

/// Keep ordinary edits in TextEdit's event order. Normalize inserted newlines
/// at the buffer boundary rather than applying Paste ahead of earlier keys.
pub(super) struct EditorText<'a, 'text> {
    pub text: &'a mut std::borrow::Cow<'text, str>,
    pub change: Option<(usize, usize, usize)>,
    pub crlf: bool,
    pub index: Option<Arc<super::editor_layout::TextIndex>>,
}

impl EditorText<'_, '_> {
    fn record(&mut self, start: usize, end: usize, inserted: usize) {
        if start == end && inserted == 0 {
            return;
        }
        self.change = Some(match self.change {
            Some((prior_start, old_end, new_end)) => (
                prior_start.min(start),
                old_end + end.saturating_sub(new_end),
                new_end.max(end) - (end - start) + inserted,
            ),
            None => (start, end, start + inserted),
        });
    }

    fn snap_cursor(&self, cursor: &mut egui::text::CCursor) {
        let at = self.byte_index_from_char_index(cursor.index).0;
        if at > 0
            && self.text.as_bytes().get(at - 1) == Some(&b'\r')
            && self.text.as_bytes().get(at) == Some(&b'\n')
        {
            cursor.index -= 1;
        }
    }
}

impl egui::TextBuffer for EditorText<'_, '_> {
    fn is_mutable(&self) -> bool {
        true
    }
    fn as_str(&self) -> &str {
        self.text.as_ref()
    }
    fn byte_index_from_char_index(&self, at: CharIndex) -> ByteIndex {
        ByteIndex(self.index.as_ref().map_or_else(
            || advanced::byte(self.text, at.0),
            |index| index.byte(self.text, at.0),
        ))
    }
    fn char_range(&self, range: std::ops::Range<CharIndex>) -> &str {
        &self.text[self.byte_index_from_char_index(range.start).0
            ..self.byte_index_from_char_index(range.end).0]
    }
    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<EditorText<'static, 'static>>()
    }
    fn insert_text(&mut self, text: &str, at: CharIndex) -> usize {
        let text = line_endings(text, self.crlf);
        let byte = self.byte_index_from_char_index(at).0;
        if let Some(index) = &mut self.index {
            let at = at.0.min(index.chars);
            Arc::make_mut(index).replace(self.text, byte..byte, at..at, &text);
        }
        self.record(byte, byte, text.len());
        if !text.is_empty() {
            self.text.to_mut().insert_str(byte, &text);
        }
        text.chars().count()
    }
    fn delete_char_range(&mut self, range: std::ops::Range<CharIndex>) {
        let start = self.byte_index_from_char_index(range.start).0;
        let end = self.byte_index_from_char_index(range.end).0;
        if let Some(index) = &mut self.index {
            let (a, b) = (range.start.0.min(index.chars), range.end.0.min(index.chars));
            Arc::make_mut(index).replace(self.text, start..end, a..b, "");
        }
        self.record(start, end, 0);
        if start != end {
            self.text.to_mut().replace_range(start..end, "");
        }
    }
    fn delete_selected_ccursor_range(
        &mut self,
        [mut min, mut max]: [egui::text::CCursor; 2],
    ) -> egui::text::CCursor {
        self.snap_cursor(&mut min);
        self.snap_cursor(&mut max);
        self.delete_char_range(min.index..max.index);
        egui::text::CCursor {
            index: min.index,
            prefer_next_row: true,
        }
    }
    fn delete_previous_char(&mut self, mut cursor: egui::text::CCursor) -> egui::text::CCursor {
        self.snap_cursor(&mut cursor);
        let at = self.byte_index_from_char_index(cursor.index).0;
        if self.text[..at].ends_with("\r\n") {
            self.delete_selected_ccursor_range([cursor - 2, cursor])
        } else if cursor.index == CharIndex::ZERO {
            cursor
        } else {
            self.delete_selected_ccursor_range([cursor - 1, cursor])
        }
    }
    fn delete_next_char(&mut self, mut cursor: egui::text::CCursor) -> egui::text::CCursor {
        self.snap_cursor(&mut cursor);
        let at = self.byte_index_from_char_index(cursor.index).0;
        if self.text[at..].starts_with("\r\n") {
            self.delete_selected_ccursor_range([cursor, cursor + 2])
        } else {
            self.delete_selected_ccursor_range([cursor, cursor + 1])
        }
    }
    fn insert_text_at(&mut self, cursor: &mut egui::text::CCursor, text: &str, limit: usize) {
        // End can leave the cursor between CR and LF. Insert before the pair
        // and let TextEdit track the actual inserted character count.
        self.snap_cursor(cursor);
        let text = line_endings(text, self.crlf);
        let end = if limit == usize::MAX {
            text.len()
        } else {
            let cutoff = limit.saturating_sub(
                self.index
                    .as_ref()
                    .map_or_else(|| self.text.chars().count(), |index| index.chars),
            );
            text.char_indices()
                .nth(cutoff)
                .map_or(text.len(), |(at, _)| at)
        };
        cursor.index += self.insert_text(&text[..end], cursor.index);
    }
}

/// Pasted text re-indented for a line indented by `target`. With `whole`, the
/// text goes at the start of a line: its common indentation becomes `target`
/// on every line, the first included. Otherwise it goes after other text on
/// the line, so its first line (often copied from mid-line) is left out of
/// the common indentation and pasted as it is, without leading spaces.
pub(super) fn reindent_paste(text: &str, target: &str, whole: bool, tab_width: usize) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let blank = |line: &str| {
        let content = line
            .strip_suffix("\r\n")
            .or_else(|| line.strip_suffix('\n'))
            .unwrap_or(line);
        content.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
    };
    let indents = lines
        .iter()
        .skip(usize::from(!whole))
        .filter(|line| !blank(line))
        .map(|l| indentation(l));
    let tab_width = tab_width.max(1);
    let column_after = |start, indent: &str| {
        indent.bytes().fold(start, |column, byte| {
            column
                + if byte == b'\t' {
                    tab_width - column % tab_width
                } else {
                    1
                }
        })
    };
    let common = indents
        .clone()
        .min_by_key(|indent| column_after(0, indent))
        .unwrap_or("");
    let baseline = column_after(0, common);
    let target_column = column_after(0, target);
    let mut out = String::with_capacity(text.len());
    for (n, line) in lines.iter().enumerate() {
        if (n == 0 && !whole) || blank(line) {
            out.push_str(line.trim_start_matches([' ', '\t']));
        } else {
            out.push_str(target);
            let indent = indentation(line);
            let relative = column_after(0, indent) - baseline;
            if let Some(suffix) = indent
                .strip_prefix(common)
                .filter(|suffix| column_after(target_column, suffix) == target_column + relative)
            {
                out.push_str(suffix);
            } else {
                out.extend(std::iter::repeat_n(' ', relative));
            }
            out.push_str(&line[indent.len()..]);
        }
    }
    out
}

impl HeditApp {
    fn all_empty(&self) -> bool {
        self.selections().iter().all(|s| s.start() == s.end())
    }
    /// Copy (or cut) the cursors' whole lines. False when it does not apply.
    pub(super) fn copy_lines(&mut self, ctx: &egui::Context, cut: bool) -> bool {
        self.line_clip = None;
        let b = self.active();
        if !self.settings.copy_with_empty_selection || b.page.info.paged || !self.all_empty() {
            return false;
        }
        let text = &b.page.text;
        let lines = cursor_lines(text, &self.selections());
        let mut copied = String::new();
        for &(start, end) in &lines {
            let a = advanced::byte(text, start);
            let z = a + advanced::byte(&text[a..], end - start);
            let line = &text[a..z];
            copied.push_str(line);
            if !line.ends_with('\n') {
                copied.push_str(if b.page.info.eol == "CRLF" {
                    "\r\n"
                } else {
                    "\n"
                });
            }
        }
        let cut = cut.then(|| {
            let ranges: Vec<Selection> = lines
                .iter()
                .map(|&(start, end)| Selection {
                    anchor: start,
                    head: end,
                })
                .collect();
            advanced::replace(text, &ranges, "")
        });
        let follow = b.follow;
        ctx.copy_text(copied.clone());
        // egui pastes with LF line breaks, so remember it that way.
        self.line_clip = Some(copied.replace("\r\n", "\n"));
        if let Some(batch) = cut.filter(|_| !follow) {
            self.batch(batch);
        }
        true
    }
    /// Paste a line copied by `copy_lines` above each cursor's line. False
    /// when the clipboard holds something else.
    fn paste_lines(&mut self, text: &str) -> bool {
        if self.line_clip.as_deref() != Some(text) || !self.all_empty() || self.active().follow {
            return false;
        }
        let b = self.active();
        let crlf = b.page.info.eol == "CRLF";
        let insert = line_endings(text, crlf);
        let selections = self.selections();
        let lines = cursor_lines(&b.page.text, &selections);
        let length = insert.chars().count();
        let mut out = String::with_capacity(b.page.text.len() + lines.len() * insert.len());
        let mut previous = 0;
        for &(start, _) in &lines {
            let a = advanced::byte(&b.page.text, start);
            out.push_str(&b.page.text[previous..a]);
            out.push_str(&insert);
            previous = a;
        }
        out.push_str(&b.page.text[previous..]);
        // Each cursor stays where it was in its line, below the pasted text.
        let cursors = selections
            .iter()
            .map(|s| {
                let above = lines.iter().filter(|(start, _)| *start <= s.head).count();
                Selection::caret(s.head + above * length)
            })
            .collect();
        self.batch(advanced::Batch {
            text: out,
            selections: cursors,
        });
        true
    }
    /// Paste `text` with its lines re-indented for the cursor's line.
    fn paste_indented(&mut self, text: &str) {
        let b = self.active();
        let selections = paste_selections(&b.page.text, self.selections());
        let start = selections.iter().map(|s| s.start()).min().unwrap_or(0);
        let at = advanced::byte(&b.page.text, start);
        let line = b.page.text[..at].rfind('\n').map_or(0, |n| n + 1);
        let target = indentation(&b.page.text[line..]).to_owned();
        // Only indentation before a single cursor: the text replaces it.
        let whole = selections.len() == 1 && b.page.text[line..at].trim().is_empty();
        let width = b.view.tab_size.unwrap_or(4) as usize;
        let pasted = reindent_paste(text, &target, whole, width);
        let pasted = line_endings(&pasted, b.page.info.eol == "CRLF");
        let ranges = if whole {
            let anchor = b.page.text[..line].chars().count();
            let selection = selections[0];
            let head = if selection.start() == selection.end() && pasted.ends_with('\n') {
                anchor
            } else {
                selection.end()
            };
            vec![Selection { anchor, head }]
        } else {
            selections
        };
        let batch = advanced::replace(&b.page.text, &ranges, &pasted);
        self.batch(batch);
    }
    /// Copy, Cut, and Paste events for the editor, before it handles them.
    pub(super) fn clipboard_events(&mut self, ctx: &egui::Context) {
        if self.active().page.info.paged {
            return;
        }
        // Ctrl+Shift+V; Shift+Insert, a plain paste on Windows, has no Ctrl.
        let shifted = ctx.input(|i| i.modifiers.shift && i.modifiers.command);
        let events = ctx.input(|i| i.events.clone());
        let mut handled = vec![];
        for (index, event) in events.iter().enumerate() {
            let done = match event {
                egui::Event::Copy => self.copy_lines(ctx, false),
                egui::Event::Cut => self.copy_lines(ctx, true),
                egui::Event::Paste(_) if self.busy || self.closing => false,
                egui::Event::Paste(text) if std::mem::take(&mut self.paste_indent) || shifted => {
                    if self.active().follow {
                        false
                    } else {
                        self.paste_indented(text);
                        true
                    }
                }
                egui::Event::Paste(text) => self.paste_lines(text),
                _ => false,
            };
            if done {
                handled.push(index);
            }
        }
        if !handled.is_empty() {
            ctx.input_mut(|i| {
                let mut index = 0;
                i.events.retain(|_| {
                    let keep = !handled.contains(&index);
                    index += 1;
                    keep
                });
            });
        }
    }
}

#[cfg(test)]
mod indexed_buffer_tests {
    use super::*;
    #[test]
    fn indexed_buffer_keeps_coordinates_current_across_ordered_unicode_edits() {
        let original = "a🦀\nשלום\nlast";
        let mut text = std::borrow::Cow::Borrowed(original);
        let mut buffer = EditorText {
            index: Some(Arc::new(editor_layout::TextIndex::new(&text))),
            text: &mut text,
            change: None,
            crlf: false,
        };
        let mut reference = buffer.as_str().to_owned();
        let mut random = 29u64;
        for _ in 0..200 {
            let count = reference.chars().count();
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let a = random as usize % (count + 1);
            let b = (random >> 32) as usize % (count + 1);
            if random & 1 == 0 {
                let insert = ["x", "🦀\n", "العربية\n"][random as usize % 3];
                buffer.insert_text(insert, CharIndex(a));
                reference.insert_text(insert, CharIndex(a));
            } else {
                buffer.delete_char_range(CharIndex(a.min(b))..CharIndex(a.max(b)));
                reference.delete_char_range(CharIndex(a.min(b))..CharIndex(a.max(b)));
            }
            assert_eq!(buffer.as_str(), reference);
            if let Some((start, old_end, new_end)) = buffer.change {
                assert_eq!(
                    format!(
                        "{}{}{}",
                        &original[..start],
                        &reference[start..new_end],
                        &original[old_end..]
                    ),
                    reference
                );
            }
            for cursor in 0..=reference.chars().count() + 1 {
                assert_eq!(
                    buffer.byte_index_from_char_index(CharIndex(cursor)).0,
                    advanced::byte(&reference, cursor)
                );
            }
            assert_eq!(
                buffer.index.as_ref().unwrap().chars,
                reference.chars().count()
            );
        }
    }
}
