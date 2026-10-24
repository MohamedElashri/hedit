use super::*;
use hedit::advanced::{self, Batch, Selection};

impl HeditApp {
    pub(super) fn selections(&self) -> Vec<Selection> {
        let b = self.active();
        let mut ranges = vec![Selection {
            anchor: b.view.anchor,
            head: b.view.head,
        }];
        ranges.extend_from_slice(&b.view.selections);
        advanced::normalize(&ranges, b.page.text.chars().count())
    }
    pub(super) fn set_selections(&mut self, mut ranges: Vec<Selection>) {
        let index = self.workspace.active;
        let b = &mut self.workspace.open[index];
        ranges = advanced::normalize(&ranges, b.page.text.chars().count());
        let primary = ranges.pop().unwrap_or_default();
        b.view.anchor = primary.anchor;
        b.view.head = primary.head;
        b.view.selections = ranges;
        b.restore_cursor = true;
        b.reveal_cursor = true;
        self.focus_editor = true;
        self.changed();
    }
    pub(super) fn batch(&mut self, batch: Batch) {
        let crlf = None;
        let primary = batch.selections.last().copied().unwrap_or_default();
        let success = self.apply_edit(
            hedit::editing::Edit {
                text: batch.text,
                anchor: primary.anchor,
                head: primary.head,
            },
            crlf,
        );
        if success {
            self.set_selections(batch.selections);
        }
    }
    /// Record each selection change, for Undo selection change (Sublime's soft undo).
    pub(super) fn track_selection(&mut self) {
        let current = (self.active().view.id, self.selections());
        let (undo, redo, last) = &mut self.soft_history;
        if last.as_ref() == Some(&current) {
            return;
        }
        if let Some(previous) = last.replace(current) {
            undo.push(previous);
            if undo.len() > 200 {
                undo.remove(0);
            }
            redo.clear();
        }
    }
    /// Step back (or forward) through selection changes in the active file.
    pub(super) fn soft_undo(&mut self, redo: bool) {
        let view = self.active().view.id;
        let (back, forward, last) = &mut self.soft_history;
        let (from, to) = if redo {
            (forward, back)
        } else {
            (back, forward)
        };
        while let Some((id, cursors)) = from.pop() {
            if id != view {
                continue;
            }
            if let Some(current) = last.take() {
                to.push(current);
            }
            *last = Some((id, cursors.clone()));
            self.set_selections(cursors);
            return;
        }
    }
    /// Add a cursor on the line above or below. Repeated presses keep the
    /// first cursor's column, even past shorter lines.
    pub(super) fn add_line_cursor(&mut self, below: bool) {
        let b = self.active();
        let width = b.view.tab_size.unwrap_or(4) as usize;
        let current = self.selections();
        let goal = match &self.line_cursor_goal {
            Some((selections, column)) if *selections == current => *column,
            _ => advanced::line_column(&b.page.text, b.view.head, width).1,
        };
        let ranges =
            advanced::add_line_selections(&b.page.text, &current, below, width, Some(goal));
        self.set_selections(ranges);
        // Show the newest cursor: the first one when adding above.
        if !below {
            let b = &mut self.workspace.open[self.workspace.active];
            if let Some(top) = b.view.selections.first_mut() {
                let primary = Selection {
                    anchor: b.view.anchor,
                    head: b.view.head,
                };
                (b.view.anchor, b.view.head) = (top.anchor, top.head);
                *top = primary;
            }
        }
        self.line_cursor_goal = Some((self.selections(), goal));
    }
    pub(super) fn occurrence(&mut self, all: bool) {
        let ranges = advanced::occurrences(&self.active().page.text, &self.selections(), all);
        self.set_selections(ranges);
        if self.selections().len() == advanced::MAX_SELECTIONS {
            self.notice = "Selection limit: 1,000 cursors.".into();
        }
    }
    pub(super) fn detect_indent(&mut self) {
        let (tabs, width) = advanced::detect_indentation(&self.active().page.text);
        let b = &mut self.workspace.open[self.workspace.active];
        b.view.indent = Some(if tabs { "tabs" } else { "spaces" }.into());
        b.view.tab_size = Some(width);
        self.changed();
    }
    pub(super) fn fold(&mut self, all: bool) {
        let b = &mut self.workspace.open[self.workspace.active];
        let width = b.view.tab_size.unwrap_or(4) as usize;
        let candidates = advanced::indentation_folds(&b.page.text, width);
        if all {
            b.view.folds = candidates;
        } else {
            let a = b.view.anchor.min(b.view.head);
            let z = b.view.anchor.max(b.view.head);
            let selected = if a != z {
                Some(advanced::Fold { start: a, end: z })
            } else {
                candidates
                    .into_iter()
                    .filter(|f| {
                        let before = &b.page.text[..advanced::byte(&b.page.text, f.start)];
                        let header = before
                            .trim_end_matches(['\r', '\n'])
                            .rfind('\n')
                            .map_or(0, |i| before[..=i].chars().count());
                        header <= a && a < f.end
                    })
                    .min_by_key(|f| f.end - f.start)
            };
            if let Some(fold) = selected {
                b.view.folds.push(fold);
                b.view.anchor = fold.start;
                b.view.head = fold.start;
            }
        }
        b.view.selections.clear();
        b.restore_cursor = true;
        self.changed();
    }
    pub(super) fn advanced_input(&mut self, ctx: &egui::Context) {
        let multiple = !self.active().view.selections.is_empty();
        let readonly = self.active().follow || self.busy || self.closing;
        let paged = self.active().page.info.paged;
        let events = ctx.input(|i| i.events.clone());
        let mut consumed = vec![];
        for (i, event) in events.iter().enumerate() {
            if (readonly || paged) && !matches!(event, egui::Event::Copy | egui::Event::Cut) {
                continue;
            }
            let mut replacement = None;
            if let egui::Event::Text(typed) = event {
                if let Some(batch) = self.auto_pair(typed) {
                    self.batch(batch);
                    consumed.push(i);
                    continue;
                }
            }
            match event {
                egui::Event::Text(text) | egui::Event::Paste(text) if multiple => {
                    replacement = Some(text.clone())
                }
                egui::Event::Ime(egui::ImeEvent::Commit(text)) if multiple => {
                    replacement = Some(text.clone())
                }
                egui::Event::Ime(_) if multiple => consumed.push(i),
                egui::Event::Copy | egui::Event::Cut => {
                    let ranges = self.selections();
                    let source = &self.active().page.text;
                    ctx.copy_text(
                        ranges
                            .iter()
                            .map(|s| {
                                source[advanced::byte(source, s.start())
                                    ..advanced::byte(source, s.end())]
                                    .to_owned()
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                    if matches!(event, egui::Event::Cut) && !readonly {
                        replacement = Some(String::new());
                    } else {
                        consumed.push(i);
                    }
                }
                egui::Event::Key {
                    key: Key::Enter,
                    pressed: true,
                    modifiers,
                    ..
                } if !modifiers.command && !modifiers.alt => {
                    if self.settings.auto_indent {
                        self.insert_smart_newline();
                        consumed.push(i);
                    } else if multiple {
                        replacement = Some(
                            if self.active().page.info.eol == "CRLF" {
                                "\r\n"
                            } else {
                                "\n"
                            }
                            .into(),
                        );
                    }
                }
                egui::Event::Key {
                    key: Key::Backspace,
                    pressed: true,
                    modifiers,
                    ..
                } if multiple || (!modifiers.command && !modifiers.alt) => {
                    let text = &self.active().page.text;
                    let width = self.active().view.tab_size.unwrap_or(4) as usize;
                    let mut ranges = self.selections();
                    let mut special = false;
                    for s in &mut ranges {
                        if s.start() == s.end() && s.head > 0 {
                            if modifiers.mac_cmd {
                                let (line, _) = advanced::line_column(text, s.head, width);
                                s.anchor = advanced::at_column(text, line, 0, width);
                            } else if modifiers.command || modifiers.ctrl || modifiers.alt {
                                s.anchor = advanced::word_boundary(text, s.head, false);
                            } else if self.settings.auto_match_enabled
                                && advanced::inside_empty_pair(text, s.head)
                            {
                                // Backspace in `(|)` removes both halves.
                                special = true;
                                s.anchor = s.head - 1;
                                s.head += 1;
                            } else {
                                let before: String = text.chars().take(s.head).collect();
                                let leading = before.rsplit('\n').next().unwrap_or("");
                                let n = if before.ends_with("\r\n") {
                                    special = true;
                                    2
                                } else if before.ends_with('\t') {
                                    special = true;
                                    1
                                } else if self.settings.smart_backspace
                                    && !leading.is_empty()
                                    && leading.chars().all(|c| c == ' ')
                                {
                                    special = true;
                                    let n = leading.len();
                                    if n.is_multiple_of(width) {
                                        width.min(n)
                                    } else {
                                        n % width
                                    }
                                } else {
                                    1
                                };
                                s.anchor = s.head.saturating_sub(n);
                            }
                        }
                    }
                    if multiple || special {
                        self.batch(advanced::replace(text, &ranges, ""));
                        consumed.push(i);
                    }
                }
                egui::Event::Key {
                    key: Key::Delete,
                    pressed: true,
                    modifiers,
                    ..
                } if multiple
                    || (!modifiers.command
                        && !modifiers.alt
                        && self
                            .active()
                            .page
                            .text
                            .chars()
                            .skip(self.active().view.head)
                            .take(2)
                            .collect::<String>()
                            == "\r\n") =>
                {
                    let text = &self.active().page.text;
                    let width = self.active().view.tab_size.unwrap_or(4) as usize;
                    let mut ranges = self.selections();
                    for s in &mut ranges {
                        if s.start() == s.end() {
                            s.head = if modifiers.mac_cmd {
                                let (line, _) = advanced::line_column(text, s.head, width);
                                advanced::at_column(text, line, usize::MAX, width)
                            } else if modifiers.command || modifiers.ctrl || modifiers.alt {
                                advanced::word_boundary(text, s.head, true)
                            } else {
                                (s.head
                                    + if text.chars().skip(s.head).take(2).collect::<String>()
                                        == "\r\n"
                                    {
                                        2
                                    } else {
                                        1
                                    })
                                .min(text.chars().count())
                            };
                        }
                    }
                    self.batch(advanced::replace(text, &ranges, ""));
                    consumed.push(i);
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if (multiple
                    || (*key == Key::ArrowRight
                        && self.active().page.text.chars().nth(self.active().view.head)
                            == Some('\t')))
                    && matches!(
                        key,
                        Key::ArrowLeft
                            | Key::ArrowRight
                            | Key::ArrowUp
                            | Key::ArrowDown
                            | Key::Home
                            | Key::End
                    ) =>
                {
                    let text = &self.active().page.text;
                    let length = text.chars().count();
                    let width = self.active().view.tab_size.unwrap_or(4) as usize;
                    let ranges = self
                        .selections()
                        .into_iter()
                        .map(|s| {
                            let (line, col) = advanced::line_column(text, s.head, width);
                            let head = match key {
                                Key::ArrowLeft => {
                                    if !modifiers.shift && s.start() != s.end() {
                                        s.start()
                                    } else if modifiers.mac_cmd {
                                        advanced::at_column(text, line, 0, width)
                                    } else if modifiers.command || modifiers.ctrl || modifiers.alt {
                                        advanced::word_boundary(text, s.head, false)
                                    } else {
                                        s.head.saturating_sub(1)
                                    }
                                }
                                Key::ArrowRight => {
                                    if !modifiers.shift && s.start() != s.end() {
                                        s.end()
                                    } else if modifiers.mac_cmd {
                                        advanced::at_column(text, line, usize::MAX, width)
                                    } else if modifiers.command || modifiers.ctrl || modifiers.alt {
                                        advanced::word_boundary(text, s.head, true)
                                    } else {
                                        (s.head + 1).min(length)
                                    }
                                }
                                Key::ArrowUp => {
                                    advanced::at_column(text, line.saturating_sub(1), col, width)
                                }
                                Key::ArrowDown => advanced::at_column(text, line + 1, col, width),
                                Key::Home => advanced::at_column(text, line, 0, width),
                                _ => advanced::at_column(text, line, usize::MAX, width),
                            };
                            Selection {
                                anchor: if modifiers.shift { s.anchor } else { head },
                                head,
                            }
                        })
                        .collect();
                    self.set_selections(ranges);
                    consumed.push(i);
                }
                _ => {}
            }
            if let Some(text) = replacement {
                let edit = advanced::replace(&self.active().page.text, &self.selections(), &text);
                self.batch(edit);
                consumed.push(i);
            }
        }
        ctx.input_mut(|input| {
            let mut index = 0;
            input.events.retain(|_| {
                let keep = !consumed.contains(&index);
                index += 1;
                keep
            });
        });
    }
    /// Bracket and quote pairing for one typed character, when it applies.
    fn auto_pair(&self, typed: &str) -> Option<Batch> {
        let mut chars = typed.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return None;
        };
        if !self.settings.auto_match_enabled {
            return None;
        }
        use syntax::Language::*;
        // Apostrophes in prose and Rust lifetimes are not quotes.
        let single_quotes = !matches!(
            self.active().language(),
            PlainText | Markdown | Latex | Log | Rust
        );
        advanced::pair(
            &self.active().page.text,
            &self.selections(),
            c,
            single_quotes,
        )
    }
    fn insert_smart_newline(&mut self) {
        let b = self.active();
        let width = b.view.tab_size.unwrap_or(4) as usize;
        let tabs = b.view.indent.as_deref() == Some("tabs");
        let crlf = b.page.info.eol == "CRLF";
        let mut text = b.page.text.clone();
        let mut cursors: Vec<Selection> = vec![];
        for s in self.selections().into_iter().rev() {
            let edit = advanced::newline(&text, s, width, tabs, self.settings.smart_indent, crlf);
            let delta = edit.text.chars().count() as isize - text.chars().count() as isize;
            for p in &mut cursors {
                p.anchor = p.anchor.saturating_add_signed(delta);
                p.head = p.head.saturating_add_signed(delta);
            }
            cursors.extend(edit.selections);
            text = edit.text;
        }
        self.batch(Batch {
            text,
            selections: cursors,
        });
    }
}
