//! UI-independent buffer ownership and session views.
use crate::{
    document::{Document, Page},
    session::{Bookmark, FolderView, SessionStore, SessionView, TabView},
    syntax::Language,
    Editor,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct Change {
    pub revision: u64,
    pub start: usize,
    pub end: usize,
    pub added: usize,
}
pub struct Buffer {
    pub page: Page,
    pub view: TabView,
    // Zero means this buffer is the only one using the fallback title.
    untitled_number: Option<usize>,
    pub external: bool,
    pub follow: bool,
    pub restore_cursor: bool,
    pub reveal_cursor: bool,
    pub changes: std::collections::VecDeque<Change>,
    pub cursor_undo: Vec<Vec<crate::advanced::Selection>>,
    pub cursor_redo: Vec<Vec<crate::advanced::Selection>>,
    pub pending_selection: Option<Vec<crate::advanced::Selection>>,
}
impl Buffer {
    pub fn language_override(&self) -> Option<Language> {
        self.view
            .language_override
            .as_deref()
            .and_then(Language::from_id)
    }
    pub fn language(&self) -> Language {
        self.language_override()
            .unwrap_or_else(|| Language::detect(&self.page.info.name))
    }
    /// Syntax is view metadata: it never edits text or creates an undo step.
    pub fn set_language(&mut self, language: Option<Language>) -> bool {
        let id = language.map(|language| language.id().to_owned());
        if self.view.language_override == id {
            return false;
        }
        self.view.language_override = id;
        true
    }
    pub fn new(page: Page, view: Option<TabView>) -> Self {
        let detect = view.is_none() && page.info.path.is_some();
        let mut view = view.unwrap_or_else(|| TabView {
            pinned: false,
            id: page.info.id,
            name: String::new(),
            page_start: page.start,
            anchor: 0,
            head: 0,
            selections: vec![],
            folds: vec![],
            scroll_top: 0.0,
            scroll_left: 0.0,
            language_override: None,
            indent: None,
            tab_size: Some(4),
            custom_name: None,
            bookmarks: vec![],
            markdown_view: Default::default(),
        });
        view.tab_size = Some(view.tab_size.unwrap_or(4).clamp(1, 8));
        if detect {
            let (tabs, width) = crate::advanced::detect_indentation(&page.text);
            view.tab_size = Some(width);
            view.indent = Some(if tabs { "tabs" } else { "spaces" }.into());
        }
        view.selections = crate::advanced::normalize(&view.selections, page.text.chars().count());
        view.selections
            .truncate(crate::advanced::MAX_SELECTIONS - 1);
        let mut buffer = Self {
            changes: std::collections::VecDeque::new(),
            cursor_undo: vec![],
            cursor_redo: vec![],
            pending_selection: None,
            page,
            view,
            untitled_number: None,
            external: false,
            follow: false,
            restore_cursor: true,
            reveal_cursor: false,
        };
        buffer.refresh_name();
        buffer
    }
    pub fn set_page(&mut self, page: Page) {
        if !page.info.paged
            && !self.page.info.paged
            && page.start == self.page.start
            && page.text != self.page.text
        {
            let (start, end, inserted) = splice(&self.page.text, &page.text);
            self.changes.push_back(Change {
                revision: self.page.info.revision,
                start: self.page.text[..start].chars().count(),
                end: self.page.text[..end].chars().count(),
                added: inserted.chars().count(),
            });
            if self.changes.len() > 256 {
                self.changes.pop_front();
            }
        } else if page.start != self.page.start {
            self.changes.clear();
        }
        self.view.selections.clear();
        self.view.folds.clear();
        self.view.page_start = page.start;
        self.page = page;
        self.refresh_name();
        self.restore_cursor = true;
    }
    pub fn refresh_name(&mut self) {
        let name = if self.page.info.path.is_none() {
            self.view
                .custom_name
                .clone()
                .or_else(|| draft_name(&self.page.text))
        } else {
            Some(self.page.info.name.clone())
        };
        if let Some(name) = name {
            self.view.name = name;
            self.untitled_number = None;
        } else if self.untitled_number.is_none() {
            self.set_untitled_number(0);
        }
    }
    fn set_untitled_number(&mut self, number: usize) {
        if self.untitled_number != Some(number) {
            self.untitled_number = Some(number);
            self.view.name = if number == 0 {
                "Untitled".into()
            } else {
                format!("Untitled {number}")
            };
        }
    }
    pub fn cursor_offset(&self) -> u64 {
        self.page.start
            + self
                .page
                .text
                .chars()
                .take(self.view.head)
                .map(char::len_utf8)
                .sum::<usize>() as u64
    }
}
pub struct Workspace {
    pub remote: Option<Arc<crate::remote::Connection>>,
    pub editor: Arc<Editor>,
    pub store: Arc<SessionStore>,
    pub open: Vec<Buffer>,
    pub closed: Vec<TabView>,
    pub active: usize,
    pub folders: Vec<FolderView>,
}
impl Workspace {
    pub fn connect_remote(
        directory: PathBuf,
        config: crate::remote::Config,
    ) -> Result<(Self, Vec<String>), String> {
        let remote = crate::remote::Connection::connect(config)?;
        let editor = Arc::new(Editor::default());
        let store = Arc::new(SessionStore::new(directory));
        let mut workspace = Self {
            remote: Some(remote.clone()),
            editor,
            store,
            open: vec![],
            closed: vec![],
            active: 0,
            folders: vec![FolderView {
                path: remote.root().to_string_lossy().into_owned(),
                visible: true,
                expanded: vec![],
                collapsed: false,
            }],
        };
        let saved = remote.metadata(None).ok().and_then(|value| {
            let saved_folders = value.get("folders").is_some() || value.get("folder").is_some();
            serde_json::from_value::<SessionView>(value)
                .ok()
                .map(|view| (view, saved_folders))
        });
        let mut identities = std::collections::HashMap::new();
        for info in remote.list()? {
            let host_id = info.document;
            let id = workspace.editor.id();
            identities.insert(host_id, id);
            let mut document = remote.recover(id, info)?;
            let page = document.page(0)?;
            workspace.add(document, page)?;
        }
        if let Some((saved, saved_folders)) = saved {
            if saved_folders {
                if let Some(folders) = restore_remote_folders(&remote, saved.folder, saved.folders)
                {
                    workspace.folders = folders;
                }
            }
            let mut buffers = std::mem::take(&mut workspace.open);
            let mut open = vec![];
            for mut view in saved.open {
                view.markdown_view = Default::default();
                if let Some(id) = identities.get(&view.id) {
                    view.id = *id;
                    if let Some(n) = buffers.iter().position(|b| b.view.id == *id) {
                        let mut b = buffers.remove(n);
                        if let Ok(doc) = workspace.editor.get(*id) {
                            if let Ok(mut doc) = doc.lock() {
                                if let Ok(page) = doc.page(view.page_start) {
                                    b = Buffer::new(page, Some(view));
                                }
                            }
                        }
                        open.push(b);
                    }
                }
            }
            for mut view in saved.closed {
                view.markdown_view = Default::default();
                if let Some(id) = identities.get(&view.id) {
                    view.id = *id;
                    if let Some(n) = buffers.iter().position(|b| b.view.id == *id) {
                        buffers.remove(n);
                        workspace.closed.push(view);
                    }
                }
            }
            // A cancelled read can finish on the host after the view was saved.
            // Keep its clean baseline recoverable without reopening an unwanted
            // tab; unlisted drafts must always remain visible after a crash.
            for buffer in buffers {
                if buffer.page.info.path.is_some() && !buffer.page.info.dirty {
                    workspace.closed.push(buffer.view);
                } else {
                    open.push(buffer);
                }
            }
            workspace.active = saved
                .active
                .and_then(|id| identities.get(&id))
                .and_then(|id| open.iter().position(|b| b.view.id == *id))
                .unwrap_or(0);
            workspace.open = open;
            workspace.refresh_untitled_names();
            for view in &workspace.closed {
                workspace.editor.close_document_lifetime(view.id)?;
            }
        }
        if workspace.open.is_empty() {
            let mut document = remote.scratch(workspace.editor.id())?;
            let page = document.page(0)?;
            workspace.add(document, page)?;
        }
        Ok((workspace, vec![]))
    }
    pub fn restore(directory: PathBuf) -> Result<(Self, Vec<String>), String> {
        let editor = Arc::new(Editor::default());
        let store = Arc::new(SessionStore::new(directory));
        let restored = store.restore(&editor)?;
        let mut errors = restored.errors;
        let mut open = Vec::new();
        let mut failed = Vec::new();
        let mut active = 0;
        for (index, tab) in restored.open.into_iter().enumerate() {
            match editor
                .get(tab.info.id)?
                .lock()
                .map_err(|e| e.to_string())?
                .page(tab.view.page_start)
            {
                Ok(page) => {
                    if index == restored.active_index {
                        active = open.len();
                    }
                    open.push(Buffer::new(page, Some(tab.view)));
                }
                Err(e) => {
                    errors.push(format!("{}: {e}. Retained in closed files.", tab.view.name));
                    failed.push(tab.view);
                }
            }
        }
        let mut closed: Vec<_> = restored.closed.into_iter().map(|t| t.view).collect();
        closed.extend(failed);
        // Older sessions may have shelved blank scratchpads. Keep disk files
        // and anything whose contents cannot safely be checked.
        closed.retain(|view| {
            let blank = editor
                .get(view.id)
                .ok()
                .and_then(|doc| {
                    doc.lock()
                        .ok()
                        .map(|d| d.info().path.is_none() && d.info().size == 0)
                })
                .unwrap_or(false);
            if blank {
                let _ = editor.remove(view.id);
            }
            !blank
        });
        for view in &closed {
            editor.close_document_lifetime(view.id)?;
        }
        let mut workspace = Self {
            remote: None,
            editor,
            store,
            open,
            closed,
            active,
            folders: if restored.folders.is_empty() {
                restored.folder.into_iter().collect()
            } else {
                restored.folders
            },
        };
        if workspace.open.is_empty() {
            workspace.new_buffer()?;
        } else {
            workspace.refresh_untitled_names();
        }
        Ok((workspace, errors))
    }
    pub fn add(&mut self, document: Document, page: Page) -> Result<(), String> {
        self.editor.insert(document)?;
        self.open.push(Buffer::new(page, None));
        self.active = self.open.len() - 1;
        if self.open[self.active].untitled_number.is_some() {
            self.refresh_untitled_names();
        }
        Ok(())
    }
    pub fn new_buffer(&mut self) -> Result<(), String> {
        if self.remote.is_some() {
            return Err(
                "Use New remote file in the SSH bar to create a host-backed scratchpad.".into(),
            );
        }
        let mut document = Document::empty(self.editor.id());
        let page = document.page(0)?;
        self.add(document, page)
    }
    /// Derive fallback titles from the current open tabs in workspace order.
    pub fn refresh_untitled_names(&mut self) {
        let mut tabs = self
            .open
            .iter_mut()
            .filter(|b| b.untitled_number.is_some())
            .peekable();
        let Some(first) = tabs.next() else {
            return;
        };
        if tabs.peek().is_none() {
            first.set_untitled_number(0);
            return;
        }
        for (ordinal, buffer) in std::iter::once(first).chain(tabs).enumerate() {
            buffer.set_untitled_number(ordinal + 1);
        }
    }
    pub fn view(&self) -> SessionView {
        SessionView {
            open: self.open.iter().map(|b| b.view.clone()).collect(),
            closed: self.closed.clone(),
            active: self.open.get(self.active).map(|b| b.page.info.id),
            secondary: None,
            folder: self.folders.first().cloned(),
            folders: self.folders.clone(),
        }
    }
    pub fn remote_view(&self) -> Result<serde_json::Value, String> {
        let remote = self.remote.as_ref().ok_or("Not an SSH workspace.")?;
        let mut view = self.view();
        let mut ids = std::collections::HashMap::new();
        for tab in view.open.iter_mut().chain(&mut view.closed) {
            let shared = self.editor.get(tab.id)?;
            let doc = shared.lock().map_err(|e| e.to_string())?;
            let path = doc
                .remote_location()
                .ok_or("A local buffer cannot enter a remote workspace.")?;
            let host = remote.identity(path)?;
            ids.insert(tab.id, host);
            tab.id = host;
        }
        view.active = view.active.and_then(|id| ids.get(&id).copied());
        serde_json::to_value(view).map_err(|e| e.to_string())
    }
    pub fn checkpoint(&self) -> Result<(), String> {
        if let Some(remote) = &self.remote {
            return remote.metadata(Some(self.remote_view()?)).map(|_| ());
        }
        self.store.save(&self.editor, self.view())
    }
    /// Keep the last empty, unnamed scratchpad as the editor's blank state.
    pub fn can_close(&self, index: usize) -> bool {
        self.open.get(index).is_some_and(|buffer| {
            self.open.len() > 1
                || buffer.page.info.path.is_some()
                || buffer.view.custom_name.is_some()
                || buffer.page.info.paged
                || !buffer.page.text.is_empty()
        })
    }
    /// Close keeps the document, including untitled text, on a recoverable shelf.
    pub fn close(&mut self, index: usize) -> Result<(), String> {
        self.editor.check_membership_writable()?;
        if !self.can_close(index) {
            return Ok(());
        }
        if self.remote.is_some() && self.open.len() == 1 {
            return Err(
                "Keep one remote tab open. Create another remote file before closing this tab."
                    .into(),
            );
        }
        let buffer = &self.open[index];
        if buffer.page.info.path.is_none()
            && !buffer.page.info.paged
            && buffer.page.info.size == 0
            && buffer.page.text.is_empty()
        {
            return self.forget(index);
        }
        // Fifty entries is a soft history limit: evict only saved, unchanged
        // files. Unsaved notes must neither be lost nor prevent another tab closing.
        if self.closed.len() >= 50 {
            let clean = self.closed.iter().position(|view| {
                self.editor
                    .get(view.id)
                    .ok()
                    .and_then(|doc| {
                        doc.lock()
                            .ok()
                            .map(|d| !d.info().dirty && d.info().path.is_some())
                    })
                    .unwrap_or(false)
            });
            if let Some(clean) = clean {
                self.editor.remove(self.closed[clean].id)?;
                self.closed.remove(clean);
            } else {
                let buffer = &self.open[index];
                if !buffer.page.info.dirty && buffer.page.info.path.is_some() {
                    return self.forget(index);
                }
            }
        }
        self.editor
            .close_document_lifetime(self.open[index].view.id)?;
        let mut buffer = self.open.remove(index);
        if buffer.untitled_number.is_some() {
            // Whitespace-only drafts can be recovered from the shelf. Give
            // them stable labels even as the set of open tabs changes.
            buffer.view.name = format!("Untitled {}", buffer.view.id);
        }
        self.closed.push(buffer.view);
        self.after_close(index)
    }
    /// Explicitly discard a tab without adding it to recovery or the shelf.
    pub fn forget(&mut self, index: usize) -> Result<(), String> {
        let Some(buffer) = self.open.get(index) else {
            return Ok(());
        };
        if self.remote.is_some() && self.open.len() == 1 {
            return Err(
                "Keep one remote tab open. Create another remote file before discarding this tab."
                    .into(),
            );
        }
        self.editor.remove(buffer.view.id)?;
        self.open.remove(index);
        self.after_close(index)
    }
    fn after_close(&mut self, index: usize) -> Result<(), String> {
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.open.len().saturating_sub(1));
        if self.open.is_empty() {
            self.new_buffer()?;
        } else {
            self.refresh_untitled_names();
        }
        Ok(())
    }
    pub fn reopen(&mut self) -> Result<(), String> {
        let Some(index) = self.closed.len().checked_sub(1) else {
            return Ok(());
        };
        self.reopen_at(index)
    }
    pub fn reopen_at(&mut self, index: usize) -> Result<(), String> {
        self.editor.check_membership_writable()?;
        let view = self
            .closed
            .get(index)
            .cloned()
            .ok_or("That file is no longer on the shelf.")?;
        let page = self
            .editor
            .get(view.id)?
            .lock()
            .map_err(|e| e.to_string())?
            .page(view.page_start)?;
        self.editor.open_document_lifetime(view.id)?;
        self.closed.remove(index);
        self.open.push(Buffer::new(page, Some(view)));
        self.active = self.open.len() - 1;
        if self.open[self.active].untitled_number.is_some() {
            self.refresh_untitled_names();
        }
        Ok(())
    }
    pub fn name_scratchpad(&mut self, index: usize, name: &str) -> Result<(), String> {
        let buffer = self
            .open
            .get_mut(index)
            .ok_or("That file is no longer open.")?;
        if buffer.page.info.path.is_some() {
            return Err(
                "Only unsaved files can be named. Use Save As to save a copy of a saved file."
                    .into(),
            );
        }
        let name = name.trim();
        if name.chars().count() > 80 || name.chars().any(char::is_control) {
            return Err("Use a name of at most 80 characters on one line.".into());
        }
        buffer.view.custom_name = if name.is_empty() {
            None
        } else {
            Some(name.into())
        };
        buffer.refresh_name();
        self.refresh_untitled_names();
        Ok(())
    }
    pub fn toggle_bookmark(&mut self, index: usize) -> Result<bool, String> {
        let buffer = self
            .open
            .get_mut(index)
            .ok_or("That file is no longer open.")?;
        let offset = buffer.cursor_offset();
        if let Some(index) = buffer
            .view
            .bookmarks
            .iter()
            .position(|b| b.offset == offset)
        {
            buffer.view.bookmarks.remove(index);
            return Ok(false);
        }
        if buffer.view.bookmarks.len() >= 1000 {
            return Err("This file already has 1,000 bookmarks.".into());
        }
        let local = (offset - buffer.page.start) as usize;
        let before = &buffer.page.text[..local];
        let start = before.rfind('\n').map_or(0, |n| n + 1);
        let text = buffer.page.text[start..]
            .lines()
            .next()
            .unwrap_or("")
            .trim();
        let label: String = text.chars().take(60).collect();
        buffer.view.bookmarks.push(Bookmark {
            offset,
            label: if label.is_empty() {
                format!("Byte {offset}")
            } else {
                label
            },
        });
        buffer.view.bookmarks.sort_by_key(|b| b.offset);
        Ok(true)
    }
    pub fn adjacent_bookmark(&self, index: usize, forward: bool) -> Option<u64> {
        let buffer = self.open.get(index)?;
        let offset = buffer.cursor_offset();
        let bookmarks = &buffer.view.bookmarks;
        if forward {
            bookmarks
                .iter()
                .find(|b| b.offset > offset)
                .or_else(|| bookmarks.first())
                .map(|b| b.offset)
        } else {
            bookmarks
                .iter()
                .rev()
                .find(|b| b.offset < offset)
                .or_else(|| bookmarks.last())
                .map(|b| b.offset)
        }
    }
    /// Commit a UTF-8-safe minimal splice so history shares unchanged bytes.
    pub fn edit(&mut self, index: usize, old: &str) -> Result<(), String> {
        self.edit_with_line_ending(index, old, None)
    }
    pub fn edit_with_line_ending(
        &mut self,
        index: usize,
        old: &str,
        crlf: Option<bool>,
    ) -> Result<(), String> {
        self.edit_with_splice(index, old, crlf, None)
    }

    /// Apply a widget edit whose changed span is expressed in old and new bytes.
    pub fn edit_with_splice(
        &mut self,
        index: usize,
        old: &str,
        crlf: Option<bool>,
        changed: Option<(usize, usize, usize)>,
    ) -> Result<(), String> {
        let buffer = &mut self.open[index];
        let changed = changed.filter(|&(start, old_end, new_end)| {
            old.get(start..old_end).is_some()
                && buffer.page.text.get(start..new_end).is_some()
                && old.len() - old_end == buffer.page.text.len() - new_end
        });
        let (start, end, replacement) = match changed {
            Some((start, old_end, new_end)) => {
                let (local_start, local_end, replacement) =
                    splice(&old[start..old_end], &buffer.page.text[start..new_end]);
                (start + local_start, start + local_end, replacement)
            }
            None => splice(old, &buffer.page.text),
        };
        let editor = self.editor.get(buffer.page.info.id)?;
        let mut document = editor.lock().map_err(|e| e.to_string())?;
        if document.workspace_frozen() {
            buffer.page.text = old.to_owned();
            return Err("This buffer is awaiting a workspace commit.".into());
        }
        let info = if let Some(crlf) = crlf {
            document.replace_with_line_ending(
                buffer.page.start + start as u64,
                buffer.page.start + end as u64,
                replacement.to_owned(),
                buffer.page.info.revision,
                crlf,
            )?
        } else {
            document.replace(
                buffer.page.start + start as u64,
                buffer.page.start + end as u64,
                replacement.to_owned(),
                buffer.page.info.revision,
            )?
        };
        let selection = buffer.pending_selection.take().unwrap_or_else(|| {
            let mut ranges = buffer.view.selections.clone();
            ranges.push(crate::advanced::Selection {
                anchor: buffer.view.anchor,
                head: buffer.view.head,
            });
            ranges
        });
        buffer.cursor_undo.push(selection);
        buffer.cursor_redo.clear();
        if buffer.cursor_undo.len() > 256 {
            buffer.cursor_undo.remove(0);
        }
        buffer.changes.push_back(Change {
            revision: buffer.page.info.revision,
            start: old[..start].chars().count(),
            end: old[..end].chars().count(),
            added: replacement.chars().count(),
        });
        if buffer.changes.len() > 256 {
            buffer.changes.pop_front();
        }
        buffer.page.end = buffer.page.start + buffer.page.text.len() as u64;
        buffer.page.info = info;
        let absolute_start = buffer.page.start + start as u64;
        let absolute_end = buffer.page.start + end as u64;
        let added = replacement.len() as u64;
        for bookmark in &mut buffer.view.bookmarks {
            if bookmark.offset >= absolute_end {
                bookmark.offset = bookmark
                    .offset
                    .saturating_sub(absolute_end - absolute_start)
                    .saturating_add(added);
            } else if bookmark.offset > absolute_start {
                bookmark.offset = absolute_start;
            }
        }
        buffer.view.bookmarks.sort_by_key(|b| b.offset);
        buffer.view.bookmarks.dedup_by_key(|b| b.offset);
        let was_untitled = buffer.untitled_number.is_some();
        buffer.refresh_name();
        if buffer.untitled_number.is_some() != was_untitled {
            self.refresh_untitled_names();
        }
        Ok(())
    }
}

