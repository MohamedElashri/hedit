use super::*;

/// The Find bar's query field, which keeps focus while stepping through matches.
pub(super) const FIND_INPUT: &str = "find-input";

fn menu_item(ui: &mut egui::Ui, app: &HeditApp, command: Command) -> bool {
    if matches!(command, Command::Gpu | Command::ToolSettings)
        && !hedit::debugger::LOCAL_TOOLS_SUPPORTED
    {
        return false;
    }
    let enabled = app.command_enabled(command);
    // Reserve a layer under the entry so unavailable entries can still show
    // which row the pointer is on; egui gives disabled widgets no hover state.
    let backdrop = ui.painter().add(egui::Shape::Noop);
    let response = ui.add_enabled(
        enabled,
        egui::Button::new(command.label())
            .selected(app.command_selected(command))
            .shortcut_text(app.shortcut_text(command)),
    );
    if !enabled && response.contains_pointer() {
        let hovered = &ui.visuals().widgets.hovered;
        ui.painter().set(
            backdrop,
            egui::Shape::rect_filled(
                response.rect.expand(hovered.expansion),
                hovered.corner_radius,
                app.theme.hover().color().gamma_multiply(0.6),
            ),
        );
    }
    response.clicked()
}
impl HeditApp {
    pub(super) fn chrome(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        let mut action = None;
        let mut open_menu = None;
        egui::TopBottomPanel::top("menu-bar").frame(egui::Frame::new().fill(theme.panel.color())
            .inner_margin(egui::Margin::symmetric(8, 3))).show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                for (title, groups) in [
                    ("File", vec![vec![Command::New, Command::NewWindow, Command::Open, Command::OpenFolder], vec![Command::RefreshFolder, Command::CloseFolder], vec![Command::Save, Command::SaveAs, Command::Rename, Command::Encoding], vec![Command::OpenRecent], vec![Command::Close, Command::CloseAll, Command::CloseUnmodified, Command::Reopen, Command::Shelf], if cfg!(target_os = "macos") { vec![] } else { vec![Command::Quit] }]),
                    ("Edit", vec![]),
                    ("Find", vec![vec![Command::Find, Command::Replace, Command::FindPrevious, Command::ReplaceAll, Command::FindFiles], vec![Command::SelectAllMatches, Command::UseSelectionForFind, Command::UseSelectionForReplace, Command::QuickFindNext, Command::QuickFindPrevious]]),
                    ("View", vec![]),
                    ("Goto", vec![vec![Command::Switcher, Command::Goto, Command::Symbols, Command::GotoDefinition, Command::GotoReferences], vec![Command::NextTab, Command::PreviousTab, Command::RecentTab], vec![Command::NextChange, Command::PreviousChange], vec![Command::NavBack, Command::NavForward], vec![Command::Bookmark, Command::NextBookmark, Command::PreviousBookmark, Command::SelectBookmarks, Command::ClearBookmarks, Command::Bookmarks]]),
                    ("Project", vec![vec![Command::OpenProject,Command::SaveProject,Command::SaveProjectAs,Command::RecentProjects],vec![Command::ProjectSettings]]),
                    ("Tools", vec![vec![Command::Palette, Command::Gpu, Command::GitDiff, Command::RevertChange, Command::CompareDisk, Command::Complete, Command::Snippets], vec![Command::RecordMacro, Command::PlayMacro, Command::SaveMacro, Command::RunMacro], vec![Command::Build, Command::BuildWith, Command::CancelBuild, Command::BuildResults, Command::NextResult, Command::PreviousResult], vec![Command::Checkpoint]]),
                    ("Preferences", vec![vec![Command::Settings, Command::Appearance, Command::ChooseFont, Command::ToolSettings, Command::KeyBindings]]),
                    // macOS's own app menu already has About and Quit.
                    ("Help", vec![if cfg!(target_os = "macos") { vec![Command::Updates] } else { vec![Command::Updates, Command::About] }]),
                ] {
                    // egui does not mark the open menu's title, so keep it filled
                    // like a hovered title while its menu is showing.
                    let mut button = egui::Button::new(title);
                    if self.open_menu == Some(title) {
                        button = button.fill(theme.hover().color());
                    }
                    egui::menu::menu_custom_button(ui, button, |ui| {
                        // Contents run only while this menu is open.
                        open_menu = Some(title);
                        ui.set_min_width(245.0);
                        if title == "Edit" { self.edit_menu(ui, &mut action); return; }
                        if title == "View" { self.view_menu(ui, &mut action); return; }
                        for (index, group) in groups.iter().filter(|g| !g.is_empty()).enumerate() {
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
        if self.open_menu != open_menu {
            self.open_menu = open_menu;
            ctx.request_repaint();
        }
        if let Some(command) = action {
            self.execute(ctx, command);
        }
        self.status_bar(ctx);
        self.buffer_sidebar(ctx);
        self.tabs(ctx);
    }
    fn tabs(&mut self, ctx: &egui::Context) {
        if !self.settings.show_tab_bar || self.distraction_free {
            return;
        }
        let theme = self.theme.clone();
        let mut select = None;
        let mut close = None;
        let mut action = None;
        let mut reorder = None;
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
                                let can_close = self.workspace.can_close(index);
                                let title: String = hedit::bidi::display(&buffer.view.name)
                                    .chars()
                                    .take(30)
                                    .collect();
                                let title = if buffer.view.name.chars().count() > 30 {
                                    format!("{title}…")
                                } else {
                                    title
                                };
                                let title = if buffer.view.pinned {
                                    format!("◆ {title}")
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
                                                            (if self.quality.preview
                                                                == Some(buffer.view.id)
                                                            {
                                                                RichText::new(title).italics()
                                                            } else {
                                                                RichText::new(title)
                                                            })
                                                            .size(12.0)
                                                            .color(if active {
                                                                theme.text.color()
                                                            } else {
                                                                theme.muted.color()
                                                            }),
                                                        )
                                                        .frame(false)
                                                        .min_size(egui::vec2(80.0, 20.0))
                                                        .sense(egui::Sense::click_and_drag()),
                                                    )
                                                    .on_hover_text(
                                                        buffer.page.info.path.as_deref().unwrap_or(
                                                            "Unsaved file · included in recovery",
                                                        ),
                                                    );
                                                if response.drag_started() {
                                                    self.quality.tab_drag = Some(buffer.view.id);
                                                }
                                                if response.contains_pointer()
                                                    && ui.input(|i| i.pointer.any_released())
                                                {
                                                    if let Some(id) = self.quality.tab_drag.take() {
                                                        reorder = Some((id, index));
                                                    }
                                                }
                                                if response.double_clicked() {
                                                    self.quality.preview = None;
                                                }
                                                if response.clicked() {
                                                    select = Some(index);
                                                }
                                                if response.clicked_by(egui::PointerButton::Middle)
                                                    && can_close
                                                {
                                                    close = Some(index);
                                                }
                                                response.context_menu(|ui| {
                                                    for command in [
                                                        Command::PinTab,
                                                        Command::CloseOthers,
                                                        Command::CloseRight,
                                                        Command::CopyPath,
                                                        Command::RevealInSidebar,
                                                    ] {
                                                        if matches!(
                                                            command,
                                                            Command::CopyPath
                                                                | Command::RevealInSidebar
                                                        ) && buffer.page.info.path.is_none()
                                                        {
                                                            continue;
                                                        }
                                                        if ui.button(command.label()).clicked() {
                                                            select = Some(index);
                                                            action = Some(command);
                                                            ui.close_menu();
                                                        }
                                                    }
                                                    if ui
                                                        .add_enabled(
                                                            can_close,
                                                            egui::Button::new("Close tab"),
                                                        )
                                                        .clicked()
                                                    {
                                                        close = Some(index);
                                                        ui.close_menu();
                                                    }
                                                    if ui.button("Name unsaved file…").clicked() {
                                                        select = Some(index);
                                                        action = Some(Command::Rename);
                                                        ui.close_menu();
                                                    }
                                                    if ui.button("Browse closed files…").clicked()
                                                    {
                                                        action = Some(Command::Shelf);
                                                        ui.close_menu();
                                                    }
                                                });
                                                if can_close
                                                    && ui
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
                                .on_hover_text(format!("New file · {}", Command::New.shortcut()))
                                .clicked()
                            {
                                action = Some(Command::New);
                            }
                        });
                    });
            });
        if !self.busy && !self.closing {
            if let Some((id, target)) = reorder {
                let active = self.active().view.id;
                if let Some(source) = self.workspace.open.iter().position(|b| b.view.id == id) {
                    let buffer = self.workspace.open.remove(source);
                    self.workspace
                        .open
                        .insert(target.min(self.workspace.open.len()), buffer);
                    self.workspace.active = self
                        .workspace
                        .open
                        .iter()
                        .position(|b| b.view.id == active)
                        .unwrap_or(0);
                    self.changed();
                }
                self.quality.tab_drag = None;
            }
            if let Some(index) = select {
                self.select_tab(index);
                self.focus_editor = true;
            }
            if let Some(index) = close {
                self.request_close(vec![self.workspace.open[index].view.id]);
            }
            if let Some(command) = action {
                self.execute(ctx, command);
            }
        }
    }
    fn status_bar(&mut self, ctx: &egui::Context) {
        if !self.settings.show_status_bar || self.distraction_free {
            return;
        }
        let mut status_action = None;
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
                    let (line, col) = hedit::advanced::line_column(
                        &buffer.page.text,
                        buffer.view.head,
                        buffer.view.tab_size.unwrap_or(4) as usize,
                    );
                    let (line, col) = (line + 1, col + 1);
                    ui.label(
                        RichText::new(format!(
                            "{}Ln {line}, Col {col}",
                            if buffer.page.info.paged { "Page " } else { "" }
                        ))
                        .size(11.0),
                    );
                    if !buffer.view.selections.is_empty() {
                        ui.label(
                            RichText::new(format!("{} cursors", buffer.view.selections.len() + 1))
                                .size(10.0)
                                .color(theme.accent.color()),
                        );
                    }
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
                    // Show each new notice briefly, like Sublime's status messages.
                    if self.notice_shown.0 != self.notice {
                        self.notice_shown = (self.notice.clone(), Instant::now());
                    }
                    let age = self.notice_shown.1.elapsed();
                    if age < NOTICE_TIME && self.notice != "Ready." {
                        ui.separator();
                        ui.label(
                            RichText::new(&self.notice)
                                .size(11.0)
                                .color(theme.muted.color()),
                        );
                        ctx.request_repaint_after(NOTICE_TIME - age);
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
                        self.language_menu(ui);
                        let info = &self.active().page.info;
                        ui.label(
                            RichText::new(format_bytes(info.size))
                                .size(11.0)
                                .color(theme.muted.color()),
                        );
                        self.status_menus(ui, &mut status_action);
                    });
                });
            });
        if let Some(command) = status_action {
            self.execute(ctx, command);
        }
    }
    /// Indentation, line ending, and encoding, each clickable, like Sublime.
    fn status_menus(&self, ui: &mut egui::Ui, action: &mut Option<Command>) {
        let muted = self.theme.muted.color();
        let small = |text: String| {
            egui::Button::new(RichText::new(text).size(11.0).color(muted)).frame(false)
        };
        let info = &self.active().page.info;
        if ui
            .add(small(format!(
                "{}{}",
                info.encoding,
                if info.bom { " BOM" } else { "" }
            )))
            .on_hover_text("Reopen or save with another encoding")
            .clicked()
        {
            *action = Some(Command::Encoding);
        }
        egui::menu::menu_custom_button(ui, small(info.eol.to_owned()), |ui| {
            for command in [Command::ToLf, Command::ToCrlf] {
                if menu_item(ui, self, command) {
                    *action = Some(command);
                    ui.close_menu();
                }
            }
        })
        .response
        .on_hover_text("Line endings");
        let view = &self.active().view;
        let width = view.tab_size.unwrap_or(4);
        let indent = if view.indent.as_deref() == Some("tabs") {
            format!("Tab width: {width}")
        } else {
            format!("Spaces: {width}")
        };
        egui::menu::menu_custom_button(ui, small(indent), |ui| {
            ui.set_min_width(220.0);
            for command in [Command::IndentSpaces, Command::IndentTabs] {
                if menu_item(ui, self, command) {
                    *action = Some(command);
                    ui.close_menu();
                }
            }
            ui.separator();
            for command in [Command::Indent2, Command::Indent4, Command::Indent8] {
                if menu_item(ui, self, command) {
                    *action = Some(command);
                    ui.close_menu();
                }
            }
            ui.separator();
            for command in [Command::DetectIndent, Command::ToSpaces, Command::ToTabs] {
                if menu_item(ui, self, command) {
                    *action = Some(command);
                    ui.close_menu();
                }
            }
        })
        .response
        .on_hover_text("Indentation");
    }
    /// Choose the active file's syntax (`None` detects it from the name) and
    /// apply that syntax's formatting rules.
    pub(super) fn set_syntax(&mut self, language: Option<syntax::Language>) {
        if self.workspace.open[self.workspace.active].set_language(language) {
            self.changed();
            self.apply_format_preferences();
        }
    }
    fn language_menu(&mut self, ui: &mut egui::Ui) {
        let buffer = self.active();
        let language = buffer.language();
        let detected = syntax::Language::detect(&buffer.page.info.name);
        let selected = buffer.language_override();
        let popup_id = egui::Id::new("syntax-popup");
        if ui.input(|input| input.key_pressed(Key::Escape)) {
            ui.memory_mut(|memory| memory.close_popup());
        }
        let response = ui.add(
            egui::Button::new(
                RichText::new(format!("{} ⏷", language.label()))
                    .size(14.0)
                    .color(
                        if selected.is_some() {
                            self.theme.accent
                        } else {
                            self.theme.muted
                        }
                        .color(),
                    ),
            )
            .min_size(egui::vec2(156.0, 28.0))
            .selected(ui.memory(|memory| memory.is_popup_open(popup_id))),
        );
        if response.clicked() {
            ui.memory_mut(|memory| memory.toggle_popup(popup_id));
        }
        let first_frame = self.syntax_focus;
        let mut choice = None;
        let height =
            (response.rect.top() - ui.ctx().screen_rect().top() - 150.0).clamp(80.0, 480.0);
        egui::popup::popup_above_or_below_widget(
            ui,
            popup_id,
            &response,
            egui::AboveOrBelow::Above,
            egui::popup::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                let width = (ui.ctx().screen_rect().width() - 40.0).clamp(240.0, 380.0);
                ui.set_width(width);
                ui.spacing_mut().interact_size.y = 28.0;
                ui.style_mut()
                    .text_styles
                    .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
                ui.label(RichText::new("Syntax for this file").size(16.0).strong());
                let search = ui.add_sized(
                    [width, 30.0],
                    egui::TextEdit::singleline(&mut self.syntax_query)
                        .id(egui::Id::new("syntax-filter"))
                        .hint_text("Filter by language or extension…")
                        .font(egui::FontId::proportional(14.0)),
                );
                if first_frame && !ui.is_sizing_pass() {
                    search.request_focus();
                    self.syntax_focus = false;
                }
                let query_changed = self.syntax_query != self.syntax_last_query;
                if query_changed {
                    self.syntax_matches = syntax::Language::ALL
                        .into_iter()
                        .filter(|language| language.matches_query(&self.syntax_query))
                        .collect();
                    self.syntax_last_query.clone_from(&self.syntax_query);
                }
                let matches = &self.syntax_matches;
                if search.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                    ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));
                    if matches.len() == 1 {
                        choice = Some(Some(matches[0]));
                    } else {
                        search.request_focus();
                    }
                }
                if ui
                    .selectable_label(
                        selected.is_none(),
                        format!("Automatic ({})", detected.label()),
                    )
                    .clicked()
                {
                    choice = Some(None);
                }
                ui.separator();
                let row_height = 28.0;
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt("syntax-choices")
                    .max_height(height)
                    .auto_shrink([false, false]);
                if first_frame || query_changed {
                    let offset = if first_frame && self.syntax_query.is_empty() {
                        let index = matches
                            .iter()
                            .position(|&language| language == selected.unwrap_or(detected))
                            .unwrap_or(0);
                        (index as f32 * (row_height + ui.spacing().item_spacing.y) - height / 2.0)
                            .max(0.0)
                    } else {
                        0.0
                    };
                    scroll = scroll.vertical_scroll_offset(offset);
                }
                if matches.is_empty() {
                    scroll.show(ui, |ui| {
                        ui.label("No matching syntax");
                    });
                } else {
                    scroll.show_rows(ui, row_height, matches.len(), |ui, visible| {
                        for &language in &matches[visible] {
                            let row = ui
                                .push_id(language.id(), |ui| {
                                    ui.selectable_label(
                                        selected == Some(language),
                                        language.label(),
                                    )
                                })
                                .inner;
                            if row.clicked() {
                                choice = Some(Some(language));
                            }
                        }
                    });
                }
            },
        );
        if let Some(language) = choice {
            ui.memory_mut(|memory| memory.close_popup());
            self.set_syntax(language);
            self.focus_editor = true;
        }
        if !ui.memory(|memory| memory.is_popup_open(popup_id)) {
            self.syntax_query.clear();
            self.syntax_focus = true;
        }
        response.on_hover_text(format!(
            "Change syntax · {}\nApplies only to this file and is kept in recovery.",
            if selected.is_some() {
                "manually selected"
            } else {
                "detected from filename"
            },
        ));
    }
    fn buffer_sidebar(&mut self, ctx: &egui::Context) {
        if !self.settings.show_sidebar || self.distraction_free {
            return;
        }
        let theme = self.theme.clone();
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
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!(
                                "CLOSED FILES    {}",
                                self.workspace.closed.len()
                            ))
                            .size(10.0)
                            .color(theme.muted.color()),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Browse and reopen closed files")
                    .clicked()
                {
                    self.shelf_open = true;
                }
                ui.add_enabled_ui(!self.busy && !self.closing, |ui| self.folder_tree(ui));
            });
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
                    let input_id = egui::Id::new(FIND_INPUT);
                    let focused = ctx.memory(|m| m.has_focus(input_id));
                    // Shift+Enter searches backwards, like Sublime Text.
                    let previous =
                        focused && ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Enter));
                    let enter =
                        focused && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                    let response = bidi_view::field(ui, &mut self.query, |edit| {
                        edit.id(input_id)
                            .desired_width(width)
                            .hint_text("Search text or regular expression")
                    });
                    if response.changed() {
                        self.find_as_you_type();
                    }
                    if self.find_focus {
                        response.request_focus();
                        self.find_focus = false;
                    }
                    let clicked = ui
                        .add_enabled(!self.busy && !self.closing, egui::Button::new("Find next"))
                        .clicked();
                    if (clicked || enter) && !self.busy && !self.closing {
                        self.find(ctx);
                    } else if previous && !self.busy && !self.closing {
                        self.find_advanced(ctx, true);
                    }
                    ui.toggle_value(&mut self.replace_open, "Replace");
                    if controls::close_button(ui, "Close search · Esc").clicked() {
                        self.find_open = false;
                        self.focus_editor = true;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    let options = [
                        ui.checkbox(&mut self.quality.options.regex, "Regex"),
                        ui.checkbox(&mut self.quality.options.case_sensitive, "Case sensitive"),
                        ui.checkbox(&mut self.quality.options.whole_word, "Whole words"),
                    ];
                    if options.iter().any(|r| r.changed()) {
                        self.find_as_you_type();
                    }
                    ui.add_enabled_ui(self.quality.scope.is_some(), |ui| {
                        ui.checkbox(&mut self.quality.selection_only, "Selection only");
                    });
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Previous"))
                        .clicked()
                    {
                        self.find_advanced(ctx, true);
                    }
                    ui.label(&self.quality.match_status);
                });
                if self.replace_open {
                    ui.horizontal(|ui| {
                        ui.label("With");
                        let width = (ui.available_width() - 150.0).max(100.0);
                        let response = bidi_view::field(ui, &mut self.replacement, |edit| {
                            edit.desired_width(width).hint_text("Replacement text")
                        });
                        if response.lost_focus()
                            && ui.input(|i| i.key_pressed(Key::Enter))
                            && !self.busy
                            && !self.closing
                        {
                            self.find(ctx);
                            response.request_focus();
                        }
                        if ui
                            .add_enabled(
                                self.command_enabled(Command::Replace),
                                egui::Button::new("Replace selected"),
                            )
                            .clicked()
                        {
                            self.replace_selection();
                            self.focus_editor = false;
                        }
                        if ui
                            .add_enabled(
                                self.command_enabled(Command::Replace),
                                egui::Button::new("Replace all"),
                            )
                            .clicked()
                        {
                            self.replace_advanced(true);
                            self.focus_editor = false;
                        }
                    });
                }
            });
    }
}

