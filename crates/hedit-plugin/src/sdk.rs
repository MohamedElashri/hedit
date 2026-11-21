//! Versioned editing SDK: scoped snapshots and validated single-document edits.
use crate::{options, Error, MAX_SELECTIONS, MAX_SELECTION_BYTES, MAX_SOURCE_BYTES};
use serde::{de, Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, marker::PhantomData};

pub(crate) mod lua;
#[cfg(test)]
mod tests;

pub const VERSION: u32 = 1;
pub const MAX_EDITS: usize = 64;
pub const MAX_PACKAGE_BYTES: usize = 64 * 1024;
pub const MAX_RESOURCES: usize = 16;
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub document: String,
    pub revision: String,
    pub language: String,
    pub text: String,
    #[serde(deserialize_with = "read_selections")]
    pub selections: Vec<Selection>,
    pub crlf: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub sdk_version: u32,
    pub source: String,
    #[serde(default, deserialize_with = "read_modules")]
    pub modules: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "read_resources")]
    pub resources: BTreeMap<String, String>,
    pub snapshot: Snapshot,
    #[serde(deserialize_with = "read_options")]
    pub settings: BTreeMap<String, options::Value>,
    #[serde(deserialize_with = "read_options")]
    pub arguments: BTreeMap<String, options::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<Storage>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub start: usize,
    pub finish: usize,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    pub document: String,
    pub revision: String,
    #[serde(deserialize_with = "read_edits")]
    pub edits: Vec<Edit>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "read_optional_selections"
    )]
    pub selections: Option<Vec<Selection>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<Storage>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub script: String,
    pub line: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub kind: Error,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
}
impl From<Error> for Failure {
    fn from(kind: Error) -> Self {
        Self {
            kind,
            location: None,
        }
    }
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(location) = &self.location {
            write!(f, " ({}:{})", location.script, location.line)?;
        }
        Ok(())
    }
}
impl std::error::Error for Failure {}
impl Failure {
    pub(crate) fn validate(&self, input: &Input) -> Result<(), Error> {
        if let Some(location) = &self.location {
            let source = if location.script == "plugin" {
                &input.source
            } else {
                input.modules.get(&location.script).ok_or(Error::Worker)?
            };
            if location.script.len() > 52
                || location.line == 0
                || location.line as usize > source.bytes().filter(|b| *b == b'\n').count() + 1
            {
                return Err(Error::Worker);
            }
        }
        Ok(())
    }
    pub(crate) fn evaluation(kind: Error, location: Option<Location>) -> Self {
        Self {
            kind: match kind {
                Error::Worker => Error::SdkTransaction,
                Error::ReturnType => Error::SdkType,
                Error::ReturnSize => Error::SdkLimit,
                other => other,
            },
            location,
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reply {
    pub protocol_version: u32,
    #[serde(default)]
    pub worker_time_us: u64,
    pub outcome: Outcome,
}
#[derive(Deserialize, Serialize)]
// External tagging lets serde stream bounded transaction arrays directly;
// internal tagging would buffer the entire untrusted outcome before validation.
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    Success(Transaction),
    Failure(Failure),
}

pub struct Prepared {
    pub text: String,
    pub selections: Vec<Selection>,
    pub storage: Option<Storage>,
}

#[derive(Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Storage(
    #[serde(deserialize_with = "read_options")] pub BTreeMap<String, options::Value>,
);
impl fmt::Debug for Storage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Storage")
            .field("entries", &self.0.len())
            .finish_non_exhaustive()
    }
}
impl Storage {
    pub fn validate(&self) -> Result<(), Error> {
        if self.0.len() > options::MAX_SETTINGS
            || self
                .0
                .iter()
                .any(|(key, value)| !options::valid_id(key) || !value.valid())
        {
            return Err(Error::Request);
        }
        Ok(())
    }
}

pub fn boundary(text: &str, byte: usize) -> bool {
    text.is_char_boundary(byte)
        && !(byte > 0 && text.as_bytes().get(byte - 1..=byte) == Some(b"\r\n"))
}

