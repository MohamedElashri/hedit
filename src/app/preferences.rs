use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreferencesPage {
    Editor,
    Appearance,
    Tools,
    Recovery,
}
impl PreferencesPage {
    fn title(self) -> &'static str {
        match self {
            Self::Editor => "Editor",
            Self::Appearance => "Appearance",
            Self::Tools => "CUDA Tools",
            Self::Recovery => "Recovery",
        }
    }
}
impl HeditApp {
    pub(super) fn open_preferences(&mut self, page: PreferencesPage) {
        if !self.preferences_open {
            self.appearance.original = self.theme.clone();
        }
        self.preferences_page =
            if page == PreferencesPage::Tools && !hedit::debugger::LOCAL_TOOLS_SUPPORTED {
                PreferencesPage::Editor
            } else {
                page
            };
        self.preferences_open = true;
        self.focus_editor = false;
    }
    pub(super) fn preferences(&mut self, ctx: &egui::Context) {
        if self.preferences_open {
            let before = self.theme.clone();
            let mut open = true;
            let body_height = (ctx.screen_rect().height() - 190.0).clamp(190.0, 520.0);
            let width = (ctx.screen_rect().width() - 64.0).clamp(540.0, 780.0);
            egui::Window::new("Preferences")
                .id(egui::Id::new("preferences"))
                .open(&mut open)
                .fixed_size(egui::vec2(width, body_height + 74.0))
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.set_height(body_height);
                        ui.vertical(|ui| {
                            ui.set_width(120.0);
                            ui.add_space(8.0);
                            for page in [
                                PreferencesPage::Editor,
                                PreferencesPage::Appearance,
                                PreferencesPage::Tools,
                                PreferencesPage::Recovery,
                            ] {
                                if page == PreferencesPage::Tools
                                    && !hedit::debugger::LOCAL_TOOLS_SUPPORTED
                                {
                                    continue;
                                }
                                if ui
                                    .add_sized(
                                        [120.0, 30.0],
                                        egui::Button::new(page.title())
                                            .selected(self.preferences_page == page),
                                    )
                                    .clicked()
                                {
                                    self.preferences_page = page;
                                }
                            }
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            ui.set_min_width(350.0);
                            ui.heading(self.preferences_page.title());
                            ui.add_space(10.0);
                            egui::ScrollArea::vertical()
                                .id_salt(("preferences-body", self.preferences_page as u8))
                                .max_height(body_height - 40.0)
                                .show(ui, |ui| match self.preferences_page {
                                    PreferencesPage::Editor => self.editor_preferences(ui),
                                    PreferencesPage::Appearance => self.theme_preferences(ui, ctx),
                                    PreferencesPage::Tools => self.tool_preferences(ui),
                                    PreferencesPage::Recovery => self.recovery_preferences(ui, ctx),
                                });
                        });
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.weak("Preferences save automatically on this device.");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Done").clicked() {
                                self.preferences_open = false;
                                self.focus_editor = true;
                            }
                        });
                    });
                });
            self.preferences_open &= open;
            if !open {
                self.focus_editor = true;
            }
            if self.theme != before {
                self.theme_edited(ctx, &before);
            }
        }
        self.navigation_dialog(ctx);
        if self.about_open {
            let mut open = true;
            egui::Window::new("About hedit")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        brand::paint(
                            ui,
                            36.0,
                            self.theme.accent.color(),
                            self.theme.muted.color(),
                        );
                        ui.heading("hedit");
                    });
                    ui.label("Keep your train of thought.");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.separator();
                    ui.hyperlink_to(
                        "User guide",
                        "https://github.com/MohamedElashri/hedit/blob/main/USER_GUIDE.md",
                    );
                    ui.hyperlink_to(
                        "Releases",
                        "https://github.com/MohamedElashri/hedit/releases",
                    );
                    ui.label("MIT licensed");
                });
            self.about_open = open;
        }
    }
    fn editor_preferences(&mut self, ui: &mut egui::Ui) {
        ui.weak("Choose how your workspace and text are displayed.");
        ui.add_space(12.0);
        ui.checkbox(&mut self.wrap, "Word wrap");
        ui.checkbox(&mut self.line_numbers, "Show line numbers");
        ui.checkbox(&mut self.show_tabs, "Show tabs");
        ui.checkbox(&mut self.show_status, "Show status bar");
        ui.checkbox(&mut self.highlight_line, "Highlight current line");
        ui.checkbox(&mut self.show_whitespace, "Show whitespace");
        ui.checkbox(&mut self.indent_guides, "Show indentation guides");
        ui.checkbox(&mut self.minimap, "Show minimap");
        ui.checkbox(&mut self.auto_indent, "Automatic indentation");
        ui.checkbox(&mut self.smart_indent, "Smart indentation");
        ui.checkbox(&mut self.soft_tabs, "Backspace to indentation stops");
        ui.horizontal(|ui| {
            ui.label("Column ruler");
            egui::ComboBox::from_id_salt("ruler-column")
                .selected_text(if self.ruler_column == 0 {
                    "None".into()
                } else {
                    self.ruler_column.to_string()
                })
                .show_ui(ui, |ui| {
                    for column in [0, 80, 100, 120] {
                        ui.selectable_value(
                            &mut self.ruler_column,
                            column,
                            if column == 0 {
                                "None".into()
                            } else {
                                column.to_string()
                            },
                        );
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Word wrap column");
            egui::ComboBox::from_id_salt("wrap-column")
                .selected_text(if self.wrap_column == 0 {
                    "Window".into()
                } else {
                    self.wrap_column.to_string()
                })
                .show_ui(ui, |ui| {
                    for column in [0, 80, 100, 120] {
                        if ui
                            .selectable_value(
                                &mut self.wrap_column,
                                column,
                                if column == 0 {
                                    "Window".into()
                                } else {
                                    column.to_string()
                                },
                            )
                            .changed()
                        {
                            self.wrap = true;
                        }
                    }
                });
        });
        ui.checkbox(&mut self.sidebar, "Show sidebar");
        if ui
            .checkbox(&mut self.folder_sidebar, "Show folders in sidebar")
            .changed()
        {
            self.sync_folder_view();
        }
        ui.add_space(14.0);
        ui.label("Editor font size");
        ui.add(
            egui::Slider::new(&mut self.font_size, 10.0..=28.0)
                .step_by(1.0)
                .suffix(" px"),
        );
        ui.weak(format!(
            "{} and {} adjust the editor font while you work.",
            Command::ZoomIn.shortcut(),
            Command::ZoomOut.shortcut()
        ));
        ui.add_space(18.0);
        if ui.button("Restore editor defaults").clicked() {
            self.wrap = false;
            self.line_numbers = true;
            self.show_tabs = true;
            self.show_status = true;
            self.highlight_line = true;
            self.show_whitespace = false;
            self.indent_guides = true;
            self.minimap = false;
            self.auto_indent = true;
            self.smart_indent = true;
            self.soft_tabs = true;
            self.ruler_column = 0;
            self.wrap_column = 0;
            self.sidebar = true;
            self.folder_sidebar = true;
            self.font_size = 14.0;
            self.sync_folder_view();
        }
    }
    fn tool_preferences(&mut self, ui: &mut egui::Ui) {
        if !hedit::debugger::LOCAL_TOOLS_SUPPORTED {
            ui.label("CUDA source editing is available. Local CUDA tools are disabled on macOS.");
            return;
        }
        ui.weak("Configure NVIDIA tools here. Choose a program and arguments in the CUDA panel.");
        ui.add_space(12.0);
        ui.label("CUDA-GDB executable");
        let live = self
            .debug
            .as_ref()
            .is_some_and(|d| d.state != State::Exited);
        ui.add_enabled(
            !live,
            egui::TextEdit::singleline(&mut self.debugger)
                .desired_width(f32::INFINITY)
                .hint_text("cuda-gdb or /path/to/cuda-gdb"),
        );
        if live {
            ui.weak("Stop the running tool session before changing the debugger path.");
        }
        ui.add_space(8.0);
        ui.label("Compute Sanitizer and nvidia-smi are found on PATH.");
        ui.label("Install the CUDA Toolkit and a compatible NVIDIA driver separately.");
        ui.add_space(14.0);
        if ui.button("Open CUDA panel").clicked() {
            self.gpu_open = true;
        }
    }
    fn recovery_preferences(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label("Unsaved and modified files are checkpointed every two seconds while work changes, and on normal exit.");
        ui.add_space(12.0);
        ui.label(if self.checkpoint_pending {
            "Checkpoint in progress…"
        } else if self.persisted == self.generation {
            "Recovery up to date"
        } else {
            "Changes waiting for a checkpoint"
        });
        if let Some(saved) = self.last_saved {
            ui.weak(format!(
                "Last completed checkpoint: {} seconds ago",
                saved.elapsed().as_secs()
            ));
        }
        if ui
            .add_enabled(
                !self.checkpoint_pending && !self.closing,
                egui::Button::new("Checkpoint now"),
            )
            .clicked()
        {
            self.checkpoint(ctx);
        }
        ui.add_space(12.0);
        ui.label("Closed files stay on the shelf. Unsaved entries are never silently evicted; the shelf holds up to 50 files.");
        if ui.button("Browse closed files…").clicked() {
            self.shelf_open = true;
        }
        ui.add_space(12.0);
        ui.weak("Recovery is periodic. A crash or power loss can lose edits since the last completed checkpoint. Keep independent backups for important work.");
    }
    fn navigation_dialog(&mut self, ctx: &egui::Context) {
        if !self.goto_open {
            return;
        }
        let mut open = true;
        let mut go = false;
        let shown = egui::Window::new("Go to line")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 90.0])
            .show(ctx, |ui| {
                ui.label("Line number in the whole document");
                let input_id = egui::Id::new("goto-line-input");
                let enter = ctx.memory(|m| m.has_focus(input_id))
                    && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.goto)
                        .id(input_id)
                        .desired_width(f32::INFINITY)
                        .hint_text("1"),
                );
                if self.goto_focus {
                    response.request_focus();
                    self.goto_focus = false;
                }
                let valid = self.goto.parse::<u64>().is_ok_and(|line| line > 0);
                go = (ui
                    .add_enabled(!self.busy && valid, egui::Button::new("Go"))
                    .clicked()
                    || enter)
                    && valid;
                if !valid && !self.goto.is_empty() {
                    ui.colored_label(
                        self.theme.warning.color(),
                        "Enter a positive whole line number.",
                    );
                }
            });
        if go && !self.busy {
            if self.goto.parse::<u64>().is_ok_and(|line| line > 0) {
                self.goto_line(ctx);
                open = false;
            } else {
                self.notice = "Enter a positive line number.".into();
            }
        }
        if super::overlays::clicked_away(ctx, &shown) {
            open = false;
        }
        self.goto_open = open;
        if !open {
            self.focus_editor = true;
        }
    }
}
