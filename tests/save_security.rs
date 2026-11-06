use hedit::document::Document;
use std::{fs, path::Path, process::Command};

fn save_edit(path: &Path) {
    let mut document = Document::open(1, path).unwrap();
    document
        .replace(0, document.info().size, "edited secret\n".into(), 0)
        .unwrap();
    document.save(None).unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), "edited secret\n");
}

#[cfg(target_os = "macos")]
fn macos_acl(path: &Path) -> Option<Vec<u8>> {
    use std::{
        ffi::{c_int, c_void},
        os::fd::AsRawFd,
    };
    unsafe extern "C" {
        fn acl_get_fd(fd: c_int) -> *mut c_void;
        fn acl_size(acl: *mut c_void) -> isize;
        fn acl_copy_ext(buffer: *mut c_void, acl: *mut c_void, size: isize) -> isize;
        fn acl_free(acl: *mut c_void) -> c_int;
    }
    let file = fs::File::open(path).unwrap();
    // SAFETY: The descriptor is live; a successful result is owned until freed.
    let acl = unsafe { acl_get_fd(file.as_raw_fd()) };
    if acl.is_null() {
        assert_eq!(
            std::io::Error::last_os_error().kind(),
            std::io::ErrorKind::NotFound
        );
        return None;
    }
    // SAFETY: `acl` is a valid allocation returned by acl_get_fd.
    let size = unsafe { acl_size(acl) };
    assert!(size > 0 && size <= 1024 * 1024);
    let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: The buffer is aligned and at least `size` bytes, and the ACL is live.
    let copied = unsafe { acl_copy_ext(buffer.as_mut_ptr().cast(), acl, size) };
    // SAFETY: This is the allocation from acl_get_fd, freed exactly once.
    unsafe { acl_free(acl) };
    assert!(copied >= 0 && copied <= size);
    // SAFETY: acl_copy_ext initialized `copied` bytes in this live allocation.
    let bytes =
        unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), copied as usize) };
    Some(bytes.to_vec())
}

#[cfg(target_os = "macos")]
#[test]
fn saving_preserves_macos_deny_acl() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("secret.txt");
    fs::write(&path, "secret\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Command::new("/bin/chmod")
        .args(["+a", "user:nobody deny read"])
        .arg(&path)
        .status()
        .unwrap()
        .success());
    let before = macos_acl(&path);
    assert!(before.is_some());
    save_edit(&path);
    assert_eq!(
        macos_acl(&path),
        before,
        "saving must retain the restrictive ACL"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn saving_keeps_acl_rules_that_disallow_changing_the_acl_itself() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("restricted-metadata.txt");
    fs::write(&path, "secret\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let user = Command::new("id").arg("-un").output().unwrap();
    assert!(user.status.success());
    let user = String::from_utf8(user.stdout).unwrap();
    assert!(Command::new("/bin/chmod")
        .args(["+a", &format!("user:{} deny writesecurity", user.trim())])
        .arg(&path)
        .status()
        .unwrap()
        .success());
    let before = macos_acl(&path);
    assert!(before.is_some());
    save_edit(&path);
    assert_eq!(macos_acl(&path), before);
}

#[cfg(target_os = "macos")]
#[test]
fn saving_does_not_add_inherited_macos_access_rules() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("secret.txt");
    fs::write(&path, "secret\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(Command::new("/bin/chmod")
        .args(["+a", "user:nobody allow read,file_inherit"])
        .arg(directory.path())
        .status()
        .unwrap()
        .success());
    save_edit(&path);
    assert!(
        macos_acl(&path).is_none(),
        "the save must not grant an inherited reader"
    );
}

#[cfg(unix)]
#[test]
fn saving_preserves_the_files_group() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("secret.txt");
    fs::write(&path, "secret\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let original = fs::metadata(&path).unwrap().gid();
    let output = Command::new("id").arg("-G").output().unwrap();
    assert!(output.status.success());
    let groups = String::from_utf8(output.stdout).unwrap();
    let Some(group) = groups
        .split_whitespace()
        .filter_map(|g| g.parse::<u32>().ok())
        .find(|g| *g != original)
    else {
        return; // A single-group account cannot create this fixture unprivileged.
    };
    let file = fs::File::open(&path).unwrap();
    rustix::fs::fchown(&file, None, Some(rustix::process::Gid::from_raw(group))).unwrap();
    save_edit(&path);
    assert_eq!(
        fs::metadata(&path).unwrap().gid(),
        group,
        "saving must not change who group-readable content is shared with"
    );
}

#[cfg(unix)]
#[test]
fn save_as_can_replace_a_write_only_destination_without_granting_read_access() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("write-only.txt");
    fs::write(&path, "old text\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o200)).unwrap();
    let mut document = Document::empty(1);
    document.replace(0, 0, "replacement\n".into(), 0).unwrap();
    let result = document.save(Some(path.clone()));
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    // Restore fixture read access before asserting so its contents can be checked.
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(result.is_ok(), "{:?}", result.err());
    assert_eq!(mode, 0o200);
    assert_eq!(fs::read_to_string(&path).unwrap(), "replacement\n");
}

