//! Keep a destination's access restrictions when publishing a replacement inode.
use std::{fs, io, path::Path};
use tempfile::NamedTempFile;

pub(super) fn copy(source: &Path, destination: &NamedTempFile) -> io::Result<()> {
    let source = open_for_metadata(source)?;
    let metadata = source.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::other(
            "The save destination must be a regular file.",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let staged = destination.as_file().metadata()?;
        // The same mode with a different group can expose group-readable data.
        // Do not silently change ownership when it cannot be retained.
        if metadata.uid() != staged.uid() || metadata.gid() != staged.gid() {
            rustix::fs::fchown(
                destination.as_file(),
                Some(rustix::process::Uid::from_raw(metadata.uid())),
                Some(rustix::process::Gid::from_raw(metadata.gid())),
            )?;
        }
    }
    #[cfg(target_os = "macos")]
    copy_acl(&source, destination.as_file())?;
    #[cfg(target_os = "linux")]
    copy_acl(&source, destination.as_file())?;
    #[cfg(windows)]
    copy_acl(&source, destination)?;
    destination
        .as_file()
        .set_permissions(metadata.permissions())
}

fn open_for_metadata(path: &Path) -> io::Result<fs::File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_READ_ATTRIBUTES, READ_CONTROL};
        // Reading the DACL and attributes does not require reading file data.
        fs::OpenOptions::new()
            .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES)
            .open(path)
    }
    #[cfg(not(windows))]
    {
        match crate::file_io::open_regular(path) {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                let mut options = fs::OpenOptions::new();
                // A write-only destination can still be overwritten. Opening
                // without create/truncate never changes its data; the handle
                // is used only for metadata and ACL queries.
                options.write(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
                }
                options.open(path)
            }
            result => result,
        }
    }
}

#[cfg(target_os = "macos")]
fn copy_acl(source: &fs::File, destination: &fs::File) -> io::Result<()> {
    use std::{
        ffi::{c_int, c_void},
        os::fd::AsRawFd,
    };
    unsafe extern "C" {
        fn acl_get_fd(fd: c_int) -> *mut c_void;
        fn acl_init(count: c_int) -> *mut c_void;
        fn acl_set_fd(fd: c_int, acl: *mut c_void) -> c_int;
        fn acl_free(acl: *mut c_void) -> c_int;
    }
    // SAFETY: The source descriptor remains live. A non-null result is an
    // independent ACL allocation, owned here until the single acl_free below.
    let mut acl = unsafe { acl_get_fd(source.as_raw_fd()) };
    if acl.is_null() {
        let error = io::Error::last_os_error();
        if error.raw_os_error().is_some_and(|code| {
            code == rustix::io::Errno::NOTSUP.raw_os_error()
                || code == rustix::io::Errno::OPNOTSUPP.raw_os_error()
        }) {
            return Ok(()); // This filesystem does not support ACLs.
        }
        if error.kind() != io::ErrorKind::NotFound {
            return Err(error);
        }
        // ENOENT on a live descriptor means there is no extended ACL. Apply
        // an empty ACL so the stage cannot grant extra inherited access.
        // SAFETY: Zero is a valid entry count; the result is checked below.
        acl = unsafe { acl_init(0) };
        if acl.is_null() {
            return Err(io::Error::last_os_error());
        }
    }
    // SAFETY: The destination descriptor and allocated ACL are live. Unlike
    // fcopyfile(COPYFILE_ACL), this replaces rather than merges inherited ACLs.
    let result = unsafe { acl_set_fd(destination.as_raw_fd(), acl) };
    let error = (result != 0).then(io::Error::last_os_error);
    // SAFETY: The ACL came from acl_get_fd or acl_init and is freed once.
    unsafe { acl_free(acl) };
    error.map_or(Ok(()), Err)
}

#[cfg(target_os = "linux")]
fn copy_acl(source: &fs::File, destination: &fs::File) -> io::Result<()> {
    use rustix::{
        fs::{fgetxattr, fremovexattr, fsetxattr, XattrFlags},
        io::Errno,
    };
    const ACL: &str = "system.posix_acl_access";
    // Linux limits each extended-attribute value to 64 KiB. Reading in one
    // bounded call also avoids a size-query/allocation race.
    let mut value = vec![0u8; 64 * 1024];
    match fgetxattr(source, ACL, value.as_mut_slice()) {
        Ok(size) => fsetxattr(destination, ACL, &value[..size], XattrFlags::empty())?,
        Err(Errno::NODATA) => match fremovexattr(destination, ACL) {
            // Remove a default ACL inherited from the staging directory too.
            Ok(()) | Err(Errno::NODATA) => (),
            Err(e) => return Err(e.into()),
        },
        Err(Errno::OPNOTSUPP) => (), // This filesystem has no POSIX ACL support.
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

#[cfg(windows)]
fn copy_acl(source: &fs::File, destination: &NamedTempFile) -> io::Result<()> {
    use std::os::windows::{ffi::OsStrExt, io::AsRawHandle};
    use windows_sys::Win32::Security::{
        GetKernelObjectSecurity, GetSecurityDescriptorControl, SetFileSecurityW,
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
        UNPROTECTED_DACL_SECURITY_INFORMATION,
    };
    let source = source.as_raw_handle();
    let mut needed = 0;
    // SAFETY: The handle is live; a null, zero-length buffer queries the size.
    unsafe {
        GetKernelObjectSecurity(
            source,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    if needed == 0 || needed > 1024 * 1024 {
        return Err(io::Error::last_os_error());
    }
    // SECURITY_DESCRIPTOR requires DWORD alignment, hence u32 storage.
    let mut descriptor = vec![0u32; (needed as usize).div_ceil(4)];
    let ptr = descriptor.as_mut_ptr().cast();
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: The descriptor buffer is aligned and at least `needed` bytes;
    // all handles and output pointers remain valid for these calls.
    if unsafe {
        GetKernelObjectSecurity(source, DACL_SECURITY_INFORMATION, ptr, needed, &mut needed)
    } == 0
        || unsafe { GetSecurityDescriptorControl(ptr, &mut control, &mut revision) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let protection = if control & SE_DACL_PROTECTED != 0 {
        PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        UNPROTECTED_DACL_SECURITY_INFORMATION
    };
    let path: Vec<u16> = destination
        .path()
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // Use the path API: the staging file's original read/write handle does not
    // include WRITE_DAC. SetFileSecurityW opens it with the required access.
    // SAFETY: `ptr` is a complete self-relative descriptor returned above;
    // the NUL-terminated path names the still-live save stage.
    if unsafe { SetFileSecurityW(path.as_ptr(), DACL_SECURITY_INFORMATION | protection, ptr) } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
