//! Settings passed into an invocation. Only the command's declared keys are visible.
use super::{
    activity::{Invocation, InvocationOutcome},
    catalog,
    manifest::{ExtensionAction, LuaPlugin},
};
use crate::{
    app::{clipboard_lines, settings_store, Action, HeditApp},
    plugin_runtime::{self, Input},
};
use eframe::egui;
use hedit::{
    editing,
    plugin_options::{Options, SaveHook, Value, MAX_VALUE_BYTES},
    settings::Settings,
    syntax::Language,
};
use std::{collections::BTreeSet, fs::File, path::Path};

pub(super) fn input(
    options: &Options,
    plugin: &LuaPlugin,
    language: &str,
    selections: Vec<String>,
) -> Input {
    let settings = options.settings.get(&plugin.plugin_id);
    let settings = plugin
        .settings
        .iter()
        .map(|(key, definition)| {
            let value = settings
                .and_then(|values| values.get(key))
                .filter(|value| value.same_type(&definition.default) && value.valid())
                .unwrap_or(&definition.default);
            (key.clone(), value.clone())
        })
        .collect();
    Input {
        source: plugin.source.to_string(),
        selections,
        language: language.into(),
        settings,
    }
}

pub(in crate::app) struct SaveResult {
    id: u64,
    revision: u64,
    language: &'static str,
    binding: SaveHook,
    result: Result<(String, File), InvocationOutcome>,
    invocation: Invocation,
}

impl HeditApp {
    pub(in crate::app) fn extension_options(&mut self, ui: &mut egui::Ui) {
        if !plugin_runtime::SUPPORTED {
            return;
        }
        ui.separator();
        ui.checkbox(
            &mut self.settings.extensions.preview_edits,
            "Preview Lua edits before applying",
        );
        let mut seen = BTreeSet::new();
        for command in &self.extensions {
            let ExtensionAction::Lua(plugin) = &command.action else {
                continue;
            };
            if plugin.settings.is_empty() || !seen.insert(&plugin.plugin_id) {
                continue;
            }
            ui.push_id(&plugin.plugin_id, |ui| {
                ui.collapsing(format!("{} settings", plugin.plugin_id), |ui| {
                    for (key, definition) in plugin.settings.iter() {
                        ui.push_id(key, |ui| {
                            let values = self.settings.extensions.settings.get(&plugin.plugin_id);
                            let mut value = values
                                .and_then(|values| values.get(key))
                                .filter(|value| value.same_type(&definition.default))
                                .unwrap_or(&definition.default)
                                .clone();
                            let changed = setting_control(ui, &definition.label, &mut value);
                            if changed && value.valid() {
                                self.settings
                                    .extensions
                                    .settings
                                    .entry(plugin.plugin_id.clone())
                                    .or_default()
                                    .insert(key.clone(), value);
                            }
                        });
                    }
                });
            });
        }
        self.extension_save_options(ui);
    }

