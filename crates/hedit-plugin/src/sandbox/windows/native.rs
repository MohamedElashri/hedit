//! Stronger token and private-copy permissions for native tool processes.
use super::*;
use windows_sys::Win32::System::SystemServices::SE_GROUP_INTEGRITY;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Lua,
    Tool,
    #[cfg(test)]
    ToolLow,
    #[cfg(test)]
    ToolProbe,
}
impl Mode {
    fn integrity(self) -> u32 {
        match self {
            Self::Tool => 0,
            #[cfg(test)]
            Self::ToolProbe => 0,
            _ => 4096,
        }
    }
}

struct LocalSid(PSID);
impl Drop for LocalSid {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}
fn integrity_sid(rid: u32) -> io::Result<LocalSid> {
    let mut sid = ptr::null_mut();
    check(unsafe { ConvertStringSidToSidW(wide(format!("S-1-16-{rid}")).as_ptr(), &mut sid) })?;
    Ok(LocalSid(sid))
}
pub(super) fn lower_integrity(process: HANDLE, mode: Mode) -> io::Result<()> {
    let mut handle = ptr::null_mut();
    check(unsafe { OpenProcessToken(process, TOKEN_QUERY | TOKEN_ADJUST_DEFAULT, &mut handle) })?;
    let token = unsafe { owned(handle) }?;
    let sid = integrity_sid(mode.integrity())?;
    let length = unsafe { GetLengthSid(sid.0) } as usize;
    let offset = mem::size_of::<TOKEN_MANDATORY_LABEL>();
    let mut bytes = vec![0usize; (offset + length).div_ceil(mem::size_of::<usize>())];
    let copied = unsafe { bytes.as_mut_ptr().cast::<u8>().add(offset).cast() };
    check(unsafe { CopySid(length as u32, copied, sid.0) })?;
    unsafe {
        bytes
            .as_mut_ptr()
            .cast::<TOKEN_MANDATORY_LABEL>()
            .write(TOKEN_MANDATORY_LABEL {
                Label: SID_AND_ATTRIBUTES {
                    Sid: copied,
                    Attributes: SE_GROUP_INTEGRITY as u32,
                },
            });
    }
    check(unsafe {
        SetTokenInformation(
            token.as_raw_handle(),
            TokenIntegrityLevel,
            bytes.as_ptr().cast(),
            (offset + length) as u32,
        )
    })
}
/// Verify the documented LPAC access semantics rather than class 46, which
/// current Windows kernels reject from user mode. Positive and negative probes
/// distinguish missing access, ordinary AppContainers, and LPACs before resume.
pub(super) fn require_lpac(process: HANDLE) -> io::Result<()> {
    let mut primary = ptr::null_mut();
    check(unsafe { OpenProcessToken(process, TOKEN_QUERY | TOKEN_DUPLICATE, &mut primary) })?;
    let primary = unsafe { owned(primary) }?;
    let mut duplicate = ptr::null_mut();
    check(unsafe {
        DuplicateToken(
            primary.as_raw_handle(),
            SecurityIdentification,
            &mut duplicate,
        )
    })?;
    let duplicate = unsafe { owned(duplicate) }?;
    for (package, expected) in [
        (None, false),
        (Some("S-1-15-2-1"), false),
        (Some("S-1-15-2-2"), true),
    ] {
        // Everyone satisfies the ordinary user check; the second ACE exercises
        // the separate package check. Use read access, independent of MIC level.
        let extra = package.map_or(String::new(), |sid| format!("(A;;0x1;;;{sid})"));
        let sddl = wide(format!("O:SYG:SYD:(A;;0x1;;;WD){extra}"));
        let mut descriptor = ptr::null_mut();
        check(unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            )
        })?;
        let descriptor = LocalSid(descriptor);
        let mapping = GENERIC_MAPPING {
            GenericRead: 1,
            GenericWrite: 2,
            GenericExecute: 4,
            GenericAll: 7,
        };
        // No privileged rights are requested, so one PRIVILEGE_SET is enough.
        // A larger requirement or any API error fails closed.
        let mut privileges = PRIVILEGE_SET::default();
        let mut length = mem::size_of_val(&privileges) as u32;
        let mut granted = 0;
        let mut allowed = 0;
        check(unsafe {
            AccessCheck(
                descriptor.0,
                duplicate.as_raw_handle(),
                1,
                &mapping,
                &mut privileges,
                &mut length,
                &mut granted,
                &mut allowed,
            )
        })?;
        if (allowed != 0) != expected || granted != u32::from(expected) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Native tool LPAC access semantics verification failed.",
            ));
        }
    }
    Ok(())
}
/// A fresh restricting SID has no grants on any existing object. Restrict
/// writes as well as applying the independent AppContainer package check.
pub(super) struct WriteToken {
    pub token: OwnedHandle,
    sid: LocalSid,
}
impl WriteToken {
    pub fn create() -> io::Result<Self> {
        let mut handle = ptr::null_mut();
        check(unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY,
                &mut handle,
            )
        })?;
        let primary = unsafe { owned(handle) }?;
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        let parts: Vec<_> = random
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        let mut sid = ptr::null_mut();
        check(unsafe {
            ConvertStringSidToSidW(
                wide(format!(
                    "S-1-5-21-{}-{}-{}-{}",
                    parts[0], parts[1], parts[2], parts[3]
                ))
                .as_ptr(),
                &mut sid,
            )
        })?;
        let sid = LocalSid(sid);
        let restriction = SID_AND_ATTRIBUTES {
            Sid: sid.0,
            Attributes: 0,
        };
        let mut restricted = ptr::null_mut();
        check(unsafe {
            CreateRestrictedToken(
                primary.as_raw_handle(),
                DISABLE_MAX_PRIVILEGE | WRITE_RESTRICTED,
                0,
                ptr::null(),
                0,
                ptr::null(),
                1,
                &restriction,
                &mut restricted,
            )
        })?;
        Ok(Self {
            token: unsafe { owned(restricted) }?,
            sid,
        })
    }
    pub fn verify(&self, process: HANDLE) -> io::Result<()> {
        let token = token(process)?;
        let bytes = token_information(&token, TokenRestrictedSids)?;
        let groups = unsafe { &*bytes.as_ptr().cast::<TOKEN_GROUPS>() };
        if groups.GroupCount != 1 || unsafe { EqualSid(groups.Groups[0].Sid, self.sid.0) } == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Native tool write restriction was not retained.",
            ));
        }
        Ok(())
    }
}

