//! Choosing the editor font from the installed families, in the command
//! palette or in Settings → Editor.
use super::*;

/// What the font list shows beside a family: monospace and other scripts.
pub(super) fn family_detail(family: &hedit::fonts::Family) -> String {
    let mut parts: Vec<&str> = vec![];
    if family.monospace {
        parts.push("monospace");
    }
    parts.extend(&family.scripts);
    parts.join(", ")
}

/// Shown under the font choice: Latin letters, look-alike characters, code
/// punctuation, and other scripts, which come from fallback fonts when the
/// chosen font lacks them.
pub(super) const SAMPLE: &str = "Aa Bb 0O 1lI {}[]=> العربية עברית 日本語 한국어";

impl HeditApp {
    /// Use the shared background inventory without loading each family for preview.
    pub(super) fn ui_font_choice(&mut self, ui: &mut egui::Ui) -> Option<String> {
        self.request_font_families(ui.ctx());
        let id = egui::Id::unique("ui-font-filter");
        let mut filter: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
        let mut choice = None;
        egui::ComboBox::from_id_salt("installed-ui-font")
            .selected_text("Choose installed font…")
            .width(ui.available_width().min(360.0))
            .height(240.0)
            .truncate()
            .show_ui(ui, |ui| {
                ui.set_max_width(360.0);
                ui.add(
                    egui::TextEdit::singleline(&mut filter)
                        .id(egui::Id::unique("ui-font-filter-input"))
                        .hint_text("Filter fonts by family or script")
                        .desired_width(f32::INFINITY),
                );
                if ui
                    .selectable_label(
                        self.settings.ui_font_face.is_empty(),
                        "Built-in (Noto Sans)",
                    )
                    .clicked()
                {
                    choice = Some(String::new());
                }
                match &self.font_families {
                    None => {
                        ui.weak("Listing installed fonts…");
                    }
                    Some(families) => {
                        let mut matches = 0;
                        for family in families {
                            let detail = family_detail(family);
                            if !filter.is_empty()
                                && hedit::folder::fuzzy_match(
                                    &format!("{} {detail}", family.name),
                                    &filter,
                                )
                                .is_none()
                            {
                                continue;
                            }
                            matches += 1;
                            if ui
                                .add(
                                    egui::Button::new(&family.name)
                                        .selected(self.settings.ui_font_face == family.name)
                                        .frame(false)
                                        .truncate(),
                                )
                                .on_hover_text(detail)
                                .clicked()
                            {
                                choice = Some(family.name.clone());
                            }
                        }
                        if matches == 0 {
                            ui.weak("No matching installed fonts.");
                        }
                    }
                }
                if choice.is_some() {
                    ui.close();
                }
            });
        ui.data_mut(|d| d.insert_temp(id, filter));
        choice
    }
    /// List installed families off the UI thread, once.
    pub(super) fn request_font_families(&mut self, ctx: &egui::Context) {
        if self.font_families.is_some() || self.fonts_listing {
            return;
        }
        self.fonts_listing = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(Ok(Action::FontFamilies(hedit::fonts::families())));
            ctx.request_repaint();
        });
    }
    /// Choose editor font…: the built-in font and every installed family.
    pub(super) fn open_font_picker(&mut self, ctx: &egui::Context) {
        let Some(families) = &self.font_families else {
            self.request_font_families(ctx);
            self.font_picker_pending = true;
            self.notice = "Listing installed fonts…".into();
            return;
        };
        let mut items = vec![(
            "Built-in (Hack)".to_owned(),
            "monospace".to_owned(),
            commands::Pick::Font(String::new()),
        )];
        items.extend(families.iter().map(|f| {
            (
                f.name.clone(),
                family_detail(f),
                commands::Pick::Font(f.name.clone()),
            )
        }));
        self.open_pick_list(
            ctx,
            "Editor font",
            "Filter fonts, such as mono or arabic…",
            items,
        );
    }
    /// The font field in Settings → Editor: a searchable list of families,
    /// with a path or name typed directly still accepted.
    pub(super) fn font_setting(&mut self, ui: &mut egui::Ui) {
        if self.font_families.is_none() {
            self.request_font_families(ui.ctx());
        }
        let current = if self.settings.font_face.trim().is_empty() {
            "Built-in (Hack)".to_owned()
        } else {
            self.settings.font_face.clone()
        };
        let filter_id = egui::Id::unique("font-filter");
        let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
        let mut chosen = None;
        egui::ComboBox::from_id_salt("font-face")
            .selected_text(current)
            .width(240.0)
            .height(320.0)
            .show_ui(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut filter)
                        .hint_text("Filter, such as mono or arabic")
                        .desired_width(f32::INFINITY),
                );
                if ui
                    .selectable_label(self.settings.font_face.is_empty(), "Built-in (Hack)")
                    .clicked()
                {
                    chosen = Some(String::new());
                }
                match &self.font_families {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.weak("Listing installed fonts…");
                        });
                    }
                    Some(families) => {
                        for family in families {
                            let detail = family_detail(family);
                            if !filter.is_empty()
                                && hedit::folder::fuzzy_match(
                                    &format!("{} {detail}", family.name),
                                    &filter,
                                )
                                .is_none()
                            {
                                continue;
                            }
                            let selected = self.settings.font_face == family.name;
                            let label = if detail.is_empty() {
                                family.name.clone()
                            } else {
                                format!("{}  ·  {detail}", family.name)
                            };
                            if ui.selectable_label(selected, label).clicked() {
                                chosen = Some(family.name.clone());
                            }
                        }
                    }
                }
            });
        ui.data_mut(|d| d.insert_temp(filter_id, filter));
        if let Some(name) = chosen {
            self.settings.font_face = name;
        }
    }
}
