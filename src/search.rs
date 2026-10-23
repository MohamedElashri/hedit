//! Shared Unicode search and reviewable, guarded folder replacements.
use regex::{Regex, RegexBuilder};
use sha2::{Digest, Sha256};
use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub const MATCH_LIMIT: usize = 20_000;
pub const FILE_LIMIT: u64 = 16 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Options {
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            regex: false,
            case_sensitive: true,
            whole_word: false,
        }
    }
}
impl Options {
    pub fn compile(&self, query: &str) -> Result<Regex, String> {
        if query.is_empty() || query.len() > 4096 {
            return Err("Enter 1–4096 bytes to search.".into());
        }
        let mut pattern = if self.regex {
            query.to_owned()
        } else {
            regex::escape(query)
        };
        if self.whole_word {
            pattern = format!(r"\b(?:{pattern})\b");
        }
        RegexBuilder::new(&pattern)
            .case_insensitive(!self.case_sensitive)
            .multi_line(true)
            .size_limit(2 * 1024 * 1024)
            .dfa_size_limit(4 * 1024 * 1024)
            .build()
            .map_err(|e| e.to_string())
    }
}
pub fn matches(
    text: &str,
    query: &str,
    options: &Options,
    scope: Range<usize>,
) -> Result<Vec<Range<usize>>, String> {
    if !text.is_char_boundary(scope.start)
        || !text.is_char_boundary(scope.end)
        || scope.start > scope.end
    {
        return Err("Invalid search selection.".into());
    }
    let re = options.compile(query)?;
    let mut found = Vec::new();
    for m in re.find_iter(&text[scope.clone()]) {
        if found.len() == MATCH_LIMIT {
            return Err("More than 20,000 matches; narrow the search.".into());
        }
        found.push(m.start() + scope.start..m.end() + scope.start);
    }
    Ok(found)
}
pub fn replace(
    text: &str,
    query: &str,
    replacement: &str,
    options: &Options,
    scope: Range<usize>,
) -> Result<(String, usize), String> {
    let found = matches(text, query, options, scope.clone())?;
    let re = options.compile(query)?;
    let replacement = if options.regex {
        re.replace_all(&text[scope.clone()], replacement)
    } else {
        re.replace_all(&text[scope.clone()], regex::NoExpand(replacement))
    };
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..scope.start]);
    out.push_str(&replacement);
    out.push_str(&text[scope.end..]);
    if out.len() > 64 * 1024 * 1024 {
        return Err("Replacement exceeds 64 MiB; narrow the search.".into());
    }
    Ok((out, found.len()))
}
pub struct Hit {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub text: String,
}
pub struct FilePlan {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
    pub hash: [u8; 32],
    pub encoding: String,
    pub bom: bool,
    pub selected: bool,
}
#[derive(Default)]
pub struct Results {
    pub hits: Vec<Hit>,
    pub plans: Vec<FilePlan>,
    pub skipped: usize,
    pub truncated: bool,
}