pub(crate) struct Coordinates {
    text: std::sync::Arc<str>,
    starts: Vec<usize>,
}
impl Coordinates {
    pub(crate) fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
        Self {
            text: text.into(),
            starts,
        }
    }
    pub(crate) fn line_column(&self, byte: usize) -> Result<(usize, usize), Error> {
        if !boundary(&self.text, byte) {
            return Err(Error::Request);
        }
        let line = self.starts.partition_point(|at| *at <= byte) - 1;
        Ok((line, byte - self.starts[line]))
    }
    pub(crate) fn offset(&self, line: usize, column: usize) -> Result<usize, Error> {
        let start = *self.starts.get(line).ok_or(Error::Request)?;
        let finish = self.starts.get(line + 1).map_or(self.text.len(), |next| {
            let finish = next - 1;
            if finish > start && self.text.as_bytes()[finish - 1] == b'\r' {
                finish - 1
            } else {
                finish
            }
        });
        let byte = start.checked_add(column).ok_or(Error::Request)?;
        if byte > finish || !boundary(&self.text, byte) {
            return Err(Error::Request);
        }
        Ok(byte)
    }
}

impl Snapshot {
    pub fn validate(&self) -> Result<(), Error> {
        if self.document.len() != 32
            || !self
                .document
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self
                .revision
                .parse::<u64>()
                .ok()
                .map(|v| v.to_string())
                .as_deref()
                != Some(self.revision.as_str())
            || self.language.len() > 256
            || self.text.len() > MAX_SELECTION_BYTES
            || !(1..=MAX_SELECTIONS).contains(&self.selections.len())
            || self
                .selections
                .iter()
                .any(|s| !boundary(&self.text, s.anchor) || !boundary(&self.text, s.head))
        {
            return Err(Error::Request);
        }
        Ok(())
    }

    /// Zero-based line and UTF-8 byte column in this exact snapshot.
    pub fn line_column(&self, byte: usize) -> Result<(usize, usize), Error> {
        Coordinates::new(&self.text).line_column(byte)
    }
    pub fn offset(&self, line: usize, column: usize) -> Result<usize, Error> {
        Coordinates::new(&self.text).offset(line, column)
    }
}

impl Input {
    pub fn validate(&self) -> Result<(), Error> {
        self.snapshot.validate()?;
        if let Some(storage) = &self.storage {
            storage.validate()?;
        }
        if self.sdk_version != VERSION
            || self.source.len() > MAX_SOURCE_BYTES
            || self.source.len()
                + self.modules.values().map(String::len).sum::<usize>()
                + self.resources.values().map(String::len).sum::<usize>()
                > MAX_PACKAGE_BYTES
            || !valid_resources(&self.modules, &self.resources)
        {
            return Err(Error::Request);
        }
        for values in [&self.settings, &self.arguments] {
            if values.len() > options::MAX_SETTINGS
                || values
                    .iter()
                    .any(|(k, v)| !options::valid_id(k) || !v.valid())
            {
                return Err(Error::Request);
            }
        }
        Ok(())
    }
}

/// Encoded replacement bytes after the SDK's LF/CRLF normalization.
pub fn normalized_replacement_bytes(text: &str, crlf: bool) -> usize {
    let newlines = text.matches('\n').count();
    if newlines == 0 {
        return text.len();
    }
    let pairs = text.matches("\r\n").count();
    text.len() - pairs + if crlf { newlines } else { 0 }
}
fn normalize_replacement(text: &str, crlf: bool, bytes: usize) -> std::borrow::Cow<'_, str> {
    if bytes == text.len() {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut output = String::with_capacity(bytes);
    for line in text.split_inclusive('\n') {
        if let Some(line) = line.strip_suffix('\n') {
            output.push_str(line.strip_suffix('\r').unwrap_or(line));
            output.push_str(if crlf { "\r\n" } else { "\n" });
        } else {
            output.push_str(line);
        }
    }
    std::borrow::Cow::Owned(output)
}

