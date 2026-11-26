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
            .crlf(true)
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
    for m in scoped_matches(&re, text, scope) {
        if found.len() == MATCH_LIMIT {
            return Err("More than 20,000 matches; narrow the search.".into());
        }
        found.push(m.range());
    }
    Ok(found)
}

/// A scope limits which matches are accepted, while regex assertions still
/// see the surrounding text.
pub fn scoped_matches<'r, 'h>(
    regex: &'r Regex,
    text: &'h str,
    scope: Range<usize>,
) -> impl Iterator<Item = regex::Match<'h>> + use<'r, 'h> {
    let mut at = scope.start;
    let mut previous_end = None;
    std::iter::from_fn(move || {
        while at <= scope.end.min(text.len()) {
            let hit = regex.find_at(text, at)?;
            if hit.end() > scope.end {
                return None;
            }
            at = hit.end();
            if hit.is_empty() {
                at += text[at..].chars().next().map_or(1, char::len_utf8);
                if previous_end == Some(hit.end()) {
                    continue;
                }
            }
            previous_end = Some(hit.end());
            return Some(hit);
        }
        None
    })
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
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    if options.regex {
        for hit in &found {
            let captures = re.captures_at(text, hit.start).unwrap();
            out.push_str(&text[at..hit.start]);
            captures.expand(replacement, &mut out);
            at = hit.end;
            if out.len() > 64 * 1024 * 1024 {
                return Err("Replacement exceeds 64 MiB; narrow the search.".into());
            }
        }
    } else {
        for hit in &found {
            out.push_str(&text[at..hit.start]);
            out.push_str(replacement);
            at = hit.end;
            if out.len() > 64 * 1024 * 1024 {
                return Err("Replacement exceeds 64 MiB; narrow the search.".into());
            }
        }
    }
    if out.len() + text.len() - at > 64 * 1024 * 1024 {
        return Err("Replacement exceeds 64 MiB; narrow the search.".into());
    }
    out.push_str(&text[at..]);
    Ok((out, found.len()))
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Hit {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub text: String,
}
/// A line shown in the results: matching lines carry the column of their
/// first match, and the lines around them none.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContextLine {
    pub line: usize,
    pub text: String,
    pub column: Option<usize>,
}
/// One file's matches, with a line of context around each.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FileHits {
    pub path: PathBuf,
    pub count: usize,
    pub lines: Vec<ContextLine>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FilePlan {
    pub path: PathBuf,
    /// The text came from a file open in the editor rather than from disk.
    pub open: bool,
    pub before: String,
    pub after: String,
    pub hash: [u8; 32],
    pub encoding: String,
    pub bom: bool,
    pub selected: bool,
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct Results {
    pub hits: Vec<Hit>,
    pub files: Vec<FileHits>,
    pub plans: Vec<FilePlan>,
    pub skipped: usize,
    pub truncated: bool,
}

pub fn walk(roots: &[PathBuf], filter: &str, cancel: &AtomicBool) -> Result<Vec<PathBuf>, String> {
    walk_counting_skips(roots, filter, cancel).map(|(paths, _)| paths)
}

fn walk_counting_skips(
    roots: &[PathBuf],
    filter: &str,
    cancel: &AtomicBool,
) -> Result<(Vec<PathBuf>, usize), String> {
    let Some(first) = roots.first() else {
        return Err("Open a folder first.".into());
    };
    for root in roots {
        let metadata = std::fs::metadata(root)
            .map_err(|error| format!("Cannot search {}: {error}", root.display()))?;
        if !metadata.is_dir() {
            return Err(format!("{} is not a folder.", root.display()));
        }
    }
    let mut builder = ignore::WalkBuilder::new(first);
    for root in &roots[1..] {
        builder.add(root);
    }
    builder
        .hidden(false)
        .follow_links(false)
        .require_git(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            let directory = entry.file_type().is_some_and(|t| t.is_dir());
            (directory || !name.ends_with(".hedit-workspace"))
                && !crate::folder::excluded(&name, directory)
        });
    if filter.len() > 4096 {
        return Err("Filename filters are limited to 4096 bytes.".into());
    }
    let patterns = filename_patterns(filter)?;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut skipped = 0;
    for entry in builder.build() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Search cancelled.".into());
        }
        let Ok(entry) = entry else {
            skipped += 1;
            continue;
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.into_path();
        if !filename_matches(&path, roots, &patterns) {
            continue;
        }
        let Ok(path) = std::fs::canonicalize(&path) else {
            skipped += 1;
            continue;
        };
        if seen.insert(path.clone()) {
            out.push(path);
        }
        if out.len() > 50_000 {
            return Err("More than 50,000 files; narrow the folder or filter.".into());
        }
    }
    out.sort();
    Ok((out, skipped))
}

