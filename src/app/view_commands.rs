//! Small file and view commands: recent files, closing many tabs, selecting or
//! clearing bookmarks, and scrolling without moving the cursor.
use super::*;

/// How many recent files are kept.
const RECENT: usize = 20;

fn location_text(path: &str, line: u64, column: u64) -> String {
    if path.contains(['@', ':', '#', '"']) || path.trim() != path {
        format!("\"{path}\":{line}:{column}")
    } else {
        format!("{path}:{line}:{column}")
    }
}

impl HeditApp {
    pub(super) fn copy_file_location(&mut self, ctx: &egui::Context) {
        let buffer = &self.workspace.open[self.workspace.active];
        let Some(path) = buffer.page.info.path.as_deref() else {
            return;
        };
        let roots: Vec<_> = self
            .workspace
            .folders
            .iter()
            .map(|f| PathBuf::from(&f.path))
            .collect();
        let path = hedit::folder::display_path(&roots, std::path::Path::new(path));
        if buffer.page.start != 0 {
            let id = buffer.view.id;
            let offset = buffer.cursor_offset();
            let revision = buffer.page.info.revision;
            let editor = self.workspace.editor.clone();
            self.job(ctx, move || {
                let shared = editor.get(id)?;
                let doc = shared.lock().map_err(|error| error.to_string())?;
                if doc.revision() != revision {
                    return Err("The file changed before copying its location.".into());
                }
                let (line, column) = doc.navigation_position(offset)?;
                Ok(Action::CopyFileLocation(location_text(&path, line, column)))
            });
            return;
        }
        let (line, column) = self.projections[self.panes.focused]
            .source_index(&buffer.page.text)
            .navigation_position(&buffer.page.text, buffer.view.head);
        ctx.copy_text(location_text(&path, line as u64, column as u64));
        self.notice = "File location copied.".into();
    }
    /// Note the active file as the most recently used.
    pub(super) fn track_recent_file(&mut self) {
        if self.workspace.remote.is_some() {
            return;
        }
        let Some(path) = self.active().page.info.path.as_ref() else {
            return;
        };
        if self.recent_files.first() != Some(path) {
            let path = path.clone();
            self.recent_files.retain(|p| *p != path);
            self.recent_files.insert(0, path);
            self.recent_files.truncate(RECENT);
        }
    }
    /// File → Open recent…: pick from recently used files.
    pub(super) fn open_recent(&mut self, ctx: &egui::Context) {
        let roots: Vec<PathBuf> = self
            .workspace
            .folders
            .iter()
            .map(|f| PathBuf::from(&f.path))
            .collect();
        let items = self
            .recent_files
            .iter()
            .filter(|p| std::path::Path::new(p).is_file())
            .map(|p| {
                let shown = hedit::folder::display_path(&roots, std::path::Path::new(p));
                let name = std::path::Path::new(p)
                    .file_name()
                    .map_or(p.clone(), |n| n.to_string_lossy().into_owned());
                (name, shown, commands::Pick::File(PathBuf::from(p)))
            })
            .collect();
        self.open_pick_list(ctx, "Open recent file", "Filter recent files…", items);
    }
    /// Close every tab, or only those without unsaved changes. Pinned tabs
    /// stay; unsaved files and non-empty untitled tabs prompt before closing.
    pub(super) fn close_tabs(&mut self, unmodified: bool) {
        self.sync_groups();
        let tabs = self
            .group_tab_ids(self.panes.focused)
            .into_iter()
            .rev()
            .filter(|id| {
                self.workspace.open.iter().any(|b| {
                    b.view.id == *id && !b.view.pinned && (!unmodified || !b.page.info.dirty)
                })
            })
            .collect();
        self.request_close(tabs);
    }
    /// Alt+F2: a cursor on every bookmarked line.
    pub(super) fn select_bookmarks(&mut self) {
        let b = self.active();
        let text = &b.page.text;
        let ranges: Vec<hedit::advanced::Selection> = b
            .view
            .bookmarks
            .iter()
            .filter_map(|mark| {
                let mut at = b.bookmark_on_page(mark.offset)?;
                while !text.is_char_boundary(at) {
                    at -= 1;
                }
                Some(hedit::advanced::Selection::caret(
                    text[..at].chars().count(),
                ))
            })
            .collect();
        if ranges.is_empty() {
            self.notice = if b.page.info.paged {
                "No bookmarks on this page."
            } else {
                "No bookmarks in this file."
            }
            .into();
        } else {
            self.set_selections(ranges);
        }
    }
    /// Ctrl+Shift+F2: remove every bookmark in the file.
    pub(super) fn clear_bookmarks(&mut self) {
        let b = &mut self.workspace.open[self.workspace.active];
        let count = b.view.bookmarks.len();
        b.view.bookmarks.clear();
        self.notice = format!("Removed {count} bookmarks.");
        self.changed();
    }
    /// Ctrl+Up / Ctrl+Down: scroll one line, leaving the cursor where it is.
    pub(super) fn scroll_lines(&mut self, ctx: &egui::Context, lines: f64) {
        let row = ctx.fonts(|f| f.row_height(&FontId::monospace(self.settings.font_size)))
            + self.settings.line_padding;
        let b = &mut self.workspace.open[self.workspace.active];
        let top = match self.scroll_request {
            // Presses before the last one was drawn add up.
            Some((view, top)) if view == b.view.id => top,
            _ => b.view.scroll_top,
        };
        let top = (top + lines * row as f64).max(0.0);
        self.scroll_request = Some((b.view.id, top));
        self.focus_editor = true;
    }
}
