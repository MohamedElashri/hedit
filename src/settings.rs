//! User preferences in one readable JSON file, in the style of Sublime Text's
//! `Preferences.sublime-settings`: comments and trailing commas are allowed,
//! unknown keys are reported but ignored, and every key has a default.
use crate::projects::{Defaults, Formatting};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

/// Formatting keys, usable at the top level and per syntax. Unset keys leave
/// each file's own style alone.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FormatSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_size: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translate_tabs_to_spaces: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trim_trailing_white_space_on_save: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ensure_newline_at_eof_on_save: Option<bool>,
    /// `"lf"` or `"crlf"`; files are converted when saved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_ending: Option<String>,
    /// An encoding label such as `"utf-8"`; files are converted when saved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
}
impl FormatSettings {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
    pub fn to_formatting(&self) -> Formatting {
        Formatting {
            indent_style: self
                .translate_tabs_to_spaces
                .map(|spaces| if spaces { "spaces" } else { "tabs" }.into()),
            indent_size: self.tab_size,
            end_of_line: self.line_ending.clone(),
            insert_final_newline: self.ensure_newline_at_eof_on_save,
            trim_trailing_whitespace: self.trim_trailing_white_space_on_save,
            charset: self.encoding.clone(),
        }
    }
    pub fn from_formatting(formatting: &Formatting) -> Self {
        Self {
            tab_size: formatting.indent_size,
            translate_tabs_to_spaces: formatting
                .indent_style
                .as_deref()
                .map(|style| matches!(style, "spaces" | "space")),
            trim_trailing_white_space_on_save: formatting.trim_trailing_whitespace,
            ensure_newline_at_eof_on_save: formatting.insert_final_newline,
            line_ending: formatting.end_of_line.clone(),
            encoding: formatting.charset.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // Editor
    /// A font family name or font file path; empty uses the built-in font.
    pub font_face: String,
    pub font_size: f32,
    /// Extra space between lines, in points.
    pub line_padding: f32,
    pub caret_width: f32,
    pub caret_blink: bool,
    /// Scroll the last line up to the top of the editor.
    pub scroll_past_end: bool,
    pub word_wrap: bool,
    /// Wrap at this column; 0 wraps at the window edge.
    pub wrap_width: u16,
    pub rulers: Vec<u16>,
    pub line_numbers: bool,
    pub highlight_line: bool,
    pub draw_white_space: bool,
    pub draw_indent_guides: bool,
    pub show_minimap: bool,
    pub auto_indent: bool,
    pub smart_indent: bool,
    /// Backspace in leading whitespace removes one indentation level.
    pub smart_backspace: bool,
    /// Insert closing brackets and quotes, wrap selections, and step over closers.
    pub auto_match_enabled: bool,
    /// Highlight the bracket matching the one beside the cursor.
    pub match_brackets: bool,
    /// Outline other occurrences of a selected word.
    pub match_selection: bool,
    /// Use the indentation a file already has instead of the defaults.
    pub detect_indentation: bool,
    /// Offer completions in a list while typing.
    pub auto_complete: bool,
    /// Milliseconds after typing before the list opens.
    pub auto_complete_delay: u16,
    /// Only Tab accepts a completion; Enter starts a new line.
    pub auto_complete_commit_on_tab: bool,
    /// List a name's definitions when the pointer rests on it.
    pub show_definitions: bool,
    /// Underline misspelled words in prose, comments, and strings.
    pub spell_check: bool,
    /// Words spell checking accepts.
    pub added_words: Vec<String>,
    // Window
    pub show_tab_bar: bool,
    pub show_status_bar: bool,
    pub show_sidebar: bool,
    pub show_folders: bool,
    // Formatting
    #[serde(flatten)]
    pub format: FormatSettings,
    /// Formatting per syntax ID, such as `"python"` or `"makefile"`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub syntax_specific: BTreeMap<String, FormatSettings>,
    // Folders and find
    /// Folder names hidden from the folder tree, Go to file, and Find in folders.
    pub folder_exclude_patterns: Vec<String>,
    /// File names hidden from the folder tree, Go to file, and Find in folders.
    pub file_exclude_patterns: Vec<String>,
    pub find_case_sensitive: bool,
    pub find_regex: bool,
    pub find_whole_word: bool,
    // Appearance: theme names, built-in or saved in the themes folder.
    pub color_theme: String,
    pub follow_system_appearance: bool,
    pub dark_color_theme: String,
    pub light_color_theme: String,
    // Build
    /// `"automatic"` picks by syntax; otherwise a build system's name.
    pub build_system: String,
    /// Build systems added to, or replacing, the built-in ones.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub build_systems: Vec<crate::build::BuildSystem>,
    // Tools
    pub cuda_gdb_path: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            font_face: String::new(),
            font_size: 14.0,
            line_padding: 0.0,
            caret_width: 1.5,
            caret_blink: true,
            scroll_past_end: true,
            word_wrap: false,
            wrap_width: 0,
            rulers: vec![],
            line_numbers: true,
            highlight_line: true,
            draw_white_space: false,
            draw_indent_guides: true,
            show_minimap: false,
            auto_indent: true,
            smart_indent: true,
            smart_backspace: true,
            auto_match_enabled: true,
            match_brackets: true,
            match_selection: true,
            detect_indentation: true,
            auto_complete: true,
            auto_complete_delay: 50,
            auto_complete_commit_on_tab: false,
            show_definitions: true,
            spell_check: false,
            added_words: Vec::new(),
            show_tab_bar: true,
            show_status_bar: true,
            show_sidebar: true,
            show_folders: true,
            format: FormatSettings::default(),
            syntax_specific: BTreeMap::new(),
            folder_exclude_patterns: crate::folder::DEFAULT_FOLDER_EXCLUDES
                .map(String::from)
                .to_vec(),
            file_exclude_patterns: crate::folder::DEFAULT_FILE_EXCLUDES
                .map(String::from)
                .to_vec(),
            find_case_sensitive: true,
            find_regex: false,
            find_whole_word: false,
            color_theme: crate::theme::INK.into(),
            follow_system_appearance: false,
            dark_color_theme: crate::theme::INK.into(),
            light_color_theme: crate::theme::PAPER.into(),
            build_system: "automatic".into(),
            build_systems: Vec::new(),
            cuda_gdb_path: "cuda-gdb".into(),
        }
    }
}

