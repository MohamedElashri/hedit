//! Bounded LSP framing and duplicate-safe JSON; this module conveys no authority.
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::{fmt, io};

pub const READ_BYTES: usize = 4096;
pub const MAX_MESSAGE: usize = 8 * 1024 * 1024;
const HEADER_BYTES: usize = 8192;
const DEPTH: usize = 64;
#[derive(Clone, Copy)]
pub struct Limits {
    message: usize,
    nodes: usize,
}
impl Limits {
    pub const BUILTIN: Self = Self {
        message: MAX_MESSAGE,
        nodes: 131_072,
    };
    pub const SCOPED: Self = Self {
        message: 256 * 1024,
        nodes: 8192,
    };
    pub const DATA: Self = Self {
        message: 64 * 1024,
        nodes: 8192,
    };
    pub const fn message_bytes(self) -> usize {
        self.message
    }
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid or oversized language-server message.",
    )
}

/// The caller drains complete frames before adding more bounded read chunks.
/// A scan cursor makes one-byte fragmentation linear in header length.
pub struct Decoder {
    limits: Limits,
    bytes: Vec<u8>,
    start: usize,
    scan: usize,
    body: Option<(usize, usize)>,
    failed: bool,
}
impl Decoder {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            bytes: Vec::new(),
            start: 0,
            scan: 0,
            body: None,
            failed: false,
        }
    }
    pub fn push(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.failed || bytes.len() > READ_BYTES {
            self.failed = true;
            return Err(invalid());
        }
        if self.start > 0 {
            self.bytes.drain(..self.start);
            self.scan -= self.start;
            if let Some((offset, _)) = &mut self.body {
                *offset -= self.start;
            }
            self.start = 0;
        }
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|bytes| bytes > self.limits.message + HEADER_BYTES + READ_BYTES)
        {
            self.failed = true;
            return Err(invalid());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    pub fn next_message(&mut self) -> io::Result<Option<Value>> {
        if self.failed {
            return Err(invalid());
        }
        let result = self.next_inner();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn next_inner(&mut self) -> io::Result<Option<Value>> {
        if self.body.is_none() {
            while self.scan + 4 <= self.bytes.len() {
                if self.scan + 4 - self.start > HEADER_BYTES {
                    return Err(invalid());
                }
                if self.bytes[self.scan..self.scan + 4] == *b"\r\n\r\n" {
                    let end = self.scan + 4;
                    let length = content_length(&self.bytes[self.start..end], self.limits)?;
                    self.body = Some((end, length));
                    break;
                }
                self.scan += 1;
            }
            if self.body.is_none() && self.bytes.len() - self.start > HEADER_BYTES {
                return Err(invalid());
            }
        }
        let Some((offset, length)) = self.body else {
            return Ok(None);
        };
        let end = offset.checked_add(length).ok_or_else(invalid)?;
        if end > self.bytes.len() {
            return Ok(None);
        }
        let value = parse_json(&self.bytes[offset..end], self.limits)?;
        self.body = None;
        self.start = end;
        self.scan = end;
        if end == self.bytes.len() {
            self.bytes.clear();
            self.start = 0;
            self.scan = 0;
        }
        Ok(Some(value))
    }
    pub fn finish(&mut self) -> io::Result<()> {
        if self.failed || self.start != self.bytes.len() || self.body.is_some() {
            self.failed = true;
            Err(invalid())
        } else {
            Ok(())
        }
    }
}

pub fn content_length(header: &[u8], limits: Limits) -> io::Result<usize> {
    if header.len() > HEADER_BYTES || !header.is_ascii() || !header.ends_with(b"\r\n\r\n") {
        return Err(invalid());
    }
    let text = std::str::from_utf8(&header[..header.len() - 4]).map_err(|_| invalid())?;
    let mut length = None;
    let mut content_type = false;
    for line in text.split("\r\n") {
        let (name, value) = line.split_once(':').ok_or_else(invalid)?;
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(invalid());
        }
        let value = value.trim_matches([' ', '\t']);
        if name.eq_ignore_ascii_case("Content-Length") {
            if length.is_some() || value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            length = Some(value.parse::<usize>().map_err(|_| invalid())?);
        } else if name.eq_ignore_ascii_case("Content-Type") {
            if content_type {
                return Err(invalid());
            }
            content_type = true;
            let mut pieces = value.split(';').map(str::trim);
            let mime = pieces.next().ok_or_else(invalid)?;
            if !mime.eq_ignore_ascii_case("application/vscode-jsonrpc")
                && !mime.eq_ignore_ascii_case("application/json")
            {
                return Err(invalid());
            }
            if let Some(charset) = pieces.next() {
                let (name, value) = charset.split_once('=').ok_or_else(invalid)?;
                if !name.trim().eq_ignore_ascii_case("charset")
                    || !(value.trim().eq_ignore_ascii_case("utf-8")
                        || value.trim().eq_ignore_ascii_case("utf8"))
                {
                    return Err(invalid());
                }
            }
            if pieces.next().is_some() {
                return Err(invalid());
            }
        } else {
            return Err(invalid());
        }
    }
    length
        .filter(|length| *length > 0 && *length <= limits.message)
        .ok_or_else(invalid)
}

