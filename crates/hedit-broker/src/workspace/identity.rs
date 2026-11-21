//! Native identity only; path policy and all broker semantics are shared.
use super::*;
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub volume: u64,
    #[serde(with = "file_id")]
    pub file: u128,
}
mod file_id {
    pub fn serialize<S: serde::Serializer>(value: &u128, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("{value:032x}"))
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<u128, D::Error> {
        let value = <String as serde::Deserialize>::deserialize(decoder)?;
        if value.len() != 32
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(serde::de::Error::custom("invalid physical file identity"));
        }
        u128::from_str_radix(&value, 16).map_err(serde::de::Error::custom)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stamp {
    pub identity: Identity,
    pub bytes: u64,
    pub changed: i128,
    pub modified: i128,
}
#[cfg(unix)]
fn stamp(metadata: &std::fs::Metadata) -> Stamp {
    use std::os::unix::fs::MetadataExt;
    Stamp {
        identity: Identity {
            volume: metadata.dev(),
            file: u128::from(metadata.ino()),
        },
        bytes: metadata.len(),
        changed: i128::from(metadata.ctime()) * 1_000_000_000 + i128::from(metadata.ctime_nsec()),
        modified: i128::from(metadata.mtime()) * 1_000_000_000 + i128::from(metadata.mtime_nsec()),
    }
}
pub(crate) fn directory(directory: &Dir) -> io::Result<Identity> {
    let file = directory.try_clone()?.into_std_file();
    #[cfg(unix)]
    {
        let metadata = file.metadata()?;
        if !metadata.is_dir() {
            return Err(denied());
        }
        Ok(stamp(&metadata).identity)
    }
    #[cfg(windows)]
    {
        let (stamp, attributes, _) = windows(&file)?;
        if attributes & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY == 0 {
            return Err(denied());
        }
        Ok(stamp.identity)
    }
}
pub(crate) fn regular(file: &File) -> io::Result<Stamp> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let file = file.try_clone()?.into_std();
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(denied());
        }
        Ok(stamp(&metadata))
    }
    #[cfg(windows)]
    {
        let file = file.try_clone()?.into_std();
        let (stamp, attributes, links) = windows(&file)?;
        if attributes & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY != 0
            || links != 1
        {
            return Err(denied());
        }
        Ok(stamp)
    }
}
#[cfg(windows)]
fn windows(file: &std::fs::File) -> io::Result<(Stamp, u32, u32)> {
    use std::{mem, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::*;
    let handle = file.as_raw_handle();
    let mut basic = FILE_BASIC_INFO::default();
    let mut id = FILE_ID_INFO::default();
    let mut standard = FILE_STANDARD_INFO::default();
    if unsafe { GetFileType(handle) } != FILE_TYPE_DISK {
        return Err(denied());
    }
    for (class, pointer, size) in [
        (
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast(),
            mem::size_of_val(&basic),
        ),
        (
            FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),
            mem::size_of_val(&id),
        ),
        (
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            mem::size_of_val(&standard),
        ),
    ] {
        if unsafe { GetFileInformationByHandleEx(handle, class, pointer, size as u32) } == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    if basic.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || standard.DeletePending
        || standard.EndOfFile < 0
    {
        return Err(denied());
    }
    Ok((
        Stamp {
            identity: Identity {
                volume: id.VolumeSerialNumber,
                file: u128::from_le_bytes(id.FileId.Identifier),
            },
            bytes: standard.EndOfFile as u64,
            changed: i128::from(basic.ChangeTime) * 100,
            modified: i128::from(basic.LastWriteTime) * 100,
        },
        basic.FileAttributes,
        standard.NumberOfLinks,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_width_native_file_identities_round_trip_without_json_number_precision_loss() {
        let identity = Identity {
            volume: u64::MAX,
            file: u128::MAX - 42,
        };
        let value = serde_json::to_value(identity).unwrap();
        assert_eq!(value["file"], "ffffffffffffffffffffffffffffffd5");
        assert_eq!(
            serde_json::from_value::<Identity>(value.clone()).unwrap(),
            identity
        );
        for file in [
            serde_json::json!(42),
            serde_json::json!("ff"),
            serde_json::json!("F".repeat(32)),
            serde_json::json!("g".repeat(32)),
        ] {
            let mut value = value.clone();
            value["file"] = file;
            assert!(serde_json::from_value::<Identity>(value).is_err());
        }
    }
}
