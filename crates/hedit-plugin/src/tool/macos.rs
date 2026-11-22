//! Fixed Seatbelt policy for native tools, separate from the Lua App Sandbox.
use super::Launch;
use std::{ffi::CString, io, os::unix::ffi::OsStrExt};

const PROFILE: &std::ffi::CStr = c"
(version 1)
(deny default)
(allow file-read* file-test-existence
  (literal \"/\") (literal \"/System/Volumes\")
  (literal \"/System/Volumes/Preboot\")
  (literal \"/System/Volumes/Preboot/Cryptexes\"))
(allow file-read* file-test-existence file-map-executable
  (subpath \"/System/Volumes/Preboot/Cryptexes/OS\"))
(allow system-fcntl (fcntl-command F_GETPATH F_CHECK_LV F_ADDFILESIGS_RETURN))
(with-filter (mac-policy-name \"Sandbox\")
  (allow system-mac-syscall (mac-syscall-number 2)))
(allow process-exec (literal (param \"EXE\")))
(allow file-map-executable (literal (param \"EXE\"))
  (subpath \"/usr/lib\") (subpath \"/System/Library\"))
(allow file-read* (literal (param \"EXE\"))
  (subpath \"/usr/lib\") (subpath \"/System/Library\")
  (subpath (param \"SNAPSHOT\")))
(allow sysctl-read (sysctl-name \"hw.ncpu\") (sysctl-name \"hw.pagesize_compat\"))
(allow signal (target self))
(allow process-info* (target self))
";

type Initialize = unsafe extern "C" fn(
    *const libc::c_char,
    u64,
    *const *const libc::c_char,
    *mut *mut libc::c_char,
) -> libc::c_int;
type FreeError = unsafe extern "C" fn(*mut libc::c_char);

pub(super) fn check_resources(pid: u32) -> io::Result<()> {
    if crate::sandbox::over_memory_limit_pid(pid)? {
        return Err(io::Error::other("Tool memory limit exceeded."));
    }
    let mut task = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
    let bytes = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTASKINFO,
            0,
            task.as_mut_ptr().cast(),
            std::mem::size_of_val(&task) as i32,
        )
    };
    if bytes as usize != std::mem::size_of_val(&task) {
        return Err(io::Error::last_os_error());
    }
    if unsafe { task.assume_init() }.pti_threadnum > 32 {
        return Err(io::Error::other("Tool thread limit exceeded."));
    }
    Ok(())
}

pub(super) fn enter(launch: &Launch) -> io::Result<()> {
    let executable = CString::new(launch.executable.as_os_str().as_bytes())?;
    let snapshot = CString::new(launch.snapshot.as_os_str().as_bytes())?;
    let parameters = [
        c"EXE".as_ptr(),
        executable.as_ptr(),
        c"SNAPSHOT".as_ptr(),
        snapshot.as_ptr(),
        std::ptr::null(),
    ];
    // Resolve only the trusted system library before applying deny-by-default.
    let library = unsafe {
        libc::dlopen(
            c"/usr/lib/libsandbox.1.dylib".as_ptr(),
            libc::RTLD_NOW | libc::RTLD_LOCAL,
        )
    };
    if library.is_null() {
        return Err(io::Error::other("Native tool policy is unavailable."));
    }
    let initialize = unsafe { libc::dlsym(library, c"sandbox_init_with_parameters".as_ptr()) };
    let free = unsafe { libc::dlsym(library, c"sandbox_free_error".as_ptr()) };
    if initialize.is_null() || free.is_null() {
        unsafe { libc::dlclose(library) };
        return Err(io::Error::other("Native tool policy is unavailable."));
    }
    // These system sandbox SPI exports use the signatures above. Keep the library
    // loaded through policy initialization; exec replaces this address space.
    let initialize: Initialize = unsafe { std::mem::transmute(initialize) };
    let free: FreeError = unsafe { std::mem::transmute(free) };
    let mut error = std::ptr::null_mut();
    let status = unsafe { initialize(PROFILE.as_ptr(), 0, parameters.as_ptr(), &mut error) };
    if !error.is_null() {
        unsafe { free(error) };
    }
    if status != 0 {
        return Err(io::Error::other("Native tool confinement failed."));
    }
    Ok(())
}

pub(super) fn prepare() -> io::Result<()> {
    crate::sandbox::close_tool_descriptors()?;
    // Lower hard limits too: a hostile image must not raise them after exec.
    for (resource, maximum) in [
        (libc::RLIMIT_CORE, 0),
        (libc::RLIMIT_CPU, 2),
        (libc::RLIMIT_NPROC, 0),
        (libc::RLIMIT_NOFILE, 64),
    ] {
        let mut limits = std::mem::MaybeUninit::<libc::rlimit>::uninit();
        if unsafe { libc::getrlimit(resource, limits.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut limits = unsafe { limits.assume_init() };
        limits.rlim_cur = limits.rlim_cur.min(maximum);
        limits.rlim_max = limits.rlim_max.min(maximum);
        if unsafe { libc::setrlimit(resource, &limits) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 10) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
