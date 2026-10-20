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
            self.appearance_original = self.theme.clone();
        }
        self.preferences_page = page;
        self.preferences_open = true;
        self.focus_editor = false;
    }
    pub(super) fn preferences(&mut self, ctx: &egui::Context) {
        if self.preferences_open {
            let before = self.theme.clone();
            let mut open = true;
            let body_height = (ctx.screen_rect().height() - 190.0).clamp(190.0, 420.0);
            let width = (ctx.screen_rect().width() - 64.0).clamp(540.0, 700.0);
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
                if (self.theme.dark != before.dark
                    || Role::ALL
                        .into_iter()
                        .any(|role| self.theme.color(role) != before.color(role)))
                    && Preset::ALL.into_iter().any(|p| self.theme.name == p.name())
                    && !Preset::ALL.into_iter().any(|p| self.theme == p.theme())
                {
                    self.theme.name = "My theme".into();
                }
                self.theme.apply(ctx);
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
        ui.checkbox(&mut self.sidebar, "Show open files sidebar");
        ui.add_space(14.0);
        ui.label("Editor font size");
        ui.add(
            egui::Slider::new(&mut self.font_size, 10.0..=28.0)
                .step_by(1.0)
                .suffix(" px"),
        );
        ui.weak("Ctrl++ and Ctrl+- adjust the editor font while you work.");
        ui.add_space(18.0);
        if ui.button("Restore editor defaults").clicked() {
            self.wrap = false;
            self.line_numbers = true;
            self.sidebar = true;
            self.font_size = 14.0;
        }
    }
    fn theme_preferences(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.weak("Choose a palette, then customize its colors. Changes preview live.");
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for preset in Preset::ALL {
                let theme = preset.theme();
                if ui
                    .add(
                        egui::Button::new(RichText::new(preset.name()).color(theme.text.color()))
                            .fill(theme.background.color())
                            .stroke(egui::Stroke::new(
                                if self.theme == theme { 2.0 } else { 1.0 },
                                if self.theme == theme {
                                    self.theme.accent.color()
                                } else {
                                    self.theme.border.color()
                                },
                            )),
                    )
                    .clicked()
                {
                    self.set_theme(ctx, theme);
                }
            }
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.add(
                egui::TextEdit::singleline(&mut self.theme.name)
                    .char_limit(80)
                    .desired_width(175.0),
            );
            ui.checkbox(&mut self.theme.dark, "Dark controls");
        });
        ui.add_space(10.0);
        egui::Grid::new("theme-colors")
            .num_columns(3)
            .spacing([14.0, 6.0])
            .show(ui, |ui| {
                for (index, role) in Role::ALL.into_iter().enumerate() {
                    ui.label(role.label());
                    let color = self.theme.color_mut(role);
                    if ui.color_edit_button_srgb(&mut color.0).changed() {
                        self.theme_hex[index] = color.hex();
                    }
                    let valid = Rgb::try_from(self.theme_hex[index].clone()).is_ok();
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.theme_hex[index])
                            .id_salt(("hex", index))
                            .desired_width(85.0)
                            .font(egui::TextStyle::Monospace)
                            .char_limit(7)
                            .text_color(if valid {
                                self.theme.text.color()
                            } else {
                                self.theme.error.color()
                            }),
                    );
                    if response.changed() {
                        if let Ok(color) = Rgb::try_from(self.theme_hex[index].clone()) {
                            *self.theme.color_mut(role) = color;
                        }
                    }
                    if !valid {
                        response.on_hover_text("Use #RRGGBB. Your last valid color stays active.");
                    }
                    ui.end_row();
                }
            });
        ui.add_space(10.0);
        let contrast = Theme::contrast(self.theme.text, self.theme.background);
        let selection = Theme::contrast(self.theme.selection_text, self.theme.selection);
        ui.colored_label(
            if contrast >= 4.5 && selection >= 4.5 {
                self.theme.muted.color()
            } else {
                self.theme.warning.color()
            },
            format!(
                "Text {contrast:.1}:1 · Selection {selection:.1}:1{}",
                if contrast < 4.5 || selection < 4.5 {
                    " · Low contrast"
                } else {
                    ""
                }
            ),
        );
        egui::Frame::new()
            .fill(self.theme.background.color())
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.label(syntax::highlight_with_theme(
                    "__global__ void kernel() {\n    const int value = 42; // preview\n}",
                    12.0,
                    ui.available_width(),
                    true,
                    &self.theme,
                ));
            });
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Import JSON…"))
                .clicked()
            {
                self.import_theme(ctx);
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Export JSON…"))
                .clicked()
            {
                self.export_theme(ctx);
            }
            if ui.button("Revert theme").clicked() {
                self.set_theme(ctx, self.appearance_original.clone());
            }
        });
    }
    fn tool_preferences(&mut self, ui: &mut egui::Ui) {
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
        ui.label("Scratchpads and modified files are checkpointed every two seconds while work changes, and on normal exit.");
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
        ui.label("Closed buffers stay on the shelf. Unsaved entries are never silently evicted; the shelf holds up to 50 buffers.");
        if ui.button("Browse closed buffers…").clicked() {
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
        egui::Window::new("Go to line")
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
        self.goto_open = open;
        if !open {
            self.focus_editor = true;
        }
    }
}
