use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Command {
    FindFiles,
    FindPrevious,
    ReplaceAll,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    RecentProjects,
    ProjectSettings,
    Symbols,
    NavBack,
    NavForward,
    CompareDisk,
    GitDiff,
    KeyBindings,
    Encoding,
    Complete,
    Snippets,
    PinTab,
    CloseOthers,
    CloseRight,

    NextOccurrence,
    Build,
    BuildWith,
    CancelBuild,
    BuildResults,
    NextResult,
    PreviousResult,
    SkipOccurrence,
    ExpandSelection,
    ExpandToBrackets,
    ExpandToIndentation,
    JumpToBracket,
    SplitLines,
    SoftUndo,
    SoftRedo,
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
        Self::FindFiles,
        Self::FindPrevious,
        Self::ReplaceAll,
        Self::OpenProject,
        Self::SaveProject,
        Self::SaveProjectAs,
        Self::RecentProjects,
        Self::ProjectSettings,
        Self::Symbols,
        Self::NavBack,
        Self::NavForward,
        Self::CompareDisk,
        Self::GitDiff,
        Self::KeyBindings,
        Self::Encoding,
        Self::Complete,
        Self::Snippets,
        Self::PinTab,
        Self::CloseOthers,
        Self::CloseRight,
        Self::NextOccurrence,
        Self::Build,
        Self::BuildWith,
        Self::CancelBuild,
        Self::BuildResults,
        Self::NextResult,
        Self::PreviousResult,
        Self::SkipOccurrence,
        Self::ExpandSelection,
        Self::ExpandToBrackets,
        Self::ExpandToIndentation,
        Self::JumpToBracket,
        Self::SplitLines,
        Self::SoftUndo,
        Self::SoftRedo,
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
    /// Commands that show a window or the Find bar; any other window closes first.
    fn opens_window(self) -> bool {
        matches!(
            self,
            Self::Switcher
                | Self::Palette
                | Self::Appearance
                | Self::Goto
                | Self::Find
                | Self::Replace
                | Self::Shelf
                | Self::Bookmarks
                | Self::Rename
                | Self::Settings
                | Self::ToolSettings
                | Self::KeyBindings
                | Self::ProjectSettings
                | Self::About
                | Self::Updates
                | Self::FindFiles
                | Self::Symbols
                | Self::CompareDisk
                | Self::GitDiff
                | Self::Encoding
                | Self::Complete
                | Self::Snippets
                | Self::RecentProjects
        )
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::FindFiles => "Find in folders…",
            Self::FindPrevious => "Find previous",
            Self::ReplaceAll => "Replace all in file",
            Self::OpenProject => "Open project…",
            Self::SaveProject => "Save project",
            Self::SaveProjectAs => "Save project as…",
            Self::RecentProjects => "Recent projects…",
            Self::ProjectSettings => "Project settings…",
            Self::Symbols => "Go to symbol…",
            Self::NavBack => "Navigate back",
            Self::NavForward => "Navigate forward",
            Self::CompareDisk => "Compare with disk…",
            Self::GitDiff => "Git changes…",
            Self::KeyBindings => "Key bindings and snippets…",
            Self::Encoding => "File encoding…",
            Self::Complete => "Complete word…",
            Self::Snippets => "Insert snippet…",
            Self::PinTab => "Pin tab",
            Self::CloseOthers => "Close other tabs",
            Self::CloseRight => "Close tabs to the right",

            Self::NextOccurrence => "Select next occurrence",
            Self::Build => "Build",
            Self::BuildWith => "Build with…",
            Self::CancelBuild => "Cancel build",
            Self::BuildResults => "Show build results",
            Self::NextResult => "Next build result",
            Self::PreviousResult => "Previous build result",
            Self::SkipOccurrence => "Skip occurrence",
            Self::ExpandSelection => "Expand selection",
            Self::ExpandToBrackets => "Expand selection to brackets",
            Self::ExpandToIndentation => "Expand selection to indentation",
            Self::JumpToBracket => "Jump to matching bracket",
            Self::SplitLines => "Split selection into lines",
            Self::SoftUndo => "Undo selection change",
            Self::SoftRedo => "Redo selection change",
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
            Self::Indent2 => "Tab width: 2",
            Self::Indent4 => "Tab width: 4",
            Self::Indent8 => "Tab width: 8",
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
            Self::FolderSidebar => "Show folders in the sidebar",
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
            Self::FindFiles => "Ctrl+Shift+F",
            Self::FindPrevious => "Shift+F3",
            Self::Symbols => "Ctrl+R",
            // Option+arrows move by word on macOS, so use Sublime's Control+-.
            Self::NavBack if cfg!(target_os = "macos") => "Control+-",
            Self::NavForward if cfg!(target_os = "macos") => "Control+Shift+-",
            Self::NavBack => "Alt+Left",
            Self::NavForward => "Alt+Right",
            // Cmd+Space is Spotlight on macOS; Sublime uses Control+Space there.
            Self::Complete if cfg!(target_os = "macos") => "Control+Space",
            Self::Complete => "Ctrl+Space",
            Self::NextOccurrence => "Ctrl+D",
            Self::Build => "Ctrl+B",
            Self::BuildWith => "Ctrl+Shift+B",
            Self::NextResult => "F4",
            Self::PreviousResult => "Shift+F4",
            Self::SkipOccurrence => "Ctrl+K, Ctrl+D",
            Self::Uppercase => "Ctrl+K, Ctrl+U",
            Self::Lowercase => "Ctrl+K, Ctrl+L",
            Self::FoldAll => "Ctrl+K, Ctrl+1",
            Self::UnfoldAll => "Ctrl+K, Ctrl+J",
            // Sublime: Alt+F3 on Linux, Control+Cmd+G on macOS.
            Self::AllOccurrences if cfg!(target_os = "macos") => "Control+Ctrl+G",
            Self::AllOccurrences => "Alt+F3",
            Self::SplitLines => "Ctrl+Shift+L",
            Self::ExpandSelection => "Ctrl+Shift+Space",
            Self::ExpandToBrackets => "Ctrl+Shift+M",
            Self::ExpandToIndentation => "Ctrl+Shift+J",
            Self::JumpToBracket if cfg!(target_os = "macos") => "Control+M",
            Self::JumpToBracket => "Ctrl+M",
            Self::SoftUndo => "Ctrl+U",
            Self::SoftRedo => "Ctrl+Shift+U",
            Self::FocusPane => "Ctrl+Alt+Right",
            Self::MovePane => "Ctrl+Alt+Shift+Right",
            Self::Fold => "Ctrl+Shift+[",
            Self::Unfold => "Ctrl+Shift+]",
            Self::New => "Ctrl+N",
            Self::Open => "Ctrl+O",
            Self::OpenFolder => "Ctrl+Shift+O",
            Self::Sidebar => "Ctrl+K, Ctrl+B",
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
            // macOS reserves F11; use its Control+Cmd+F convention.
            Self::Fullscreen if cfg!(target_os = "macos") => "Control+Ctrl+F",
            Self::DistractionFree if cfg!(target_os = "macos") => "Control+Shift+Ctrl+F",
            Self::Fullscreen => "F11",
            Self::DistractionFree => "Shift+F11",
            Self::Quit => "Ctrl+Q",
            Self::ResetZoom => "Ctrl+0",

            Self::Find => "Ctrl+F",
            // Cmd+H hides the app on macOS; Sublime uses Cmd+Option+F there.
            Self::Replace if cfg!(target_os = "macos") => "Ctrl+Alt+F",
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
            | Command::SkipOccurrence
            | Command::ExpandSelection
            | Command::ExpandToBrackets
            | Command::ExpandToIndentation
            | Command::JumpToBracket
            | Command::SplitLines
            | Command::Rectangle
            | Command::Fold
            | Command::FoldAll => !self.active().page.info.paged,
            Command::FocusPane | Command::MovePane => self.panes.mode != 0,
            Command::CancelBuild => self.build.running(),
            Command::NextResult | Command::PreviousResult => !self.build.locations.is_empty(),
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
            Command::PinTab => self.active().view.pinned,
            Command::AutoIndent => self.settings.auto_indent,
            Command::SmartIndent => self.settings.smart_indent,
            Command::SoftTabs => self.settings.smart_backspace,
            Command::IndentGuides => self.settings.draw_indent_guides,
            Command::Minimap => self.settings.show_minimap,
            Command::SinglePane => self.panes.mode == 0,
            Command::SplitColumns => self.panes.mode == 1,
            Command::SplitRows => self.panes.mode == 2,
            Command::Sidebar => self.settings.show_sidebar,
            Command::FolderSidebar => self.settings.show_sidebar && self.settings.show_folders,
            Command::Tabs => self.settings.show_tab_bar,
            Command::StatusBar => self.settings.show_status_bar,
            Command::CurrentLine => self.settings.highlight_line,
            Command::Whitespace => self.settings.draw_white_space,
            Command::ToLf => self.active().page.info.eol == "LF",
            Command::ToCrlf => self.active().page.info.eol == "CRLF",
            Command::Fullscreen => self.fullscreen,
            Command::DistractionFree => self.distraction_free,
            Command::RulerNone => self.settings.rulers.is_empty(),
            Command::Ruler80 => self.settings.rulers == [80],
            Command::Ruler100 => self.settings.rulers == [100],
            Command::Ruler120 => self.settings.rulers == [120],
            Command::WrapWindow => self.settings.wrap_width == 0,
            Command::Wrap80 => self.settings.wrap_width == 80,
            Command::Wrap100 => self.settings.wrap_width == 100,
            Command::Wrap120 => self.settings.wrap_width == 120,
            Command::IndentTabs => self.active().view.indent.as_deref() == Some("tabs"),
            Command::IndentSpaces => self.active().view.indent.as_deref() != Some("tabs"),
            Command::Indent2 => self.active().view.tab_size == Some(2),
            Command::Indent4 => self.active().view.tab_size.unwrap_or(4) == 4,
            Command::Indent8 => self.active().view.tab_size == Some(8),
            Command::Wrap => self.settings.word_wrap,
            Command::LineNumbers => self.settings.line_numbers,
            Command::Follow => self.active().follow,
            Command::Gpu => self.gpu_open,
            Command::BuildResults => self.build.open,
            _ => false,
        }
    }
    pub(super) fn execute(&mut self, ctx: &egui::Context, command: Command) {
        if !self.command_enabled(command) {
            return;
        }
        if command.opens_window() {
            self.close_transient(ctx);
        }
        match command {
            Command::FindFiles => {
                self.quality.folders_open = true;
                self.quality.folder_focus = true;
                self.focus_editor = false;
            }
            Command::FindPrevious => self.find_advanced(ctx, true),
            Command::ReplaceAll => self.replace_advanced(true),
            Command::OpenProject => self.load_project(ctx, None),
            Command::SaveProject => self.save_project(ctx, false),
            Command::SaveProjectAs => self.save_project(ctx, true),
            Command::RecentProjects => self.quality.recent_open = true,
            Command::ProjectSettings => {
                self.quality.settings_project = true;
                self.quality.settings_json =
                    serde_json::to_string_pretty(&self.quality.project_settings)
                        .unwrap_or_default();
                self.quality.settings_open = true;
            }
            Command::Symbols => self.list_symbols(ctx),
            Command::NavBack => self.navigate(ctx, false),
            Command::NavForward => self.navigate(ctx, true),
            Command::CompareDisk => self.compare_disk(ctx, false),
            Command::GitDiff => self.compare_disk(ctx, true),
            Command::KeyBindings => self.open_preferences(PreferencesPage::Keys),
            Command::Encoding => {
                self.quality.encoding = self.active().page.info.encoding.clone();
                self.quality.encoding_open = true;
            }
            Command::Complete => self.open_completion(),
            Command::Snippets => {
                self.quality.snippets_open = true;
                self.quality.snippet_focus = true;
                self.focus_editor = false;
            }
            Command::PinTab => {
                let b = &mut self.workspace.open[self.workspace.active];
                b.view.pinned = !b.view.pinned;
                self.quality.preview = None;
                self.changed();
            }
            Command::CloseOthers => self.close_tab_group(false),
            Command::CloseRight => self.close_tab_group(true),
            Command::NextOccurrence => self.occurrence(false),
            Command::Build => self.build(ctx),
            Command::BuildWith => {
                // Choose from "Build with: …" entries in the command palette.
                self.palette = true;
                self.commands_only = true;
                self.palette_query = "build with: ".into();
                self.palette_focus = true;
                self.palette_index = 0;
            }
            Command::CancelBuild => self.cancel_build(),
            Command::BuildResults => self.build.open = !self.build.open,
            Command::NextResult => self.next_result(true),
            Command::PreviousResult => self.next_result(false),
            Command::SkipOccurrence => {
                let ranges =
                    hedit::advanced::skip_occurrence(&self.active().page.text, &self.selections());
                self.set_selections(ranges);
            }
            Command::ExpandSelection
            | Command::ExpandToBrackets
            | Command::ExpandToIndentation
            | Command::JumpToBracket => {
                use hedit::advanced as a;
                let text = self.active().page.text.clone();
                let expand = match command {
                    Command::ExpandSelection => a::expand_selection,
                    Command::ExpandToBrackets => a::expand_to_brackets,
                    Command::ExpandToIndentation => a::expand_to_indentation,
                    _ => a::jump_to_bracket,
                };
                let ranges = self
                    .selections()
                    .into_iter()
                    .map(|s| expand(&text, s))
                    .collect();
                self.set_selections(ranges);
            }
            Command::SplitLines => {
                let ranges =
                    hedit::advanced::split_into_lines(&self.active().page.text, &self.selections());
                self.set_selections(ranges);
            }
            Command::SoftUndo => self.soft_undo(false),
            Command::SoftRedo => self.soft_undo(true),
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
            Command::AutoIndent => self.settings.auto_indent = !self.settings.auto_indent,
            Command::SmartIndent => self.settings.smart_indent = !self.settings.smart_indent,
            Command::SoftTabs => self.settings.smart_backspace = !self.settings.smart_backspace,
            Command::IndentGuides => {
                self.settings.draw_indent_guides = !self.settings.draw_indent_guides
            }
            Command::Minimap => self.settings.show_minimap = !self.settings.show_minimap,
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
                if r.is_ok() {
                    self.apply_format_preferences();
                }
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
                self.quality.paste_requested = true;
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
                let b = self.active();
                self.quality.scope = (b.view.anchor != b.view.head).then(|| {
                    (
                        b.view.id,
                        b.page.start as usize
                            + hedit::advanced::byte(&b.page.text, b.view.anchor.min(b.view.head)),
                        b.page.start as usize
                            + hedit::advanced::byte(&b.page.text, b.view.anchor.max(b.view.head)),
                    )
                });
                self.quality.selection_only = false;
                // Typing in Find searches from here.
                let b = self.active();
                self.live_find.origin = Some((b.view.id, b.view.anchor.min(b.view.head)));
                self.find_open = true;
                self.find_focus = true;
                // The palette asks for editor focus when it runs a command.
                self.focus_editor = false;
                self.replace_open = command == Command::Replace;
            }
            Command::Goto => {
                self.goto_open = true;
                self.goto_focus = true;
                self.focus_editor = false;
            }
            Command::Switcher | Command::Palette => {
                self.cancel_theme_palette(ctx);
                self.palette = true;
                self.commands_only = command == Command::Palette;
                self.palette_query.clear();
                self.palette_focus = true;
                self.palette_index = 0;
            }
            Command::Sidebar => self.settings.show_sidebar = !self.settings.show_sidebar,
            Command::FolderSidebar => {
                self.settings.show_folders =
                    !(self.settings.show_sidebar && self.settings.show_folders);
                if self.settings.show_folders {
                    self.settings.show_sidebar = true;
                }
                self.sync_folder_view();
            }
            Command::Tabs => self.settings.show_tab_bar = !self.settings.show_tab_bar,
            Command::StatusBar => self.settings.show_status_bar = !self.settings.show_status_bar,
            Command::CurrentLine => self.settings.highlight_line = !self.settings.highlight_line,
            Command::Whitespace => self.settings.draw_white_space = !self.settings.draw_white_space,
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
            Command::RulerNone => self.settings.rulers.clear(),
            Command::Ruler80 => self.settings.rulers = vec![80],
            Command::Ruler100 => self.settings.rulers = vec![100],
            Command::Ruler120 => self.settings.rulers = vec![120],
            Command::WrapWindow | Command::Wrap80 | Command::Wrap100 | Command::Wrap120 => {
                self.settings.wrap_width = match command {
                    Command::Wrap80 => 80,
                    Command::Wrap100 => 100,
                    Command::Wrap120 => 120,
                    _ => 0,
                };
                self.settings.word_wrap = true;
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
            Command::Wrap => self.settings.word_wrap = !self.settings.word_wrap,
            Command::LineNumbers => self.settings.line_numbers = !self.settings.line_numbers,
            Command::ZoomIn => self.settings.font_size = (self.settings.font_size + 1.0).min(28.0),
            Command::ZoomOut => self.settings.font_size = (self.settings.font_size - 1.0).max(10.0),
            Command::ResetZoom => self.settings.font_size = 14.0,
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
            Command::Appearance => self.open_theme_palette(),
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
        if self.appearance.preview.is_some() {
            return self.theme_palette(ctx);
        }
        #[derive(Clone)]
        enum Item {
            Buffer(usize),
            File(String),
            Command(Command),
            Syntax(syntax::Language),
            /// Remember a build system (or "automatic") and build with it.
            BuildWith(String),
            /// A 1-based line and column in the current file.
            Line(usize, Option<usize>),
        }
        let commands = self.commands_only || self.palette_query.starts_with('>');
        let mut target: Option<goto_anything::Target> = None;
        let mut preview = None;
        let down = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown));
        let up = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp));
        let enter = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        let mut chosen = None;
        let shown = egui::Window::new(if commands {
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
                        "File name, then @symbol, :line, or #text; > for commands…"
                    }),
            );
            if self.palette_focus {
                response.request_focus();
                self.palette_focus = false;
            }
            let changed = response.changed();
            if changed {
                self.palette_index = 0;
            }
            let commands = self.commands_only || self.palette_query.starts_with('>');
            let filter = self
                .palette_query
                .trim_start_matches('>')
                .trim()
                .to_lowercase();
            // Go to Anything: `file@symbol`, `file:line`, `file#text`.
            let (file_filter, goto) = if commands {
                (filter.as_str(), None)
            } else {
                goto_anything::parse(&filter)
            };
            let filter = file_filter.to_owned();
            let here = goto.is_some() && filter.is_empty();
            target = goto.clone();
            if !commands && !here && !self.workspace.folders.is_empty() {
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
                // Build with a chosen system, remembered for Ctrl+B.
                if filter.starts_with("build") {
                    let names = std::iter::once("Automatic".to_owned()).chain(
                        hedit::build::all(&self.settings.build_systems)
                            .into_iter()
                            .map(|s| s.name),
                    );
                    for name in names {
                        let label = format!("Build with: {name}");
                        let current = self.settings.build_system.eq_ignore_ascii_case(&name);
                        if label.to_lowercase().contains(&filter) {
                            items.push((
                                label,
                                if current { "current".into() } else { String::new() },
                                Item::BuildWith(name),
                            ));
                        }
                    }
                }
                // Like Sublime's “Set Syntax: …” commands.
                if !filter.is_empty() {
                    for language in syntax::Language::ALL {
                        let label = format!("Set syntax: {}", language.label());
                        if label.to_lowercase().contains(&filter) {
                            items.push((label, String::new(), Item::Syntax(language)));
                        }
                    }
                }
            } else if here {
                // Targets in the current file, previewed as you move.
                let b = self.active();
                let path = b.page.info.path.clone().map(PathBuf::from).unwrap_or_default();
                match goto.clone().unwrap() {
                    goto_anything::Target::Symbol(query) => {
                        for (name, line) in goto_anything::symbol_matches(&b.page.text, &path, &query)
                            .into_iter()
                            .take(500)
                        {
                            items.push((name, format!("line {line}"), Item::Line(line, None)));
                        }
                    }
                    goto_anything::Target::Line(line, column) => {
                        if let Some(line) = line {
                            let label = match column {
                                Some(c) => format!("Go to line {line}, column {c}"),
                                None => format!("Go to line {line}"),
                            };
                            items.push((label, String::new(), Item::Line(line, column)));
                        }
                    }
                    goto_anything::Target::Text(query) => {
                        for (text, line) in goto_anything::text_matches(&b.page.text, &query, 500) {
                            items.push((text, format!("line {line}"), Item::Line(line, None)));
                        }
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
            if !commands && !here && !self.workspace.folders.is_empty() {
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
            if navigate || changed {
                if let Some((_, _, Item::Line(line, column))) = items.get(self.palette_index) {
                    preview = Some((*line, *column));
                }
            }
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
        if chosen.is_none() && overlays::clicked_away(ctx, &shown) {
            self.cancel_goto_preview();
            self.palette = false;
            self.focus_editor = true;
            return;
        }
        if let Some((line, column)) = preview {
            self.preview_line(ctx, line, column);
        }
        if let Some(item) = chosen {
            self.palette = false;
            self.focus_editor = true;
            match item {
                Item::Line(line, column) => {
                    // Mark where the search started, so Back returns there.
                    self.cancel_goto_preview();
                    self.navigation_mark();
                    self.jump_to_line(ctx, line, column);
                }
                Item::Buffer(index) => {
                    self.workspace.active = index;
                    self.changed();
                    if let Some(target) = target {
                        self.go_to_target(ctx, None, target);
                    }
                }
                Item::File(path) if target.is_some() => {
                    self.go_to_target(ctx, Some(path), target.unwrap());
                }
                Item::File(path) => self.open(ctx, Some(path.into())),
                Item::Command(command) => self.execute(ctx, command),
                Item::Syntax(language) => self.set_syntax(Some(language)),
                Item::BuildWith(name) => {
                    self.settings.build_system = if name == "Automatic" {
                        "automatic".into()
                    } else {
                        name
                    };
                    self.build(ctx);
                }
            }
        }
    }
}
