//! Owner-only ACLs for the private storage directory and its inherited files.
use std::{fs::File, io, mem, os::windows::io::AsRawHandle, ptr};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{SystemServices::ACCESS_ALLOWED_ACE_TYPE, Threading::*},
};
fn denied() -> io::Error {
    io::Error::other("Private storage requires an owner-only access policy.")
}
fn information(class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<usize>> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut bytes = 0;
    unsafe { GetTokenInformation(token, class, ptr::null_mut(), 0, &mut bytes) };
    let mut buffer = vec![0usize; (bytes as usize).div_ceil(mem::size_of::<usize>())];
    let result =
        unsafe { GetTokenInformation(token, class, buffer.as_mut_ptr().cast(), bytes, &mut bytes) };
    let failure = (result == 0).then(io::Error::last_os_error);
    unsafe { CloseHandle(token) };
    if let Some(failure) = failure {
        return Err(failure);
    }
    Ok(buffer)
}
fn sid(user: &[usize]) -> PSID {
    unsafe { (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid }
}
fn validate(handle: HANDLE, owner: PSID, check_acl: bool) -> io::Result<()> {
    let mut descriptor = ptr::null_mut();
    let mut actual = ptr::null_mut();
    let mut acl = ptr::null_mut();
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut actual,
            ptr::null_mut(),
            &mut acl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let result = (|| {
        if actual.is_null() {
            return Err(denied());
        }
        if unsafe { EqualSid(actual, owner) } == 0 {
            // Elevated Windows processes may create files owned by Administrators.
            // Accept only that well-known privileged default owner, not arbitrary
            // groups; the DACL must still grant data access solely to this user.
            let default = information(TokenOwner)?;
            let default = unsafe { (*(default.as_ptr().cast::<TOKEN_OWNER>())).Owner };
            if unsafe { EqualSid(actual, default) } == 0
                || unsafe { IsWellKnownSid(actual, WinBuiltinAdministratorsSid) } == 0
            {
                return Err(denied());
            }
        }
        if !check_acl {
            return Ok(());
        }
        if acl.is_null() || unsafe { (*acl).AceCount } == 0 || unsafe { (*acl).AceCount } > 32 {
            return Err(denied());
        }
        for index in 0..unsafe { (*acl).AceCount } {
            let mut ace = ptr::null_mut();
            if unsafe { GetAce(acl, index as u32, &mut ace) } == 0
                || unsafe { (*(ace.cast::<ACE_HEADER>())).AceType } as u32
                    != ACCESS_ALLOWED_ACE_TYPE
            {
                return Err(denied());
            }
            let allowed = ace.cast::<ACCESS_ALLOWED_ACE>();
            let trustee = unsafe { ptr::addr_of_mut!((*allowed).SidStart).cast() };
            if unsafe { EqualSid(trustee, owner) } == 0 {
                return Err(denied());
            }
        }
        Ok(())
    })();
    unsafe { LocalFree(descriptor.cast()) };
    result
}
pub(super) fn verify_file(file: &File) -> io::Result<()> {
    let user = information(TokenUser)?;
    validate(file.as_raw_handle().cast(), sid(&user), true)
}
pub(super) fn protect_directory(directory: &File) -> io::Result<()> {
    let user = information(TokenUser)?;
    let owner = sid(&user);
    validate(directory.as_raw_handle().cast(), owner, false)?;
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: FILE_ALL_ACCESS,
        grfAccessMode: SET_ACCESS,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_UNKNOWN,
            ptstrName: owner.cast(),
            ..Default::default()
        },
    };
    let mut acl = ptr::null_mut();
    let status = unsafe { SetEntriesInAclW(1, &entry, ptr::null(), &mut acl) };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let status = unsafe {
        SetSecurityInfo(
            directory.as_raw_handle().cast(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            acl,
            ptr::null(),
        )
    };
    unsafe { LocalFree(acl.cast()) };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}
