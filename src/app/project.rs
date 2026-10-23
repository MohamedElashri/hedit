use super::*;
use hedit::{
    folder::{self, FolderListing, QuickOpen},
    session::FolderView,
};
use std::collections::{BTreeSet, HashMap, HashSet};

struct ListingResult {
    generation: u64,
    path: String,
    listing: Result<FolderListing, String>,
}

struct SearchResult {
    generation: u64,
    query: String,
    result: Result<QuickOpen, String>,
}
pub(super) struct CreateEntry {
    parent: String,
    directory: bool,
    name: String,
    focus: bool,
}

/// Directory reads and bounded fuzzy scans never run on the paint thread.
pub(super) struct ProjectBrowser {
    // Shared so drawing the tree each frame does not copy up to 2,000 entries per folder.
    listings: HashMap<String, std::sync::Arc<Result<FolderListing, String>>>,
    pending: HashSet<String>,
    pub expanded: BTreeSet<String>,
    generation: u64,
    tx: Sender<ListingResult>,
    rx: Receiver<ListingResult>,
    search_tx: Sender<SearchResult>,
    search_rx: Receiver<SearchResult>,
    search_pending: bool,
    search_query: Option<String>,
    search_requested: String,
    search_changed: Instant,
    pub search: Option<Result<QuickOpen, String>>,
    pub create: Option<CreateEntry>,
}
impl ProjectBrowser {
    pub fn new(views: &[FolderView]) -> Self {
        let (tx, rx) = mpsc::channel();
        let (search_tx, search_rx) = mpsc::channel();
        Self {
            listings: HashMap::new(),
            pending: HashSet::new(),
            expanded: views
                .iter()
                .flat_map(|f| f.expanded.iter().cloned())
                .collect(),
            generation: 0,
            tx,
            rx,
            search_tx,
            search_rx,
            search_pending: false,
            search_query: None,
            search_requested: String::new(),
            search_changed: Instant::now(),
            search: None,
            create: None,
        }
    }
    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.listings.clear();
        self.pending.clear();
        self.search_query = None;
        self.search = None;
    }
    pub fn poll(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            if result.generation == self.generation {
                self.pending.remove(&result.path);
                self.listings
                    .insert(result.path, std::sync::Arc::new(result.listing));
            }
        }
        while let Ok(result) = self.search_rx.try_recv() {
            self.search_pending = false;
            if result.generation == self.generation && result.query == self.search_requested {
                self.search = Some(result.result);
            } else {
                self.search_query = None;
            }
        }
    }
    fn request_listing(&mut self, ctx: &egui::Context, path: &str) {
        if self.pending.len() >= 8
            || self.listings.contains_key(path)
            || !self.pending.insert(path.to_owned())
        {
            return;
        }
        let path = path.to_owned();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let generation = self.generation;
        thread::spawn(move || {
            let listing = folder::read_directory(PathBuf::from(&path));
            let _ = tx.send(ListingResult {
                generation,
                path,
                listing,
            });
            ctx.request_repaint();
        });
    }
    pub fn search(&mut self, ctx: &egui::Context, roots: &[PathBuf], query: &str) {
        self.poll();
        if self.search_requested != query {
            self.search_requested = query.to_owned();
            self.search_changed = Instant::now();
            self.search = None;
            self.search_query = None;
        }
        if self.search_pending || self.search_query.as_deref() == Some(query) {
            return;
        }
        if !query.is_empty() && self.search_changed.elapsed() < Duration::from_millis(120) {
            ctx.request_repaint_after(Duration::from_millis(120));
            return;
        }
        self.search_pending = true;
        self.search_query = Some(query.to_owned());
        let query = query.to_owned();
        let roots = roots.to_vec();
        let generation = self.generation;
        let tx = self.search_tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = folder::quick_open_roots(&roots, &query);
            let _ = tx.send(SearchResult {
                generation,
                query,
                result,
            });
            ctx.request_repaint();
        });
    }
}

