//! Resolve personal bindings, an optional preset, and Hedit defaults once per edit.
use super::*;
use hedit::settings::KeybindingPreset;
use quality::Binding;
use std::cell::{Ref, RefCell};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct State(RefCell<Cache>);
#[derive(Default)]
struct Cache {
    ready: bool,
    preset: KeybindingPreset,
    personal: Vec<Binding>,
    configured: Vec<Binding>,
    commands: HashMap<Command, (String, bool)>,
}
fn preset() -> &'static [Binding] {
    static BINDINGS: std::sync::LazyLock<Vec<Binding>> = std::sync::LazyLock::new(|| {
        [
            (Command::Sidebar, "Primary+B"),
            (Command::FocusExplorer, "Primary+Shift+E"),
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
            command,
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
    fn resolve(&self, selected: KeybindingPreset, personal: &[Binding]) -> Ref<'_, Cache> {
        let changed = {
            let cache = self.0.borrow();
            !cache.ready || cache.preset != selected || cache.personal != personal
        };
        if changed {
            let mut cache = self.0.borrow_mut();
            cache.ready = true;
            cache.preset = selected;
            cache.personal = personal.to_vec();
            cache.configured.clear();
            cache.commands.clear();
            let mut claimed: Vec<Vec<egui::KeyboardShortcut>> = vec![];
            // Retain personal aliases; first binding wins when different commands claim keys.
            for binding in personal.iter().chain(preset().iter().filter(|binding| {
                selected == KeybindingPreset::Vscode
                    && !personal.iter().any(|p| p.command == binding.command)
            })) {
                let parsed = keys(&binding.shortcut);
                let blocked = claimed.iter().any(|k| collides(k, &parsed));
                let entry = cache
                    .commands
                    .entry(binding.command)
                    .or_insert((String::new(), true));
                if entry.0.is_empty() && !blocked {
                    entry.0 = display(&binding.shortcut);
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
    pub(super) fn configured_bindings(&self) -> Ref<'_, [Binding]> {
        Ref::map(
            self.shortcut_preset.resolve(
                self.settings.keybinding_preset,
                &self.quality.config.keybindings,
            ),
            |c| c.configured.as_slice(),
        )
    }
    pub(super) fn shortcut_text(&self, command: Command) -> String {
        self.shortcut_preset
            .resolve(
                self.settings.keybinding_preset,
                &self.quality.config.keybindings,
            )
            .commands
            .get(&command)
            .map_or_else(String::new, |(label, _)| label.clone())
    }
    /// True when an explicit binding replaces this command or claims its default keys.
    pub(super) fn overridden(&self, command: Command) -> bool {
        self.shortcut_preset
            .resolve(
                self.settings.keybinding_preset,
                &self.quality.config.keybindings,
            )
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
    #[test]
    fn default_preset_preserves_hedit_and_switching_never_rewrites_personal_bindings() {
        let (_dir, _ctx, mut app) = fixture();
        assert_eq!(app.settings.keybinding_preset, KeybindingPreset::Hedit);
        for &command in Command::ALL {
            assert_eq!(app.shortcut_text(command), command.shortcut());
        }
        let personal = vec![Binding {
            command: Command::Goto,
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
                command: Command::Build,
                shortcut: "Primary+B".into(),
                editor_only: false,
            },
            Binding {
                command: Command::Save,
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
            command: Command::Close,
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
            command: Command::Build,
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
            command: Command::Save,
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
                command: Command::Build,
                shortcut: "F8".into(),
                editor_only: false,
            },
            Binding {
                command: Command::Build,
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
                command: Command::Build,
                shortcut: "F8".into(),
                editor_only: false,
            },
            Binding {
                command: Command::Build,
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
