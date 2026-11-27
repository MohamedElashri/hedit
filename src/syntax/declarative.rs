//! Bounded declarative grammars using literal delimiters and native theme roles.
use super::{lexer, Language};
use crate::theme::Theme;
use eframe::egui::Color32;
use serde::Deserialize;
use std::collections::BTreeSet;
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub name: String,
    pub extensions: Vec<String>,
    #[serde(default)]
    pub keywords: BTreeSet<String>,
    #[serde(default)]
    pub line_comment: Option<String>,
    #[serde(default)]
    pub quoted_strings: bool,
    #[serde(default)]
    regions: Vec<Region>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Region {
    start: String,
    end: String,
    kind: RegionKind,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    escape: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RegionKind {
    Comment,
    String,
    Embedded,
}

impl Region {
    fn closing_offset(&self, text: &str) -> Option<usize> {
        if !self.escape {
            return text.find(&self.end);
        }
        let mut escaped = false;
        for (at, ch) in text.char_indices() {
            if !escaped && text[at..].starts_with(&self.end) {
                return Some(at);
            }
            escaped = !escaped && ch == '\\';
        }
        None
    }

    fn highlight(&self, text: &str, theme: &Theme, runs: &mut Vec<(usize, Color32)>) -> usize {
        let body = &text[self.start.len()..];
        let close = self.closing_offset(body);
        let content_len = close.unwrap_or(body.len());
        let end_len = close.map_or(0, |_| self.end.len());
        let length = self.start.len() + content_len + end_len;
        match self.kind {
            RegionKind::Comment => push_run(runs, length, theme.comment.color()),
            RegionKind::String => push_run(runs, length, theme.string.color()),
            RegionKind::Embedded => {
                push_run(runs, self.start.len(), theme.keyword.color());
                let language = self
                    .language
                    .as_deref()
                    .and_then(Language::from_id)
                    .unwrap_or(Language::PlainText);
                let job =
                    lexer::highlight(&body[..content_len], 12.0, f32::INFINITY, language, theme);
                for section in job.sections {
                    push_run(
                        runs,
                        section.byte_range.end.0 - section.byte_range.start.0,
                        section.format.color,
                    );
                }
                push_run(runs, end_len, theme.keyword.color());
            }
        }
        length
    }
}

fn push_run(runs: &mut Vec<(usize, Color32)>, len: usize, color: Color32) {
    if len == 0 {
        return;
    }
    if let Some(last) = runs.last_mut().filter(|last| last.1 == color) {
        last.0 += len;
    } else {
        runs.push((len, color));
    }
}
impl Definition {
    pub fn validate(&self) -> Result<(), String> {
        let word = |word: &str| {
            !word.is_empty()
                && word.len() <= 48
                && word.bytes().enumerate().all(|(at, b)| {
                    b == b'_' || b.is_ascii_alphabetic() || at > 0 && b.is_ascii_digit()
                })
        };
        if self.name.trim() != self.name
            || self.name.is_empty()
            || self.name.len() > 80
            || self.name.chars().any(char::is_control)
            || self.extensions.is_empty()
            || self.extensions.len() > 16
            || self.extensions.iter().enumerate().any(|(at, ext)| {
                ext.is_empty()
                    || ext.len() > 16
                    || !ext
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                    || self.extensions[..at].contains(ext)
            })
            || self.keywords.len() > 256
            || !self.keywords.iter().all(|item| word(item))
            || self.line_comment.as_ref().is_some_and(|comment| {
                comment.is_empty()
                    || comment.len() > 8
                    || comment
                        .bytes()
                        .any(|b| b.is_ascii_whitespace() || !b.is_ascii_punctuation())
            })
        {
            return Err("Language packs require a name, up to 16 simple extensions, 256 ASCII keywords, and an optional 1–8 byte punctuation comment delimiter.".into());
        }
        let valid_delimiter = |value: &str| {
            !value.is_empty() && value.len() <= 32 && !value.chars().any(char::is_control)
        };
        let mut starts = BTreeSet::new();
        if self.regions.len() > 16
            || self.regions.iter().any(|region| {
                !valid_delimiter(&region.start)
                    || !valid_delimiter(&region.end)
                    || !starts.insert(&region.start)
                    || self.line_comment.as_ref() == Some(&region.start)
                    || match region.kind {
                        RegionKind::Comment => region.language.is_some() || region.escape,
                        RegionKind::String => region.language.is_some(),
                        RegionKind::Embedded => {
                            region.escape
                                || region
                                    .language
                                    .as_deref()
                                    .and_then(Language::from_id)
                                    .is_none()
                        }
                    }
            })
        {
            return Err("Use up to 16 regions with unique 1–32 byte opening delimiters and closing delimiters. Embedded regions require a built-in language. Only string regions accept escapes.".into());
        }
        Ok(())
    }

    pub fn matches_query(&self, query: &str) -> bool {
        let query = query.trim().trim_start_matches('.').to_ascii_lowercase();
        self.name.to_ascii_lowercase().contains(&query)
            || self
                .extensions
                .iter()
                .any(|extension| extension.contains(&query))
    }

    pub fn matches(&self, filename: &str) -> bool {
        filename.rsplit_once('.').is_some_and(|(_, extension)| {
            self.extensions
                .iter()
                .any(|value| value.eq_ignore_ascii_case(extension))
        })
    }
    /// Linear scan with bounded keyword lookups, no nesting or backtracking.
    /// Large pages use plain text so a pack cannot multiply work on huge files.
    pub fn highlight(&self, text: &str, theme: &Theme) -> Vec<(usize, Color32)> {
        if text.len() > 64 * 1024 {
            return vec![(text.len(), theme.text.color())];
        }
        let mut runs: Vec<(usize, Color32)> = Vec::new();
        let mut at = 0;
        while at < text.len() {
            let rest = &text[at..];
            let comment = self
                .line_comment
                .as_deref()
                .filter(|prefix| rest.starts_with(prefix));
            if let Some(region) = self
                .regions
                .iter()
                .filter(|region| rest.starts_with(&region.start))
                .filter(|region| comment.is_none_or(|prefix| region.start.len() > prefix.len()))
                .max_by_key(|region| region.start.len())
            {
                at += region.highlight(rest, theme, &mut runs);
                continue;
            }
            let first = rest.as_bytes()[0];
            let (len, color) = if comment.is_some() {
                (
                    rest.find('\n')
                        .unwrap_or(rest.len())
                        .max(rest.chars().next().unwrap().len_utf8()),
                    theme.comment.color(),
                )
            } else if self.quoted_strings && matches!(first, b'\'' | b'"') {
                let mut end = 1;
                let mut escaped = false;
                for c in rest[1..].chars() {
                    end += c.len_utf8();
                    if c == '\n' {
                        break;
                    }
                    if !escaped && c as u32 == u32::from(first) {
                        break;
                    }
                    escaped = !escaped && c == '\\';
                }
                (end, theme.string.color())
            } else if first.is_ascii_alphabetic() || first == b'_' {
                let len = rest
                    .bytes()
                    .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                    .count();
                (
                    len,
                    if self.keywords.contains(&rest[..len]) {
                        theme.keyword.color()
                    } else {
                        theme.text.color()
                    },
                )
            } else if first.is_ascii_digit() {
                (
                    rest.bytes()
                        .take_while(|b| b.is_ascii_digit() || *b == b'.')
                        .count(),
                    theme.number.color(),
                )
            } else {
                (rest.chars().next().unwrap().len_utf8(), theme.text.color())
            };
            push_run(&mut runs, len, color);
            at += len;
        }
        runs
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regions_highlight_multiline_and_embedded_text_without_losing_bytes() {
        let grammar: Definition = serde_json::from_str(
            r#"{
            "name":"Example", "extensions":["demo"], "line_comment":"/",
            "regions":[
                {"start":"/*","end":"*/","kind":"comment"},
                {"start":"<code>","end":"</code>","kind":"embedded","language":"rust"},
                {"start":"`","end":"`","kind":"string","escape":true}
            ]
        }"#,
        )
        .unwrap();
        grammar.validate().unwrap();
        let theme = Theme::default();
        let source = "/* one\ntwo */ <code>fn demo() {}</code> `a\\`b\n東京`";
        let mut at = 0;
        let runs: Vec<_> = grammar
            .highlight(source, &theme)
            .into_iter()
            .map(|(len, color)| {
                let text = &source[at..at + len];
                at += len;
                (text, color)
            })
            .collect();
        assert_eq!(at, source.len());
        assert!(runs.contains(&("/* one\ntwo */", theme.comment.color())));
        assert!(runs
            .iter()
            .any(|(text, color)| text.contains("fn") && *color == theme.keyword.color()));
        assert!(runs.contains(&("`a\\`b\n東京`", theme.string.color())));
    }

    #[test]
    fn untrusted_grammars_preserve_utf8_and_bound_pathological_inputs() {
        let grammar:Definition=serde_json::from_str(r#"{"name":"Example","extensions":["demo"],"keywords":["let"],"line_comment":"//","quoted_strings":true}"#).unwrap();
        grammar.validate().unwrap();
        for text in [
            "let café = '東京🦀' // λ\n",
            "'unterminated",
            "//",
            "\\",
            "",
            &"λ".repeat(100_000),
        ] {
            let runs = grammar.highlight(text, &Theme::default());
            let mut at = 0;
            for (len, _) in &runs {
                at += len;
                assert!(text.is_char_boundary(at));
            }
            assert_eq!(at, text.len());
        }
        for invalid in [
            r#"{"name":"X","extensions":["../secret"]}"#,
            r#"{"name":"X","extensions":["x"],"regex":"(a+)+"}"#,
        ] {
            assert!(serde_json::from_str::<Definition>(invalid)
                .map(|value| value.validate().is_err())
                .unwrap_or(true));
        }
    }
}
