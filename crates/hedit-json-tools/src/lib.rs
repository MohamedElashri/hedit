//! Bounded JSON formatting and single-document LSP. Project mode reads only cwd.
use hedit_protocol::lsp::{self, Decoder, Limits, READ_BYTES};
use serde_json::{json, Value};
use std::io::{self, Read, Write};
const TEXT: usize = 64 * 1024;
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid bounded JSON document or protocol state.",
    )
}
struct Output(Vec<u8>);
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > TEXT.saturating_sub(self.0.len()) {
            return Err(invalid());
        }
        self.0.extend(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn pretty(value: &Value) -> io::Result<String> {
    let mut out = Output(Vec::new());
    serde_json::to_writer_pretty(&mut out, value).map_err(|_| invalid())?;
    out.write_all(b"\n")?;
    String::from_utf8(out.0).map_err(|_| invalid())
}
pub fn format(bytes: &[u8]) -> io::Result<String> {
    pretty(&lsp::parse_data(bytes, Limits::DATA)?)
}
fn position(text: &str, byte: usize) -> Value {
    let prefix = &text[..byte];
    json!({"line":prefix.bytes().filter(|b| *b == b'\n').count(),
        "character":prefix.rfind('\n').map_or(byte, |at| byte-at-1)})
}
fn range(text: &str, start: usize, finish: usize) -> Value {
    json!({"start":position(text,start),"end":position(text,finish)})
}
fn string<'a>(value: &'a Value, name: &str) -> io::Result<&'a str> {
    value[name].as_str().ok_or_else(invalid)
}
fn version(value: &Value) -> io::Result<i32> {
    value["version"]
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(invalid)
}
struct Document {
    uri: String,
    version: i32,
    text: String,
    value: Option<Value>,
    keys: Vec<(String, usize, usize)>,
}
impl Document {
    fn new(uri: &str, version: i32, text: &str) -> io::Result<Self> {
        if text.len() > TEXT || !uri.starts_with("file://") || uri.len() > 4096 {
            return Err(invalid());
        }
        let value = lsp::parse_data(text.as_bytes(), Limits::DATA).ok();
        let keys = if value.as_ref().is_some_and(Value::is_object) {
            root_keys(text)?
        } else {
            vec![]
        };
        Ok(Self {
            uri: uri.into(),
            version,
            text: text.into(),
            value,
            keys,
        })
    }
    fn edits(&self) -> io::Result<Value> {
        let Some(formatted) = self.value.as_ref().and_then(|value| pretty(value).ok()) else {
            return Ok(json!([]));
        };
        Ok(json!([{"range":range(&self.text,0,self.text.len()),"newText":formatted}]))
    }
    fn result(&self, method: &str) -> io::Result<Value> {
        match method {
            "textDocument/diagnostic" => Ok(
                json!({"kind":"full","items":if self.value.is_some() { vec![] } else {
                    vec![json!({"range":range(&self.text,0,0),"severity":1,
                        "message":"Invalid JSON, duplicate object keys, or document complexity limit exceeded."})]
                }}),
            ),
            "textDocument/formatting" => self.edits(),
            "textDocument/documentSymbol" => Ok(Value::Array(
                self.keys
                    .iter()
                    .take(128)
                    .map(|(key, start, end)| {
                        json!({"name":key,"kind":13,"range":range(&self.text,*start,*end),
                    "selectionRange":range(&self.text,*start,*end)})
                    })
                    .collect(),
            )),
            "textDocument/hover" => {
                let Some(value) = &self.value else {
                    return Ok(Value::Null);
                };
                let summary = match value {
                    Value::Object(values) => {
                        format!("JSON object with {} properties", values.len())
                    }
                    Value::Array(values) => format!("JSON array with {} elements", values.len()),
                    Value::String(value) => {
                        format!("JSON string with {} characters", value.chars().count())
                    }
                    Value::Number(_) => "JSON number".into(),
                    Value::Bool(_) => "JSON boolean".into(),
                    Value::Null => "JSON null".into(),
                };
                Ok(json!({"range":range(&self.text,0,self.text.len()),
                    "contents":{"kind":"plaintext","value":summary}}))
            }
            _ => Err(invalid()),
        }
    }
}
// Locate top-level key spans only after the authoritative bounded JSON parse.
// Strings/escapes are skipped as complete lexemes; their decoded names come
// from serde_json, never from a second handwritten JSON value parser.
fn root_keys(text: &str) -> io::Result<Vec<(String, usize, usize)>> {
    let bytes = text.as_bytes();
    let (mut depth, mut at) = (0usize, 0usize);
    let mut keys = Vec::new();
    while at < bytes.len() {
        match bytes[at] {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth = depth.checked_sub(1).ok_or_else(invalid)?,
            b'"' => {
                let start = at;
                at += 1;
                while at < bytes.len() && bytes[at] != b'"' {
                    if bytes[at] == b'\\' {
                        at += 1;
                    }
                    at += 1;
                }
                if at >= bytes.len() {
                    return Err(invalid());
                }
                let end = at + 1;
                let mut next = end;
                while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                    next += 1;
                }
                if depth == 1 && bytes.get(next) == Some(&b':') && keys.len() < 128 {
                    let key: String =
                        serde_json::from_str(&text[start..end]).map_err(|_| invalid())?;
                    // The host's symbol label budget is 256 UTF-8 bytes.
                    if key.len() <= 256
                        && !key.chars().any(char::is_control)
                        && !key.trim().is_empty()
                    {
                        keys.push((key, start, end));
                    }
                }
            }
            _ => {}
        }
        at += 1;
    }
    Ok(keys)
}
#[derive(Default)]
struct Server {
    initialized: bool,
    ready: bool,
    shutdown: bool,
    document: Option<Document>,
    summary: Option<String>,
}
impl Server {
    fn message(&mut self, message: Value) -> io::Result<(Option<Value>, bool)> {
        let method = string(&message, "method")?;
        let id = message.get("id").cloned();
        let params = &message["params"];
        if self.shutdown {
            return if method == "exit" && id.is_none() {
                Ok((None, true))
            } else {
                Err(invalid())
            };
        }
        if method == "initialize" && id.is_some() && !self.initialized {
            self.initialized = true;
            return Ok((
                Some(json!({"jsonrpc":"2.0","id":id,"result":{"capabilities":{
                "positionEncoding":"utf-8","textDocumentSync":{"openClose":true,"change":1},
                "hoverProvider":true,"documentSymbolProvider":true,"documentFormattingProvider":true,
                "diagnosticProvider":{"interFileDependencies":false,"workspaceDiagnostics":false}
            },"serverInfo":{"name":"hedit JSON reference","version":env!("CARGO_PKG_VERSION")}}})),
                false,
            ));
        }
        if !self.initialized {
            return Err(invalid());
        }
        if method == "initialized" && id.is_none() && !self.ready {
            self.ready = true;
            return Ok((None, false));
        }
        if method == "shutdown" && id.is_some() {
            self.shutdown = true;
            return Ok((Some(json!({"jsonrpc":"2.0","id":id,"result":null})), false));
        }
        if !self.ready {
            return Err(invalid());
        }
        match method {
            "$/cancelRequest" if id.is_none() => return Ok((None, false)),
            "textDocument/didOpen" if id.is_none() && self.document.is_none() => {
                let doc = &params["textDocument"];
                self.document = Some(Document::new(
                    string(doc, "uri")?,
                    version(doc)?,
                    string(doc, "text")?,
                )?);
                return Ok((None, false));
            }
            "textDocument/didChange" if id.is_none() => {
                let old = self.document.as_ref().ok_or_else(invalid)?;
                let doc = &params["textDocument"];
                let changes = params["contentChanges"]
                    .as_array()
                    .filter(|changes| changes.len() == 1)
                    .ok_or_else(invalid)?;
                if string(doc, "uri")? != old.uri
                    || version(doc)? <= old.version
                    || changes[0].get("range").is_some()
                {
                    return Err(invalid());
                }
                self.document = Some(Document::new(
                    &old.uri,
                    version(doc)?,
                    string(&changes[0], "text")?,
                )?);
                return Ok((None, false));
            }
            "textDocument/didClose" if id.is_none() => {
                if string(&params["textDocument"], "uri")?
                    != self.document.as_ref().ok_or_else(invalid)?.uri
                {
                    return Err(invalid());
                }
                self.document = None;
                return Ok((None, false));
            }
            _ => {}
        }
        let id = id.ok_or_else(invalid)?;
        let document = self.document.as_ref().ok_or_else(invalid)?;
        if string(&params["textDocument"], "uri")? != document.uri {
            return Err(invalid());
        }
        let result = match method {
            "textDocument/diagnostic"
            | "textDocument/formatting"
            | "textDocument/documentSymbol"
            | "textDocument/hover" => {
                let mut result = document.result(method)?;
                if method == "textDocument/hover" && !result.is_null() {
                    if let Some(summary) = &self.summary {
                        if let Some(text) = result["contents"]["value"].as_str() {
                            result["contents"]["value"] = format!("{text}\n{summary}").into();
                        }
                    }
                }
                json!({"jsonrpc":"2.0","id":id,"result":result})
            }
            _ => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unsupported method"}})
            }
        };
        Ok((Some(result), false))
    }
}
pub fn serve(input: impl Read, output: impl Write) -> io::Result<()> {
    serve_summary(input, output, None)
}
/// Summarize at most 32 top-level JSON files in the host's immutable projection.
pub fn serve_project(input: impl Read, output: impl Write) -> io::Result<()> {
    serve_summary(
        input,
        output,
        Some(project_summary(std::path::Path::new("."))?),
    )
}
fn project_summary(root: &std::path::Path) -> io::Result<String> {
    let (mut files, mut valid) = (0usize, 0usize);
    for (index, entry) in std::fs::read_dir(root)?.enumerate() {
        if index >= 512 {
            return Err(invalid());
        }
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        if !entry.file_type()?.is_file() {
            return Err(invalid());
        }
        files += 1;
        if files > 32 || entry.metadata()?.len() > TEXT as u64 {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(entry.path())?
            .take((TEXT + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > TEXT {
            return Err(invalid());
        }
        if lsp::parse_data(&bytes, Limits::DATA).is_ok() {
            valid += 1;
        }
    }
    Ok(format!(
        "Approved project: {files} top-level JSON files ({valid} valid)"
    ))
}
fn serve_summary(
    mut input: impl Read,
    mut output: impl Write,
    summary: Option<String>,
) -> io::Result<()> {
    let mut server = Server {
        summary,
        ..Default::default()
    };
    let mut decoder = Decoder::new(Limits::SCOPED);
    let mut bytes = [0; READ_BYTES];
    let mut messages = 0usize;
    loop {
        while let Some(message) = decoder.next_message()? {
            messages += 1;
            if messages > 100_000 {
                return Err(invalid());
            }
            let (response, exit) = server.message(message)?;
            if let Some(response) = response {
                output.write_all(&lsp::encode(&response, Limits::SCOPED)?)?;
                output.flush()?;
            }
            if exit {
                return Ok(());
            }
        }
        let count = input.read(&mut bytes)?;
        if count == 0 {
            return decoder.finish();
        }
        decoder.push(&bytes[..count])?;
    }
}
#[cfg(test)]
mod tests;
