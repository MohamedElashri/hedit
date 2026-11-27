//! LSP coordinates are negotiated independently of the editor's scalar offsets.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Encoding {
    Utf8,
    #[default]
    Utf16,
    Utf32,
}
impl Encoding {
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.unwrap_or("utf-16") {
            "utf-8" => Ok(Self::Utf8),
            "utf-16" => Ok(Self::Utf16),
            "utf-32" => Ok(Self::Utf32),
            other => Err(format!("Unsupported LSP position encoding: {other}")),
        }
    }
    fn width(self, ch: char) -> usize {
        match self {
            Self::Utf8 => ch.len_utf8(),
            Self::Utf16 => ch.len_utf16(),
            Self::Utf32 => 1,
        }
    }
    pub fn position(self, text: &str, scalar: usize) -> Result<Position, String> {
        let mut p = Position::default();
        let mut count = 0;
        for ch in text.chars().take(scalar) {
            if ch == '\n' {
                p.line += 1;
                p.character = 0;
            } else {
                p.character += self.width(ch) as u32;
            }
            count += 1;
        }
        if count != scalar {
            return Err("Cursor is outside the document.".into());
        }
        Ok(p)
    }
    /// Reject split UTF-8 scalars, UTF-16 surrogates, and out-of-range positions.
    /// CRLF is one line break; its CR is never part of the content column.
    pub fn byte(self, text: &str, p: Position) -> Result<usize, String> {
        let mut start = 0;
        for _ in 0..p.line {
            start += text[start..]
                .find('\n')
                .ok_or("LSP line is outside the document.")?
                + 1;
        }
        let end = text[start..].find('\n').map_or(text.len(), |n| start + n);
        let end = if end < text.len() && end > start && text.as_bytes()[end - 1] == b'\r' {
            end - 1
        } else {
            end
        };
        let mut units = 0;
        for (at, ch) in text[start..end].char_indices() {
            if units == p.character as usize {
                return Ok(start + at);
            }
            units += self.width(ch);
            if units > p.character as usize {
                return Err("LSP position splits a Unicode character.".into());
            }
        }
        if units == p.character as usize {
            Ok(end)
        } else {
            Err("LSP column is outside the line.".into())
        }
    }
}

