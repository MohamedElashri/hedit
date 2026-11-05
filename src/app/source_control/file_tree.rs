//! Path-based, compact folder trees for working changes and commit review.
use super::*;
use std::ffi::OsString;

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    depth: usize,
    file: Option<usize>,
    count: usize,
}
#[derive(Default)]
struct Node {
    path: PathBuf,
    children: BTreeMap<OsString, Node>,
    file: Option<usize>,
}
impl Node {
    fn flatten(&self, depth: usize, entries: &mut Vec<Entry>) -> usize {
        let mut node = self;
        let mut name = self
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        while node.file.is_none() && node.children.len() == 1 {
            let child = node.children.values().next().unwrap();
            if child.file.is_some() {
                break;
            }
            name.push('/');
            name.push_str(&child.path.file_name().unwrap_or_default().to_string_lossy());
            node = child;
        }
        let index = entries.len();
        entries.push(Entry {
            path: node.path.clone(),
            name,
            depth,
            file: node.file,
            count: 0,
        });
        let mut count = usize::from(node.file.is_some());
        // Folders precede files, with stable platform-native path ordering.
        for directory in [true, false] {
            for child in node
                .children
                .values()
                .filter(|n| n.file.is_none() == directory)
            {
                count += child.flatten(depth + 1, entries);
            }
        }
        entries[index].count = count;
        count
    }
}

#[derive(Default)]
pub(super) struct FileTree {
    files: Vec<FileStatus>,
    entries: Vec<Entry>,
    collapsed: BTreeSet<PathBuf>,
    visible: Arc<Vec<Entry>>,
    query: Option<(String, bool)>,
}
impl FileTree {
    fn update(&mut self, files: &[FileStatus]) {
        if self.files == files {
            return;
        }
        self.files = files.to_vec();
        self.query = None;
        let mut root = Node::default();
        for (index, file) in files.iter().enumerate() {
            let mut node = &mut root;
            let mut path = PathBuf::new();
            for component in file.path.components() {
                path.push(component);
                node = node
                    .children
                    .entry(component.as_os_str().into())
                    .or_insert_with(|| Node {
                        path: path.clone(),
                        ..Default::default()
                    });
            }
            node.file = Some(index);
        }
        self.entries.clear();
        for directory in [true, false] {
            for child in root
                .children
                .values()
                .filter(|n| n.file.is_none() == directory)
            {
                child.flatten(0, &mut self.entries);
            }
        }
    }
    fn visible(&mut self, filter: &str, tree: bool) -> Arc<Vec<Entry>> {
        if self
            .query
            .as_ref()
            .is_some_and(|(old, mode)| old == filter && *mode == tree)
        {
            return self.visible.clone();
        }
        let mut matches = BTreeSet::new();
        for file in self.files.iter().filter(|_| !filter.is_empty()) {
            if matches_filter(&file.path.to_string_lossy(), filter) {
                matches.extend(file.path.ancestors().map(Path::to_path_buf));
            }
        }
        self.visible = Arc::new(
            self.entries
                .iter()
                .filter(|entry| {
                    (filter.is_empty() || matches.contains(&entry.path))
                        && (tree || entry.file.is_some())
                        && (!tree
                            || !filter.is_empty()
                            || !entry
                                .path
                                .ancestors()
                                .skip(1)
                                .any(|p| self.collapsed.contains(p)))
                })
                .cloned()
                .collect(),
        );
        self.query = Some((filter.into(), tree));
        self.visible.clone()
    }
}