    fn extension_save_options(&mut self, ui: &mut egui::Ui) {
        let active_language = self.active().language();
        let filename = self.active().page.info.name.clone();
        let mut hooks = self
            .extensions
            .iter()
            .filter_map(|command| {
                let ExtensionAction::Lua(plugin) = &command.action else {
                    return None;
                };
                plugin
                    .on_save
                    .then(|| plugin.approval_id().map(|approval| (command, approval)))
                    .flatten()
            })
            .peekable();
        if hooks.peek().is_none() && self.settings.extensions.save_hooks.is_empty() {
            return;
        }
        ui.collapsing("Lua on Save", |ui| {
            let language_key = ui.id().with("save-hook-language");
            let mut language = ui
                .data(|data| data.get_temp(language_key))
                .unwrap_or(active_language);
            egui::ComboBox::from_id_salt(language_key)
                .selected_text(language.label())
                .show_ui(ui, |ui| {
                    for candidate in Language::ALL {
                        ui.selectable_value(&mut language, candidate, candidate.label());
                    }
                });
            ui.data_mut(|data| data.insert_temp(language_key, language));
            ui.label("An enabled command can read and replace the whole local file when you save.");
            let active = self.settings.extensions.save_hooks.get(language.id());
            if let Some(binding) = active {
                let mut remove = false;
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("On Save · {}", binding.command));
                    remove = ui.button("Turn off").clicked();
                });
                if remove {
                    self.settings.extensions.save_hooks.remove(language.id());
                }
            }
            for (command, approval) in hooks {
                if !command.when.matches(language, None, true)
                    && !command.when.matches(language, Some(&filename), true)
                {
                    continue;
                }
                let binding = SaveHook {
                    command: command.id.clone(),
                    approval,
                };
                let enabled =
                    self.settings.extensions.save_hooks.get(language.id()) == Some(&binding);
                if ui
                    .add_enabled(
                        !enabled,
                        egui::Button::new(format!("Allow {} on Save", command.title)),
                    )
                    .clicked()
                {
                    self.settings
                        .extensions
                        .save_hooks
                        .insert(language.id().into(), binding);
                }
            }
        });
    }

    pub(in crate::app) fn save_from_command(&mut self, ctx: &egui::Context) {
        if !self.start_extension_save(ctx) {
            self.save_file(ctx, false);
        }
    }

    fn start_extension_save(&mut self, ctx: &egui::Context) -> bool {
        let editor = self.active();
        let info = &editor.page.info;
        let language = editor.language();
        if !plugin_runtime::SUPPORTED
            || self.busy
            || self.closing
            || self.preferences_open
            || self.workspace.remote.is_some()
            || editor.follow
            || info.paged
            || info.path.is_none()
            || !info.dirty
            || editor.page.text.len() > plugin_runtime::MAX_SELECTION_BYTES
        {
            return false;
        }
        let Some(binding) = self
            .settings
            .extensions
            .save_hooks
            .get(language.id())
            .cloned()
        else {
            return false;
        };
        let Some(command) = self
            .extensions
            .iter()
            .find(|command| command.id == binding.command)
        else {
            return false;
        };
        let ExtensionAction::Lua(plugin) = &command.action else {
            return false;
        };
        if !plugin.on_save
            || plugin.approval_id().as_deref() != Some(binding.approval.as_str())
            || !command.when.matches(language, Some(&info.name), true)
        {
            return false;
        }
        let Some(config) = self.settings_files.dir.clone() else {
            return false;
        };
        let id = info.id;
        let revision = info.revision;
        let plugin = plugin.clone();
        let text = editor.page.text.clone();
        let language = language.id();
        self.job(ctx, move || {
            let invocation = Invocation::on_save(&binding.command);
            let result = run_hook(&config, &binding, language, &plugin, text);
            Ok(Action::ExtensionSave(SaveResult {
                id,
                revision,
                language,
                binding,
                result,
                invocation,
            }))
        });
        true
    }

    pub(in crate::app) fn complete_extension_save(
        &mut self,
        ctx: &egui::Context,
        result: SaveResult,
    ) {
        let SaveResult {
            id,
            revision,
            language,
            binding,
            result,
            invocation,
        } = result;
        let same = self.active().page.info.id == id
            && self.active().page.info.revision == revision
            && self.active().language().id() == language
            && self.settings.extensions.save_hooks.get(language) == Some(&binding)
            && self.workspace.remote.is_none()
            && !self.closing;
        let outcome = match result {
            Ok((replacement, _approval)) if same => {
                let active = self.active();
                let replacement =
                    clipboard_lines::line_endings(&replacement, active.page.info.eol == "CRLF");
                let chars = replacement.chars().count();
                let edit = editing::Edit {
                    text: replacement,
                    anchor: active.view.anchor.min(chars),
                    head: active.view.head.min(chars),
                };
                if self.apply_edit(edit, None) {
                    InvocationOutcome::Applied
                } else {
                    InvocationOutcome::Failed
                }
            }
            Ok(_) => InvocationOutcome::Cancelled,
            Err(outcome) => outcome,
        };
        self.extension_manager.record(invocation, outcome);
        self.save_to_path(ctx, id, None);
    }
}

fn setting_control(ui: &mut egui::Ui, label: &str, value: &mut Value) -> bool {
    match value {
        Value::Boolean(value) => ui.checkbox(value, label).changed(),
        Value::Integer(value) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(label);
                ui.add(egui::DragValue::new(value)).changed()
            })
            .inner
        }
        Value::Text(value) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(label);
                ui.add(egui::TextEdit::singleline(value).char_limit(MAX_VALUE_BYTES))
                    .changed()
            })
            .inner
        }
    }
}