pub(crate) const SEMANTIC_TYPES: &[&str] = &[
    "namespace",
    "type",
    "class",
    "enum",
    "interface",
    "struct",
    "typeParameter",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "event",
    "function",
    "method",
    "macro",
    "keyword",
    "modifier",
    "comment",
    "string",
    "number",
    "regexp",
    "operator",
    "decorator",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticKind {
    Type,
    Parameter,
    Property,
    Function,
    Keyword,
    Comment,
    String,
    Number,
    Operator,
}

impl SemanticKind {
    fn classify(name: &str) -> Option<Self> {
        Some(match name {
            "namespace" | "type" | "class" | "enum" | "interface" | "struct" | "typeParameter" => {
                Self::Type
            }
            "parameter" | "enumMember" => Self::Parameter,
            "property" | "event" => Self::Property,
            "function" | "method" | "macro" | "decorator" => Self::Function,
            "keyword" | "modifier" => Self::Keyword,
            "comment" => Self::Comment,
            "string" | "regexp" => Self::String,
            "number" => Self::Number,
            "operator" => Self::Operator,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticToken {
    pub start: usize,
    pub end: usize,
    pub kind: SemanticKind,
}

pub(crate) fn semantic_tokens(
    source: &str,
    encoding: Encoding,
    capability: &serde_json::Value,
    result: &serde_json::Value,
) -> Result<Vec<SemanticToken>, String> {
    if result.is_null() {
        return Ok(vec![]);
    }
    let invalid = || "Invalid language-server semantic tokens.".to_owned();
    let legend = capability["legend"]["tokenTypes"]
        .as_array()
        .ok_or_else(invalid)?;
    let data = result["data"].as_array().ok_or_else(invalid)?;
    if legend.len() > 256 || data.len() > 100_000 || !data.len().is_multiple_of(5) {
        return Err(invalid());
    }
    fn content(line: &str) -> &str {
        line.strip_suffix("\r\n")
            .or_else(|| line.strip_suffix('\n'))
            .unwrap_or(line)
    }
    let mut lines = source.split_inclusive('\n');
    let mut line = lines.next().unwrap_or("");
    let mut chars = content(line).chars();
    let mut previous_column = 0u32;
    let mut units = 0u32;
    let mut scalar = 0;
    let mut line_start = 0;
    let mut tokens = Vec::new();
    for chunk in data.as_chunks::<5>().0 {
        let number = |i: usize| {
            chunk[i]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(invalid)
        };
        let delta_line = number(0)?;
        if delta_line != 0 {
            for _ in 0..delta_line {
                line_start += line.chars().count();
                line = lines.next().ok_or_else(invalid)?;
            }
            chars = content(line).chars();
            previous_column = 0;
            units = 0;
            scalar = line_start;
        }
        let column = previous_column
            .checked_add(number(1)?)
            .ok_or_else(invalid)?;
        let length = number(2)?;
        if length == 0 || column < units {
            return Err(invalid());
        }
        let end_column = column.checked_add(length).ok_or_else(invalid)?;
        let mut advance = |target| {
            while units < target {
                units += encoding.width(chars.next().ok_or_else(invalid)?) as u32;
                scalar += 1;
            }
            (units == target).then_some(scalar).ok_or_else(invalid)
        };
        let start = advance(column)?;
        let end = advance(end_column)?;
        let name = legend
            .get(number(3)? as usize)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(invalid)?;
        let _ = number(4)?;
        if let Some(kind) = SemanticKind::classify(name) {
            tokens.push(SemanticToken { start, end, kind });
        }
        previous_column = column;
    }
    Ok(tokens)
}

pub fn uri(path: &Path) -> Result<String, String> {
    url::Url::from_file_path(path)
        .map(String::from)
        .map_err(|_| "LSP needs an absolute file path.".into())
}
pub fn path(uri: &str) -> Result<PathBuf, String> {
    let url = url::Url::parse(uri).map_err(|e| e.to_string())?;
    if url.scheme() != "file" || url.query().is_some() || url.fragment().is_some() {
        return Err("Language server returned an unsupported document URI.".into());
    }
    url.to_file_path().map_err(|_| "Invalid file URI.".into())
}

/// File URIs omit Windows' verbatim path prefix. Compare their file identities
/// without filesystem access so open, unsaved documents keep their source text.
pub fn same_path(left: &Path, right: &Path) -> bool {
    left == right || matches!((uri(left), uri(right)), (Ok(a), Ok(b)) if a == b)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}
/// Validate the entire edit set before changing anything. Equal-position inserts
/// preserve server order. Overlaps and excessive growth fail without mutation.
pub fn apply(text: &str, edits: &[TextEdit], encoding: Encoding) -> Result<String, String> {
    if edits.len() > 10_000 {
        return Err("Too many language-server edits.".into());
    }
    let mut ranges = edits
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let a = encoding.byte(text, e.range.start)?;
            let b = encoding.byte(text, e.range.end)?;
            if a > b {
                return Err("Reversed language-server edit.".into());
            }
            Ok((a, b, i, &e.new_text))
        })
        .collect::<Result<Vec<_>, String>>()?;
    ranges.sort_by_key(|&(a, b, i, _)| (a, b, i));
    let mut end = 0;
    let mut length = text.len();
    for &(a, b, _, replacement) in &ranges {
        if a < end {
            return Err("Overlapping language-server edits.".into());
        }
        length = length
            .checked_sub(b - a)
            .and_then(|n| n.checked_add(replacement.len()))
            .ok_or("Edit size overflow.")?;
        if length > super::MAX_DOCUMENT {
            return Err("Language-server edit exceeds the document limit.".into());
        }
        end = b;
    }
    let mut out = String::with_capacity(length);
    end = 0;
    for (a, b, _, replacement) in ranges {
        out.push_str(&text[end..a]);
        out.push_str(replacement);
        end = b;
    }
    out.push_str(&text[end..]);
    Ok(out)
}

#[cfg(test)]
mod semantic_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn semantic_tokens_convert_utf16_positions_across_crlf() {
        let capability = json!({"legend": {"tokenTypes": ["parameter", "property"]}});
        let source = "🦀 foo\r\n\tbar";
        let response = json!({"data": [0, 3, 3, 0, 0, 1, 1, 3, 1, 0]});
        assert_eq!(
            semantic_tokens(source, Encoding::Utf16, &capability, &response).unwrap(),
            [
                SemanticToken {
                    start: 2,
                    end: 5,
                    kind: SemanticKind::Parameter
                },
                SemanticToken {
                    start: 8,
                    end: 11,
                    kind: SemanticKind::Property
                },
            ]
        );
        let split_surrogate = json!({"data": [0, 1, 1, 0, 0]});
        assert!(semantic_tokens(source, Encoding::Utf16, &capability, &split_surrogate).is_err());
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlayHint {
    pub scalar: usize,
    pub label: String,
}

pub(crate) const INLAY_DOCUMENT_LIMIT: usize = 512 * 1024;

pub(crate) fn inlay_hints(
    source: &str,
    encoding: Encoding,
    value: &serde_json::Value,
) -> Vec<InlayHint> {
    let mut start = 0;
    let lines: Vec<_> = source
        .split_inclusive('\n')
        .map(|line| {
            let entry = (line, start);
            start += line.chars().count();
            entry
        })
        .collect();
    let mut hints = Vec::new();
    for item in value.as_array().into_iter().flatten().take(2000) {
        let Ok(position) = serde_json::from_value::<Position>(item["position"].clone()) else {
            continue;
        };
        if position.character > 16_384 {
            continue;
        }
        let (line, start) = lines
            .get(position.line as usize)
            .copied()
            .unwrap_or_else(|| {
                if position.line as usize == lines.len()
                    && (source.is_empty() || source.ends_with('\n'))
                {
                    ("", start)
                } else {
                    ("", usize::MAX)
                }
            });
        if start == usize::MAX {
            continue;
        }
        let Ok(byte) = encoding.byte(
            line,
            Position {
                line: 0,
                character: position.character,
            },
        ) else {
            continue;
        };
        let label = &item["label"];
        let mut parts = Vec::new();
        if let Some(label) = label.as_str() {
            parts.push(label);
        } else if let Some(label) = label.as_array() {
            parts.extend(
                label
                    .iter()
                    .take(16)
                    .filter_map(|part| part["value"].as_str()),
            );
        }
        let label: String = parts
            .into_iter()
            .flat_map(str::chars)
            .filter(|ch| {
                !ch.is_control() && !matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
            .take(120)
            .collect();
        if !label.trim().is_empty() {
            hints.push(InlayHint {
                scalar: start + line[..byte].chars().count(),
                label,
            });
        }
    }
    hints.sort_by_key(|hint| hint.scalar);
    hints.dedup();
    hints
}

#[cfg(test)]
#[test]
fn inlay_hints_convert_utf16_and_read_label_parts() {
    let value = serde_json::json!([
        {"position":{"line":0,"character":1},"label":"ignored"},
        {"position":{"line":0,"character":3},"label":[{"value":"x"},{"value":":"}]},
        {"position":{"line":1,"character":0},"label":"i32"},
        {"position":{"line":2,"character":0},"label":"outside"}
    ]);
    assert_eq!(
        inlay_hints("🦀 x\r\nword", Encoding::Utf16, &value),
        [
            InlayHint {
                scalar: 2,
                label: "x:".into()
            },
            InlayHint {
                scalar: 5,
                label: "i32".into()
            }
        ]
    );
}
