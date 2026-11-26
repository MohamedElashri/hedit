//! Browse directories in the active SSH workspace with read-only requests.
use super::*;
use hedit::{folder::FolderListing, remote::Connection};
use hedit_core::backend::{self, Call, FsQuery};

#[derive(Default)]
pub(super) struct FolderPicker {
    path: String,
    folder: Option<Folder>,
    pending: Option<Pending>,
    selected: Option<usize>,
    focus: bool,
    error: Option<String>,
}

struct Folder {
    listing: FolderListing,
    location: String,
    parent: Option<PathBuf>,
    directories: Vec<usize>,
}

impl Folder {
    fn new(listing: FolderListing, location: String, parent: Option<PathBuf>) -> Self {
        let directories = listing
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.is_directory && !entry.is_symlink)
            .map(|(index, _)| index)
            .collect();
        Self {
            listing,
            location,
            parent,
            directories,
        }
    }
}

struct Pending {
    result: mpsc::Receiver<Result<Folder, String>>,
    cancel: Arc<AtomicBool>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

enum Action {
    Navigate(PathBuf),
    BrowsePath,
    AddFolder,
    Close,
}

pub(super) enum Outcome {
    Browsing,
    Closed,
    Opened(FolderListing),
}

impl FolderPicker {
    pub(super) fn open(ctx: &egui::Context, remote: &Connection) -> Self {
        let mut picker = Self::default();
        picker.navigate(ctx, remote, remote.root());
        picker
    }

    fn navigate(&mut self, ctx: &egui::Context, remote: &Connection, path: PathBuf) {
        self.error = match Self::read(ctx, remote, path) {
            Ok(pending) => {
                self.pending = Some(pending);
                None
            }
            Err(error) => {
                self.focus = true;
                ctx.request_repaint();
                Some(error)
            }
        };
    }

    fn read(ctx: &egui::Context, remote: &Connection, path: PathBuf) -> Result<Pending, String> {
        let mut relative = remote.relative(&path)?;
        if relative.contains(&0) {
            return Err("Choose a directory within this SSH workspace.".into());
        }
        while relative.last() == Some(&b'/') {
            relative.pop();
        }
        let path = remote.uri(&relative);
        let parent = parent_path(&relative).map(|parent| remote.uri(parent));
        let location = if relative.is_empty() {
            remote.config.root.clone()
        } else {
            format!(
                "{}/{}",
                remote.config.root.trim_end_matches('/'),
                String::from_utf8_lossy(&relative)
            )
        };
        let (tx, result) = mpsc::channel();
        let ctx = ctx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        thread::Builder::new()
            .name("ssh-folder-picker".into())
            .spawn(move || {
                if flag.load(Ordering::Relaxed) {
                    return;
                }
                let response = backend::route::<FolderListing>(
                    &path,
                    Call::File {
                        path: path.clone(),
                        query: FsQuery::List,
                    },
                    &flag,
                )
                .unwrap_or_else(|| Err("Choose a directory within this SSH workspace.".into()))
                .map(|listing| Folder::new(listing, location, parent));
                if tx.send(response).is_ok() {
                    ctx.request_repaint();
                }
            })
            .map_err(|error| format!("Cannot start the remote folder request. {error}"))?;
        Ok(Pending { result, cancel })
    }