fn restore_remote_folders(
    remote: &crate::remote::Connection,
    legacy: Option<FolderView>,
    mut folders: Vec<FolderView>,
) -> Option<Vec<FolderView>> {
    if folders.is_empty() {
        folders.extend(legacy);
    }
    if folders.is_empty() {
        return Some(folders);
    }

    // Match the helper's path-byte limit, including up to three URI bytes per
    // encoded byte. The host separately bounds the whole view to 256 KiB.
    const MAX_PATH_BYTES: usize = 4096;
    let max_uri_len = remote.root().as_os_str().len() + 3 * MAX_PATH_BYTES;
    let normalize = |path: &str| -> Option<String> {
        if path.len() > max_uri_len {
            return None;
        }
        let relative = remote.relative(Path::new(path)).ok()?;
        if relative.len() > MAX_PATH_BYTES
            || relative.contains(&0)
            || std::str::from_utf8(&relative).is_err()
            || (!relative.is_empty() && relative.split(|&byte| byte == b'/').any(<[u8]>::is_empty))
        {
            return None;
        }
        Some(remote.uri(&relative).to_string_lossy().into_owned())
    };
    let mut roots = HashSet::new();
    folders.retain_mut(|folder| {
        let Some(path) = normalize(&folder.path) else {
            return false;
        };
        if !roots.insert(path.clone()) {
            return false;
        }
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let mut expanded_paths = HashSet::new();
        folder.expanded.retain_mut(|entry| {
            let Some(expanded) = normalize(entry) else {
                return false;
            };
            if (expanded != path && !expanded.starts_with(&prefix))
                || !expanded_paths.insert(expanded.clone())
            {
                return false;
            }
            *entry = expanded;
            true
        });
        folder.path = path;
        true
    });
    (!folders.is_empty()).then_some(folders)
}

