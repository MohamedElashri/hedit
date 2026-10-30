//! Shared behavior for transient windows: one at a time, Escape closes the
//! topmost, and quick pickers close when the user clicks elsewhere.
use super::*;

/// Overlays from topmost to bottom, the order Escape closes them in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Overlay {
    GitPrompt,
    UserGuide,
    Palette,
    Goto,
    Rename,
    Create,
    FileAction,
    Completion,
    MacroName,
    Snippets,
    Symbols,
    Encoding,
    Recent,
    Shelf,
    Bookmarks,
    Diff,
    Folders,
    Settings,
    About,
    Updates,
    Preferences,
    Find,
    Build,
}
impl Overlay {
    const ALL: [Self; 23] = [
        Self::GitPrompt,
        Self::UserGuide,
        Self::Palette,
        Self::Goto,
        Self::Rename,
        Self::Create,
        Self::FileAction,
        Self::Completion,
        Self::MacroName,
        Self::Snippets,
        Self::Symbols,
        Self::Encoding,
        Self::Recent,
        Self::Shelf,
        Self::Bookmarks,
        Self::Diff,
        Self::Folders,
        Self::Settings,
        Self::About,
        Self::Updates,
        Self::Preferences,
        Self::Find,
        Self::Build,
    ];
}

/// True when a click landed outside an overlay window that was already on
/// screen last frame, so the click that opened it does not close it again.
pub(super) fn clicked_away<R>(ctx: &egui::Context, shown: &Option<egui::InnerResponse<R>>) -> bool {
    shown.as_ref().is_some_and(|shown| {
        shown.response.clicked_elsewhere()
            && ctx.memory(|m| m.areas().visible_last_frame(&shown.response.layer_id))
    })
}

impl HeditApp {
    pub(super) fn find_input_focused(&self, ctx: &egui::Context) -> bool {
        self.find_open && ctx.memory(|m| m.has_focus(egui::Id::new(chrome::FIND_INPUT)))
    }
    pub(super) fn overlay_open(&self, overlay: Overlay) -> bool {
        match overlay {
            Overlay::GitPrompt => self.source_control.prompt.is_some(),
            Overlay::UserGuide => self.guide.open,
            Overlay::Palette => self.palette,
            Overlay::Goto => self.goto_open,
            Overlay::Rename => self.rename_target.is_some(),
            Overlay::Create => self.project.create.is_some(),
            Overlay::FileAction => self.quality.file_action.is_some(),
            Overlay::Completion => self.auto.popup.is_some(),
            Overlay::MacroName => self.macros.naming.is_some(),
            Overlay::Snippets => self.quality.snippets_open,
            Overlay::Symbols => self.quality.symbols_open,
            Overlay::Encoding => self.quality.encoding_open,
            Overlay::Recent => self.quality.recent_open,
            Overlay::Shelf => self.shelf_open,
            Overlay::Bookmarks => self.bookmarks_open,
            Overlay::Diff => self.quality.diff_open,
            Overlay::Folders => self.quality.folders_open,
            Overlay::Settings => self.quality.settings_open,
            Overlay::About => self.about_open,
            Overlay::Updates => self.updates.open,
            Overlay::Preferences => self.preferences_open,
            Overlay::Find => self.find_open,
            Overlay::Build => self.build.open,
        }
    }
    pub(super) fn close_overlay(&mut self, ctx: &egui::Context, overlay: Overlay) {
        match overlay {
            Overlay::GitPrompt => self.source_control.prompt = None,
            Overlay::UserGuide => self.guide.open = false,
            Overlay::Palette => {
                self.cancel_theme_palette(ctx);
                self.cancel_goto_preview();
                self.palette = false;
            }
            Overlay::Goto => self.goto_open = false,
            Overlay::Rename => self.rename_target = None,
            Overlay::Create => self.project.create = None,
            Overlay::FileAction => self.quality.file_action = None,
            Overlay::Completion => self.auto.popup = None,
            Overlay::MacroName => self.macros.naming = None,
            Overlay::Snippets => self.quality.snippets_open = false,
            Overlay::Symbols => self.quality.symbols_open = false,
            Overlay::Encoding => self.quality.encoding_open = false,
            Overlay::Recent => self.quality.recent_open = false,
            Overlay::Shelf => self.shelf_open = false,
            Overlay::Bookmarks => self.bookmarks_open = false,
            Overlay::Diff => self.quality.diff_open = false,
            Overlay::Folders => self.quality.folders_open = false,
            Overlay::Settings => self.quality.settings_open = false,
            Overlay::About => self.about_open = false,
            Overlay::Updates => self.updates.open = false,
            Overlay::Preferences => {
                self.preferences_open = false;
                self.appearance.save_name = None;
            }
            Overlay::Find => {
                self.find_open = false;
                self.replace_open = false;
            }
            Overlay::Build => self.build.open = false,
        }
    }
    /// Close every transient window before opening another. The Find bar is a
    /// panel, not a window, so it stays.
    pub(super) fn close_transient(&mut self, ctx: &egui::Context) {
        for overlay in Overlay::ALL {
            if !matches!(overlay, Overlay::Find | Overlay::Build) {
                self.close_overlay(ctx, overlay);
            }
        }
    }
    /// Escape closes egui's own popups first, then the topmost overlay, then
    /// the error banner, and finally collapses extra cursors in the editor.
    pub(super) fn escape(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.key_pressed(Key::Escape)) {
            return;
        }
        // Menus, combo boxes, and context menus dismiss themselves.
        if self.open_menu.is_some() || ctx.memory(|m| m.any_popup_open()) {
            return;
        }
        if let Some(overlay) = Overlay::ALL.into_iter().find(|&o| self.overlay_open(o)) {
            self.close_overlay(ctx, overlay);
            ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
            self.focus_editor = true;
            return;
        }
        if self.error.take().is_some() {
            self.transient_error = None;
            ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
            return;
        }
        if self.snippet.take().is_some() {
            ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
            self.focus_editor = true;
            return;
        }
        // egui drops focus on Escape before this runs; give it back to the
        // editor so typing continues.
        ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        if ctx.memory(|m| m.focused().is_none()) {
            self.focus_editor = true;
        }
        let b = &mut self.workspace.open[self.workspace.active];
        if !b.view.selections.is_empty() {
            b.view.selections.clear();
            b.restore_cursor = true;
            self.focus_editor = true;
        }
    }
}
