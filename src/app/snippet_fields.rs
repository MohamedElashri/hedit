//! Snippet fields: after a snippet with `$1`, `${2:text}` … is inserted, Tab
//! and Shift+Tab move between its fields until the last one, Escape, or a
//! cursor leaving the field ends it.
use super::*;
use hedit::advanced::Selection;
use similar::DiffOp;

/// Changes larger than this end field navigation rather than being diffed.
const MAX_DIFF: usize = 20_000;

/// End can place the editor cursor between CR and LF. Snippets insert
/// before the complete separator and match triggers against content only.
pub(super) fn insertion_byte(text: &str, character: usize) -> usize {
    let at = hedit::advanced::byte(text, character);
    if at > 0
        && text.as_bytes().get(at - 1) == Some(&b'\r')
        && text.as_bytes().get(at) == Some(&b'\n')
    {
        at - 1
    } else {
        at
    }
}

pub(super) struct SnippetSession {
    view: u64,
    /// The text that `fields` refers to.
    text: String,
    /// Each field's places, as character ranges in `text`; the last field is
    /// where the cursor ends.
    fields: Vec<Vec<(usize, usize)>>,
    current: usize,
}

/// Where a position between characters of `old` lands in `new`, given the
/// diff of the changed region. `after` keeps it after text inserted there.
fn map(position: usize, ops: &[DiffOp], after: bool) -> usize {
    let mut found: Option<usize> = None;
    for op in ops {
        let (o0, o1, n0, n1) = match *op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                if (old_index..=old_index + len).contains(&position) {
                    let at = new_index + position - old_index;
                    found = Some(match found {
                        Some(f) if after => f.max(at),
                        Some(f) => f.min(at),
                        None => at,
                    });
                }
                continue;
            }
            DiffOp::Delete {
                old_index,
                old_len,
                new_index,
            } => (old_index, old_index + old_len, new_index, new_index),
            DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => (old_index, old_index, new_index, new_index + new_len),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => (
                old_index,
                old_index + old_len,
                new_index,
                new_index + new_len,
            ),
        };
        if (o0..=o1).contains(&position) {
            let at = if after { n1 } else { n0 };
            found = Some(match found {
                Some(f) if after => f.max(at),
                Some(f) => f.min(at),
                None => at,
            });
        }
    }
    found.unwrap_or(position)
}

