use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Command {
    Notifications,
    FocusExplorer,
    FocusEditor,
    FocusSearch,
    FocusTerminal,
    TogglePanel,
    MaximizePanel,
    ExplorerNewFile,
    ExplorerNewFolder,
    ExplorerRename,
    ConnectSsh,
    SshOutput,
    MarkdownSource,
    MarkdownSplit,
    MarkdownPreview,
    MarkdownRefresh,
    FindFiles,
    FindPrevious,
    SelectAllMatches,
    UseSelectionForFind,
    UseSelectionForReplace,
    QuickFindNext,
    QuickFindPrevious,
    ReplaceAll,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    RecentProjects,
    ProjectSettings,
    Symbols,
    GotoDefinition,
    GotoReferences,
    SwitchCounterpart,
    OpenFileReference,
    NavBack,
    NavForward,
    CompareDisk,
    GitDiff,
    SourceControl,
    GitCommit,
    GitNextFile,
    GitPreviousFile,
    NextChange,
    PreviousChange,
    RevertChange,
    KeyBindings,
    Encoding,
    Complete,
    Snippets,
    RecordMacro,
    PlayMacro,
    SaveMacro,
    RunMacro,
    PinTab,
    CloseOthers,
    CloseRight,
    OpenRecent,
    CloseAll,
    CloseUnmodified,
    SelectBookmarks,
    ClearBookmarks,
    ScrollLineUp,
    ScrollLineDown,
    CopyPath,
    CopyFileLocation,
    RevealInSidebar,
    NextTab,
    PreviousTab,
    RecentTab,
    RecentTabBack,
    Tab1,
    Tab2,
    Tab3,
    Tab4,
    Tab5,
    Tab6,
    Tab7,
    Tab8,
    Tab9,

    NextOccurrence,
    Build,
    BuildWith,
    RunProjectTask,
    RerunProjectTask,
    CancelBuild,
    BuildResults,
    Terminal,
    TerminalSettings,
    NewTerminal,
    CloseTerminal,
    NextTerminal,
    PreviousTerminal,
    RestartTerminal,
    NextResult,
    PreviousResult,
    SkipOccurrence,
    ExpandSelection,
    ExpandToBrackets,
    ExpandToIndentation,
    ExpandToTag,
    JumpToBracket,
    SplitLines,
    AddCursorAbove,
    AddCursorBelow,
    SoftUndo,
    SoftRedo,
    AllOccurrences,
    SingleSelection,
    Rectangle,
    SplitColumns,
    SplitRows,
    SinglePane,
    Columns3,
    Columns4,
    Rows3,
    Grid,
    FocusGroup1,
    FocusGroup2,
    FocusGroup3,
    FocusGroup4,
    FocusPane,
    MovePane,
    AutoIndent,
    SmartIndent,
    SoftTabs,
    DetectIndent,
    Fold,
    FoldAll,
    FoldLevel1,
    FoldLevel2,
    FoldLevel3,
    Unfold,
    UnfoldAll,
    IndentGuides,
    Minimap,
    New,
    NewFileFromTemplate,
    NewWindow,
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
    PasteHistory,
    PasteIndent,
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
    ToggleBlockComment,
    WrapLines,
    Transpose,
    TitleCase,
    SwapCase,
    DeleteToLineStart,
    DeleteToLineEnd,
    Reindent,
    SortLinesIgnoreCase,
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
    Breadcrumbs,
    Outline,
    FolderSidebar,
    Wrap,
    LineNumbers,
    ZoomIn,
    ZoomOut,
    ResetZoom,
    ToggleSpellCheck,
    NextMisspelling,
    PreviousMisspelling,
    Bookmark,
    NextBookmark,
    PreviousBookmark,
    Bookmarks,
    Follow,
    Gpu,
    Settings,
    Appearance,
    ChooseFont,
    ToolSettings,
    LanguageServers,
    LanguageHover,
    LanguageFormat,
    LanguageRename,
    LanguageQuickFix,
    LanguageProblems,
    NextProblem,
    PreviousProblem,
    LanguageRestart,
    Checkpoint,
    UserGuide,
    About,
    Updates,
}
impl Command {
    fn requires_local(self) -> bool {
        matches!(
            self,
            Self::OpenProject
                | Self::SaveProject
                | Self::SaveProjectAs
                | Self::RecentProjects
                | Self::SaveMacro
                | Self::MarkdownSplit
                | Self::MarkdownPreview
                | Self::MarkdownRefresh
                | Self::SwitchCounterpart
                | Self::OpenFileReference
                | Self::CompareDisk
                | Self::GitDiff
                | Self::Gpu
                | Self::ToolSettings
        )
    }
    pub(super) const ALL: &'static [Self] = &[
        Self::Notifications,
        Self::FocusExplorer,
        Self::FocusEditor,
        Self::FocusSearch,
        Self::FocusTerminal,
        Self::TogglePanel,
        Self::MaximizePanel,
        Self::ExplorerNewFile,
        Self::ExplorerNewFolder,
        Self::ExplorerRename,
        Self::Palette,
        Self::ConnectSsh,
        Self::SshOutput,
        Self::MarkdownSource,
        Self::MarkdownSplit,
        Self::MarkdownPreview,
        Self::MarkdownRefresh,
        Self::FindFiles,
        Self::FindPrevious,
        Self::SelectAllMatches,
        Self::UseSelectionForFind,
        Self::UseSelectionForReplace,
        Self::QuickFindNext,
        Self::QuickFindPrevious,
        Self::ReplaceAll,
        Self::OpenProject,
        Self::SaveProject,
        Self::SaveProjectAs,
        Self::RecentProjects,
        Self::ProjectSettings,
        Self::Symbols,
        Self::GotoDefinition,
        Self::GotoReferences,
        Self::SwitchCounterpart,
        Self::OpenFileReference,
        Self::NavBack,
        Self::NavForward,
        Self::CompareDisk,
        Self::GitDiff,
        Self::SourceControl,
        Self::GitCommit,
        Self::GitNextFile,
        Self::GitPreviousFile,
        Self::NextChange,
        Self::PreviousChange,
        Self::RevertChange,
        Self::KeyBindings,
        Self::Encoding,
        Self::Complete,
        Self::Snippets,
        Self::RecordMacro,
        Self::PlayMacro,
        Self::SaveMacro,
        Self::RunMacro,
        Self::PinTab,
        Self::CloseOthers,
        Self::CloseRight,
        Self::OpenRecent,
        Self::CloseAll,
        Self::CloseUnmodified,
        Self::SelectBookmarks,
        Self::ClearBookmarks,
        Self::ScrollLineUp,
        Self::ScrollLineDown,
        Self::CopyPath,
        Self::CopyFileLocation,
        Self::RevealInSidebar,
        Self::NextTab,
        Self::PreviousTab,
        Self::RecentTab,
        Self::RecentTabBack,
        Self::Tab1,
        Self::Tab2,
        Self::Tab3,
        Self::Tab4,
        Self::Tab5,
        Self::Tab6,
        Self::Tab7,
        Self::Tab8,
        Self::Tab9,
        Self::NextOccurrence,
        Self::Build,
        Self::BuildWith,
        Self::RunProjectTask,
        Self::RerunProjectTask,
        Self::CancelBuild,
        Self::BuildResults,
        Self::Terminal,
        Self::TerminalSettings,
        Self::NewTerminal,
        Self::CloseTerminal,
        Self::NextTerminal,
        Self::PreviousTerminal,
        Self::RestartTerminal,
        Self::NextResult,
        Self::PreviousResult,
        Self::SkipOccurrence,
        Self::ExpandSelection,
        Self::ExpandToBrackets,
        Self::ExpandToIndentation,
        Self::ExpandToTag,
        Self::JumpToBracket,
        Self::SplitLines,
        Self::AddCursorAbove,
        Self::AddCursorBelow,
        Self::SoftUndo,
        Self::SoftRedo,
        Self::AllOccurrences,
        Self::SingleSelection,
        Self::Rectangle,
        Self::SplitColumns,
        Self::SplitRows,
        Self::SinglePane,
        Self::Columns3,
        Self::Columns4,
        Self::Rows3,
        Self::Grid,
        Self::FocusGroup1,
        Self::FocusGroup2,
        Self::FocusGroup3,
        Self::FocusGroup4,
        Self::FocusPane,
        Self::MovePane,
        Self::AutoIndent,
        Self::SmartIndent,
        Self::SoftTabs,
        Self::DetectIndent,
        Self::Fold,
        Self::FoldAll,
        Self::FoldLevel1,
        Self::FoldLevel2,
        Self::FoldLevel3,
        Self::Unfold,
        Self::UnfoldAll,
        Self::IndentGuides,
        Self::Minimap,
        Self::New,
        Self::NewFileFromTemplate,
        Self::NewWindow,
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
        Self::PasteHistory,
        Self::PasteIndent,
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
        Self::ToggleBlockComment,
        Self::WrapLines,
        Self::Transpose,
        Self::TitleCase,
        Self::SwapCase,
        Self::DeleteToLineStart,
        Self::DeleteToLineEnd,
        Self::Reindent,
        Self::SortLinesIgnoreCase,
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
        Self::Breadcrumbs,
        Self::Outline,
        Self::FolderSidebar,
        Self::Wrap,
        Self::LineNumbers,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::ResetZoom,
        Self::ToggleSpellCheck,
        Self::NextMisspelling,
        Self::PreviousMisspelling,
        Self::Bookmark,
        Self::NextBookmark,
        Self::PreviousBookmark,
        Self::Bookmarks,
        Self::Follow,
        Self::Gpu,
        Self::Settings,
        Self::Appearance,
        Self::ChooseFont,
        Self::ToolSettings,
        Self::LanguageServers,
        Self::LanguageHover,
        Self::LanguageFormat,
        Self::LanguageRename,
        Self::LanguageQuickFix,
        Self::LanguageProblems,
        Self::NextProblem,
        Self::PreviousProblem,
        Self::LanguageRestart,
        Self::Checkpoint,
        Self::UserGuide,
        Self::About,
        Self::Updates,
        Self::Quit,
    ];
    /// Commands that show a window or the Find bar; any other window closes first.
    fn opens_window(self) -> bool {
        matches!(
            self,
            Self::ConnectSsh
                | Self::Switcher
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
                | Self::LanguageServers
                | Self::LanguageHover
                | Self::LanguageRename
                | Self::LanguageQuickFix
                | Self::TerminalSettings
                | Self::KeyBindings
                | Self::ProjectSettings
                | Self::UserGuide
                | Self::About
                | Self::Updates
                | Self::FindFiles
                | Self::Symbols
                | Self::CompareDisk
                | Self::GitDiff
                | Self::Encoding
                | Self::Snippets
                | Self::RecentProjects
                | Self::PasteHistory
                | Self::OpenRecent
                | Self::ChooseFont
                | Self::SaveMacro
                | Self::RunMacro
        )
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::ConnectSsh => "Connect via SSH…",
            Self::SshOutput => "Show SSH Output",
            Self::MarkdownSource => "Markdown: show source",
            Self::MarkdownSplit => "Markdown: split source and preview",
            Self::MarkdownPreview => "Markdown: render preview",
            Self::MarkdownRefresh => "Markdown: refresh preview",
            Self::FindFiles => "Find in folders…",
            Self::FindPrevious => "Find previous",
            Self::SelectAllMatches => "Select all matches",
            Self::UseSelectionForFind => "Use selection for Find",
            Self::UseSelectionForReplace => "Use selection for Replace",
            Self::QuickFindNext => "Quick find next",
            Self::QuickFindPrevious => "Quick find previous",
            Self::ReplaceAll => "Replace all in file",
            Self::OpenProject => "Open project…",
            Self::SaveProject => "Save project",
            Self::SaveProjectAs => "Save project as…",
            Self::RecentProjects => "Recent projects…",
            Self::ProjectSettings => "Project settings…",
            Self::Symbols => "Go to symbol…",
            Self::GotoDefinition => "Go to definition",
            Self::GotoReferences => "Go to references…",
            Self::SwitchCounterpart => "Switch header/implementation",
            Self::OpenFileReference => "Open file under cursor",
            Self::NavBack => "Navigate back",
            Self::NavForward => "Navigate forward",
            Self::CompareDisk => "Compare with disk…",
            Self::GitDiff => "Compare file with Git HEAD…",
            Self::FocusEditor => "Focus editor",
            Self::FocusSearch => "Focus Search",
            Self::FocusTerminal => "Focus terminal",
            Self::TogglePanel => "Toggle last used panel",
            Self::MaximizePanel => "Maximize or restore panel",
            Self::Notifications => "Toggle notification history",
            Self::FocusExplorer => "Focus Explorer",
            Self::ExplorerNewFile => "Explorer: new file…",
            Self::ExplorerNewFolder => "Explorer: new folder…",
            Self::ExplorerRename => "Explorer: rename…",
            Self::SourceControl => "Source Control",
            Self::GitCommit => "Git: Commit staged changes",
            Self::GitNextFile => "Git: Next file",
            Self::GitPreviousFile => "Git: Previous file",
            Self::NextChange => "Next change",
            Self::PreviousChange => "Previous change",
            Self::RevertChange => "Revert change",
            Self::KeyBindings => "Key bindings and snippets…",
            Self::Encoding => "File encoding…",
            Self::Complete => "Complete word…",
            Self::Snippets => "Insert snippet…",
            Self::RecordMacro => "Record macro",
            Self::PlayMacro => "Play macro",
            Self::SaveMacro => "Save macro…",
            Self::RunMacro => "Run saved macro…",
            Self::PinTab => "Pin tab",
            Self::CloseOthers => "Close other file tabs in group",
            Self::CloseRight => "Close tabs to the right",
            Self::OpenRecent => "Open recent file…",
            Self::CloseAll => "Close all file tabs in group",
            Self::CloseUnmodified => "Close unmodified tabs",
            Self::SelectBookmarks => "Select all bookmarks",
            Self::ClearBookmarks => "Clear bookmarks",
            Self::ScrollLineUp => "Scroll up one line",
            Self::ScrollLineDown => "Scroll down one line",
            Self::CopyPath => "Copy file path",
            Self::CopyFileLocation => "Copy File Location",
            Self::RevealInSidebar => "Reveal in sidebar",
            Self::NextTab => "Next tab",
            Self::PreviousTab => "Previous tab",
            Self::RecentTab => "Next recently used tab",
            Self::RecentTabBack => "Previous recently used tab",
            Self::Tab1 => "Go to tab 1",
            Self::Tab2 => "Go to tab 2",
            Self::Tab3 => "Go to tab 3",
            Self::Tab4 => "Go to tab 4",
            Self::Tab5 => "Go to tab 5",
            Self::Tab6 => "Go to tab 6",
            Self::Tab7 => "Go to tab 7",
            Self::Tab8 => "Go to tab 8",
            Self::Tab9 => "Go to tab 9",

            Self::NextOccurrence => "Select next occurrence",
            Self::Build => "Build",
            Self::BuildWith => "Build with…",
            Self::RunProjectTask => "Run Project Task…",
            Self::RerunProjectTask => "Rerun Last Project Task",
            Self::CancelBuild => "Cancel build",
            Self::BuildResults => "Toggle Output",
            Self::Terminal => "Toggle terminal",
            Self::TerminalSettings => "Terminal settings…",
            Self::NewTerminal => "Terminal: new tab",
            Self::CloseTerminal => "Terminal: close tab",
            Self::NextTerminal => "Terminal: next tab",
            Self::PreviousTerminal => "Terminal: previous tab",
            Self::RestartTerminal => "Terminal: restart shell",
            Self::NextResult => "Next result",
            Self::PreviousResult => "Previous result",
            Self::SkipOccurrence => "Skip occurrence",
            Self::ExpandSelection => "Expand selection",
            Self::ExpandToBrackets => "Expand selection to brackets",
            Self::ExpandToIndentation => "Expand selection to indentation",
            Self::ExpandToTag => "Expand selection to HTML/XML tag",
            Self::JumpToBracket => "Jump to matching bracket",
            Self::SplitLines => "Split selection into lines",
            Self::AddCursorAbove => "Add cursor above",
            Self::AddCursorBelow => "Add cursor below",
            Self::SoftUndo => "Undo selection change",
            Self::SoftRedo => "Redo selection change",
            Self::AllOccurrences => "Select all occurrences",
            Self::SingleSelection => "Single selection",
            Self::Rectangle => "Rectangular selection",
            Self::SplitColumns => "Layout: two columns",
            Self::Columns3 => "Layout: three columns",
            Self::Columns4 => "Layout: four columns",
            Self::Rows3 => "Layout: three rows",
            Self::Grid => "Layout: grid of four",
            Self::FocusGroup1 => "Focus group 1",
            Self::FocusGroup2 => "Focus group 2",
            Self::FocusGroup3 => "Focus group 3",
            Self::FocusGroup4 => "Focus group 4",
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
            Self::FoldLevel1 => "Fold level 1",
            Self::FoldLevel2 => "Fold level 2",
            Self::FoldLevel3 => "Fold level 3",
            Self::Unfold => "Unfold block",
            Self::UnfoldAll => "Unfold all",
            Self::IndentGuides => "Show indentation guides",
            Self::Minimap => "Show minimap",
            Self::New => "New file",
            Self::NewFileFromTemplate => "New File from Template…",
            Self::NewWindow => "New window",
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
            Self::PasteHistory => "Paste from history…",
            Self::PasteIndent => "Paste and indent",
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
            Self::ToggleBlockComment => "Toggle block comment",
            Self::WrapLines => "Wrap paragraph at ruler",
            Self::Transpose => "Transpose",
            Self::TitleCase => "Title case",
            Self::SwapCase => "Swap case",
            Self::DeleteToLineStart => "Delete to start of line",
            Self::DeleteToLineEnd => "Delete to end of line",
            Self::Reindent => "Reindent lines",
            Self::SortLinesIgnoreCase => "Sort lines, ignoring case",
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
            Self::Breadcrumbs => "Show breadcrumbs",
            Self::Outline => "Show Outline",
            Self::FolderSidebar => "Show folders in the sidebar",
            Self::Wrap => "Word wrap",
            Self::LineNumbers => "Show line numbers",
            Self::ZoomIn => "Increase font size",
            Self::ZoomOut => "Decrease font size",
            Self::ResetZoom => "Reset font size",
            Self::ToggleSpellCheck => "Spell check",
            Self::NextMisspelling => "Next misspelling",
            Self::PreviousMisspelling => "Previous misspelling",
            Self::Bookmark => "Toggle bookmark",
            Self::NextBookmark => "Next bookmark",
            Self::PreviousBookmark => "Previous bookmark",
            Self::Bookmarks => "Bookmarks…",
            Self::Follow => "Follow log (read only)",
            Self::Gpu => "CUDA panel",
            Self::Settings => "Settings…",
            Self::Appearance => "Color theme…",
            Self::ChooseFont => "Editor font…",
            Self::ToolSettings => "CUDA tool settings…",
            Self::LanguageServers => "Language servers: manage…",
            Self::LanguageHover => "Language server: show hover information",
            Self::LanguageFormat => "Language server: format document",
            Self::LanguageRename => "Language server: rename symbol…",
            Self::LanguageQuickFix => "Language server: quick fixes…",
            Self::LanguageProblems => "Toggle Problems",
            Self::NextProblem => "Next diagnostic",
            Self::PreviousProblem => "Previous diagnostic",
            Self::LanguageRestart => "Language servers: restart",
            Self::Checkpoint => "Checkpoint now",
            Self::UserGuide => "User guide",
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
            Self::Terminal if cfg!(target_os = "macos") => "Control+`",
            Self::Terminal => "Ctrl+`",
            Self::NewTerminal => "Ctrl+Shift+T",
            Self::CloseTerminal => "Ctrl+Shift+W",
            Self::NextTerminal if cfg!(target_os = "macos") => "Control+Page Down",
            Self::PreviousTerminal if cfg!(target_os = "macos") => "Control+Page Up",
            Self::NextTerminal => "Ctrl+Page Down",
            Self::PreviousTerminal => "Ctrl+Page Up",
            Self::SourceControl => "Ctrl+Shift+G",
            Self::GitCommit => "Ctrl+Enter",
            Self::GitNextFile => "Alt+Down",
            Self::GitPreviousFile => "Alt+Up",
            Self::UserGuide => "F1",
            Self::FindFiles => "Ctrl+Shift+F",
            Self::CloseAll => "Ctrl+K, Ctrl+W",
            Self::SelectBookmarks => "Alt+F2",
            Self::ClearBookmarks => "Ctrl+Shift+F2",
            Self::ScrollLineUp if cfg!(target_os = "macos") => "Control+Alt+Up",
            Self::ScrollLineDown if cfg!(target_os = "macos") => "Control+Alt+Down",
            Self::ScrollLineUp => "Ctrl+Up",
            Self::ScrollLineDown => "Ctrl+Down",
            Self::RecordMacro if cfg!(target_os = "macos") => "Control+Q",
            Self::PlayMacro if cfg!(target_os = "macos") => "Control+Shift+Q",
            Self::RecordMacro => "Ctrl+Alt+Q",
            Self::PlayMacro => "Ctrl+Alt+Shift+Q",
            Self::NextChange => "Ctrl+K, Ctrl+.",
            Self::PreviousChange => "Ctrl+K, Ctrl+,",
            Self::RevertChange => "Ctrl+K, Ctrl+Z",
            // Control+Tab everywhere: Cmd+Tab switches applications on macOS.
            Self::RecentTab if cfg!(target_os = "macos") => "Control+Tab",
            Self::RecentTabBack if cfg!(target_os = "macos") => "Control+Shift+Tab",
            Self::NextTab if cfg!(target_os = "macos") => "Control+PageDown",
            Self::PreviousTab if cfg!(target_os = "macos") => "Control+PageUp",
            Self::RecentTab => "Ctrl+Tab",
            Self::RecentTabBack => "Ctrl+Shift+Tab",
            Self::NextTab => "Ctrl+PageDown",
            Self::PreviousTab => "Ctrl+PageUp",
            Self::Tab1 if cfg!(target_os = "macos") => "Ctrl+1",
            Self::Tab2 if cfg!(target_os = "macos") => "Ctrl+2",
            Self::Tab3 if cfg!(target_os = "macos") => "Ctrl+3",
            Self::Tab4 if cfg!(target_os = "macos") => "Ctrl+4",
            Self::Tab5 if cfg!(target_os = "macos") => "Ctrl+5",
            Self::Tab6 if cfg!(target_os = "macos") => "Ctrl+6",
            Self::Tab7 if cfg!(target_os = "macos") => "Ctrl+7",
            Self::Tab8 if cfg!(target_os = "macos") => "Ctrl+8",
            Self::Tab9 if cfg!(target_os = "macos") => "Ctrl+9",
            Self::Tab1 => "Alt+1",
            Self::Tab2 => "Alt+2",
            Self::Tab3 => "Alt+3",
            Self::Tab4 => "Alt+4",
            Self::Tab5 => "Alt+5",
            Self::Tab6 => "Alt+6",
            Self::Tab7 => "Alt+7",
            Self::Tab8 => "Alt+8",
            Self::Tab9 => "Alt+9",
            Self::FindPrevious => "Shift+F3",
            Self::SelectAllMatches => "Alt+Enter",
            Self::UseSelectionForFind => "Ctrl+E",
            Self::UseSelectionForReplace => "Ctrl+Shift+E",
            Self::QuickFindNext => "Ctrl+F3",
            Self::QuickFindPrevious => "Ctrl+Shift+F3",
            Self::Symbols => "Ctrl+R",
            Self::GotoDefinition => "F12",
            Self::GotoReferences => "Shift+F12",
            Self::SwitchCounterpart => "Alt+O",
            Self::OpenFileReference => "Alt+D",
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
            Self::RerunProjectTask => "Ctrl+Alt+B",
            Self::NextResult => "F4",
            Self::PreviousResult => "Shift+F4",
            Self::SkipOccurrence => "Ctrl+K, Ctrl+D",
            Self::Uppercase => "Ctrl+K, Ctrl+U",
            Self::Lowercase => "Ctrl+K, Ctrl+L",
            Self::FoldLevel1 => "Ctrl+K, Ctrl+1",
            Self::FoldLevel2 => "Ctrl+K, Ctrl+2",
            Self::FoldLevel3 => "Ctrl+K, Ctrl+3",
            Self::UnfoldAll => "Ctrl+K, Ctrl+J",
            // Sublime: Alt+F3 on Linux, Control+Cmd+G on macOS.
            Self::AllOccurrences if cfg!(target_os = "macos") => "Control+Ctrl+G",
            Self::AllOccurrences => "Alt+F3",
            Self::SplitLines => "Ctrl+Shift+L",
            Self::AddCursorAbove if cfg!(target_os = "macos") => "Control+Shift+Up",
            Self::AddCursorBelow if cfg!(target_os = "macos") => "Control+Shift+Down",
            Self::AddCursorAbove => "Alt+Shift+Up",
            Self::AddCursorBelow => "Alt+Shift+Down",
            Self::ExpandSelection => "Ctrl+Shift+Space",
            Self::ExpandToBrackets => "Ctrl+Shift+M",
            Self::ExpandToIndentation => "Ctrl+Shift+J",
            Self::ExpandToTag => "Ctrl+Shift+A",
            Self::JumpToBracket if cfg!(target_os = "macos") => "Control+M",
            Self::JumpToBracket => "Ctrl+M",
            Self::SoftUndo => "Ctrl+U",
            Self::SoftRedo => "Ctrl+Shift+U",
            Self::FocusPane => "Ctrl+Alt+Right",
            // Sublime: Alt+Shift+N on Linux, Cmd+Option+N on macOS.
            Self::SinglePane if cfg!(target_os = "macos") => "Ctrl+Alt+1",
            Self::SplitColumns if cfg!(target_os = "macos") => "Ctrl+Alt+2",
            Self::Columns3 if cfg!(target_os = "macos") => "Ctrl+Alt+3",
            Self::Columns4 if cfg!(target_os = "macos") => "Ctrl+Alt+4",
            Self::Grid if cfg!(target_os = "macos") => "Ctrl+Alt+5",
            Self::SplitRows if cfg!(target_os = "macos") => "Ctrl+Alt+Shift+2",
            Self::Rows3 if cfg!(target_os = "macos") => "Ctrl+Alt+Shift+3",
            Self::SinglePane => "Alt+Shift+1",
            Self::SplitColumns => "Alt+Shift+2",
            Self::Columns3 => "Alt+Shift+3",
            Self::Columns4 => "Alt+Shift+4",
            Self::Grid => "Alt+Shift+5",
            Self::SplitRows => "Alt+Shift+8",
            Self::Rows3 => "Alt+Shift+9",
            Self::FocusGroup1 if cfg!(target_os = "macos") => "Control+1",
            Self::FocusGroup2 if cfg!(target_os = "macos") => "Control+2",
            Self::FocusGroup3 if cfg!(target_os = "macos") => "Control+3",
            Self::FocusGroup4 if cfg!(target_os = "macos") => "Control+4",
            Self::FocusGroup1 => "Ctrl+1",
            Self::FocusGroup2 => "Ctrl+2",
            Self::FocusGroup3 => "Ctrl+3",
            Self::FocusGroup4 => "Ctrl+4",
            Self::MovePane => "Ctrl+Alt+Shift+Right",
            Self::Fold => "Ctrl+Shift+[",
            Self::Unfold => "Ctrl+Shift+]",
            Self::ConnectSsh => "",
            Self::New => "Ctrl+N",
            Self::NewWindow => "Ctrl+Shift+N",
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
            Self::PasteHistory => "Ctrl+K, Ctrl+V",
            Self::PasteIndent => "Ctrl+Shift+V",
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
            Self::ToggleBlockComment if cfg!(target_os = "macos") => "Ctrl+Alt+/",
            Self::ToggleBlockComment => "Ctrl+Shift+/",
            Self::WrapLines if cfg!(target_os = "macos") => "Ctrl+Alt+Q",
            Self::WrapLines => "Alt+Q",
            Self::Transpose if cfg!(target_os = "macos") => "Control+T",
            Self::Transpose => "Ctrl+T",
            Self::DeleteToLineStart if cfg!(target_os = "macos") => "Ctrl+Backspace",
            Self::DeleteToLineStart => "Ctrl+Shift+Backspace",
            Self::DeleteToLineEnd => "Ctrl+K, Ctrl+K",
            Self::SortLines => "Ctrl+F9",
            Self::SortLinesIgnoreCase => "F9",
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
            Self::ToggleSpellCheck => "F6",
            Self::NextMisspelling => "Ctrl+F6",
            Self::PreviousMisspelling => "Ctrl+Shift+F6",
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
            Self::SortLinesIgnoreCase => Operation::SortLinesIgnoreCase,
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
/// A label with the letters matching `filter` drawn in `color`. Text that is
/// reordered for right-to-left display is shown without highlights.
fn highlighted(ui: &egui::Ui, label: &str, filter: &str, color: egui::Color32) -> egui::WidgetText {
    let shown = hedit::bidi::display(label);
    let positions = match hedit::folder::fuzzy_match(label, filter) {
        Some((_, positions)) if !positions.is_empty() && shown == label => positions,
        _ => return shown.into(),
    };
    let font = egui::TextStyle::Button.resolve(ui.style());
    let normal = ui.visuals().text_color();
    let mut job = egui::text::LayoutJob::default();
    let mut append = |run: &str, matched: bool| {
        job.append(
            run,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color: if matched { color } else { normal },
                underline: if matched {
                    egui::Stroke::new(1.0_f32, color)
                } else {
                    egui::Stroke::NONE
                },
                ..Default::default()
            },
        );
    };
    // Runs of matched and unmatched letters.
    let mut run = String::new();
    let mut in_match = false;
    for (index, c) in label.chars().enumerate() {
        let matched = positions.contains(&index);
        if matched != in_match && !run.is_empty() {
            append(&run, in_match);
            run.clear();
        }
        in_match = matched;
        run.push(c);
    }
    append(&run, in_match);
    job.into()
}

