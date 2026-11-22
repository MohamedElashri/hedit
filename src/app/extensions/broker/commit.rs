//! Buffer preflight and reconciliation stay on the broker's commit I/O thread.
use crate::app::Action;
use hedit::{
    document::{Document, Page},
    Editor, SharedDocument, WorkspaceFreeze,
};
use hedit_broker::{plan::Plan, workspace};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::Sender,
        Arc,
    },
};
static BACKUP_BYTES: AtomicUsize = AtomicUsize::new(0);
const MAX_BACKUPS: usize = 64 * 1024 * 1024;
use super::limit::Reservation;
pub(in crate::app) struct Refresh {
    pub pages: Vec<Page>,
    pub conflicts: bool,
    pub changed: usize,
    pub applied: bool,
    pub files: usize,
    _reservations: Vec<Reservation>,
}
struct Buffer {
    document: SharedDocument,
    backup: Document,
    file: usize,
    was_before: bool,
    identity: Option<workspace::Identity>,
}
enum Changes {
    Plan(Arc<Plan>),
    Recovery(Arc<hedit_broker::journal::View>, std::path::PathBuf),
}
struct Change<'a> {
    before: &'a str,
    after: &'a str,
    identities: [workspace::Identity; 2],
    path: Option<std::path::PathBuf>,
}
impl Changes {
    fn len(&self) -> usize {
        match self {
            Self::Plan(plan) => plan.files().len(),
            Self::Recovery(view, _) => view.files.len(),
        }
    }
    fn file(&self, index: usize) -> Change<'_> {
        match self {
            Self::Plan(plan) => {
                let f = &plan.files()[index];
                Change {
                    before: f.before(),
                    after: f.after(),
                    identities: [f.identity(); 2],
                    path: None,
                }
            }
            Self::Recovery(view, root) => {
                let f = &view.files[index];
                Change {
                    before: f.before(),
                    after: f.after(),
                    identities: [f.original, f.replacement],
                    path: Some(root.join(&f.relative)),
                }
            }
        }
    }
}
struct Commit {
    changes: Changes,
    entries: Vec<Buffer>,
    freeze: Option<WorkspaceFreeze>,
    reservations: Vec<Reservation>,
    sender: Sender<Result<Action, String>>,
    wake: eframe::egui::Context,
    lease: u128,
    signals: Signals,
}
#[derive(Clone, Default)]
pub(super) struct Signals {
    pub committing: Arc<AtomicBool>,
    pub applied: Arc<AtomicBool>,
}
impl hedit_broker::host::WorkspaceCommit for Commit {
    fn applied(&mut self) {
        self.signals.applied.store(true, Ordering::Release);
    }
}
fn error(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}
pub(super) fn prepare(
    editor: &Arc<Editor>,
    plan: &Arc<Plan>,
    cancel: &AtomicBool,
    sender: Sender<Result<Action, String>>,
    wake: eframe::egui::Context,
    following: &std::collections::BTreeSet<u64>,
    signals: Signals,
) -> io::Result<Box<dyn hedit_broker::host::WorkspaceCommit>> {
    prepare_changes(
        editor,
        Changes::Plan(plan.clone()),
        cancel,
        sender,
        wake,
        following,
        signals,
    )
}
pub(in crate::app::extensions) fn prepare_recovery(
    editor: &Arc<Editor>,
    view: Arc<hedit_broker::journal::View>,
    root: std::path::PathBuf,
    cancel: &AtomicBool,
    sender: Sender<Result<Action, String>>,
    wake: eframe::egui::Context,
    following: &std::collections::BTreeSet<u64>,
) -> io::Result<Box<dyn hedit_broker::host::WorkspaceCommit>> {
    prepare_changes(
        editor,
        Changes::Recovery(view, root),
        cancel,
        sender,
        wake,
        following,
        Signals::default(),
    )
}
#[allow(clippy::too_many_arguments)]
fn prepare_changes(
    editor: &Arc<Editor>,
    changes: Changes,
    cancel: &AtomicBool,
    sender: Sender<Result<Action, String>>,
    wake: eframe::egui::Context,
    following: &std::collections::BTreeSet<u64>,
    signals: Signals,
) -> io::Result<Box<dyn hedit_broker::host::WorkspaceCommit>> {
    let (generation, documents) = editor.document_set().map_err(error)?;
    if documents.len() > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::OutOfMemory,
            "Too many open buffers for workspace preflight.",
        ));
    }
    let started = std::time::Instant::now();
    let mut entries = Vec::new();
    let mut expected = Vec::new();
    let mut reservations = Vec::new();
    for document in documents {
        if started.elapsed() >= std::time::Duration::from_secs(5) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Workspace buffer preflight timed out.",
            ));
        }
        if cancel.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Workspace commit cancelled.",
            ));
        }
        // Clone metadata and shared piece storage, then release the document
        // mutex before identities, text, pages, or other filesystem work.
        let snapshot = document
            .lock()
            .map_err(|_| error("Buffer lock failed."))?
            .workspace_snapshot();
        if snapshot.is_ephemeral() {
            continue;
        }
        let info = snapshot.info();
        let path = snapshot.local_path();
        let path_id = path.and_then(|path| workspace::local_path_identity(path).ok());
        let source_id = snapshot
            .source_file()
            .map(workspace::local_file_identity)
            .transpose()?;
        let matches: Vec<_> = (0..changes.len())
            .filter(|&index| {
                let file = changes.file(index);
                file.identities
                    .iter()
                    .any(|id| Some(*id) == path_id || Some(*id) == source_id)
                    || file
                        .path
                        .as_deref()
                        .is_some_and(|expected| path == Some(expected))
            })
            .collect();
        let Some(&file) = matches.first() else {
            continue;
        };
        if matches.len() != 1
            || following.contains(&info.id)
            || info.dirty
            || info.paged
            || info.encoding != "UTF-8"
            || path.is_none()
            || entries.len() >= 128
        {
            return Err(error("Close followed or paged files and save affected buffers before reviewing workspace edits."));
        }
        let text = snapshot
            .text(workspace::MAX_FILE_BYTES as u64)
            .map_err(error)?;
        let reviewed = changes.file(file);
        let before = reviewed.before;
        let expected_text = if info.bom {
            before.strip_prefix('\u{feff}')
        } else {
            Some(before)
        };
        let was_before = expected_text == Some(text.as_str());
        let after = if info.bom {
            reviewed.after.strip_prefix('\u{feff}')
        } else {
            Some(reviewed.after)
        };
        if !was_before
            && !(matches!(changes, Changes::Recovery(..)) && after == Some(text.as_str()))
        {
            return Err(error(
                "An affected buffer differs from the workspace preview.",
            ));
        }
        reservations.push(Reservation::reserve(
            &BACKUP_BYTES,
            text.len()
                .max(reviewed.after.len())
                .saturating_mul(3)
                .saturating_add(8192),
            MAX_BACKUPS,
        )?);
        let backup = snapshot
            .workspace_backup(workspace::MAX_FILE_BYTES as u64)
            .map_err(error)?;
        expected.push((info.id, info.revision));
        entries.push(Buffer {
            document,
            backup,
            file,
            was_before,
            identity: path_id,
        });
    }
    let mut nonce = [0; 16];
    getrandom::fill(&mut nonce).map_err(|_| error("Cannot create a workspace lease."))?;
    let lease = u128::from_le_bytes(nonce);
    let freeze = editor
        .freeze_workspace(generation, lease, &expected)
        .map_err(error)?;
    Ok(Box::new(Commit {
        changes,
        entries,
        freeze: Some(freeze),
        reservations,
        sender,
        wake,
        lease,
        signals,
    }))
}
impl Drop for Commit {
    fn drop(&mut self) {
        let mut pages = Vec::new();
        let mut conflicts = false;
        let mut changed = 0;
        for entry in &mut self.entries {
            let file = self.changes.file(entry.file);
            let info = entry.backup.info();
            let Some(path) = entry.backup.local_path() else {
                conflicts = true;
                continue;
            };
            let fresh = Document::open(info.id, path).and_then(|mut document| {
                if document.info().paged || document.info().size > workspace::MAX_FILE_BYTES as u64
                {
                    return Err("Workspace file grew beyond the reviewed limit.".into());
                }
                let page = document.page(0)?;
                let text = if page.info.bom {
                    format!("\u{feff}{}", page.text)
                } else {
                    page.text.clone()
                };
                if text != file.before && text != file.after {
                    return Err("Workspace file changed during commit.".into());
                }
                Ok((
                    document,
                    page,
                    if entry.was_before {
                        text == file.before
                    } else {
                        text == file.after
                    },
                ))
            });
            let outcome = match fresh {
                Ok((document, page, unchanged)) => {
                    let same_object = workspace::local_path_identity(path)
                        .is_ok_and(|id| Some(id) == entry.identity);
                    let same_source = entry
                        .document
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .same_workspace_source(&document);
                    if unchanged && same_object && same_source {
                        // Preserve the original Undo history when no replacement
                        // occurred, including failed preparation/cancellation.
                        Ok((None, page))
                    } else {
                        changed += 1;
                        Ok((Some(document), page))
                    }
                }
                Err(_) => {
                    conflicts = true;
                    let mut backup = entry.backup.clone();
                    backup.page(0).map(|page| (Some(backup), page))
                }
            };
            if let Ok((replacement, mut page)) = outcome {
                let mut document = entry.document.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(replacement) = replacement {
                    if document
                        .accept_workspace_copy(self.lease, replacement)
                        .is_err()
                    {
                        conflicts = true;
                        continue;
                    }
                }
                page.info = document.info();
                pages.push(page);
            } else {
                conflicts = true;
            }
        }
        self.freeze.take();
        let refresh = Refresh {
            pages,
            conflicts,
            changed,
            applied: self.signals.applied.load(Ordering::Acquire),
            files: self.changes.len(),
            _reservations: std::mem::take(&mut self.reservations),
        };
        let _ = self.sender.send(Ok(Action::ExtensionWorkspace(refresh)));
        self.signals.committing.store(false, Ordering::Release);
        self.wake.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hedit_broker::{
        resources::Resources,
        state::State,
        workspace::{Budget, Policy, Right, Root},
    };
    use hedit_plugin::sdk;
    fn fixture() -> (tempfile::TempDir, Arc<Editor>, Resources, Arc<Plan>, State) {
        fixture_named("text.txt")
    }
    fn fixture_named(name: &str) -> (tempfile::TempDir, Arc<Editor>, Resources, Arc<Plan>, State) {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("project");
        let config = directory.path().join("config");
        std::fs::create_dir(&project).unwrap();
        std::fs::create_dir(&config).unwrap();
        let path = project.join(name);
        std::fs::write(&path, "old café / 東京\r\n").unwrap();
        let state = State::open(&config, []).unwrap();
        let root = Root::open_selected(
            &project,
            Policy::new([Right::Read, Right::Edit]).unwrap(),
            state.protected(),
        )
        .unwrap();
        let mut resources = Resources::new(root).unwrap();
        let flag = AtomicBool::new(false);
        let mut budget = Budget::new(&flag, std::time::Duration::from_secs(10), 64 * 1024 * 1024);
        let resource = resources
            .load(resources.scope(), name, &mut budget)
            .unwrap();
        let plan = Arc::new(
            Plan::prepare(
                &resources,
                resources.scope(),
                vec![hedit_broker::plan::Change {
                    resource: resource.resource,
                    version: resource.version,
                    edits: vec![sdk::Edit {
                        start: 0,
                        finish: 3,
                        text: "new".into(),
                    }],
                }],
                &mut budget,
            )
            .unwrap(),
        );
        let editor = Arc::new(Editor::default());
        let mut document = Document::open(1, &path).unwrap();
        document.page(0).unwrap();
        editor.insert(document).unwrap();
        (directory, editor, resources, plan, state)
    }
    #[test]
    fn explicit_recovery_freezes_stale_and_missing_file_buffers_then_reconciles_originals() {
        for missing in [false, true] {
            let (_directory, editor, resources, plan, state) = fixture();
            let cancel = AtomicBool::new(false);
            let root = resources.root(plan.scope()).unwrap();
            let mut budget = Budget::new(
                &cancel,
                std::time::Duration::from_secs(10),
                64 * 1024 * 1024,
            );
            let journal = hedit_broker::journal::Journal::prepare(
                &resources,
                &plan,
                &mut budget,
                &state.recovery(),
            )
            .unwrap();
            if missing {
                std::fs::rename(
                    root.locator().join("text.txt"),
                    root.locator().join(journal.name()).join("0.old"),
                )
                .unwrap();
            } else {
                journal.apply(root, &mut budget, || Ok(())).unwrap();
            }
            let view = Arc::new(journal.inspect(root, &mut budget).unwrap());
            let (sender, receiver) = std::sync::mpsc::channel();
            let revision = editor.get(1).unwrap().lock().unwrap().info().revision;
            let guard = prepare_recovery(
                &editor,
                view,
                root.locator().to_owned(),
                &cancel,
                sender,
                Default::default(),
                &Default::default(),
            )
            .unwrap();
            assert!(editor.workspace_document_frozen(1));
            assert!(editor
                .get(1)
                .unwrap()
                .lock()
                .unwrap()
                .replace(0, 3, "own".into(), 0)
                .is_err());
            journal.restore(root, &mut budget, || Ok(())).unwrap();
            drop(guard);
            assert!(!editor.workspace_document_frozen(1));
            let doc = editor.get(1).unwrap();
            let doc = doc.lock().unwrap();
            assert_eq!(doc.text(1024).unwrap(), "old café / 東京\r\n");
            assert!(!doc.info().dirty);
            assert!(doc.info().revision > revision);
            assert!(matches!(
                receiver.try_recv().unwrap(),
                Ok(Action::ExtensionWorkspace(_))
            ));
        }
    }
    #[test]
    fn recovery_refuses_dirty_buffers_and_preserves_text_when_a_writer_races_restore() {
        let (_directory, editor, resources, plan, state) = fixture();
        let cancel = AtomicBool::new(false);
        let root = resources.root(plan.scope()).unwrap();
        let mut budget = Budget::new(
            &cancel,
            std::time::Duration::from_secs(10),
            64 * 1024 * 1024,
        );
        let journal = hedit_broker::journal::Journal::prepare(
            &resources,
            &plan,
            &mut budget,
            &state.recovery(),
        )
        .unwrap();
        let view = Arc::new(journal.inspect(root, &mut budget).unwrap());
        editor
            .get(1)
            .unwrap()
            .lock()
            .unwrap()
            .replace(0, 3, "own".into(), 0)
            .unwrap();
        let (sender, _) = std::sync::mpsc::channel();
        assert!(prepare_recovery(
            &editor,
            view.clone(),
            root.locator().to_owned(),
            &cancel,
            sender,
            Default::default(),
            &Default::default()
        )
        .is_err());
        assert!(!editor.workspace_document_frozen(1));
        {
            let doc = editor.get(1).unwrap();
            let mut doc = doc.lock().unwrap();
            let revision = doc.info().revision;
            doc.history(false, 0, revision).unwrap();
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let guard = prepare_recovery(
            &editor,
            view,
            root.locator().to_owned(),
            &cancel,
            sender,
            Default::default(),
            &Default::default(),
        )
        .unwrap();
        journal.apply(root, &mut budget, || Ok(())).unwrap();
        std::fs::write(root.locator().join("text.txt"), "external writer").unwrap();
        assert!(journal.restore(root, &mut budget, || Ok(())).is_err());
        drop(guard);
        assert!(!editor.workspace_document_frozen(1));
        let doc = editor.get(1).unwrap();
        let doc = doc.lock().unwrap();
        assert_eq!(doc.text(1024).unwrap(), "old café / 東京\r\n");
        assert!(doc.info().dirty);
        assert_eq!(
            std::fs::read_to_string(root.locator().join("text.txt")).unwrap(),
            "external writer"
        );
        let Ok(Action::ExtensionWorkspace(refresh)) = receiver.try_recv().unwrap() else {
            panic!("missing reconciliation")
        };
        assert!(refresh.conflicts);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn non_utf8_buffer_paths_cannot_alias_a_lossy_display_name() {
        use std::os::unix::ffi::OsStringExt;
        let (directory, editor, _resources, plan, _state) = fixture_named("t�ext.txt");
        let path = directory
            .path()
            .join("project")
            .join(std::ffi::OsString::from_vec(b"t\xffext.txt".to_vec()));
        std::fs::write(&path, "old café / 東京\r\n").unwrap();
        let mut unrelated = Document::open(2, &path).unwrap();
        unrelated.page(0).unwrap();
        unrelated.replace(0, 3, "own".into(), 0).unwrap();
        editor.insert(unrelated).unwrap();
        let (sender, _receiver) = std::sync::mpsc::channel();
        let guard = prepare(
            &editor,
            &plan,
            &AtomicBool::new(false),
            sender,
            Default::default(),
            &Default::default(),
            Signals::default(),
        )
        .unwrap();
        assert!(editor.workspace_document_frozen(1));
        assert!(!editor.workspace_document_frozen(2));
        drop(guard);
        assert_eq!(
            editor.get(2).unwrap().lock().unwrap().text(1024).unwrap(),
            "own café / 東京\r\n"
        );
    }
    #[test]
    fn journal_commit_reloads_clean_buffers_and_keeps_revisions_monotonic() {
        let (_directory, editor, resources, plan, state) = fixture();
        let (sender, receiver) = std::sync::mpsc::channel();
        let flag = AtomicBool::new(false);
        let guard = prepare(
            &editor,
            &plan,
            &flag,
            sender,
            Default::default(),
            &Default::default(),
            Signals::default(),
        )
        .unwrap();
        assert!(editor.workspace_document_frozen(1));
        let mut budget = Budget::new(&flag, std::time::Duration::from_secs(10), 64 * 1024 * 1024);
        let journal = hedit_broker::journal::Journal::prepare(
            &resources,
            &plan,
            &mut budget,
            &state.recovery(),
        )
        .unwrap();
        journal
            .apply(
                resources.root(resources.scope()).unwrap(),
                &mut budget,
                &mut || Ok(()),
            )
            .unwrap();
        drop(guard);
        assert!(!editor.workspace_document_frozen(1));
        let document = editor.get(1).unwrap();
        let document = document.lock().unwrap();
        assert_eq!(document.text(1024).unwrap(), "new café / 東京\r\n");
        assert_eq!(document.info().revision, 1);
        assert!(!document.info().dirty);
        let Ok(Action::ExtensionWorkspace(refresh)) = receiver.try_recv().unwrap() else {
            panic!()
        };
        assert!(!refresh.conflicts);
        assert_eq!(refresh.changed, 1);
        assert_eq!(refresh.pages[0].info.revision, 1);
    }
    #[test]
    fn conflicts_preserve_independent_original_text_and_all_error_paths_thaw() {
        for conflict in [false, true] {
            let (directory, editor, _resources, plan, _state) = fixture();
            let (sender, receiver) = std::sync::mpsc::channel();
            let guard = prepare(
                &editor,
                &plan,
                &AtomicBool::new(false),
                sender,
                Default::default(),
                &Default::default(),
                Signals::default(),
            )
            .unwrap();
            if conflict {
                std::fs::write(directory.path().join("project/text.txt"), "external writer")
                    .unwrap();
            }
            drop(guard);
            let shared = editor.get(1).unwrap();
            let document = shared.lock().unwrap();
            assert_eq!(document.text(1024).unwrap(), "old café / 東京\r\n");
            assert_eq!(document.info().dirty, conflict);
            assert_eq!(document.info().revision, u64::from(conflict));
            assert!(!document.workspace_frozen());
            let Ok(Action::ExtensionWorkspace(refresh)) = receiver.try_recv().unwrap() else {
                panic!()
            };
            assert_eq!(refresh.conflicts, conflict);
        }
    }
    #[test]
    fn dirty_followed_and_unavailable_consumers_cannot_bypass_preflight_or_strand_leases() {
        let (_directory, editor, _resources, plan, _state) = fixture();
        let (sender, receiver) = std::sync::mpsc::channel();
        let flag = AtomicBool::new(false);
        assert!(prepare(
            &editor,
            &plan,
            &flag,
            sender.clone(),
            Default::default(),
            &std::collections::BTreeSet::from([1]),
            Signals::default(),
        )
        .is_err());
        let shared = editor.get(1).unwrap();
        shared
            .lock()
            .unwrap()
            .replace(0, 3, "own".into(), 0)
            .unwrap();
        assert!(prepare(
            &editor,
            &plan,
            &flag,
            sender.clone(),
            Default::default(),
            &Default::default(),
            Signals::default(),
        )
        .is_err());
        assert!(!editor.workspace_document_frozen(1));
        let revision = shared.lock().unwrap().info().revision;
        shared.lock().unwrap().history(false, 0, revision).unwrap();
        let guard = prepare(
            &editor,
            &plan,
            &flag,
            sender,
            Default::default(),
            &Default::default(),
            Signals::default(),
        )
        .unwrap();
        drop(receiver);
        drop(guard);
        assert!(!editor.workspace_document_frozen(1));
        assert!(editor.check_membership_writable().is_ok());
    }
}
