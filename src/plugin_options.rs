//! User-owned extension preferences and the scalar values exposed to Lua.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_SETTINGS: usize = 16;
pub const MAX_VALUE_BYTES: usize = 256;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Value {
    Boolean(bool),
    Integer(i32),
    Text(String),
}

impl Value {
    pub fn valid(&self) -> bool {
        match self {
            Self::Text(text) => text.len() <= MAX_VALUE_BYTES && !text.contains('\0'),
            _ => true,
        }
    }

    pub fn same_type(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SaveHook {
    pub command: String,
    pub approval: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub preview_edits: bool,
    pub settings: BTreeMap<String, BTreeMap<String, Value>>,
    pub save_hooks: BTreeMap<String, SaveHook>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            preview_edits: true,
            settings: BTreeMap::new(),
            save_hooks: BTreeMap::new(),
        }
    }
}

pub fn valid_id(value: &str) -> bool {
    value.len() <= 48
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

impl Options {
    pub fn validate(&self) -> Result<(), String> {
        if self.settings.len() > 64
            || self.settings.iter().any(|(id, values)| {
                !valid_id(id)
                    || values.len() > MAX_SETTINGS
                    || values
                        .iter()
                        .any(|(key, value)| !valid_id(key) || !value.valid())
            })
        {
            return Err("Invalid extension settings.".into());
        }
        if self.save_hooks.len() > crate::syntax::Language::ALL.len()
            || self.save_hooks.iter().any(|(language, hook)| {
                crate::syntax::Language::from_id(language).is_none()
                    || !hook
                        .command
                        .split_once('.')
                        .is_some_and(|(id, command)| valid_id(id) && valid_id(command))
                    || hook.approval.len() != 32
                    || !hook.approval.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err("Invalid extension save hook settings.".into());
        }
        Ok(())
    }
}
