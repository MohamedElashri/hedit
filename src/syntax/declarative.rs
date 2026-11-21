//! Bounded keyword grammars. Untrusted packs never introduce a regex engine.
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
        Ok(())
    }
    pub fn matches(&self, filename: &str) -> bool {
        filename
            .rsplit_once('.')
            .is_some_and(|(_, extension)| self.extensions.iter().any(|value| value == extension))
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
            let first = rest.as_bytes()[0];
            let (len, color) = if self
                .line_comment
                .as_ref()
                .is_some_and(|prefix| rest.starts_with(prefix))
            {
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
            if let Some(last) = runs.last_mut().filter(|last| last.1 == color) {
                last.0 += len;
            } else {
                runs.push((len, color));
            }
            at += len;
        }
        runs
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
