//! Revision-bound provider requests and host-rendered results for SDK 2.
//! Each callback rotates the native confined worker; cache is explicit data.
mod coordinates;
pub(crate) mod lua;
pub mod schedule;
#[cfg(test)]
mod tests;
pub use coordinates::{utf16_offset, utf16_position};

use crate::{options, sdk, Error};
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const MAX_RESULT_BYTES: usize = 256 * 1024;
pub const MAX_ITEMS: usize = 128;
pub const STARTUP_MILLIS: u64 = 2_000;
pub(crate) const READY: u8 = 0x7f;
pub const CALLBACK_MILLIS: u64 = 750;
pub const CALLBACK_INSTRUCTIONS: u32 = 200_000;
pub const MAX_ACTIVE: usize = 4;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Diagnostics,
    Completions,
    Hover,
    Symbols,
    Formatting,
    CodeActions,
    Presentation,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Open,
    Change,
    Selection,
    Save,
    Manual,
    Interaction,
    Close,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub kind: Kind,
    pub trigger: Trigger,
    pub position: usize,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub choice: Option<options::Value>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub protocol_version: u32,
    pub request: Request,
    pub package: sdk::Input,
    pub cache: sdk::Storage,
    pub may_edit: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub request: String,
    pub document: String,
    pub revision: String,
    pub cache: sdk::Storage,
    pub result: ResultSet,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultSet {
    Closed,
    Diagnostics(#[serde(deserialize_with = "rows")] Vec<Diagnostic>),
    Completions(#[serde(deserialize_with = "rows")] Vec<Completion>),
    Hover(Hover),
    Symbols(#[serde(deserialize_with = "rows")] Vec<Symbol>),
    Formatting(sdk::Transaction),
    CodeActions(#[serde(deserialize_with = "rows")] Vec<ActionItem>),
    Presentation(Presentation),
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub start: usize,
    pub finish: usize,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: Severity,
    pub message: String,
    #[serde(default, deserialize_with = "actions")]
    pub actions: Vec<ActionItem>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Completion {
    pub label: String,
    #[serde(default)]
    pub detail: String,
    pub range: Range,
    pub insert: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Hover {
    pub range: Range,
    pub text: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Symbol {
    pub name: String,
    #[serde(default)]
    pub detail: String,
    pub range: Range,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActionItem {
    pub title: String,
    pub action: Action,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Edit(sdk::Transaction),
    Select(sdk::Selection),
    Callback { name: String, value: options::Value },
}
#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Presentation {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default, deserialize_with = "rows")]
    pub annotations: Vec<Annotation>,
    #[serde(default)]
    pub quick_list: Option<QuickList>,
    #[serde(default)]
    pub input: Option<Prompt>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Annotation {
    pub range: Range,
    pub text: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuickList {
    pub title: String,
    #[serde(deserialize_with = "rows")]
    pub items: Vec<ActionItem>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Prompt {
    pub title: String,
    pub name: String,
    pub default: options::Value,
    #[serde(default, deserialize_with = "actions")]
    pub choices: Vec<options::Value>,
}

fn rows<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<T>, D::Error> {
    sdk::bounded::<D, T, MAX_ITEMS>(d)
}
fn actions<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Vec<T>, D::Error> {
    sdk::bounded::<D, T, 8>(d)
}
fn text(value: &str, limit: usize) -> Result<(), Error> {
    if value.len() > limit
        || value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        Err(Error::ReturnSize)
    } else {
        Ok(())
    }
}
fn label(value: &str, limit: usize) -> Result<(), Error> {
    text(value, limit)?;
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(Error::ReturnType);
    }
    Ok(())
}
fn token(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Input {
    pub fn validate(&self) -> Result<(), Error> {
        self.package.validate()?;
        self.cache.validate()?;
        if self.protocol_version != VERSION || self.package.storage.is_some() {
            return Err(Error::Request);
        }
        self.request.validate(
            &self.package.snapshot,
            self.cache.0.is_empty(),
            self.may_edit,
        )
    }
}
impl Request {
    pub fn validate(
        &self,
        snapshot: &sdk::Snapshot,
        cache_empty: bool,
        may_edit: bool,
    ) -> Result<(), Error> {
        if !token(&self.id)
            || !sdk::boundary(&snapshot.text, self.position)
            || self
                .action
                .as_deref()
                .is_some_and(|name| !options::valid_id(name))
            || self.choice.as_ref().is_some_and(|value| !value.valid())
            || (self.trigger == Trigger::Interaction
                && (self.action.is_none() || self.choice.is_none()))
            || (self.trigger == Trigger::Close
                && (!snapshot.text.is_empty() || !cache_empty || may_edit))
            || (self.trigger != Trigger::Interaction
                && (self.action.is_some() || self.choice.is_some()))
        {
            return Err(Error::Request);
        }
        Ok(())
    }
}

impl Range {
    pub fn validate(&self, snapshot: &sdk::Snapshot) -> Result<(), Error> {
        if self.start > self.finish
            || !sdk::boundary(&snapshot.text, self.start)
            || !sdk::boundary(&snapshot.text, self.finish)
        {
            return Err(Error::Worker);
        }
        Ok(())
    }
}
impl Action {
    pub fn validate(&self, input: &Input) -> Result<(), Error> {
        self.validate_for(&input.package.snapshot, input.may_edit)
    }
    fn validate_for(&self, snapshot: &sdk::Snapshot, may_edit: bool) -> Result<(), Error> {
        match self {
            Self::Edit(transaction) => {
                if !may_edit || transaction.storage.is_some() {
                    return Err(Error::Worker);
                }
                transaction.prepare(snapshot)?;
            }
            Self::Select(selection) => {
                Range {
                    start: selection.anchor.min(selection.head),
                    finish: selection.anchor.max(selection.head),
                }
                .validate(snapshot)?;
            }
            Self::Callback { name, value } => {
                if !options::valid_id(name) || !value.valid() {
                    return Err(Error::Worker);
                }
            }
        }
        Ok(())
    }
}
impl ActionItem {
    fn validate_for(&self, snapshot: &sdk::Snapshot, may_edit: bool) -> Result<(), Error> {
        label(&self.title, 256)?;
        self.action.validate_for(snapshot, may_edit)
    }
}
impl Output {
    pub fn validate(&self, input: &Input) -> Result<(), Error> {
        input.validate()?;
        self.cache.validate()?;
        if self.request != input.request.id
            || self.document != input.package.snapshot.document
            || self.revision != input.package.snapshot.revision
        {
            return Err(Error::Worker);
        }
        if input.request.trigger == Trigger::Close {
            return if matches!(self.result, ResultSet::Closed) && self.cache.0.is_empty() {
                Ok(())
            } else {
                Err(Error::Worker)
            };
        }
        self.result
            .validate(&input.package.snapshot, input.request.kind, input.may_edit)
    }
}
impl ResultSet {
    /// Validate host-produced results with the same document/permission limits
    /// as worker results, without copying a package or transaction.
    pub fn validate(
        &self,
        snapshot: &sdk::Snapshot,
        kind: Kind,
        may_edit: bool,
    ) -> Result<(), Error> {
        snapshot.validate()?;
        if serde_json::to_vec(self).map_err(|_| Error::Worker)?.len() > MAX_RESULT_BYTES {
            return Err(Error::ReturnSize);
        }
        match (self, kind) {
            (ResultSet::Diagnostics(items), Kind::Diagnostics) => {
                if items.len() > MAX_ITEMS {
                    return Err(Error::ReturnSize);
                }
                for item in items {
                    item.range.validate(snapshot)?;
                    text(&item.message, 1024)?;
                    if item.actions.len() > 8 {
                        return Err(Error::ReturnSize);
                    }
                    for action in &item.actions {
                        action.validate_for(snapshot, may_edit)?;
                    }
                }
            }
            (ResultSet::Completions(items), Kind::Completions) => {
                if !may_edit {
                    return Err(Error::Worker);
                }
                if items.len() > MAX_ITEMS {
                    return Err(Error::ReturnSize);
                }
                for item in items {
                    item.range.validate(snapshot)?;
                    label(&item.label, 256)?;
                    text(&item.detail, 1024)?;
                    text(&item.insert, 4096)?;
                }
            }
            (ResultSet::Hover(item), Kind::Hover) => {
                item.range.validate(snapshot)?;
                text(&item.text, 16 * 1024)?;
            }
            (ResultSet::Symbols(items), Kind::Symbols) => {
                if items.len() > MAX_ITEMS {
                    return Err(Error::ReturnSize);
                }
                for item in items {
                    item.range.validate(snapshot)?;
                    label(&item.name, 256)?;
                    text(&item.detail, 1024)?;
                }
            }
            (ResultSet::Formatting(transaction), Kind::Formatting) => {
                if !may_edit || transaction.storage.is_some() {
                    return Err(Error::Worker);
                }
                transaction.prepare(snapshot)?;
            }
            (ResultSet::CodeActions(items), Kind::CodeActions) => {
                if items.len() > MAX_ITEMS {
                    return Err(Error::ReturnSize);
                }
                for item in items {
                    item.validate_for(snapshot, may_edit)?;
                }
            }
            (ResultSet::Presentation(ui), Kind::Presentation) => {
                if let Some(status) = &ui.status {
                    text(status, 256)?;
                }
                if let Some(output) = &ui.output {
                    text(output, 16 * 1024)?;
                }
                if ui.annotations.len() > MAX_ITEMS {
                    return Err(Error::ReturnSize);
                }
                for item in &ui.annotations {
                    item.range.validate(snapshot)?;
                    text(&item.text, 1024)?;
                }
                if let Some(list) = &ui.quick_list {
                    label(&list.title, 256)?;
                    if list.items.len() > MAX_ITEMS {
                        return Err(Error::ReturnSize);
                    }
                    for item in &list.items {
                        item.validate_for(snapshot, may_edit)?;
                    }
                }
                if let Some(prompt) = &ui.input {
                    label(&prompt.title, 256)?;
                    if !options::valid_id(&prompt.name)
                        || !prompt.default.valid()
                        || prompt.choices.len() > 8
                        || prompt.choices.iter().enumerate().any(|(at, v)| {
                            !v.valid()
                                || !v.same_type(&prompt.default)
                                || prompt.choices[..at].contains(v)
                        })
                        || (!prompt.choices.is_empty() && !prompt.choices.contains(&prompt.default))
                    {
                        return Err(Error::Worker);
                    }
                }
            }
            _ => return Err(Error::Worker),
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reply {
    pub protocol_version: u32,
    pub outcome: Outcome,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    Success(Box<Output>),
    Failure(sdk::Failure),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub kind: Kind,
    pub events: Vec<Trigger>,
    #[serde(default)]
    pub priority: i16,
    #[serde(default = "default_debounce")]
    pub debounce_ms: u16,
    #[serde(default)]
    pub may_edit: bool,
}
fn default_debounce() -> u16 {
    150
}
impl Definition {
    pub fn validate(&self) -> Result<(), Error> {
        if self.events.is_empty()
            || self.events.len() > 7
            || !(-100..=100).contains(&self.priority)
            || !(50..=1000).contains(&self.debounce_ms)
            || self.events.iter().enumerate().any(|(n, event)| {
                *event == Trigger::Interaction || self.events[..n].contains(event)
            })
            || (matches!(self.kind, Kind::Formatting | Kind::Completions) && !self.may_edit)
        {
            return Err(Error::Request);
        }
        Ok(())
    }
    pub fn automatic(&self) -> bool {
        self.events.iter().any(|event| *event != Trigger::Manual)
    }
}
