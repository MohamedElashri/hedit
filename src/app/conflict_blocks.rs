//! Resolve the merge markers in an editor buffer without touching the index.
use super::*;
use std::ops::Range;

const LIMIT: usize = 512 * 1024;

struct Block {
    whole: Range<usize>,
    current: Range<usize>,
    incoming: Range<usize>,
}

fn marker(line: &str, symbol: char) -> Option<usize> {
    let length = line.chars().take_while(|&c| c == symbol).count();
    (length >= 7
        && line[length..]
            .chars()
            .next()
            .is_none_or(char::is_whitespace))
    .then_some(length)
}

fn blocks(text: &str) -> Result<Vec<Block>, &'static str> {
    let mut offset = 0;
    let mut lines = text.split_inclusive('\n');
    let mut blocks = Vec::new();
    while let Some(line) = lines.next() {
        let start = offset;
        offset += line.len();
        let Some(width) = marker(line, '<') else {
            continue;
        };
        let current_start = offset;
        let mut base = None;
        let mut separator = None;
        let mut closed = false;
        for line in lines.by_ref() {
            let at = offset;
            offset += line.len();
            if marker(line, '<') == Some(width) {
                break;
            }
            if marker(line, '|') == Some(width) && separator.is_none() {
                base = Some(at);
            } else if marker(line, '=') == Some(width) && line.trim_end().len() == width {
                if separator.is_some() {
                    break;
                }
                separator = Some((at, offset));
            } else if marker(line, '>') == Some(width) {
                if let Some((divide, incoming_start)) = separator {
                    blocks.push(Block {
                        whole: start..offset,
                        current: current_start..base.unwrap_or(divide),
                        incoming: incoming_start..at,
                    });
                    closed = true;
                }
                break;
            }
        }
        if !closed {
            return Err("The conflict markers are incomplete or nested.");
        }
    }
    Ok(blocks)
}

pub(super) struct Draft {
    id: u64,
    revision: u64,
    path: String,
    text: String,
    blocks: Result<Vec<Block>, &'static str>,
    selected: usize,
}

impl Draft {
    fn choose(&mut self, current: bool, incoming: bool) {
        let Ok(blocks) = &self.blocks else { return };
        let Some(block) = blocks.get(self.selected) else {
            return;
        };
        let mut replacement = String::new();
        if current {
            replacement.push_str(&self.text[block.current.clone()]);
        }
        if incoming {
            replacement.push_str(&self.text[block.incoming.clone()]);
        }
        self.text.replace_range(block.whole.clone(), &replacement);
        self.rescan();
    }

    fn rescan(&mut self) {
        self.blocks = if self.text.len() <= LIMIT {
            blocks(&self.text)
        } else {
            Err("The result exceeds the 512 KiB limit.")
        };
        self.selected = self.selected.min(
            self.blocks
                .as_ref()
                .map_or(0, |blocks| blocks.len().saturating_sub(1)),
        );
    }
}

impl HeditApp {
    pub(super) fn resolve_conflict_blocks(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.quality.conflict_path = Some(path.clone());
        self.quality.conflict_draft = None;
        self.open_existing(ctx, path);
    }

