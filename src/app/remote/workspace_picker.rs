//! Browse an SSH host before opening a workspace or acquiring a writer.
use super::*;
use hedit::remote::bootstrap::{browse_directories, DirectoryPreview};

pub(super) struct WorkspacePicker {
    profile: RemoteConfig,
    input: String,
    directory: Option<DirectoryPreview>,
    pending: Option<mpsc::Receiver<Result<DirectoryPreview, String>>>,
    cancel: Arc<AtomicBool>,
    error: Option<String>,
    focus: bool,
}

impl Drop for WorkspacePicker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub(super) enum Outcome {
    Browsing,
    Closed,
    Open(RemoteConfig),
    Configure(RemoteConfig),
}

enum Action {
    Browse(String),
    Open,
}

impl WorkspacePicker {
    pub(super) fn new(ctx: &egui::Context, profile: RemoteConfig) -> Self {
        let initial = profile.root.clone();
        let mut picker = Self {
            profile,
            input: initial.clone(),
            directory: None,
            pending: None,
            cancel: Arc::new(AtomicBool::new(false)),
            error: None,
            focus: true,
        };
        picker.browse(ctx, initial);
        picker
    }

    fn browse(&mut self, ctx: &egui::Context, path: String) {
        self.error = None;
        self.input.clone_from(&path);
        let profile = self.profile.clone();
        let cancel = self.cancel.clone();
        self.pending = Some(remote_worker(ctx, move || {
            let selected = (!path.is_empty()).then_some(path.as_str());
            browse_directories(&profile, selected, &cancel)
        }));
    }

    pub(super) fn show(&mut self, ctx: &egui::Context) -> Outcome {
        if let Some(result) = remote_worker_result(&mut self.pending, "SSH folder preview") {
            self.focus = true;
            match result {
                Ok(directory) => {
                    self.input.clone_from(&directory.path);
                    self.directory = Some(directory);
                }
                Err(error) => self.error = Some(error),
            }
        }
        let ready = self.pending.is_none();
        let input_id = egui::Id::new("ssh-workspace-directory");
        let enter = ready
            && ctx.memory(|memory| memory.has_focus(input_id))
            && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));
        let shown = egui::Modal::new(egui::Id::new("ssh-choose-workspace")).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 40.0).clamp(0.0, 480.0));
            ui.heading("Choose remote folder");
            ui.label(&self.profile.target.host);
            let mut action = None;
            ui.horizontal(|ui| {
                let input = ui.add_enabled(
                    ready,
                    egui::TextEdit::singleline(&mut self.input)
                        .id(input_id)
                        .hint_text("Absolute path or blank for remote home")
                        .desired_width((ui.available_width() - 85.0).max(0.0)),
                );
                if ready && self.focus {
                    input.request_focus();
                    self.focus = ui.is_sizing_pass();
                }
                let browse = ui.add_enabled(ready, egui::Button::new("Browse"));
                if browse.clicked() || enter {
                    action = Some(Action::Browse(self.input.clone()));
                }
            });
            if !ready {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Reading remote directories…");
                });
            }
            if let Some(directory) = &self.directory {
                ui.horizontal(|ui| {
                    if let Some(parent) = parent(&directory.path) {
                        if ui.add_enabled(ready, egui::Button::new("Up")).clicked() {
                            action = Some(Action::Browse(parent.into()));
                        }
                    }
                    ui.label(&directory.path);
                });
                if directory.directories.is_empty() {
                    ui.weak("No subfolders.");
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(230.0_f32.min(ctx.screen_rect().height() * 0.4))
                        .show_rows(ui, 25.0, directory.directories.len(), |ui, rows| {
                            for row in rows {
                                let name = &directory.directories[row];
                                if ui
                                    .add_enabled(
                                        ready,
                                        egui::Button::new(name)
                                            .truncate()
                                            .min_size(egui::vec2(ui.available_width(), 25.0)),
                                    )
                                    .clicked()
                                {
                                    action = Some(Action::Browse(format!(
                                        "{}/{name}",
                                        directory.path.trim_end_matches('/')
                                    )));
                                }
                            }
                        });
                }
                if directory.incomplete {
                    ui.weak("Some directory names were omitted. You can browse by path.");
                }
            }
            if let Some(error) = &self.error {
                ui.label(error);
            }
            let mut cancel = false;
            ui.horizontal(|ui| {
                let matching_preview = self
                    .directory
                    .as_ref()
                    .is_some_and(|directory| directory.path == self.input);
                if ui
                    .add_enabled(
                        ready && matching_preview,
                        egui::Button::new("Open workspace"),
                    )
                    .clicked()
                {
                    action = Some(Action::Open);
                }
                cancel = ui.button("Cancel").clicked();
            });
            (action, cancel)
        });
        if shown.should_close() || shown.inner.1 {
            return Outcome::Closed;
        }
        match shown.inner.0 {
            Some(Action::Browse(path)) => self.browse(ctx, path),
            Some(Action::Open) => {
                if let Some(directory) = &self.directory {
                    return workspace_config(&self.profile, &directory.path);
                }
            }
            None => (),
        }
        Outcome::Browsing
    }
}

fn parent(path: &str) -> Option<&str> {
    let path = path.trim_end_matches('/');
    (!path.is_empty()).then(|| {
        let (parent, _) = path.rsplit_once('/').expect("absolute preview path");
        if parent.is_empty() {
            "/"
        } else {
            parent
        }
    })
}

fn workspace_config(profile: &RemoteConfig, root: &str) -> Outcome {
    let mut profile = profile.clone();
    if profile.root.is_empty() || profile.root.trim_end_matches('/') != root.trim_end_matches('/') {
        profile.root = root.to_owned();
        profile.target.state.clear();
    }
    profile.takeover = false;
    if !profile.managed && profile.target.state.is_empty() {
        Outcome::Configure(profile)
    } else {
        Outcome::Open(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_another_root_preserves_the_connection_and_uses_new_recovery() {
        let mut profile = ConnectChoice::Host("dev@cluster").config(Some(PathBuf::from("/ssh")));
        profile.root = "/work/a".into();
        profile.target.state = "/private/a".into();
        profile.target.port = 4444;
        profile.takeover = true;
        let Outcome::Open(chosen) = workspace_config(&profile, "/work/b") else {
            panic!("managed profile needs no installation settings");
        };
        assert_eq!(chosen.root, "/work/b");
        assert_eq!(chosen.target.host, profile.target.host);
        assert_eq!(chosen.target.user, profile.target.user);
        assert_eq!(chosen.target.port, profile.target.port);
        assert_eq!(chosen.target.config, profile.target.config);
        assert!(chosen.target.state.is_empty());
        assert!(!chosen.takeover);

        profile.managed = false;
        profile.target.helper = "/opt/helper".into();
        let Outcome::Configure(chosen) = workspace_config(&profile, "/work/b") else {
            panic!("a new manual root needs its own recovery directory");
        };
        assert_eq!(chosen.target.helper, profile.target.helper);
        assert!(chosen.target.state.is_empty());
        let Outcome::Open(chosen) = workspace_config(&profile, "/work/a/") else {
            panic!("an existing manual root can reopen");
        };
        assert_eq!(chosen.target.state, profile.target.state);
    }

    #[test]
    fn preview_can_browse_to_the_remote_filesystem_root() {
        assert_eq!(parent("/work/a"), Some("/work"));
        assert_eq!(parent("/work"), Some("/"));
        assert_eq!(parent("/"), None);
    }
}
