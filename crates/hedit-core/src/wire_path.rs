//! Lossless Unix path bytes, including when a Windows client displays them.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::PathBuf;
pub fn bytes(path: &std::path::Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        let text = path.to_string_lossy();
        if let Some(hex) = text.strip_prefix("hedit-bytes:") {
            if hex.len() % 2 == 0 {
                if let Some(bytes) = hex
                    .as_bytes()
                    .chunks(2)
                    .map(|p| {
                        std::str::from_utf8(p)
                            .ok()
                            .and_then(|s| u8::from_str_radix(s, 16).ok())
                    })
                    .collect::<Option<Vec<_>>>()
                {
                    return bytes;
                }
            }
        }
        text.as_bytes().to_vec()
    }
}
pub fn from_bytes(bytes: Vec<u8>) -> PathBuf {
    #[cfg(unix)]
    {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        PathBuf::from(OsString::from_vec(bytes))
    }
    #[cfg(not(unix))]
    {
        match String::from_utf8(bytes) {
            Ok(s) => PathBuf::from(s),
            Err(e) => PathBuf::from(format!(
                "hedit-bytes:{}",
                e.as_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            )),
        }
    }
}
pub fn serialize<S: Serializer>(p: &std::path::Path, s: S) -> Result<S::Ok, S::Error> {
    bytes(p).serialize(s)
}
pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PathBuf, D::Error> {
    Vec::<u8>::deserialize(d).map(from_bytes)
}
pub mod option {
    use super::*;
    pub fn serialize<S: Serializer>(p: &Option<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
        p.as_ref().map(|p| bytes(p)).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<PathBuf>, D::Error> {
        Option::<Vec<u8>>::deserialize(d).map(|p| p.map(from_bytes))
    }
}
pub mod vec {
    use super::*;
    pub fn serialize<S: Serializer>(p: &[PathBuf], s: S) -> Result<S::Ok, S::Error> {
        p.iter().map(|p| bytes(p)).collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<PathBuf>, D::Error> {
        Vec::<Vec<u8>>::deserialize(d).map(|p| p.into_iter().map(from_bytes).collect())
    }
}
