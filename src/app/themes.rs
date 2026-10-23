//! Theme gallery, quick theme switcher, user theme library, and system appearance.
use super::*;
use hedit::theme::{self as catalog, Group, UserTheme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Slot {
    Dark,
    Light,
}

pub(super) struct Appearance {
    /// Switch between `dark` and `light` with the system appearance.
    pub(super) follow_system: bool,
    pub(super) dark: Theme,
    pub(super) light: Theme,
    /// The slot shown and edited while the Appearance page is open in follow-system mode.
    pub(super) slot: Slot,
    /// The theme when Preferences opened, for Revert.
    pub(super) original: Theme,
    pub(super) hex: Vec<String>,
    pub(super) query: String,
    pub(super) dir: Option<PathBuf>,
    pub(super) library: Vec<UserTheme>,
    pub(super) library_errors: Vec<String>,
    /// The name field of an open “Save as new theme” form.
    pub(super) save_name: Option<String>,
    /// The theme to restore when the quick switcher is dismissed.
    pub(super) preview: Option<Theme>,
    /// Show the color editor instead of the theme gallery.
    pub(super) customizing: bool,
}

const SAMPLE: &str = "#include <cstdio>
// Scale a vector on the GPU
__global__ void scale(float* data, int n) {
    const int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < n) data[i] *= SCALE_FACTOR + 0.5f;
}
struct Result { const char* label = \"done\"; };";

impl Appearance {
    pub(super) fn load(storage: Option<&dyn eframe::Storage>, theme: &Theme) -> Self {
        let stored = |key: &str, fallback: Theme| {
            storage
                .and_then(|s| s.get_string(key))
                .and_then(|json| Theme::from_json(&json).ok())
                .unwrap_or(fallback)
        };
        let dir = if cfg!(test) {
            None
        } else {
            std::env::var_os("HEDIT_THEMES_DIR")
                .map(PathBuf::from)
                .or_else(|| {
                    directories::ProjectDirs::from("", "hedit", "hedit")
                        .map(|d| d.config_dir().join("themes"))
                })
        };
        let mut appearance = Self {
            follow_system: storage
                .and_then(|s| eframe::get_value(s, "theme_follow_system"))
                .unwrap_or(false),
            dark: stored("theme_dark", Theme::default()),
            light: stored("theme_light", Theme::paper()),
            slot: if theme.dark { Slot::Dark } else { Slot::Light },
            original: theme.clone(),
            hex: hex(theme),
            query: String::new(),
            dir,
            library: Vec::new(),
            library_errors: Vec::new(),
            save_name: None,
            preview: None,
            customizing: false,
        };
        appearance.reload();
        appearance
    }
    pub(super) fn save(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "theme_follow_system", &self.follow_system);
        for (key, theme) in [("theme_dark", &self.dark), ("theme_light", &self.light)] {
            if let Ok(json) = theme.to_json() {
                storage.set_string(key, json);
            }
        }
    }
    pub(super) fn reload(&mut self) {
        (self.library, self.library_errors) = match &self.dir {
            Some(dir) => catalog::load_library(dir),
            None => (Vec::new(), Vec::new()),
        };
    }
    /// A built-in or saved theme by name.
    pub(super) fn known(&self, name: &str) -> Option<Theme> {
        catalog::builtin(name).or_else(|| {
            self.library
                .iter()
                .find(|t| t.theme.name == name)
                .map(|t| t.theme.clone())
        })
    }
    fn slot_mut(&mut self, slot: Slot) -> &mut Theme {
        match slot {
            Slot::Dark => &mut self.dark,
            Slot::Light => &mut self.light,
        }
    }
    /// Every selectable theme: built-in dark, built-in light, then saved themes.
    fn all(&self) -> Vec<(Theme, Option<PathBuf>)> {
        catalog::builtins()
            .iter()
            .map(|t| (t.clone(), None))
            .chain(
                self.library
                    .iter()
                    .map(|t| (t.theme.clone(), Some(t.path.clone()))),
            )
            .collect()
    }
}

fn hex(theme: &Theme) -> Vec<String> {
    Role::ALL
        .into_iter()
        .map(|r| theme.color(r).hex())
        .collect()
}

