//! Shared host-rendered extension results. All output is bounded plain text.
use crate::plugin_runtime::{
    self,
    service::{Action, ResultSet},
};
use eframe::egui;
fn rows(ui: &mut egui::Ui, id: &str, count: usize, mut row: impl FnMut(&mut egui::Ui, usize)) {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(280.0)
        .show_rows(
            ui,
            ui.text_style_height(&egui::TextStyle::Body) + 8.0,
            count,
            |ui, visible| {
                for index in visible {
                    row(ui, index);
                }
            },
        );
}
pub(super) fn plain(ui: &mut egui::Ui, text: &str, page: &mut usize) {
    // Bounded pages also handle a single adversarial line without laying out 16 KiB.
    let pages = text.chars().count().max(1).div_ceil(1024);
    *page = (*page).min(pages - 1);
    let start = text
        .char_indices()
        .nth(*page * 1024)
        .map_or(text.len(), |p| p.0);
    let end = text[start..]
        .char_indices()
        .nth(1024)
        .map_or(text.len(), |p| start + p.0);
    indexed_page(ui, &text[start..end], page, pages);
}
pub(super) fn indexed_page(ui: &mut egui::Ui, text: &str, page: &mut usize, pages: usize) {
    *page = (*page).min(pages.max(1) - 1);
    egui::ScrollArea::both()
        .id_salt(ui.next_auto_id())
        .max_height(160.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.monospace(text);
        });
    if pages > 1 {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(*page > 0, egui::Button::new("Previous"))
                .clicked()
            {
                *page -= 1;
            }
            ui.label(format!("{} / {pages}", *page + 1));
            if ui
                .add_enabled(*page + 1 < pages, egui::Button::new("Next"))
                .clicked()
            {
                *page += 1;
            }
        });
    }
}

pub(super) fn render(
    ui: &mut egui::Ui,
    result: &ResultSet,
    document: &str,
    revision: &str,
    page: &mut usize,
    input: &mut Option<hedit::plugin_options::Value>,
) -> Option<Action> {
    let mut chosen = None;
    let mut button = |ui: &mut egui::Ui, title: &str, action: &dyn Fn() -> Action| {
        if ui.add(egui::Button::new(title).truncate()).clicked() {
            chosen = Some(action());
        }
    };
    match result {
        ResultSet::Closed => {}
        ResultSet::Diagnostics(items) => rows(ui, "diagnostics", items.len(), |ui, index| {
            let item = &items[index];
            ui.horizontal(|ui| {
                if !item.actions.is_empty() {
                    ui.menu_button("Fixes", |ui| {
                        for action in &item.actions {
                            button(ui, &action.title, &|| action.action.clone());
                        }
                    });
                }
                ui.add(egui::Label::new(&item.message).truncate())
                    .on_hover_text(&item.message);
            });
        }),
        ResultSet::Completions(items) => rows(ui, "completions", items.len(), |ui, index| {
            let item = &items[index];
            button(ui, &item.label, &|| {
                Action::Edit(plugin_runtime::sdk::Transaction {
                    document: document.to_owned(),
                    revision: revision.to_owned(),
                    edits: vec![plugin_runtime::sdk::Edit {
                        start: item.range.start,
                        finish: item.range.finish,
                        text: item.insert.clone(),
                    }],
                    selections: None,
                    storage: None,
                })
            });
        }),
        ResultSet::Hover(item) => plain(ui, &item.text, page),
        ResultSet::Symbols(items) => rows(ui, "symbols", items.len(), |ui, index| {
            let item = &items[index];
            button(ui, &item.name, &|| {
                Action::Select(plugin_runtime::sdk::Selection {
                    anchor: item.range.start,
                    head: item.range.finish,
                })
            });
        }),
        ResultSet::Formatting(transaction) => button(ui, "Review formatting", &|| {
            Action::Edit(transaction.clone())
        }),
        ResultSet::CodeActions(items) => rows(ui, "actions", items.len(), |ui, index| {
            let item = &items[index];
            button(ui, &item.title, &|| item.action.clone());
        }),
        ResultSet::Presentation(presentation) => {
            if let Some(status) = &presentation.status {
                ui.label(status);
            }
            if let Some(output) = &presentation.output {
                plain(ui, output, page);
            }
            rows(
                ui,
                "annotations",
                presentation.annotations.len(),
                |ui, index| {
                    let item = &presentation.annotations[index];
                    button(ui, &item.text, &|| {
                        Action::Select(plugin_runtime::sdk::Selection {
                            anchor: item.range.start,
                            head: item.range.finish,
                        })
                    });
                },
            );
            if let Some(list) = &presentation.quick_list {
                ui.label(&list.title);
                rows(ui, "list", list.items.len(), |ui, index| {
                    let item = &list.items[index];
                    button(ui, &item.title, &|| item.action.clone());
                });
            }
            if let Some(prompt) = &presentation.input {
                ui.label(&prompt.title);
                let value = input.get_or_insert_with(|| prompt.default.clone());
                if prompt.choices.is_empty() {
                    match value {
                        hedit::plugin_options::Value::Text(text) => {
                            ui.add(egui::TextEdit::singleline(text).char_limit(256));
                        }
                        hedit::plugin_options::Value::Integer(number) => {
                            ui.add(egui::DragValue::new(number).range(i32::MIN..=i32::MAX));
                        }
                        hedit::plugin_options::Value::Boolean(value) => {
                            ui.checkbox(value, "Enabled");
                        }
                    }
                } else {
                    for option in &prompt.choices {
                        ui.selectable_value(value, option.clone(), format!("{option:?}"));
                    }
                }
                let valid = value.valid()
                    && value.same_type(&prompt.default)
                    && (prompt.choices.is_empty() || prompt.choices.contains(value));
                if ui.add_enabled(valid, egui::Button::new("Submit")).clicked() {
                    chosen = Some(Action::Callback {
                        name: prompt.name.clone(),
                        value: value.clone(),
                    });
                }
            }
        }
    }

    chosen
}
