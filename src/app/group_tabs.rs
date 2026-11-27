//! Per-group tab strips contain IDs and derived geometry, never text buffers.
use super::*;
use hedit::icons::{self, Icon};

impl HeditApp {
    pub(super) fn finish_tab_drag(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.pointer.any_released() || !i.pointer.any_down()) {
            self.quality.tab_drag = None;
            self.quality.tab_drag_group = None;
        }
    }
    pub(super) fn group_tabs(&mut self, ui: &mut egui::Ui, group: usize, focused: usize) -> bool {
        if !self.settings.show_tab_bar || self.distraction_free {
            return false;
        }
        let metrics = hedit::ui_style::Metrics::for_ui(ui);
        let theme = self.theme.clone();
        self.tab_strip.sync(ui, &self.workspace);
        let lookup: std::collections::HashMap<_, _> = self
            .workspace
            .open
            .iter()
            .enumerate()
            .map(|(i, b)| (b.view.id, i))
            .collect();
        let indices: Vec<_> = self
            .group_tab_ids(group)
            .iter()
            .filter_map(|id| lookup.get(id).copied())
            .collect();
        let mut offsets = Vec::with_capacity(indices.len() + 1);
        offsets.push(0.0);
        for &index in &indices {
            offsets.push(offsets.last().copied().unwrap() + self.tab_strip.entry_width(index));
        }
        let total = offsets.last().copied().unwrap_or(0.0);
        let mut select = None;
        let mut close = None;
        let mut action = None;
        let mut drop = None;
        let mut focus = false;
        let review_here = self.source_control.review_tab
            && self.source_control.review_group == Some(self.panes.focused_id());
        let mut review = false;
        let mut close_review = false;
        let bar = ui
            .horizontal(|ui| {
                focus |= ui
                    .selectable_label(group == focused, format!("Group {}", group + 1))
                    .clicked();
                egui::ScrollArea::horizontal()
                    .id_salt(("group-tabs", group))
                    .max_height(metrics.tab)
                    .auto_shrink([false, false])
                    .show_viewport(ui, |ui, viewport| {
                        let first = offsets
                            .partition_point(|x| *x < viewport.left())
                            .saturating_sub(2)
                            .min(indices.len());
                        let end = (offsets.partition_point(|x| *x <= viewport.right()) + 1)
                            .min(indices.len());
                        let origin = ui.cursor().min;
                        if group == focused {
                            if let Some(position) =
                                indices.iter().position(|i| *i == self.workspace.active)
                            {
                                if self.tab_strip.reveal(
                                    self.active().view.id,
                                    position,
                                    viewport.width(),
                                ) {
                                    ui.scroll_to_rect(
                                        egui::Rect::from_min_max(
                                            origin + egui::vec2(offsets[position], 0.0),
                                            origin + egui::vec2(offsets[position + 1], metrics.tab),
                                        ),
                                        None,
                                    );
                                }
                            }
                        }
                        ui.set_min_size(egui::vec2(
                            total + metrics.control + if review_here { 160.0 } else { 0.0 },
                            metrics.tab,
                        ));
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.horizontal(|ui| {
                            ui.add_space(offsets[first]);
                            for (position, &index) in
                                indices.iter().enumerate().take(end).skip(first)
                            {
                                let buffer = &self.workspace.open[index];
                                let id = buffer.view.id;
                                let selected = index == self.workspace.active
                                    && !(review_here && self.source_control.review_selected);
                                let label = self.tab_strip.label(index).to_owned();
                                let button = self.tab_strip.button(
                                    ui,
                                    index,
                                    if selected {
                                        theme.text.color()
                                    } else {
                                        theme.muted.color()
                                    },
                                    theme.muted.color(),
                                    self.quality.preview == Some(id),
                                );
                                let path = buffer.page.info.path.clone();
                                let dirty = buffer.page.info.dirty;
                                let can_close = self.can_close_group_tab(index);
                                let menu: Vec<_> = [
                                    Command::PinTab,
                                    Command::Close,
                                    Command::CloseOthers,
                                    Command::CloseRight,
                                    Command::CopyFileLocation,
                                    Command::SelectForCompare,
                                    Command::CompareWithSelected,
                                    Command::MovePane,
                                ]
                                .into_iter()
                                .map(|command| {
                                    (
                                        command,
                                        match command {
                                            Command::CopyFileLocation => {
                                                path.is_some() && buffer.page.start == 0
                                            }
                                            Command::SelectForCompare => {
                                                path.is_some() && self.workspace.remote.is_none()
                                            }
                                            Command::CompareWithSelected => {
                                                self.workspace.remote.is_none()
                                                    && path.as_deref().is_some_and(|path| {
                                                        self.quality
                                                            .compare_source
                                                            .as_deref()
                                                            .is_some_and(|first| {
                                                                first != std::path::Path::new(path)
                                                            })
                                                    })
                                            }
                                            _ => self.command_enabled(command),
                                        },
                                        self.shortcut_text(command),
                                    )
                                })
                                .collect();
                                let shown = ui.push_id(("group-tab", group, id), |ui| {
                                    egui::Frame::new()
                                        .fill(if selected {
                                            theme.background.color()
                                        } else {
                                            theme.panel.color()
                                        })
                                        .inner_margin(egui::Margin::symmetric(10, 3))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                let response = ui.add(button).on_hover_text(
                                                    path.as_deref().unwrap_or(
                                                        "Unsaved file · included in recovery",
                                                    ),
                                                );
                                                response.widget_info(|| {
                                                    egui::WidgetInfo::labeled(
                                                        egui::Role::Button,
                                                        true,
                                                        &label,
                                                    )
                                                });
                                                if response.clicked() {
                                                    select = Some(index);
                                                }
                                                if response.drag_started() {
                                                    self.quality.tab_drag = Some(id);
                                                    self.quality.tab_drag_group = Some(group);
                                                }
                                                if response.hovered()
                                                    && ui.input(|i| i.pointer.any_released())
                                                {
                                                    if let Some(id) = self.quality.tab_drag.take() {
                                                        drop = Some((
                                                            self.quality
                                                                .tab_drag_group
                                                                .take()
                                                                .unwrap_or(group),
                                                            id,
                                                            position,
                                                        ));
                                                    }
                                                }
                                                response.context_menu(|ui| {
                                                    for (command, enabled, shortcut) in &menu {
                                                        if ui
                                                            .add_enabled(
                                                                *enabled,
                                                                egui::Button::new(command.label())
                                                                    .shortcut_text(shortcut),
                                                            )
                                                            .clicked()
                                                        {
                                                            select = Some(index);
                                                            action = Some(*command);
                                                            ui.close();
                                                        }
                                                    }
                                                });
                                                if can_close
                                                    && (controls::tab_close_button(ui, dirty)
                                                        .on_hover_text(format!("Close {label}"))
                                                        .clicked()
                                                        || response.clicked_by(
                                                            egui::PointerButton::Middle,
                                                        ))
                                                {
                                                    close = Some(index);
                                                }
                                            });
                                        })
                                        .response
                                });
                                if self.tab_strip.measured(index, shown.inner.rect.width()) {
                                    ui.ctx().request_repaint();
                                }
                                if selected {
                                    ui.painter().hline(
                                        shown.inner.rect.x_range(),
                                        shown.inner.rect.top(),
                                        egui::Stroke::new(1.0_f32, theme.accent.color()),
                                    );
                                }
                            }
                            ui.add_space(total - offsets[end]);
                            if review_here {
                                ui.horizontal(|ui| {
                                    let tab = ui.selectable_label(
                                        self.source_control.review_selected,
                                        "Git review",
                                    );
                                    if std::mem::take(&mut self.source_control.reveal_review) {
                                        tab.scroll_to_me(Some(egui::Align::Max));
                                    }
                                    review = tab.clicked();
                                    close_review =
                                        controls::close_button(ui, "Close Git review").clicked();
                                });
                            }
                            if icons::button(ui, Icon::Add, "New file in this group").clicked() {
                                action = Some(Command::New);
                                focus = true;
                            }
                        });
                    });
            })
            .response;
        if ui.rect_contains_pointer(bar.rect)
            && ui.input(|i| i.pointer.any_released())
            && drop.is_none()
        {
            if let Some(id) = self.quality.tab_drag.take() {
                drop = Some((
                    self.quality.tab_drag_group.take().unwrap_or(group),
                    id,
                    indices.len(),
                ));
            }
        }
        if self.busy || self.closing {
            return false;
        }
        if review {
            self.activate_review();
            focus = true;
        }
        if close_review {
            self.source_control.review_tab = false;
            self.leave_review();
            self.focus_editor = true;
            focus = true;
        }
        if let Some((source, id, position)) = drop {
            self.move_group_tab(source, group, id, position);
            return true;
        }
        if let Some(index) = select {
            self.select_tab(index);
            focus = true;
        }
        if let Some(index) = close {
            self.select_tab(index);
            self.request_close(vec![self.workspace.open[index].view.id]);
            focus = true;
        }
        if let Some(command) = action {
            self.execute(ui.ctx(), command);
            focus = true;
        }
        focus
    }
}
