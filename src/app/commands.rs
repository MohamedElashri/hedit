use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Command {
    NextOccurrence,
    AllOccurrences,
    SingleSelection,
    Rectangle,
    SplitColumns,
    SplitRows,
    SinglePane,
    FocusPane,
    MovePane,
    AutoIndent,
    SmartIndent,
    SoftTabs,
    DetectIndent,
    Fold,
    FoldAll,
    Unfold,
    UnfoldAll,
    IndentGuides,
    Minimap,
    New,
    Open,
    OpenFolder,
    CloseFolder,
    RefreshFolder,
    Save,
    SaveAs,
    Close,
    Reopen,
    Shelf,
    Rename,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    SelectLine,
    Duplicate,
    DeleteLine,
    InsertBefore,
    InsertAfter,
    MoveUp,
    MoveDown,
    JoinLines,
    Indent,
    Unindent,
    Uppercase,
    Lowercase,
    SortLines,
    ReverseLines,
    UniqueLines,
    TrimWhitespace,
    ToSpaces,
    ToTabs,
    ToggleComment,
    ToLf,
    ToCrlf,
    IndentSpaces,
    IndentTabs,
    Indent2,
    Indent4,
    Indent8,
    Tabs,
    StatusBar,
    CurrentLine,
    Whitespace,
    Fullscreen,
    DistractionFree,
    RulerNone,
    Ruler80,
    Ruler100,
    Ruler120,
    WrapWindow,
    Wrap80,
    Wrap100,
    Wrap120,

    Find,
    Replace,
    Goto,
    Switcher,
    Palette,
    Sidebar,
    FolderSidebar,
    Wrap,
    LineNumbers,
    ZoomIn,
    ZoomOut,
    ResetZoom,
    Bookmark,
    NextBookmark,
    PreviousBookmark,
    Bookmarks,
    Follow,
    Gpu,
    Settings,
    Appearance,
    ToolSettings,
    Checkpoint,
    About,
    Updates,
}
impl Command {
    pub(super) const ALL: &'static [Self] = &[
        Self::NextOccurrence,
        Self::AllOccurrences,
        Self::SingleSelection,
        Self::Rectangle,
        Self::SplitColumns,
        Self::SplitRows,
        Self::SinglePane,
        Self::FocusPane,
        Self::MovePane,
        Self::AutoIndent,
        Self::SmartIndent,
        Self::SoftTabs,
        Self::DetectIndent,
        Self::Fold,
        Self::FoldAll,
        Self::Unfold,
        Self::UnfoldAll,
        Self::IndentGuides,
        Self::Minimap,
        Self::New,
        Self::Open,
        Self::OpenFolder,
        Self::CloseFolder,
        Self::RefreshFolder,
        Self::Save,
        Self::SaveAs,
        Self::Close,
        Self::Reopen,
        Self::Shelf,
        Self::Rename,
        Self::Undo,
        Self::Redo,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::SelectAll,
        Self::SelectLine,
        Self::Duplicate,
        Self::DeleteLine,
        Self::InsertBefore,
        Self::InsertAfter,
        Self::MoveUp,
        Self::MoveDown,
        Self::JoinLines,
        Self::Indent,
        Self::Unindent,
        Self::Uppercase,
        Self::Lowercase,
        Self::SortLines,
        Self::ReverseLines,
        Self::UniqueLines,
        Self::TrimWhitespace,
        Self::ToSpaces,
        Self::ToTabs,
        Self::ToggleComment,
        Self::ToLf,
        Self::ToCrlf,
        Self::IndentSpaces,
        Self::IndentTabs,
        Self::Indent2,
        Self::Indent4,
        Self::Indent8,
        Self::Tabs,
        Self::StatusBar,
        Self::CurrentLine,
        Self::Whitespace,
        Self::Fullscreen,
        Self::DistractionFree,
        Self::RulerNone,
        Self::Ruler80,
        Self::Ruler100,
        Self::Ruler120,
        Self::WrapWindow,
        Self::Wrap80,
        Self::Wrap100,
        Self::Wrap120,
        Self::Find,
        Self::Replace,
        Self::Goto,
        Self::Switcher,
        Self::Sidebar,
        Self::FolderSidebar,
        Self::Wrap,
        Self::LineNumbers,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::ResetZoom,
        Self::Bookmark,
        Self::NextBookmark,
        Self::PreviousBookmark,
        Self::Bookmarks,
        Self::Follow,
        Self::Gpu,
        Self::Settings,
        Self::Appearance,
        Self::ToolSettings,
        Self::Checkpoint,
        Self::About,
        Self::Updates,
        Self::Quit,
    ];
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::NextOccurrence => "Select next occurrence",
            Self::AllOccurrences => "Select all occurrences",
            Self::SingleSelection => "Single selection",
            Self::Rectangle => "Rectangular selection",
            Self::SplitColumns => "Layout: two columns",
            Self::SplitRows => "Layout: two rows",
            Self::SinglePane => "Layout: single pane",
            Self::FocusPane => "Focus other group",
            Self::MovePane => "Move file to other group",
            Self::AutoIndent => "Automatic indentation",
            Self::SmartIndent => "Smart indentation",
            Self::SoftTabs => "Backspace to indentation stops",
            Self::DetectIndent => "Detect indentation from file",
            Self::Fold => "Fold block or selection",
            Self::FoldAll => "Fold all blocks",
            Self::Unfold => "Unfold block",
            Self::UnfoldAll => "Unfold all",
            Self::IndentGuides => "Show indentation guides",
            Self::Minimap => "Show minimap",
            Self::New => "New file",
            Self::Open => "Open file…",
            Self::OpenFolder => "Open folder…",
            Self::CloseFolder => "Close all folders",
            Self::RefreshFolder => "Refresh folders",
            Self::Save => "Save",
            Self::SaveAs => "Save as…",
            Self::Close => "Close tab",
            Self::Reopen => "Reopen closed tab",
            Self::Shelf => "Browse closed files…",
            Self::Rename => "Name unsaved file…",
            Self::Quit => "Quit hedit",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::SelectAll => "Select all",
            Self::SelectLine => "Select line",
            Self::Duplicate => "Duplicate selection or line",
            Self::DeleteLine => "Delete line",
            Self::InsertBefore => "Insert line before",
            Self::InsertAfter => "Insert line after",
            Self::MoveUp => "Move lines up",
            Self::MoveDown => "Move lines down",
            Self::JoinLines => "Join lines",
            Self::Indent => "Indent",
            Self::Unindent => "Unindent",
            Self::Uppercase => "Uppercase",
            Self::Lowercase => "Lowercase",
            Self::SortLines => "Sort lines",
            Self::ReverseLines => "Reverse lines",
            Self::UniqueLines => "Remove duplicate lines",
            Self::TrimWhitespace => "Trim trailing whitespace",
            Self::ToSpaces => "Convert indentation to spaces",
            Self::ToTabs => "Convert indentation to tabs",
            Self::ToggleComment => "Toggle line comment",
            Self::ToLf => "Convert to LF (Unix)",
            Self::ToCrlf => "Convert to CRLF (Windows)",
            Self::IndentSpaces => "Indent using spaces",
            Self::IndentTabs => "Indent using tabs",
            Self::Indent2 => "2 spaces",
            Self::Indent4 => "4 spaces",
            Self::Indent8 => "8 spaces",
            Self::Tabs => "Show tabs",
            Self::StatusBar => "Show status bar",
            Self::CurrentLine => "Highlight current line",
            Self::Whitespace => "Show whitespace",
            Self::Fullscreen => "Full screen",
            Self::DistractionFree => "Distraction-free mode",
            Self::RulerNone => "No ruler",
            Self::Ruler80 => "Ruler at 80 columns",
            Self::Ruler100 => "Ruler at 100 columns",
            Self::Ruler120 => "Ruler at 120 columns",
            Self::WrapWindow => "Wrap at window width",
            Self::Wrap80 => "Wrap at 80 columns",
            Self::Wrap100 => "Wrap at 100 columns",
            Self::Wrap120 => "Wrap at 120 columns",

            Self::Find => "Find…",
            Self::Replace => "Replace…",
            Self::Goto => "Go to line…",
            Self::Switcher => "Go to file…",
            Self::Palette => "Command palette…",
            Self::Sidebar => "Show sidebar",
            Self::FolderSidebar => "Show folder sidebar",
            Self::Wrap => "Word wrap",
            Self::LineNumbers => "Show line numbers",
            Self::ZoomIn => "Increase font size",
            Self::ZoomOut => "Decrease font size",
            Self::ResetZoom => "Reset font size",
            Self::Bookmark => "Toggle bookmark",
            Self::NextBookmark => "Next bookmark",
            Self::PreviousBookmark => "Previous bookmark",
            Self::Bookmarks => "Bookmarks…",
            Self::Follow => "Follow log (read only)",
            Self::Gpu => "CUDA panel",
            Self::Settings => "Settings…",
            Self::Appearance => "Color theme…",
            Self::ToolSettings => "CUDA tool settings…",
            Self::Checkpoint => "Checkpoint now",
            Self::About => "About hedit",
            Self::Updates => "Check for updates…",
        }
    }
    pub(super) fn shortcut(self) -> String {
        let shortcut = self.shortcut_label();
        if cfg!(target_os = "macos") {
            shortcut.replace("Ctrl", "Cmd").replace("Alt", "Option")
        } else {
            shortcut.to_owned()
        }
    }
    fn shortcut_label(self) -> &'static str {
        match self {
            Self::NextOccurrence => "Ctrl+D",
            Self::AllOccurrences => "Ctrl+Shift+L",
            Self::FocusPane => "Ctrl+Alt+Right",
            Self::MovePane => "Ctrl+Alt+Shift+Right",
            Self::Fold => "Ctrl+Shift+[",
            Self::Unfold => "Ctrl+Shift+]",
            Self::New => "Ctrl+N",
            Self::Open => "Ctrl+O",
            Self::OpenFolder => "Ctrl+Shift+O",
            Self::Sidebar => "Ctrl+B",
            Self::FolderSidebar => "Ctrl+Alt+B",
            Self::Save => "Ctrl+S",
            Self::SaveAs => "Ctrl+Shift+S",
            Self::Close => "Ctrl+W",
            Self::Reopen => "Ctrl+Shift+T",
            Self::Rename => "Ctrl+Shift+R",
            Self::Undo => "Ctrl+Z",
            Self::Redo => "Ctrl+Shift+Z",
            Self::Cut => "Ctrl+X",
            Self::Copy => "Ctrl+C",
            Self::Paste => "Ctrl+V",
            Self::SelectAll => "Ctrl+A",
            Self::SelectLine => "Ctrl+L",
            Self::Duplicate => "Ctrl+Shift+D",
            Self::DeleteLine => "Ctrl+Shift+K",
            Self::InsertBefore => "Ctrl+Shift+Enter",
            Self::InsertAfter => "Ctrl+Enter",
            Self::MoveUp => "Ctrl+Shift+Up",
            Self::MoveDown => "Ctrl+Shift+Down",
            Self::JoinLines => "Ctrl+J",
            Self::Indent => "Ctrl+]",
            Self::Unindent => "Ctrl+[",
            Self::ToggleComment => "Ctrl+/",
            Self::Fullscreen => "F11",
            Self::DistractionFree => "Shift+F11",

            Self::Find => "Ctrl+F",
            Self::Replace => "Ctrl+H",
            Self::Goto => "Ctrl+G",
            Self::Switcher => "Ctrl+P",
            Self::Palette => "Ctrl+Shift+P",
            Self::Wrap => "Ctrl+Shift+W",
            Self::ZoomIn => "Ctrl++",
            Self::ZoomOut => "Ctrl+-",
            Self::Bookmark => "Ctrl+F2",
            Self::NextBookmark => "F2",
            Self::PreviousBookmark => "Shift+F2",
            Self::Settings => "Ctrl+,",
            _ => "",
        }
    }
    pub(super) fn operation(self) -> Option<hedit::editing::Operation> {
        use hedit::editing::Operation;
        Some(match self {
            Self::SelectLine => Operation::SelectLine,
            Self::Duplicate => Operation::Duplicate,
            Self::DeleteLine => Operation::DeleteLine,
            Self::InsertBefore => Operation::InsertBefore,
            Self::InsertAfter => Operation::InsertAfter,
            Self::MoveUp => Operation::MoveUp,
            Self::MoveDown => Operation::MoveDown,
            Self::JoinLines => Operation::JoinLines,
            Self::Indent => Operation::Indent,
            Self::Unindent => Operation::Unindent,
            Self::Uppercase => Operation::Uppercase,
            Self::Lowercase => Operation::Lowercase,
            Self::SortLines => Operation::SortLines,
            Self::ReverseLines => Operation::ReverseLines,
            Self::UniqueLines => Operation::UniqueLines,
            Self::TrimWhitespace => Operation::TrimWhitespace,
            Self::ToSpaces => Operation::ToSpaces,
            Self::ToTabs => Operation::ToTabs,
            Self::ToggleComment => Operation::ToggleComment,
            Self::ToLf => Operation::ToLf,
            Self::ToCrlf => Operation::ToCrlf,
            _ => return None,
        })
    }
}
impl HeditApp {
    pub(super) fn command_enabled(&self, command: Command) -> bool {
        if self.busy || self.closing {
            return false;
        }
        if command
            .operation()
            .is_some_and(|op| op != hedit::editing::Operation::SelectLine)
            && (self.active().follow || self.active().page.info.paged)
        {
            return false;
        }
        match command {
            Command::NextOccurrence
            | Command::AllOccurrences
            | Command::Rectangle
            | Command::Fold
            | Command::FoldAll => !self.active().page.info.paged,
            Command::FocusPane | Command::MovePane => self.panes.mode != 0,
            Command::Gpu | Command::ToolSettings => hedit::debugger::LOCAL_TOOLS_SUPPORTED,
            Command::Uppercase | Command::Lowercase => {
                self.active().view.anchor != self.active().view.head
            }
            Command::ToggleComment => {
                hedit::editing::comment_prefix(self.active().language().id()).is_some()
            }
            Command::CloseFolder | Command::RefreshFolder => !self.workspace.folders.is_empty(),
            Command::Rename => self.active().page.info.path.is_none(),
            Command::Reopen => !self.workspace.closed.is_empty(),
            Command::Follow => {
                self.active().page.info.path.is_some() && !self.active().page.info.dirty
            }
            Command::Undo | Command::Redo | Command::Cut | Command::Paste | Command::Replace => {
                !self.active().follow
            }
            _ => true,
        }
    }
    pub(super) fn command_selected(&self, command: Command) -> bool {
        match command {
            Command::AutoIndent => self.auto_indent,
            Command::SmartIndent => self.smart_indent,
            Command::SoftTabs => self.soft_tabs,
            Command::IndentGuides => self.indent_guides,
            Command::Minimap => self.minimap,
            Command::SinglePane => self.panes.mode == 0,
            Command::SplitColumns => self.panes.mode == 1,
            Command::SplitRows => self.panes.mode == 2,
            Command::Sidebar => self.sidebar,
            Command::FolderSidebar => self.sidebar && self.folder_sidebar,
            Command::Tabs => self.show_tabs,
            Command::StatusBar => self.show_status,
            Command::CurrentLine => self.highlight_line,
            Command::Whitespace => self.show_whitespace,
            Command::ToLf => self.active().page.info.eol == "LF",
            Command::ToCrlf => self.active().page.info.eol == "CRLF",
            Command::Fullscreen => self.fullscreen,
            Command::DistractionFree => self.distraction_free,
            Command::RulerNone => self.ruler_column == 0,
            Command::Ruler80 => self.ruler_column == 80,
            Command::Ruler100 => self.ruler_column == 100,
            Command::Ruler120 => self.ruler_column == 120,
            Command::WrapWindow => self.wrap_column == 0,
            Command::Wrap80 => self.wrap_column == 80,
            Command::Wrap100 => self.wrap_column == 100,
            Command::Wrap120 => self.wrap_column == 120,
            Command::IndentTabs => self.active().view.indent.as_deref() == Some("tabs"),
            Command::IndentSpaces => self.active().view.indent.as_deref() != Some("tabs"),
            Command::Indent2 => self.active().view.tab_size == Some(2),
            Command::Indent4 => self.active().view.tab_size.unwrap_or(4) == 4,
            Command::Indent8 => self.active().view.tab_size == Some(8),
            Command::Wrap => self.wrap,
            Command::LineNumbers => self.line_numbers,
            Command::Follow => self.active().follow,
            Command::Gpu => self.gpu_open,
            _ => false,
        }
    }
    pub(super) fn execute(&mut self, ctx: &egui::Context, command: Command) {
        if !self.command_enabled(command) {
            return;
        }
        match command {
            Command::NextOccurrence => self.occurrence(false),
            Command::AllOccurrences => {
                if self.active().view.anchor == self.active().view.head {
                    self.occurrence(false);
                }
                self.occurrence(true);
            }
            Command::SingleSelection => {
                self.workspace.open[self.workspace.active]
                    .view
                    .selections
                    .clear();
                self.focus_editor = true;
            }
            Command::Rectangle => {
                let b = self.active();
                let ranges = hedit::advanced::rectangle(
                    &b.page.text,
                    b.view.anchor,
                    b.view.head,
                    b.view.tab_size.unwrap_or(4) as usize,
                );
                self.set_selections(ranges);
            }
            Command::SinglePane => self.split(0),
            Command::SplitColumns => self.split(1),
            Command::SplitRows => self.split(2),
            Command::FocusPane => self.focus_pane(true),
            Command::MovePane => self.focus_pane(false),
            Command::AutoIndent => self.auto_indent = !self.auto_indent,
            Command::SmartIndent => self.smart_indent = !self.smart_indent,
            Command::SoftTabs => self.soft_tabs = !self.soft_tabs,
            Command::IndentGuides => self.indent_guides = !self.indent_guides,
            Command::Minimap => self.minimap = !self.minimap,
            Command::DetectIndent => self.detect_indent(),
            Command::Fold => self.fold(false),
            Command::FoldAll => self.fold(true),
            Command::UnfoldAll => {
                let b = &mut self.workspace.open[self.workspace.active];
                b.view.folds.clear();
                b.restore_cursor = true;
                self.changed();
            }
            Command::Unfold => {
                let b = &mut self.workspace.open[self.workspace.active];
                let at = b.view.head;
                let candidate = b
                    .view
                    .folds
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| f.start <= at && at <= f.end)
                    .min_by_key(|(_, f)| f.end - f.start)
                    .map(|(i, _)| i)
                    .or_else(|| (!b.view.folds.is_empty()).then_some(0));
                if let Some(i) = candidate {
                    b.view.folds.remove(i);
                    b.restore_cursor = true;
                    self.changed();
                }
            }
            Command::New => {
                let r = self.workspace.new_buffer();
                self.report(r);
            }
            Command::Open => self.open(ctx, None),
            Command::OpenFolder => self.open_folder(ctx),
            Command::CloseFolder => self.close_folder(),
            Command::RefreshFolder => self.refresh_folder(),
            Command::Save => self.save_file(ctx, false),
            Command::SaveAs => self.save_file(ctx, true),
            Command::Close => {
                let r = self.workspace.close(self.workspace.active);
                self.report(r);
            }
            Command::Reopen => {
                let r = self.workspace.reopen();
                self.report(r);
            }
            Command::Shelf => self.shelf_open = true,
            Command::Rename => self.open_unsaved_file_name(),
            Command::Quit => {
                self.closing = true;
                self.checkpoint(ctx);
            }
            Command::Undo => self.history(ctx, false),
            Command::Redo => self.history(ctx, true),
            Command::Copy | Command::Cut if !self.active().view.selections.is_empty() => {
                let ranges = self.selections();
                let text = &self.active().page.text;
                ctx.copy_text(
                    ranges
                        .iter()
                        .map(|r| {
                            text[hedit::advanced::byte(text, r.start())
                                ..hedit::advanced::byte(text, r.end())]
                                .to_owned()
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
                if command == Command::Cut {
                    let batch = hedit::advanced::replace(text, &ranges, "");
                    self.batch(batch);
                }
            }
            Command::Copy | Command::Cut => {
                let buffer = self.active();
                let start = buffer.view.anchor.min(buffer.view.head);
                let end = buffer.view.anchor.max(buffer.view.head);
                ctx.copy_text(
                    buffer
                        .page
                        .text
                        .chars()
                        .skip(start)
                        .take(end - start)
                        .collect(),
                );
                if command == Command::Cut && start != end {
                    let index = self.workspace.active;
                    let old = self.active().page.text.clone();
                    let text: String = old
                        .chars()
                        .take(start)
                        .chain(old.chars().skip(end))
                        .collect();
                    self.workspace.open[index].page.text = text;
                    match self.workspace.edit(index, &old) {
                        Ok(()) => {
                            self.workspace.open[index].view.anchor = start;
                            self.workspace.open[index].view.head = start;
                            self.changed();
                        }
                        Err(error) => {
                            self.workspace.open[index].page.text = old;
                            self.error = Some(error);
                        }
                    }
                    self.workspace.open[index].restore_cursor = true;
                }
                self.focus_editor = true;
            }
            Command::Paste => {
                self.focus_editor = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
            Command::SelectAll => {
                let index = self.workspace.active;
                let buffer = &mut self.workspace.open[index];
                buffer.view.anchor = 0;
                buffer.view.head = buffer.page.text.chars().count();
                buffer.restore_cursor = true;
                self.focus_editor = true;
                self.changed();
            }
            Command::Find | Command::Replace => {
                self.find_open = true;
                self.find_focus = true;
                self.replace_open = command == Command::Replace;
            }
            Command::Goto => {
                self.goto_open = true;
                self.goto_focus = true;
            }
            Command::Switcher | Command::Palette => {
                self.palette = true;
                self.commands_only = command == Command::Palette;
                self.palette_query.clear();
                self.palette_focus = true;
                self.palette_index = 0;
            }
            Command::Sidebar => self.sidebar = !self.sidebar,
            Command::FolderSidebar => {
                self.folder_sidebar = !(self.sidebar && self.folder_sidebar);
                if self.folder_sidebar {
                    self.sidebar = true;
                }
                self.sync_folder_view();
            }
            Command::Tabs => self.show_tabs = !self.show_tabs,
            Command::StatusBar => self.show_status = !self.show_status,
            Command::CurrentLine => self.highlight_line = !self.highlight_line,
            Command::Whitespace => self.show_whitespace = !self.show_whitespace,
            Command::Fullscreen => {
                self.fullscreen = !self.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            Command::DistractionFree => {
                if !self.distraction_free {
                    self.distraction_was_fullscreen = self.fullscreen;
                    self.fullscreen = true;
                } else {
                    self.fullscreen = self.distraction_was_fullscreen;
                }
                self.distraction_free = !self.distraction_free;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            Command::RulerNone => self.ruler_column = 0,
            Command::Ruler80 => self.ruler_column = 80,
            Command::Ruler100 => self.ruler_column = 100,
            Command::Ruler120 => self.ruler_column = 120,
            Command::WrapWindow | Command::Wrap80 | Command::Wrap100 | Command::Wrap120 => {
                self.wrap_column = match command {
                    Command::Wrap80 => 80,
                    Command::Wrap100 => 100,
                    Command::Wrap120 => 120,
                    _ => 0,
                };
                self.wrap = true;
            }
            Command::IndentSpaces
            | Command::IndentTabs
            | Command::Indent2
            | Command::Indent4
            | Command::Indent8 => {
                let view = &mut self.workspace.open[self.workspace.active].view;
                match command {
                    Command::IndentSpaces => view.indent = Some("spaces".into()),
                    Command::IndentTabs => view.indent = Some("tabs".into()),
                    Command::Indent2 => view.tab_size = Some(2),
                    Command::Indent8 => view.tab_size = Some(8),
                    _ => view.tab_size = Some(4),
                }
                self.changed();
            }
            Command::Wrap => self.wrap = !self.wrap,
            Command::LineNumbers => self.line_numbers = !self.line_numbers,
            Command::ZoomIn => self.font_size = (self.font_size + 1.0).min(28.0),
            Command::ZoomOut => self.font_size = (self.font_size - 1.0).max(10.0),
            Command::ResetZoom => self.font_size = 14.0,
            Command::Bookmark => self.toggle_bookmark(),
            Command::NextBookmark => self.adjacent_bookmark(ctx, true),
            Command::PreviousBookmark => self.adjacent_bookmark(ctx, false),
            Command::Bookmarks => self.bookmarks_open = true,
            Command::Follow => {
                let index = self.workspace.active;
                self.workspace.open[index].follow = !self.workspace.open[index].follow;
                if self.active().follow {
                    self.load_page(ctx, u64::MAX, true);
                }
            }
            Command::Gpu => self.gpu_open = !self.gpu_open,
            Command::Settings => self.open_preferences(self.preferences_page),
            Command::Appearance => self.open_appearance(),
            Command::ToolSettings => self.open_preferences(PreferencesPage::Tools),
            Command::Checkpoint => self.checkpoint(ctx),
            Command::About => self.about_open = true,
            Command::Updates => self.open_updates(ctx),
            other => {
                if let Some(operation) = other.operation() {
                    self.edit_command(operation);
                }
            }
        }
    }
    pub(super) fn command_palette(&mut self, ctx: &egui::Context) {
        if !self.palette {
            return;
        }
        #[derive(Clone)]
        enum Item {
            Buffer(usize),
            File(String),
            Command(Command),
        }
        let commands = self.commands_only || self.palette_query.starts_with('>');
        let down = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown));
        let up = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp));
        let enter = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        let mut chosen = None;
        egui::Window::new(if commands {
            "Command palette"
        } else {
            "Go to file"
        })
        .id(egui::Id::new("command-palette"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 70.0])
        .default_width(520.0)
        .show(ctx, |ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.palette_query)
                    .desired_width(f32::INFINITY)
                    .hint_text(if commands {
                        "Type a command…"
                    } else {
                        "Type a file name, or > for commands…"
                    }),
            );
            if self.palette_focus {
                response.request_focus();
                self.palette_focus = false;
            }
            if response.changed() {
                self.palette_index = 0;
            }
            let commands = self.commands_only || self.palette_query.starts_with('>');
            let filter = self
                .palette_query
                .trim_start_matches('>')
                .trim()
                .to_lowercase();
            if !commands && !self.workspace.folders.is_empty() {
                let roots: Vec<_> = self.workspace.folders.iter().map(|f| PathBuf::from(&f.path)).collect();
                self.project.search(ctx, &roots, &filter);
            }
            let mut items = Vec::new();
            if commands {
                for &command in Command::ALL {
                    if command.label().to_lowercase().contains(&filter)
                        && self.command_enabled(command)
                    {
                        items.push((
                            command.label().to_owned(),
                            command.shortcut(),
                            Item::Command(command),
                        ));
                    }
                }
            } else {
                for (index, buffer) in self.workspace.open.iter().enumerate() {
                    if buffer.view.name.to_lowercase().contains(&filter)
                        || buffer
                            .page
                            .info
                            .path
                            .as_ref()
                            .is_some_and(|path| path.to_lowercase().contains(&filter))
                    {
                        let roots: Vec<_> = self.workspace.folders.iter().map(|f| PathBuf::from(&f.path)).collect();
                        let label = buffer.page.info.path.as_ref().map(|path| hedit::folder::display_path(&roots, std::path::Path::new(path))).unwrap_or_else(|| buffer.view.name.clone());
                        items.push((label, "open".to_owned(), Item::Buffer(index)));
                    }
                }
            }
            if !commands && !self.workspace.folders.is_empty() {
                match &self.project.search {
                    Some(Ok(results)) => {
                        for entry in &results.entries {
                            if let Some(index) = self.workspace.open.iter().position(|b| b.page.info.path.as_deref() == Some(&entry.path)) {
                                if !items.iter().any(|(_, _, item)| matches!(item, Item::Buffer(prior) if *prior == index)) {
                                    items.push((entry.name.clone(), "open".to_owned(), Item::Buffer(index)));
                                }
                            } else {
                                items.push((entry.name.clone(), "folder".to_owned(), Item::File(entry.path.clone())));
                            }
                        }
                        items.sort_by_key(|(_, _, item)| !matches!(item, Item::Buffer(_)));
                        if results.truncated {
                            ui.weak("Search limit reached; results may be incomplete.");
                        }
                        if results.skipped_directories > 0 {
                            ui.weak(format!(
                                "{} folders could not be read.",
                                results.skipped_directories
                            ));
                        }
                    }
                    Some(Err(error)) => {
                        ui.colored_label(self.theme.warning.color(), error);
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.weak("Searching folders…");
                        });
                    }
                }
            }
            self.palette_index = self.palette_index.min(items.len().saturating_sub(1));
            if down && !items.is_empty() {
                self.palette_index = (self.palette_index + 1) % items.len();
            }
            if up && !items.is_empty() {
                self.palette_index = (self.palette_index + items.len() - 1) % items.len();
            }
            let navigate = down || up;
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height((ctx.screen_rect().height() - 190.0).clamp(120.0, 320.0))
                .show(ui, |ui| {
                    for (index, (label, shortcut, item)) in items.iter().enumerate() {
                        let response = ui.add(
                            egui::Button::new(label)
                                .selected(index == self.palette_index)
                                .shortcut_text(shortcut)
                                .min_size(egui::vec2(ui.available_width(), 28.0)),
                        );
                        if navigate && index == self.palette_index {
                            response.scroll_to_me(None);
                        }
                        if response.clicked() {
                            chosen = Some(item.clone());
                        }
                    }
                });
            if items.is_empty() {
                ui.label("No matching results.");
            }
            if enter {
                chosen = items
                    .get(self.palette_index)
                    .map(|(_, _, item)| item.clone());
            }
            ui.separator();
            ui.weak("↑ ↓ navigate    Enter select    Esc close");
        });
        if let Some(item) = chosen {
            self.palette = false;
            self.focus_editor = true;
            match item {
                Item::Buffer(index) => {
                    self.workspace.active = index;
                    self.changed();
                }
                Item::File(path) => self.open(ctx, Some(path.into())),
                Item::Command(command) => self.execute(ctx, command),
            }
        }
    }
}
