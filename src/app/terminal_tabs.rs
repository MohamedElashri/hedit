//! Compact terminal chrome: one row, with secondary controls in menus.
use super::*;
use hedit::icons::Icon;

#[derive(Default)]
pub(super) struct Actions {
    pub activate: Option<usize>,
    pub close: Option<usize>,
    pub restart: Option<usize>,
    pub stop: Option<usize>,
    pub reorder: Option<(u64, usize)>,
    pub new: bool,
    pub hide: bool,
    pub copy: bool,
    pub focus: bool,
}

pub(super) fn bar(ui: &mut egui::Ui, state: &mut TerminalState, theme: &Theme) -> Actions {
    let metrics = hedit::ui_style::Metrics::for_ui(ui);
    let control = metrics.control;
    let mut actions = Actions::default();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), metrics.tab),
        egui::Sense::hover(),
    );
    ui.interact(
        rect,
        egui::Id::new("terminal-tab-bar"),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, 0.0, theme.panel.color());
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0_f32, theme.border.color()),
    );
    let controls_width = control * 2.0 + 8.0;
    let tabs_rect = egui::Rect::from_min_max(
        rect.min + egui::vec2(4.0, 0.0),
        egui::pos2(
            (rect.right() - controls_width).max(rect.left() + 1.0),
            rect.bottom(),
        ),
    );
    let tab_width = (tabs_rect.width() / state.tabs.len().max(1) as f32).clamp(96.0, 200.0);
    let width_id = ui.make_persistent_id("terminal-tabs-width");
    let resized = ui.data_mut(|data| {
        let previous = data.get_temp::<f32>(width_id);
        data.insert_temp(width_id, tabs_rect.width());
        previous.is_none_or(|width| (width - tabs_rect.width()).abs() > 0.5)
    });
    let mut strip = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("terminal-tabs")
            .max_rect(tabs_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    strip.set_clip_rect(ui.clip_rect().intersect(tabs_rect));
    egui::ScrollArea::horizontal()
        .id_salt("terminal-tab-strip")
        .animated(false)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .auto_shrink([false, false])
        .show(&mut strip, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal(|ui| {
                for (index, tab) in state.tabs.iter_mut().enumerate() {
                    let active = state.active == index;
                    let title = title(tab);
                    let unread = !active && tab.seen != tab.session.output_generation;
                    let (response, rect) = tab_button(
                        ui,
                        theme,
                        tab,
                        TabView {
                            width: tab_width,
                            active,
                            unread,
                            title: &title,
                            index,
                        },
                        &mut actions,
                    );
                    if response.drag_started() {
                        state.dragged_tab = Some(tab.id);
                    }
                    if ui.rect_contains_pointer(rect) && ui.input(|i| i.pointer.any_released()) {
                        if let Some(id) = state.dragged_tab.take() {
                            actions.reorder = Some((id, index));
                        }
                    }
                    if response.clicked() {
                        actions.activate = Some(index);
                    }
                    if active && (state.focus || resized) {
                        ui.scroll_to_rect(rect, None);
                    }
                    response.context_menu(|ui| tab_menu(ui, tab, index, &mut actions));
                }
            });
        });
    if ui.input(|i| i.pointer.any_released()) {
        state.dragged_tab = None;
    }
    let controls_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - controls_width + 2.0, rect.top()),
        rect.max,
    );
    let mut controls = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("terminal-controls")
            .max_rect(controls_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    controls.spacing_mut().item_spacing.x = 0.0;
    let enabled = state.tabs.len() < MAX_TERMINAL_TABS;
    controls.add_enabled_ui(enabled, |ui| {
        actions.new = icon_button(
            ui,
            theme,
            "terminal-new-tab",
            "New terminal tab · Ctrl+Shift+T (Cmd+Shift+T on macOS)",
            Icon::Add,
        )
        .clicked();
    });
    controls.scope(|ui| {
        ui.spacing_mut().button_padding = egui::vec2(6.0, 2.0);
        ui.spacing_mut().interact_size = egui::vec2(control, control);
        ui.visuals_mut().widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
        ui.visuals_mut().widgets.inactive.bg_stroke = egui::Stroke::NONE;
        let response = egui::menu::menu_custom_button(
            ui,
            egui::Button::new("")
                .frame(false)
                .min_size(egui::vec2(control, control)),
            |ui| {
                ui.set_max_width(300.0);
                ui.weak("Terminal tabs");
                egui::ScrollArea::vertical()
                    .id_salt("terminal-tab-menu-list")
                    .max_height((ui.ctx().screen_rect().height() * 0.35).clamp(80.0, 240.0))
                    .show(ui, |ui| {
                        for (index, tab) in state.tabs.iter().enumerate() {
                            if ui
                                .selectable_label(
                                    index == state.active,
                                    hedit::bidi::display(&title(tab)),
                                )
                                .clicked()
                            {
                                actions.activate = Some(index);
                                ui.close_menu();
                            }
                        }
                    });
                ui.separator();
                if let Some(tab) = state.tabs.get_mut(state.active) {
                    tab_menu(ui, tab, state.active, &mut actions);
                    ui.separator();
                    if ui.button("Copy selection").clicked() {
                        actions.copy = true;
                        ui.close_menu();
                    }
                }
                if ui.button("Hide terminal").clicked() {
                    actions.hide = true;
                    ui.close_menu();
                }
            },
        )
        .response;
        Icon::More.paint(
            ui,
            egui::Rect::from_center_size(response.rect.center(), egui::vec2(16.0, 16.0)),
            theme.muted.color(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Terminal actions")
        });
        response.on_hover_text("Terminal actions and all tabs");
    });
    actions
}