/// Shared UTF-8/CRLF edit validation for host-owned workspace receipts. This does
/// not grant document authority; SDK transactions still validate their snapshots.
/// Input is capped at 1 MiB; replacements retain the SDK's 256 KiB/64-edit limits.
pub fn prepare_text_edits(
    original: &str,
    crlf: bool,
    edits: &[Edit],
) -> Result<(String, Option<usize>), Error> {
    if original.len() > 1024 * 1024 || original.contains('\0') || edits.len() > MAX_EDITS {
        return Err(Error::Request);
    }
    let mut normalized_edits = Vec::with_capacity(edits.len());
    let mut final_bytes = original.len();
    let mut previous = 0;
    let mut replacement_bytes = 0usize;
    let mut raw_bytes = 0usize;
    for (index, edit) in edits.iter().enumerate() {
        if edit.start > edit.finish
            || edit.start < previous
            || !boundary(original, edit.start)
            || !boundary(original, edit.finish)
            || (index > 0 && edits[index - 1].start == edit.start)
        {
            return Err(Error::Worker);
        }
        if edit.text.contains('\0') {
            return Err(Error::ReturnEncoding);
        }
        raw_bytes = raw_bytes
            .checked_add(edit.text.len())
            .ok_or(Error::ReturnSize)?;
        if raw_bytes > MAX_OUTPUT_BYTES {
            return Err(Error::ReturnSize);
        }
        let normalized_bytes = normalized_replacement_bytes(&edit.text, crlf);
        replacement_bytes = replacement_bytes
            .checked_add(normalized_bytes)
            .ok_or(Error::ReturnSize)?;
        if replacement_bytes > MAX_OUTPUT_BYTES {
            return Err(Error::ReturnSize);
        }
        let normalized = normalize_replacement(&edit.text, crlf, normalized_bytes);
        final_bytes = final_bytes
            .checked_sub(edit.finish - edit.start)
            .and_then(|bytes| bytes.checked_add(normalized.len()))
            .ok_or(Error::ReturnSize)?;
        normalized_edits.push((edit, normalized));
        previous = edit.finish;
    }
    let mut text = String::with_capacity(final_bytes);
    let mut previous = 0;
    let mut caret = None;
    for (edit, normalized) in normalized_edits {
        text.push_str(&original[previous..edit.start]);
        text.push_str(&normalized);
        caret = Some(text.len());
        previous = edit.finish;
    }
    text.push_str(&original[previous..]);
    Ok((text, caret))
}

impl Transaction {
    /// Revalidate untrusted output against the original host snapshot before effects.
    pub fn prepare(&self, snapshot: &Snapshot) -> Result<Prepared, Error> {
        snapshot.validate()?;
        if let Some(storage) = &self.storage {
            storage.validate()?;
            if !self.edits.is_empty() || self.selections.is_some() {
                return Err(Error::Worker);
            }
        }
        if self.document != snapshot.document
            || self.revision != snapshot.revision
            || self.edits.len() > MAX_EDITS
        {
            return Err(Error::Worker);
        }
        let (text, caret) = prepare_text_edits(&snapshot.text, snapshot.crlf, &self.edits)?;
        let selections = self.selections.clone().unwrap_or_else(|| {
            if text == snapshot.text {
                return snapshot.selections.clone();
            }
            caret.map_or_else(
                || snapshot.selections.clone(),
                |byte| {
                    let byte = if boundary(&text, byte) {
                        byte
                    } else {
                        byte.saturating_sub(1)
                    };
                    vec![Selection {
                        anchor: byte,
                        head: byte,
                    }]
                },
            )
        });
        if !(1..=MAX_SELECTIONS).contains(&selections.len())
            || selections
                .iter()
                .any(|s| !boundary(&text, s.anchor) || !boundary(&text, s.head))
        {
            return Err(Error::Worker);
        }
        Ok(Prepared {
            text,
            selections,
            storage: self.storage.clone(),
        })
    }
}

