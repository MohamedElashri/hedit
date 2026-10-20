use super::*;

fn menu_item(ui: &mut egui::Ui, app: &HeditApp, command: Command) -> bool {
    ui.add_enabled(
        app.command_enabled(command),
        egui::Button::new(command.label())
            .selected(app.command_selected(command))
            .shortcut_text(command.shortcut()),
    )
    .clicked()
}
impl HeditApp {
    pub(super) fn chrome(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        let mut action = None;
        egui::TopBottomPanel::top("menu-bar").frame(egui::Frame::new().fill(theme.panel.color())
            .inner_margin(egui::Margin::symmetric(8, 3))).show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                for (title, groups) in [
                    ("File", vec![vec![Command::New, Command::Open], vec![Command::Save, Command::SaveAs, Command::Rename], vec![Command::Close, Command::Reopen, Command::Shelf], vec![Command::Quit]]),
                    ("Edit", vec![vec![Command::Undo, Command::Redo], vec![Command::Cut, Command::Copy, Command::Paste, Command::SelectAll]]),
                    ("Find", vec![vec![Command::Find, Command::Replace]]),
                    ("View", vec![vec![Command::Sidebar, Command::LineNumbers, Command::Wrap], vec![Command::Follow], vec![Command::ZoomIn, Command::ZoomOut, Command::ResetZoom]]),
                    ("Goto", vec![vec![Command::Switcher, Command::Goto], vec![Command::Bookmark, Command::NextBookmark, Command::PreviousBookmark, Command::Bookmarks]]),
                    ("Tools", vec![vec![Command::Palette, Command::Gpu], vec![Command::Checkpoint]]),
                    ("Preferences", vec![vec![Command::Settings, Command::Appearance, Command::ToolSettings]]),
                    ("Help", vec![vec![Command::Updates, Command::About]]),
                ] {
                    ui.menu_button(title, |ui| {
                        ui.set_min_width(230.0);
                        for (index, group) in groups.iter().enumerate() {
                            if index > 0 { ui.separator(); }
                            for &command in group {
                                if menu_item(ui, self, command) { action = Some(command); ui.close_menu(); }
                            }
                        }
                        if title == "Help" {
                            ui.separator();
                            ui.hyperlink_to("User guide ↗", "https://github.com/MohamedElashri/hedit/blob/main/USER_GUIDE.md");
                        }
                    });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("hedit").small().color(theme.muted.color()));
                });
            });
        });
        if let Some(command) = action {
            self.execute(ctx, command);
        }
        self.tabs(ctx);
        self.status_bar(ctx);
        self.buffer_sidebar(ctx);
    }
    fn tabs(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        let mut select = None;
        let mut close = None;
        let mut action = None;
        egui::TopBottomPanel::top("tabs")
            .frame(
                egui::Frame::new()
                    .fill(theme.panel.color())
                    .inner_margin(egui::Margin::symmetric(0, 0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt("tab-strip")
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        ui.horizontal(|ui| {
                            for (index, buffer) in self.workspace.open.iter().enumerate() {
                                let active = index == self.workspace.active;
                                let title: String = buffer.view.name.chars().take(30).collect();
                                let title = if buffer.view.name.chars().count() > 30 {
                                    format!("{title}…")
                                } else {
                                    title
                                };
                                let output = egui::Frame::new()
                                    .fill(if active {
                                        theme.background.color()
                                    } else {
                                        theme.surface.color()
                                    })
                                    .inner_margin(egui::Margin::symmetric(12, 7))
                                    .show(ui, |ui| {
                                        ui.push_id(buffer.page.info.id, |ui| {
                                            ui.horizontal(|ui| {
                                                let response = ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new(title).size(12.0).color(
                                                                if active {
                                                                    theme.text.color()
                                                                } else {
                                                                    theme.muted.color()
                                                                },
                                                            ),
                                                        )
                                                        .frame(false)
                                                        .min_size(egui::vec2(80.0, 20.0)),
                                                    )
                                                    .on_hover_text(
                                                        buffer.page.info.path.as_deref().unwrap_or(
                                                            "Scratchpad · included in recovery",
                                                        ),
                                                    );
                                                if response.clicked() {
                                                    select = Some(index);
                                                }
                                                if response.clicked_by(egui::PointerButton::Middle)
                                                {
                                                    close = Some(index);
                                                }
                                                response.context_menu(|ui| {
                                                    if ui.button("Close tab").clicked() {
                                                        close = Some(index);
                                                        ui.close_menu();
                                                    }
                                                    if ui.button("Name scratchpad…").clicked() {
                                                        select = Some(index);
                                                        action = Some(Command::Rename);
                                                        ui.close_menu();
                                                    }
                                                    if ui.button("Browse closed buffers…").clicked()
                                                    {
                                                        action = Some(Command::Shelf);
                                                        ui.close_menu();
                                                    }
                                                });
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new(
                                                                if buffer.page.info.dirty {
                                                                    "●"
                                                                } else {
                                                                    "×"
                                                                },
                                                            )
                                                            .size(11.0)
                                                            .color(if buffer.page.info.dirty {
                                                                theme.accent.color()
                                                            } else {
                                                                theme.muted.color()
                                                            }),
                                                        )
                                                        .frame(false),
                                                    )
                                                    .on_hover_text(
                                                        "Close tab · text stays on the shelf",
                                                    )
                                                    .clicked()
                                                {
                                                    close = Some(index);
                                                }
                                            });
                                        });
                                    });
                                if active {
                                    let r = output.response.rect;
                                    ui.painter().line_segment(
                                        [r.left_top(), r.right_top()],
                                        egui::Stroke::new(2.0, theme.accent.color()),
                                    );
                                }
                            }
                            if ui
                                .add_enabled(
                                    !self.busy && !self.closing,
                                    egui::Button::new("+").frame(false),
                                )
                                .on_hover_text("New scratchpad · Ctrl+N")
                                .clicked()
                            {
                                action = Some(Command::New);
                            }
                        });
                    });
            });
        if !self.busy && !self.closing {
            if let Some(index) = select {
                self.workspace.active = index;
                self.changed();
                self.focus_editor = true;
            }
            if let Some(index) = close {
                let result = self.workspace.close(index);
                self.report(result);
            }
            if let Some(command) = action {
                self.execute(ctx, command);
            }
        }
    }
    fn status_bar(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme.panel.color())
                    .inner_margin(egui::Margin::symmetric(10, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let buffer = self.active();
                    let prefix: String = buffer.page.text.chars().take(buffer.view.head).collect();
                    let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
                    let col = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
                    ui.label(
                        RichText::new(format!(
                            "{}Ln {line}, Col {col}",
                            if buffer.page.info.paged { "Page " } else { "" }
                        ))
                        .size(11.0),
                    );
                    if buffer.follow {
                        ui.label(
                            RichText::new("READ ONLY · FOLLOWING")
                                .size(10.0)
                                .color(theme.accent.color()),
                        );
                    }
                    if self.busy {
                        ui.spinner();
                        if ui.small_button("Cancel").clicked() {
                            self.cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (label, color) = if self.closing {
                            ("Finishing recovery…", theme.warning)
                        } else if self.checkpoint_pending {
                            ("Checkpointing…", theme.warning)
                        } else if self.persisted == self.generation {
                            ("Recovery up to date", theme.success)
                        } else {
                            ("Recovery pending", theme.warning)
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(label).size(11.0).color(color.color()),
                                )
                                .frame(false),
                            )
                            .on_hover_text("View recovery settings and checkpoint status")
                            .clicked()
                        {
                            self.open_preferences(PreferencesPage::Recovery);
                        }
                        ui.separator();
                        let info = &self.active().page.info;
                        ui.label(
                            RichText::new(syntax::language_label(&info.name))
                                .size(11.0)
                                .color(theme.muted.color()),
                        );
                        ui.label(
                            RichText::new(format!(
                                "UTF-8{} · {} · {}",
                                if info.bom { " BOM" } else { "" },
                                info.eol,
                                format_bytes(info.size)
                            ))
                            .size(11.0)
                            .color(theme.muted.color()),
                        );
                    });
                });
            });
    }
    fn buffer_sidebar(&mut self, ctx: &egui::Context) {
        if !self.sidebar {
            return;
        }
        let theme = self.theme.clone();
        let mut select = None;
        egui::SidePanel::left("buffers")
            .default_width(205.0)
            .min_width(140.0)
            .max_width(360.0)
            .frame(
                egui::Frame::new()
                    .fill(theme.panel.color())
                    .inner_margin(egui::Margin::symmetric(10, 14)),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("OPEN FILES")
                        .size(10.0)
                        .strong()
                        .color(theme.muted.color()),
                );
                ui.add_space(7.0);
                egui::ScrollArea::vertical()
                    .id_salt("buffer-list")
                    .max_height((ui.available_height() - 75.0).max(60.0))
                    .show(ui, |ui| {
                        for (index, buffer) in self.workspace.open.iter().enumerate() {
                            let active = index == self.workspace.active;
                            let text = format!(
                                "{}  {}",
                                if buffer.page.info.dirty { "●" } else { "·" },
                                buffer.view.name
                            );
                            let response = ui
                                .add(
                                    egui::Button::new(RichText::new(text).size(12.0).color(
                                        if active {
                                            theme.text.color()
                                        } else {
                                            theme.muted.color()
                                        },
                                    ))
                                    .selected(active)
                                    .fill(if active {
                                        theme.elevated.color()
                                    } else {
                                        theme.panel.color()
                                    })
                                    .stroke(egui::Stroke::NONE)
                                    .min_size(egui::vec2(ui.available_width(), 27.0))
                                    .truncate(),
                                )
                                .on_hover_text(
                                    buffer
                                        .page
                                        .info
                                        .path
                                        .as_deref()
                                        .unwrap_or("Unsaved scratchpad"),
                                );
                            if response.clicked() {
                                select = Some(index);
                            }
                        }
                    });
                ui.add_space(16.0);
                ui.separator();
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!(
                                "CLOSED BUFFERS    {}",
                                self.workspace.closed.len()
                            ))
                            .size(10.0)
                            .color(theme.muted.color()),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Browse and reopen closed buffers")
                    .clicked()
                {
                    self.shelf_open = true;
                }
            });
        if !self.busy && !self.closing {
            if let Some(index) = select {
                self.workspace.active = index;
                self.changed();
                self.focus_editor = true;
            }
        }
    }
    pub(super) fn search_panel(&mut self, ctx: &egui::Context) {
        if !self.find_open {
            return;
        }
        egui::TopBottomPanel::bottom("find-panel")
            .frame(
                egui::Frame::new()
                    .fill(self.theme.surface.color())
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Find");
                    let width = (ui.available_width() - 225.0).max(100.0);
                    let input_id = ui.make_persistent_id("find-input");
                    let enter = ctx.memory(|m| m.has_focus(input_id))
                        && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .id(input_id)
                            .desired_width(width)
                            .hint_text("Literal, case-sensitive text"),
                    );
                    if self.find_focus {
                        response.request_focus();
                        self.find_focus = false;
                    }
                    let clicked = ui
                        .add_enabled(!self.busy && !self.closing, egui::Button::new("Find next"))
                        .clicked();
                    if (clicked || enter) && !self.busy && !self.closing {
                        self.find(ctx);
                    }
                    ui.toggle_value(&mut self.replace_open, "Replace");
                    if ui.button("×").on_hover_text("Close search · Esc").clicked() {
                        self.find_open = false;
                        self.focus_editor = true;
                    }
                });
                if self.replace_open {
                    ui.horizontal(|ui| {
                        ui.label("With");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.replacement)
                                .desired_width((ui.available_width() - 150.0).max(100.0))
                                .hint_text("Replacement text"),
                        );
                        if ui
                            .add_enabled(
                                self.command_enabled(Command::Replace),
                                egui::Button::new("Replace selected"),
                            )
                            .clicked()
                        {
                            self.replace_selection();
                        }
                    });
                }
                ui.label(
                    RichText::new(&self.notice)
                        .size(11.0)
                        .color(self.theme.muted.color()),
                );
            });
    }
}
