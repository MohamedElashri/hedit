use super::*;
use hedit::editing::{self, Operation};

impl HeditApp {
    pub(super) fn edit_command(&mut self, operation: Operation) {
        if !self.active().view.selections.is_empty()
            && !matches!(operation, Operation::ToLf | Operation::ToCrlf)
        {
            let b = self.active();
            let batch = hedit::advanced::transform(
                &b.page.text,
                &self.selections(),
                operation,
                b.view.tab_size.unwrap_or(4) as usize,
                b.view.indent.as_deref() == Some("tabs"),
                editing::comment_prefix(b.language().id()),
            );
            self.batch(batch);
            return;
        }
        let buffer = self.active();
        let edited = editing::apply(
            &buffer.page.text,
            buffer.view.anchor,
            buffer.view.head,
            operation,
            buffer.view.tab_size.unwrap_or(4) as usize,
            buffer.view.indent.as_deref() == Some("tabs"),
            editing::comment_prefix(buffer.language().id()),
        );
        let crlf = match operation {
            Operation::ToLf => Some(false),
            Operation::ToCrlf => Some(true),
            _ => None,
        };
        self.apply_edit(edited, crlf);
    }

    /// Commands from `text_commands`, applied to every selection as one undo step.
    pub(super) fn text_command(&mut self, command: Command) {
        use hedit::text_commands as t;
        let b = self.active();
        let text = &b.page.text;
        let selections = self.selections();
        let width = b.view.tab_size.unwrap_or(4) as usize;
        let language = b.language().id();
        let batch = match command {
            Command::ToggleBlockComment => match t::block_comment(language) {
                Some((open, close)) => t::toggle_block_comment(text, &selections, open, close),
                // Without block comments, comment the lines instead.
                None => return self.edit_command(Operation::ToggleComment),
            },
            Command::WrapLines => {
                let column = match (self.settings.wrap_width, self.settings.rulers.first()) {
                    (0, Some(&ruler)) => ruler as usize,
                    (0, None) => 80,
                    (wrap, _) => wrap as usize,
                };
                let markers = t::wrap_markers(language, editing::comment_prefix(language));
                t::wrap_lines(text, &selections, column, width, &markers)
            }
            Command::Transpose => t::transpose(text, &selections),
            Command::TitleCase | Command::SwapCase => {
                t::change_case(text, &selections, command == Command::TitleCase)
            }
            Command::DeleteToLineStart | Command::DeleteToLineEnd => {
                t::delete_to_line_edge(text, &selections, command == Command::DeleteToLineEnd)
            }
            Command::Reindent => t::reindent(
                text,
                &selections,
                width,
                b.view.indent.as_deref() == Some("tabs"),
            ),
            _ => return,
        };
        if batch.text != self.active().page.text || batch.selections != selections {
            self.batch(batch);
        }
    }
    pub(super) fn apply_edit(&mut self, edited: editing::Edit, crlf: Option<bool>) -> bool {
        let index = self.workspace.active;
        let old = self.active().page.text.clone();
        if edited.text != old
            || crlf.is_some_and(|crlf| (self.active().page.info.eol == "CRLF") != crlf)
        {
            self.workspace.open[index].page.text = edited.text;
            if let Err(error) = self.workspace.edit_with_line_ending(index, &old, crlf) {
                self.workspace.open[index].page.text = old;
                self.error = Some(error);
                return false;
            }
        }
        let buffer = &mut self.workspace.open[index];
        buffer.view.selections.clear();
        buffer.view.folds.clear();
        buffer.view.anchor = edited.anchor;
        buffer.view.head = edited.head;
        buffer.restore_cursor = true;
        buffer.reveal_cursor = true;
        self.changed();
        self.focus_editor = true;
        true
    }

    pub(super) fn insert_indentation(&mut self) {
        if !self.active().follow && !self.busy && !self.closing && self.expand_trigger() {
            return;
        }
        if self.active().follow || self.closing || self.busy {
            return;
        }
        let buffer = self.active();
        if !buffer.view.selections.is_empty() {
            let replacement = if buffer.view.indent.as_deref() == Some("tabs") {
                "\t".into()
            } else {
                " ".repeat(buffer.view.tab_size.unwrap_or(4) as usize)
            };
            self.batch(hedit::advanced::replace(
                &buffer.page.text,
                &self.selections(),
                &replacement,
            ));
            return;
        }
        if buffer.view.anchor != buffer.view.head {
            if !buffer.page.info.paged {
                self.edit_command(Operation::Indent);
            }
            return;
        }
        let before: String = buffer.page.text.chars().take(buffer.view.head).collect();
        let width = usize::from(buffer.view.tab_size.unwrap_or(4)).clamp(1, 8);
        let column = before
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .chars()
            .fold(0, |col, ch| {
                col + if ch == '\t' { width - col % width } else { 1 }
            });
        let indent = if buffer.view.indent.as_deref() == Some("tabs") {
            "\t".to_owned()
        } else {
            " ".repeat(width - column % width)
        };
        let mut text = buffer.page.text.clone();
        text.insert_str(before.len(), &indent);
        let cursor = buffer.view.head + indent.chars().count();
        self.apply_edit(
            editing::Edit {
                text,
                anchor: cursor,
                head: cursor,
            },
            None,
        );
    }

    pub(super) fn insert_crlf(&mut self) {
        if self.active().follow || self.busy || self.closing {
            return;
        }
        let buffer = self.active();
        let start = buffer.view.anchor.min(buffer.view.head);
        let end = buffer.view.anchor.max(buffer.view.head);
        let mut text: String = buffer.page.text.chars().take(start).collect();
        text.push_str("\r\n");
        text.extend(buffer.page.text.chars().skip(end));
        self.apply_edit(
            editing::Edit {
                text,
                anchor: start + 2,
                head: start + 2,
            },
            None,
        );
    }
}