/// Move field ranges from `old` to `new`. The current field grows with text
/// typed at its edges; other fields do not. None when the change is too large.
fn map_fields(
    old: &str,
    new: &str,
    fields: &[Vec<(usize, usize)>],
    current: usize,
) -> Option<Vec<Vec<(usize, usize)>>> {
    let old: Vec<char> = old.chars().collect();
    let new: Vec<char> = new.chars().collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let room = old.len().min(new.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    let (old_end, new_end) = (old.len() - suffix, new.len() - suffix);
    if old_end - prefix + new_end - prefix > MAX_DIFF {
        return None;
    }
    let ops = similar::capture_diff_slices(
        similar::Algorithm::Myers,
        &old[prefix..old_end],
        &new[prefix..new_end],
    );
    let shift = |position: usize, after: bool| {
        if position < prefix {
            position
        } else if position > old_end {
            position + new_end - old_end
        } else {
            prefix + map(position - prefix, &ops, after)
        }
    };
    Some(
        fields
            .iter()
            .enumerate()
            .map(|(index, places)| {
                places
                    .iter()
                    .map(|&(start, end)| {
                        let (start, end) = if index == current {
                            (shift(start, false), shift(end, true))
                        } else if start == end {
                            let at = shift(start, true);
                            (at, at)
                        } else {
                            (shift(start, true), shift(end, false))
                        };
                        (start.min(end), end)
                    })
                    .collect()
            })
            .collect(),
    )
}

/// Indent a snippet's later lines like the line it is inserted on, with
/// leading tabs turned into the file's indentation.
pub(super) fn indent_body(body: &str, indent: &str, unit: &str) -> String {
    body.split('\n')
        .enumerate()
        .map(|(index, line)| {
            let tabs = line.chars().take_while(|&c| c == '\t').count();
            let line = format!("{}{}", unit.repeat(tabs), &line[tabs..]);
            if index == 0 {
                line
            } else {
                format!("{indent}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl HeditApp {
    /// Start field navigation for a snippet inserted at `base`.
    pub(super) fn start_snippet(&mut self, base: usize, fields: Vec<Vec<(usize, usize)>>) {
        let fields: Vec<Vec<(usize, usize)>> = fields
            .into_iter()
            .map(|places| {
                places
                    .into_iter()
                    .map(|(a, b)| (base + a, base + b))
                    .collect()
            })
            .collect();
        let b = self.active();
        self.snippet = Some(SnippetSession {
            view: b.view.id,
            text: b.page.text.clone(),
            fields,
            current: 0,
        });
        self.select_field();
    }
    fn select_field(&mut self) {
        let Some(session) = &self.snippet else {
            return;
        };
        let ranges = session.fields[session.current]
            .iter()
            .map(|&(a, b)| Selection { anchor: a, head: b })
            .collect();
        let last = session.current + 1 == session.fields.len();
        self.set_selections(ranges);
        if last {
            self.snippet = None;
        } else if let Some(session) = &mut self.snippet {
            // Selecting may not change the text, but keep it current.
            session.text = self.workspace.open[self.workspace.active].page.text.clone();
        }
    }
    /// Follow edits, and end field navigation when it no longer applies.
    pub(super) fn sync_snippet(&mut self) {
        let Some(session) = &mut self.snippet else {
            return;
        };
        let b = &self.workspace.open[self.workspace.active];
        if b.view.id != session.view {
            self.snippet = None;
            return;
        }
        if b.page.text != session.text {
            match map_fields(
                &session.text,
                &b.page.text,
                &session.fields,
                session.current,
            ) {
                Some(fields) => {
                    session.fields = fields;
                    session.text = b.page.text.clone();
                }
                None => {
                    self.snippet = None;
                    return;
                }
            }
        }
        let places = session.fields[session.current].clone();
        let inside = self
            .selections()
            .iter()
            .all(|s| places.iter().any(|&(a, b)| a <= s.start() && s.end() <= b));
        if !inside {
            self.snippet = None;
        }
    }
    /// Tab or Shift+Tab during field navigation. False when there is none.
    pub(super) fn next_field(&mut self, back: bool) -> bool {
        self.sync_snippet();
        let Some(session) = &mut self.snippet else {
            return false;
        };
        if back {
            session.current = session.current.saturating_sub(1);
        } else {
            session.current += 1;
        }
        self.select_field();
        true
    }
}

#[cfg(test)]
mod newline_tests {
    use super::*;

    #[test]
    fn crlf_snippet_triggers_expand_at_the_end_key_position() {
        let dir = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(dir.path().to_owned()).unwrap();
        workspace.open[0].page.text = "audit_snip\r\n".into();
        workspace.edit(0, "").unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.edit_command(hedit::editing::Operation::ToCrlf);
        app.quality.config.snippets = vec![hedit::intelligence::Snippet {
            trigger: "audit_snip".into(),
            description: String::new(),
            body: "${1:one}\n${2:two}$0".into(),
            language: None,
        }];
        app.set_selections(vec![Selection::caret("audit_snip\r".chars().count())]);
        assert!(app.expand_trigger());
        assert_eq!(app.active().page.text, "one\r\ntwo\r\n");
        assert_eq!(app.selections(), [Selection { anchor: 0, head: 3 }]);
    }

    #[test]
    fn multiline_snippets_keep_document_endings_and_field_positions() {
        for crlf in [false, true] {
            for body in ["${1:one}\n${2:two}$0", "${1:one}\r\n${2:two}$0"] {
                let dir = tempfile::tempdir().unwrap();
                let (mut workspace, _) = Workspace::restore(dir.path().to_owned()).unwrap();
                workspace.open[0].page.text = "  \n".into();
                workspace.edit(0, "").unwrap();
                let ctx = egui::Context::default();
                let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
                if crlf {
                    app.edit_command(hedit::editing::Operation::ToCrlf);
                }
                let original = app.active().page.text.clone();
                app.set_selections(vec![Selection::caret(2)]);
                app.insert_snippet(
                    hedit::intelligence::Snippet {
                        trigger: String::new(),
                        description: String::new(),
                        body: body.into(),
                        language: None,
                    },
                    0,
                );
                let eol = if crlf { "\r\n" } else { "\n" };
                assert_eq!(app.active().page.text, format!("  one{eol}  two{eol}"));
                for field in ["one", "two"] {
                    let range = app.selections()[0];
                    let text = &app.active().page.text;
                    assert_eq!(
                        &text[hedit::advanced::byte(text, range.start())
                            ..hedit::advanced::byte(text, range.end())],
                        field
                    );
                    assert!(app.next_field(false));
                }
                assert!(app.snippet.is_none());
                app.history(&ctx, false);
                tests::finish_app_job(&mut app, &ctx);
                assert_eq!(app.active().page.text, original);
            }
        }
    }
}