const NOTICE_TIME: std::time::Duration = std::time::Duration::from_secs(5);

fn command_group(
    ui: &mut egui::Ui,
    app: &HeditApp,
    title: &str,
    commands: &[Command],
    action: &mut Option<Command>,
) {
    ui.menu_button(title, |ui| {
        ui.set_min_width(250.0);
        for &command in commands {
            if menu_item(ui, app, command) {
                *action = Some(command);
                ui.close_menu();
            }
        }
    });
}
impl HeditApp {
    fn edit_menu(&self, ui: &mut egui::Ui, action: &mut Option<Command>) {
        for command in [Command::Undo, Command::Redo] {
            if menu_item(ui, self, command) {
                *action = Some(command);
                ui.close_menu();
            }
        }
        ui.separator();
        for command in [
            Command::Cut,
            Command::Copy,
            Command::Paste,
            Command::PasteIndent,
            Command::PasteHistory,
        ] {
            if menu_item(ui, self, command) {
                *action = Some(command);
                ui.close_menu();
            }
        }
        ui.separator();
        command_group(
            ui,
            self,
            "Selection",
            &[
                Command::SelectAll,
                Command::SelectLine,
                Command::NextOccurrence,
                Command::SkipOccurrence,
                Command::AllOccurrences,
                Command::SplitLines,
                Command::AddCursorAbove,
                Command::AddCursorBelow,
                Command::Rectangle,
                Command::SingleSelection,
                Command::ExpandSelection,
                Command::ExpandToBrackets,
                Command::ExpandToIndentation,
                Command::JumpToBracket,
                Command::SoftUndo,
                Command::SoftRedo,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Line",
            &[
                Command::Duplicate,
                Command::DeleteLine,
                Command::InsertBefore,
                Command::InsertAfter,
                Command::MoveUp,
                Command::MoveDown,
                Command::JoinLines,
                Command::DeleteToLineStart,
                Command::DeleteToLineEnd,
                Command::SortLines,
                Command::SortLinesIgnoreCase,
                Command::ReverseLines,
                Command::UniqueLines,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Text",
            &[
                Command::Uppercase,
                Command::Lowercase,
                Command::TitleCase,
                Command::SwapCase,
                Command::Transpose,
                Command::WrapLines,
                Command::Reindent,
                Command::TrimWhitespace,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Line endings",
            &[Command::ToLf, Command::ToCrlf],
            action,
        );
        ui.menu_button("Indentation", |ui| {
            egui::ScrollArea::vertical()
                .id_salt("indent-menu-scroll")
                .max_height((ui.ctx().screen_rect().height() - 70.0).max(140.0))
                .show(ui, |ui| {
                    for commands in [
                        &[Command::Indent, Command::Unindent][..],
                        &[Command::IndentSpaces, Command::IndentTabs][..],
                        &[Command::Indent2, Command::Indent4, Command::Indent8][..],
                        &[Command::ToSpaces, Command::ToTabs][..],
                        &[
                            Command::AutoIndent,
                            Command::SmartIndent,
                            Command::SoftTabs,
                            Command::DetectIndent,
                        ][..],
                    ] {
                        for &command in commands {
                            if menu_item(ui, self, command) {
                                *action = Some(command);
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                    }
                });
        });
        ui.separator();
        for command in [Command::ToggleComment, Command::ToggleBlockComment] {
            if menu_item(ui, self, command) {
                *action = Some(command);
                ui.close_menu();
            }
        }
    }
    fn view_menu(&self, ui: &mut egui::Ui, action: &mut Option<Command>) {
        command_group(
            ui,
            self,
            "Layout",
            &[
                Command::SinglePane,
                Command::SplitColumns,
                Command::Columns3,
                Command::Columns4,
                Command::SplitRows,
                Command::Rows3,
                Command::Grid,
                Command::FocusPane,
                Command::MovePane,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Code folding",
            &[
                Command::Fold,
                Command::Unfold,
                Command::FoldAll,
                Command::UnfoldAll,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Side bar",
            &[Command::Sidebar, Command::FolderSidebar],
            action,
        );
        command_group(
            ui,
            self,
            "Editor",
            &[
                Command::Tabs,
                Command::StatusBar,
                Command::LineNumbers,
                Command::CurrentLine,
                Command::Whitespace,
                Command::IndentGuides,
                Command::Minimap,
                Command::Wrap,
                Command::ToggleSpellCheck,
                Command::NextMisspelling,
                Command::PreviousMisspelling,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Font size",
            &[Command::ZoomIn, Command::ZoomOut, Command::ResetZoom],
            action,
        );
        command_group(
            ui,
            self,
            "Ruler",
            &[
                Command::RulerNone,
                Command::Ruler80,
                Command::Ruler100,
                Command::Ruler120,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Word wrap column",
            &[
                Command::WrapWindow,
                Command::Wrap80,
                Command::Wrap100,
                Command::Wrap120,
            ],
            action,
        );
        ui.separator();
        for command in [
            Command::Fullscreen,
            Command::DistractionFree,
            Command::Follow,
        ] {
            if menu_item(ui, self, command) {
                *action = Some(command);
                ui.close_menu();
            }
        }
    }
}