    fn poll(&mut self) {
        let Some(pending) = self.pending.as_ref() else {
            return;
        };
        let result = match pending.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The remote folder request stopped. Try browsing again.".into())
            }
        };
        self.pending = None;
        self.focus = true;
        match result {
            Ok(folder) => {
                self.folder = Some(folder);
                self.path.clear();
                self.selected = None;
                self.error = None;
            }
            Err(error) => self.error = Some(format!("Cannot browse this remote folder. {error}")),
        }
    }

    pub(super) fn show(&mut self, ctx: &egui::Context, remote: &Connection) -> Outcome {
        self.poll();
        match self.controls(ctx) {
            Some(Action::Navigate(path)) => self.navigate(ctx, remote, path),
            Some(Action::BrowsePath) => {
                let path = if self.path.trim_end_matches('/')
                    == remote.config.root.trim_end_matches('/')
                {
                    Ok(remote.root())
                } else {
                    remote.path(&self.path)
                };
                match path {
                    Ok(path) => self.navigate(ctx, remote, path),
                    Err(error) => {
                        self.error = Some(error);
                        self.focus = true;
                        ctx.request_repaint();
                    }
                }
            }
            Some(Action::AddFolder) => {
                if let Some(folder) = self.folder.take() {
                    return Outcome::Opened(folder.listing);
                }
            }
            Some(Action::Close) => return Outcome::Closed,
            None => (),
        }
        Outcome::Browsing
    }

    fn controls(&mut self, ctx: &egui::Context) -> Option<Action> {
        let ready = self.pending.is_none();
        let input_id = egui::Id::unique("ssh-folder-picker-path");
        let focused = ctx.memory(|memory| memory.has_focus(input_id));
        let take = |key| {
            ready && focused && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, key))
        };
        let down = take(Key::ArrowDown);
        let up = take(Key::ArrowUp);
        let enter = take(Key::Enter);
        let shown = egui::Modal::new(egui::Id::unique("ssh-folder-picker")).show(ctx, |ui| {
            ui.set_width((ctx.viewport_rect().width() - 40.0).clamp(0.0, 500.0));
            ui.heading("Open remote folder");
            let mut action = None;
            if let Some(folder) = &self.folder {
                ui.horizontal(|ui| {
                    if let Some(parent) = &folder.parent {
                        if ui.add_enabled(ready, egui::Button::new("Up")).clicked() {
                            action = Some(Action::Navigate(parent.clone()));
                        }
                    }
                    ui.label(&folder.location);
                });
            }
            ui.horizontal(|ui| {
                let input = ui.add_enabled(
                    ready,
                    egui::TextEdit::singleline(&mut self.path)
                        .id(input_id)
                        .hint_text("Path relative to the SSH workspace")
                        .desired_width((ui.available_width() - 100.0).max(0.0)),
                );
                if self.focus && ready {
                    if !input.has_focus() {
                        input.request_focus();
                        ctx.request_repaint();
                    }
                    self.focus = ui.is_sizing_pass();
                }
                if input.changed() {
                    self.selected = None;
                    self.error = None;
                }
                if ui
                    .add_enabled(ready && !self.path.is_empty(), egui::Button::new("Browse"))
                    .clicked()
                {
                    action = Some(Action::BrowsePath);
                }
            });
            if !ready {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Reading remote folder…");
                });
            }
            if let Some(folder) = &self.folder {
                let count = folder.directories.len();
                if count == 0 && ready {
                    ui.label("No subfolders.");
                } else if count > 0 {
                    if down {
                        self.selected = Some(self.selected.map_or(0, |i| (i + 1).min(count - 1)));
                    } else if up {
                        self.selected = self.selected.and_then(|i| i.checked_sub(1));
                    }
                    let mut scroll = egui::ScrollArea::vertical()
                        .max_height(230.0_f32.min(ctx.viewport_rect().height() * 0.4));
                    if up || down {
                        let stride = 26.0 + ui.spacing().item_spacing.y;
                        scroll =
                            scroll.vertical_scroll_offset(self.selected.map_or(0.0, |index| {
                                (index as f32 * stride - 2.0 * stride).max(0.0)
                            }));
                    }
                    scroll.show_rows(ui, 26.0, count, |ui, rows| {
                        for row in rows {
                            let entry = &folder.listing.entries[folder.directories[row]];
                            if ui
                                .add_enabled(
                                    ready,
                                    egui::Button::new(&entry.name)
                                        .selected(self.selected == Some(row))
                                        .min_size(egui::vec2(ui.available_width(), 26.0))
                                        .truncate(),
                                )
                                .clicked()
                            {
                                action = Some(Action::Navigate(PathBuf::from(&entry.path)));
                            }
                        }
                    });
                    if enter {
                        if let Some(row) = self.selected {
                            let entry = &folder.listing.entries[folder.directories[row]];
                            action = Some(Action::Navigate(PathBuf::from(&entry.path)));
                        }
                    }
                }
                if folder.listing.truncated || folder.listing.unsupported_names {
                    ui.weak("Some entries could not be listed.");
                }
            }
            if enter && action.is_none() {
                if !self.path.is_empty() {
                    action = Some(Action::BrowsePath);
                } else if self.folder.is_some() {
                    action = Some(Action::AddFolder);
                }
            }
            if let Some(error) = &self.error {
                ui.label(error);
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        ready && self.folder.is_some(),
                        egui::Button::new("Add Folder"),
                    )
                    .clicked()
                {
                    action = Some(Action::AddFolder);
                }
                if ui.button("Cancel").clicked() {
                    action = Some(Action::Close);
                }
            });
            action
        });
        if shown.should_close() {
            Some(Action::Close)
        } else {
            shown.inner
        }
    }
}