fn filename_patterns(filter: &str) -> Result<Vec<globset::GlobMatcher>, String> {
    let mut start = 0;
    let mut braces = 0usize;
    let mut class = false;
    let mut patterns = Vec::new();
    for (at, ch) in filter.char_indices() {
        match ch {
            '[' => class = true,
            ']' => class = false,
            '{' if !class => braces += 1,
            '}' if !class => braces = braces.saturating_sub(1),
            ',' if !class && braces == 0 => {
                patterns.push(&filter[start..at]);
                start = at + 1;
            }
            _ => (),
        }
    }
    patterns.push(&filter[start..]);
    patterns
        .into_iter()
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
        .collect()
}

fn filename_matches(path: &Path, roots: &[PathBuf], patterns: &[globset::GlobMatcher]) -> bool {
    patterns.is_empty()
        || patterns.iter().any(|pattern| {
            pattern.is_match(path.file_name().unwrap_or_default())
                || pattern.is_match(path)
                || roots.iter().any(|root| {
                    path.strip_prefix(root)
                        .is_ok_and(|relative| pattern.is_match(relative))
                })
        })
}

fn draft_is_ignored(path: &Path, root: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    if relative.ancestors().any(|part| {
        part.file_name().is_some_and(|name| {
            let name = name.to_string_lossy();
            crate::folder::excluded(&name, part != relative)
                || (part == relative && name.ends_with(".hedit-workspace"))
        })
    }) {
        return true;
    }
    for filename in [".ignore", ".gitignore"] {
        for directory in path
            .ancestors()
            .skip(1)
            .take_while(|dir| dir.starts_with(root))
        {
            let rules = directory.join(filename);
            if rules.is_file() {
                let (matcher, _) = ignore::gitignore::Gitignore::new(rules);
                let matched = matcher.matched_path_or_any_parents(path, false);
                if !matched.is_none() {
                    return matched.is_ignore();
                }
            }
        }
    }
    if let Some(repo) = path
        .ancestors()
        .skip(1)
        .find(|dir| dir.join(".git").is_dir())
    {
        let rules = repo.join(".git/info/exclude");
        if rules.is_file() {
            let mut builder = ignore::gitignore::GitignoreBuilder::new(repo);
            builder.add(rules);
            if let Ok(matcher) = builder.build() {
                return matcher.matched_path_or_any_parents(path, false).is_ignore();
            }
        }
    }
    false
}
pub fn glob(pattern: &str, text: &str) -> bool {
    globset::GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .is_ok_and(|g| g.compile_matcher().is_match(text))
}
/// The lines to show for matches on `matched` lines (1-based, with the
/// column of the first match): each with the line before and after it.
fn context(text: &str, matched: &[(usize, usize, String)]) -> Vec<ContextLine> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut wanted: Vec<usize> = matched
        .iter()
        .flat_map(|(line, _, _)| line.saturating_sub(1).max(1)..=(line + 1).min(lines.len()))
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    wanted
        .into_iter()
        .map(|line| {
            let hit = matched
                .binary_search_by_key(&line, |(number, _, _)| *number)
                .ok();
            ContextLine {
                line,
                text: hit.map_or_else(
                    || {
                        lines[line - 1]
                            .trim_end_matches('\r')
                            .chars()
                            .take(240)
                            .collect()
                    },
                    |index| matched[index].2.clone(),
                ),
                column: hit.map(|index| matched[index].1),
            }
        })
        .collect()
}