impl HeditApp {
    pub(super) fn git_tree_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.source_control.tree_view, true, "Tree");
            ui.selectable_value(&mut self.source_control.tree_view, false, "List");
            if self.source_control.tree_view && ui.small_button("Expand all").clicked() {
                for tree in self.source_control.trees.values_mut() {
                    tree.collapsed.clear();
                    tree.query = None;
                }
            }
        });
    }

    pub(super) fn git_file_tree(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        id: &str,
        files: &[FileStatus],
        kind: DiffKind,
    ) {
        let tree_view = self.source_control.tree_view;
        let tree = self.source_control.trees.entry(id.into()).or_default();
        tree.update(files);
        let filter = if id == "commit-files" {
            &self.source_control.commit_file_filter
        } else {
            &self.source_control.filter
        };
        let rows = tree.visible(filter, tree_view);
        let filtered = !filter.is_empty();
        if rows.is_empty() && !files.is_empty() {
            ui.label(
                RichText::new("No matching files")
                    .small()
                    .color(self.theme.muted.color()),
            );
            return;
        }
        // Both layouts mount only visible rows, even in repositories with many
        // changed files. Folder state is keyed by raw paths, never display text.
        egui::ScrollArea::vertical()
            .id_salt(("git-tree", id))
            .max_height(350.0)
            .auto_shrink([false, true])
            .show_rows(ui, 23.0, rows.len(), |ui, range| {
                for row in &rows[range] {
                    if let Some(index) = row.file {
                        let file = &files[index];
                        let kind = if kind == DiffKind::Working && file.untracked() {
                            DiffKind::Untracked
                        } else {
                            kind.clone()
                        };
                        self.git_file_row_at(
                            ui,
                            ctx,
                            file,
                            kind.clone(),
                            if tree_view { row.depth } else { 0 },
                            !tree_view,
                        );
                    } else {
                        let expanded = !self.source_control.trees[id].collapsed.contains(&row.path)
                            || filtered;
                        let response = ui
                            .push_id(&row.path, |ui| {
                                tree_row::show(
                                    ui,
                                    &self.theme,
                                    tree_row::Row {
                                        name: &format!("{}  ({})", row.name, row.count),
                                        path: &row.path.to_string_lossy(),
                                        directory: true,
                                        expanded,
                                        selected: false,
                                        depth: row.depth,
                                        symlink: false,
                                    },
                                )
                            })
                            .inner;
                        if response.clicked() {
                            let tree = self.source_control.trees.get_mut(id).unwrap();
                            tree.query = None;
                            if !tree.collapsed.remove(&row.path) {
                                tree.collapsed.insert(row.path.clone());
                            }
                        }
                        if response.has_focus()
                            && ui.input(|i| {
                                i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowRight)
                            })
                        {
                            self.source_control.trees.get_mut(id).unwrap().query = None;
                            if ui.input(|i| i.key_pressed(Key::ArrowLeft)) {
                                self.source_control
                                    .trees
                                    .get_mut(id)
                                    .unwrap()
                                    .collapsed
                                    .insert(row.path.clone());
                            }
                            if ui.input(|i| i.key_pressed(Key::ArrowRight)) {
                                self.source_control
                                    .trees
                                    .get_mut(id)
                                    .unwrap()
                                    .collapsed
                                    .remove(&row.path);
                            }
                        }
                    }
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compacts_paths_preserves_distinct_files_and_reveals_filtered_descendants() {
        let files: Vec<_> = [
            "src/app/ui/one.rs",
            "src/app/ui/two.rs",
            "tests/one.rs",
            "README.md",
        ]
        .iter()
        .map(|p| FileStatus {
            path: p.into(),
            original: None,
            index: '.',
            worktree: 'M',
            conflict: false,
        })
        .collect();
        let mut tree = FileTree::default();
        tree.update(&files);
        assert_eq!(tree.entries[0].name, "src/app/ui");
        assert_eq!(tree.entries[0].count, 2);
        tree.collapsed.insert("src/app/ui".into());
        assert_eq!(tree.visible("", true).len(), 4);
        let found = tree.visible("one.rs", true);
        assert_eq!(found.len(), 4);
        assert_eq!(found.iter().filter(|r| r.file.is_some()).count(), 2);
        assert_eq!(tree.visible("src/app/ui/two", true).len(), 2);
        assert_eq!(tree.visible("", false).len(), 4);
        assert!(tree.visible("missing", true).is_empty());
    }
}
