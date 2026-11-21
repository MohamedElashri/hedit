//! Only the rename/access/ACL primitives differ between native platforms.
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, File, OpenOptions};
use std::io;
pub(super) fn writable(parent: &Dir, name: &str) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .follow(FollowSymlinks::No)
        .nonblock(true);
    #[cfg(windows)]
    {
        use cap_std::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        options
            .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | READ_CONTROL)
            .share_mode(FILE_SHARE_READ);
    }
    let file = parent.open_with(name, &options)?;
    crate::workspace::identity::regular(&file)?;
    if file.metadata()?.permissions().readonly() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Workspace files are read-only.",
        ));
    }
    Ok(file)
}
pub(super) fn new_file(parent: &Dir, name: &str) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(windows)]
    {
        use cap_std::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        options
            .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | READ_CONTROL | WRITE_DAC)
            .share_mode(FILE_SHARE_READ);
    }
    parent.open_with(name, &options)
}
pub(crate) fn private_directory(parent: &Dir, name: &str) -> io::Result<Dir> {
    let mut builder = cap_std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use cap_std::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    parent.create_dir_with(name, &builder)?;
    let directory = cap_fs_ext::DirExt::open_dir_nofollow(parent, name)?;
    #[cfg(unix)]
    let file = directory.try_clone()?.into_std_file();
    #[cfg(windows)]
    let file = {
        use cap_std::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        let mut options = OpenOptions::new();
        options
            .access_mode(FILE_GENERIC_READ | WRITE_DAC)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .follow(FollowSymlinks::No);
        directory.open_with(".", &options)?.into_std()
    };
    hedit_core::private_io::protect_directory(&file)?;
    Ok(directory)
}
pub(crate) fn sync_dir(directory: &Dir) -> io::Result<()> {
    #[cfg(unix)]
    {
        directory.try_clone()?.into_std_file().sync_all()
    }
    #[cfg(windows)]
    {
        let _ = directory;
        Ok(())
    } // File receipts are flushed; Windows has no directory-fsync guarantee.
}
pub(super) fn copy_permissions(source: &File, destination: &File) -> io::Result<()> {
    let source = source.try_clone()?.into_std();
    let destination = destination.try_clone()?.into_std();
    #[cfg(unix)]
    {
        hedit_core::document::copy_permissions(&source, &destination)
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Security::*;
        let mut needed = 0;
        unsafe {
            GetKernelObjectSecurity(
                source.as_raw_handle(),
                DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                0,
                &mut needed,
            )
        };
        if needed == 0 || needed > 1024 * 1024 {
            return Err(io::Error::last_os_error());
        }
        let mut descriptor = vec![0u32; (needed as usize).div_ceil(4)];
        let pointer = descriptor.as_mut_ptr().cast();
        let mut control = 0;
        let mut revision = 0;
        if unsafe {
            GetKernelObjectSecurity(
                source.as_raw_handle(),
                DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION,
                pointer,
                needed,
                &mut needed,
            )
        } == 0
            || unsafe { GetSecurityDescriptorControl(pointer, &mut control, &mut revision) } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut needed = 0;
        // Query the destination identity too: copying an OWNER RIGHTS ACE to a
        // different owner would change who receives access after replacement.
        unsafe {
            GetKernelObjectSecurity(
                destination.as_raw_handle(),
                OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                0,
                &mut needed,
            )
        };
        if needed == 0 || needed > 1024 * 1024 {
            return Err(io::Error::last_os_error());
        }
        let mut target = vec![0u32; (needed as usize).div_ceil(4)];
        let target_pointer = target.as_mut_ptr().cast();
        if unsafe {
            GetKernelObjectSecurity(
                destination.as_raw_handle(),
                OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION,
                target_pointer,
                needed,
                &mut needed,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut source_owner = std::ptr::null_mut();
        let mut target_owner = std::ptr::null_mut();
        let mut source_group = std::ptr::null_mut();
        let mut target_group = std::ptr::null_mut();
        let mut defaulted = 0;
        // SAFETY: both descriptors came from the kernel into aligned, bounded
        // buffers; returned SIDs are borrowed until comparison completes.
        let valid = unsafe {
            GetSecurityDescriptorOwner(pointer, &mut source_owner, &mut defaulted) != 0
                && GetSecurityDescriptorOwner(target_pointer, &mut target_owner, &mut defaulted)
                    != 0
                && GetSecurityDescriptorGroup(pointer, &mut source_group, &mut defaulted) != 0
                && GetSecurityDescriptorGroup(target_pointer, &mut target_group, &mut defaulted)
                    != 0
                && !source_owner.is_null()
                && !target_owner.is_null()
                && !source_group.is_null()
                && !target_group.is_null()
                && EqualSid(source_owner, target_owner) != 0
                && EqualSid(source_group, target_group) != 0
        };
        if !valid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Workspace edits must preserve file ownership.",
            ));
        }
        let protection = if control & SE_DACL_PROTECTED != 0 {
            PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            UNPROTECTED_DACL_SECURITY_INFORMATION
        };
        if unsafe {
            SetKernelObjectSecurity(
                destination.as_raw_handle(),
                DACL_SECURITY_INFORMATION | protection,
                pointer,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        destination.set_permissions(source.metadata()?.permissions())
    }
}
/// Atomically reserve an absent destination. Never replace a concurrent entry.
pub(super) fn rename(
    from: &Dir,
    name: &str,
    file: Option<&File>,
    to: &Dir,
    target: &str,
) -> io::Result<()> {
    #[cfg(unix)]
    {
        let _ = file;
        rustix::fs::renameat_with(from, name, to, target, rustix::fs::RenameFlags::NOREPLACE)
            .map_err(Into::into)
    }
    #[cfg(windows)]
    {
        use std::{mem, os::windows::io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::*;
        let opened;
        let file = match file {
            Some(file) => file,
            None => {
                let mut options = OpenOptions::new();
                use cap_std::fs::OpenOptionsExt;
                options
                    .access_mode(DELETE | FILE_READ_ATTRIBUTES)
                    .share_mode(FILE_SHARE_READ)
                    .follow(FollowSymlinks::No)
                    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
                opened = from.open_with(name, &options)?;
                &opened
            }
        };
        let name: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
        let offset = mem::offset_of!(FILE_RENAME_INFO, FileName);
        let bytes = offset + name.len() * 2;
        let mut storage = vec![0usize; bytes.div_ceil(mem::size_of::<usize>())];
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        // SAFETY: aligned storage covers the flexible UTF-16 member; both handles
        // are live, target is a validated single component, replacement is false.
        unsafe {
            (*info).Anonymous.ReplaceIfExists = 0;
            (*info).RootDirectory = to.as_raw_handle();
            (*info).FileNameLength = ((name.len() - 1) * 2) as u32;
            std::ptr::copy_nonoverlapping(
                name.as_ptr(),
                std::ptr::addr_of_mut!((*info).FileName).cast(),
                name.len(),
            );
        }
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileRenameInfo,
                info.cast(),
                bytes as u32,
            )
        } == 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
