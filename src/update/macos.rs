use super::*;
use std::{collections::HashSet, os::unix::fs::PermissionsExt};

#[cfg(test)]
mod tests;

pub(super) fn installation_root(executable: &Path) -> Result<PathBuf, String> {
    let bundle = executable
        .ancestors()
        .nth(3)
        .filter(|path| path.extension().is_some_and(|ext| ext == "app"))
        .ok_or("Source builds cannot update in place. Install the release app first.")?;
    if bundle.join("Contents/MacOS/hedit") != executable {
        return Err("Unrecognized app bundle layout. Install the release app first.".into());
    }
    if bundle
        .components()
        .any(|part| part.as_os_str() == "AppTranslocation")
    {
        return Err("Move hedit.app to Applications and reopen it before updating.".into());
    }
    bundle_info(bundle, None)?;
    bundle
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Missing app directory.".into())
}

fn bundle_info(bundle: &Path, version: Option<&str>) -> Result<(), String> {
    let path = bundle.join("Contents/Info.plist");
    if !fs::symlink_metadata(&path).map_err(error)?.is_file() {
        return Err("App Info.plist must be a regular file.".into());
    }
    if path.metadata().map_err(error)?.len() > METADATA_LIMIT {
        return Err("App Info.plist is too large.".into());
    }
    let value = plist::Value::from_file(path).map_err(error)?;
    let info = value.as_dictionary().ok_or("Invalid app Info.plist.")?;
    for (key, expected) in [
        ("CFBundleIdentifier", "hedit.hedit"),
        ("CFBundleExecutable", "hedit"),
        ("CFBundlePackageType", "APPL"),
    ] {
        if info.get(key).and_then(plist::Value::as_string) != Some(expected) {
            return Err(format!("Unrecognized app bundle: invalid {key}."));
        }
    }
    if let Some(version) = version {
        for key in ["CFBundleVersion", "CFBundleShortVersionString"] {
            if info.get(key).and_then(plist::Value::as_string) != Some(version) {
                return Err("App bundle version does not match the selected release.".into());
            }
        }
    }
    Ok(())
}

pub(super) fn verify_bundle(bundle: &Path, version: &str) -> Result<(), String> {
    bundle_info(bundle, Some(version))?;
    verify_signature(bundle)
}

pub(super) fn verify_signature(bundle: &Path) -> Result<(), String> {
    let output = tempfile::tempfile().map_err(error)?;
    run_tool(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(bundle),
        &output,
        4096,
        Duration::from_secs(60),
    )
    .map_err(|e| format!("App signature verification failed. Nothing was installed.\n{e}"))
}

// Read ZIP entries ourselves: no links are created, and AppleDouble metadata
// (__MACOSX) is discarded. Code signatures are verified on the resulting bundle.
pub(super) fn unpack(archive: &Path, destination: &Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(File::open(archive).map_err(error)?).map_err(error)?;
    if zip.is_empty() || zip.len() > 10_000 {
        return Err("Release archive is empty or contains too many entries.".into());
    }
    let mut paths = HashSet::new();
    let mut total = 0u64;
    // Validate the whole archive before writing anything, including metadata.
    for index in 0..zip.len() {
        let file = zip.by_index(index).map_err(error)?;
        let name = file.name().trim_end_matches('/');
        let mode = file.unix_mode().unwrap_or(0) & 0o170000;
        if file.enclosed_name().is_none()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"/_.-+@".contains(&b))
            || name.split('/').any(|part| part.is_empty() || matches!(part, "." | ".."))
            || !(name == "hedit.app" || name.starts_with("hedit.app/")
                || name == "__MACOSX" || name.starts_with("__MACOSX/"))
            || !matches!(mode, 0 | 0o100000 | 0o040000)
            || (mode == 0o040000 && !file.is_dir())
            || file.encrypted()
            // Case-insensitive filesystems must not alias two entries.
            || !paths.insert(name.to_ascii_lowercase())
        {
            return Err("Archive contains an unsafe, duplicate, or unsupported path.".into());
        }
        total = total
            .checked_add(file.size())
            .ok_or("Archive size overflow.")?;
        if total > 1024 * 1024 * 1024 {
            return Err("Unpacked release exceeds 1 GiB.".into());
        }
    }
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).map_err(error)?;
        if file.name().starts_with("__MACOSX/") || file.name() == "__MACOSX" {
            continue;
        }
        let path = destination.join(file.enclosed_name().ok_or("Unsafe archive path.")?);
        if file.is_dir() {
            fs::create_dir_all(path).map_err(error)?;
        } else {
            fs::create_dir_all(path.parent().ok_or("Missing archive parent.")?).map_err(error)?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(error)?;
            let expected = file.size();
            let count =
                std::io::copy(&mut file.by_ref().take(expected + 1), &mut output).map_err(error)?;
            if count != expected {
                return Err("Archive entry size does not match its metadata.".into());
            }
            // Preserve executable bits without accepting setuid/setgid or readonly files.
            let mode = if file.unix_mode().unwrap_or(0) & 0o111 != 0 {
                0o755
            } else {
                0o644
            };
            fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(error)?;
        }
    }
    Ok(())
}