pub fn splice<'a>(old: &str, new: &'a str) -> (usize, usize, &'a str) {
    // Comparing blocks lets the compiler use vectorized equality for the
    // unchanged region; scalar comparison only locates the differing byte.
    let matched = old
        .as_bytes()
        .as_chunks::<64>()
        .0
        .iter()
        .zip(new.as_bytes().as_chunks::<64>().0.iter())
        .take_while(|(a, b)| a == b)
        .count()
        * 64;
    let mut prefix = matched
        + old.as_bytes()[matched..]
            .iter()
            .zip(new.as_bytes()[matched..].iter())
            .take_while(|(a, b)| a == b)
            .count();
    while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let matched = old.as_bytes()[prefix..]
        .rchunks_exact(64)
        .zip(new.as_bytes()[prefix..].rchunks_exact(64))
        .take_while(|(a, b)| a == b)
        .count()
        * 64;
    let mut suffix = matched
        + old.as_bytes()[prefix..old.len() - matched]
            .iter()
            .rev()
            .zip(new.as_bytes()[prefix..new.len() - matched].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    (prefix, old.len() - suffix, &new[prefix..new.len() - suffix])
}

fn draft_name(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let mut chars = line.chars();
    let mut label: String = chars.by_ref().take(40).collect();
    if chars.next().is_some() {
        label.push('…');
    }
    Some(label)
}

#[cfg(test)]
mod remote_folder_restore_tests {
    use super::*;

    fn folder(path: impl Into<String>) -> FolderView {
        FolderView {
            path: path.into(),
            visible: false,
            expanded: vec![],
            collapsed: true,
        }
    }

    #[test]
    fn folder_restore_preserves_remote_views_and_drops_unscoped_paths() {
        let remote = crate::remote::tests::disconnected();
        let mut nested = folder("hedit-ssh://test/src");
        nested.expanded = vec![
            "hedit-ssh://test/src/subfolder".into(),
            "hedit-ssh://test/src/%73ubfolder".into(),
            "hedit-ssh://test/src-other/sibling".into(),
            "hedit-ssh://test/src/%2e%2e".into(),
            "hedit-ssh://test/src/%FF".into(),
            "hedit-ssh://other/src/subfolder".into(),
            "/local/src/subfolder".into(),
        ];
        let mut root = folder("hedit-ssh://test");
        root.visible = true;
        root.expanded = vec!["hedit-ssh://test/src".into()];
        let folders = restore_remote_folders(
            &remote,
            None,
            vec![
                nested,
                root,
                folder("hedit-ssh://test/%73rc"),
                folder("hedit-ssh://test/~scratch/test"),
                folder("hedit-ssh://other/foreign"),
                folder("/local"),
                folder(format!("hedit-ssh://test/{}", "a".repeat(4097))),
            ],
        )
        .unwrap();
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0].path, "hedit-ssh://test/src");
        assert_eq!(folders[0].expanded, ["hedit-ssh://test/src/subfolder"]);
        assert!(!folders[0].visible && folders[0].collapsed);
        assert_eq!(folders[1].path, "hedit-ssh://test/");
        assert_eq!(folders[1].expanded, ["hedit-ssh://test/src"]);
        assert!(folders[1].visible);
    }

    #[test]
    fn folder_restore_accepts_legacy_and_intentionally_empty_views() {
        let remote = crate::remote::tests::disconnected();
        let legacy = folder("hedit-ssh://test/legacy");
        let folders = restore_remote_folders(&remote, Some(legacy), vec![]).unwrap();
        assert_eq!(folders[0].path, "hedit-ssh://test/legacy");
        assert!(restore_remote_folders(&remote, None, vec![])
            .unwrap()
            .is_empty());
        assert!(restore_remote_folders(&remote, Some(folder("/local")), vec![]).is_none());
    }
}
