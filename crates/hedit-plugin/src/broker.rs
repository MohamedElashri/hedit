//! SDK 3 continuation protocol. These types carry requests, never host authority.
use crate::{sdk, service, Error};
use serde::{Deserialize, Serialize};
mod handle;
pub use handle::Handle;
pub(crate) mod json;
pub(crate) mod lua;
pub const VERSION: u32 = 1;
pub const SDK_VERSION: u32 = 3;
pub const MANIFEST_VERSION: u32 = 5;
pub const MAX_MESSAGE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CALLBACKS: usize = 32;
pub const MAX_CHUNK_BYTES: usize = 64 * 1024;
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub protocol_version: u32,
    pub request: Handle,
    pub step: u8,
    pub package: sdk::Input,
    pub scope: Option<Handle>,
    pub may_edit: bool,
    pub workspace_edit: bool,
    pub may_present: bool,
    pub provider: Option<service::Request>,
    pub response: Option<Response>,
    pub cache: sdk::Storage,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub resource: Handle,
    pub version: Handle,
    #[serde(deserialize_with = "edits")]
    pub edits: Vec<sdk::Edit>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Intent {
    Discover {
        scope: Handle,
        query: String,
    },
    Search {
        scope: Handle,
        query: String,
    },
    Open {
        scope: Handle,
        relative: String,
    },
    OpenMany {
        scope: Handle,
        #[serde(deserialize_with = "file_paths")]
        paths: Vec<String>,
    },
    Read {
        resource: Handle,
        version: Handle,
        offset: usize,
        maximum: usize,
    },
    Release {
        resource: Handle,
    },
    StageEdits {
        scope: Handle,
        change: Change,
    },
    ReviewEdits {
        scope: Handle,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "optional_changes"
        )]
        changes: Option<Vec<Change>>,
    },
    Watch {
        scope: Handle,
    },
    Unwatch {
        watch: Handle,
    },
    WaitChanges {
        watch: Handle,
    },
    Tool {
        profile: String,
        input: String,
        arguments: sdk::Storage,
    },
    LanguageServer {
        profile: String,
        method: LspMethod,
        position: Option<usize>,
    },
    Https {
        endpoint: String,
        body: String,
    },
    Finish {
        transaction: Option<sdk::Transaction>,
        result: Option<Box<service::ResultSet>>,
    },
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LspMethod {
    Diagnostics,
    Completions,
    Hover,
    Symbols,
    Formatting,
    CodeActions,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub relative: String,
    pub directory: bool,
    pub bytes: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchMatch {
    pub relative: String,
    pub start: usize,
    pub finish: usize,
    pub excerpt: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub resource: Handle,
    pub version: Handle,
    pub relative: String,
    pub bytes: usize,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceText {
    pub resource: Resource,
    pub text: String,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FailureCode {
    Unavailable,
    Denied,
    Changed,
    Cancelled,
    Limit,
    Failed,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Discovered {
        #[serde(deserialize_with = "entries")]
        entries: Vec<Entry>,
        truncated: bool,
    },
    Searched {
        #[serde(deserialize_with = "matches")]
        matches: Vec<SearchMatch>,
        truncated: bool,
    },
    Opened(Resource),
    OpenedMany {
        #[serde(deserialize_with = "resource_texts")]
        files: Vec<ResourceText>,
    },
    Read {
        text: String,
        next: Option<usize>,
    },
    Released {},
    Staged {
        files: usize,
    },
    Edited {
        files: usize,
    },
    Watching {
        watch: Handle,
        truncated: bool,
    },
    Changed {
        #[serde(deserialize_with = "paths")]
        paths: Vec<String>,
        truncated: bool,
    },
    Unwatched {},
    Tool {
        status: i32,
        output: String,
    },
    LanguageServer {
        result: Box<service::ResultSet>,
    },
    Https {
        status: u16,
        body: String,
    },
    Failed {
        code: FailureCode,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub request: Handle,
    pub step: u8,
    pub intent: Intent,
    pub cache: sdk::Storage,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reply {
    pub protocol_version: u32,
    pub outcome: Outcome,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Outcome {
    Success(Box<Output>),
    Failure(sdk::Failure),
}
fn text(value: &str, maximum: usize) -> bool {
    value.len() <= maximum && !value.contains('\0')
}
impl Input {
    pub fn validate(&self) -> Result<(), Error> {
        self.package.validate()?;
        self.cache.validate()?;
        if self.protocol_version != VERSION
            || usize::from(self.step) >= MAX_CALLBACKS
            || self.package.storage.is_some()
            || (self.workspace_edit && self.scope.is_none())
        {
            return Err(Error::Request);
        }
        if let Some(provider) = &self.provider {
            provider.validate(
                &self.package.snapshot,
                self.cache.0.is_empty(),
                self.may_edit,
            )?;
        }
        if let Some(response) = &self.response {
            response.validate()?;
        }
        if serde_json::to_vec(self).map_err(|_| Error::Request)?.len() > MAX_MESSAGE_BYTES {
            return Err(Error::Request);
        }
        Ok(())
    }
}
impl Response {
    pub fn validate(&self) -> Result<(), Error> {
        let valid = match self {
            Self::Discovered { entries, .. } => {
                entries.len() <= 512
                    && entries
                        .iter()
                        .all(|entry| text(&entry.relative, 4096) && entry.bytes <= 1024 * 1024 + 1)
            }
            Self::Searched { matches, .. } => {
                matches.len() <= 512
                    && matches.iter().all(|entry| {
                        entry.start <= entry.finish
                            && entry.finish <= 1024 * 1024
                            && text(&entry.relative, 4096)
                            && text(&entry.excerpt, 288)
                    })
            }
            Self::OpenedMany { files } => {
                files.len() <= 32
                    && files.iter().all(|file| {
                        text(&file.resource.relative, 4096)
                            && file.text.len() == file.resource.bytes
                            && text(&file.text, MAX_CHUNK_BYTES)
                    })
            }
            Self::Opened(resource) => {
                text(&resource.relative, 4096) && resource.bytes <= 1024 * 1024
            }
            Self::Read { text: body, .. }
            | Self::Https { body, .. }
            | Self::Tool { output: body, .. } => text(body, MAX_CHUNK_BYTES),
            Self::Changed { paths, .. } => {
                paths.len() <= 256 && paths.iter().all(|path| text(path, 4096))
            }
            Self::Staged { files } | Self::Edited { files } => *files <= 32,
            Self::LanguageServer { .. } => true, // Validated against the originating document by the host and Output.
            _ => true,
        };
        let disclosed: usize = match self {
            Self::Discovered { entries, .. } => {
                entries.iter().map(|entry| entry.relative.len()).sum()
            }
            Self::Searched { matches, .. } => matches
                .iter()
                .map(|entry| entry.relative.len() + entry.excerpt.len())
                .sum(),
            Self::Changed { paths, .. } => paths.iter().map(String::len).sum(),
            Self::Opened(resource) => resource.relative.len(),
            Self::OpenedMany { files } => files
                .iter()
                .map(|file| file.resource.relative.len() + file.text.len())
                .sum(),
            Self::Read { text, .. }
            | Self::Https { body: text, .. }
            | Self::Tool { output: text, .. } => text.len(),
            _ => 0,
        };
        if disclosed > MAX_CHUNK_BYTES {
            return Err(Error::Request);
        }
        if !valid || serde_json::to_vec(self).map_err(|_| Error::Request)?.len() > MAX_MESSAGE_BYTES
        {
            return Err(Error::Request);
        }
        Ok(())
    }
}
impl Output {
    pub fn validate(&self, input: &Input) -> Result<(), Error> {
        if self.request != input.request || self.step != input.step {
            return Err(Error::Worker);
        }
        self.cache.validate()?;
        if input
            .provider
            .as_ref()
            .is_some_and(|request| request.trigger == service::Trigger::Close)
            && !matches!(&self.intent,Intent::Finish{transaction:None,result:Some(result)} if matches!(**result,service::ResultSet::Closed))
        {
            return Err(Error::Worker);
        }
        let valid = match &self.intent {
            Intent::Discover { scope, query } => Some(*scope) == input.scope && text(query, 128),
            Intent::Search { scope, query } => {
                Some(*scope) == input.scope && !query.is_empty() && text(query, 128)
            }
            Intent::Open { scope, relative } => {
                Some(*scope) == input.scope && !relative.is_empty() && text(relative, 4096)
            }
            Intent::OpenMany { scope, paths } => {
                Some(*scope) == input.scope
                    && !paths.is_empty()
                    && paths.len() <= 32
                    && paths
                        .iter()
                        .all(|path| !path.is_empty() && text(path, 4096))
            }
            Intent::Read { maximum, .. } => (1..=MAX_CHUNK_BYTES).contains(maximum),
            Intent::StageEdits { scope, change } => {
                input.workspace_edit
                    && Some(*scope) == input.scope
                    && change.edits.len() <= sdk::MAX_EDITS
                    && change
                        .edits
                        .iter()
                        .map(|edit| edit.text.len())
                        .sum::<usize>()
                        <= sdk::MAX_OUTPUT_BYTES
            }
            Intent::ReviewEdits { scope, changes } => {
                input.workspace_edit
                    && Some(*scope) == input.scope
                    && changes.as_ref().is_none_or(|changes| {
                        !changes.is_empty()
                            && changes.len() <= 32
                            && changes
                                .iter()
                                .all(|change| change.edits.len() <= sdk::MAX_EDITS)
                            && changes
                                .iter()
                                .flat_map(|change| &change.edits)
                                .map(|edit| edit.text.len())
                                .sum::<usize>()
                                <= sdk::MAX_OUTPUT_BYTES
                    })
            }
            Intent::Watch { scope } => Some(*scope) == input.scope,
            Intent::Tool {
                profile,
                input,
                arguments,
            } => {
                arguments.validate()?;
                crate::options::valid_id(profile) && text(input, MAX_CHUNK_BYTES)
            }
            Intent::LanguageServer {
                profile, position, ..
            } => {
                crate::options::valid_id(profile)
                    && position.is_none_or(|position| {
                        sdk::boundary(&input.package.snapshot.text, position)
                    })
            }
            Intent::Https { endpoint, body } => {
                crate::options::valid_id(endpoint) && text(body, MAX_CHUNK_BYTES)
            }
            Intent::Finish {
                transaction,
                result,
            } => {
                if (transaction.is_some() && result.is_some())
                    || (transaction.is_none() && result.is_none() && input.provider.is_some())
                {
                    return Err(Error::Worker);
                }
                if let Some(transaction) = transaction {
                    if !input.may_edit || input.provider.is_some() || transaction.storage.is_some()
                    {
                        return Err(Error::Worker);
                    }
                    transaction.prepare(&input.package.snapshot)?;
                }
                if let Some(result) = result {
                    if !input.may_present {
                        return Err(Error::Worker);
                    }
                    let command_request = service::Request {
                        id: input.request.to_string(),
                        kind: service::Kind::Presentation,
                        trigger: service::Trigger::Manual,
                        position: 0,
                        action: None,
                        choice: None,
                    };
                    let request = input.provider.as_ref().unwrap_or(&command_request);
                    let output = service::Output {
                        request: request.id.clone(),
                        document: input.package.snapshot.document.clone(),
                        revision: input.package.snapshot.revision.clone(),
                        cache: Default::default(),
                        result: (**result).clone(),
                    };
                    output.validate(&service::Input {
                        protocol_version: service::VERSION,
                        request: request.clone(),
                        package: input.package.clone(),
                        cache: Default::default(),
                        may_edit: input.may_edit,
                    })?;
                }
                true
            }
            _ => true,
        };
        if !valid || serde_json::to_vec(self).map_err(|_| Error::Worker)?.len() > MAX_MESSAGE_BYTES
        {
            return Err(Error::Worker);
        }
        Ok(())
    }
}
fn bounded<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>, const MAX: usize>(
    decoder: D,
) -> Result<Vec<T>, D::Error> {
    struct Array<T, const MAX: usize>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const MAX: usize> serde::de::Visitor<'de> for Array<T, MAX> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "an array of at most {MAX} entries")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
            let mut result = Vec::new();
            while let Some(value) = seq.next_element()? {
                if result.len() >= MAX {
                    return Err(serde::de::Error::custom("array exceeds its limit"));
                }
                result.push(value);
            }
            Ok(result)
        }
    }
    decoder.deserialize_seq(Array::<T, MAX>(std::marker::PhantomData))
}
fn edits<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<sdk::Edit>, D::Error> {
    bounded::<_, _, 64>(d)
}
fn entries<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Entry>, D::Error> {
    bounded::<_, _, 512>(d)
}
fn matches<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<SearchMatch>, D::Error> {
    bounded::<_, _, 512>(d)
}
fn paths<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    bounded::<_, _, 256>(d)
}
#[cfg(test)]
pub(crate) mod tests;

fn file_paths<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    bounded::<_, _, 32>(d)
}
fn resource_texts<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<ResourceText>, D::Error> {
    bounded::<_, _, 32>(d)
}
fn optional_changes<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Vec<Change>>, D::Error> {
    bounded::<_, _, 32>(d).map(Some)
}
