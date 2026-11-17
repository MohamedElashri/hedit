//! Cache the expanded hierarchy separately from viewport widgets. Paths keep
//! row identities stable when rows above them expand, collapse or arrive.
use super::*;
const MAX_ROWS: usize = 50_000;
const MAX_BYTES: usize = 8 * 1024 * 1024;
#[derive(Clone)]
pub(super) enum Kind {
    Root,
    Entry { directory: bool, symlink: bool },
    Message { warning: bool },
}
pub(super) struct Node {
    pub name: String,
    pub path: String,
    pub depth: usize,
    pub expanded: bool,
    pub kind: Kind,
}
pub(super) struct Tree {
    pub rows: Vec<Node>,
    pub missing: Vec<String>,
    pub used: HashSet<String>,
    pub scopes: Vec<(usize, egui::Id)>,
    cache_limited: bool,
    roots: Vec<(String, bool)>,
    expanded: BTreeSet<String>,
    bytes: usize,
    limited: bool,
}
impl Tree {
    pub fn current(&self, roots: &[FolderView], expanded: &BTreeSet<String>) -> bool {
        self.expanded == *expanded
            && self.roots.len() == roots.len()
            && self
                .roots
                .iter()
                .zip(roots)
                .all(|((path, collapsed), root)| *path == root.path && *collapsed == root.collapsed)
    }
    pub fn build(
        roots: &[FolderView],
        expanded: &BTreeSet<String>,
        listings: &HashMap<String, Arc<Result<FolderListing, String>>>,
        remote: Option<&hedit::remote::Connection>,
        cache_limited: bool,
    ) -> Self {
        let mut tree = Self {
            rows: vec![],
            missing: vec![],
            scopes: vec![],
            used: roots.iter().map(|r| r.path.clone()).collect(),
            cache_limited,
            roots: roots
                .iter()
                .map(|f| (f.path.clone(), f.collapsed))
                .collect(),
            expanded: expanded.clone(),
            bytes: 0,
            limited: false,
        };
        for root in roots {
            tree.scopes
                .push((tree.rows.len(), egui::Id::new(&root.path)));
            let display = remote
                .filter(|r| r.root() == std::path::Path::new(&root.path))
                .map(|r| r.config.root.as_str())
                .unwrap_or(&root.path);
            let name = std::path::Path::new(display)
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new(display))
                .to_string_lossy()
                .into_owned();
            if !tree.push(Node {
                name,
                path: root.path.clone(),
                depth: 0,
                expanded: !root.collapsed,
                kind: Kind::Root,
            }) {
                break;
            }
            if !root.collapsed {
                tree.directory(&root.path, 1, listings);
            }
        }
        if tree.limited {
            tree.rows.push(Node {
                name: "Tree limit reached. Collapse folders to show other entries.".into(),
                path: String::new(),
                depth: 0,
                expanded: false,
                kind: Kind::Message { warning: true },
            });
        }
        tree
    }
    fn push(&mut self, row: Node) -> bool {
        let bytes = row.name.len() + row.path.len() + std::mem::size_of::<Node>();
        if self.rows.len() >= MAX_ROWS || self.bytes + bytes > MAX_BYTES {
            self.limited = true;
            return false;
        }
        self.bytes += bytes;
        self.rows.push(row);
        true
    }
    fn message(&mut self, path: &str, depth: usize, name: &str, warning: bool) {
        self.push(Node {
            name: name.into(),
            path: path.into(),
            depth,
            expanded: false,
            kind: Kind::Message { warning },
        });
    }
    fn directory(
        &mut self,
        path: &str,
        depth: usize,
        listings: &HashMap<String, Arc<Result<FolderListing, String>>>,
    ) {
        if self.limited {
            return;
        }
        if depth >= 32 {
            self.message(path, depth, "Folder nesting limit reached.", true);
            return;
        }
        self.used.insert(path.into());
        let Some(listing) = listings.get(path) else {
            if self.cache_limited {
                self.message(
                    path,
                    depth,
                    "Folder limit reached. Collapse folders and refresh.",
                    true,
                );
                return;
            }
            self.missing.push(path.into());
            self.message(path, depth, "Loading…", false);
            return;
        };
        let listing = match listing.as_ref() {
            Ok(listing) => listing,
            Err(error) => {
                self.message(path, depth, error, true);
                return;
            }
        };
        if listing.entries.is_empty() {
            self.message(path, depth, "Empty folder", false);
        }
        for entry in &listing.entries {
            let expanded = entry.is_directory && self.expanded.contains(&entry.path);
            if !self.push(Node {
                name: entry.name.clone(),
                path: entry.path.clone(),
                depth,
                expanded,
                kind: Kind::Entry {
                    directory: entry.is_directory,
                    symlink: entry.is_symlink,
                },
            }) {
                break;
            }
            if expanded {
                self.directory(&entry.path, depth + 1, listings);
            }
        }
        if listing.truncated {
            self.message(
                path,
                depth,
                "Showing up to 2,000 entries in this folder.",
                true,
            );
        }
        if listing.unsupported_names {
            self.message(
                path,
                depth,
                "Some names cannot be displayed as UTF-8.",
                true,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root(path: &str) -> FolderView {
        FolderView {
            path: path.into(),
            collapsed: false,
            expanded: vec![],
            visible: true,
        }
    }
    fn listing(
        path: &str,
        entries: Vec<folder::FolderEntry>,
    ) -> Arc<Result<FolderListing, String>> {
        Arc::new(Ok(FolderListing {
            path: path.into(),
            name: path.into(),
            entries,
            truncated: false,
            unsupported_names: false,
        }))
    }
    #[test]
    fn nested_rows_have_stable_root_scopes_and_collapse_without_losing_expansions() {
        let roots = [root("/root"), root("/root/child")];
        let expanded = BTreeSet::from(["/root/child".into()]);
        let listings = HashMap::from([
            (
                "/root".into(),
                listing(
                    "/root",
                    vec![folder::FolderEntry {
                        name: "child".into(),
                        path: "/root/child".into(),
                        is_directory: true,
                        is_symlink: false,
                    }],
                ),
            ),
            ("/root/child".into(), listing("/root/child", vec![])),
        ]);
        let tree = Tree::build(&roots, &expanded, &listings, None, false);
        assert!(tree.current(&roots, &expanded));
        assert_eq!(tree.scopes.len(), 2);
        assert_ne!(tree.scopes[0].1, tree.scopes[1].1);
        assert_eq!(tree.rows[1].depth, 1);
        assert_eq!(tree.rows[2].depth, 2);
        let mut collapsed = roots.clone();
        collapsed[0].collapsed = true;
        assert!(!tree.current(&collapsed, &expanded));
        let tree = Tree::build(&collapsed, &expanded, &listings, None, false);
        assert_eq!(tree.scopes[1].0, 1);
        assert!(tree.current(&collapsed, &expanded));
    }
    #[test]
    fn loading_errors_empty_truncated_and_cache_limit_have_explicit_rows() {
        let roots = [root("/loading"), root("/error"), root("/empty")];
        let mut empty = FolderListing {
            path: "/empty".into(),
            name: "empty".into(),
            entries: vec![],
            truncated: true,
            unsupported_names: true,
        };
        let listings = HashMap::from([
            ("/error".into(), Arc::new(Err("Permission denied".into()))),
            ("/empty".into(), Arc::new(Ok(empty.clone()))),
        ]);
        let tree = Tree::build(&roots, &BTreeSet::new(), &listings, None, false);
        assert_eq!(tree.missing, vec!["/loading"]);
        for label in [
            "Loading…",
            "Permission denied",
            "Empty folder",
            "Showing up to 2,000 entries in this folder.",
            "Some names cannot be displayed as UTF-8.",
        ] {
            assert!(tree.rows.iter().any(|r| r.name == label), "missing {label}");
        }
        empty.entries.clear();
        let tree = Tree::build(&roots, &BTreeSet::new(), &listings, None, true);
        assert!(tree.missing.is_empty());
        assert!(tree
            .rows
            .iter()
            .any(|r| r.name.contains("Folder limit reached")));
    }
    #[test]
    fn symlink_cycles_and_large_metadata_are_bounded() {
        let roots = [root("/root")];
        let listings = HashMap::from([(
            "/root".into(),
            listing(
                "/root",
                vec![folder::FolderEntry {
                    name: "self".into(),
                    path: "/root".into(),
                    is_directory: true,
                    is_symlink: true,
                }],
            ),
        )]);
        let tree = Tree::build(
            &roots,
            &BTreeSet::from(["/root".into()]),
            &listings,
            None,
            false,
        );
        assert!(tree.rows.len() <= 34);
        assert!(tree.rows.last().unwrap().name.contains("nesting limit"));
        let listings = HashMap::from([(
            "/root".into(),
            listing(
                "/root",
                (0..60_000)
                    .map(|i| folder::FolderEntry {
                        name: format!("file{i}"),
                        path: format!("/root/file{i}"),
                        is_directory: false,
                        is_symlink: false,
                    })
                    .collect(),
            ),
        )]);
        let tree = Tree::build(&roots, &BTreeSet::new(), &listings, None, false);
        assert!(tree.limited);
        assert!(tree.rows.len() <= MAX_ROWS + 1);
        assert!(tree.bytes <= MAX_BYTES);
        assert!(tree
            .rows
            .last()
            .unwrap()
            .name
            .contains("Tree limit reached"));
    }
}