fn run_hook(
    config: &Path,
    binding: &SaveHook,
    language: &str,
    plugin: &LuaPlugin,
    text: String,
) -> Result<(String, File), InvocationOutcome> {
    let approved = || -> Option<(File, Options)> {
        let authority = catalog::lua_authorization(config, plugin)?;
        let path = config.join(settings_store::SETTINGS_FILE);
        let (settings, _) = Settings::load(&path).ok()?;
        (settings.extensions.save_hooks.get(language) == Some(binding))
            .then_some((authority, settings.extensions))
    };
    let Some((authority, options)) = approved() else {
        return Err(InvocationOutcome::Denied);
    };
    let replacement =
        plugin_runtime::run_save(input(&options, plugin, language, vec![text]), authority)
            .map_err(InvocationOutcome::worker_failure)?;
    let (authority, _) = approved().ok_or(InvocationOutcome::Denied)?;
    Ok((replacement, authority))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{extensions::tests::app_with, tests::finish_app_job};
    use serde_json::json;

    #[test]
    fn save_hook_finishes_with_the_replacement_or_falls_through_to_save() {
        let config = tempfile::tempdir().unwrap();
        let file = config.path().join("sample.txt");
        std::fs::write(&file, "original").unwrap();
        let (_session, ctx, mut app) = app_with("", Some(config.path()));
        app.open(&ctx, Some(file.clone()));
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "original");

        let binding = SaveHook {
            command: "formatter.trim".into(),
            approval: "0".repeat(32),
        };
        app.settings
            .extensions
            .save_hooks
            .insert("plain-text".into(), binding.clone());
        for (dirty, response, saved) in [
            (
                "first dirty value ",
                Ok("first dirty value"),
                "first dirty value",
            ),
            (
                "second dirty value ",
                Err(InvocationOutcome::Failed),
                "second dirty value ",
            ),
        ] {
            assert!(app.apply_edit(
                editing::Edit {
                    text: dirty.into(),
                    anchor: 4,
                    head: 4
                },
                None
            ));
            let id = app.active().page.info.id;
            let revision = app.active().page.info.revision;
            // The authority handle belongs to policy storage, never the
            // document that Save atomically replaces.
            let result = response
                .map(|replacement| (replacement.to_string(), tempfile::tempfile().unwrap()));
            app.complete_extension_save(
                &ctx,
                SaveResult {
                    id,
                    revision,
                    language: "plain-text",
                    binding: binding.clone(),
                    result,
                    invocation: Invocation::on_save(&binding.command),
                },
            );
            finish_app_job(&mut app, &ctx);
            assert_eq!(std::fs::read_to_string(&file).unwrap(), saved);
        }
    }

    #[test]
    fn invocation_settings_are_filtered_by_plugin_and_declared_type() {
        let config = tempfile::tempdir().unwrap();
        let path = config.path().join("extensions/formatter.json");
        std::fs::create_dir(path.parent().unwrap()).unwrap();
        let text = json!({
            "api_version": 2, "id": "formatter",
            "permissions": ["selection.read", "selection.replace"],
            "scripts": {"run.lua": "return function(text) return text end"},
            "commands": [{
                "id": "trim", "title": "Trim selection", "on_save": true,
                "action": {"kind": "lua", "source": "run.lua"}
            }],
            "settings": {
                "indent": {"label": "Indent", "default": 4},
                "enabled": {"label": "Enabled", "default": true}
            }
        })
        .to_string();
        std::fs::write(&path, &text).unwrap();
        let catalog = catalog::set_lua_approved(
            config.path(),
            &path,
            "formatter",
            "formatter.trim",
            &super::super::manifest::digest(text),
            true,
        )
        .unwrap();
        let ExtensionAction::Lua(plugin) = &catalog.contributions[0].action else {
            unreachable!()
        };
        let mut opts = Options::default();
        opts.settings
            .entry("formatter".into())
            .or_default()
            .extend([
                ("indent".into(), Value::Text("wrong type".into())),
                ("enabled".into(), Value::Boolean(false)),
                ("undeclared".into(), Value::Text("hidden".into())),
            ]);
        opts.settings
            .entry("other-plugin".into())
            .or_default()
            .insert("secret".into(), Value::Text("hidden".into()));
        let invocation = input(&opts, plugin, "plain-text", vec!["example".into()]);
        assert_eq!(invocation.settings.len(), 2);
        assert_eq!(invocation.settings["indent"], Value::Integer(4));
        assert_eq!(invocation.settings["enabled"], Value::Boolean(false));
    }
}