struct Seed<'a> {
    remaining: &'a mut usize,
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, decoder: D) -> Result<Value, D::Error> {
        if *self.remaining == 0 || self.depth > DEPTH {
            return Err(de::Error::custom("language-server JSON budget exhausted"));
        }
        *self.remaining -= 1;
        decoder.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Seed<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("bounded duplicate-free JSON")
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("invalid JSON number"))
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.into()))
    }
    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }
    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(Seed {
            remaining: self.remaining,
            depth: self.depth + 1,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut source: A) -> Result<Value, A::Error> {
        let mut values = Map::new();
        while let Some(key) = source.next_key::<String>()? {
            if *self.remaining == 0 || values.contains_key(&key) {
                return Err(de::Error::custom(
                    "duplicate field or JSON budget exhausted",
                ));
            }
            *self.remaining -= 1;
            let value = source.next_value_seed(Seed {
                remaining: self.remaining,
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}
pub fn parse_json(bytes: &[u8], limits: Limits) -> io::Result<Value> {
    let value = parse_data(bytes, limits)?;
    validate(&value)?;
    Ok(value)
}
/// Duplicate-safe bounded JSON data. This does not validate a JSON-RPC envelope.
pub fn parse_data(bytes: &[u8], limits: Limits) -> io::Result<Value> {
    if bytes.is_empty() || bytes.len() > limits.message {
        return Err(invalid());
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let mut remaining = limits.nodes;
    let value = Seed {
        remaining: &mut remaining,
        depth: 0,
    }
    .deserialize(&mut decoder)
    .map_err(|_| invalid())?;
    decoder.end().map_err(|_| invalid())?;
    Ok(value)
}
fn id(value: &Value, null: bool) -> bool {
    (null && value.is_null())
        || value
            .as_i64()
            .is_some_and(|value| i32::try_from(value).is_ok())
        || value.as_str().is_some_and(|value| {
            !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
        })
}
/// Envelope validation is independent of request correlation and method policy.
/// Those belong to the authenticated owner of this connection.
pub fn validate(value: &Value) -> io::Result<()> {
    let object = value.as_object().ok_or_else(invalid)?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(invalid());
    }
    if let Some(method) = object.get("method") {
        if !method.as_str().is_some_and(|method| {
            !method.is_empty() && method.len() <= 256 && !method.chars().any(char::is_control)
        }) || object
            .keys()
            .any(|key| !matches!(key.as_str(), "jsonrpc" | "id" | "method" | "params"))
            || object.get("id").is_some_and(|value| !id(value, false))
            || object
                .get("params")
                .is_some_and(|value| !value.is_null() && !value.is_object() && !value.is_array())
        {
            return Err(invalid());
        }
    } else {
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "jsonrpc" | "id" | "result" | "error"))
            || object.get("id").is_none_or(|value| !id(value, true))
            || object.contains_key("result") == object.contains_key("error")
        {
            return Err(invalid());
        }
        if let Some(error) = object.get("error") {
            let error = error.as_object().ok_or_else(invalid)?;
            if error
                .keys()
                .any(|key| !matches!(key.as_str(), "code" | "message" | "data"))
                || !error
                    .get("code")
                    .and_then(Value::as_i64)
                    .is_some_and(|code| i32::try_from(code).is_ok())
                || !error.get("message").is_some_and(Value::is_string)
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}
fn structure(value: &Value, depth: usize, remaining: &mut usize) -> io::Result<()> {
    if *remaining == 0 || depth > DEPTH {
        return Err(invalid());
    }
    *remaining -= 1;
    match value {
        Value::Array(values) => {
            for value in values {
                structure(value, depth + 1, remaining)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                if *remaining == 0 {
                    return Err(invalid());
                }
                *remaining -= 1; // Field name, matching the duplicate-safe decoder.
                structure(value, depth + 1, remaining)?;
            }
        }
        _ => {}
    }
    Ok(())
}
struct Encoded {
    bytes: Vec<u8>,
    maximum: usize,
}
impl io::Write for Encoded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes.len()) {
            return Err(invalid());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub fn encode(value: &Value, limits: Limits) -> io::Result<Vec<u8>> {
    validate(value)?;
    let mut remaining = limits.nodes;
    structure(value, 0, &mut remaining)?;
    // Reserve framing space up front. Serialization stops at the byte limit;
    // it never reparses or clones a host-owned document just to count nodes.
    const PREFIX: usize = 32;
    let mut encoded = Encoded {
        bytes: vec![0; PREFIX],
        maximum: limits.message + PREFIX,
    };
    serde_json::to_writer(&mut encoded, value).map_err(|_| invalid())?;
    let length = encoded.bytes.len() - PREFIX;
    let header = format!("Content-Length: {length}\r\n\r\n");
    if header.len() > PREFIX {
        return Err(invalid());
    }
    encoded.bytes.copy_within(PREFIX.., header.len());
    encoded.bytes.truncate(header.len() + length);
    encoded.bytes[..header.len()].copy_from_slice(header.as_bytes());
    Ok(encoded.bytes)
}

#[cfg(test)]
mod tests;