fn matches(theme: &Theme, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || theme.name.to_lowercase().contains(&query)
        || (if theme.dark { "dark" } else { "light" }).contains(&query)
}

impl HeditApp {
    pub(super) fn set_theme(&mut self, ctx: &egui::Context, theme: Theme) {
        self.appearance.hex = hex(&theme);
        self.theme = theme;
        self.theme.apply(ctx);
    }
    /// The slot on screen: the one being edited in Preferences, otherwise the system's.
    fn displayed_slot(&self, ctx: &egui::Context) -> Slot {
        if self.preferences_open && self.preferences_page == PreferencesPage::Appearance {
            self.appearance.slot
        } else if ctx.system_theme() == Some(egui::Theme::Light) {
            Slot::Light
        } else {
            Slot::Dark
        }
    }
    /// Show the slot theme that matches the system appearance.
    pub(super) fn sync_appearance(&mut self, ctx: &egui::Context) {
        if !self.appearance.follow_system || self.appearance.preview.is_some() {
            return;
        }
        let slot = self.displayed_slot(ctx);
        if self.theme != *self.appearance.slot_mut(slot) {
            let wanted = self.appearance.slot_mut(slot).clone();
            self.set_theme(ctx, wanted);
        }
    }
    /// Record edits to the displayed theme in its follow-system slot.
    pub(super) fn store_displayed_theme(&mut self, ctx: &egui::Context) {
        if self.appearance.follow_system {
            let slot = self.displayed_slot(ctx);
            *self.appearance.slot_mut(slot) = self.theme.clone();
        }
    }
    pub(super) fn choose_theme(&mut self, ctx: &egui::Context, theme: Theme) {
        self.appearance.save_name = None;
        self.set_theme(ctx, theme);
        self.store_displayed_theme(ctx);
    }
    pub(super) fn set_follow_system(&mut self, ctx: &egui::Context, follow: bool) {
        self.appearance.follow_system = follow;
        if follow {
            let slot = if self.theme.dark {
                Slot::Dark
            } else {
                Slot::Light
            };
            *self.appearance.slot_mut(slot) = self.theme.clone();
            self.appearance.slot = slot;
            self.sync_appearance(ctx);
        }
    }
    /// After Preferences edits: mark modified built-ins and keep the slot current.
    pub(super) fn theme_edited(&mut self, ctx: &egui::Context, before: &Theme) {
        let recolored = self.theme.dark != before.dark
            || Role::ALL
                .into_iter()
                .any(|role| self.theme.color(role) != before.color(role));
        if recolored && self.theme.name == before.name {
            if let Some(builtin) = catalog::builtin(&self.theme.name) {
                if builtin != self.theme {
                    self.theme.name = format!("{} (modified)", builtin.name);
                }
            }
        }
        self.theme.apply(ctx);
        self.store_displayed_theme(ctx);
    }
    pub(super) fn add_imported_theme(&mut self, ctx: &egui::Context, mut theme: Theme) {
        let Some(dir) = self.appearance.dir.clone() else {
            self.choose_theme(ctx, theme);
            self.notice = "Theme imported.".into();
            return;
        };
        let taken: Vec<_> = self
            .appearance
            .all()
            .into_iter()
            .map(|(t, _)| t.name)
            .collect();
        theme.name = catalog::unique_name(&theme.name, taken.iter().map(String::as_str));
        match catalog::save_to_library(&dir, &theme, None) {
            Ok(_) => {
                self.appearance.reload();
                self.choose_theme(ctx, theme.clone());
                self.notice = format!("Theme “{}” added to your themes.", theme.name);
            }
            Err(error) => self.error = Some(format!("Cannot add the theme: {error}")),
        }
    }
    pub(super) fn save_theme(
        &mut self,
        ctx: &egui::Context,
        name: String,
        existing: Option<PathBuf>,
    ) {
        let Some(dir) = self.appearance.dir.clone() else {
            self.error = Some("No theme folder is available on this system.".into());
            return;
        };
        let theme = Theme {
            name: name.trim().into(),
            ..self.theme.clone()
        };
        if let Err(error) = theme.validate() {
            self.error = Some(error);
            return;
        }
        if existing.is_none()
            && self
                .appearance
                .library
                .iter()
                .any(|t| t.theme.name.eq_ignore_ascii_case(&theme.name))
        {
            self.error = Some(format!(
                "A saved theme is already named “{}”. Choose another name.",
                theme.name
            ));
            return;
        }
        match catalog::save_to_library(&dir, &theme, existing.as_deref()) {
            Ok(_) => {
                self.appearance.reload();
                self.choose_theme(ctx, theme.clone());
                self.notice = format!("Theme “{}” saved.", theme.name);
            }
            Err(error) => self.error = Some(error),
        }
    }
    fn delete_theme(&mut self, path: &std::path::Path) {
        match trash::delete(path) {
            Ok(()) => {
                self.appearance.reload();
                self.notice = "Theme moved to the trash.".into();
            }
            Err(error) => self.error = Some(format!("Cannot delete the theme: {error}")),
        }
    }

