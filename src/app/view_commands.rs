//! Small file and view commands: recent files, closing many tabs, selecting or
//! clearing bookmarks, and scrolling without moving the cursor.
use super::*;

/// How many recent files are kept.
const RECENT: usize = 20;

impl HeditApp {
    /// Note the active file as the most recently used.
    pub(super) fn track_recent_file(&mut self) {
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
        let tabs = self
            .workspace
            .open
            .iter()
            .rev()
            .filter(|b| !b.view.pinned && (!unmodified || !b.page.info.dirty))
            .map(|b| b.view.id)
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
                let mut at = (mark.offset.checked_sub(b.page.start)? as usize).min(text.len());
                while !text.is_char_boundary(at) {
                    at -= 1;
                }
                Some(hedit::advanced::Selection::caret(
                    text[..at].chars().count(),
                ))
            })
            .collect();
        if ranges.is_empty() {
            self.notice = "No bookmarks in this file.".into();
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