fn title(tab: &TerminalTab) -> String {
    if !tab.name.trim().is_empty() {
        return tab.name.clone();
    }
    let title = tab.session.vt.title().unwrap_or("");
    if !title.trim().is_empty() {
        return title.chars().filter(|c| !c.is_control()).take(60).collect();
    }
    format!("Terminal {}", tab.id)
}

struct TabView<'a> {
    width: f32,
    active: bool,
    unread: bool,
    title: &'a str,
    index: usize,
}

fn tab_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    tab: &TerminalTab,
    view: TabView<'_>,
    actions: &mut Actions,
) -> (egui::Response, egui::Rect) {
    let TabView {
        width,
        active,
        unread,
        title,
        index,
    } = view;
    let metrics = hedit::ui_style::Metrics::for_ui(ui);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, metrics.tab), egui::Sense::hover());
    let close_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - metrics.control / 2.0 - 4.0, rect.center().y),
        egui::vec2(metrics.control, metrics.control),
    );
    let body = egui::Rect::from_min_max(rect.min, egui::pos2(close_rect.left(), rect.bottom()));
    let response = ui.interact(
        body,
        egui::Id::new(("terminal-tab", tab.id)),
        egui::Sense::click_and_drag(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, active, title)
    });
    let hovered = ui.rect_contains_pointer(rect);
    let background = egui::Rect::from_min_max(
        rect.min + egui::vec2(1.0, 3.0),
        rect.max - egui::vec2(1.0, 0.0),
    );
    let rounding = egui::CornerRadius::ZERO;
    if active {
        ui.painter().hline(
            background.x_range(),
            background.top(),
            egui::Stroke::new(1.0_f32, theme.accent.color()),
        );
    }
    if active || hovered || response.has_focus() {
        ui.painter().rect_filled(
            background,
            rounding,
            if active {
                theme.surface.color()
            } else {
                theme.hover().color()
            },
        );
    } else {
        ui.painter().vline(
            rect.right(),
            rect.top() + 9.0..=rect.bottom() - 8.0,
            egui::Stroke::new(1.0_f32, theme.border.color().gamma_multiply(0.7)),
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            background.shrink(1.0),
            rounding,
            egui::Stroke::new(1.0_f32, theme.accent.color()),
            egui::StrokeKind::Inside,
        );
    }
    if unread {
        ui.painter().circle_filled(
            egui::pos2(rect.left() + 10.0, rect.center().y + 1.0),
            2.5,
            theme.accent.color(),
        );
    } else if !tab.session.running {
        ui.painter().rect_stroke(
            egui::Rect::from_center_size(
                egui::pos2(rect.left() + 10.0, rect.center().y + 1.0),
                egui::vec2(5.0, 5.0),
            ),
            1.0,
            egui::Stroke::new(1.0_f32, theme.muted.color()),
            egui::StrokeKind::Inside,
        );
    }
    let color = if active {
        theme.text.color()
    } else {
        theme.muted.color()
    };
    let mut job = egui::text::LayoutJob::simple_singleline(
        hedit::bidi::display(title).into_owned(),
        FontId::proportional(metrics.small),
        color,
    );
    job.wrap.max_width = (body.width() - 26.0).max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts(|fonts| fonts.layout_job(job));
    ui.painter().galley(
        egui::pos2(
            rect.left() + 20.0,
            rect.center().y + 1.0 - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
    if response.clicked_by(egui::PointerButton::Middle) {
        actions.close = Some(index);
    }
    if active || hovered || response.has_focus() {
        let close = ui.interact(
            close_rect,
            egui::Id::new(("terminal-tab-close", tab.id)),
            egui::Sense::click(),
        );
        close.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Close {title}"))
        });
        if close.hovered() || close.has_focus() {
            ui.painter()
                .rect_filled(close_rect, 3.0, theme.hover().color());
        }
        Icon::Close.paint(
            ui,
            egui::Rect::from_center_size(close_rect.center(), egui::vec2(16.0, 16.0)),
            if close.hovered() || close.has_focus() {
                theme.text.color()
            } else {
                theme.muted.color()
            },
        );
        if close.clicked() || close.clicked_by(egui::PointerButton::Middle) {
            actions.close = Some(index);
        }
        close.on_hover_text(format!("Close {title} · middle-click also closes a tab"));
    }
    (
        response.on_hover_text(format!(
            "{title}\n{}\n{}",
            tab.session.directory.display(),
            tab.session.status
        )),
        rect,
    )
}

