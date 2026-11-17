//! Portable project definitions, personal workspace snapshots and formatting rules.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Formatting {
    pub indent_style: Option<String>,
    pub indent_size: Option<u8>,
    pub end_of_line: Option<String>,
    pub insert_final_newline: Option<bool>,
    pub trim_trailing_whitespace: Option<bool>,
    pub charset: Option<String>,
}
impl Formatting {
    pub fn overlay(&mut self, other: &Self) {
        macro_rules! overlay {($($field:ident),*)=>{$(if other.$field.is_some(){self.$field=other.$field.clone();})*};}
        overlay!(
            indent_style,
            indent_size,
            end_of_line,
            insert_final_newline,
            trim_trailing_whitespace,
            charset
        );
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.indent_size.is_some_and(|n| !(1..=8).contains(&n)) {
            return Err("Indent size must be 1–8.".into());
        }
        if self
            .indent_style
            .as_deref()
            .is_some_and(|s| !matches!(s, "tabs" | "spaces" | "tab" | "space"))
        {
            return Err("Indent style must be spaces or tabs.".into());
        }
        if self
            .end_of_line
            .as_deref()
            .is_some_and(|s| !matches!(s, "lf" | "crlf"))
        {
            return Err("Line ending must be lf or crlf.".into());
        }
        if self.charset.as_deref().is_some_and(|s| {
            s != "utf-8-bom" && encoding_rs::Encoding::for_label(s.as_bytes()).is_none()
        }) {
            return Err("Unknown character encoding.".into());
        }
        Ok(())
    }
}
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Defaults {
    pub formatting: Formatting,
    pub languages: BTreeMap<String, Formatting>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub folders: Vec<PathBuf>,
    pub settings: Defaults,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Tab {
    pub path: Option<PathBuf>,
    pub text: Option<String>,
    pub view: crate::session::TabView,
    pub encoding: String,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub pane_mode: u8,
    pub secondary: Option<usize>,
    pub wrap: bool,
    pub font_size: f32,
    pub pane_views: Option<serde_json::Value>,
}

pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut temp, value).map_err(|e| e.to_string())?;
    temp.flush().map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
