//! Lightweight text navigation and completion; does not require a language server.
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
pub struct Symbol {
    pub path: PathBuf,
    pub name: String,
    pub line: usize,
}
pub fn symbols(text: &str, path: &Path) -> Vec<Symbol> {
    let definitions=regex::Regex::new(r"(?x)^\s*(?:(?:pub(?:\([^)]*\))?|async|export|default|static|inline|virtual|const|unsafe|extern)\s+)*(?:(?:fn|def|class|struct|enum|trait|interface|function|type|mod|namespace)\s+([\pL_][\pL\pN_]*)|([\pL_][\pL\pN_:<>*&\s]*)\s+([\pL_][\pL\pN_]*)\s*\([^;]*\)\s*(?:const\s*)?\{)").unwrap();
    text.lines()
        .enumerate()
        .filter_map(|(line, text)| {
            if ["//", "#", "/*", "*", "if ", "for ", "while ", "switch "]
                .iter()
                .any(|p| text.trim_start().starts_with(p))
            {
                return None;
            }
            let c = definitions.captures(text)?;
            let name = c.get(1).or_else(|| c.get(3))?.as_str().to_owned();
            Some(Symbol {
                path: path.to_owned(),
                name,
                line: line + 1,
            })
        })
        .take(10_000)
        .collect()
}
pub fn completions(text: &str, cursor: usize) -> (std::ops::Range<usize>, Vec<String>) {
    let byte = crate::advanced::byte(text, cursor);
    let start = text[..byte]
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
        .last()
        .map_or(byte, |(i, _)| i);
    let prefix = &text[start..byte];
    if prefix.is_empty() {
        return (start..byte, vec![]);
    }
    let mut words = BTreeSet::new();
    for word in text.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if word.starts_with(prefix) && word != prefix && word.len() <= 256 {
            words.insert(word.to_owned());
        }
        if words.len() >= 200 {
            break;
        }
    }
    (start..byte, words.into_iter().collect())
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Snippet {
    pub trigger: String,
    pub description: String,
    pub body: String,
    pub language: Option<String>,
}
pub fn expand(body: &str, selection: &str) -> (String, usize) {
    let mut segments = body.split("$0");
    let first = segments
        .next()
        .unwrap_or_default()
        .replace("${SELECTION}", selection);
    let cursor = first.chars().count();
    let mut text = first;
    for segment in segments {
        text.push_str(&segment.replace("${SELECTION}", selection));
    }
    (text, cursor)
}
pub fn default_snippets() -> Vec<Snippet> {
    vec![
        Snippet {
            trigger: "todo".into(),
            description: "TODO note".into(),
            body: "TODO: $0".into(),
            language: None,
        },
        Snippet {
            trigger: "guard".into(),
            description: "C/C++ header guard".into(),
            body: "#pragma once\n\n${SELECTION}$0".into(),
            language: Some("cpp".into()),
        },
    ]
}

pub fn git_diff(path: &Path, text: &str) -> Result<String, String> {
    let parent = path.parent().ok_or("No file directory.")?;
    let root = git_output(parent, &["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(root.trim_end_matches(['\r', '\n']));
    let relative = path.strip_prefix(&root).map_err(|e| e.to_string())?;
    let relative = relative
        .to_str()
        .ok_or("Git paths must be UTF-8.")?
        .replace('\\', "/");
    let base = git_output(&root, &["show", &format!("HEAD:{relative}")])?;
    Ok(crate::search::diff(
        base.strip_prefix('\u{feff}').unwrap_or(&base),
        text,
        path,
    ))
}
pub fn git_output(directory: &Path, args: &[&str]) -> Result<String, String> {
    let mut child = Command::new("git")
        .args(["--no-pager", "--literal-pathspecs"])
        .args(args)
        .current_dir(directory)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut output = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output
            .by_ref()
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if start.elapsed() > std::time::Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Git operation timed out.".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let bytes = reader
        .join()
        .map_err(|_| "Git output worker failed.")?
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("Git HEAD is unavailable for this file.".into());
    }
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Git output exceeds 16 MiB.".into());
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}
pub fn changed_lines(diff: &str) -> BTreeSet<usize> {
    let re = regex::Regex::new(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@").unwrap();
    let mut lines = BTreeSet::new();
    let mut current = None;
    for line in diff.lines() {
        if let Some(c) = re.captures(line) {
            current = c[1].parse::<usize>().ok();
            continue;
        }
        if let Some(n) = current.as_mut() {
            match line.as_bytes().first() {
                Some(b'+') => {
                    lines.insert((*n).max(1));
                    *n += 1;
                }
                Some(b'-') => {
                    lines.insert((*n).max(1));
                }
                Some(b' ') => *n += 1,
                _ => (),
            }
        }
    }
    lines
}
pub fn hunk_at(diff: &str, line: usize) -> String {
    let re = regex::Regex::new(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@").unwrap();
    let mut selected = false;
    let mut out = String::new();
    for text in diff.lines() {
        if let Some(c) = re.captures(text) {
            if selected {
                break;
            }
            let start = c[1].parse::<usize>().unwrap_or(1).max(1);
            let count = c
                .get(2)
                .map_or(1, |m| m.as_str().parse::<usize>().unwrap_or(1));
            selected = start <= line && line <= start.saturating_add(count);
        }
        if selected {
            out.push_str(text);
            out.push('\n');
            if out.len() > 16 * 1024 {
                out.push('…');
                break;
            }
        }
    }
    out
}
