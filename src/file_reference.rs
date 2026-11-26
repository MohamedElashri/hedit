//! Extract local file references and resolve them without a shell or file creation.
use crate::advanced::{self, Selection};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const LIMIT: usize = 4096;

fn clean(value: &str) -> Option<String> {
    (!value.trim().is_empty()
        && value.len() <= LIMIT
        && !value.contains(['\n', '\r', '\0'])
        && !value.contains("://"))
    .then(|| value.to_owned())
}

fn location_suffix(suffix: &str) -> Option<&str> {
    let numbers = suffix.strip_prefix(':')?;
    (numbers.split(':').count() <= 2
        && numbers
            .split(':')
            .all(|part| part.parse::<u64>().is_ok_and(|n| n > 0)))
    .then_some(suffix)
}

/// A selection takes precedence; otherwise inspect the cursor's line only.
/// Quoted paths may contain spaces; bare paths stop at common source delimiters.
pub fn at(text: &str, selection: Selection) -> Option<String> {
    let start = advanced::byte(text, selection.start());
    let end = advanced::byte(text, selection.end());
    if start != end {
        let selected = text.get(start..end)?.trim();
        let unwrapped = ['\'', '"', '`', '<'].into_iter().find_map(|open| {
            let close = if open == '<' { '>' } else { open };
            let (path, suffix) = selected.strip_prefix(open)?.rsplit_once(close)?;
            if suffix.is_empty() {
                clean(path)
            } else {
                clean(&format!("{path}{}", location_suffix(suffix)?))
            }
        });
        return unwrapped.or_else(|| clean(selected));
    }
    let before = &text[..start];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let line = &text[line_start..line_end];
    if line.len() > LIMIT {
        return None;
    }
    let position = start - line_start;
    let include_open = line
        .trim_start()
        .strip_prefix('#')
        .and_then(|s| s.trim_start().strip_prefix("include"))
        .map(str::trim_start)
        .filter(|s| s.starts_with('<'))
        .map(|s| line.len() - s.len());
    let mut quoted = None;
    for (offset, ch) in line.char_indices() {
        match quoted {
            Some((quote, begin)) if ch == quote => {
                let after_quote = offset + ch.len_utf8();
                let suffix = line[after_quote..]
                    .split(|c: char| c != ':' && !c.is_ascii_digit())
                    .next()
                    .unwrap_or("")
                    .trim_end_matches(':');
                let suffix = location_suffix(suffix).unwrap_or_default();
                let end = after_quote + suffix.len();
                if begin - 1 <= position && position <= end {
                    return clean(&format!("{}{suffix}", &line[begin..offset]));
                }
                quoted = None;
            }
            None if include_open == Some(offset) => {
                quoted = Some(('>', offset + 1));
            }
            None if matches!(ch, '\'' | '"' | '`')
                && line[..offset]
                    .chars()
                    .next_back()
                    .is_none_or(|before| !before.is_alphanumeric() && before != '_') =>
            {
                quoted = Some((ch, offset + ch.len_utf8()));
            }
            _ => {}
        }
    }
    let delimiter = |c: char| c.is_whitespace() || "()[]{}<>,;='\"`".contains(c);
    let left = line[..position]
        .rfind(delimiter)
        .map_or(0, |i| i + line[i..].chars().next().unwrap().len_utf8());
    let right = line[position..]
        .find(delimiter)
        .map_or(line.len(), |i| position + i);
    // A log's trailing colon is punctuation; quoted/selected names are literal.
    clean(line[left..right].trim_end_matches(':'))
}

fn existing(candidate: PathBuf) -> Option<PathBuf> {
    let (path, _, _) = crate::cli::location(&candidate);
    if !path.is_file() {
        return None;
    }
    let canonical = path.canonicalize().ok()?;
    if path == candidate {
        Some(canonical)
    } else {
        // Keep the original basename and suffix. Re-encoding a symlink's
        // canonical target could collide with a different literal colon filename.
        Some(
            candidate
                .parent()?
                .canonicalize()
                .ok()?
                .join(candidate.file_name()?),
        )
    }
}

/// Paths beside the current document take priority over project-root matches.
/// Each root also supplies an `include` directory for C-family includes.
pub fn resolve(reference: &str, document: Option<&Path>, roots: &[PathBuf]) -> Vec<PathBuf> {
    let Some(reference) = clean(reference) else {
        return vec![];
    };
    let reference = Path::new(&reference);
    if reference.is_absolute() {
        return existing(reference.into()).into_iter().collect();
    }
    if let Some(parent) = document.and_then(Path::parent) {
        if let Some(found) = existing(parent.join(reference)) {
            return vec![found];
        }
    }
    let mut found = BTreeSet::new();
    for root in roots {
        for base in [root.clone(), root.join("include")] {
            if let Some(path) = existing(base.join(reference)) {
                found.insert(path);
            }
        }
    }
    found.into_iter().collect()
}
