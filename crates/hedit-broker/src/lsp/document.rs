//! Pure translation against one host-issued URI and exact immutable snapshot.
//! Server paths never reach filesystem APIs or the built-in LSP edit launcher.
use super::Method;
use hedit_core::language_server::text::{self, Encoding};
use hedit_plugin::{
    sdk,
    service::{self, ResultSet},
};
use serde_json::Value;
use std::io;

pub struct Document {
    snapshot: sdk::Snapshot,
    uri: String,
    version: i32,
    points: Vec<(usize, u32, u32)>,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid scoped language-server result.",
    )
}
fn string(value: &Value, maximum: usize) -> io::Result<&str> {
    value
        .as_str()
        .filter(|text| text.len() <= maximum && !text.contains('\0'))
        .ok_or_else(invalid)
}
fn optional<'a>(value: &'a Value, key: &str, maximum: usize) -> io::Result<&'a str> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(""),
        Some(value) => string(value, maximum),
    }
}
fn rows(value: &Value, maximum: usize) -> io::Result<&[Value]> {
    value
        .as_array()
        .filter(|items| items.len() <= maximum)
        .map(Vec::as_slice)
        .ok_or_else(invalid)
}
impl Document {
    pub fn new(
        snapshot: sdk::Snapshot,
        uri: String,
        version: i32,
        encoding: Encoding,
    ) -> io::Result<Self> {
        snapshot.validate().map_err(|_| invalid())?;
        // Syntax validation only. The exact host-issued string is the lookup
        // key; never canonicalize or read a server-supplied URI.
        text::path(&uri).map_err(|_| invalid())?;
        if uri.len() > 8192 || version <= 0 {
            return Err(invalid());
        }
        let mut points = Vec::with_capacity(snapshot.text.chars().count() + 1);
        let (mut line, mut column) = (0, 0);
        for (byte, ch) in snapshot.text.char_indices() {
            if sdk::boundary(&snapshot.text, byte) {
                points.push((byte, line, column));
            }
            if ch == '\n' {
                line += 1;
                column = 0;
            } else {
                column += match encoding {
                    Encoding::Utf8 => ch.len_utf8(),
                    Encoding::Utf16 => ch.len_utf16(),
                    Encoding::Utf32 => 1,
                } as u32;
            }
        }
        points.push((snapshot.text.len(), line, column));
        Ok(Self {
            snapshot,
            uri,
            version,
            points,
        })
    }
    pub fn uri(&self) -> &str {
        &self.uri
    }
    pub fn version(&self) -> i32 {
        self.version
    }
    pub fn snapshot(&self) -> &sdk::Snapshot {
        &self.snapshot
    }
    pub fn position(&self, byte: usize) -> io::Result<text::Position> {
        let at = self
            .points
            .binary_search_by_key(&byte, |point| point.0)
            .map_err(|_| invalid())?;
        Ok(text::Position {
            line: self.points[at].1,
            character: self.points[at].2,
        })
    }
    fn byte(&self, position: text::Position) -> io::Result<usize> {
        let at = self
            .points
            .binary_search_by_key(&(position.line, position.character), |point| {
                (point.1, point.2)
            })
            .map_err(|_| invalid())?;
        Ok(self.points[at].0)
    }
    fn range(&self, value: &Value) -> io::Result<service::Range> {
        let range: text::Range = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
        let range = service::Range {
            start: self.byte(range.start)?,
            finish: self.byte(range.end)?,
        };
        range.validate(&self.snapshot).map_err(|_| invalid())?;
        Ok(range)
    }
    fn diagnostics(&self, value: &Value) -> io::Result<Vec<service::Diagnostic>> {
        rows(value, service::MAX_ITEMS)?
            .iter()
            .map(|item| {
                let severity = match item
                    .get("severity")
                    .map_or(Ok(1), |value| value.as_u64().ok_or_else(invalid))?
                {
                    1 => service::Severity::Error,
                    2 => service::Severity::Warning,
                    3 => service::Severity::Information,
                    4 => service::Severity::Hint,
                    _ => return Err(invalid()),
                };
                Ok(service::Diagnostic {
                    range: self.range(&item["range"])?,
                    severity,
                    message: string(&item["message"], 1024)?.into(),
                    actions: vec![],
                })
            })
            .collect()
    }
    pub fn published_diagnostics(&self, params: &Value) -> io::Result<ResultSet> {
        if params["uri"].as_str() != Some(&self.uri)
            || params["version"].as_i64() != Some(i64::from(self.version))
        {
            return Err(invalid());
        }
        let result = ResultSet::Diagnostics(self.diagnostics(&params["diagnostics"])?);
        result
            .validate(&self.snapshot, service::Kind::Diagnostics, false)
            .map_err(|_| invalid())?;
        Ok(result)
    }
    fn transaction(&self, value: &Value) -> io::Result<sdk::Transaction> {
        let mut edits = rows(value, sdk::MAX_EDITS)?
            .iter()
            .map(|edit| {
                let range = self.range(&edit["range"])?;
                Ok(sdk::Edit {
                    start: range.start,
                    finish: range.finish,
                    text: string(&edit["newText"], 256 * 1024)?.into(),
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        edits.sort_by_key(|edit| (edit.start, edit.finish));
        let transaction = sdk::Transaction {
            document: self.snapshot.document.clone(),
            revision: self.snapshot.revision.clone(),
            edits,
            selections: None,
            storage: None,
        };
        transaction.prepare(&self.snapshot).map_err(|_| invalid())?;
        Ok(transaction)
    }
    fn prefix(&self, position: usize) -> service::Range {
        let start = self.snapshot.text[..position]
            .char_indices()
            .rev()
            .take_while(|(_, ch)| ch.is_alphanumeric() || *ch == '_')
            .last()
            .map_or(position, |(at, _)| at);
        service::Range {
            start,
            finish: position,
        }
    }
    fn completions(&self, value: &Value, position: usize) -> io::Result<Vec<service::Completion>> {
        if value.is_null() {
            return Ok(vec![]);
        }
        let values = if value.is_object() {
            // Defaults can supply implicit edit ranges/snippets. Do not silently
            // reinterpret them as simple plain-text completion items.
            if value.get("itemDefaults").is_some() {
                return Err(invalid());
            }
            rows(&value["items"], service::MAX_ITEMS)?
        } else {
            rows(value, service::MAX_ITEMS)?
        };
        values
            .iter()
            .map(|item| {
                if item.get("command").is_some_and(|value| !value.is_null())
                    || item
                        .get("insertTextFormat")
                        .is_some_and(|value| value.as_u64() != Some(1))
                    || item
                        .get("insertTextMode")
                        .is_some_and(|value| value.as_u64() != Some(1))
                    || item
                        .get("additionalTextEdits")
                        .is_some_and(|value| value.as_array().is_none_or(|edits| !edits.is_empty()))
                {
                    return Err(invalid());
                }
                let label = string(&item["label"], 256)?;
                let (range, insert) = if let Some(edit) = item.get("textEdit") {
                    (self.range(&edit["range"])?, string(&edit["newText"], 4096)?)
                } else {
                    (
                        self.prefix(position),
                        match item.get("insertText") {
                            Some(value) => string(value, 4096)?,
                            None => label,
                        },
                    )
                };
                Ok(service::Completion {
                    label: label.into(),
                    detail: optional(item, "detail", 1024)?.into(),
                    range,
                    insert: insert.into(),
                })
            })
            .collect()
    }
    fn hover_text(value: &Value) -> io::Result<String> {
        let text = if let Some(text) = value.as_str() {
            text
        } else if value.is_object() {
            if value
                .get("kind")
                .is_some_and(|kind| !matches!(kind.as_str(), Some("plaintext" | "markdown")))
            {
                return Err(invalid());
            }
            string(&value["value"], 16 * 1024)?
        } else {
            return Err(invalid());
        };
        if text.len() > 16 * 1024 || text.contains('\0') {
            return Err(invalid());
        }
        Ok(text.into())
    }
    fn hover(&self, value: &Value, position: usize) -> io::Result<service::Hover> {
        if value.is_null() {
            return Ok(service::Hover {
                range: service::Range {
                    start: position,
                    finish: position,
                },
                text: String::new(),
            });
        }
        let contents = &value["contents"];
        let text = if contents.is_array() {
            let mut output = String::new();
            for value in rows(contents, 8)? {
                let text = Self::hover_text(value)?;
                if output.len() + text.len() + 2 > 16 * 1024 {
                    return Err(invalid());
                }
                if !output.is_empty() {
                    output.push_str("\n\n");
                }
                output.push_str(&text);
            }
            output
        } else {
            Self::hover_text(contents)?
        };
        let range = match value.get("range") {
            Some(range) => self.range(range)?,
            None => service::Range {
                start: position,
                finish: position,
            },
        };
        Ok(service::Hover { range, text })
    }
    fn symbols(&self, value: &Value) -> io::Result<Vec<service::Symbol>> {
        if value.is_null() {
            return Ok(vec![]);
        }
        let mut stack: Vec<_> = rows(value, service::MAX_ITEMS)?.iter().rev().collect();
        let mut output = Vec::new();
        while let Some(item) = stack.pop() {
            if output.len() >= service::MAX_ITEMS {
                return Err(invalid());
            }
            let range = if let Some(location) = item.get("location") {
                if location["uri"].as_str() != Some(&self.uri) {
                    return Err(invalid());
                }
                self.range(&location["range"])?
            } else {
                let outer = self.range(&item["range"])?;
                let range = self.range(&item["selectionRange"])?;
                if range.start < outer.start || range.finish > outer.finish {
                    return Err(invalid());
                }
                range
            };
            output.push(service::Symbol {
                name: string(&item["name"], 256)?.into(),
                detail: optional(item, "detail", 1024)?.into(),
                range,
            });
            if let Some(children) = item.get("children") {
                let children = rows(children, service::MAX_ITEMS)?;
                if stack.len() + children.len() + output.len() > service::MAX_ITEMS {
                    return Err(invalid());
                }
                stack.extend(children.iter().rev());
            }
        }
        Ok(output)
    }
    fn actions(&self, value: &Value) -> io::Result<Vec<service::ActionItem>> {
        if value.is_null() {
            return Ok(vec![]);
        }
        rows(value, service::MAX_ITEMS)?
            .iter()
            .filter(|item| item.get("disabled").is_none())
            .map(|item| {
                if item.get("command").is_some_and(|value| !value.is_null()) {
                    return Err(invalid());
                }
                let edit = item.get("edit").ok_or_else(invalid)?;
                let edits = if let Some(changes) = edit.get("changes") {
                    if edit.get("documentChanges").is_some() {
                        return Err(invalid());
                    }
                    let changes = changes
                        .as_object()
                        .filter(|changes| changes.len() == 1)
                        .ok_or_else(invalid)?;
                    changes.get(&self.uri).ok_or_else(invalid)?
                } else {
                    let changes = rows(&edit["documentChanges"], 1)?;
                    let change = changes.first().ok_or_else(invalid)?;
                    if change.get("kind").is_some()
                        || change["textDocument"]["uri"].as_str() != Some(&self.uri)
                        || change["textDocument"]["version"].as_i64()
                            != Some(i64::from(self.version))
                    {
                        return Err(invalid());
                    }
                    &change["edits"]
                };
                Ok(service::ActionItem {
                    title: string(&item["title"], 256)?.into(),
                    action: service::Action::Edit(self.transaction(edits)?),
                })
            })
            .collect()
    }
    pub fn translate(
        &self,
        method: Method,
        value: &Value,
        position: usize,
        may_edit: bool,
    ) -> io::Result<ResultSet> {
        self.position(position)?;
        if !may_edit
            && matches!(
                method,
                Method::Completions | Method::Formatting | Method::CodeActions
            )
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Document edit approval is required.",
            ));
        }
        let (result, kind) = match method {
            Method::Diagnostics => {
                if value["kind"] != "full" || value.get("relatedDocuments").is_some() {
                    return Err(invalid());
                }
                (
                    ResultSet::Diagnostics(self.diagnostics(&value["items"])?),
                    service::Kind::Diagnostics,
                )
            }
            Method::Completions => (
                ResultSet::Completions(self.completions(value, position)?),
                service::Kind::Completions,
            ),
            Method::Hover => (
                ResultSet::Hover(self.hover(value, position)?),
                service::Kind::Hover,
            ),
            Method::Symbols => (
                ResultSet::Symbols(self.symbols(value)?),
                service::Kind::Symbols,
            ),
            Method::Formatting => {
                let empty = Value::Array(vec![]);
                (
                    ResultSet::Formatting(self.transaction(if value.is_null() {
                        &empty
                    } else {
                        value
                    })?),
                    service::Kind::Formatting,
                )
            }
            Method::CodeActions => (
                ResultSet::CodeActions(self.actions(value)?),
                service::Kind::CodeActions,
            ),
        };
        result
            .validate(&self.snapshot, kind, may_edit)
            .map_err(|_| invalid())?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
