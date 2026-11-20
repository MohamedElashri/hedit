//! User-owned extension preferences; runtime scalar types live in the shared core.
pub use hedit_plugin::options::{valid_command_id, valid_id, Value, MAX_SETTINGS, MAX_VALUE_BYTES};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
                    || !valid_command_id(&hook.command)
                    || hook.approval.len() != 32
                    || !hook.approval.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err("Invalid extension save hook settings.".into());
        }
        Ok(())
    }
}