fn tab_menu(ui: &mut egui::Ui, tab: &mut TerminalTab, index: usize, actions: &mut Actions) {
    ui.set_max_width(300.0);
    ui.weak(hedit::bidi::display(
        &tab.session.directory.display().to_string(),
    ));
    ui.weak(&tab.session.status);
    ui.separator();
    ui.label("Tab name");
    let rename = ui.add(
        egui::TextEdit::singleline(&mut tab.name)
            .hint_text("Use shell title")
            .char_limit(60),
    );
    if rename.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        actions.focus = true;
        ui.close_menu();
    }
    if !tab.name.is_empty() && ui.button("Use shell title").clicked() {
        tab.name.clear();
        actions.focus = true;
        ui.close_menu();
    }
    ui.separator();
    if ui.button("Restart shell").clicked() {
        actions.restart = Some(index);
        ui.close_menu();
    }
    if tab.session.running && ui.button("Stop shell").clicked() {
        actions.stop = Some(index);
        ui.close_menu();
    }
    if ui.button("Close tab").clicked() {
        actions.close = Some(index);
        ui.close_menu();
    }
}

fn icon_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    id: &str,
    hint: &str,
    icon: Icon,
) -> egui::Response {
    let control = hedit::ui_style::Metrics::for_ui(ui).control;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(control, control), egui::Sense::hover());
    let response = ui.interact(rect, egui::Id::new(id), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            hint.split('·').next().unwrap_or(hint).trim(),
        )
    });
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 3.0, theme.hover().color());
    }
    let color = if !ui.is_enabled() {
        theme.muted.color().gamma_multiply(0.4)
    } else if response.hovered() {
        theme.text.color()
    } else {
        theme.muted.color()
    };
    icon.paint(
        ui,
        egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0)),
        color,
    );
    response.on_hover_text(hint)
}