/// Read a UTF-8 text file of at most `limit` bytes.
pub fn read_text(path: &Path, limit: u64) -> Result<String, String> {
    String::from_utf8(read_bytes(path, limit)?).map_err(|e| e.to_string())
}
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path, limit: u64) -> Result<T, String> {
    serde_json::from_slice(&read_bytes(path, limit)?).map_err(|e| e.to_string())
}
fn read_bytes(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = crate::file_io::open_regular(path).map_err(|e| e.to_string())?;
    crate::file_io::read_bounded(file, limit).map_err(|e| e.to_string())
}
pub fn load(path: &Path) -> Result<(Project, State), String> {
    let mut project: Project = read_json(path, 1024 * 1024)?;
    let parent = path.parent().unwrap_or(Path::new("."));
    for folder in &mut project.folders {
        if folder.is_relative() {
            *folder = parent.join(&*folder);
        }
        *folder =
            std::fs::canonicalize(&*folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        if !folder.is_dir() {
            return Err("Project roots must be folders.".into());
        }
    }
    project.settings.formatting.validate()?;
    for f in project.settings.languages.values() {
        f.validate()?;
    }
    let state_path = path.with_extension("hedit-workspace");
    let state: State = if state_path.exists() {
        read_json(&state_path, 64 * 1024 * 1024)?
    } else {
        State::default()
    };
    if state.tabs.len() > 200
        || state
            .tabs
            .iter()
            .any(|t| t.text.as_ref().is_some_and(|s| s.len() > 16 * 1024 * 1024))
    {
        return Err("Project workspace exceeds supported tab/text limits.".into());
    }
    Ok((project, state))
}

/// The saved scope responsible for one resolved formatting property.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormattingSource {
    User,
    UserLanguage,
    Project,
    ProjectLanguage,
    EditorConfig {
        path: PathBuf,
        property: &'static str,
        unset: bool,
    },
}
pub type FormattingSources = BTreeMap<&'static str, FormattingSource>;
pub struct ResolvedFormatting {
    pub formatting: Formatting,
    pub sources: FormattingSources,
}
fn record_sources(sources: &mut FormattingSources, value: &Formatting, source: FormattingSource) {
    macro_rules! record { ($($field:ident),*) => {$(if value.$field.is_some() { sources.insert(stringify!($field), source.clone()); })*}; }
    record!(
        indent_style,
        indent_size,
        end_of_line,
        insert_final_newline,
        trim_trailing_whitespace,
        charset
    );
}
/// Resolve without collecting provenance on ordinary editor open/save paths.
pub fn formatting(
    path: &Path,
    language: &str,
    defaults: &Defaults,
    project: Option<&Defaults>,
) -> Result<Formatting, String> {
    resolve_formatting(path, language, defaults, project, None)
}
/// Settings inspectors request provenance explicitly and use the same cascade.
pub fn resolved_formatting(
    path: &Path,
    language: &str,
    defaults: &Defaults,
    project: Option<&Defaults>,
) -> Result<ResolvedFormatting, String> {
    let mut sources = FormattingSources::new();
    let formatting = resolve_formatting(path, language, defaults, project, Some(&mut sources))?;
    Ok(ResolvedFormatting {
        formatting,
        sources,
    })
}
fn resolve_formatting(
    path: &Path,
    language: &str,
    defaults: &Defaults,
    project: Option<&Defaults>,
    mut sources: Option<&mut FormattingSources>,
) -> Result<Formatting, String> {
    let mut format = defaults.formatting.clone();
    if let Some(sources) = sources.as_deref_mut() {
        record_sources(sources, &format, FormattingSource::User);
    }
    if let Some(p) = defaults.languages.get(language) {
        format.overlay(p);
        if let Some(sources) = sources.as_deref_mut() {
            record_sources(sources, p, FormattingSource::UserLanguage);
        }
    }
    if let Some(p) = project {
        format.overlay(&p.formatting);
        if let Some(sources) = sources.as_deref_mut() {
            record_sources(sources, &p.formatting, FormattingSource::Project);
        }
        if let Some(f) = p.languages.get(language) {
            format.overlay(f);
            if let Some(sources) = sources.as_deref_mut() {
                record_sources(sources, f, FormattingSource::ProjectLanguage);
            }
        }
    }
    let mut configs = Vec::new();
    for parent in path.ancestors().skip(1) {
        let config = parent.join(".editorconfig");
        if !config.exists() {
            continue;
        }
        let text =
            read_text(&config, 256 * 1024).map_err(|e| format!("{}: {e}", config.display()))?;
        let root = text
            .lines()
            .take_while(|l| !l.trim().starts_with('['))
            .any(|l| {
                l.split_once('=').is_some_and(|(k, v)| {
                    k.trim().eq_ignore_ascii_case("root") && v.trim().eq_ignore_ascii_case("true")
                })
            });
        configs.push((parent.to_path_buf(), text));
        if root {
            break;
        }
    }
    let mut values = BTreeMap::new();
    let mut origins = sources.is_some().then(BTreeMap::<String, PathBuf>::new);
    for (parent, text) in configs.into_iter().rev() {
        let relative = path
            .strip_prefix(&parent)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let mut applies = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with(['#', ';']) || line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                let pattern = &line[1..line.len() - 1];
                let target = if pattern.contains('/') {
                    relative.as_str()
                } else {
                    relative.rsplit('/').next().unwrap_or(&relative)
                };
                applies = editor_glob(pattern.trim_start_matches('/'), target);
                continue;
            }
            if applies {
                if let Some((k, v)) = line.split_once('=') {
                    let key = k.trim().to_lowercase();
                    if let Some(origins) = &mut origins {
                        origins.insert(key.clone(), parent.join(".editorconfig"));
                    }
                    values.insert(key, v.trim().to_lowercase());
                }
            }
        }
    }
    macro_rules! origin {
        ($field:literal,$property:literal,$unset:expr) => {
            if let (Some(sources), Some(origins)) = (sources.as_deref_mut(), origins.as_ref()) {
                if let Some(path) = origins.get($property) {
                    sources.insert(
                        $field,
                        FormattingSource::EditorConfig {
                            path: path.clone(),
                            property: $property,
                            unset: $unset,
                        },
                    );
                }
            }
        };
    }
    // Resolve dependent indentation values after the complete cascade. A child
    // tab_width must also affect an inherited indent_size = tab.
    let width = |value: &String| value.parse::<u8>().ok().filter(|n| (1..=8).contains(n));
    match values.get("indent_size").map(String::as_str) {
        Some("unset") => {
            format.indent_size = None;
            origin!("indent_size", "indent_size", true);
        }
        Some("tab") | None => {
            if let Some(value) = values.get("tab_width").and_then(width) {
                format.indent_size = Some(value);
                origin!("indent_size", "tab_width", false);
            }
        }
        Some(_) => {
            if let Some(value) = values.get("indent_size").and_then(width) {
                format.indent_size = Some(value);
                origin!("indent_size", "indent_size", false);
            }
        }
    }
    for (key, value) in values {
        // EditorConfig plugins ignore unsupported values. Project JSON is
        // validated separately and still reports configuration errors.
        match (key.as_str(), value.as_str()) {
            ("indent_style", "tab") => {
                origin!("indent_style", "indent_style", false);
                format.indent_style = Some("tabs".into());
            }
            ("indent_style", "space") => {
                origin!("indent_style", "indent_style", false);
                format.indent_style = Some("spaces".into());
            }
            ("indent_style", "unset") => {
                origin!("indent_style", "indent_style", true);
                format.indent_style = None;
            }
            ("end_of_line", "lf" | "crlf") => {
                origin!("end_of_line", "end_of_line", false);
                format.end_of_line = Some(value);
            }
            ("end_of_line", "unset") => {
                origin!("end_of_line", "end_of_line", true);
                format.end_of_line = None;
            }
            ("insert_final_newline", "true" | "false") => {
                origin!("insert_final_newline", "insert_final_newline", false);
                format.insert_final_newline = Some(value == "true");
            }
            ("insert_final_newline", "unset") => {
                origin!("insert_final_newline", "insert_final_newline", true);
                format.insert_final_newline = None;
            }
            ("trim_trailing_whitespace", "true" | "false") => {
                origin!(
                    "trim_trailing_whitespace",
                    "trim_trailing_whitespace",
                    false
                );
                format.trim_trailing_whitespace = Some(value == "true");
            }
            ("trim_trailing_whitespace", "unset") => {
                origin!("trim_trailing_whitespace", "trim_trailing_whitespace", true);
                format.trim_trailing_whitespace = None;
            }
            ("charset", "latin1") => {
                origin!("charset", "charset", false);
                format.charset = Some("ISO-8859-1".into());
            }
            ("charset", "utf-8" | "utf-8-bom" | "utf-16be" | "utf-16le") => {
                origin!("charset", "charset", false);
                format.charset = Some(value);
            }
            ("charset", "unset") => {
                origin!("charset", "charset", true);
                format.charset = None;
            }
            _ => (),
        }
    }
    format.validate()?;
    Ok(format)
}
fn editor_glob(pattern: &str, text: &str) -> bool {
    if let Some(a) = pattern.find('{') {
        if let Some(b) = pattern[a..].find('}') {
            return pattern[a + 1..a + b].split(',').any(|choice| {
                editor_glob(
                    &format!("{}{}{}", &pattern[..a], choice, &pattern[a + b + 1..]),
                    text,
                )
            });
        }
    }
    crate::search::glob(pattern, text)
        || pattern
            .strip_prefix("**/")
            .is_some_and(|p| crate::search::glob(p, text))
}
pub fn clean(text: &str, format: &Formatting, eol: &str) -> String {
    let mut text = if format.trim_trailing_whitespace == Some(true) {
        text.split_inclusive('\n')
            .map(|line| {
                let (content, ending) = if let Some(t) = line.strip_suffix("\r\n") {
                    (t, "\r\n")
                } else if let Some(t) = line.strip_suffix('\n') {
                    (t, "\n")
                } else {
                    (line, "")
                };
                content.trim_end_matches([' ', '\t']).to_owned() + ending
            })
            .collect::<String>()
    } else {
        text.to_owned()
    };
    if format.insert_final_newline == Some(true) && !text.is_empty() && !text.ends_with('\n') {
        text.push_str(if eol == "CRLF" { "\r\n" } else { "\n" });
    }
    text
}

