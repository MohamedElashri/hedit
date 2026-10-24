//! Clipboard behavior for lines: with nothing selected, Copy and Cut take the
//! whole line, and pasting such a line puts it above the cursor's line. Paste
//! and indent (Ctrl+Shift+V) fits pasted lines to the indentation where they land.
use super::*;
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

/// Pasted text re-indented for a line indented by `target`. With `whole`, the
/// text goes at the start of a line: its common indentation becomes `target`
/// on every line, the first included. Otherwise it goes after other text on
/// the line, so its first line (often copied from mid-line) is left out of
/// the common indentation and pasted as it is, without leading spaces.
pub(super) fn reindent_paste(text: &str, target: &str, whole: bool) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let common = lines
        .iter()
        .skip(usize::from(!whole))
        .filter(|l| !l.trim().is_empty())
        .map(|l| indentation(l))
        .min_by_key(|i| i.len())
        .unwrap_or("");
    let mut out = String::with_capacity(text.len());
    for (n, line) in lines.iter().enumerate() {
        if n == 0 && !whole {
            out.push_str(line.trim_start_matches([' ', '\t']));
        } else if line.trim().is_empty() {
            // Blank lines keep only their line break.
            out.push_str(&line[line.trim_end_matches(['\r', '\n']).len()..]);
        } else {
            out.push_str(target);
            out.push_str(
                line.strip_prefix(common)
                    .unwrap_or(line.trim_start_matches([' ', '\t'])),
            );
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
            copied.push_str(&text[a..z]);
            if !copied.ends_with('\n') {
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
        let insert = if crlf {
            text.replace('\n', "\r\n")
        } else {
            text.to_owned()
        };
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
        let selections = self.selections();
        let start = selections.iter().map(|s| s.start()).min().unwrap_or(0);
        let at = advanced::byte(&b.page.text, start);
        let line = b.page.text[..at].rfind('\n').map_or(0, |n| n + 1);
        let target = indentation(&b.page.text[line..]).to_owned();
        // Only indentation before a single cursor: the text replaces it.
        let whole = selections.len() == 1 && b.page.text[line..at].trim().is_empty();
        let pasted = reindent_paste(text, &target, whole);
        let ranges = if whole {
            vec![Selection {
                anchor: b.page.text[..line].chars().count(),
                head: selections[0].end(),
            }]
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