    pub(super) fn conflict_blocks_window(&mut self, ctx: &egui::Context) {
        if !self.opens.pending() && !self.busy {
            if let Some(path) = self.quality.conflict_path.take() {
                let buffer = self.active();
                if buffer.page.info.path.as_deref().is_some_and(|name| {
                    hedit::language_server::text::same_path(std::path::Path::new(name), &path)
                }) {
                    if buffer.page.info.paged || buffer.follow || buffer.page.text.len() > LIMIT {
                        self.error = Some(
                            "Resolve blocks requires a fully loaded file up to 512 KiB.".into(),
                        );
                    } else {
                        let parsed = blocks(&buffer.page.text);
                        if parsed.as_ref().is_ok_and(Vec::is_empty) {
                            self.notice = "No conflict markers found in the editor.".into();
                        } else {
                            self.quality.conflict_draft = Some(Draft {
                                id: buffer.view.id,
                                revision: buffer.page.info.revision,
                                path: buffer.page.info.path.clone().unwrap(),
                                blocks: parsed,
                                text: buffer.page.text.clone(),
                                selected: 0,
                            });
                        }
                    }
                }
            }
        }
        let active = self.active();
        let matches = |draft: &Draft| {
            active.view.id == draft.id && active.page.info.revision == draft.revision
        };
        let valid = self.quality.conflict_draft.as_ref().is_some_and(matches);
        let Some(draft) = self.quality.conflict_draft.as_mut() else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let mut choice = None;
        egui::Window::new("Resolve Conflict Blocks")
            .open(&mut open)
            .default_width(740.0)
            .max_width((ctx.viewport_rect().width() - 32.0).max(300.0))
            .collapsible(false)
            .show(ctx, |ui| {
                match &draft.blocks {
                    Ok(blocks) if !blocks.is_empty() => {
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(draft.selected > 0, egui::Button::new("Previous"))
                                .clicked()
                            {
                                draft.selected -= 1;
                            }
                            ui.label(format!(
                                "Conflict {} of {}",
                                draft.selected + 1,
                                blocks.len()
                            ));
                            if ui
                                .add_enabled(
                                    draft.selected + 1 < blocks.len(),
                                    egui::Button::new("Next"),
                                )
                                .clicked()
                            {
                                draft.selected += 1;
                            }
                            for (label, first, second) in [
                                ("Accept Current", true, false),
                                ("Accept Incoming", false, true),
                                ("Accept Both", true, true),
                            ] {
                                if ui.button(label).clicked() {
                                    choice = Some((first, second));
                                }
                            }
                        });
                        let block = &blocks[draft.selected];
                        ui.columns(2, |columns| {
                            for (column, label, range) in [
                                (0, "Current", &block.current),
                                (1, "Incoming", &block.incoming),
                            ] {
                                columns[column].strong(label);
                                egui::ScrollArea::both()
                                    .id_salt(label)
                                    .max_height(140.0)
                                    .show(&mut columns[column], |ui| {
                                        ui.monospace(&draft.text[range.clone()]);
                                    });
                            }
                        });
                    }
                    Ok(_) => {
                        ui.label("All conflict blocks resolved.");
                    }
                    Err(error) => {
                        ui.label(*error);
                    }
                }
                if let Some((first, second)) = choice {
                    draft.choose(first, second);
                }
                ui.separator();
                ui.strong("Result");
                let changed = egui::ScrollArea::both()
                    .max_height(230.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut draft.text)
                                .code_editor()
                                .desired_width(f32::INFINITY),
                        )
                        .changed()
                    })
                    .inner;
                if changed {
                    draft.rescan();
                }
                if !valid {
                    ui.label("The editor changed. Reopen the conflict to apply a new result.");
                }
                apply = ui
                    .add_enabled(
                        valid
                            && !self.busy
                            && !self.closing
                            && draft.blocks.as_ref().is_ok_and(Vec::is_empty),
                        egui::Button::new("Apply to Editor"),
                    )
                    .clicked();
            });
        if apply {
            let draft = self.quality.conflict_draft.take().unwrap();
            let editor = Arc::clone(&self.workspace.editor);
            self.job(ctx, move || {
                let shared = editor.get(draft.id)?;
                let mut doc = shared.lock().map_err(|error| error.to_string())?;
                let info = doc.info();
                if info.path.as_ref() != Some(&draft.path) || info.revision != draft.revision {
                    return Err(
                        "The editor changed. Reopen the conflict to apply a new result.".into(),
                    );
                }
                doc.replace(0, info.size, draft.text, draft.revision)?;
                Ok(Action::Page(draft.id, doc.page(0)?, None))
            });
        } else if !open {
            self.quality.conflict_draft = None;
        }
    }
}

#[cfg(test)]
#[test]
fn resolves_multiple_diff3_blocks_without_changing_surrounding_lines() {
    let text = "keep\r\n<<<<<<< HEAD\r\na\r\n||||||| base\r\nold\r\n=======\r\nb\r\n>>>>>>> other\r\nmiddle\n<<<<<<< HEAD\n=======\nc\n>>>>>>> other\nend";
    let mut draft = Draft {
        id: 1,
        revision: 1,
        path: String::new(),
        text: text.into(),
        blocks: blocks(text),
        selected: 0,
    };
    assert_eq!(draft.blocks.as_ref().unwrap().len(), 2);
    draft.choose(true, true);
    draft.choose(true, false);
    assert_eq!(draft.text, "keep\r\na\r\nb\r\nmiddle\nend");
    assert!(draft.blocks.unwrap().is_empty());
}
