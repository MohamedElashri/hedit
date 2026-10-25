//! Close disk-backed files only after resolving their unsaved changes.
use super::*;

pub(super) struct CloseFlow {
    tabs: VecDeque<u64>,
    quit: bool,
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
        });
        self.advance_close();
    }

    pub(super) fn cancel_close(&mut self) {
        self.close_flow = None;
        self.focus_editor |= self.closing;
        self.closing = false;
    }

    /// IDs survive index changes as earlier tabs close. A dirty disk file stays
    /// at the front until its save or discard finishes successfully.
    pub(super) fn advance_close(&mut self) {
        if self.busy {
            return;
        }
        while let Some(flow) = self.close_flow.as_mut() {
            let Some(&id) = flow.tabs.front() else {
                let quit = flow.quit;
                // A job already running when the window closed may have
                // opened another modified disk file after the initial list.
                if quit {
                    if let Some(buffer) = self
                        .workspace
                        .open
                        .iter()
                        .find(|b| b.page.info.path.is_some() && b.page.info.dirty)
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
            if buffer.page.info.path.is_some() && buffer.page.info.dirty {
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
                self.save_to_path(ctx, id, None);
                // Save-time formatting can fail before a worker starts.
                if !self.busy {
                    self.cancel_close();
                }
            }
            Choice::Discard => {
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
        if !buffer.page.info.dirty || buffer.page.info.path.is_none() {
            return;
        }
        let mut choice = None;
        let response = egui::Modal::new(egui::Id::new("save-before-close")).show(ctx, |ui| {
            ui.set_max_width(480.0);
            ui.heading(format!("Save changes to {}?", buffer.view.name));
            ui.label(buffer.page.info.path.as_deref().unwrap_or_default());
            ui.add_space(8.0);
            ui.label("Your changes will be lost if you don't save them.");
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
