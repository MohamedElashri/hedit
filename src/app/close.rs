//! Resolve unsaved changes before closing tabs, retaining untitled notes on the shelf.
use super::*;

pub(super) struct CloseFlow {
    tabs: VecDeque<u64>,
    quit: bool,
    /// Untitled notes acknowledged during quit remain in session recovery.
    preserved: std::collections::HashSet<u64>,
}

fn needs_confirmation(buffer: &Buffer) -> bool {
    if buffer.page.info.path.is_some() {
        buffer.page.info.dirty
    } else {
        buffer.page.info.size > 0 || !buffer.page.text.is_empty()
    }
}

#[derive(Clone, Copy)]
pub(super) enum Choice {
    Save,
    Discard,
    Cancel,
}

impl HeditApp {
    pub(super) fn request_close(&mut self, tabs: Vec<u64>) {
        self.begin_close(tabs, false);
    }

    pub(super) fn request_quit(&mut self) {
        let tabs = self.workspace.open.iter().map(|b| b.view.id).collect();
        self.begin_close(tabs, true);
    }

    fn begin_close(&mut self, tabs: Vec<u64>, quit: bool) {
        if self.closing {
            return;
        }
        self.closing = true;
        self.close_flow = Some(CloseFlow {
            tabs: tabs.into(),
            quit,
            preserved: Default::default(),
        });
        self.advance_close();
    }

    pub(super) fn cancel_close(&mut self) {
        self.close_flow = None;
        self.focus_editor |= self.closing;
        self.closing = false;
    }

    /// IDs survive index changes as earlier tabs close. Unsaved tabs stay at
    /// the front until the user saves, closes without saving, or cancels.
    pub(super) fn advance_close(&mut self) {
        if self.busy {
            return;
        }
        while let Some(flow) = self.close_flow.as_mut() {
            let Some(&id) = flow.tabs.front() else {
                let quit = flow.quit;
                // A job already running when the window closed may have
                // opened another unsaved tab after the initial list.
                if quit {
                    if let Some(buffer) = self
                        .workspace
                        .open
                        .iter()
                        .find(|b| needs_confirmation(b) && !flow.preserved.contains(&b.view.id))
                    {
                        flow.tabs.push_back(buffer.view.id);
                        continue;
                    }
                }
                self.close_flow = None;
                self.closing = quit;
                self.focus_editor = !quit;
                return;
            };
            let Some(index) = self.workspace.open.iter().position(|b| b.view.id == id) else {
                flow.tabs.pop_front();
                continue;
            };
            let buffer = &self.workspace.open[index];
            if needs_confirmation(buffer) && !flow.preserved.contains(&id) {
                if self.workspace.active != index {
                    self.workspace.active = index;
                    self.changed();
                }
                self.focus_editor = false;
                return;
            }
            flow.tabs.pop_front();
            if !flow.quit {
                let result = self.workspace.close(index);
                if let Err(error) = result {
                    self.error = Some(error);
                    self.cancel_close();
                    return;
                }
                self.changed();
            }
        }
    }

    pub(super) fn choose_close(&mut self, ctx: &egui::Context, choice: Choice) {
        if self.busy {
            return;
        }
        let Some(id) = self
            .close_flow
            .as_ref()
            .and_then(|f| f.tabs.front())
            .copied()
        else {
            return;
        };
        match choice {
            Choice::Cancel => self.cancel_close(),
            Choice::Save => {
                let Some(index) = self.workspace.open.iter().position(|b| b.view.id == id) else {
                    return;
                };
                self.workspace.active = index;
                self.save_file(ctx, false);
                // Save-time formatting can fail before a worker starts.
                if !self.busy {
                    self.cancel_close();
                }
            }
            Choice::Discard => {
                let Some(index) = self.workspace.open.iter().position(|b| b.view.id == id) else {
                    return;
                };
                if self.workspace.open[index].page.info.path.is_none() {
                    let quit = self.close_flow.as_ref().is_some_and(|flow| flow.quit);
                    if !quit {
                        if let Err(error) = self.workspace.close(index) {
                            self.error = Some(error);
                            self.cancel_close();
                            return;
                        }
                    }
                    if let Some(flow) = self.close_flow.as_mut() {
                        flow.tabs.pop_front();
                        if quit {
                            flow.preserved.insert(id);
                        }
                    }
                    self.changed();
                    self.advance_close();
                    return;
                }
                let editor = Arc::clone(&self.workspace.editor);
                self.job(ctx, move || {
                    let shared = editor.get(id)?;
                    let mut doc = shared.lock().map_err(|e| e.to_string())?;
                    let page = match doc.discard_changes() {
                        Ok(page) => Some(page),
                        Err(error) => {
                            // There is no disk copy to put on the shelf when
                            // a named file was never saved or was deleted.
                            let missing = doc.info().path.is_some_and(|path| {
                                matches!(std::path::Path::new(&path).try_exists(), Ok(false))
                            });
                            if !missing {
                                return Err(error);
                            }
                            None
                        }
                    };
                    Ok(Action::Discarded(id, page))
                });
            }
        }
    }

    pub(super) fn close_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self
            .close_flow
            .as_ref()
            .and_then(|f| f.tabs.front())
            .copied()
        else {
            return;
        };
        let Some(buffer) = self.workspace.open.iter().find(|b| b.view.id == id) else {
            return;
        };
        // A window close can arrive while another job is still running.
        if !needs_confirmation(buffer) {
            return;
        }
        let mut choice = None;
        let response = egui::Modal::new(egui::Id::new("save-before-close")).show(ctx, |ui| {
            ui.set_max_width(480.0);
            ui.heading(format!("Save changes to {}?", buffer.view.name));
            if let Some(path) = &buffer.page.info.path {
                ui.label(path);
            } else {
                ui.label("This tab hasn't been saved to a file.");
            }
            ui.add_space(8.0);
            ui.label(if buffer.page.info.path.is_some() {
                "Your changes will be lost if you don't save them."
            } else if self.close_flow.as_ref().is_some_and(|flow| flow.quit) {
                "Your note will be restored when you reopen hedit."
            } else {
                "Your note will remain in Closed Files if you don't save it."
            });
            ui.add_space(12.0);
            ui.add_enabled_ui(!self.busy, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        choice = Some(Choice::Save);
                    }
                    if ui.button("Don't Save").clicked() {
                        choice = Some(Choice::Discard);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(Choice::Cancel);
                    }
                });
            });
            if self.busy {
                ui.spinner();
            }
        });
        if !self.busy && response.should_close() {
            choice = Some(Choice::Cancel);
        }
        if !self.busy
            && choice.is_none()
            && response.is_top_modal
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
        {
            choice = Some(Choice::Save);
        }
        if let Some(choice) = choice {
            self.choose_close(ctx, choice);
        }
    }
}
