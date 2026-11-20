//! Bounded scalar values shared by the editor and the Lua worker.
use serde::{Deserialize, Serialize};

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

pub fn valid_id(value: &str) -> bool {
    value.len() <= 48
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}
