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
/// A place in a file: 1-based line and column, and the line's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub line: usize,
    pub column: usize,
    pub text: String,
}

fn place(line: usize, text: &str, byte: usize) -> Place {
    Place {
        line: line + 1,
        column: text[..byte].chars().count() + 1,
        text: text.trim().chars().take(160).collect(),
    }
}

/// Where `name` is defined: functions, types, and other symbols, plus
/// `#define`, `typedef`, and variable or constant declarations.
pub fn definitions(text: &str, name: &str, path: &Path) -> Vec<Place> {
    let word = regex::escape(name);
    let Ok(other) = regex::Regex::new(&format!(
        r"^\s*(?:#\s*define\s+({word})\b|typedef\b.*\b({word})\s*;|(?:(?:pub(?:\([^)]*\))?|export|static|final|private|public|protected)\s+)*(?:const|let|var|val|static|macro_rules!|using|alias)\s+(?:mut\s+)?({word})\b|({word})\s*=[^=]|((?:[\pL_][\pL\pN_:<>]*[\s*&]+)+)({word})\s*(?:=[^=]|;|\[|,))"
    )) else {
        return vec![];
    };
    let symbol_lines: std::collections::HashSet<usize> = symbols(text, path)
        .into_iter()
        .filter(|s| s.name == name)
        .map(|s| s.line)
        .collect();
    let at_word = |line: &str| {
        line.match_indices(name)
            .find(|(i, _)| {
                let before = line[..*i].chars().last();
                let after = line[i + name.len()..].chars().next();
                !before.is_some_and(is_word) && !after.is_some_and(is_word)
            })
            .map(|(i, _)| i)
    };
    text.lines()
        .enumerate()
        .filter_map(|(n, line)| {
            if symbol_lines.contains(&(n + 1)) {
                return at_word(line).map(|i| place(n, line, i));
            }
            let found = other.captures(line)?;
            // `type name = …;` declarations, but not `return name;`.
            if let Some(kind) = found.get(5) {
                let first = kind.as_str().split_whitespace().next().unwrap_or("");
                if matches!(
                    first,
                    "return"
                        | "else"
                        | "case"
                        | "goto"
                        | "delete"
                        | "throw"
                        | "new"
                        | "yield"
                        | "await"
                        | "print"
                        | "echo"
                        | "assert"
                        | "del"
                        | "not"
                        | "and"
                        | "or"
                ) {
                    return None;
                }
            }
            let m = [1, 2, 3, 4, 6].into_iter().find_map(|g| found.get(g))?;
            Some(place(n, line, m.start()))
        })
        .take(1_000)
        .collect()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Lines that use `name` as a whole word, with the first use on each line.
pub fn references(text: &str, name: &str, limit: usize) -> Vec<Place> {
    if name.is_empty() {
        return vec![];
    }
    let mut found = vec![];
    for (n, line) in text.lines().enumerate() {
        if found.len() >= limit {
            break;
        }
        let first = line.match_indices(name).find(|(i, _)| {
            let before = line[..*i].chars().last();
            let after = line[i + name.len()..].chars().next();
            !before.is_some_and(is_word) && !after.is_some_and(is_word)
        });
        if let Some((i, _)) = first {
            found.push(place(n, line, i));
        }
    }
    found
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
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Snippet {
    pub trigger: String,
    pub description: String,
    pub body: String,
    pub language: Option<String>,
}
/// Expand a snippet body and return the text with the final cursor position.
pub fn expand(body: &str, selection: &str) -> (String, usize) {
    let expansion = expand_fields(body, selection);
    let cursor = expansion.fields.last().map_or(0, |f| f[0].0);
    (expansion.text, cursor)
}

/// A snippet body after expansion: its text, and its fields in the order Tab
/// visits them. Each field lists every place it appears as character ranges;
/// the last field is where the cursor ends (`$0`, or the end of the text).
#[derive(Debug, PartialEq, Eq)]
pub struct Expansion {
    pub text: String,
    pub fields: Vec<Vec<(usize, usize)>>,
}

/// Expand `$1`, `${1:default}` (fields may nest, and a number used again
/// mirrors the first one's text), `$0`, and `${SELECTION}`. `\$`, `\}`, and
/// `\\` are literal, as is a `$` that starts none of these.
pub fn expand_fields(body: &str, selection: &str) -> Expansion {
    let chars: Vec<char> = body.chars().collect();
    let mut first = FieldParser {
        chars: &chars,
        selection,
        defaults: Default::default(),
        text: String::new(),
        count: 0,
        fields: Default::default(),
    };
    first.parse(&mut 0, false);
    // A bare `$1` before `${1:text}` still shows `text`.
    let mut parser = FieldParser {
        defaults: first.defaults,
        text: String::new(),
        count: 0,
        fields: Default::default(),
        ..first
    };
    parser.parse(&mut 0, false);
    let FieldParser {
        text,
        count,
        mut fields,
        ..
    } = parser;
    let last = fields.remove(&0).unwrap_or_else(|| vec![(count, count)]);
    let mut fields: Vec<_> = fields.into_values().collect();
    fields.push(last);
    Expansion { text, fields }
}

struct FieldParser<'a> {
    chars: &'a [char],
    selection: &'a str,
    defaults: std::collections::HashMap<u32, String>,
    text: String,
    /// Characters in `text`.
    count: usize,
    fields: std::collections::BTreeMap<u32, Vec<(usize, usize)>>,
}

impl FieldParser<'_> {
    fn push(&mut self, text: &str) {
        self.text.push_str(text);
        self.count += text.chars().count();
    }
    fn number(&self, at: &mut usize) -> Option<u32> {
        let digits = self.chars[*at..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        let number = self.chars[*at..*at + digits]
            .iter()
            .collect::<String>()
            .parse()
            .ok()?;
        *at += digits;
        Some(number)
    }
    fn field(&mut self, number: u32, start: usize) {
        self.fields
            .entry(number)
            .or_default()
            .push((start, self.count));
    }
    /// Parse until the end, or until an unescaped `}` when `nested`.
    fn parse(&mut self, at: &mut usize, nested: bool) {
        let chars = self.chars;
        while *at < chars.len() {
            let c = chars[*at];
            match c {
                '\\' if matches!(chars.get(*at + 1), Some('$' | '}' | '\\')) => {
                    self.push(&chars[*at + 1].to_string());
                    *at += 2;
                }
                '}' if nested => {
                    *at += 1;
                    return;
                }
                '$' => {
                    let mut next = *at + 1;
                    if let Some(number) = self.number(&mut next) {
                        let start = self.count;
                        if let Some(default) = self.defaults.get(&number).cloned() {
                            self.push(&default);
                        }
                        self.field(number, start);
                        *at = next;
                        continue;
                    }
                    if chars.get(next) == Some(&'{') {
                        let rest: String = chars[next..].iter().take(12).collect();
                        if rest.starts_with("{SELECTION}") {
                            let selection = self.selection;
                            self.push(selection);
                            *at = next + "{SELECTION}".len();
                            continue;
                        }
                        let mut inner = next + 1;
                        if let Some(number) = self.number(&mut inner) {
                            match chars.get(inner) {
                                Some('}') => {
                                    let start = self.count;
                                    if let Some(default) = self.defaults.get(&number).cloned() {
                                        self.push(&default);
                                    }
                                    self.field(number, start);
                                    *at = inner + 1;
                                    continue;
                                }
                                Some(':') => {
                                    let start = self.count;
                                    let byte = self.text.len();
                                    *at = inner + 1;
                                    self.parse(at, true);
                                    let default = self.text[byte..].to_owned();
                                    self.defaults.entry(number).or_insert(default);
                                    self.field(number, start);
                                    continue;
                                }
                                _ => {}
                            }
                        }
                    }
                    self.push("$");
                    *at += 1;
                }
                c => {
                    self.text.push(c);
                    self.count += 1;
                    *at += 1;
                }
            }
        }
    }
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
    // Git reports the real location; resolve symlinks (such as /var → /private/var
    // on macOS) in the file path too, so the repository prefix matches.
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let relative = resolved.strip_prefix(&root).map_err(|e| e.to_string())?;
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