/// Keys earlier versions wrote that no longer do anything; they are ignored
/// quietly so old settings files load without warnings.
const RETIRED: &[&str] = &["update_with_github_cli"];

pub const FONT_SIZES: std::ops::RangeInclusive<f32> = 10.0..=28.0;
pub const MAX_COLUMN: u16 = 400;
pub const FILE_LIMIT: u64 = 1024 * 1024;

const HEADER: &str = "// hedit settings. Edit them here or in Settings (Ctrl+, / Cmd+,).
// Comments and trailing commas are allowed. Delete a key to restore its default.
// Every key is described in the user guide under “Settings”.
";

impl Settings {
    /// Parse settings text, returning warnings for keys hedit does not know.
    pub fn parse(text: &str) -> Result<(Self, Vec<String>), String> {
        let value: serde_json::Value = serde_json::from_str(&strip_comments(text))
            .map_err(|e| format!("Invalid settings: {e}"))?;
        let serde_json::Value::Object(map) = &value else {
            return Err("Settings must be a JSON object.".into());
        };
        let known = serde_json::to_value(Self::with_every_key()).expect("settings serialize");
        let warnings = map
            .keys()
            .filter(|key| !known.as_object().unwrap().contains_key(*key))
            .filter(|key| !RETIRED.contains(&key.as_str()))
            .map(|key| format!("Unknown setting “{key}” is ignored."))
            .collect();
        let settings: Self =
            serde_json::from_value(value).map_err(|e| format!("Invalid settings: {e}"))?;
        settings.validate()?;
        Ok((settings, warnings))
    }
    /// Defaults with every optional key present, to list the known keys.
    fn with_every_key() -> Self {
        let format = FormatSettings {
            tab_size: Some(4),
            translate_tabs_to_spaces: Some(true),
            trim_trailing_white_space_on_save: Some(false),
            ensure_newline_at_eof_on_save: Some(false),
            line_ending: Some("lf".into()),
            encoding: Some("utf-8".into()),
        };
        Self {
            syntax_specific: BTreeMap::from([("python".into(), format.clone())]),
            format,
            ..Self::default()
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if !FONT_SIZES.contains(&self.font_size) {
            return Err("font_size must be between 10 and 28.".into());
        }
        if !(0.0..=20.0).contains(&self.line_padding) {
            return Err("line_padding must be between 0 and 20.".into());
        }
        if !(1.0..=4.0).contains(&self.caret_width) {
            return Err("caret_width must be between 1 and 4.".into());
        }
        if self.auto_complete_delay > 2000 {
            return Err("auto_complete_delay must be 0–2000 milliseconds.".into());
        }
        if self.wrap_width > MAX_COLUMN {
            return Err(format!("wrap_width must be 0–{MAX_COLUMN}."));
        }
        if self.rulers.iter().any(|&c| c == 0 || c > MAX_COLUMN) || self.rulers.len() > 8 {
            return Err(format!(
                "rulers takes up to 8 columns from 1 to {MAX_COLUMN}."
            ));
        }
        crate::folder::validate_excludes(
            &self.folder_exclude_patterns,
            &self.file_exclude_patterns,
        )?;
        for system in &self.build_systems {
            system.validate()?;
        }
        if self.cuda_gdb_path.trim().is_empty() {
            return Err("cuda_gdb_path cannot be empty.".into());
        }
        self.format
            .to_formatting()
            .validate()
            .map_err(|e| format!("Formatting: {e}"))?;
        for (syntax, format) in &self.syntax_specific {
            format
                .to_formatting()
                .validate()
                .map_err(|e| format!("syntax_specific.{syntax}: {e}"))?;
        }
        Ok(())
    }
    pub fn to_json(&self) -> String {
        format!(
            "{HEADER}{}\n",
            serde_json::to_string_pretty(self).expect("settings serialize")
        )
    }
    pub fn load(path: &Path) -> Result<(Self, Vec<String>), String> {
        let text = crate::projects::read_text(path, FILE_LIMIT)?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
    /// Write atomically so a crash never leaves a half-written file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        file.write_all(self.to_json().as_bytes())
            .map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(path).map_err(|e| e.to_string())?;
        crate::session::sync_directory(parent)
    }
    /// The formatting rules these settings define, in project form.
    pub fn defaults(&self) -> Defaults {
        Defaults {
            formatting: self.format.to_formatting(),
            languages: self
                .syntax_specific
                .iter()
                .map(|(syntax, format)| (syntax.clone(), format.to_formatting()))
                .collect(),
        }
    }
    pub fn set_defaults(&mut self, defaults: &Defaults) {
        self.format = FormatSettings::from_formatting(&defaults.formatting);
        self.syntax_specific = defaults
            .languages
            .iter()
            .map(|(syntax, f)| (syntax.clone(), FormatSettings::from_formatting(f)))
            .collect();
    }
}

/// Where hedit keeps settings, key bindings, snippets, and themes:
/// `HEDIT_CONFIG_DIR`, else the platform configuration directory.
pub fn config_dir() -> Option<PathBuf> {
    std::env::var_os("HEDIT_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            directories::ProjectDirs::from("", "hedit", "hedit").map(|d| d.config_dir().into())
        })
}

/// Remove `//` and `/* */` comments outside strings, and trailing commas.
pub fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut string = false;
    while let Some(c) = chars.next() {
        if string {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == '"' {
                string = false;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut previous = ' ';
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                    }
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
            }
            _ => out.push(c),
        }
    }
    // Drop commas that only precede whitespace and a closing bracket.
    let mut result = String::with_capacity(out.len());
    let mut string = false;
    let mut escaped = false;
    let bytes: Vec<char> = out.chars().collect();
    for (i, &c) in bytes.iter().enumerate() {
        if string {
            string = c != '"' || escaped;
            escaped = c == '\\' && !escaped;
        } else if c == '"' {
            string = true;
        } else if c == ','
            && bytes[i + 1..]
                .iter()
                .find(|c| !c.is_whitespace())
                .is_some_and(|c| matches!(c, '}' | ']'))
        {
            continue;
        }
        result.push(c);
    }
    result
}