    pub(super) fn theme_preferences(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(!self.appearance.customizing, "Themes")
                .clicked()
            {
                self.appearance.customizing = false;
            }
            if ui
                .selectable_label(self.appearance.customizing, "Customize colors")
                .on_hover_text(format!("Edit the colors of {}", self.theme.name))
                .clicked()
            {
                self.appearance.customizing = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Export…"))
                    .on_hover_text("Save the current colors as a JSON file")
                    .clicked()
                {
                    self.export_theme(ctx);
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Import…"))
                    .on_hover_text("Add a hedit theme JSON file to your themes")
                    .clicked()
                {
                    self.import_theme(ctx);
                }
            });
        });
        ui.separator();
        if self.appearance.customizing {
            self.customize_theme(ui, ctx);
        } else {
            self.theme_gallery(ui, ctx);
        }
        ui.add_space(6.0);
        if self.theme != self.appearance.original
            && ui
                .button(format!("Revert to {}", self.appearance.original.name))
                .on_hover_text("Restore the theme you had when Preferences opened")
                .clicked()
        {
            let original = self.appearance.original.clone();
            self.choose_theme(ctx, original);
        }
    }

    fn theme_gallery(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut follow = self.appearance.follow_system;
        if ui
            .checkbox(&mut follow, "Match the system light or dark appearance")
            .changed()
        {
            self.set_follow_system(ctx, follow);
        }
        if self.appearance.follow_system {
            ui.horizontal(|ui| {
                for (slot, label, theme) in [
                    (Slot::Dark, "When dark", &self.appearance.dark),
                    (Slot::Light, "When light", &self.appearance.light),
                ] {
                    let text = format!("{label}: {}", theme.name);
                    if ui
                        .selectable_label(self.appearance.slot == slot, text)
                        .on_hover_text("Choose and customize the theme used in this appearance")
                        .clicked()
                    {
                        self.appearance.slot = slot;
                    }
                }
            });
        }
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.appearance.query)
                .hint_text("Filter themes…")
                .desired_width(220.0),
        );
        ui.add_space(4.0);
        let all = self.appearance.all();
        // Lead with the kind of theme in use when Preferences opened, without
        // reordering the gallery under the pointer as themes are chosen.
        let light_first = if self.appearance.follow_system {
            self.appearance.slot == Slot::Light
        } else {
            !self.appearance.original.dark
        };
        let mut groups = vec![
            (
                "Dark",
                all.iter()
                    .filter(|(t, path)| path.is_none() && t.dark)
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
            (
                "Light",
                all.iter()
                    .filter(|(t, path)| path.is_none() && !t.dark)
                    .cloned()
                    .collect(),
            ),
        ];
        if light_first {
            groups.swap(0, 1);
        }
        groups.push((
            "Your themes",
            all.iter()
                .filter(|(_, path)| path.is_some())
                .cloned()
                .collect(),
        ));
        let mut chosen = None;
        let mut delete = None;
        let mut export = None;
        let mut shown = 0;
        for (title, themes) in groups {
            let themes: Vec<_> = themes
                .into_iter()
                .filter(|(t, _)| matches(t, &self.appearance.query))
                .collect();
            if themes.is_empty() {
                continue;
            }
            shown += themes.len();
            ui.add_space(6.0);
            ui.label(RichText::new(title).strong());
            ui.add_space(2.0);
            let gap = 10.0;
            // Leave room for the scroll bar beside the last column.
            let available = ui.available_width() - 14.0;
            let columns = ((available + gap) / (136.0 + gap)).floor().max(1.0) as usize;
            let width = ((available - gap * (columns - 1) as f32) / columns as f32).floor();
            for row in themes.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (theme, path) in row {
                        let selected = self.theme == *theme;
                        let modified =
                            !selected && self.theme.name == format!("{} (modified)", theme.name);
                        let response = theme_card(
                            ui,
                            theme,
                            selected || modified,
                            self.theme.accent.color(),
                            width,
                        )
                        .on_hover_text(if modified {
                            format!("{} — you have changed some colors", theme.name)
                        } else {
                            theme.name.clone()
                        });
                        if response.clicked() {
                            chosen = Some(theme.clone());
                        }
                        if let Some(path) = path {
                            response.context_menu(|ui| {
                                if ui.button("Export…").clicked() {
                                    export = Some(theme.clone());
                                    ui.close_menu();
                                }
                                if ui.button("Move to trash").clicked() {
                                    delete = Some(path.clone());
                                    ui.close_menu();
                                }
                            });
                        }
                    }
                });
                ui.add_space(4.0);
            }
        }
        if shown == 0 {
            ui.weak("No themes match the filter.");
        }
        if self.appearance.library.is_empty() && self.appearance.query.is_empty() {
            ui.add_space(4.0);
            ui.weak("Themes you import or save appear under “Your themes”. Right-click one to export or remove it.");
        }
        for error in &self.appearance.library_errors {
            ui.colored_label(self.theme.warning.color(), error);
        }
        if let Some(theme) = chosen {
            self.choose_theme(ctx, theme);
        }
        if let Some(path) = delete {
            self.delete_theme(&path);
        }
        if let Some(theme) = export {
            let current = std::mem::replace(&mut self.theme, theme);
            self.export_theme(ctx);
            self.theme = current;
        }
    }

    fn customize_theme(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::Frame::new()
            .fill(self.theme.background.color())
            .stroke(egui::Stroke::new(1.0, self.theme.border.color()))
            .corner_radius(4.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(syntax::highlight_with_theme(
                    SAMPLE,
                    12.0,
                    ui.available_width(),
                    true,
                    &self.theme,
                ));
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.add(
                egui::TextEdit::singleline(&mut self.theme.name)
                    .char_limit(80)
                    .desired_width(190.0),
            );
            ui.checkbox(&mut self.theme.dark, "Dark controls")
                .on_hover_text("Use dark menus, scroll bars, and other native controls");
        });
        // The saved or built-in theme these colors started from, for resets.
        let base = self
            .appearance
            .known(self.theme.name.trim_end_matches(" (modified)"));
        ui.add_space(4.0);
        egui::Grid::new("theme-colors")
            .num_columns(4)
            .spacing([12.0, 5.0])
            .show(ui, |ui| {
                for group in Group::ALL {
                    if group != Group::ALL[0] {
                        ui.label("");
                        ui.end_row();
                    }
                    ui.label(RichText::new(group.label()).strong());
                    ui.end_row();
                    for (index, role) in Role::ALL.into_iter().enumerate() {
                        if role.group() != group {
                            continue;
                        }
                        ui.label(role.label());
                        let color = self.theme.color_mut(role);
                        if ui.color_edit_button_srgb(&mut color.0).changed() {
                            self.appearance.hex[index] = color.hex();
                        }
                        let valid = Rgb::try_from(self.appearance.hex[index].clone()).is_ok();
                        // A fixed size: grid cells would otherwise keep a first-frame width.
                        let response = ui.add_sized(
                            [76.0, 20.0],
                            egui::TextEdit::singleline(&mut self.appearance.hex[index])
                                .id_salt(("hex", index))
                                .font(egui::TextStyle::Monospace)
                                .char_limit(7)
                                .text_color(if valid {
                                    self.theme.text.color()
                                } else {
                                    self.theme.error.color()
                                }),
                        );
                        if response.changed() {
                            if let Ok(color) = Rgb::try_from(self.appearance.hex[index].clone()) {
                                *self.theme.color_mut(role) = color;
                            }
                        }
                        if !valid {
                            response
                                .on_hover_text("Use #RRGGBB. Your last valid color stays active.");
                        }
                        match &base {
                            Some(base) if base.color(role) != self.theme.color(role) => {
                                if ui
                                    .small_button("↺")
                                    .on_hover_text(format!(
                                        "Reset to {} from {}",
                                        base.color(role).hex(),
                                        base.name
                                    ))
                                    .clicked()
                                {
                                    *self.theme.color_mut(role) = base.color(role);
                                    self.appearance.hex[index] = base.color(role).hex();
                                }
                            }
                            _ => {
                                ui.label("");
                            }
                        }
                        ui.end_row();
                    }
                }
            });
        ui.add_space(8.0);
        let contrast = Theme::contrast(self.theme.text, self.theme.background);
        let selection = Theme::contrast(self.theme.selection_text, self.theme.selection);
        let low = contrast < 4.5 || selection < 4.5;
        ui.colored_label(
            if low {
                self.theme.warning.color()
            } else {
                self.theme.muted.color()
            },
            format!(
                "Contrast: text {contrast:.1}:1 · selection {selection:.1}:1{}",
                if low {
                    " · below 4.5:1, may be hard to read"
                } else {
                    ""
                }
            ),
        );
        ui.add_space(6.0);
        let saved = self
            .appearance
            .library
            .iter()
            .find(|t| t.theme.name == self.theme.name)
            .map(|t| (t.path.clone(), t.theme != self.theme));
        let mut save = None;
        ui.horizontal_wrapped(|ui| {
            if let Some((path, true)) = &saved {
                if ui.button("Save changes").clicked() {
                    save = Some((self.theme.name.clone(), Some(path.clone())));
                }
            }
            if self.appearance.save_name.is_none()
                && ui
                    .add_enabled(
                        self.appearance.dir.is_some(),
                        egui::Button::new("Save as new theme…"),
                    )
                    .on_disabled_hover_text("No theme folder is available on this system.")
                    .clicked()
            {
                let taken: Vec<_> = self
                    .appearance
                    .all()
                    .into_iter()
                    .map(|(t, _)| t.name)
                    .collect();
                let name = self.theme.name.trim_end_matches(" (modified)");
                self.appearance.save_name = Some(catalog::unique_name(
                    &format!("My {name}"),
                    taken.iter().map(String::as_str),
                ));
            }
            if let Some(base) = &base {
                if *base != self.theme && ui.button(format!("Reset all to {}", base.name)).clicked()
                {
                    let base = base.clone();
                    self.choose_theme(ctx, base);
                }
            }
        });
        if let Some(name) = &mut self.appearance.save_name {
            let mut cancel = false;
            ui.horizontal(|ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(name)
                        .char_limit(80)
                        .desired_width(190.0)
                        .hint_text("Theme name"),
                );
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if ui
                    .add_enabled(!name.trim().is_empty(), egui::Button::new("Save"))
                    .clicked()
                    || (enter && !name.trim().is_empty())
                {
                    save = Some((name.clone(), None));
                }
                cancel = ui.button("Cancel").clicked();
            });
            if cancel {
                self.appearance.save_name = None;
            }
        }
        if let Some((name, existing)) = save {
            self.save_theme(ctx, name, existing);
        }
    }

    /// The quick switcher: the command palette listing themes, previewing as you move.
    pub(super) fn theme_palette(&mut self, ctx: &egui::Context) {
        let down = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown));
        let up = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp));
        let enter = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        let items: Vec<_> = self
            .appearance
            .all()
            .into_iter()
            .filter(|(t, _)| matches(t, &self.palette_query))
            .map(|(t, path)| (t, path.is_some()))
            .collect();
        let mut chosen = None;
        let mut browse = false;
        let mut moved = down || up;
        // Bring the current theme into view when the switcher opens.
        let opened = self.palette_focus;
        let shown = egui::Window::new("Color theme")
            .id(egui::Id::new("theme-switcher"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 70.0])
            .default_width(420.0)
            .show(ctx, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.palette_query)
                        .desired_width(f32::INFINITY)
                        .hint_text("Type a theme name, “dark”, or “light”…"),
                );
                if self.palette_focus {
                    response.request_focus();
                    self.palette_focus = false;
                }
                if response.changed() {
                    self.palette_index = 0;
                    moved = true;
                }
                self.palette_index = self.palette_index.min(items.len().saturating_sub(1));
                if down && !items.is_empty() {
                    self.palette_index = (self.palette_index + 1) % items.len();
                }
                if up && !items.is_empty() {
                    self.palette_index = (self.palette_index + items.len() - 1) % items.len();
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height((ctx.screen_rect().height() - 190.0).clamp(120.0, 360.0))
                    .show(ui, |ui| {
                        let mut previous: Option<(bool, bool)> = None;
                        for (index, (theme, saved)) in items.iter().enumerate() {
                            let group = (*saved, theme.dark);
                            if previous != Some(group) {
                                ui.weak(match group {
                                    (true, _) => "Your themes",
                                    (false, true) => "Dark",
                                    (false, false) => "Light",
                                });
                                previous = Some(group);
                            }
                            let current =
                                self.appearance.preview.as_ref().is_some_and(|t| t == theme);
                            let label = if current {
                                format!("{}  (current)", theme.name)
                            } else {
                                theme.name.clone()
                            };
                            let response = ui.add(
                                egui::Button::new(label)
                                    .selected(index == self.palette_index)
                                    .min_size(egui::vec2(ui.available_width(), 26.0)),
                            );
                            swatches(ui, response.rect, theme);
                            if (down || up || opened) && index == self.palette_index {
                                response.scroll_to_me(None);
                            }
                            if response.clicked() {
                                chosen = Some(theme.clone());
                            }
                        }
                    });
                if items.is_empty() {
                    ui.label("No matching themes.");
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.weak("↑ ↓ preview    Enter choose    Esc cancel");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        browse = ui.small_button("Customize…").clicked();
                    });
                });
            });
        if chosen.is_none() && !browse && super::overlays::clicked_away(ctx, &shown) {
            self.cancel_theme_palette(ctx);
            self.palette = false;
            self.focus_editor = true;
            return;
        }
        if enter {
            chosen = items.get(self.palette_index).map(|(t, _)| t.clone());
        }
        if let Some(theme) = chosen {
            self.appearance.preview = None;
            self.palette = false;
            self.focus_editor = true;
            self.choose_theme(ctx, theme);
        } else if browse {
            self.appearance.preview = None;
            self.palette = false;
            let theme = self.theme.clone();
            self.choose_theme(ctx, theme);
            self.appearance.customizing = false;
            self.open_appearance();
        } else if moved {
            if let Some((theme, _)) = items.get(self.palette_index) {
                if *theme != self.theme {
                    self.set_theme(ctx, theme.clone());
                }
            }
        }
    }
    pub(super) fn open_theme_palette(&mut self) {
        self.appearance.preview = Some(self.theme.clone());
        self.palette = true;
        self.commands_only = false;
        self.palette_query.clear();
        self.palette_focus = true;
        self.palette_index = self
            .appearance
            .all()
            .iter()
            .position(|(t, _)| *t == self.theme)
            .unwrap_or(0);
    }
    /// Dismiss the quick switcher and restore the theme it started from.
    pub(super) fn cancel_theme_palette(&mut self, ctx: &egui::Context) {
        if let Some(original) = self.appearance.preview.take() {
            self.set_theme(ctx, original);
        }
    }
}