fn parent_path(relative: &[u8]) -> Option<&[u8]> {
    (!relative.is_empty()).then(|| {
        let end = relative.iter().rposition(|&byte| byte == b'/').unwrap_or(0);
        &relative[..end]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hedit::folder::FolderEntry;

    fn root_listing() -> Folder {
        let prefix = "hedit-ssh:test/";
        let entries = [
            ("notes.txt", false, false),
            ("alias", true, true),
            ("project", true, false),
        ]
        .map(|(name, is_directory, is_symlink)| FolderEntry {
            name: name.into(),
            path: format!("{prefix}{name}"),
            is_directory,
            is_symlink,
        });
        Folder::new(
            FolderListing {
                name: "work".into(),
                path: prefix.into(),
                entries: entries.into(),
                truncated: false,
                unsupported_names: false,
            },
            "/home/editor/work".into(),
            None,
        )
    }

    fn frame(
        ctx: &egui::Context,
        picker: &mut FolderPicker,
        events: Vec<egui::Event>,
    ) -> Option<Action> {
        let mut result = None;
        let _ = crate::app::tests::run_pass(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(720.0, 480.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                if let Some(action) = picker.controls(ctx) {
                    result = Some(action);
                }
            },
        );
        result
    }

    #[test]
    fn folder_picker_keyboard_skips_files_and_symlinks() {
        let ctx = egui::Context::default();
        let mut picker = FolderPicker {
            folder: Some(root_listing()),
            focus: true,
            ..Default::default()
        };
        let key = |key| crate::app::tests::key(key, Modifiers::NONE);
        frame(&ctx, &mut picker, vec![]);
        frame(&ctx, &mut picker, vec![]);
        frame(&ctx, &mut picker, vec![key(Key::ArrowDown)]);
        assert!(matches!(
            frame(&ctx, &mut picker, vec![key(Key::Enter)]),
            Some(Action::Navigate(path)) if path == std::path::Path::new("hedit-ssh:test/project")
        ));
        frame(&ctx, &mut picker, vec![key(Key::ArrowUp)]);
        assert!(picker.selected.is_none());
        assert!(matches!(
            frame(&ctx, &mut picker, vec![key(Key::Enter)]),
            Some(Action::AddFolder)
        ));
        frame(
            &ctx,
            &mut picker,
            vec![egui::Event::Text("project/src".into())],
        );
        assert!(matches!(
            frame(&ctx, &mut picker, vec![key(Key::Enter)]),
            Some(Action::BrowsePath)
        ));
        assert_eq!(picker.path, "project/src");
    }

    #[test]
    fn closing_a_pending_folder_request_cancels_it() {
        let ctx = egui::Context::default();
        let (tx, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let mut picker = FolderPicker {
            pending: Some(Pending {
                result,
                cancel: cancel.clone(),
            }),
            ..Default::default()
        };
        picker.poll();
        assert!(picker.pending.is_some());
        frame(&ctx, &mut picker, vec![]);
        assert!(matches!(
            frame(
                &ctx,
                &mut picker,
                vec![crate::app::tests::key(Key::Escape, Modifiers::NONE)]
            ),
            Some(Action::Close)
        ));
        drop(picker);
        assert!(cancel.load(Ordering::Relaxed));
        assert!(tx.send(Ok(root_listing())).is_err());
    }

    #[test]
    fn parent_browsing_stops_at_the_authorized_root() {
        assert_eq!(parent_path(b"project/src"), Some(b"project".as_slice()));
        assert_eq!(parent_path(b"project"), Some(b"".as_slice()));
        assert_eq!(parent_path(b""), None);
    }
}