fn word(token: &OwnedHandle, class: TOKEN_INFORMATION_CLASS) -> io::Result<u32> {
    let mut value = 0u32;
    let mut length = mem::size_of_val(&value) as u32;
    check(unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            (&mut value as *mut u32).cast(),
            mem::size_of_val(&value) as u32,
            &mut length,
        )
    })
    .map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("Native token scalar class {class} query failed: {error}"),
        )
    })?;
    if length != mem::size_of_val(&value) as u32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid native token scalar length.",
        ));
    }
    Ok(value)
}
pub(super) fn require_token(process: HANDLE, mode: Mode) -> io::Result<()> {
    let token = token(process)?;
    require_lpac(process)?;
    let policy = word(&token, TokenMandatoryPolicy)?;
    if policy & TOKEN_MANDATORY_POLICY_NO_WRITE_UP == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Native tool mandatory write-up policy verification failed.",
        ));
    }
    let label = token_information(&token, TokenIntegrityLevel)?;
    let label = unsafe { &*label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    let expected = integrity_sid(mode.integrity())?;
    if unsafe { EqualSid(label.Label.Sid, expected.0) } == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Native tool integrity verification failed.",
        ));
    }
    let mut flags = 0u32;
    check(unsafe {
        GetProcessMitigationPolicy(
            process,
            ProcessSystemCallDisablePolicy,
            (&mut flags as *mut u32).cast(),
            mem::size_of_val(&flags),
        )
    })
    .map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("Native Win32k policy query failed: {error}"),
        )
    })?;
    if flags & 1 == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Native tool Win32k verification failed.",
        ));
    }
    Ok(())
}

pub(super) fn snapshot_permissions(root: &Path, sid: PSID) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    let mut pending = vec![root.to_path_buf()];
    let mut paths = Vec::new();
    let mut bytes = 0u64;
    while let Some(path) = pending.pop() {
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || paths.len() >= 2048 {
            return Err(denied());
        }
        if metadata.is_dir() {
            for entry in std::fs::read_dir(&path)? {
                if paths.len() + pending.len() >= 2048 {
                    return Err(denied());
                }
                pending.push(entry?.path());
            }
        } else if metadata.is_file() {
            bytes = bytes.checked_add(metadata.len()).ok_or_else(denied)?;
            if bytes > 64 * 1024 * 1024 {
                return Err(denied());
            }
        } else {
            return Err(denied());
        }
        paths.push(path);
    }
    // Validate the complete host-owned immutable tree before changing any ACL;
    // inheritable entries must never propagate through an unvalidated junction.
    for path in paths {
        image_permissions(&path, sid)?;
    }
    Ok(())
}

pub(super) fn command_line(executable: &Path, arguments: &[&str]) -> io::Result<Vec<u16>> {
    let mut command = Vec::new();
    for argument in std::iter::once(executable.as_os_str()).chain(arguments.iter().map(OsStr::new))
    {
        if !command.is_empty() {
            command.push(b' ' as u16);
        }
        command.push(b'"' as u16);
        let mut slashes = 0;
        for unit in argument.encode_wide() {
            if unit == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid tool argument.",
                ));
            }
            if unit == b'\\' as u16 {
                slashes += 1;
                continue;
            }
            command.extend(std::iter::repeat_n(
                b'\\' as u16,
                if unit == b'"' as u16 {
                    2 * slashes + 1
                } else {
                    slashes
                },
            ));
            slashes = 0;
            command.push(unit);
        }
        command.extend(std::iter::repeat_n(b'\\' as u16, 2 * slashes));
        command.push(b'"' as u16);
    }
    command.push(0);
    if command.len() > 32767 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Tool command line is too long.",
        ));
    }
    Ok(command)
}

pub(super) fn require_job(handle: HANDLE) -> io::Result<()> {
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    check(unsafe {
        QueryInformationJobObject(
            handle,
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            mem::size_of_val(&limits) as u32,
            ptr::null_mut(),
        )
    })?;
    if limits.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_JOB_MEMORY == 0
        || limits.JobMemoryLimit == 0
        || limits.JobMemoryLimit > PROCESS_BYTES
    {
        return Err(denied());
    }
    Ok(())
}