/// A row of a theme's characteristic colors at the right edge of `rect`.
fn swatches(ui: &egui::Ui, rect: egui::Rect, theme: &Theme) {
    let colors = [
        theme.background,
        theme.keyword,
        theme.function,
        theme.string,
        theme.accent,
    ];
    let size = 11.0;
    let mut x = rect.right() - 8.0 - colors.len() as f32 * (size + 3.0);
    for color in colors {
        let swatch = egui::Rect::from_min_size(
            egui::pos2(x, rect.center().y - size / 2.0),
            egui::vec2(size, size),
        );
        ui.painter().rect_filled(swatch, 2.0, color.color());
        ui.painter().rect_stroke(
            swatch,
            2.0,
            egui::Stroke::new(1.0, theme.border.color()),
            egui::StrokeKind::Inside,
        );
        x += size + 3.0;
    }
}

/// A miniature editor window painted in `theme`'s colors, with its name below.
fn theme_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: bool,
    ring: egui::Color32,
    width: f32,
) -> egui::Response {
    let height = (width * 0.56).round();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, height + 20.0), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter_at(rect.expand(2.0));
    let preview = egui::Rect::from_min_size(rect.min, egui::vec2(width, height));
    painter.rect_filled(preview, 6.0, theme.background.color());
    let side = egui::Rect::from_min_max(
        preview.min,
        egui::pos2(preview.left() + (width * 0.2).round(), preview.bottom()),
    );
    painter.rect_filled(
        side,
        egui::CornerRadius {
            nw: 6,
            sw: 6,
            ne: 0,
            se: 0,
        },
        theme.panel.color(),
    );
    let line = (height / 7.5).max(6.0);
    let bar = |x: f32, y: f32, w: f32, color: egui::Color32| {
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, (line * 0.42).max(3.0))),
            2.0,
            color,
        );
    };
    for row in 0..4 {
        let y = preview.top() + line * (1.0 + row as f32 * 1.1);
        bar(
            side.left() + 6.0,
            y,
            side.width() * if row == 1 { 0.45 } else { 0.62 },
            theme.muted.color().gamma_multiply(0.7),
        );
    }
    let code = preview.width() - side.width() - 16.0;
    let x0 = side.right() + 8.0;
    let indent = 0.08;
    let rows: [&[(f32, f32, Rgb)]; 6] = [
        &[(0.0, 0.16, theme.preprocessor), (0.19, 0.3, theme.string)],
        &[(0.0, 0.62, theme.comment)],
        &[
            (0.0, 0.18, theme.keyword),
            (0.21, 0.24, theme.function),
            (0.48, 0.2, theme.type_name),
        ],
        &[
            (indent, 0.14, theme.keyword),
            (indent + 0.17, 0.12, theme.text),
            (indent + 0.32, 0.06, theme.operator),
            (indent + 0.41, 0.22, theme.constant),
        ],
        &[
            (indent, 0.1, theme.keyword),
            (indent + 0.13, 0.3, theme.text),
            (indent + 0.46, 0.12, theme.number),
        ],
        &[(0.0, 0.06, theme.text)],
    ];
    for (row, tokens) in rows.iter().enumerate() {
        let y = preview.top() + line * (1.0 + row as f32);
        if row == 3 {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(side.right(), y - line * 0.28),
                    egui::pos2(preview.right(), y + line * 0.7),
                ),
                0.0,
                theme.selection.color(),
            );
        }
        for &(start, w, color) in tokens.iter() {
            bar(x0 + start * code, y, (w * code).max(3.0), color.color());
        }
        if row == 4 {
            let x = x0 + indent + 0.62 * code;
            painter.line_segment(
                [egui::pos2(x, y - line * 0.2), egui::pos2(x, y + line * 0.6)],
                egui::Stroke::new(1.5, theme.accent.color()),
            );
        }
    }
    let stroke = if selected {
        egui::Stroke::new(2.0, ring)
    } else if response.hovered() {
        egui::Stroke::new(1.5, ring.gamma_multiply(0.6))
    } else {
        egui::Stroke::new(1.0, theme.border.color())
    };
    painter.rect_stroke(preview, 6.0, stroke, egui::StrokeKind::Inside);
    let mut job = egui::text::LayoutJob::single_section(
        if selected {
            format!("✓ {}", theme.name)
        } else {
            theme.name.clone()
        },
        egui::TextFormat {
            font_id: FontId::proportional(12.0),
            color: if selected {
                ui.visuals().strong_text_color()
            } else {
                ui.visuals().text_color()
            },
            ..Default::default()
        },
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    let galley = ui.fonts(|f| f.layout_job(job));
    painter.galley(
        egui::pos2(rect.left() + 1.0, preview.bottom() + 4.0),
        galley,
        ui.visuals().text_color(),
    );
    response
}
