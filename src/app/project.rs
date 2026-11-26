use super::*;
use hedit::{
    folder::{self, FolderListing, QuickOpen},
    session::FolderView,
};
use std::collections::{BTreeSet, HashMap, HashSet};
mod names;
pub(super) mod tree;

fn listing_bytes(listing: &Result<FolderListing, String>) -> usize {
    match listing {
        Ok(l) => {
            l.path.len() * 2
                + l.name.len()
                + 256
                + l.entries
                    .iter()
                    .map(|e| {
                        e.name.len() + e.path.len() + std::mem::size_of::<folder::FolderEntry>()
                    })
                    .sum::<usize>()
        }
        Err(e) => e.len() + 256,
    }
}

struct ListingResult {
    generation: u64,
    path: String,
    listing: Result<FolderListing, String>,
}

pub(super) struct CreateEntry {
    pub(super) parent: String,
    pub(super) directory: bool,
    pub(super) name: String,
    pub(super) focus: bool,
    pub(super) inline: bool,
    pub(super) rename: Option<PathBuf>,
    pub(super) error: Option<String>,
    pub(super) pending: bool,
    pub(super) token: u64,
}

/// Directory reads and bounded fuzzy scans never run on the paint thread.
pub(super) struct ProjectBrowser {
    // Shared so drawing the tree each frame does not copy up to 2,000 entries per folder.
    listings: HashMap<String, std::sync::Arc<Result<FolderListing, String>>>,
    pending: HashSet<String>,
    active_listings: usize,
    pub(super) tree: Option<Arc<tree::Tree>>,
    listing_limited: bool,
    pub expanded: BTreeSet<String>,
    /// A file row to scroll into view once it is drawn, until a deadline in
    /// case it never is (for example, when it is excluded from the tree).
    pub reveal: Option<(String, Instant)>,
    generation: u64,
    tx: Sender<ListingResult>,
    rx: Receiver<ListingResult>,
    names: names::Names,
    pub search: Option<Result<QuickOpen, String>>,
    pub create: Option<CreateEntry>,
}
impl ProjectBrowser {
    pub fn new(views: &[FolderView]) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            listings: HashMap::new(),
            pending: HashSet::new(),
            active_listings: 0,
            tree: None,
            listing_limited: false,
            expanded: views
                .iter()
                .flat_map(|f| f.expanded.iter().cloned())
                .collect(),
            reveal: None,
            generation: 0,
            tx,
            rx,
            names: names::Names::default(),
            search: None,
            create: None,
        }
    }
    pub(super) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.tree = None;
        self.listings.clear();
        self.listing_limited = false;
        self.pending.clear();
        self.names.invalidate();
        self.search = None;
    }
    pub fn poll(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            self.active_listings = self.active_listings.saturating_sub(1);
            self.tree = None;
            if result.generation == self.generation {
                self.pending.remove(&result.path);
                let bytes: usize = self.listings.values().map(|l| listing_bytes(l)).sum();
                if self.listings.len() >= 256
                    || bytes.saturating_add(listing_bytes(&result.listing)) > 16 * 1024 * 1024
                {
                    self.listing_limited = true;
                } else {
                    self.listings.insert(result.path, Arc::new(result.listing));
                }
            }
        }
    }
    fn request_listing(&mut self, ctx: &egui::Context, path: &str) {
        if self.active_listings >= 4
            || self.listing_limited
            || self.listings.contains_key(path)
            || !self.pending.insert(path.to_owned())
        {
            return;
        }
        self.active_listings += 1;
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
    pub fn refresh_search(&mut self) {
        self.invalidate();
    }
    pub fn begin_search(&mut self) {
        if self.names.begin() {
            self.search = None;
        }
    }
    pub fn indexing(&self) -> bool {
        self.names.indexing
    }
    pub fn search(&mut self, ctx: &egui::Context, roots: &[PathBuf], query: &str) {
        self.poll();
        if !self.names.current(roots, query) {
            self.search = None;
        }
        if let Some(result) = self.names.search(ctx, roots, query) {
            self.search = Some(Ok(result));
        }
    }
}

