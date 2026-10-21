use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Command {
    New,
    Open,
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
    Find,
    Replace,
    Goto,
    Switcher,
    Palette,
    Sidebar,
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
        Self::New,
        Self::Open,
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
        Self::Find,
        Self::Replace,
        Self::Goto,
        Self::Switcher,
        Self::Sidebar,
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
            Self::New => "New file",
            Self::Open => "Open file…",
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
            Self::Find => "Find…",
            Self::Replace => "Replace…",
            Self::Goto => "Go to line…",
            Self::Switcher => "Switch file…",
            Self::Palette => "Command palette…",
            Self::Sidebar => "Show sidebar",
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
    pub(super) fn shortcut(self) -> &'static str {
        match self {
            Self::New => "Ctrl+N",
            Self::Open => "Ctrl+O",
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
}
impl HeditApp {
    pub(super) fn command_enabled(&self, command: Command) -> bool {
        if self.busy || self.closing {
            return false;
        }
        match command {
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
            Command::Sidebar => self.sidebar,
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
            Command::New => {
                let r = self.workspace.new_buffer();
                self.report(r);
            }
            Command::Open => self.open(ctx, None),
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
        }
    }
    pub(super) fn command_palette(&mut self, ctx: &egui::Context) {
        if !self.palette {
            return;
        }
        #[derive(Clone, Copy)]
        enum Item {
            Buffer(usize),
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
            "Switch file"
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
                    if buffer.view.name.to_lowercase().contains(&filter) {
                        items.push((buffer.view.name.clone(), "file", Item::Buffer(index)));
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
                                .shortcut_text(*shortcut)
                                .min_size(egui::vec2(ui.available_width(), 28.0)),
                        );
                        if navigate && index == self.palette_index {
                            response.scroll_to_me(None);
                        }
                        if response.clicked() {
                            chosen = Some(*item);
                        }
                    }
                });
            if items.is_empty() {
                ui.label("No matching results.");
            }
            if enter {
                chosen = items.get(self.palette_index).map(|(_, _, item)| *item);
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
                Item::Command(command) => self.execute(ctx, command),
            }
        }
    }
}
