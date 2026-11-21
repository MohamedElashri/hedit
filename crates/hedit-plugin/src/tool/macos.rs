//! Fixed Seatbelt policy for native tools, separate from the Lua App Sandbox.
use super::{Launch, HEADER_BYTES, READY};
use std::{
    ffi::CString,
    io::{self, Read, Write},
    os::unix::{ffi::OsStrExt, process::CommandExt},
    process::Command,
};

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

pub(super) fn check_resources(child: &std::process::Child) -> io::Result<()> {
    if crate::sandbox::over_memory_limit(child)? {
        return Err(io::Error::other("Tool memory limit exceeded."));
    }
    let mut task = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
    let bytes = unsafe {
        libc::proc_pidinfo(
            child.id() as i32,
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

fn enter(launch: &Launch) -> io::Result<()> {
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

pub(super) fn bootstrap() -> io::Result<()> {
    crate::sandbox::close_tool_descriptors()?;
    for (fd, mode) in [
        (0, libc::O_RDONLY),
        (1, libc::O_WRONLY),
        (2, libc::O_WRONLY),
    ] {
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0
            || unsafe { stat.assume_init() }.st_mode & libc::S_IFMT != libc::S_IFIFO
            || unsafe { libc::fcntl(fd, libc::F_GETFL) } & libc::O_ACCMODE != mode
        {
            return Err(io::Error::other("Tool transport is unavailable."));
        }
    }
    let mut input = io::stdin().lock();
    let mut length = [0; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > HEADER_BYTES {
        return Err(super::invalid());
    }
    let mut header = vec![0; length];
    input.read_exact(&mut header)?;
    let launch: Launch = serde_json::from_slice(&header).map_err(|_| super::invalid())?;
    launch.validate()?;
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
    let mut command = Command::new(&launch.executable);
    command
        .args(&launch.arguments)
        .env_clear()
        .current_dir(&launch.snapshot);
    enter(&launch)?;
    io::stdout().write_all(READY)?;
    io::stdout().flush()?;
    drop(input);
    Err(command.exec())
}
