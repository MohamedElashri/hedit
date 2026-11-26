use eframe::egui;
use hedit::remote::explorer::HostImports;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct ImportHosts {
    query: String,
    selected: BTreeSet<String>,
}

impl ImportHosts {
    pub(super) fn new(imports: &HostImports, aliases: &[String]) -> Self {
        let selected = match imports {
            HostImports::All => aliases
                .iter()
                .map(|alias| alias.to_ascii_lowercase())
                .collect(),
            HostImports::Selected(selected) => selected.clone(),
        };
        Self {
            query: String::new(),
            selected,
        }
    }

    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        aliases: &[String],
        ready: bool,
    ) -> (bool, Option<HostImports>) {
        let shown = egui::Modal::new(egui::Id::unique("ssh-host-import")).show(ctx, |ui| {
            ui.set_width((ctx.viewport_rect().width() - 40.0).clamp(0.0, 420.0));
            ui.heading("Import SSH hosts");
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Filter SSH aliases")
                    .desired_width(f32::INFINITY),
            );
            let mut hosts: BTreeMap<String, Option<&str>> = self
                .selected
                .iter()
                .map(|alias| (alias.clone(), None))
                .collect();
            for alias in aliases {
                hosts.insert(alias.to_ascii_lowercase(), Some(alias));
            }
            let query = self.query.trim().to_ascii_lowercase();
            egui::ScrollArea::vertical()
                .max_height((ctx.viewport_rect().height() - 220.0).clamp(50.0, 280.0))
                .show(ui, |ui| {
                    for (key, alias) in &hosts {
                        if !key.contains(&query) {
                            continue;
                        }
                        let mut checked = self.selected.contains(key);
                        let label = match alias {
                            Some(alias) => (*alias).to_owned(),
                            None => format!("{key} (missing from SSH config)"),
                        };
                        if ui.checkbox(&mut checked, label).changed() {
                            if checked {
                                self.selected.insert(key.clone());
                            } else {
                                self.selected.remove(key);
                            }
                        }
                    }
                    if hosts.is_empty() {
                        ui.label("No SSH aliases found.");
                    }
                });
            ui.horizontal(|ui| {
                ui.weak(format!("{} selected", self.selected.len()));
                if ui.button("Clear selection").clicked() {
                    self.selected.clear();
                }
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                let selected = ui
                    .add_enabled(ready, egui::Button::new("Import selected"))
                    .clicked();
                let all = ui
                    .add_enabled(ready, egui::Button::new("Sync all hosts"))
                    .on_hover_text("Also import aliases added to SSH config later.")
                    .clicked();
                let cancel = ui.button("Cancel").clicked();
                let imports = if selected {
                    Some(HostImports::Selected(self.selected.clone()))
                } else {
                    all.then_some(HostImports::All)
                };
                (cancel, imports)
            })
            .inner
        });
        (shown.should_close() || shown.inner.0, shown.inner.1)
    }
}