fn excerpt(line: &str, match_byte: usize) -> String {
    const LIMIT: usize = 240;
    let line = line.trim_end_matches('\r');
    if line.len() <= LIMIT || line.chars().nth(LIMIT).is_none() {
        return line.into();
    }
    let before = line[..match_byte.min(line.len())].chars().count();
    let skip = before.saturating_sub(LIMIT / 2);
    let mut text: String = (skip > 0).then_some('…').into_iter().collect();
    text.extend(line.chars().skip(skip).take(LIMIT - usize::from(skip > 0)));
    text
}
/// Search the files under `roots`, reading files open in the editor from
/// `open` (their current, possibly unsaved, text) instead of from disk, and
/// plan replacements when `replacement` is given.
pub fn folders(
    roots: &[PathBuf],
    filter: &str,
    query: &str,
    replacement: Option<&str>,
    options: &Options,
    open: &std::collections::HashMap<PathBuf, String>,
    cancel: &AtomicBool,
) -> Result<Results, String> {
    if let Some(first) = roots.first() {
        if let Some(result) = crate::backend::route(
            first,
            crate::backend::Call::Search {
                roots: roots.to_vec(),
                filter: filter.into(),
                query: query.into(),
                replacement: replacement.map(str::to_owned),
                options: options.clone(),
                open: open.clone(),
            },
            cancel,
        ) {
            return result;
        }
    }

    let regex = options.compile(query)?;
    let mut result = Results::default();
    let mut retained = 0;
    let (mut paths, skipped) = walk_counting_skips(roots, filter, cancel)?;
    result.skipped = skipped;
    let unsaved = open.keys().filter(|path| {
        path.is_absolute()
            && path
                .symlink_metadata()
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    });
    if unsaved.clone().next().is_some() {
        let roots: Vec<_> = roots
            .iter()
            .filter_map(|root| root.canonicalize().ok())
            .collect();
        let patterns = filename_patterns(filter)?;
        paths.extend(
            unsaved
                .filter(|path| {
                    filename_matches(path, &roots, &patterns)
                        && roots
                            .iter()
                            .filter(|root| path.starts_with(root))
                            .max_by_key(|root| root.components().count())
                            .is_some_and(|root| !draft_is_ignored(path, root))
                })
                .cloned(),
        );
        paths.sort();
        paths.dedup();
        if paths.len() > 50_000 {
            return Err("More than 50,000 files; narrow the folder or filter.".into());
        }
    }
    for path in paths {
        if cancel.load(Ordering::Relaxed) {
            return Err("Search cancelled.".into());
        }
        let in_editor = open.get(&path);
        let (text, bytes, encoding, bom) = if let Some(text) = in_editor {
            (text.clone(), vec![], "", false)
        } else {
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
            (text, bytes, encoding, bom)
        };
        let first_hit = result.hits.len();
        let mut matched = Vec::new();
        let mut line = 1;
        let mut column = 1;
        let mut previous = 0;
        let mut line_start = 0;
        for hit in regex.find_iter(&text) {
            if result.hits.len() == MATCH_LIMIT {
                result.truncated = true;
                break;
            }
            for newline in memchr::memchr_iter(b'\n', &text.as_bytes()[previous..hit.start()]) {
                line += 1;
                line_start = previous + newline + 1;
                column = 1;
            }
            column += text[line_start.max(previous)..hit.start()]
                .encode_utf16()
                .count();
            previous = hit.start();
            if matched.last().is_none_or(|(number, _, _)| *number != line) {
                let end = text[hit.start()..]
                    .find('\n')
                    .map_or(text.len(), |i| hit.start() + i);
                matched.push((
                    line,
                    column,
                    excerpt(&text[line_start..end], hit.start() - line_start),
                ));
            }
            result.hits.push(Hit {
                path: path.clone(),
                line,
                column,
                text: matched.last().unwrap().2.clone(),
            });
        }
        if result.hits.len() == first_hit {
            if result.truncated {
                break;
            }
            continue;
        }
        result.files.push(FileHits {
            path: path.clone(),
            count: result.hits.len() - first_hit,
            lines: context(&text, &matched),
        });
        if result.truncated {
            break;
        }
        if let Some(replacement) = replacement {
            let (after, _) = replace(&text, query, replacement, options, 0..text.len())?;
            if after == text {
                continue;
            }
            retained += text.len() + after.len();
            if retained > 128 * 1024 * 1024 {
                return Err("Replacement preview exceeds 128 MiB; narrow the search.".into());
            }
            result.plans.push(FilePlan {
                path,
                open: in_editor.is_some(),
                before: text,
                after,
                hash: Sha256::digest(&bytes).into(),
                encoding: encoding.into(),
                bom,
                selected: true,
            });
        }
    }
    Ok(result)
}
/// What a replacement wrote to disk, to put the file back.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Restore {
    pub path: PathBuf,
    pub before: String,
    /// The file's bytes after the replacement, to tell whether it changed since.
    pub written: [u8; 32],
}
/// Put back a file changed by `apply`, unless it has changed since.
pub fn restore(record: &Restore) -> Result<(), String> {
    if let Some(result) = crate::backend::route(
        &record.path,
        crate::backend::Call::Restore(record.clone()),
        &AtomicBool::new(false),
    ) {
        return result;
    }

    let current = std::fs::read(&record.path).map_err(|e| e.to_string())?;
    if <[u8; 32]>::from(Sha256::digest(&current)) != record.written {
        return Err(format!(
            "{} changed after the replacement; it was left as it is.",
            record.path.display()
        ));
    }
    let mut doc = crate::document::Document::open(1, &record.path)?;
    doc.replace(
        0,
        doc.info().size,
        record.before.clone(),
        doc.info().revision,
    )?;
    doc.save(None)?;
    Ok(())
}
/// Apply a reviewed replacement and return how to undo it.
pub fn apply_recorded(plan: &FilePlan) -> Result<Restore, String> {
    if let Some(result) = crate::backend::route(
        &plan.path,
        crate::backend::Call::Replace(plan.clone()),
        &AtomicBool::new(false),
    ) {
        return result;
    }

    apply(plan)?;
    let written = std::fs::read(&plan.path).map_err(|e| e.to_string())?;
    Ok(Restore {
        path: plan.path.clone(),
        before: plan.before.clone(),
        written: Sha256::digest(&written).into(),
    })
}
pub fn apply(plan: &FilePlan) -> Result<(), String> {
    if plan.open {
        return Err(format!(
            "{} is open; its replacement belongs in the editor.",
            plan.path.display()
        ));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn search_folder(root: &Path, replacement: Option<&str>) -> Result<Results, String> {
        folders(
            &[root.to_owned()],
            "",
            "needle",
            replacement,
            &Options::default(),
            &Default::default(),
            &AtomicBool::new(false),
        )
    }

    #[test]
    fn regression_folder_search_limits_dense_files_without_losing_results() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("dense.txt");
        std::fs::write(&file, "needle ".repeat(MATCH_LIMIT + 1)).unwrap();
        let excess = search_folder(dir.path(), None).unwrap();
        assert_eq!(excess.hits.len(), MATCH_LIMIT);
        assert!(excess.truncated);
        std::fs::write(file, "needle ".repeat(MATCH_LIMIT)).unwrap();
        let exact = search_folder(dir.path(), None).unwrap();
        assert_eq!(exact.hits.len(), MATCH_LIMIT);
        assert!(!exact.truncated);
    }

    #[test]
    fn regression_folder_search_omits_unchanged_replacement_plans() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), "needle\n").unwrap();
        let results = search_folder(dir.path(), Some("needle")).unwrap();
        assert_eq!(results.hits.len(), 1);
        assert!(results.plans.is_empty());
    }

    #[test]
    fn regression_regex_line_anchors_preserve_crlf_on_replacement() {
        let source = "needle\r\nnext\n";
        let options = Options {
            regex: true,
            ..Options::default()
        };
        let (result, count) =
            replace(source, "^needle$", "new", &options, 0..source.len()).unwrap();
        assert_eq!((result.as_str(), count), ("new\r\nnext\n", 1));
    }

    #[test]
    fn regression_folder_search_reports_a_missing_root() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let error = search_folder(&missing, None)
            .err()
            .expect("missing folder looked empty");
        assert!(error.contains("missing"));
    }

    #[test]
    fn regression_folder_search_snippet_shows_the_match_after_a_long_prefix() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("file.txt"),
            format!("{}needle tail\n", "🙂".repeat(280)),
        )
        .unwrap();
        let results = search_folder(dir.path(), None).unwrap();
        assert_eq!(results.hits[0].column, 561);
        assert!(results.files[0].lines[0].text.contains("needle tail"));
    }
}
