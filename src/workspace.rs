//! UI-independent buffer ownership and session views.
use crate::{
    document::{Document, Page},
    session::{Bookmark, FolderView, SessionStore, SessionView, TabView},
    syntax::Language,
    Editor,
};
use std::{path::PathBuf, sync::Arc};

pub struct Change {
    pub revision: u64,
    pub start: usize,
    pub end: usize,
    pub added: usize,
}
pub struct Buffer {
    pub page: Page,
    pub view: TabView,
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
        let mut view = view.unwrap_or(TabView {
            id: page.info.id,
            name: label(&page),
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
        Self {
            changes: std::collections::VecDeque::new(),
            cursor_undo: vec![],
            cursor_redo: vec![],
            pending_selection: None,
            page,
            view,
            external: false,
            follow: false,
            restore_cursor: true,
            reveal_cursor: false,
        }
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
        self.view.name = if self.page.info.path.is_none() {
            self.view
                .custom_name
                .clone()
                .unwrap_or_else(|| label(&self.page))
        } else {
            self.page.info.name.clone()
        };
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
    pub editor: Arc<Editor>,
    pub store: Arc<SessionStore>,
    pub open: Vec<Buffer>,
    pub closed: Vec<TabView>,
    pub active: usize,
    pub folders: Vec<FolderView>,
}
impl Workspace {
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
        let mut workspace = Self {
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
        }
        Ok((workspace, errors))
    }
    pub fn add(&mut self, document: Document, page: Page) -> Result<(), String> {
        self.editor.insert(document)?;
        self.open.push(Buffer::new(page, None));
        self.active = self.open.len() - 1;
        Ok(())
    }
    pub fn new_buffer(&mut self) -> Result<(), String> {
        let mut document = Document::empty(self.editor.id());
        let page = document.page(0)?;
        self.add(document, page)
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
    pub fn checkpoint(&self) -> Result<(), String> {
        self.store.save(&self.editor, self.view())
    }
    /// Close keeps the document, including untitled text, on a recoverable shelf.
    pub fn close(&mut self, index: usize) -> Result<(), String> {
        if index >= self.open.len() {
            return Ok(());
        }
        // Never silently evict an unsaved buffer from the 50-item history.
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
            let Some(clean) = clean else {
                return Err("The closed-files shelf is full of unsaved text. Reopen and save a file before closing more.".into());
            };
            let removed = self.closed.remove(clean);
            self.editor.remove(removed.id)?;
        }
        let buffer = self.open.remove(index);
        self.closed.push(buffer.view);
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.open.len().saturating_sub(1));
        if self.open.is_empty() {
            self.new_buffer()?;
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
        self.closed.remove(index);
        self.open.push(Buffer::new(page, Some(view)));
        self.active = self.open.len() - 1;
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
        let buffer = &mut self.open[index];
        let (start, end, replacement) = splice(old, &buffer.page.text);
        let editor = self.editor.get(buffer.page.info.id)?;
        let mut document = editor.lock().map_err(|e| e.to_string())?;
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
        buffer.refresh_name();
        Ok(())
    }
}

pub fn splice<'a>(old: &str, new: &'a str) -> (usize, usize, &'a str) {
    let mut prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let mut suffix = old[prefix..]
        .bytes()
        .rev()
        .zip(new[prefix..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    (prefix, old.len() - suffix, &new[prefix..new.len() - suffix])
}

fn label(page: &Page) -> String {
    if page.info.path.is_some() {
        return page.info.name.clone();
    }
    let line = page
        .text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    if line.is_empty() {
        format!("Untitled {}", page.info.id)
    } else {
        let mut label: String = line.chars().take(40).collect();
        if line.chars().count() > 40 {
            label.push('…');
        }
        label
    }
}
