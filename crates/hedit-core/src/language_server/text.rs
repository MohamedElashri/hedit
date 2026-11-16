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
