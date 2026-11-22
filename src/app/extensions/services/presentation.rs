//! Native, owner-scoped result views. Only the selected provider renders details.
use super::*;
impl Services {
    pub(super) fn show(&mut self, app: &mut HeditApp, ctx: &egui::Context, now: u64) {
        if !self.open {
            return;
        }
        let mut open = true;
        let mut chosen = None;
        let mut selected = self.selected.clone();
        egui::Window::new("Extension results")
            .id(egui::Id::new("plugin-results"))
            .open(&mut open)
            .default_width(480.0)
            .show(ctx, |ui| {
                if let Some(error) = &self.error {
                    ui.label(error);
                }
                egui::ScrollArea::vertical()
                    .max_height(520.0)
                    .show(ui, |ui| {
                        for entry in &mut self.entries {
                            ui.push_id(entry.contribution.id.clone(), |ui| {
                                ui.horizontal(|ui| {
                                    if ui
                                        .selectable_label(
                                            selected.as_deref()
                                                == Some(entry.contribution.id.as_str()),
                                            &entry.contribution.title,
                                        )
                                        .clicked()
                                    {
                                        selected = Some(entry.contribution.id.clone());
                                    }
                                    if entry.state.stopped || entry.state.quarantined {
                                        if ui.button("Resume").clicked() {
                                            self.stopped.remove(&entry.contribution.id);
                                            if let Some(token) = entry.plugin.approval_id() {
                                                self.quarantined.remove(&(
                                                    entry.contribution.id.clone(),
                                                    token,
                                                ));
                                            }
                                            entry.state.resume();
                                            entry.scope = None;
                                            if let Some(scoped) = &mut entry.scoped {
                                                scoped.reset();
                                            }
                                            entry.error = None;
                                        }
                                    } else if ui.button("Stop").clicked() {
                                        if let Some(job) = entry.job.take() {
                                            app.extension_manager.record(
                                                job.invocation,
                                                InvocationOutcome::Cancelled,
                                            );
                                        }
                                        if let Some(scoped) = &mut entry.scoped {
                                            scoped.stop(app);
                                        }
                                        entry.close = None;
                                        entry.ready = None;
                                        entry.choice = None;
                                        entry.input = None;
                                        entry.state.stop(now);
                                        self.stopped.insert(entry.contribution.id.clone());
                                    }
                                    if entry.job.is_some()
                                        || entry
                                            .scoped
                                            .as_ref()
                                            .is_some_and(|s| s.running() && !s.reviewing())
                                    {
                                        ui.spinner();
                                    }
                                });
                                ui.weak(&entry.plugin.plugin_id);
                                if entry.state.quarantined {
                                    ui.label("Stopped after repeated failures. Resume to retry.");
                                }
                                if let Some(error) = &entry.error {
                                    ui.label(error);
                                }
                                if selected.as_deref() != Some(entry.contribution.id.as_str()) {
                                    ui.separator();
                                    return;
                                }
                                if entry.scoped.is_some() {
                                    scoped::show(entry, app, ctx, ui, now);
                                    ui.separator();
                                    return;
                                }
                                let Some(ready) = &entry.ready else {
                                    return;
                                };
                                if let Some(action) = super::super::presentation::render(
                                    ui,
                                    &ready.output.result,
                                    &ready.output.document,
                                    &ready.output.revision,
                                    &mut entry.output_page,
                                    &mut entry.input,
                                ) {
                                    chosen = Some((
                                        entry.contribution.id.clone(),
                                        ready.output.request.clone(),
                                        action,
                                    ));
                                }
                                ui.separator();
                            });
                        }
                    });
            });
        self.open = open;
        self.selected = selected;
        if let Some((id, request, action)) = chosen {
            let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| entry.contribution.id == id)
            else {
                return;
            };
            let Some(ready) = &entry.ready else {
                return;
            };
            if ready.output.request != request
                || now.saturating_sub(ready.created) >= ACTION_LIFETIME_MS
                || app.lua_target().as_ref() != Some(&ready.target)
            {
                entry.ready = None;
                return;
            }
            match action {
                Action::Callback { name, value } => {
                    entry.choice = Some((name, value));
                    entry.state.queue(Trigger::Interaction, now, 0);
                    entry.ready = None;
                    entry.input = None;
                }
                action => {
                    let transaction = match action {
                        Action::Edit(transaction) => transaction,
                        Action::Select(selection) => plugin_runtime::sdk::Transaction {
                            document: ready.output.document.clone(),
                            revision: ready.output.revision.clone(),
                            edits: vec![],
                            selections: Some(vec![selection]),
                            storage: None,
                        },
                        _ => unreachable!(),
                    };
                    let plugin = entry.plugin.clone();
                    let document = ready.output.document.clone();
                    entry.ready = None;
                    self.open = false;
                    if let Err(error) =
                        app.propose_provider_edit(ctx, &id, plugin, &document, transaction)
                    {
                        app.error = Some(error);
                    }
                }
            }
        }
    }
}