pub fn walk(roots: &[PathBuf], filter: &str, cancel: &AtomicBool) -> Result<Vec<PathBuf>, String> {
    let Some(first) = roots.first() else {
        return Err("Open a folder first.".into());
    };
    let mut builder = ignore::WalkBuilder::new(first);
    for root in &roots[1..] {
        builder.add(root);
    }
    builder
        .hidden(false)
        .follow_links(false)
        .require_git(false)
        .filter_entry(|entry| {
            if entry
                .file_name()
                .to_string_lossy()
                .ends_with(".hedit-workspace")
            {
                return false;
            }
            !crate::folder::excluded(
                &entry.file_name().to_string_lossy(),
                entry.file_type().is_some_and(|t| t.is_dir()),
            )
        });
    if filter.len() > 4096 {
        return Err("Filename filters are limited to 4096 bytes.".into());
    }
    let patterns = filter
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|pattern| {
            globset::GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(false)
                .build()
                .map(|g| g.compile_matcher())
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for entry in builder.build() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Search cancelled.".into());
        }
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.into_path();
        if !patterns.is_empty()
            && !patterns.iter().any(|p| {
                p.is_match(path.file_name().unwrap_or_default())
                    || p.is_match(&path)
                    || roots.iter().any(|r| {
                        path.strip_prefix(r)
                            .is_ok_and(|relative| p.is_match(relative))
                    })
            })
        {
            continue;
        }
        let path = std::fs::canonicalize(&path).map_err(|e| e.to_string())?;
        if seen.insert(path.clone()) {
            out.push(path);
        }
        if out.len() > 50_000 {
            return Err("More than 50,000 files; narrow the folder or filter.".into());
        }
    }
    out.sort();
    Ok(out)
}
pub fn glob(pattern: &str, text: &str) -> bool {
    globset::GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .is_ok_and(|g| g.compile_matcher().is_match(text))
}
pub fn folders(
    roots: &[PathBuf],
    filter: &str,
    query: &str,
    replacement: Option<&str>,
    options: &Options,
    cancel: &AtomicBool,
) -> Result<Results, String> {
    options.compile(query)?;
    let mut result = Results::default();
    let mut retained = 0;
    for path in walk(roots, filter, cancel)? {
        if cancel.load(Ordering::Relaxed) {
            return Err("Search cancelled.".into());
        }
        if std::fs::metadata(&path).map_or(true, |m| m.len() > FILE_LIMIT) {
            result.skipped += 1;
            continue;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => {
                result.skipped += 1;
                continue;
            }
        };
        let encoding = crate::text_encoding::detect(&bytes);
        let (text, bom) = match crate::text_encoding::decode(&bytes, encoding) {
            Ok(t) => t,
            Err(_) => {
                result.skipped += 1;
                continue;
            }
        };
        let found = matches(&text, query, options, 0..text.len())?;
        if found.is_empty() {
            continue;
        }
        for m in &found {
            let before = &text[..m.start];
            let start = before.rfind('\n').map_or(0, |i| i + 1);
            let end = text[m.start..]
                .find('\n')
                .map_or(text.len(), |i| m.start + i);
            result.hits.push(Hit {
                path: path.clone(),
                line: before.bytes().filter(|b| *b == b'\n').count() + 1,
                column: text[start..m.start].encode_utf16().count() + 1,
                text: text[start..end].chars().take(240).collect(),
            });
            if result.hits.len() >= MATCH_LIMIT {
                result.truncated = true;
                break;
            }
        }
        if let Some(replacement) = replacement {
            let (after, _) = replace(&text, query, replacement, options, 0..text.len())?;
            retained += text.len() + after.len();
            if retained > 128 * 1024 * 1024 {
                return Err("Replacement preview exceeds 128 MiB; narrow the search.".into());
            }
            result.plans.push(FilePlan {
                path,
                before: text,
                after,
                hash: Sha256::digest(&bytes).into(),
                encoding: encoding.into(),
                bom,
                selected: true,
            });
        }
        if result.truncated {
            break;
        }
    }
    Ok(result)
}
pub fn apply(plan: &FilePlan) -> Result<(), String> {
    let current = std::fs::read(&plan.path).map_err(|e| e.to_string())?;
    if <[u8; 32]>::from(Sha256::digest(&current)) != plan.hash {
        return Err(format!(
            "{} changed since preview; search again.",
            plan.path.display()
        ));
    }
    let mut doc = crate::document::Document::open(1, &plan.path)?;
    if doc.text(FILE_LIMIT)? != plan.before
        || doc.info().encoding != plan.encoding
        || doc.info().bom != plan.bom
    {
        return Err("The file changed while opening the reviewed replacement.".into());
    }
    doc.replace(0, doc.info().size, plan.after.clone(), doc.info().revision)?;
    doc.save(None)?;
    Ok(())
}
pub fn diff(before: &str, after: &str, name: &Path) -> String {
    similar::TextDiff::configure()
        .timeout(std::time::Duration::from_millis(250))
        .diff_lines(before, after)
        .unified_diff()
        .context_radius(3)
        .header(
            &format!("{} (before)", name.display()),
            &format!("{} (after)", name.display()),
        )
        .to_string()
}