impl HeditApp {
    pub(super) fn open_folder(&mut self, ctx: &egui::Context) {
        self.job(ctx, || {
            match rfd::FileDialog::new()
                .set_title("Add folders — hedit")
                .pick_folders()
            {
                Some(paths) => paths
                    .into_iter()
                    .map(folder::read_directory)
                    .collect::<Result<Vec<_>, _>>()
                    .map(Action::Folders),
                None => Ok(Action::Nothing),
            }
        });
    }
    pub(super) fn set_folder(&mut self, listing: FolderListing) {
        // Roots are canonicalized by read_directory; reopening one refreshes it.
        if let Some(folder) = self
            .workspace
            .folders
            .iter_mut()
            .find(|f| f.path == listing.path)
        {
            folder.collapsed = false;
        } else {
            self.workspace.folders.push(FolderView {
                path: listing.path.clone(),
                visible: true,
                expanded: vec![],
                collapsed: false,
            });
        }
        self.project.invalidate();
        self.project
            .listings
            .insert(listing.path.clone(), std::sync::Arc::new(Ok(listing)));
        self.settings.show_sidebar = true;
        self.settings.show_folders = true;
        self.sync_folder_view();
    }
    pub(super) fn refresh_folder(&mut self) {
        self.project.invalidate();
    }
    pub(super) fn close_folder(&mut self) {
        self.workspace.folders.clear();
        self.project.invalidate();
        self.project.expanded.clear();
        self.project.create = None;
        self.changed();
    }
    fn remove_folder(&mut self, path: &str) {
        self.workspace.folders.retain(|f| f.path != path);
        self.project.invalidate();
        self.project.create = None;
        // Preserve expansions shared by nested or overlapping roots.
        self.project.expanded.retain(|expanded| {
            self.workspace
                .folders
                .iter()
                .any(|f| std::path::Path::new(expanded).starts_with(&f.path))
        });
        self.sync_folder_view();
    }
    pub(super) fn sync_folder_view(&mut self) {
        for folder in &mut self.workspace.folders {
            folder.expanded = self
                .project
                .expanded
                .iter()
                .filter(|path| std::path::Path::new(path).starts_with(&folder.path))
                .cloned()
                .collect();
            folder.visible = self.settings.show_folders;
        }
        self.changed();
    }
    fn create_entry(&mut self, parent: String, directory: bool) {
        self.project.create = Some(CreateEntry {
            parent,
            directory,
            name: String::new(),
            focus: true,
        });
    }
    pub(super) fn folder_tree(&mut self, ui: &mut egui::Ui) {
        if !self.settings.show_folders {
            return;
        }
        self.project.poll();
        ui.add_space(12.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("FOLDERS")
                    .size(10.0)
                    .strong()
                    .color(self.theme.muted.color()),
            );
            if ui
                .small_button("+")
                .on_hover_text("Add folders to workspace")
                .clicked()
            {
                self.open_folder(ui.ctx());
            }
            if ui
                .small_button("↻")
                .on_hover_text("Refresh folders")
                .clicked()
            {
                self.refresh_folder();
            }
            if ui
                .small_button("×")
                .on_hover_text(format!(
                    "Hide folder sidebar · {}",
                    Command::FolderSidebar.shortcut()
                ))
                .clicked()
            {
                self.settings.show_folders = false;
                self.sync_folder_view();
            }
        });
        if !self.settings.show_folders {
            return;
        }
        if self.workspace.folders.is_empty() {
            if ui.button("Open folder…").clicked() {
                self.open_folder(ui.ctx());
            }
            return;
        }
        let folders = self.workspace.folders.clone();
        egui::ScrollArea::vertical()
            .id_salt("folder-tree")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for folder in folders {
                    ui.push_id(("project-root", &folder.path), |ui| {
                        let path = &folder.path;
                        let name = std::path::Path::new(path)
                            .file_name()
                            .unwrap_or(std::ffi::OsStr::new(path))
                            .to_string_lossy();
                        let response = tree_row::show(
                            ui,
                            &self.theme,
                            tree_row::Row {
                                name: &name,
                                path,
                                directory: true,
                                expanded: !folder.collapsed,
                                selected: false,
                                depth: 0,
                                symlink: false,
                            },
                        );
                        if response.clicked() {
                            if let Some(root) =
                                self.workspace.folders.iter_mut().find(|f| f.path == *path)
                            {
                                root.collapsed = !root.collapsed;
                            }
                            self.changed();
                        }
                        response.context_menu(|ui| self.folder_context_menu(ui, path, true));
                        if self
                            .workspace
                            .folders
                            .iter()
                            .any(|f| f.path == *path && !f.collapsed)
                        {
                            self.directory_rows(ui, path, 1);
                        }
                    });
                }
            });
    }
    fn folder_context_menu(&mut self, ui: &mut egui::Ui, path: &str, directory: bool) {
        if directory {
            if ui.button("New file…").clicked() {
                self.create_entry(path.to_owned(), false);
                ui.close_menu();
            }
            if ui.button("New folder…").clicked() {
                self.create_entry(path.to_owned(), true);
                ui.close_menu();
            }
        }
        for (label, operation) in [
            ("Rename…", quality::FileOperation::Rename),
            ("Move…", quality::FileOperation::Move),
            ("Duplicate…", quality::FileOperation::Duplicate),
            ("Move to trash…", quality::FileOperation::Trash),
        ] {
            if ui.button(label).clicked() {
                self.begin_file_action(PathBuf::from(path), operation);
                ui.close_menu();
            }
        }
        if ui.button("Reveal in file manager").clicked() {
            let path = path.to_owned();
            self.job(ui.ctx(), move || {
                hedit::folder::reveal_in_file_manager(path)?;
                Ok(Action::Nothing)
            });
            ui.close_menu();
        }
        if ui.button("Copy relative path").clicked() {
            let roots = self
                .workspace
                .folders
                .iter()
                .map(|f| PathBuf::from(&f.path))
                .collect::<Vec<_>>();
            let path = std::path::Path::new(path);
            let relative = roots
                .iter()
                .filter_map(|root| path.strip_prefix(root).ok())
                .min_by_key(|p| p.components().count())
                .unwrap_or(path);
            ui.ctx().copy_text(if relative.as_os_str().is_empty() {
                ".".into()
            } else {
                relative.to_string_lossy().into_owned()
            });
            ui.close_menu();
        }
        if ui.button("Copy path").clicked() {
            ui.ctx().copy_text(path.to_owned());
            ui.close_menu();
        }
        if ui.button("Refresh folder").clicked() {
            self.refresh_folder();
            ui.close_menu();
        }
        if self.workspace.folders.iter().any(|f| f.path == path)
            && ui.button("Remove folder from workspace").clicked()
        {
            self.remove_folder(path);
            ui.close_menu();
        }
    }

    fn directory_rows(&mut self, ui: &mut egui::Ui, path: &str, depth: usize) {
        // A manually expanded symlink cycle must not overflow the UI stack.
        if depth >= 32 {
            ui.weak("Folder nesting limit reached.");
            return;
        }
        self.project.request_listing(ui.ctx(), path);
        let Some(listing) = self.project.listings.get(path).cloned() else {
            ui.weak("Loading…");
            return;
        };
        let listing = match listing.as_ref() {
            Ok(listing) => listing,
            Err(error) => {
                ui.colored_label(self.theme.warning.color(), error);
                return;
            }
        };
        if listing.entries.is_empty() {
            ui.weak("Empty folder");
        }
        for entry in &listing.entries {
            let expanded = self.project.expanded.contains(&entry.path);
            let selected = self.active().page.info.path.as_deref() == Some(entry.path.as_str());
            let response = tree_row::show(
                ui,
                &self.theme,
                tree_row::Row {
                    name: &entry.name,
                    path: &entry.path,
                    directory: entry.is_directory,
                    expanded,
                    selected,
                    depth,
                    symlink: entry.is_symlink,
                },
            );
            if response.clicked() {
                if entry.is_directory {
                    if expanded {
                        self.project.expanded.remove(&entry.path);
                    } else {
                        self.project.expanded.insert(entry.path.clone());
                    }
                    self.sync_folder_view();
                } else {
                    self.quality.preview_request = !response.double_clicked();
                    if response.double_clicked() {
                        self.quality.preview = None;
                    }
                    self.pending_paths.push_back(PathBuf::from(&entry.path));
                }
            }
            response.context_menu(|ui| {
                self.folder_context_menu(ui, &entry.path, entry.is_directory);
            });
            if entry.is_directory && self.project.expanded.contains(&entry.path) {
                self.directory_rows(ui, &entry.path, depth + 1);
            }
        }
        if listing.truncated {
            ui.weak("Showing up to 2,000 entries in this folder.");
        }
        if listing.unsupported_names {
            ui.weak("Some names cannot be displayed as UTF-8.");
        }
    }
    pub(super) fn folder_dialog(&mut self, ctx: &egui::Context) {
        let Some(create) = &mut self.project.create else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;
        egui::Window::new(if create.directory {
            "New folder"
        } else {
            "New file"
        })
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(&create.parent);
            let response = ui.add(egui::TextEdit::singleline(&mut create.name).hint_text("Name"));
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if create.focus {
                response.request_focus();
                create.focus = false;
            }
            ui.horizontal(|ui| {
                submit = (ui.button("Create").clicked() || enter) && !create.name.trim().is_empty();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if submit && !self.busy && !self.closing {
            let create = self.project.create.take().unwrap();
            self.job(ctx, move || {
                let result = if create.directory {
                    folder::create_folder(create.parent, create.name)
                } else {
                    folder::create_file(create.parent, create.name)
                };
                result.map(|path| Action::Created(path, create.directory))
            });
        } else if cancel {
            self.project.create = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshing_or_switching_folders_discards_late_results() {
        let root = tempfile::tempdir().unwrap();
        let mut browser = ProjectBrowser::new(&[]);
        let listing = folder::read_directory(root.path().into()).unwrap();
        let path = listing.path.clone();
        browser
            .tx
            .send(ListingResult {
                generation: 0,
                path: path.clone(),
                listing: Ok(listing),
            })
            .unwrap();
        browser
            .search_tx
            .send(SearchResult {
                generation: 0,
                query: String::new(),
                result: folder::quick_open(root.path().into(), ""),
            })
            .unwrap();
        browser.invalidate();
        browser.poll();
        assert!(browser.listings.is_empty());
        assert!(browser.search.is_none());
        assert!(!browser.search_pending);
        browser.request_listing(&egui::Context::default(), &path);
        let started = Instant::now();
        while !browser.pending.is_empty() {
            browser.poll();
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(5));
        }
        assert!(browser.listings[&path].is_ok());
    }

    #[test]
    fn changing_back_to_a_previous_query_starts_a_fresh_scan() {
        let root = tempfile::tempdir().unwrap();
        let mut browser = ProjectBrowser::new(&[]);
        browser.search_query = Some("first".into());
        browser.search_requested = "second".into();
        browser.search(&egui::Context::default(), &[root.path().into()], "first");
        assert!(browser.search_query.is_none());
        browser.search_changed = Instant::now() - Duration::from_secs(1);
        browser.search(&egui::Context::default(), &[root.path().into()], "first");
        assert!(browser.search_pending);
    }

    #[test]
    fn removing_a_root_keeps_open_text_and_other_roots_expanded() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        let roots: Vec<_> = ["client", "server"]
            .iter()
            .map(|name| {
                let root = directory.path().join(name);
                std::fs::create_dir_all(root.join("src")).unwrap();
                std::fs::canonicalize(root).unwrap()
            })
            .collect();
        for root in &roots {
            app.set_folder(folder::read_directory(root.clone()).unwrap());
            app.project
                .expanded
                .insert(root.join("src").to_string_lossy().into_owned());
        }
        app.workspace.open[0].page.text = "unsaved work".into();
        app.workspace.edit(0, "").unwrap();
        app.remove_folder(roots[0].to_str().unwrap());
        assert_eq!(app.workspace.folders.len(), 1);
        assert_eq!(app.workspace.folders[0].path, roots[1].to_str().unwrap());
        assert!(app
            .project
            .expanded
            .contains(roots[1].join("src").to_str().unwrap()));
        assert!(!app
            .project
            .expanded
            .contains(roots[0].join("src").to_str().unwrap()));
        assert_eq!(app.active().page.text, "unsaved work");
        assert!(roots[0].join("src").is_dir());
    }

    #[test]
    fn new_file_dialog_creates_and_opens_a_file_in_the_selected_directory() {
        let root = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(root.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.set_folder(folder::read_directory(root.path().into()).unwrap());
        app.create_entry(root.path().to_str().unwrap().to_owned(), false);
        app.project.create.as_mut().unwrap().name = "created.rs".into();
        let render = |app: &mut HeditApp, events| {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ctx| app.folder_dialog(ctx),
            );
        };
        render(&mut app, vec![]);
        render(
            &mut app,
            vec![egui::Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
        );
        let started = Instant::now();
        while app.busy || !app.pending_paths.is_empty() {
            app.receive(&ctx);
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(5));
        }
        assert!(app.error.is_none(), "{:?}", app.error);
        assert_eq!(app.active().page.info.name, "created.rs");
        assert_eq!(
            std::fs::read_to_string(root.path().join("created.rs")).unwrap(),
            ""
        );
        assert_eq!(app.active().page.text, "");
    }
}
