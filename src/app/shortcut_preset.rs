//! Resolve personal bindings, an optional preset, and Hedit defaults once per edit.
use super::*;
use hedit::settings::KeybindingPreset;
use quality::{Binding, BindingCommand};
use std::cell::{Ref, RefCell};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct State(RefCell<Cache>);
#[derive(Default)]
struct Cache {
    ready: bool,
    preset: KeybindingPreset,
    personal: Vec<Binding>,
    extensions: HashMap<String, bool>,
    configured: Vec<Binding>,
    commands: HashMap<Command, (String, bool)>,
}
fn preset() -> &'static [Binding] {
    static BINDINGS: std::sync::LazyLock<Vec<Binding>> = std::sync::LazyLock::new(|| {
        [
            (Command::Sidebar, "Primary+B"),
            (Command::FocusExplorer, "Primary+Shift+E"),
            (Command::FocusSearch, "Primary+Shift+F"),
            (Command::TogglePanel, "Primary+J"),
            (Command::Build, "Primary+Shift+B"),
            (Command::Symbols, "Primary+Shift+O"),
            (Command::LanguageProblems, "Primary+Shift+M"),
            (Command::LanguageRename, "F2"),
            (Command::LanguageQuickFix, "Primary+Period"),
            (Command::NextProblem, "F8"),
            (Command::PreviousProblem, "Shift+F8"),
            (
                Command::BuildResults,
                if cfg!(target_os = "linux") {
                    "Primary+K, Primary+H"
                } else {
                    "Primary+Shift+U"
                },
            ),
            (Command::SplitColumns, "Primary+Backslash"),
            (
                Command::LanguageFormat,
                if cfg!(target_os = "linux") {
                    "Primary+Shift+I"
                } else {
                    "Alt+Shift+F"
                },
            ),
            (Command::FocusGroup1, "Primary+1"),
            (Command::FocusGroup2, "Primary+2"),
            (Command::FocusGroup3, "Primary+3"),
            (Command::FocusGroup4, "Primary+4"),
        ]
        .into_iter()
        .map(|(command, shortcut)| Binding {
            command: command.into(),
            shortcut: shortcut.into(),
            editor_only: false,
        })
        .collect()
    });
    &BINDINGS
}
pub(super) fn display(text: &str) -> String {
    text.replace(
        "Primary",
        if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        },
    )
    .replace(
        "Alt",
        if cfg!(target_os = "macos") {
            "Option"
        } else {
            "Alt"
        },
    )
}
fn keys(text: &str) -> Vec<egui::KeyboardShortcut> {
    chords::parse(
        &text
            .replace("Page Down", "PageDown")
            .replace("Page Up", "PageUp"),
    )
    .unwrap_or_default()
}
fn physical(modifiers: Modifiers) -> (bool, bool, bool, bool) {
    (
        modifiers.ctrl || (!cfg!(target_os = "macos") && modifiers.command),
        modifiers.mac_cmd || (cfg!(target_os = "macos") && modifiers.command),
        modifiers.shift,
        modifiers.alt,
    )
}
fn same(a: &egui::KeyboardShortcut, b: &egui::KeyboardShortcut) -> bool {
    a.logical_key == b.logical_key && physical(a.modifiers) == physical(b.modifiers)
}
// A chord prefix also claims its single-key shortcut: the chord handler runs first.
fn collides(a: &[egui::KeyboardShortcut], b: &[egui::KeyboardShortcut]) -> bool {
    !a.is_empty() && !b.is_empty() && a.iter().zip(b).all(|(a, b)| same(a, b))
}
pub(super) fn bindings_collide(parsed: &[egui::KeyboardShortcut], text: &str) -> bool {
    collides(parsed, &keys(text))
}
impl State {
    fn resolve(
        &self,
        selected: KeybindingPreset,
        personal: &[Binding],
        extension_available: impl Fn(&str) -> bool,
    ) -> Ref<'_, Cache> {
        let changed = {
            let cache = self.0.borrow();
            !cache.ready
                || cache.preset != selected
                || cache.personal != personal
                || cache
                    .extensions
                    .iter()
                    .any(|(id, active)| extension_available(id) != *active)
        };
        if changed {
            let mut cache = self.0.borrow_mut();
            cache.ready = true;
            cache.preset = selected;
            cache.personal = personal.to_vec();
            cache.extensions.clear();
            cache.configured.clear();
            cache.commands.clear();
            let mut claimed: Vec<Vec<egui::KeyboardShortcut>> = vec![];
            // Retain personal aliases; first binding wins when different commands claim keys.
            for binding in personal.iter().chain(preset().iter().filter(|binding| {
                selected == KeybindingPreset::Vscode
                    && !personal.iter().any(|p| p.command == binding.command)
            })) {
                if let BindingCommand::Extension(id) = &binding.command {
                    let available = *cache
                        .extensions
                        .entry(id.clone())
                        .or_insert_with(|| extension_available(id));
                    if !available {
                        continue;
                    }
                }
                let parsed = keys(&binding.shortcut);
                let blocked = claimed.iter().any(|k| collides(k, &parsed));
                if let Some(command) = binding.command.builtin() {
                    let entry = cache
                        .commands
                        .entry(command)
                        .or_insert((String::new(), true));
                    if entry.0.is_empty() && !blocked {
                        entry.0 = display(&binding.shortcut);
                    }
                }
                if !blocked && !parsed.is_empty() {
                    claimed.push(parsed);
                    cache.configured.push(binding.clone());
                }
            }
            for &command in Command::ALL {
                if cache.commands.contains_key(&command) {
                    continue;
                }
                let label = command.shortcut();
                let parsed = keys(&label);
                let blocked = claimed.iter().any(|k| collides(k, &parsed));
                cache.commands.insert(
                    command,
                    (if blocked { String::new() } else { label }, blocked),
                );
            }
        }
        self.0.borrow()
    }
}
impl HeditApp {
    fn binding_cache(&self) -> Ref<'_, Cache> {
        self.shortcut_preset.resolve(
            self.settings.keybinding_preset,
            &self.quality.config.keybindings,
            |id| self.extension_available_by_id(id),
        )
    }

    pub(super) fn configured_bindings(&self) -> Ref<'_, [Binding]> {
        Ref::map(self.binding_cache(), |c| c.configured.as_slice())
    }
    pub(super) fn shortcut_text(&self, command: Command) -> String {
        self.binding_cache()
            .commands
            .get(&command)
            .map_or_else(String::new, |(label, _)| label.clone())
    }
    /// True when an explicit binding replaces this command or claims its default keys.
    pub(super) fn overridden(&self, command: Command) -> bool {
        self.binding_cache()
            .commands
            .get(&command)
            .is_some_and(|(_, overridden)| *overridden)
    }
    pub(super) fn personally_bound(&self, command: Command) -> bool {
        self.quality
            .config
            .keybindings
            .iter()
            .any(|b| b.command == command)
    }

    pub(super) fn extension_shortcut_text(&self, id: &str) -> String {
        self.configured_bindings()
            .iter()
            .find(|binding| matches!(&binding.command, BindingCommand::Extension(command) if command == id))
            .map_or_else(String::new, |binding| display(&binding.shortcut))
    }

    pub(super) fn configured_shortcut_enabled(
        &self,
        binding: &Binding,
        editor_focused: bool,
    ) -> bool {
        if binding.editor_only && !editor_focused {
            return false;
        }
        match &binding.command {
            BindingCommand::Builtin(command) => {
                !self.preferences_open || Self::settings_key_command(*command)
            }
            BindingCommand::Extension(id) => {
                editor_focused
                    && !self.transient_overlay_open()
                    && self.extension_available_by_id(id)
            }
        }
    }

    pub(super) fn execute_binding(&mut self, ctx: &egui::Context, binding: BindingCommand) {
        match binding {
            BindingCommand::Builtin(command) => self.execute(ctx, command),
            BindingCommand::Extension(id) => {
                self.run_extension_by_id(ctx, &id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{frame, key};
    fn fixture() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        settings_store::TEST_CONFIG_DIR.with(|p| *p.borrow_mut() = Some(dir.path().join("config")));
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.choose_terminal(false);
        (dir, ctx, app)
    }

    fn load_extension(dir: &std::path::Path, app: &mut HeditApp) {
        let config = dir.join("config");
        std::fs::create_dir_all(config.join("extensions")).unwrap();
        std::fs::write(
            config.join("extensions/samples.json"),
            r#"{
                "api_version": 1, "id": "samples",
                "commands": [
                    {"id":"first", "title":"Insert sample", "action":{"kind":"insert_snippet","body":"first$0"}},
                    {"id":"second", "title":"Insert sample", "action":{"kind":"insert_snippet","body":"second$0"}}
                ]
            }"#,
        ).unwrap();
        let (commands, errors) = extensions::load(Some(&config));
        assert!(errors.is_empty(), "{errors:?}");
        app.extensions = commands;
    }

    #[test]
    fn extension_bindings_keep_their_ids_and_release_keys_when_unavailable() {
        let (dir, ctx, mut app) = fixture();
        let saved = r#"[
            {"command":"save","shortcut":"Primary+Alt+S","editor_only":false},
            {"command":"samples.second","shortcut":"Primary+F","editor_only":true}
        ]"#;
        let bindings: Vec<Binding> = serde_json::from_str(saved).unwrap();
        assert_eq!(
            serde_json::to_value(&bindings).unwrap(),
            serde_json::from_str::<serde_json::Value>(saved).unwrap(),
        );
        assert!(serde_json::from_str::<Binding>(r#"{"command":"saev","shortcut":"F8"}"#).is_err());
        app.quality.config.keybindings = bindings.clone();
        assert_eq!(app.shortcut_text(Command::Find), Command::Find.shortcut());

        load_extension(dir.path(), &mut app);
        app.focus_editor = true;
        frame(&ctx, &mut app, vec![]);
        assert!(app.shortcut_text(Command::Find).is_empty());
        assert_eq!(
            app.extension_shortcut_text("samples.second"),
            display("Primary+F")
        );
        frame(&ctx, &mut app, vec![key(Key::F, Modifiers::COMMAND)]);
        assert_eq!(app.active().page.text, "second");
        assert!(!app.find_open);

        let commands = std::mem::take(&mut app.extensions);
        assert_eq!(app.shortcut_text(Command::Find), Command::Find.shortcut());
        frame(&ctx, &mut app, vec![key(Key::F, Modifiers::COMMAND)]);
        assert!(app.find_open);
        assert_eq!(app.quality.config.keybindings, bindings);
        app.find_open = false;
        app.extensions = commands;
        assert!(app.shortcut_text(Command::Find).is_empty());
        settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
    }

    #[test]
    fn extension_chords_use_the_existing_recorder_and_conflict_rules() {
        let (dir, ctx, mut app) = fixture();
        load_extension(dir.path(), &mut app);
        let command = BindingCommand::Extension("samples.first".into());
        app.set_binding(Command::Goto, Some("Alt+F7, F9".into()));
        app.open_preferences(PreferencesPage::Keys);
        app.key_capture = Some(command.clone());
        frame(&ctx, &mut app, vec![key(Key::F7, Modifiers::ALT)]);
        frame(&ctx, &mut app, vec![key(Key::F9, Modifiers::NONE)]);
        assert!(app.key_capture.is_none());
        assert!(app.shortcut_text(Command::Goto).is_empty());
        assert!(app.notice.contains(Command::Goto.label()));
        app.flush_settings();
        let saved: Vec<Binding> =
            hedit::projects::read_json(&dir.path().join("config/keybindings.json"), 1024 * 1024)
                .unwrap();
        assert!(saved
            .iter()
            .any(|binding| { binding.command == command && binding.shortcut == "Alt+F7, F9" }));

        app.preferences_open = false;
        app.focus_editor = true;
        frame(
            &ctx,
            &mut app,
            [Key::F7, Key::F9]
                .into_iter()
                .map(|key| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                })
                .collect(),
        );
        frame(&ctx, &mut app, vec![key(Key::F7, Modifiers::ALT)]);
        assert_eq!(app.active().page.text, "");
        assert!(app.chord.is_some());
        frame(&ctx, &mut app, vec![key(Key::F9, Modifiers::NONE)]);
        assert_eq!(app.active().page.text, "first");
        app.set_binding(
            BindingCommand::Extension("samples.second".into()),
            Some("Space".into()),
        );
        frame(
            &ctx,
            &mut app,
            vec![
                key(Key::Space, Modifiers::NONE),
                egui::Event::Text(" ".into()),
            ],
        );
        assert_eq!(app.active().page.text, "firstsecond");
        app.set_binding(Command::Goto, Some("Alt+F7, F9".into()));
        assert!(app
            .quality
            .config
            .keybindings
            .iter()
            .all(|binding| binding.command != command));
        settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
    }

    #[test]
    fn default_preset_preserves_hedit_and_switching_never_rewrites_personal_bindings() {
        let (_dir, _ctx, mut app) = fixture();
        assert_eq!(app.settings.keybinding_preset, KeybindingPreset::Hedit);
        for &command in Command::ALL {
            assert_eq!(app.shortcut_text(command), command.shortcut());
        }
        let personal = vec![Binding {
            command: Command::Goto.into(),
            shortcut: "F8".into(),
            editor_only: true,
        }];
        app.quality.config.keybindings = personal.clone();
        app.settings.keybinding_preset = KeybindingPreset::Vscode;
        assert_eq!(app.shortcut_text(Command::Sidebar), display("Primary+B"));
        assert_eq!(
            app.shortcut_text(Command::Build),
            display("Primary+Shift+B")
        );
        for command in [
            Command::BuildWith,
            Command::OpenFolder,
            Command::ExpandToBrackets,
        ] {
            assert!(app.shortcut_text(command).is_empty(), "{command:?}");
            assert!(app.overridden(command));
        }
        app.settings.keybinding_preset = KeybindingPreset::Hedit;
        assert_eq!(
            app.shortcut_text(Command::Sidebar),
            Command::Sidebar.shortcut()
        );
        assert_eq!(app.quality.config.keybindings, personal);
    }
    #[test]
    fn personal_keys_and_chord_prefixes_win_without_misleading_default_hints() {
        let (_dir, _ctx, mut app) = fixture();
        app.settings.keybinding_preset = KeybindingPreset::Vscode;
        app.quality.config.keybindings = vec![
            Binding {
                command: Command::Build.into(),
                shortcut: "Primary+B".into(),
                editor_only: false,
            },
            Binding {
                command: Command::Save.into(),
                shortcut: "Primary+K".into(),
                editor_only: false,
            },
        ];
        assert!(app.shortcut_text(Command::Sidebar).is_empty());
        assert_eq!(app.shortcut_text(Command::Build), display("Primary+B"));
        assert!(app.shortcut_text(Command::CloseAll).is_empty());
        assert!(app.overridden(Command::CloseAll));
        assert_eq!(
            app.configured_bindings()
                .iter()
                .filter(|b| b.command == Command::Build)
                .count(),
            1
        );
        app.quality.config.keybindings[0].shortcut = "F8".into();
        assert_eq!(app.shortcut_text(Command::Sidebar), display("Primary+B"));
        assert_eq!(app.shortcut_text(Command::Build), "F8");
    }
    #[test]
    fn preset_dispatches_sidebar_groups_and_problems_without_editing_the_document() {
        let (_dir, ctx, mut app) = fixture();
        app.settings.keybinding_preset = KeybindingPreset::Vscode;
        frame(&ctx, &mut app, vec![]);
        let visible = app.settings.show_sidebar;
        frame(&ctx, &mut app, vec![key(Key::B, Modifiers::COMMAND)]);
        assert_ne!(app.settings.show_sidebar, visible);
        frame(
            &ctx,
            &mut app,
            vec![key(Key::Backslash, Modifiers::COMMAND)],
        );
        assert_eq!(app.panes.groups(), 2);
        frame(
            &ctx,
            &mut app,
            vec![key(Key::M, Modifiers::COMMAND | Modifiers::SHIFT)],
        );
        assert!(app.languages.problems);
        assert!(app.active().page.text.is_empty());
        assert!(!app.build.running());
    }
    #[test]
    fn personal_editor_only_chords_do_not_run_in_settings_text_fields() {
        let (_dir, ctx, mut app) = fixture();
        app.quality.config.keybindings = vec![Binding {
            command: Command::Close.into(),
            shortcut: "Primary+K, Primary+W".into(),
            editor_only: true,
        }];
        app.open_preferences(PreferencesPage::Keys);
        frame(&ctx, &mut app, vec![]);
        for pressed in [Key::K, Key::W] {
            frame(&ctx, &mut app, vec![key(pressed, Modifiers::COMMAND)]);
        }
        assert!(app.preferences_open);
        assert!(app.chord.is_none());
    }
    #[test]
    fn resolution_is_cached_until_preferences_or_personal_bindings_change() {
        let (_dir, _ctx, mut app) = fixture();
        app.settings.keybinding_preset = KeybindingPreset::Vscode;
        let bindings = app.configured_bindings();
        let address = bindings.as_ptr();
        drop(bindings);
        for _ in 0..100 {
            assert_eq!(app.configured_bindings().as_ptr(), address);
        }
        app.quality.config.keybindings.push(Binding {
            command: Command::Build.into(),
            shortcut: "F8".into(),
            editor_only: false,
        });
        assert_eq!(app.shortcut_text(Command::Build), "F8");
    }
    #[test]
    fn settings_availability_and_safe_save_chords_follow_the_visible_scope() {
        let (_dir, ctx, mut app) = fixture();
        assert!(!app.command_enabled(Command::Close));
        app.execute(&ctx, Command::ProjectSettings);
        assert!(app.command_enabled(Command::Close));
        assert!(app.command_enabled(Command::Save));
        app.quality.project_editor.draft.formatting.indent_size = Some(6);
        app.sync_project_settings_gui();
        assert!(!app.command_enabled(Command::SaveProject));
        app.quality.config.keybindings.push(Binding {
            command: Command::Save.into(),
            shortcut: "Primary+K, Primary+S".into(),
            editor_only: false,
        });
        frame(&ctx, &mut app, vec![]);
        for pressed in [Key::K, Key::S] {
            frame(&ctx, &mut app, vec![key(pressed, Modifiers::COMMAND)]);
        }
        assert!(!app.quality.project_editor.dirty);
        assert_eq!(app.quality.project_settings.formatting.indent_size, Some(6));
        assert!(app.preferences_open);
        assert!(app.active().page.text.is_empty());
    }
    #[test]
    fn personal_aliases_survive_and_primary_matches_platform_control_or_command() {
        let (_dir, _ctx, mut app) = fixture();
        app.quality.config.keybindings = vec![
            Binding {
                command: Command::Build.into(),
                shortcut: "F8".into(),
                editor_only: false,
            },
            Binding {
                command: Command::Build.into(),
                shortcut: "F9".into(),
                editor_only: false,
            },
        ];
        assert_eq!(
            app.configured_bindings()
                .iter()
                .filter(|b| b.command == Command::Build)
                .count(),
            2
        );
        assert_eq!(app.shortcut_text(Command::Build), "F8");
        let primary = keys("Primary+Shift+B");
        assert!(bindings_collide(
            &primary,
            if cfg!(target_os = "macos") {
                "Cmd+Shift+B"
            } else {
                "Ctrl+Shift+B"
            }
        ));
        assert!(!bindings_collide(&primary, "Primary+B"));
        if cfg!(target_os = "macos") {
            assert!(!bindings_collide(&primary, "Ctrl+Shift+B"));
        }
    }
    #[test]
    fn recording_a_secondary_alias_claims_the_keys_for_the_new_command() {
        let (_dir, _ctx, mut app) = fixture();
        app.quality.config.keybindings = vec![
            Binding {
                command: Command::Build.into(),
                shortcut: "F8".into(),
                editor_only: false,
            },
            Binding {
                command: Command::Build.into(),
                shortcut: "F9".into(),
                editor_only: false,
            },
        ];
        app.set_binding(Command::Goto, Some("F9".into()));
        assert_eq!(app.shortcut_text(Command::Goto), "F9");
        assert!(app.shortcut_text(Command::Build).is_empty());
        assert_eq!(
            app.configured_bindings()
                .iter()
                .filter(|b| b.shortcut == "F9")
                .count(),
            1
        );
        assert!(app.notice.contains("Build"));
    }
}
