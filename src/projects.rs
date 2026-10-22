//! Portable project definitions, personal workspace snapshots and formatting rules.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
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
#[derive(Clone, Default, Serialize, Deserialize)]
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
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path, limit: u64) -> Result<T, String> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > limit {
        return Err("Settings/workspace file is too large.".into());
    }
    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
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

pub fn formatting(
    path: &Path,
    language: &str,
    defaults: &Defaults,
    project: Option<&Defaults>,
) -> Result<Formatting, String> {
    let mut format = defaults.formatting.clone();
    if let Some(p) = defaults.languages.get(language) {
        format.overlay(p);
    }
    if let Some(p) = project {
        format.overlay(&p.formatting);
        if let Some(f) = p.languages.get(language) {
            format.overlay(f);
        }
    }
    let mut configs = Vec::new();
    for parent in path.ancestors().skip(1) {
        let config = parent.join(".editorconfig");
        if !config.exists() {
            continue;
        }
        if std::fs::metadata(&config).map_err(|e| e.to_string())?.len() > 256 * 1024 {
            return Err(".editorconfig exceeds 256 KiB.".into());
        }
        let text = std::fs::read_to_string(&config).map_err(|e| e.to_string())?;
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
    for (parent, text) in configs.into_iter().rev() {
        let relative = path
            .strip_prefix(&parent)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let mut applies = false;
        let mut values = BTreeMap::new();
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
                    values.insert(k.trim().to_lowercase(), v.trim().to_lowercase());
                }
            }
        }
        for (key, value) in values {
            let unset = value == "unset";
            match key.as_str() {
                "indent_style" => {
                    format.indent_style = (!unset).then(|| {
                        if value == "tab" {
                            "tabs".into()
                        } else {
                            value.trim_end_matches('e').to_owned() + "es"
                        }
                    })
                }
                "indent_size" => {
                    format.indent_size = if value == "tab" {
                        format.indent_size
                    } else {
                        value.parse().ok()
                    }
                }
                "tab_width" if format.indent_size.is_none() => {
                    format.indent_size = value.parse().ok()
                }
                "end_of_line" => format.end_of_line = (!unset).then_some(value),
                "insert_final_newline" => {
                    format.insert_final_newline = if unset { None } else { Some(value == "true") }
                }
                "trim_trailing_whitespace" => {
                    format.trim_trailing_whitespace =
                        if unset { None } else { Some(value == "true") }
                }
                "charset" => {
                    format.charset = (!unset).then_some(if value == "latin1" {
                        "ISO-8859-1".into()
                    } else {
                        value
                    })
                }
                _ => (),
            }
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
