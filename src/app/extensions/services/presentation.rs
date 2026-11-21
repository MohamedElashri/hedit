//! Native, owner-scoped result views. Only the selected provider renders details.
use super::*;
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
fn plain(ui: &mut egui::Ui, text: &str, page: &mut usize) {
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
    ui.monospace(&text[start..end]);
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
impl Services {
    pub(super) fn show(&mut self, app: &mut HeditApp, ctx: &egui::Context, now: u64) {
        if !self.open {
            return;
        }
        let mut open = true;
        let mut chosen = None;
        let mut selected = self.selected.clone();
        egui::Window::new("Extension results").id(egui::Id::new("plugin-results")).open(&mut open).default_width(480.0).show(ctx,|ui| {
            if let Some(error)=&self.error{ui.label(error);}
            egui::ScrollArea::vertical().max_height(520.0).show(ui,|ui|for entry in &mut self.entries {
                ui.push_id(&entry.contribution.id,|ui| {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(selected.as_deref()==Some(entry.contribution.id.as_str()),&entry.contribution.title).clicked(){selected=Some(entry.contribution.id.clone());}
                        if entry.state.stopped||entry.state.quarantined {
                            if ui.button("Resume").clicked(){self.stopped.remove(&entry.contribution.id);if let Some(token)=entry.plugin.approval_id(){self.quarantined.remove(&(entry.contribution.id.clone(),token));}entry.state.resume();entry.scope=None;entry.error=None;}
                        }else if ui.button("Stop").clicked(){
                            if let Some(job)=entry.job.take(){app.extension_manager.record(job.invocation,InvocationOutcome::Cancelled);}
                            entry.close=None;entry.ready=None;entry.choice=None;entry.input=None;entry.state.stop(now);self.stopped.insert(entry.contribution.id.clone());
                        }
                        if entry.job.is_some(){ui.spinner();}
                    });
                    ui.weak(&entry.plugin.plugin_id);
                    if entry.state.quarantined{ui.label("Stopped after repeated failures. Resume to retry.");}
                    if let Some(error)=&entry.error{ui.label(error);}
                    if selected.as_deref()!=Some(entry.contribution.id.as_str()){ui.separator();return;}
                    let Some(ready)=&entry.ready else{return;};
                    let mut button=|ui:&mut egui::Ui,title:&str,action:&dyn Fn()->Action| {
                        if ui.add(egui::Button::new(title).truncate()).clicked(){chosen=Some((entry.contribution.id.clone(),ready.output.request.clone(),action()));ui.close_menu();}
                    };
                    match &ready.output.result {
                        ResultSet::Closed=>{},
                        ResultSet::Diagnostics(items)=>rows(ui,"diagnostics",items.len(),|ui,index|{
                            let item=&items[index];ui.horizontal(|ui| {
                                if !item.actions.is_empty(){ui.menu_button("Fixes",|ui|for action in &item.actions{button(ui,&action.title,&||action.action.clone());});}
                                ui.add(egui::Label::new(&item.message).truncate()).on_hover_text(&item.message);
                            });
                        }),
                        ResultSet::Completions(items)=>rows(ui,"completions",items.len(),|ui,index| {
                            let item=&items[index];
                            button(ui,&item.label,&||Action::Edit(plugin_runtime::sdk::Transaction{document:ready.output.document.clone(),revision:ready.output.revision.clone(),edits:vec![plugin_runtime::sdk::Edit{start:item.range.start,finish:item.range.finish,text:item.insert.clone()}],selections:None,storage:None}));
                        }),
                        ResultSet::Hover(item)=>plain(ui,&item.text,&mut entry.output_page),
                        ResultSet::Symbols(items)=>rows(ui,"symbols",items.len(),|ui,index| {let item=&items[index];button(ui,&item.name,&||Action::Select(plugin_runtime::sdk::Selection{anchor:item.range.start,head:item.range.finish}));}),
                        ResultSet::Formatting(transaction)=>button(ui,"Review formatting",&||Action::Edit(transaction.clone())),
                        ResultSet::CodeActions(items)=>rows(ui,"actions",items.len(),|ui,index| {let item=&items[index];button(ui,&item.title,&||item.action.clone());}),
                        ResultSet::Presentation(presentation)=>{
                            if let Some(status)=&presentation.status{ui.label(status);}
                            if let Some(output)=&presentation.output{plain(ui,output,&mut entry.output_page);}
                            rows(ui,"annotations",presentation.annotations.len(),|ui,index| {let item=&presentation.annotations[index];button(ui,&item.text,&||Action::Select(plugin_runtime::sdk::Selection{anchor:item.range.start,head:item.range.finish}));});
                            if let Some(list)=&presentation.quick_list{ui.label(&list.title);rows(ui,"list",list.items.len(),|ui,index| {let item=&list.items[index];button(ui,&item.title,&||item.action.clone());});}
                            if let Some(prompt)=&presentation.input {
                                ui.label(&prompt.title);let value=entry.input.get_or_insert_with(||prompt.default.clone());
                                if prompt.choices.is_empty(){match value {
                                    hedit::plugin_options::Value::Text(text)=>{ui.add(egui::TextEdit::singleline(text).char_limit(256));},
                                    hedit::plugin_options::Value::Integer(number)=>{ui.add(egui::DragValue::new(number).range(i32::MIN..=i32::MAX));},
                                    hedit::plugin_options::Value::Boolean(value)=>{ui.checkbox(value,"Enabled");},
                                }}else{for option in &prompt.choices{ui.selectable_value(value,option.clone(),format!("{option:?}"));}}
                                let valid=value.valid()&&value.same_type(&prompt.default)&&(prompt.choices.is_empty()||prompt.choices.contains(value));
                                if ui.add_enabled(valid,egui::Button::new("Submit")).clicked(){chosen=Some((entry.contribution.id.clone(),ready.output.request.clone(),Action::Callback{name:prompt.name.clone(),value:value.clone()}));}
                            }
                        },
                    }
                    ui.separator();
                });
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