#[cfg(unix)]
#[test]
fn a_read_only_save_failure_keeps_edits_for_a_later_retry() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("read-only.txt");
    fs::write(&path, "before\n").unwrap();
    let mut document = Document::open(1, &path).unwrap();
    document.replace(0, 6, "after".into(), 0).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    assert!(document.save(None).is_err());
    assert!(document.info().dirty);
    assert_eq!(document.text(1024).unwrap(), "after\n");
    assert_eq!(fs::read_to_string(&path).unwrap(), "before\n");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    document.save(None).unwrap();
    assert!(!document.info().dirty);
    assert_eq!(fs::read_to_string(&path).unwrap(), "after\n");
}

#[cfg(windows)]
#[test]
fn windows_save_as_can_replace_a_file_that_denies_data_reads() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("write-only.txt");
    fs::write(&path, "old text\n").unwrap();
    let identity = Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .unwrap();
    assert!(identity.status.success());
    let identity = String::from_utf8_lossy(&identity.stdout);
    let sid = identity.split('"').find(|s| s.starts_with("S-1-")).unwrap();
    let denied = format!("*{sid}:(RD)");
    assert!(Command::new("icacls")
        .arg(&path)
        .args(["/deny", &denied])
        .status()
        .unwrap()
        .success());
    assert!(
        fs::read(&path).is_err(),
        "the fixture must deny file data reads"
    );
    let mut document = Document::empty(1);
    document.replace(0, 0, "replacement\n".into(), 0).unwrap();
    let result = document.save(Some(path.clone()));
    let still_denied = fs::read(&path).is_err();
    // Remove only our deny ACE to check content and leave no restricted fixture.
    assert!(Command::new("icacls")
        .arg(&path)
        .args(["/remove:d", &format!("*{sid}")])
        .status()
        .unwrap()
        .success());
    assert!(result.is_ok(), "{:?}", result.err());
    assert!(still_denied, "Save As must retain the deny-read rule");
    assert_eq!(fs::read_to_string(&path).unwrap(), "replacement\n");
}

#[cfg(windows)]
#[test]
fn saving_preserves_windows_deny_acl() {
    for inheritance in ["/inheritance:e", "/inheritance:d"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("secret note Ω.txt");
        fs::write(&path, "secret\n").unwrap();
        assert!(Command::new("icacls")
            .arg(&path)
            .args([inheritance, "/deny", "*S-1-5-32-546:(R)"])
            .status()
            .unwrap()
            .success());
        let acl = || {
            let output = Command::new("icacls").arg(&path).output().unwrap();
            assert!(output.status.success());
            output.stdout
        };
        let before = acl();
        save_edit(&path);
        assert_eq!(acl(), before, "saving must retain the restrictive ACL");
    }
}

#[cfg(target_os = "linux")]
fn linux_acl() -> Vec<u8> {
    // Linux POSIX ACL xattr v2: deny uid 65534 explicitly, while other users
    // have read access. Losing this ACL broadens that user's access.
    let mut acl = 2u32.to_le_bytes().to_vec();
    for (tag, permissions, id) in [
        (0x01u16, 6u16, u32::MAX),
        (0x02, 0, 65534),
        (0x04, 4, u32::MAX),
        (0x10, 4, u32::MAX),
        (0x20, 4, u32::MAX),
    ] {
        acl.extend(tag.to_le_bytes());
        acl.extend(permissions.to_le_bytes());
        acl.extend(id.to_le_bytes());
    }
    acl
}

#[cfg(target_os = "linux")]
#[test]
fn saving_preserves_linux_access_acl() {
    use rustix::fs::{getxattr, setxattr, XattrFlags};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("secret.txt");
    fs::write(&path, "secret\n").unwrap();
    let acl = linux_acl();
    setxattr(&path, "system.posix_acl_access", &acl, XattrFlags::empty()).unwrap();
    save_edit(&path);
    let mut after = vec![0; 4096];
    let size = getxattr(&path, "system.posix_acl_access", after.as_mut_slice()).unwrap();
    assert_eq!(&after[..size], acl);
}

#[cfg(target_os = "linux")]
#[test]
fn saving_does_not_add_the_parent_directories_default_acl() {
    use rustix::{
        fs::{getxattr, setxattr, XattrFlags},
        io::Errno,
    };
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("secret.txt");
    fs::write(&path, "secret\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    // Introduce a default ACL after creating the original. A save stage will
    // inherit it, but the original must keep its existing access rules.
    let mut acl = linux_acl();
    acl[14..16].copy_from_slice(&4u16.to_le_bytes()); // named user's read permission
    setxattr(
        directory.path(),
        "system.posix_acl_default",
        &acl,
        XattrFlags::empty(),
    )
    .unwrap();
    save_edit(&path);
    let mut value = [0; 4096];
    assert_eq!(
        getxattr(&path, "system.posix_acl_access", &mut value),
        Err(Errno::NODATA)
    );
}