pub(crate) fn bounded<'de, D, T, const N: usize>(decoder: D) -> Result<Vec<T>, D::Error>
where
    D: de::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Items<T, const N: usize>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const N: usize> de::Visitor<'de> for Items<T, N> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "at most {N} entries")
        }
        fn visit_seq<A: de::SeqAccess<'de>>(self, mut sequence: A) -> Result<Vec<T>, A::Error> {
            let mut values = Vec::new();
            while values.len() < N {
                let Some(value) = sequence.next_element()? else {
                    return Ok(values);
                };
                values.push(value);
            }
            struct Excess;
            impl<'de> de::DeserializeSeed<'de> for Excess {
                type Value = ();
                fn deserialize<D: de::Deserializer<'de>>(self, _: D) -> Result<(), D::Error> {
                    Err(de::Error::custom("too many entries"))
                }
            }
            sequence.next_element_seed(Excess)?;
            Ok(values)
        }
    }
    decoder.deserialize_seq(Items::<T, N>(PhantomData))
}
fn read_selections<'de, D: de::Deserializer<'de>>(d: D) -> Result<Vec<Selection>, D::Error> {
    bounded::<D, Selection, MAX_SELECTIONS>(d)
}
fn read_edits<'de, D: de::Deserializer<'de>>(d: D) -> Result<Vec<Edit>, D::Error> {
    bounded::<D, Edit, MAX_EDITS>(d)
}
fn read_optional_selections<'de, D: de::Deserializer<'de>>(
    d: D,
) -> Result<Option<Vec<Selection>>, D::Error> {
    // Omission preserves the documented default; explicit null is not a selection list.
    read_selections(d).map(Some)
}
fn read_options<'de, D: de::Deserializer<'de>>(
    d: D,
) -> Result<BTreeMap<String, options::Value>, D::Error> {
    struct Options;
    impl<'de> de::Visitor<'de> for Options {
        type Value = BTreeMap<String, options::Value>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("unique bounded scalar options")
        }
        fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut values = BTreeMap::new();
            while let Some(key) = map.next_key::<String>()? {
                if values.len() == options::MAX_SETTINGS
                    || !options::valid_id(&key)
                    || values.contains_key(&key)
                {
                    return Err(de::Error::custom("invalid option key"));
                }
                let value: options::Value = map.next_value()?;
                if !value.valid() {
                    return Err(de::Error::custom("invalid option value"));
                }
                values.insert(key, value);
            }
            Ok(values)
        }
    }
    d.deserialize_map(Options)
}

pub fn valid_resources(
    modules: &BTreeMap<String, String>,
    resources: &BTreeMap<String, String>,
) -> bool {
    modules.len() <= MAX_RESOURCES
        && resources.len() <= MAX_RESOURCES
        && modules.iter().all(|(name, source)| {
            name.strip_suffix(".lua").is_some_and(options::valid_id)
                && !source.trim().is_empty()
                && source.len() <= MAX_SOURCE_BYTES
                && !source.contains(['\0', '\u{1b}'])
        })
        && resources
            .iter()
            .all(|(name, value)| options::valid_id(name) && value.len() <= MAX_SOURCE_BYTES)
        && modules
            .values()
            .chain(resources.values())
            .map(String::len)
            .sum::<usize>()
            <= MAX_PACKAGE_BYTES
}
fn read_modules<'de, D: de::Deserializer<'de>>(d: D) -> Result<BTreeMap<String, String>, D::Error> {
    read_resources_map::<D, true>(d)
}
fn read_resources<'de, D: de::Deserializer<'de>>(
    d: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    read_resources_map::<D, false>(d)
}
fn read_resources_map<'de, D: de::Deserializer<'de>, const MODULE: bool>(
    d: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    struct Resources<const MODULE: bool>;
    impl<'de, const MODULE: bool> de::Visitor<'de> for Resources<MODULE> {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("unique bounded package resources")
        }
        fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut values = BTreeMap::new();
            let mut bytes = 0;
            while let Some(name) = map.next_key::<String>()? {
                let valid = if MODULE {
                    name.strip_suffix(".lua").is_some_and(options::valid_id)
                } else {
                    options::valid_id(&name)
                };
                if !valid || values.len() == MAX_RESOURCES || values.contains_key(&name) {
                    return Err(de::Error::custom("invalid resource name"));
                }
                let value: String = map.next_value()?;
                bytes += value.len();
                if value.len() > MAX_SOURCE_BYTES || bytes > MAX_PACKAGE_BYTES {
                    return Err(de::Error::custom("resource limit"));
                }
                values.insert(name, value);
            }
            Ok(values)
        }
    }
    d.deserialize_map(Resources::<MODULE>)
}