#[cfg(test)]
mod provenance_tests {
    use super::*;
    #[test]
    fn sources_follow_language_project_editorconfig_and_unset_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("src");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(dir.path().join(".editorconfig"),"root = true\n[*]\nindent_size = tab\ntab_width = 6\ntrim_trailing_whitespace = unset\n").unwrap();
        std::fs::write(
            child.join(".editorconfig"),
            "[*]\ntab_width = 8\nend_of_line = crlf\ncharset = unsupported\n",
        )
        .unwrap();
        let mut user = Defaults {
            formatting: Formatting {
                indent_style: Some("tabs".into()),
                indent_size: Some(2),
                trim_trailing_whitespace: Some(true),
                charset: Some("utf-8".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        user.languages.insert(
            "rust".into(),
            Formatting {
                insert_final_newline: Some(true),
                ..Default::default()
            },
        );
        let mut project = Defaults {
            formatting: Formatting {
                indent_size: Some(4),
                ..Default::default()
            },
            ..Default::default()
        };
        project.languages.insert(
            "rust".into(),
            Formatting {
                indent_style: Some("spaces".into()),
                trim_trailing_whitespace: Some(false),
                ..Default::default()
            },
        );
        let path = child.join("main.rs");
        let result = resolved_formatting(&path, "rust", &user, Some(&project)).unwrap();
        assert_eq!(result.formatting.indent_size, Some(8));
        assert_eq!(result.formatting.indent_style.as_deref(), Some("spaces"));
        assert_eq!(result.formatting.trim_trailing_whitespace, None);
        assert_eq!(result.formatting.insert_final_newline, Some(true));
        assert_eq!(result.formatting.end_of_line.as_deref(), Some("crlf"));
        assert_eq!(result.formatting.charset.as_deref(), Some("utf-8"));
        assert_eq!(
            result.sources["indent_style"],
            FormattingSource::ProjectLanguage
        );
        assert_eq!(
            result.sources["insert_final_newline"],
            FormattingSource::UserLanguage
        );
        assert_eq!(result.sources["charset"], FormattingSource::User);
        assert_eq!(
            result.sources["indent_size"],
            FormattingSource::EditorConfig {
                path: child.join(".editorconfig"),
                property: "tab_width",
                unset: false
            }
        );
        assert_eq!(
            result.sources["trim_trailing_whitespace"],
            FormattingSource::EditorConfig {
                path: dir.path().join(".editorconfig"),
                property: "trim_trailing_whitespace",
                unset: true
            }
        );
        assert_eq!(
            formatting(&path, "rust", &user, Some(&project)).unwrap(),
            result.formatting
        );
    }
}
