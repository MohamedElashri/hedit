//! Searchable setting metadata and validated controls shared by the settings UI.
use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct State {
    pub query: String,
    pub modified: bool,
    pub category: Option<PreferencesPage>,
    pub reset_scroll: bool,
    pub focus_search: bool,
    pub terminal_font_draft: Option<String>,
    pub terminal_font_error: Option<String>,
    snapshot: Option<(hedit::settings::Settings, std::sync::Arc<Value>)>,
    drafts: BTreeMap<&'static str, String>,
    errors: BTreeMap<&'static str, String>,
}
#[derive(Clone, Copy)]
enum Kind {
    Bool,
    Number(f64, f64, f64),
    Text,
    List,
    Columns,
    OptionalBool,
    Choice(&'static [(&'static str, &'static str)]),
    Group(&'static [&'static str]),
}
struct Setting {
    key: &'static str,
    title: &'static str,
    description: &'static str,
    page: PreferencesPage,
    kind: Kind,
}
fn catalog() -> &'static [Setting] {
    static CATALOG: std::sync::LazyLock<Vec<Setting>> = std::sync::LazyLock::new(|| {
        use Kind::*;
        use PreferencesPage::*;
        let mut entries = Vec::new();
        macro_rules! setting {
            ($page:ident, $kind:expr, $key:literal, $title:literal, $description:literal) => {
                entries.push(Setting {
                    key: $key,
                    title: $title,
                    description: $description,
                    page: $page,
                    kind: $kind,
                });
            };
        }
        setting!(General, Bool, "auto_refresh_workspace", "Automatically refresh workspace", "Use bounded native filesystem watches for local folders and open files. Refocusing also refreshes folders.");
        setting!(
            General,
            Bool,
            "show_tab_bar",
            "Show tab bar",
            "Display open files as tabs above the editor."
        );
        setting!(
            General,
            Bool,
            "show_status_bar",
            "Show status bar",
            "Show file information and editing controls along the bottom."
        );
        setting!(
            General,
            Bool,
            "show_sidebar",
            "Show sidebar",
            "Keep the project and file navigation sidebar visible."
        );
        setting!(
            General,
            Bool,
            "show_folders",
            "Show folders in the sidebar",
            "Browse project folders alongside open and closed files."
        );
        setting!(
            General,
            Bool,
            "show_activity_bar",
            "Show activity bar",
            "Keep Files, Source Control, Search and Settings navigation beside the sidebar."
        );
        setting!(General,Bool,"show_breadcrumbs","Show breadcrumbs","Show the file location above each editor group; click a segment to reveal it in Files.");
        setting!(
            General,
            Bool,
            "show_outline",
            "Show Outline",
            "Show a bounded, on-demand symbol list for the active document."
        );
        setting!(Editor, Text, "font_face", "Font family or file", "Use an installed font family or an absolute font-file path. Empty selects built-in Hack.");
        setting!(
            Editor,
            Number(10.0, 28.0, 1.0),
            "font_size",
            "Font size",
            "Size of editor text, in logical pixels."
        );
        setting!(
            Editor,
            Number(0.0, 20.0, 1.0),
            "line_padding",
            "Line spacing",
            "Extra space between lines, in logical pixels."
        );
        setting!(
            Editor,
            Number(1.0, 4.0, 0.5),
            "caret_width",
            "Cursor width",
            "Width of the text caret, in logical pixels."
        );
        setting!(
            Editor,
            Bool,
            "caret_blink",
            "Blink the cursor",
            "Animate the caret while the editor is focused."
        );
        setting!(
            Editor,
            Bool,
            "scroll_past_end",
            "Scroll past the last line",
            "Allow the last line to scroll up to the top of the editor."
        );
        setting!(
            Editor,
            Bool,
            "line_numbers",
            "Show line numbers",
            "Display document line numbers in the gutter."
        );
        setting!(
            Editor,
            Bool,
            "highlight_line",
            "Highlight the current line",
            "Keep the active line easy to locate."
        );
        setting!(
            Editor,
            Bool,
            "draw_white_space",
            "Show whitespace",
            "Mark spaces and tab characters without changing the file."
        );
        setting!(
            Editor,
            Bool,
            "draw_indent_guides",
            "Show indentation guides",
            "Draw guides for nested indentation levels."
        );
        setting!(
            Editor,
            Bool,
            "show_minimap",
            "Show minimap",
            "Display a compact overview of the document."
        );
        setting!(
            Editor,
            Bool,
            "word_wrap",
            "Wrap long lines",
            "Wrap text visually without inserting line breaks."
        );
        setting!(
            Editor,
            Number(0.0, 400.0, 1.0),
            "wrap_width",
            "Wrap at column",
            "Zero wraps at the window edge; other values select a column."
        );
        setting!(
            Editor,
            Columns,
            "rulers",
            "Rulers",
            "Comma-separated guide columns, such as 80, 120. Leave empty for no rulers."
        );
        setting!(
            Editor,
            Bool,
            "auto_indent",
            "Keep previous indentation",
            "Keep the previous line's indentation when starting a new line."
        );
        setting!(
            Editor,
            Bool,
            "smart_indent",
            "Smart indentation",
            "Indent after an opening bracket or colon."
        );
        setting!(
            Editor,
            Bool,
            "smart_backspace",
            "Smart backspace",
            "Remove one indentation level when backspacing in leading whitespace."
        );
        setting!(
            Editor,
            Bool,
            "trim_automatic_white_space",
            "Clean automatic indentation",
            "Remove indentation left on lines that stay empty."
        );
        setting!(
            Editor,
            Bool,
            "copy_with_empty_selection",
            "Copy the current line",
            "Copy and cut the whole line when nothing is selected."
        );
        setting!(
            Editor,
            Bool,
            "auto_match_enabled",
            "Close brackets and quotes",
            "Insert matching closing characters automatically."
        );
        setting!(
            Editor,
            Bool,
            "match_brackets",
            "Highlight matching brackets",
            "Highlight the bracket matching the one beside the cursor."
        );
        setting!(
            Editor,
            Bool,
            "match_selection",
            "Highlight matching selections",
            "Outline other occurrences of the selected word."
        );
        setting!(
            Editor,
            Bool,
            "auto_complete",
            "Automatic completion",
            "Suggest words and language-server completions while typing."
        );
        setting!(
            Editor,
            Number(0.0, 2000.0, 10.0),
            "auto_complete_delay",
            "Completion delay",
            "Wait this many milliseconds after typing before opening suggestions."
        );
        setting!(
            Editor,
            Bool,
            "auto_complete_commit_on_tab",
            "Accept completions with Tab only",
            "Let Enter start a new line while suggestions are open."
        );
        setting!(
            Editor,
            Bool,
            "show_definitions",
            "Show definitions on hover",
            "Show definitions when the pointer rests on a name."
        );
        setting!(
            Editor,
            Bool,
            "spell_check",
            "Check spelling",
            "Check prose, comments and strings."
        );
        setting!(
            Editor,
            List,
            "added_words",
            "Personal dictionary",
            "Words accepted by spell checking, one per line."
        );
        setting!(
            Editor,
            Choice(&[
                ("auto", "Git, otherwise saved file"),
                ("saved", "Saved file"),
                ("off", "Off")
            ]),
            "mini_diff",
            "Change markers",
            "Choose what editor change markers compare against."
        );
        setting!(
            Formatting,
            Bool,
            "detect_indentation",
            "Detect file indentation",
            "Prefer the indentation each file already uses."
        );
        setting!(
            Formatting,
            OptionalBool,
            "translate_tabs_to_spaces",
            "Indent with spaces",
            "Inherit the file or project style, or explicitly select spaces or tabs."
        );
        setting!(
            Formatting,
            Choice(&[
                ("", "Inherit file / project"),
                ("1", "1"),
                ("2", "2"),
                ("3", "3"),
                ("4", "4"),
                ("5", "5"),
                ("6", "6"),
                ("7", "7"),
                ("8", "8")
            ]),
            "tab_size",
            "Tab size",
            "Default indentation width. Project and per-syntax rules can override it."
        );
        setting!(
            Formatting,
            OptionalBool,
            "trim_trailing_white_space_on_save",
            "Trim trailing whitespace on save",
            "Explicitly enable, disable, or inherit whitespace trimming."
        );
        setting!(
            Formatting,
            OptionalBool,
            "ensure_newline_at_eof_on_save",
            "Final newline on save",
            "Explicitly enable, disable, or inherit the final-newline rule."
        );
        setting!(
            Formatting,
            Choice(&[
                ("", "Inherit file / project"),
                ("lf", "LF · Unix / macOS"),
                ("crlf", "CRLF · Windows")
            ]),
            "line_ending",
            "Line endings on save",
            "Choose the line-ending convention used when saving."
        );
        setting!(
            Formatting,
            Choice(&[
                ("", "Inherit file / project"),
                ("utf-8", "UTF-8"),
                ("utf-8-bom", "UTF-8 with BOM"),
                ("utf-16le", "UTF-16 LE"),
                ("utf-16be", "UTF-16 BE"),
                ("windows-1252", "Windows-1252"),
                ("iso-8859-1", "ISO-8859-1"),
                ("shift_jis", "Shift JIS"),
                ("gbk", "GBK")
            ]),
            "encoding",
            "Encoding on save",
            "Choose a default encoding; leave inherited to preserve file or project choices."
        );
        setting!(
            Formatting,
            Group(&["syntax_specific"]),
            "syntax_specific",
            "Per-language formatting",
            "Set language-specific indentation, encoding and save rules."
        );
        setting!(Folders, List, "folder_exclude_patterns", "Excluded folders", "Glob patterns hidden from the folder tree, file picker and project search. One per line.");
        setting!(
            Folders,
            Bool,
            "explorer_auto_reveal",
            "Reveal the active file",
            "Open its ancestors in Explorer when the active file changes."
        );
        setting!(
            Folders,
            Bool,
            "explorer_compact_folders",
            "Compact single-child folders",
            "Combine folder chains without files or sibling folders."
        );
        setting!(
            Folders,
            Bool,
            "explorer_git_badges",
            "Show Git badges",
            "Decorate Explorer entries with cached Git status while Files is visible."
        );
        setting!(
            Folders,
            Bool,
            "explorer_diagnostic_badges",
            "Show diagnostic badges",
            "Show counts for current open-file diagnostics without starting language servers."
        );
        setting!(
            Folders,
            List,
            "file_exclude_patterns",
            "Excluded files",
            "File glob patterns hidden from navigation and project search. One per line."
        );
        setting!(
            Folders,
            Bool,
            "find_case_sensitive",
            "Case-sensitive search",
            "Match the exact letter case when finding text."
        );
        setting!(
            Folders,
            Bool,
            "find_regex",
            "Regular-expression search",
            "Interpret the find query as a regular expression."
        );
        setting!(
            Folders,
            Bool,
            "find_whole_word",
            "Whole-word search",
            "Match complete words when finding text."
        );
        setting!(
            Terminal,
            Bool,
            "terminal_enabled",
            "Enable integrated terminal",
            "Allow terminal sessions. Disabling ends running shells; enabling does not start one."
        );
        setting!(
            Terminal,
            Bool,
            "terminal_bidi",
            "Bidirectional terminal text",
            "Shape Arabic and arrange Hebrew and Arabic text in reading order."
        );
        setting!(Terminal, Group(&["terminal_font","terminal_colors"]), "terminal_font", "Terminal font and colors", "Customize terminal font, size, cursor, selection and ANSI palette, independently of the app.");
        setting!(Appearance, Text, "ui_font_face", "UI font family or file", "Font for menus, tabs, panels and settings. Empty uses bundled Noto Sans. Editor and terminal fonts are independent.");
        setting!(
            Appearance,
            Number(10.0, 20.0, 1.0),
            "ui_font_size",
            "UI font size",
            "Size of interface text in logical pixels, independent of editor zoom."
        );
        setting!(
            Appearance,
            Choice(&[
                ("compact", "Compact"),
                ("default", "Default"),
                ("comfortable", "Comfortable")
            ]),
            "ui_density",
            "UI density",
            "Control spacing and row height throughout the workbench."
        );
        setting!(
            Appearance,
            Group(&[
                "color_theme",
                "follow_system_appearance",
                "dark_color_theme",
                "light_color_theme"
            ]),
            "color_theme",
            "App theme and appearance",
            "Choose themes, follow system light or dark appearance, and customize colors."
        );
        setting!(
            Keys,
            Choice(&[("hedit", "Hedit"), ("vscode", "VS Code · common commands")]),
            "keybinding_preset",
            "Shortcut preset",
            "Choose Hedit or common VS Code commands. Personal bindings take priority."
        );
        setting!(
            Keys,
            Group(&["keybindings"]),
            "keybindings",
            "Keyboard shortcuts",
            "Search commands, record key combinations and change key bindings."
        );
        setting!(
            Snippets,
            Group(&["snippets"]),
            "snippets",
            "Snippets",
            "Edit reusable text snippets and tab stops."
        );
        setting!(
            Extensions,
            Group(&["extensions"]),
            "extensions",
            "Extensions",
            "Import local plugin manifests, preview their commands, and enable, disable, or reload extensions."
        );
        setting!(Tools, Text, "cuda_gdb_path", "CUDA-GDB executable", "Set the debugger executable or absolute path. Stop running tool sessions before changing it.");
        setting!(
            Editor,
            Text,
            "build_system",
            "Build system",
            "Use automatic selection by syntax, or enter a configured build system's name."
        );
        setting!(
            Editor,
            Group(&["build_systems"]),
            "build_systems",
            "Custom build systems",
            "Advanced build commands are configured in settings.json."
        );
        setting!(
            Recovery,
            Group(&["recovery"]),
            "recovery",
            "Recovery and checkpoints",
            "Inspect automatic recovery, checkpoint unsaved files and browse closed files."
        );
        setting!(LanguageServers, Group(&["language_servers"]), "language_servers", "Language servers", "Install and configure rust-analyzer, clangd, ty, Ruff, LuaLS, Tombi, Go, web and shell servers; control features, priority and project roots.");
        entries
    });
    &CATALOG
}
fn matches(setting: &Setting, query: &str) -> bool {
    let extra = match setting.kind {
        Kind::Group(keys) => keys.join(" "),
        _ => String::new(),
    };
    let aliases = if setting.key == "language_servers" {
        "rust-analyzer clangd python ty ruff lua lua-language-server toml tombi taplo go gopls typescript javascript typescript-language-server html css json vscode-html-language-server vscode-css-language-server vscode-json-language-server yaml yaml-language-server bash bash-language-server pylsp"
    } else {
        ""
    };
    let normalize = |text: &str| text.to_lowercase().replace(['_', '.', '-'], " ");
    let haystack = normalize(&format!(
        "{} {} {} {} {extra} {aliases}",
        setting.key,
        setting.title,
        setting.description,
        setting.page.title()
    ));
    normalize(query)
        .split_whitespace()
        .all(|word| haystack.contains(word))
}
fn default_text(value: &Value) -> String {
    match value {
        Value::Bool(v) => if *v { "Enabled" } else { "Disabled" }.into(),
        Value::Null => "Inherit".into(),
        Value::String(s) if s.is_empty() => "Built-in / automatic".into(),
        Value::String(s) => s.clone(),
        Value::Array(a) if a.is_empty() => "None".into(),
        Value::Array(a) => format!("{} entries", a.len()),
        _ => value.to_string(),
    }
}
fn defaults() -> &'static Value {
    static DEFAULTS: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| {
        serde_json::to_value(hedit::settings::Settings::default()).unwrap()
    });
    &DEFAULTS
}
impl HeditApp {
    pub(super) fn settings_search_active(&self) -> bool {
        !self.settings_browser.query.trim().is_empty() || self.settings_browser.modified
    }
    fn settings_document(&mut self) -> std::sync::Arc<Value> {
        if !self
            .settings_browser
            .snapshot
            .as_ref()
            .is_some_and(|(settings, _)| settings == &self.settings)
        {
            self.settings_browser.snapshot = Some((
                self.settings.clone(),
                std::sync::Arc::new(serde_json::to_value(&self.settings).unwrap()),
            ));
        }
        self.settings_browser.snapshot.as_ref().unwrap().1.clone()
    }
    pub(super) fn settings_category_counts(&mut self) -> [usize; PreferencesPage::ALL.len()] {
        let current = self.settings_document();
        let defaults = defaults();
        let mut counts = [0; PreferencesPage::ALL.len()];
        for setting in catalog().iter().filter(|s| {
            matches(s, &self.settings_browser.query)
                && (!self.settings_browser.modified || self.setting_modified(s, &current, defaults))
        }) {
            counts[setting.page as usize] += 1;
        }
        counts
    }
    fn setting_modified(&self, spec: &Setting, current: &Value, defaults: &Value) -> bool {
        if self.settings_browser.errors.contains_key(spec.key) {
            return true;
        }
        match spec.key {
            "language_servers" => !self.languages.installed.is_empty(),
            "keybindings" => {
                self.quality.config.keybindings != quality::Config::default().keybindings
            }
            "snippets" => self.quality.config.snippets != quality::Config::default().snippets,
            _ => match spec.kind {
                Kind::Group(keys) => keys.iter().any(|key| current[key] != defaults[key]),
                _ => current[spec.key] != defaults[spec.key],
            },
        }
    }
    pub(super) fn set_browser_value(
        &mut self,
        key: &'static str,
        value: Value,
    ) -> Result<(), String> {
        if !catalog()
            .iter()
            .any(|setting| setting.key == key && !matches!(setting.kind, Kind::Group(_)))
        {
            return Err("This setting uses its dedicated editor.".into());
        }
        if key == "cuda_gdb_path"
            && self
                .debug
                .as_ref()
                .is_some_and(|d| d.state != hedit::debugger::State::Exited)
        {
            return Err("Stop the debugger before changing its executable.".into());
        }
        let mut document = serde_json::to_value(&self.settings).map_err(|e| e.to_string())?;
        if value.is_null() {
            document.as_object_mut().unwrap().remove(key);
        } else {
            document[key] = value;
        }
        let mut candidate: hedit::settings::Settings =
            serde_json::from_value(document).map_err(|e| e.to_string())?;
        candidate.validate()?;
        if key == "show_folders" && candidate.show_folders {
            candidate.show_sidebar = true;
        }
        if key == "terminal_enabled" {
            candidate.terminal_onboarding_complete = true;
        }
        if key == "keybinding_preset" {
            self.chord = None;
            self.reset_terminal_binding();
        }
        self.settings = candidate;
        self.apply_settings();
        self.apply_format_preferences();
        if key == "terminal_enabled" {
            self.sync_terminal();
        }
        self.settings_browser.errors.remove(key);
        self.settings_browser.drafts.remove(key);
        Ok(())
    }
    pub(super) fn interface_settings(&mut self, ui: &mut egui::Ui) {
        let current = self.settings_document();
        for spec in catalog().iter().filter(|s| s.key.starts_with("ui_")) {
            ui.push_id(spec.key, |ui| {
                self.setting_card(ui, spec, &current, defaults())
            });
            ui.add_space(8.0);
        }
    }
    pub(super) fn settings_catalog(&mut self, ui: &mut egui::Ui, page: Option<PreferencesPage>) {
        let current = self.settings_document();
        let defaults = defaults();
        let entries: Vec<_> = catalog()
            .iter()
            .filter(|s| {
                s.page.available()
                    && page.is_none_or(|p| s.page == p)
                    && matches(s, &self.settings_browser.query)
                    && (!self.settings_browser.modified
                        || self.setting_modified(s, &current, defaults))
            })
            .collect();
        if self.settings_search_active() {
            ui.weak(format!("{} matching settings", entries.len()));
            ui.add_space(8.0);
        }
        if entries.is_empty() {
            ui.label("No matching settings.");
            ui.weak("Try a setting name, a JSON key, or clear the Modified filter.");
        }
        for spec in entries {
            ui.push_id(spec.key, |ui| {
                self.setting_card(ui, spec, &current, defaults)
            });
            ui.add_space(8.0);
        }
    }
    fn setting_card(
        &mut self,
        ui: &mut egui::Ui,
        spec: &Setting,
        current: &Value,
        defaults: &Value,
    ) {
        let modified = self.setting_modified(spec, current, defaults);
        let mut reset = false;
        let mut edited = None;
        let mut retain_draft = None;
        let background = ui.painter().add(egui::Shape::Noop);
        let row = egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(10, 10))
            .show(ui, |ui| {
                ui.set_min_width((ui.available_width() - 8.0).max(0.0));
                ui.horizontal_wrapped(|ui| {
                    ui.strong(spec.title);
                    if modified {
                        ui.label(
                            RichText::new("Modified")
                                .small()
                                .color(self.theme.accent.color()),
                        );
                    }
                    let reset_font = hedit::ui_style::Metrics::for_ui(ui).small;
                    let reset_width = ui.fonts(|f| {
                        f.layout_no_wrap(
                            "Reset".into(),
                            FontId::proportional(reset_font),
                            self.theme.text.color(),
                        )
                        .size()
                        .x
                    }) + 2.0 * ui.spacing().button_padding.x;
                    if ui.available_size_before_wrap().x < reset_width {
                        ui.end_row();
                    }
                    if !matches!(spec.kind, Kind::Group(_))
                        && ui
                            .add_enabled(
                                modified || self.settings_browser.drafts.contains_key(spec.key),
                                egui::Button::new("Reset").small(),
                            )
                            .on_hover_text(format!(
                                "Default: {}",
                                default_text(&defaults[spec.key])
                            ))
                            .clicked()
                    {
                        reset = true;
                    }
                });
                ui.label(spec.description);
                ui.weak(RichText::new(spec.key).monospace().small());
                ui.add_space(5.0);
                ui.scope(|ui| {
                    ui.set_max_width(ui.available_width().min(360.0));
                    match spec.kind {
                        Kind::Group(_) => {
                            let json_only = spec.key == "build_systems";
                            if ui
                                .button(if json_only {
                                    "Edit in settings.json"
                                } else {
                                    "Open settings…"
                                })
                                .clicked()
                            {
                                if json_only {
                                    self.open_settings_file(ui.ctx());
                                } else {
                                    self.settings_browser.query.clear();
                                    self.settings_browser.modified = false;
                                    self.settings_browser.category = None;
                                    self.preferences_page = spec.page;
                                }
                            }
                        }
                        Kind::Bool => {
                            let mut v = current[spec.key].as_bool().unwrap_or(false);
                            let enabled =
                                spec.key != "terminal_enabled" || cfg!(feature = "terminal");
                            if ui
                                .add_enabled(enabled, egui::Checkbox::new(&mut v, "Enabled"))
                                .changed()
                            {
                                edited = Some(json!(v));
                            }
                        }
                        Kind::Number(min, max, step) => {
                            let mut v = current[spec.key].as_f64().unwrap_or(min);
                            if ui
                                .add(
                                    egui::DragValue::new(&mut v)
                                        .range(min..=max)
                                        .speed(step)
                                        .max_decimals(if step < 1.0 { 1 } else { 0 }),
                                )
                                .changed()
                            {
                                edited = Some(
                                    if spec.key == "font_size"
                                        || spec.key == "line_padding"
                                        || spec.key == "caret_width"
                                    {
                                        json!(v)
                                    } else {
                                        json!(v.round() as u64)
                                    },
                                );
                            }
                        }
                        Kind::OptionalBool => {
                            let old = current[spec.key].as_bool();
                            let mut v = old;
                            egui::ComboBox::from_id_salt("value")
                                .selected_text(match v {
                                    None => "Inherit file / project",
                                    Some(true) => "Enabled",
                                    Some(false) => "Disabled",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut v, None, "Inherit file / project");
                                    ui.selectable_value(&mut v, Some(true), "Enabled");
                                    ui.selectable_value(&mut v, Some(false), "Disabled");
                                });
                            if v != old {
                                edited = Some(json!(v));
                            }
                        }
                        Kind::Choice(choices) => {
                            let old = if current[spec.key].is_null() {
                                String::new()
                            } else {
                                current[spec.key]
                                    .as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| current[spec.key].to_string())
                            };
                            let mut v = old.clone();
                            egui::ComboBox::from_id_salt("value")
                                .selected_text(
                                    choices
                                        .iter()
                                        .find(|(key, _)| *key == v)
                                        .map_or(v.as_str(), |(_, title)| *title),
                                )
                                .show_ui(ui, |ui| {
                                    for (key, title) in choices {
                                        ui.selectable_value(&mut v, (*key).into(), *title);
                                    }
                                });
                            if v != old {
                                edited = Some(if v.is_empty() {
                                    Value::Null
                                } else if spec.key == "tab_size" {
                                    json!(v.parse::<u8>().unwrap())
                                } else {
                                    json!(v)
                                });
                            }
                        }
                        kind => {
                            let initial = match kind {
                                Kind::Text => {
                                    current[spec.key].as_str().unwrap_or_default().to_owned()
                                }
                                Kind::Columns => current[spec.key]
                                    .as_array()
                                    .map(|a| {
                                        a.iter()
                                            .map(Value::to_string)
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    })
                                    .unwrap_or_default(),
                                _ => current[spec.key]
                                    .as_array()
                                    .map(|a| {
                                        a.iter()
                                            .filter_map(Value::as_str)
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    })
                                    .unwrap_or_default(),
                            };
                            let mut text = self
                                .settings_browser
                                .drafts
                                .get(spec.key)
                                .cloned()
                                .unwrap_or(initial);
                            let response = if matches!(kind, Kind::List) {
                                ui.add(
                                    egui::TextEdit::multiline(&mut text)
                                        .id(egui::Id::new(("setting-value", spec.key)))
                                        .desired_rows(3)
                                        .desired_width(f32::INFINITY)
                                        .char_limit(65536),
                                )
                            } else {
                                ui.add(
                                    egui::TextEdit::singleline(&mut text)
                                        .id(egui::Id::new(("setting-value", spec.key)))
                                        .desired_width(f32::INFINITY)
                                        .char_limit(4096),
                                )
                            };
                            if response.has_focus() {
                                retain_draft = Some(text.clone());
                            } else if !self.settings_browser.errors.contains_key(spec.key) {
                                self.settings_browser.drafts.remove(spec.key);
                            }
                            if response.changed() {
                                edited = match kind {
                                    Kind::Text => Some(json!(text)),
                                    Kind::Columns => {
                                        let numbers = text
                                            .split([',', ' ', '\n'])
                                            .filter(|s| !s.is_empty())
                                            .map(str::parse::<u16>)
                                            .collect::<Result<Vec<_>, _>>();
                                        match numbers {
                                            Ok(v) => Some(json!(v)),
                                            Err(_) => {
                                                self.settings_browser.errors.insert(
                                                    spec.key,
                                                    "Use comma-separated column numbers.".into(),
                                                );
                                                None
                                            }
                                        }
                                    }
                                    _ => Some(json!(text
                                        .lines()
                                        .map(str::trim)
                                        .filter(|s| !s.is_empty())
                                        .collect::<Vec<_>>())),
                                };
                                self.settings_browser.drafts.insert(spec.key, text.clone());
                            }
                        }
                    }
                    if spec.key == "ui_font_face" {
                        if let Some(choice) = self.ui_font_choice(ui) {
                            edited = Some(json!(choice));
                            retain_draft = None;
                        }
                    }
                });
                if !matches!(spec.kind, Kind::Group(_)) {
                    ui.weak(format!("Default: {}", default_text(&defaults[spec.key])));
                }
                if let Some(error) = self.settings_browser.errors.get(spec.key) {
                    ui.colored_label(self.theme.warning.color(), error);
                }
            });
        if ui.rect_contains_pointer(row.response.rect) {
            ui.painter().set(
                background,
                egui::Shape::rect_filled(row.response.rect, 0.0, self.theme.hover().color()),
            );
        }
        if modified {
            let rect = row.response.rect.shrink2(egui::vec2(0.0, 10.0));
            ui.painter().vline(
                rect.left(),
                rect.y_range(),
                egui::Stroke::new(2.0_f32, self.theme.accent.color()),
            );
        }
        ui.painter().hline(
            row.response.rect.x_range(),
            row.response.rect.bottom(),
            egui::Stroke::new(1.0_f32, self.theme.border.color()),
        );
        if reset {
            retain_draft = None;
            edited = Some(defaults[spec.key].clone());
        }
        if let Some(value) = edited {
            if let Err(error) = self.set_browser_value(spec.key, value) {
                self.settings_browser.errors.insert(spec.key, error);
            }
        }
        if let Some(text) = retain_draft {
            self.settings_browser.drafts.insert(spec.key, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{frame, frame_size, text_rect};
    use super::*;
    fn app() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        (directory, ctx, app)
    }
    #[test]
    fn searchable_catalog_covers_persisted_preferences_and_has_unique_keys() {
        use hedit::settings::{FormatSettings, Settings, TerminalColors, TerminalFont};
        let settings = Settings {
            terminal_font: Some(TerminalFont::default()),
            terminal_colors: Some(TerminalColors::default()),
            format: FormatSettings {
                tab_size: Some(4),
                translate_tabs_to_spaces: Some(true),
                trim_trailing_white_space_on_save: Some(false),
                ensure_newline_at_eof_on_save: Some(false),
                line_ending: Some("lf".into()),
                encoding: Some("utf-8".into()),
            },
            ..Settings::default()
        };
        let mut names = std::collections::BTreeSet::new();
        let mut covered = std::collections::BTreeSet::new();
        for item in catalog() {
            assert!(names.insert(item.key), "duplicate: {}", item.key);
            covered.insert(item.key);
            if let Kind::Group(keys) = item.kind {
                covered.extend(keys.iter().copied());
            }
        }
        let value = serde_json::to_value(settings).unwrap();
        for key in value.as_object().unwrap().keys() {
            assert!(
                key == "terminal_onboarding_complete" || covered.contains(key.as_str()),
                "not searchable: {key}"
            );
        }
        for query in [
            "font_size",
            "CURSOR blink",
            "terminal colors",
            "project roots",
            "encoding",
        ] {
            assert!(catalog().iter().any(|s| matches(s, query)), "{query}");
        }
    }
    #[test]
    fn edits_validate_reset_and_keep_documents_and_other_preferences_intact() {
        let (_dir, _ctx, mut app) = app();
        let text = app.active().page.text.clone();
        app.set_browser_value("font_size", json!(18.0)).unwrap();
        assert!(app.set_browser_value("font_size", json!(99.0)).is_err());
        assert_eq!(app.settings.font_size, 18.0);
        app.set_browser_value("trim_trailing_white_space_on_save", json!(false))
            .unwrap();
        assert_eq!(
            app.settings.format.trim_trailing_white_space_on_save,
            Some(false)
        );
        app.set_browser_value("trim_trailing_white_space_on_save", Value::Null)
            .unwrap();
        assert_eq!(app.settings.format.trim_trailing_white_space_on_save, None);
        app.set_browser_value("font_size", Value::Null).unwrap();
        assert_eq!(
            app.settings.font_size,
            hedit::settings::Settings::default().font_size
        );
        assert_eq!(app.active().page.text, text);
        app.set_browser_value("terminal_enabled", json!(true))
            .unwrap();
        assert!(app.settings.terminal_onboarding_complete);
        assert!(!app.terminal.open);
    }
    #[test]
    fn modified_filter_and_reset_work_from_search_at_minimum_size() {
        let (_dir, ctx, mut app) = app();
        app.open_preferences(PreferencesPage::Editor);
        app.settings_browser.query = "font size".into();
        app.settings_browser.modified = true;
        app.settings.font_size = 20.0;
        assert_eq!(
            app.settings_category_counts()[PreferencesPage::Editor as usize],
            1
        );
        let size = egui::vec2(720.0, 480.0);
        for _ in 0..3 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let output = frame_size(&ctx, &mut app, vec![], size);
        let point = output
            .shapes
            .iter()
            .find_map(|shape| {
                text_rect(&shape.shape, "Reset")
                    .filter(|rect| shape.clip_rect.contains(rect.center()))
            })
            .expect("reset is visible")
            .center();
        frame_size(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
            size,
        );
        frame_size(
            &ctx,
            &mut app,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
            size,
        );
        assert_eq!(
            app.settings.font_size,
            hedit::settings::Settings::default().font_size
        );
        assert_eq!(
            app.settings_category_counts()[PreferencesPage::Editor as usize],
            0
        );
    }
    #[test]
    fn invalid_setting_drafts_survive_focus_changes_and_navigation() {
        let (_dir, ctx, mut app) = app();
        app.open_preferences(PreferencesPage::Editor);
        app.settings_browser.query = "rulers".into();
        app.settings_browser
            .drafts
            .insert("rulers", "not a number".into());
        app.settings_browser
            .errors
            .insert("rulers", "Use comma-separated column numbers.".into());
        frame(&ctx, &mut app, vec![]);
        app.open_preferences(PreferencesPage::General);
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.settings_browser.drafts["rulers"], "not a number");
        assert!(app.settings.rulers.is_empty());
        app.settings_browser.modified = true;
        assert_eq!(
            app.settings_category_counts()[PreferencesPage::Editor as usize],
            1,
            "invalid drafts remain discoverable"
        );
    }
    #[test]
    fn list_and_column_editors_preserve_separators_while_typing() {
        let (_dir, ctx, mut app) = app();
        for (query, key, first, second) in [
            ("rulers", "rulers", "80, ", "120"),
            ("added_words", "added_words", "alpha\n", "beta"),
        ] {
            app.open_preferences(PreferencesPage::Editor);
            app.settings_browser.query = query.into();
            for _ in 0..2 {
                frame(&ctx, &mut app, vec![]);
            }
            ctx.memory_mut(|m| m.request_focus(egui::Id::new(("setting-value", key))));
            frame(&ctx, &mut app, vec![egui::Event::Text(first.into())]);
            assert_eq!(
                app.settings_browser.drafts.get(key).map(String::as_str),
                Some(first)
            );
            frame(&ctx, &mut app, vec![egui::Event::Text(second.into())]);
            if key == "rulers" {
                assert_eq!(app.settings.rulers, [80, 120]);
            } else {
                assert_eq!(app.settings.added_words, ["alpha", "beta"]);
            }
        }
    }
    #[test]
    #[ignore = "exports searchable settings previews to HEDIT_PREVIEW_DIR"]
    fn export_settings_browser() {
        let output = PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").unwrap());
        std::fs::create_dir_all(&output).unwrap();
        let (_dir, ctx, mut app) = app();
        for (name, size, query, modified) in [
            ("editor", egui::vec2(1280.0, 820.0), "", false),
            ("search", egui::vec2(1280.0, 820.0), "cursor", false),
            ("modified", egui::vec2(1280.0, 820.0), "", true),
            ("compact", egui::vec2(720.0, 480.0), "font size", false),
        ] {
            app.open_preferences(PreferencesPage::Editor);
            app.settings.font_size = 18.0;
            app.settings_browser.query = query.into();
            app.settings_browser.modified = modified;
            for _ in 0..3 {
                frame_size(&ctx, &mut app, vec![], size);
            }
            let frame = frame_size(&ctx, &mut app, vec![], size);
            std::fs::write(
                output.join(format!("settings-{name}.ppm")),
                super::super::rtl_tests::rasterize(&ctx, frame, size, 1.0),
            )
            .unwrap();
        }
    }
}