impl HeditApp {
    pub(super) fn open_folder(&mut self, ctx: &egui::Context) {
        if self.workspace.remote.is_some() {
            self.open_remote_folder(ctx);
            return;
        }
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
    /// Show the active file in the folder tree, opening the folders above it.
    pub(super) fn reveal_in_sidebar(&mut self) {
        let Some(path) = self.active().page.info.path.clone() else {
            return;
        };
        self.sidebar = workbench_navigation::Sidebar::Files;
        self.distraction_free = false;
        self.reveal_path_in_sidebar(path);
    }
    pub(super) fn reveal_path_in_sidebar(&mut self, path: String) {
        let file = std::path::Path::new(&path);
        let Some(root) = self
            .workspace
            .folders
            .iter_mut()
            .filter(|f| file.starts_with(&f.path))
            .max_by_key(|f| f.path.len())
        else {
            self.notice = "This file is not in an open folder.".into();
            return;
        };
        root.collapsed = false;
        let root = PathBuf::from(&root.path);
        for folder in file.ancestors().skip(1) {
            if !folder.starts_with(&root) || folder == root {
                break;
            }
            self.project
                .expanded
                .insert(folder.to_string_lossy().into_owned());
        }
        self.settings.show_sidebar = true;
        self.settings.show_folders = true;
        self.project.reveal = Some((path, Instant::now() + Duration::from_secs(2)));
        self.sync_folder_view();
    }
    pub(super) fn refresh_folder(&mut self) {
        self.explorer.badges.invalidate();
        self.workspace_watch.retry();
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
    #[cfg(test)]
    fn create_entry(&mut self, parent: String, directory: bool) {
        self.project.create = Some(CreateEntry {
            parent,
            directory,
            name: String::new(),
            focus: true,
            inline: false,
            rename: None,
            error: None,
            pending: false,
            token: 0,
        });
    }
    pub(super) fn folder_tree(&mut self, ui: &mut egui::Ui) {
        if !self.settings.show_folders {
            return;
        }
        self.project.poll();
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let metrics = hedit::ui_style::Metrics::for_ui(ui);
            let label_width =
                (ui.available_width() - 3.0 * metrics.control - 3.0 * ui.spacing().item_spacing.x)
                    .max(0.0);
            if label_width >= 2.0 * metrics.font {
                ui.allocate_ui_with_layout(
                    egui::vec2(label_width, metrics.control),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_width(label_width);
                        ui.add(
                            egui::Label::new(
                                RichText::new("FOLDERS")
                                    .size(metrics.small)
                                    .strong()
                                    .color(self.theme.muted.color()),
                            )
                            .truncate(),
                        )
                        .on_hover_text("Folders");
                    },
                );
            }
            if hedit::icons::button(ui, hedit::icons::Icon::Add, "Add folders to workspace")
                .clicked()
            {
                self.open_folder(ui.ctx());
            }
            if hedit::icons::button(ui, hedit::icons::Icon::Refresh, "Refresh folders")
                .on_hover_text(format!(
                    "{}\n{}",
                    self.workspace_watch.status, self.explorer.badges.note
                ))
                .clicked()
            {
                self.refresh_folder();
            }
            if controls::close_button(
                ui,
                &format!(
                    "Hide folder sidebar · {}",
                    self.shortcut_text(Command::FolderSidebar)
                ),
            )
            .clicked()
            {
                self.settings.show_folders = false;
                self.sync_folder_view();
            }
        });
        if !self.settings.show_folders {
            return;
        }
        let tree_focus = ui.interact(
            ui.available_rect_before_wrap(),
            egui::Id::unique("explorer-tree"),
            egui::Sense::focusable_noninteractive(),
        );
        tree_focus.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::GenericContainer,
                true,
                format!(
                    "Explorer · {}",
                    self.explorer.selected.as_deref().unwrap_or("files")
                ),
            )
        });
        let tree_has_focus = tree_focus.has_focus();
        if tree_has_focus {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    egui::Id::unique("explorer-tree"),
                    egui::EventFilter {
                        tab: false,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }
        if self.workspace.folders.is_empty() {
            if ui.button("Open folder…").clicked() {
                self.open_folder(ui.ctx());
            }
            return;
        }
        self.prepare_explorer(ui.ctx());
        ui.add_space(6.0);
        let options = tree::Options {
            compact: self.settings.explorer_compact_folders,
            draft: self
                .project
                .create
                .as_ref()
                .filter(|d| d.inline && d.rename.is_none())
                .map(|d| d.parent.as_str()),
            rename: self
                .project
                .create
                .as_ref()
                .filter(|d| d.inline)
                .and_then(|d| d.rename.as_ref())
                .and_then(|p| p.to_str()),
        };
        if self.project.tree.as_ref().is_none_or(|tree| {
            !tree.current_with(&self.workspace.folders, &self.project.expanded, options)
        }) {
            self.project.tree = Some(Arc::new(tree::Tree::build_with(
                &self.workspace.folders,
                &self.project.expanded,
                &self.project.listings,
                self.workspace.remote.as_deref(),
                self.project.listing_limited,
                options,
            )));
            let tree = self.project.tree.as_ref().unwrap();
            if let Some(selected) = self
                .explorer
                .selected
                .as_ref()
                .filter(|_| tree.missing.is_empty())
                .filter(|path| {
                    !tree.rows.iter().any(|n| {
                        n.path == **path
                            && !matches!(n.kind, tree::Kind::Message { .. } | tree::Kind::Draft)
                    })
                })
            {
                let replacement=tree.rows.iter().find(|n|matches!(&n.kind,tree::Kind::Entry {origin:Some(origin),..} if std::path::Path::new(selected).starts_with(origin) && std::path::Path::new(&n.path).starts_with(selected)))
                    .or_else(||tree.rows.iter().filter(|n|!matches!(n.kind,tree::Kind::Message{..}|tree::Kind::Draft) && std::path::Path::new(selected).starts_with(&n.path)).max_by_key(|n|n.path.len()))
                    .or_else(||tree.rows.first());
                if let Some(node) = replacement {
                    self.explorer.selected = Some(node.path.clone());
                }
            }
            let before = self.project.listings.len();
            self.project
                .listings
                .retain(|path, _| tree.used.contains(path));
            if self.project.listings.len() < before && self.project.listing_limited {
                self.project.listing_limited = false;
                self.project.tree = None;
                ui.ctx().request_repaint();
                return;
            }
        }
        let tree = self.project.tree.as_ref().unwrap().clone();
        for path in &tree.missing {
            if self.project.active_listings >= 4 {
                break;
            }
            self.project.request_listing(ui.ctx(), path);
        }
        if self
            .project
            .reveal
            .as_ref()
            .is_some_and(|(_, until)| Instant::now() > *until)
        {
            self.project.reveal = None;
        }
        if self.explorer.scroll {
            if let Some(path) = self.explorer.selected.clone() {
                self.project.reveal = Some((path, Instant::now() + Duration::from_secs(3)));
            }
            self.explorer.scroll = false;
        }
        let reveal = self.project.reveal.as_ref().and_then(|(path, _)| {
            tree.rows.iter().position(|row| {
                row.path == *path && !matches!(row.kind, tree::Kind::Message { .. })
            })
        });
        let row_height = hedit::ui_style::Metrics::for_ui(ui).row;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("folder-tree")
            .auto_shrink([false, false]);
        if let Some(index) = reveal {
            let stride = row_height + ui.spacing().item_spacing.y;
            scroll = scroll.vertical_scroll_offset(
                (index as f32 * stride - ui.available_height() / 2.0).max(0.0),
            );
            self.project.reveal = None;
        }
        scroll.show_rows(ui, row_height, tree.rows.len(), |ui, rows| {
            for index in rows {
                let node = &tree.rows[index];
                if let tree::Kind::Message { warning } = node.kind {
                    let color = if warning {
                        self.theme.warning
                    } else {
                        self.theme.muted
                    };
                    ui.add_sized(
                        [ui.available_width(), row_height],
                        egui::Label::new(RichText::new(&node.name).color(color.color())).truncate(),
                    )
                    .on_hover_text(&node.name);
                    continue;
                }
                if matches!(node.kind, tree::Kind::Draft)
                    || self.project.create.as_ref().is_some_and(|d| {
                        d.inline
                            && d.rename
                                .as_ref()
                                .is_some_and(|p| p == std::path::Path::new(&node.path))
                    })
                {
                    self.explorer_entry_row(ui, node.depth);
                    continue;
                }
                let root = matches!(node.kind, tree::Kind::Root);
                let (directory, symlink) = match node.kind {
                    tree::Kind::Entry {
                        directory, symlink, ..
                    } => (directory, symlink),
                    _ => (true, false),
                };
                let scope = tree.scopes[tree
                    .scopes
                    .partition_point(|(start, _)| *start <= index)
                    .saturating_sub(1)]
                .1;
                let git = self
                    .explorer
                    .badges
                    .files
                    .get(std::path::Path::new(&node.path))
                    .copied();
                let diagnostic = self
                    .explorer
                    .badges
                    .diagnostics
                    .get(std::path::Path::new(&node.path))
                    .copied();
                let badge = match (git, diagnostic) {
                    (Some(g), Some((n, _))) => format!("{g} · {n}"),
                    (Some(g), None) => g.to_string(),
                    (None, Some((n, _))) => n.to_string(),
                    _ => String::new(),
                };
                let hint = match (git, diagnostic) {
                    (Some(g), Some((n, _))) => format!("Git status: {g}; {n} diagnostics"),
                    (Some(g), None) => format!("Git status: {g}"),
                    (None, Some((n, _))) => format!("{n} diagnostics"),
                    _ => String::new(),
                };
                let color = if diagnostic.is_some_and(|(_, s)| s == 1) || git == Some('!') {
                    self.theme.error.color()
                } else if diagnostic.is_some_and(|(_, s)| s == 2) || git == Some('M') {
                    self.theme.warning.color()
                } else {
                    self.theme.accent.color()
                };
                let response = ui
                    .push_id(scope, |ui| {
                        tree_row::show(
                            ui,
                            &self.theme,
                            tree_row::Row {
                                name: &node.name,
                                path: &node.path,
                                icon_name: node
                                    .name
                                    .split_once(" / ")
                                    .map_or(&node.name, |(first, _)| first),
                                directory,
                                expanded: node.expanded,
                                selected: if tree_has_focus {
                                    self.explorer.selected.as_deref() == Some(&node.path)
                                } else {
                                    !directory
                                        && self.active().page.info.path.as_deref()
                                            == Some(&node.path)
                                },
                                depth: node.depth,
                                symlink,
                                badge: (!badge.is_empty()).then_some((badge.as_str(), color)),
                                badge_hint: (!hint.is_empty()).then_some(hint.as_str()),
                                roving: true,
                            },
                        )
                    })
                    .inner;
                if response.clicked() {
                    self.explorer.selected = Some(node.path.clone());
                    ui.ctx()
                        .memory_mut(|m| m.request_focus(egui::Id::unique("explorer-tree")));
                    self.focus_editor = false;
                    if root || directory {
                        self.toggle_explorer_node(node);
                    } else {
                        self.quality.preview_request = !response.double_clicked();
                        if response.double_clicked() {
                            self.quality.preview = None;
                        }
                        self.pending_paths.push_back(PathBuf::from(&node.path));
                    }
                    ui.ctx().request_repaint();
                }
                response.context_menu(|ui| self.folder_context_menu(ui, &node.path, directory));
            }
        });
    }
    fn folder_context_menu(&mut self, ui: &mut egui::Ui, path: &str, directory: bool) {
        if directory {
            if ui.button("New file…").clicked() {
                self.start_explorer_entry(ui.ctx(), path.to_owned(), false, false);
                ui.close();
            }
            if ui.button("New folder…").clicked() {
                self.start_explorer_entry(ui.ctx(), path.to_owned(), true, false);
                ui.close();
            }
            if ui
                .add_enabled(
                    self.command_enabled(Command::FindFiles),
                    egui::Button::new("Find in This Folder"),
                )
                .clicked()
            {
                self.set_folder_search_scope(Some(PathBuf::from(path)));
                self.show_search_sidebar();
                ui.close();
            }
        }
        for (label, operation) in [
            ("Rename…", quality::FileOperation::Rename),
            ("Move…", quality::FileOperation::Move),
            ("Duplicate…", quality::FileOperation::Duplicate),
            (
                if self.settings.confirm_trash {
                    "Move to Trash…"
                } else {
                    "Move to Trash"
                },
                quality::FileOperation::Trash,
            ),
            ("Delete…", quality::FileOperation::Delete),
        ] {
            if ui.button(label).clicked() {
                if operation == quality::FileOperation::Rename {
                    self.start_explorer_entry(ui.ctx(), path.to_owned(), directory, true);
                } else {
                    self.begin_file_action(ui.ctx(), PathBuf::from(path), operation);
                }
                ui.close();
            }
        }
        if cfg!(feature = "terminal")
            && ui
                .add_enabled(
                    self.command_enabled(Command::NewTerminal),
                    egui::Button::new("Open Terminal Here"),
                )
                .on_disabled_hover_text("Enable the terminal in Settings to start a shell.")
                .clicked()
        {
            let path = std::path::Path::new(path);
            if let Some(directory) = if directory { Some(path) } else { path.parent() } {
                self.new_terminal_in(ui.ctx(), Some(directory.to_path_buf()));
            }
            ui.close();
        }
        if ui.button("Reveal in file manager").clicked() {
            let path = path.to_owned();
            self.job(ui.ctx(), move || {
                hedit::folder::reveal_in_file_manager(path)?;
                Ok(Action::Nothing)
            });
            ui.close();
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
            ui.close();
        }
        if ui.button("Copy path").clicked() {
            ui.ctx().copy_text(path.to_owned());
            ui.close();
        }
        if ui.button("Refresh folder").clicked() {
            self.refresh_folder();
            ui.close();
        }
        if self.workspace.folders.iter().any(|f| f.path == path)
            && ui.button("Remove folder from workspace").clicked()
        {
            self.remove_folder(path);
            ui.close();
        }
    }

    pub(super) fn folder_dialog(&mut self, ctx: &egui::Context) {
        let Some(create) = &mut self.project.create else {
            return;
        };
        if create.inline {
            return;
        }
        let mut open = true;
        let mut choice = controls::Choice::None;
        egui::Window::new(if create.directory {
            "New folder"
        } else {
            "New file"
        })
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(320.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(&create.parent);
            let response = ui.add(egui::TextEdit::singleline(&mut create.name).hint_text("Name"));
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if create.focus {
                response.request_focus();
                create.focus = false;
            }
            let named = !create.name.trim().is_empty();
            choice = controls::dialog_buttons(ui, &self.theme, "Create", named, enter);
        });
        if choice == controls::Choice::Confirm && !self.busy && !self.closing {
            let create = self.project.create.take().unwrap();
            self.job(ctx, move || {
                let result = if create.directory {
                    folder::create_folder(create.parent, create.name)
                } else {
                    folder::create_file(create.parent, create.name)
                };
                result.map(|path| Action::Created(path, create.directory))
            });
        } else if choice == controls::Choice::Cancel || !open {
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
        browser.invalidate();
        browser.poll();
        assert!(browser.listings.is_empty());
        assert!(browser.search.is_none());
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
    fn stale_listing_requests_still_count_against_the_physical_worker_limit() {
        let mut browser = ProjectBrowser::new(&[]);
        browser.active_listings = 4;
        browser.invalidate();
        browser.request_listing(&egui::Context::default(), "/new");
        assert!(browser.pending.is_empty());
        assert_eq!(browser.active_listings, 4);
        browser
            .tx
            .send(ListingResult {
                generation: 0,
                path: "/old".into(),
                listing: Err("cancelled".into()),
            })
            .unwrap();
        browser.poll();
        assert_eq!(browser.active_listings, 3);
        assert!(browser.listings.is_empty());
    }

    #[test]
    fn directory_cache_rejects_metadata_over_its_budget() {
        let mut browser = ProjectBrowser::new(&[]);
        browser
            .tx
            .send(ListingResult {
                generation: 0,
                path: "/huge".into(),
                listing: Err("x".repeat(16 * 1024 * 1024)),
            })
            .unwrap();
        browser.poll();
        assert!(browser.listing_limited);
        assert!(browser.listings.is_empty());
        browser.invalidate();
        assert!(!browser.listing_limited);
    }

    #[test]
    // The native AccessKit bridge is enabled only in the macOS build.
    #[cfg(target_os = "macos")]
    fn sidebar_only_registers_visible_rows_and_can_reveal_the_last_file() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.settings.show_folders = true;
        app.workspace.folders.push(FolderView {
            path: "/root".into(),
            visible: true,
            collapsed: false,
            expanded: vec![],
        });
        app.project.listings.insert(
            "/root".into(),
            Arc::new(Ok(FolderListing {
                path: "/root".into(),
                name: "root".into(),
                truncated: false,
                unsupported_names: false,
                entries: (0..2000)
                    .map(|i| folder::FolderEntry {
                        name: format!("source_{i:04}.rs"),
                        path: format!("/root/source_{i:04}.rs"),
                        is_directory: false,
                        is_symlink: false,
                    })
                    .collect(),
            })),
        );
        let render = |app: &mut HeditApp| {
            crate::app::tests::run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(300.0, 300.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show_in_context(ctx, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        app.folder_tree(ui);
                    });
                },
            )
        };
        render(&mut app);
        let output = render(&mut app);
        let nodes = output.platform_output.accesskit_update.unwrap().nodes;
        let files: Vec<_> = nodes
            .iter()
            .filter_map(|(_, n)| n.label())
            .filter(|n| n.starts_with("source_"))
            .collect();
        assert!(files.len() < 20, "registered {} file widgets", files.len());
        assert!(files.contains(&"source_0000.rs"));
        assert!(!files.contains(&"source_1999.rs"));
        app.project.reveal = Some((
            "/root/source_1999.rs".into(),
            Instant::now() + Duration::from_secs(2),
        ));
        render(&mut app);
        let output = render(&mut app);
        assert!(output
            .platform_output
            .accesskit_update
            .unwrap()
            .nodes
            .iter()
            .any(|(_, n)| n.label() == Some("source_1999.rs")));
        assert!(app.project.reveal.is_none());
        app.workspace.folders[0].collapsed = true;
        let output = render(&mut app);
        assert!(!output
            .platform_output
            .accesskit_update
            .unwrap()
            .nodes
            .iter()
            .any(|(_, n)| n.label().is_some_and(|l| l.starts_with("source_"))));
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
            let _ = crate::app::tests::run_pass(
                &ctx,
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
        while app.busy || app.opens.pending() || !app.pending_paths.is_empty() {
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

#[cfg(test)]
mod performance {
    use super::*;
    use crate::app::tests::frame;
    #[test]
    #[ignore = "manual CPU profile for a large expanded folder sidebar"]
    fn profile_large_folder_sidebar() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.settings.show_sidebar = true;
        app.settings.show_folders = true;
        for n in 0..5 {
            let root = directory.path().join(format!("root-{n}"));
            let path = root.to_string_lossy().into_owned();
            app.workspace.folders.push(FolderView {
                path: path.clone(),
                visible: true,
                expanded: vec![],
                collapsed: false,
            });
            let entries = (0..2000)
                .map(|i| folder::FolderEntry {
                    name: format!("source_{i:04}.rs"),
                    path: root
                        .join(format!("source_{i:04}.rs"))
                        .to_string_lossy()
                        .into_owned(),
                    is_directory: false,
                    is_symlink: false,
                })
                .collect();
            app.project.listings.insert(
                path.clone(),
                Arc::new(Ok(FolderListing {
                    name: format!("root-{n}"),
                    path,
                    entries,
                    truncated: false,
                    unsupported_names: false,
                })),
            );
        }
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let start = Instant::now();
        for _ in 0..30 {
            frame(&ctx, &mut app, vec![]);
        }
        eprintln!(
            "Expanded sidebar with 10000 files: {:.2} ms/frame (30 warm CPU frames)",
            start.elapsed().as_secs_f64() * 1000.0 / 30.0
        );
    }
}