/// What choosing an entry of a pick list does.
#[derive(Clone, Debug)]
pub(super) enum Pick {
    Paste(String),
    /// A file to open.
    File(PathBuf),
    ExistingFile(PathBuf),
    /// An editor font family; empty for the built-in font.
    Font(String),
    /// A saved macro, by name.
    Macro(String),
    ProjectTask(String),
    /// An extension command's stable ID.
    Extension(String),
    Location(definitions::Location),
}

/// A list shown in the palette in place of files or commands.
pub(super) struct PickList {
    pub title: String,
    pub hint: &'static str,
    pub items: Vec<(String, String, Pick)>,
}

impl HeditApp {
    /// Show a list in the palette to choose from.
    pub(super) fn open_pick_list(
        &mut self,
        ctx: &egui::Context,
        title: impl Into<String>,
        hint: &'static str,
        items: Vec<(String, String, Pick)>,
    ) {
        self.cancel_theme_palette(ctx);
        self.close_transient(ctx);
        self.palette = true;
        self.pick_list = Some(PickList {
            title: title.into(),
            hint,
            items,
        });
        self.commands_only = false;
        self.palette_query.clear();
        self.palette_focus = true;
        self.palette_index = 0;
    }
    pub(super) fn command_enabled(&self, command: Command) -> bool {
        if self.busy || self.closing {
            return false;
        }
        if self.preferences_open && matches!(command, Command::Save | Command::Close) {
            return true;
        }
        if self.workspace.remote.is_some() && command.requires_local() {
            return false;
        }
        if self.preferences_open
            && self.settings_scope.scope == settings_scope::Scope::Project
            && self.quality.project_editor.dirty
            && matches!(command, Command::SaveProject | Command::SaveProjectAs)
        {
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
            Command::RunProjectTask => {
                self.workspace.remote.is_none()
                    && self.quality.project_path.is_some()
                    && !self.quality.project_tasks.is_empty()
            }
            Command::RerunProjectTask => {
                self.workspace.remote.is_none() && self.quality.last_project_task.is_some()
            }
            Command::ExplorerNewFile | Command::ExplorerNewFolder | Command::ExplorerRename => {
                !self.workspace.folders.is_empty()
            }
            Command::GitCommit => self.git_commit_context() && self.source_control.can_commit(),
            Command::GitNextFile => {
                self.source_control.open && self.source_control.can_move_review(true)
            }
            Command::GitPreviousFile => {
                self.source_control.open && self.source_control.can_move_review(false)
            }
            Command::ExpandToTag => {
                !self.active().page.info.paged
                    && matches!(
                        self.active().language(),
                        syntax::Language::Html | syntax::Language::Xml
                    )
            }
            Command::MarkdownSource => self.active().language() == syntax::Language::Markdown,
            Command::MarkdownSplit | Command::MarkdownPreview => self.markdown_available(),
            Command::MarkdownRefresh => {
                self.markdown_available()
                    && self.active().view.markdown_view != hedit::session::MarkdownView::Source
            }
            Command::Close => {
                self.source_control.open || self.can_close_group_tab(self.workspace.active)
            }
            Command::NextOccurrence
            | Command::AllOccurrences
            | Command::SkipOccurrence
            | Command::ExpandSelection
            | Command::ExpandToBrackets
            | Command::ExpandToIndentation
            | Command::JumpToBracket
            | Command::SplitLines
            | Command::AddCursorAbove
            | Command::AddCursorBelow
            | Command::Rectangle
            | Command::Fold
            | Command::FoldAll
            | Command::FoldLevel1
            | Command::FoldLevel2
            | Command::FoldLevel3 => !self.active().page.info.paged,
            Command::NextTab
            | Command::PreviousTab
            | Command::RecentTab
            | Command::RecentTabBack => self.group_tab_count() > 1,
            Command::FocusPane | Command::MovePane => self.panes.mode != 0,
            Command::FocusGroup1 => true,
            Command::FocusGroup2 => self.panes.groups() > 1,
            Command::FocusGroup3 => self.panes.groups() > 2,
            Command::FocusGroup4 => self.panes.groups() > 3,
            Command::CancelBuild => self.build.running(),
            Command::Terminal | Command::FocusTerminal => {
                cfg!(feature = "terminal") && self.settings.terminal_enabled
            }
            Command::NewTerminal => cfg!(feature = "terminal") && self.settings.terminal_enabled,
            Command::CloseTerminal
            | Command::RestartTerminal
            | Command::NextTerminal
            | Command::PreviousTerminal => {
                #[cfg(feature = "terminal")]
                {
                    self.settings.terminal_enabled && !self.terminal.tabs.is_empty()
                }
                #[cfg(not(feature = "terminal"))]
                {
                    false
                }
            }
            Command::NextResult | Command::PreviousResult => {
                !self.build.locations.is_empty()
                    || (self.quality.results_newer && !self.quality.results.hits.is_empty())
            }
            Command::Gpu | Command::ToolSettings => hedit::debugger::LOCAL_TOOLS_SUPPORTED,
            Command::Uppercase | Command::Lowercase => {
                self.active().view.anchor != self.active().view.head
            }
            Command::ToggleComment => {
                hedit::editing::comment_prefix(self.active().language().id()).is_some()
            }
            Command::ToggleBlockComment
            | Command::WrapLines
            | Command::Transpose
            | Command::TitleCase
            | Command::SwapCase
            | Command::DeleteToLineStart
            | Command::DeleteToLineEnd
            | Command::Reindent
                if self.active().page.info.paged || self.active().follow =>
            {
                false
            }
            Command::ToggleBlockComment => {
                let language = self.active().language().id();
                hedit::text_commands::block_comment(language).is_some()
                    || hedit::editing::comment_prefix(language).is_some()
            }
            Command::Reindent => hedit::text_commands::reindents(self.active().language().id()),
            Command::CloseFolder | Command::RefreshFolder => !self.workspace.folders.is_empty(),
            Command::Rename => self.active().page.info.path.is_none(),
            Command::CopyPath | Command::RevealInSidebar => self.active().page.info.path.is_some(),
            Command::CopyFileLocation => self.active().page.info.path.is_some(),
            Command::SwitchCounterpart => self
                .active()
                .page
                .info
                .path
                .as_ref()
                .is_some_and(|p| hedit::counterpart::supported(std::path::Path::new(p))),
            Command::ConnectSsh => true,
            Command::OpenRecent => !self.recent_files.is_empty(),
            Command::SelectBookmarks => {
                let buffer = self.active();
                buffer
                    .view
                    .bookmarks
                    .iter()
                    .any(|mark| buffer.bookmark_on_page(mark.offset).is_some())
            }
            Command::ClearBookmarks => !self.active().view.bookmarks.is_empty(),
            Command::PlayMacro | Command::SaveMacro => {
                !self.macros.last.is_empty() && self.macros.recording.is_none()
            }
            Command::RunMacro => {
                !self.quality.config.macros.is_empty() && self.macros.recording.is_none()
            }
            Command::NextChange | Command::PreviousChange | Command::RevertChange => {
                !self.active().page.info.paged
                    && !self.active().follow
                    && self.quality.git_base.contains_key(&self.active().view.id)
            }
            Command::Reopen => self
                .workspace
                .closed
                .iter()
                .any(|v| !self.opens.reopening(v.id)),
            Command::Follow => {
                self.active().page.info.path.is_some() && !self.active().page.info.dirty
            }
            Command::NextMisspelling | Command::PreviousMisspelling => self.settings.spell_check,
            Command::PasteHistory => {
                !self.paste_history.is_empty()
                    && !self.active().follow
                    && !self.active().page.info.paged
            }
            Command::Undo
            | Command::Redo
            | Command::Cut
            | Command::Paste
            | Command::PasteIndent
            | Command::Replace => !self.active().follow,
            _ => true,
        }
    }
    pub(super) fn command_selected(&self, command: Command) -> bool {
        match command {
            Command::SourceControl => {
                self.sidebar == workbench_navigation::Sidebar::SourceControl
                    && self.settings.show_sidebar
            }
            Command::MarkdownSource => {
                self.active().view.markdown_view == hedit::session::MarkdownView::Source
            }
            Command::MarkdownSplit => {
                self.active().view.markdown_view == hedit::session::MarkdownView::Split
            }
            Command::MarkdownPreview => {
                self.active().view.markdown_view == hedit::session::MarkdownView::Preview
            }
            Command::PinTab => self.active().view.pinned,
            Command::AutoIndent => self.settings.auto_indent,
            Command::SmartIndent => self.settings.smart_indent,
            Command::SoftTabs => self.settings.smart_backspace,
            Command::IndentGuides => self.settings.draw_indent_guides,
            Command::Minimap => self.settings.show_minimap,
            Command::SinglePane => self.panes.mode == 0,
            Command::SplitColumns => self.panes.mode == 1,
            Command::SplitRows => self.panes.mode == 2,
            Command::Columns3 => self.panes.mode == 3,
            Command::Columns4 => self.panes.mode == 4,
            Command::Rows3 => self.panes.mode == 5,
            Command::Grid => self.panes.mode == 6,
            Command::Breadcrumbs => self.settings.show_breadcrumbs && !self.distraction_free,
            Command::Outline => self.settings.show_outline && !self.distraction_free,
            Command::Sidebar => self.settings.show_sidebar && !self.distraction_free,
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
            Command::ToggleSpellCheck => self.settings.spell_check,
            Command::RecordMacro => self.macros.recording.is_some(),
            Command::LineNumbers => self.settings.line_numbers,
            Command::Follow => self.active().follow,
            Command::Gpu => self.gpu_open,
            Command::BuildResults => self.build.open,
            Command::Terminal => self.terminal.open,
            _ => false,
        }
    }
    pub(super) fn execute(&mut self, ctx: &egui::Context, command: Command) {
        if self.preferences_open {
            if command == Command::Save {
                self.save_scope_settings();
                return;
            }
            if command == Command::Close {
                self.preferences_open = false;
                self.focus_editor = true;
                return;
            }
            if self.settings_scope.scope == settings_scope::Scope::Project
                && self.quality.project_editor.dirty
                && matches!(command, Command::SaveProject | Command::SaveProjectAs)
            {
                self.quality.project_editor.error =
                    Some("Save the settings draft before saving the project file.".into());
                return;
            }
        }
        if self.workspace.remote.is_some() {
            match command {
                Command::New => {
                    self.remote_new(ctx);
                    return;
                }
                Command::OpenFolder => {
                    self.open_folder(ctx);
                    return;
                }
                command if command.requires_local() => {
                    self.error = Some("This tool is not available in SSH workspaces yet.".into());
                    return;
                }
                _ => (),
            }
        }
        if !self.command_enabled(command) {
            return;
        }
        if self.source_control.open {
            if command == Command::Close {
                self.source_control.review_tab = false;
                self.leave_review();
                self.focus_editor = true;
                return;
            }
            if !matches!(
                command,
                Command::SourceControl
                    | Command::Notifications
                    | Command::FocusExplorer
                    | Command::FocusEditor
                    | Command::FocusSearch
                    | Command::FocusTerminal
                    | Command::TogglePanel
                    | Command::MaximizePanel
                    | Command::FocusGroup1
                    | Command::FocusGroup2
                    | Command::FocusGroup3
                    | Command::FocusGroup4
                    | Command::FocusPane
                    | Command::SplitColumns
                    | Command::SplitRows
                    | Command::Columns3
                    | Command::Columns4
                    | Command::Rows3
                    | Command::Grid
                    | Command::SinglePane
                    | Command::BuildResults
                    | Command::SshOutput
                    | Command::LanguageProblems
                    | Command::Terminal
                    | Command::NewTerminal
                    | Command::CloseTerminal
                    | Command::NextTerminal
                    | Command::PreviousTerminal
                    | Command::FindFiles
                    | Command::GitCommit
                    | Command::GitNextFile
                    | Command::GitPreviousFile
                    | Command::Palette
                    | Command::UserGuide
                    | Command::About
                    | Command::Settings
                    | Command::KeyBindings
                    | Command::Appearance
                    | Command::ChooseFont
                    | Command::ZoomIn
                    | Command::ZoomOut
                    | Command::ResetZoom
                    | Command::LineNumbers
                    | Command::Sidebar
                    | Command::Breadcrumbs
                    | Command::Outline
                    | Command::StatusBar
                    | Command::Fullscreen
                    | Command::DistractionFree
                    | Command::Quit
                    | Command::Updates
            ) {
                self.leave_review();
                self.focus_editor = true;
            }
        }
        if self.markdown_preview_only()
            && matches!(
                command,
                Command::Find
                    | Command::Replace
                    | Command::FindPrevious
                    | Command::Goto
                    | Command::GotoDefinition
                    | Command::GotoReferences
                    | Command::Symbols
                    | Command::NavBack
                    | Command::NavForward
            )
        {
            self.set_markdown_view(ctx, hedit::session::MarkdownView::Split);
        }
        if command.opens_window() {
            self.close_transient(ctx);
        }
        match command {
            Command::MarkdownSource => {
                self.set_markdown_view(ctx, hedit::session::MarkdownView::Source)
            }
            Command::MarkdownSplit => {
                self.set_markdown_view(ctx, hedit::session::MarkdownView::Split)
            }
            Command::MarkdownPreview => {
                self.set_markdown_view(ctx, hedit::session::MarkdownView::Preview)
            }
            Command::MarkdownRefresh => self.refresh_markdown(ctx),
            Command::FindFiles => {
                self.show_search_sidebar();
            }
            Command::FindPrevious => self.find_advanced(ctx, true),
            Command::SelectAllMatches => self.select_all_matches(),
            Command::UseSelectionForFind => self.use_selection(false),
            Command::UseSelectionForReplace => self.use_selection(true),
            Command::QuickFindNext => self.quick_find(true),
            Command::QuickFindPrevious => self.quick_find(false),
            Command::ReplaceAll => self.replace_advanced(true),
            Command::OpenProject => self.load_project(ctx, None),
            Command::SaveProject => self.save_project(ctx, false),
            Command::SaveProjectAs => self.save_project(ctx, true),
            Command::RecentProjects => self.quality.recent_open = true,
            Command::ProjectSettings => {
                self.open_preferences(PreferencesPage::Formatting);
                self.prepare_project_scope();
            }
            Command::Symbols => self.list_symbols(ctx),
            Command::GotoDefinition => self.look_up(ctx, false),
            Command::GotoReferences => self.look_up(ctx, true),
            Command::SwitchCounterpart => self.switch_counterpart(ctx),
            Command::OpenFileReference => self.open_file_reference(ctx),
            Command::NavBack => self.navigate(ctx, false),
            Command::NavForward => self.navigate(ctx, true),
            Command::CompareDisk => self.compare_disk(ctx, false),
            Command::GitDiff => self.compare_disk(ctx, true),
            Command::Notifications => self.toggle_notifications(),
            Command::FocusExplorer => self.focus_explorer(ctx),
            Command::FocusEditor => self.focus_editor_command(ctx),
            Command::FocusSearch => {
                self.terminal.release_input();
                self.show_search_sidebar();
            }
            Command::FocusTerminal => self.focus_terminal_command(ctx),
            Command::TogglePanel => self.toggle_last_panel(ctx),
            Command::MaximizePanel => self.maximize_panel(ctx),
            Command::ExplorerNewFile | Command::ExplorerNewFolder | Command::ExplorerRename => {
                self.explorer_command(ctx, command)
            }
            Command::SourceControl => self.show_git_sidebar(ctx),
            Command::GitCommit => self.git_submit_commit(ctx),
            Command::GitNextFile => self.source_control.move_review(ctx, true),
            Command::GitPreviousFile => self.source_control.move_review(ctx, false),
            Command::NextChange => self.next_change(ctx, true),
            Command::PreviousChange => self.next_change(ctx, false),
            Command::RevertChange => self.revert_change(),
            Command::KeyBindings => self.open_preferences(PreferencesPage::Keys),
            Command::Encoding => {
                self.quality.encoding = self.active().page.info.encoding.clone();
                self.quality.encoding_open = true;
            }
            Command::Complete => self.open_completion(ctx),
            Command::RecordMacro => self.toggle_recording(),
            Command::PlayMacro => self.play_macro(self.macros.last.clone()),
            Command::SaveMacro => self.open_macro_name(),
            Command::RunMacro => {
                let items = self
                    .quality
                    .config
                    .macros
                    .iter()
                    .map(|m| {
                        let detail = format!("{} steps", m.steps.len());
                        (m.name.clone(), detail, Pick::Macro(m.name.clone()))
                    })
                    .collect();
                self.open_pick_list(ctx, "Run saved macro", "Filter macros…", items);
            }
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
            Command::OpenRecent => self.open_recent(ctx),
            Command::CloseAll => self.close_tabs(false),
            Command::CloseUnmodified => self.close_tabs(true),
            Command::SelectBookmarks => self.select_bookmarks(),
            Command::ClearBookmarks => self.clear_bookmarks(),
            Command::ScrollLineUp => self.scroll_lines(ctx, -1.0),
            Command::ScrollLineDown => self.scroll_lines(ctx, 1.0),
            Command::CopyPath => {
                if let Some(path) = self.active().page.info.path.clone() {
                    ctx.copy_text(path);
                    self.notice = "File path copied.".into();
                }
            }
            Command::CopyFileLocation => self.copy_file_location(ctx),
            Command::RevealInSidebar => self.reveal_in_sidebar(),
            Command::NextTab => self.step_tab(true),
            Command::PreviousTab => self.step_tab(false),
            Command::RecentTab => self.cycle_recent(false),
            Command::RecentTabBack => self.cycle_recent(true),
            Command::Tab1 => self.select_tab_position(0),
            Command::Tab2 => self.select_tab_position(1),
            Command::Tab3 => self.select_tab_position(2),
            Command::Tab4 => self.select_tab_position(3),
            Command::Tab5 => self.select_tab_position(4),
            Command::Tab6 => self.select_tab_position(5),
            Command::Tab7 => self.select_tab_position(6),
            Command::Tab8 => self.select_tab_position(7),
            Command::Tab9 => self.select_tab_position(8),
            Command::NextOccurrence => self.occurrence(false),
            Command::Build => self.build(ctx),
            Command::RunProjectTask => self.pick_project_task(ctx),
            Command::RerunProjectTask => {
                if let Some(task) = self.quality.last_project_task.clone() {
                    self.run_project_task(ctx, &task);
                }
            }
            Command::BuildWith => {
                // Choose from "Build with: …" entries in the command palette.
                self.palette = true;
                self.pick_list = None;
                self.commands_only = true;
                self.palette_query = "build with: ".into();
                self.palette_focus = true;
                self.palette_index = 0;
            }
            Command::CancelBuild => self.cancel_build(),
            Command::BuildResults => self.toggle_panel(workbench_panel::Panel::Output),
            Command::Terminal => self.toggle_terminal(ctx),
            Command::TerminalSettings => self.open_preferences(PreferencesPage::Terminal),
            Command::NewTerminal => self.new_terminal(ctx),
            Command::CloseTerminal => self.close_active_terminal(ctx),
            Command::NextTerminal => self.cycle_terminal(true),
            Command::PreviousTerminal => self.cycle_terminal(false),
            Command::RestartTerminal => self.restart_terminal(ctx),
            Command::NextResult => self.next_result(ctx, true),
            Command::PreviousResult => self.next_result(ctx, false),
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
            Command::ExpandToTag => self.expand_markup_selection(ctx),
            Command::SplitLines => {
                let ranges =
                    hedit::advanced::split_into_lines(&self.active().page.text, &self.selections());
                self.set_selections(ranges);
            }
            Command::ToggleBlockComment
            | Command::WrapLines
            | Command::Transpose
            | Command::TitleCase
            | Command::SwapCase
            | Command::DeleteToLineStart
            | Command::DeleteToLineEnd
            | Command::Reindent => self.text_command(command),
            Command::AddCursorAbove => self.add_line_cursor(false),
            Command::AddCursorBelow => self.add_line_cursor(true),
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
            Command::Columns3 => self.split(3),
            Command::Columns4 => self.split(4),
            Command::Rows3 => self.split(5),
            Command::Grid => self.split(6),
            Command::FocusGroup1 => self.focus_group(0, false),
            Command::FocusGroup2 => self.focus_group(1, false),
            Command::FocusGroup3 => self.focus_group(2, false),
            Command::FocusGroup4 => self.focus_group(3, false),
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
            Command::FoldLevel1 => self.fold_level(1),
            Command::FoldLevel2 => self.fold_level(2),
            Command::FoldLevel3 => self.fold_level(3),
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
            Command::NewFileFromTemplate => self.choose_extension_template(ctx),
            Command::ConnectSsh => self.show_ssh_picker(ctx),
            Command::SshOutput => self.show_ssh_output(),
            Command::Open => self.open(ctx, None),
            Command::NewWindow => {
                let session = hedit::windows::new_session(self.workspace.store.directory());
                let result = hedit::windows::executable()
                    .map_err(|e| e.to_string())
                    .and_then(|exe| hedit::windows::spawn(&exe, &session, true));
                self.report(result);
            }
            Command::OpenFolder => self.open_folder(ctx),
            Command::CloseFolder => self.close_folder(),
            Command::RefreshFolder => self.refresh_folder(),
            Command::Save => self.save_from_command(ctx),
            Command::SaveAs => self.save_file(ctx, true),
            Command::Close => {
                self.request_close(vec![self.active().view.id]);
            }
            Command::Reopen => {
                if let Some(index) = self
                    .workspace
                    .closed
                    .iter()
                    .rposition(|view| !self.opens.reopening(view.id))
                {
                    self.reopen_closed(ctx, index);
                }
            }
            Command::Shelf => self.shelf_open = true,
            Command::Rename => self.open_unsaved_file_name(),
            Command::Quit => {
                self.request_quit();
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
            // With nothing selected, the whole line.
            Command::Copy | Command::Cut if self.copy_lines(ctx, command == Command::Cut) => {}
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
            Command::PasteIndent => {
                self.paste_indent = true;
                self.quality.paste_requested = true;
                self.focus_editor = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
            Command::Paste => {
                self.quality.paste_requested = true;
                self.focus_editor = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
            Command::PasteHistory => {
                let items = self
                    .paste_history
                    .iter()
                    .map(|text| {
                        let (label, detail) = paste_history::label(text);
                        (label, detail, Pick::Paste(text.clone()))
                    })
                    .collect();
                self.open_pick_list(ctx, "Paste from history", "Filter copied text…", items);
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
                self.live_find.origin_group = self.panes.focused_id();
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
                if command == Command::Switcher {
                    self.project.begin_search();
                }
                self.palette = true;
                self.pick_list = None;
                self.commands_only = command == Command::Palette;
                self.palette_query.clear();
                self.palette_focus = true;
                self.palette_index = 0;
            }
            Command::Sidebar => {
                self.settings.show_sidebar = !self.command_selected(Command::Sidebar);
                self.distraction_free = false;
                self.focus_editor = true;
            }
            Command::FolderSidebar => {
                self.settings.show_folders =
                    !(self.settings.show_sidebar && self.settings.show_folders);
                if self.settings.show_folders {
                    self.settings.show_sidebar = true;
                }
                self.sync_folder_view();
            }
            Command::Breadcrumbs => {
                self.settings.show_breadcrumbs = !self.command_selected(Command::Breadcrumbs);
                if self.settings.show_breadcrumbs {
                    self.distraction_free = false;
                }
            }
            Command::Outline => {
                self.settings.show_outline = !self.command_selected(Command::Outline);
                if self.settings.show_outline {
                    self.distraction_free = false;
                    self.document_navigation.focus_query = !self.source_control.open;
                    if !self.source_control.open {
                        self.focus_editor = false;
                    }
                } else {
                    self.focus_editor = true;
                    ctx.memory_mut(|m| m.surrender_focus(egui::Id::unique("outline-filter")));
                }
            }
            Command::Tabs => self.settings.show_tab_bar = !self.settings.show_tab_bar,
            Command::StatusBar => self.settings.show_status_bar = !self.settings.show_status_bar,
            Command::CurrentLine => self.settings.highlight_line = !self.settings.highlight_line,
            Command::Whitespace => self.settings.draw_white_space = !self.settings.draw_white_space,
            // Full screen and distraction-free mode are independent: one is the
            // window's size, the other what the window shows.
            Command::Fullscreen => {
                // Ask the window, which may have left full screen by itself.
                let now = ctx
                    .input(|i| i.viewport().fullscreen)
                    .unwrap_or(self.fullscreen);
                self.fullscreen = !now;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            Command::DistractionFree => self.distraction_free = !self.distraction_free,
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
            Command::ToggleSpellCheck => self.settings.spell_check = !self.settings.spell_check,
            Command::NextMisspelling => self.next_misspelling(true),
            Command::PreviousMisspelling => self.next_misspelling(false),
            Command::Bookmark => self.toggle_bookmark(),
            Command::NextBookmark => self.adjacent_bookmark(ctx, true),
            Command::PreviousBookmark => self.adjacent_bookmark(ctx, false),
            Command::Bookmarks => self.bookmarks_open = true,
            Command::Follow => {
                let index = self.workspace.active;
                let followed = !self.workspace.open[index].follow;
                if let Err(error) = self
                    .workspace
                    .editor
                    .set_followed(self.workspace.open[index].page.info.id, followed)
                {
                    self.error = Some(error);
                    return;
                }
                self.workspace.open[index].follow = followed;
                if self.active().follow {
                    self.load_page(ctx, u64::MAX, true);
                }
            }
            Command::Gpu => self.gpu_open = !self.gpu_open,
            Command::Settings => self.open_preferences(self.preferences_page),
            Command::Appearance => self.open_theme_palette(),
            Command::ChooseFont => self.open_font_picker(ctx),
            Command::ToolSettings => self.open_preferences(PreferencesPage::Tools),
            Command::LanguageServers => self.open_preferences(PreferencesPage::LanguageServers),
            Command::LanguageHover => {
                if !self.request_language_feature(ctx, hedit::language_server::Feature::Hover) {
                    self.notice = "Configure a language server for this file first.".into();
                }
            }
            Command::LanguageRename => self.rename_symbol(ctx),
            Command::LanguageQuickFix => {
                if !self.request_language_feature(ctx, hedit::language_server::Feature::QuickFixes)
                {
                    self.notice = "No language server supports quick fixes for this file.".into();
                }
            }
            Command::LanguageFormat => {
                if !self.request_language_feature(ctx, hedit::language_server::Feature::Formatting)
                {
                    self.notice = "Configure a language server for this file first.".into();
                }
            }
            Command::LanguageProblems => self.toggle_panel(workbench_panel::Panel::Problems),
            Command::NextProblem => self.next_problem(true),
            Command::PreviousProblem => self.next_problem(false),
            Command::LanguageRestart => self.restart_language_servers(),
            Command::Checkpoint => self.checkpoint(ctx),
            Command::UserGuide => {
                self.guide.start();
                self.focus_editor = false;
                ctx.memory_mut(|m| {
                    if let Some(id) = m.focused() {
                        m.surrender_focus(id);
                    }
                });
            }
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
            self.pick_list = None;
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
            Extension(String),
            Syntax(syntax::Language),
            /// Remember a build system (or "automatic") and build with it.
            BuildWith(String),
            Line(goto_anything::PickerLine),
            /// An entry from a pick list.
            Pick(Pick),
        }
        let paste = self.pick_list.is_some();
        let commands = !paste && (self.commands_only || self.palette_query.starts_with('>'));
        let mut target: Option<goto_anything::Target> = None;
        let mut preview = None;
        let mut targets_ready = false;
        let mut targets_pending = false;
        let down = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown));
        let up = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp));
        let enter = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        let mut chosen = None;
        let title = self.pick_list.as_ref().map(|l| l.title.clone());
        let shown = egui::Window::new(if let Some(title) = &title {
            title.as_str()
        } else if commands {
            "Command palette"
        } else {
            "Go to file"
        })
        .id(egui::Id::unique("command-palette"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 70.0])
        .default_width(560.0)
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(10.0).corner_radius(4))
        .show(ctx, |ui| {
            let response = bidi_view::field(ui, &mut self.palette_query, |edit| {
                edit.desired_width(f32::INFINITY).font(egui::FontId::proportional(16.0)).margin(egui::Margin::symmetric(8, 7)).hint_text(if let Some(list) = &self.pick_list {
                    list.hint
                } else if commands {
                    "Type a command…"
                } else {
                    "File name, then @symbol, :line, or #text; > for commands…"
                })
            });
            if self.palette_focus {
                response.request_focus();
                self.palette_focus = false;
            }
            let changed = response.changed();
            if changed {
                self.palette_index = 0;
            }
            let commands = !paste && (self.commands_only || self.palette_query.starts_with('>'));
            let filter = self
                .palette_query
                .trim_start_matches('>')
                .trim()
                .to_lowercase();
            // Go to Anything: `file@symbol`, `file:line`, `file#text`.
            let literal_open_file = filter.contains(['@', ':', '#'])
                && self.workspace.open.iter().any(|buffer| buffer.view.name.to_lowercase() == filter);
            let (file_filter, goto) = if commands || paste || literal_open_file {
                (std::borrow::Cow::Borrowed(filter.as_str()), None)
            } else {
                goto_anything::parse_file_query(&filter)
            };
            let filter = file_filter.into_owned();
            let here = goto.is_some() && filter.is_empty();
            target = goto.clone();
            if !commands && !paste && !here && !self.workspace.folders.is_empty() {
                let roots: Vec<_> = self.workspace.folders.iter().map(|f| PathBuf::from(&f.path)).collect();
                self.project.search(ctx, &roots, &filter);
            }
            let mut items = Vec::new();
            // Letters in order, best matches first; equal scores keep their order.
            let score = |text: &str| hedit::folder::fuzzy_match(text, &filter).map(|(s, _)| s);
            let absolute_filter = std::path::Path::new(&filter)
                .is_absolute()
                .then(|| filter.replace('\\', "/"));
            let extension_detail = |id: &str| {
                let shortcut = self.extension_shortcut_text(id);
                if shortcut.is_empty() {
                    id.to_owned()
                } else {
                    format!("{id} · {shortcut}")
                }
            };
            let mut scored: Vec<(i64, (String, String, Item))> = vec![];
            if let Some(list) = &self.pick_list {
                for (label, detail, pick) in &list.items {
                    // The detail, such as a file name, can match too.
                    let found = score(label).or_else(|| score(&format!("{label} {detail}")).map(|s| s - 50));
                    if let Some(found) = found {
                        scored.push((found, (label.clone(), detail.clone(), Item::Pick(pick.clone()))));
                    }
                }
            } else if let Some(command) = self.extensions.iter().find(|c| commands && c.id == filter) {
                if self.extension_available_by_id(&command.id) {
                    scored.push((i64::MAX, (command.title.clone(), extension_detail(&command.id), Item::Extension(command.id.clone()))));
                }
            } else if commands {
                for &command in Command::ALL {
                    if let Some(found) = score(command.label()).filter(|_| command != Command::Palette && self.command_enabled(command)) {
                        scored.push((
                            found,
                            (command.label().to_owned(), self.shortcut_text(command), Item::Command(command)),
                        ));
                    }
                }
                for command in self.extension_commands() {
                    let found = score(&command.title).or_else(|| score(&command.id).map(|score| score - 50));
                    if let Some(found) = found {
                        scored.push((found, (command.title.clone(), extension_detail(&command.id), Item::Extension(command.id.clone()))));
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
                        if let Some(found) = score(&label) {
                            let detail = if current { "current".into() } else { String::new() };
                            scored.push((found, (label, detail, Item::BuildWith(name))));
                        }
                    }
                }
                // Like Sublime's “Set Syntax: …” commands.
                if !filter.is_empty() {
                    for language in syntax::Language::ALL {
                        let label = format!("Set syntax: {}", language.label());
                        if let Some(found) = score(&label) {
                            scored.push((found, (label, String::new(), Item::Syntax(language))));
                        }
                    }
                }
            } else if here {
                // Targets in the current file, previewed as you move.
                match goto.clone().unwrap() {
                    target @ (goto_anything::Target::Symbol(_) | goto_anything::Target::Text(_)) => {
                        self.restore_goto_page_for_matches();
                        let buffer = &self.workspace.open[self.workspace.active];
                        if let Some((matches, ready)) = self.goto_matches.get(ctx, buffer, &target) {
                            targets_ready = ready;
                            for (name, line) in matches.iter() {
                                let detail = if buffer.page.info.paged {
                                    format!("page line {line}")
                                } else {
                                    format!("line {line}")
                                };
                                items.push((name.clone(), detail, Item::Line(goto_anything::PickerLine::Page(*line))));
                            }
                        } else { targets_pending = true; ui.horizontal(|ui| { ui.spinner(); ui.weak("Finding matches…"); }); }
                    }
                    goto_anything::Target::Line(line, column) => {
                        if let Some(line) = line {
                            let label = match column {
                                Some(c) => format!("Go to line {line}, column {c}"),
                                None => format!("Go to line {line}"),
                            };
                            items.push((label, String::new(), Item::Line(goto_anything::PickerLine::File(line, column))));
                        }
                    }
                }
            } else {
                let roots: Vec<_> = self.workspace.folders.iter().map(|f| PathBuf::from(&f.path)).collect();
                for (index, buffer) in self.workspace.open.iter().enumerate() {
                    let label = buffer.page.info.path.as_ref().map(|path| hedit::folder::display_path(&roots, std::path::Path::new(path))).unwrap_or_else(|| buffer.view.name.clone());
                    let absolute_score = absolute_filter.as_deref().and_then(|query| {
                        buffer.page.info.path.as_deref().and_then(|path| hedit::folder::fuzzy_score(path, query))
                    });
                    if let Some(found) = absolute_score.or_else(|| score(&label)).or_else(|| score(&buffer.view.name)) {
                        scored.push((found, (label, "open".to_owned(), Item::Buffer(index))));
                    }
                }
            }
            if !filter.is_empty() {
                scored.sort_by_key(|(found, _)| std::cmp::Reverse(*found));
            }
            items.extend(scored.into_iter().map(|(_, item)| item));
            if !commands && !paste && !here && !self.workspace.folders.is_empty() {
                ui.horizontal(|ui| {
                    if self.project.indexing() {
                        targets_pending = true;
                        ui.spinner();
                        ui.weak("Indexing filenames…");
                    }
                    if ui.small_button("Refresh folders").clicked() {
                        self.project.refresh_search();
                        ctx.request_repaint();
                    }
                });
                match &self.project.search {
                    Some(Ok(results)) => {
                        let full_query = self.palette_query.trim().to_lowercase();
                        let literal = target.as_ref().and_then(|_| {
                            results.entries.iter().find(|entry| entry.name.to_lowercase() == full_query)
                        });
                        if literal.is_some() {
                            target = None;
                            items.clear();
                        }
                        let entries = literal.map_or(results.entries.as_slice(), std::slice::from_ref);
                        for entry in entries {
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
                        targets_pending = true;
                        if !self.project.indexing() {
                            ui.horizontal(|ui| { ui.spinner(); ui.weak("Finding files…"); });
                        }
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
            if navigate || changed || targets_ready {
                if let Some((_, _, Item::Line(line))) = items.get(self.palette_index) {
                    preview = Some(*line);
                }
            }
            ui.separator();
            let height = (ctx.viewport_rect().height() - 190.0).clamp(120.0, 320.0);
            let mut scroll = egui::ScrollArea::vertical().id_salt("palette-results").max_height(height);
            if navigate || changed || targets_ready {
                let stride = 28.0 + ui.spacing().item_spacing.y;
                scroll = scroll.vertical_scroll_offset((self.palette_index as f32 * stride - height / 2.0).max(0.0));
            }
            scroll.show_rows(ui, 28.0, items.len(), |ui, rows| {
                use hedit::icons::FileIcon;

                for index in rows {
                    let (label, shortcut, item) = &items[index];
                    let icon = match item {
                        Item::Buffer(index) => {
                            let info = &self.workspace.open[*index].page.info;
                            Some(info.path.as_ref().map_or(FileIcon::Generic, |_| {
                                FileIcon::for_name(&info.name)
                            }))
                        }
                        Item::File(_) => Some(
                            std::path::Path::new(label)
                                .file_name()
                                .and_then(|name| name.to_str())
                                .map_or(FileIcon::Generic, FileIcon::for_name),
                        ),
                        _ => None,
                    };
                    let text = highlighted(ui, label, &filter, self.theme.accent.color());
                    let response = ui.add(
                        egui::Button::opt_image_and_text(
                            icon.map(|icon| icon.image(ui, self.theme.muted.color())),
                            Some(text),
                        )
                        .selected(index == self.palette_index)
                        .shortcut_text(shortcut)
                        .min_size(egui::vec2(ui.available_width(), 28.0)),
                    );
                    if response.clicked() {
                        chosen = Some(item.clone());
                    }
                }
            });
            if items.is_empty() && !targets_pending {
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
        if let Some(line) = preview {
            self.preview_line(ctx, line);
        }
        if let Some(item) = chosen {
            self.palette = false;
            self.focus_editor = true;
            match item {
                Item::Line(line) => {
                    // Mark where the search started, so Back returns there.
                    self.cancel_goto_preview();
                    self.navigation_mark();
                    self.jump_to_picker_line(ctx, line);
                }
                Item::Buffer(index) => {
                    self.select_tab(index);
                    if let Some(target) = target {
                        self.go_to_target(ctx, None, target);
                    }
                }
                Item::File(path) if target.is_some() => {
                    self.go_to_target(ctx, Some(path), target.unwrap());
                }
                Item::File(path) => self.open(ctx, Some(path.into())),
                Item::Command(command) => self.execute(ctx, command),
                Item::Extension(id) => {
                    self.run_extension_by_id(ctx, &id);
                }
                Item::Pick(Pick::Paste(text)) => self.paste_entry(ctx, text),
                Item::Pick(Pick::File(path)) => self.open(ctx, Some(path)),
                Item::Pick(Pick::ExistingFile(path)) => self.open_existing(ctx, path),
                Item::Pick(Pick::Font(name)) => {
                    self.notice = if name.is_empty() {
                        "Using the built-in font.".into()
                    } else {
                        format!("Using {name}.")
                    };
                    self.settings.font_face = name;
                }
                Item::Pick(Pick::Macro(name)) => {
                    let steps = self
                        .quality
                        .config
                        .macros
                        .iter()
                        .find(|m| m.name == name)
                        .map(|m| m.steps.clone());
                    self.play_macro(steps.unwrap_or_default());
                }
                Item::Pick(Pick::Extension(id)) => {
                    self.run_extension_by_id(ctx, &id);
                }
                Item::Pick(Pick::Location(location)) => self.open_location(ctx, location),
                Item::Pick(Pick::ProjectTask(name)) => self.run_project_task(ctx, &name),
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
