//! macOS adapter: App Sandbox helper identity, descriptors, and resource checks.
//! The Lua engine and its selection protocol are shared with every platform.
use std::{ffi::c_void, fs, io, mem::MaybeUninit, path::PathBuf, process::Child, ptr};

type Ref = *const c_void;
const IDENTIFIER: &std::ffi::CStr = c"hedit.hedit.plugin-worker";
const MAX_PROCESS_BYTES: u64 = 256 * 1024 * 1024;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: Ref);
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFEqual(left: Ref, right: Ref) -> u8;
    fn CFStringCreateWithCString(allocator: Ref, text: *const i8, encoding: u32) -> Ref;
    fn CFDictionaryCreate(
        allocator: Ref,
        keys: *const Ref,
        values: *const Ref,
        count: isize,
        key_callbacks: Ref,
        value_callbacks: Ref,
    ) -> Ref;
    fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
    fn CFDictionaryGetCount(dictionary: Ref) -> isize;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(value: Ref) -> u8;
    fn CFNumberCreate(allocator: Ref, kind: isize, value: *const c_void) -> Ref;
    fn CFNumberGetValue(number: Ref, kind: isize, value: *mut c_void) -> u8;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
}

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecCodeCopySelf(flags: u32, code: *mut Ref) -> i32;
    fn SecCodeCopyGuestWithAttributes(
        host: Ref,
        attributes: Ref,
        flags: u32,
        code: *mut Ref,
    ) -> i32;
    fn SecCodeCheckValidity(code: Ref, flags: u32, requirement: Ref) -> i32;
    fn SecCodeCopySigningInformation(code: Ref, flags: u32, information: *mut Ref) -> i32;
    static kSecGuestAttributePid: Ref;
    static kSecCodeInfoIdentifier: Ref;
    static kSecCodeInfoEntitlementsDict: Ref;
    static kSecCodeInfoTeamIdentifier: Ref;
    static kSecCodeInfoFlags: Ref;
}

struct Owned(Ref);
impl Owned {
    fn new(value: Ref) -> io::Result<Self> {
        if value.is_null() {
            Err(denied())
        } else {
            Ok(Self(value))
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: Create/Copy API results are retained and released exactly once.
        unsafe { CFRelease(self.0) }
    }
}
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "The signed Lua helper is not confined as required.",
    )
}
fn signing(code: &Owned) -> io::Result<Owned> {
    let mut information = ptr::null();
    // SAFETY: Code is live; the API initializes the retained dictionary on success.
    unsafe {
        if SecCodeCheckValidity(code.0, 0, ptr::null()) != 0
            || SecCodeCopySigningInformation(code.0, 2, &mut information) != 0
        {
            return Err(denied());
        }
    }
    Owned::new(information)
}
fn self_code() -> io::Result<Owned> {
    let mut code = ptr::null();
    // SAFETY: The API initializes the retained code object on success.
    if unsafe { SecCodeCopySelf(0, &mut code) } != 0 {
        return Err(denied());
    }
    Owned::new(code)
}
fn require_worker(information: &Owned) -> io::Result<()> {
    // SAFETY: All dictionary values are borrowed while information remains alive.
    unsafe {
        let expected = Owned::new(CFStringCreateWithCString(
            ptr::null(),
            IDENTIFIER.as_ptr(),
            0x0800_0100,
        ))?;
        let identifier = CFDictionaryGetValue(information.0, kSecCodeInfoIdentifier);
        let entitlements = CFDictionaryGetValue(information.0, kSecCodeInfoEntitlementsDict);
        let flags = CFDictionaryGetValue(information.0, kSecCodeInfoFlags);
        let mut bits = 0u32;
        if identifier.is_null()
            || CFEqual(identifier, expected.0) == 0
            || entitlements.is_null()
            || CFGetTypeID(entitlements) != CFDictionaryGetTypeID()
            || CFDictionaryGetCount(entitlements) != 1
            || flags.is_null()
            || CFNumberGetValue(flags, 3, (&mut bits as *mut u32).cast()) == 0
            || bits & 0x10000 == 0
        {
            return Err(denied());
        }
        let key = Owned::new(CFStringCreateWithCString(
            ptr::null(),
            c"com.apple.security.app-sandbox".as_ptr(),
            0x0800_0100,
        ))?;
        let sandboxed = CFDictionaryGetValue(entitlements, key.0);
        if sandboxed.is_null()
            || CFGetTypeID(sandboxed) != CFBooleanGetTypeID()
            || CFBooleanGetValue(sandboxed) == 0
        {
            return Err(denied());
        }
    }
    Ok(())
}

pub(crate) fn worker_executable() -> io::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let parent = executable.parent().ok_or_else(denied)?;
    let directory = if parent.file_name().is_some_and(|name| name == "MacOS")
        && parent
            .parent()
            .is_some_and(|path| path.file_name().is_some_and(|name| name == "Contents"))
    {
        parent.parent().unwrap().join("Helpers")
    } else {
        parent.to_path_buf()
    };
    let worker = directory.join("hedit-plugin.app/Contents/MacOS/hedit");
    if !fs::symlink_metadata(&worker)?.is_file() {
        return Err(denied());
    }
    Ok(worker)
}

pub(crate) fn verify_child(child: &Child) -> io::Result<()> {
    let pid = child.id() as i32;
    // SAFETY: CFNumber/Dictionary copy their inputs; callbacks retain CF values.
    let attributes = unsafe {
        let number = Owned::new(CFNumberCreate(ptr::null(), 3, (&pid as *const i32).cast()))?;
        Owned::new(CFDictionaryCreate(
            ptr::null(),
            &kSecGuestAttributePid,
            &number.0,
            1,
            ptr::addr_of!(kCFTypeDictionaryKeyCallBacks),
            ptr::addr_of!(kCFTypeDictionaryValueCallBacks),
        ))?
    };
    let mut code = ptr::null();
    // SAFETY: The attributes identify our live child; success retains its Code.
    if unsafe { SecCodeCopyGuestWithAttributes(ptr::null(), attributes.0, 0, &mut code) } != 0 {
        return Err(denied());
    }
    let information = signing(&Owned::new(code)?)?;
    require_worker(&information)?;
    let parent = signing(&self_code()?)?;
    // Developer ID workers must have the same team as their running editor.
    // Ad-hoc development builds have no team and accept only ad-hoc helpers.
    unsafe {
        let expected = CFDictionaryGetValue(parent.0, kSecCodeInfoTeamIdentifier);
        let actual = CFDictionaryGetValue(information.0, kSecCodeInfoTeamIdentifier);
        if expected.is_null() != actual.is_null()
            || (!expected.is_null() && CFEqual(expected, actual) == 0)
        {
            return Err(denied());
        }
    }
    Ok(())
}

pub(crate) fn over_memory_limit(child: &Child) -> io::Result<bool> {
    over_memory_limit_pid(child.id())
}
pub(crate) fn over_memory_limit_pid(pid: u32) -> io::Result<bool> {
    let mut usage = MaybeUninit::<libc::rusage_info_v0>::uninit();
    // SAFETY: proc_pid_rusage initializes the requested v0 structure on success.
    if unsafe { libc::proc_pid_rusage(pid as i32, 0, usage.as_mut_ptr().cast()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { usage.assume_init() }.ri_phys_footprint > MAX_PROCESS_BYTES)
}

pub(crate) fn enter() -> io::Result<()> {
    // App Sandbox is applied by macOS at exec, from the signed helper's sole
    // entitlement. Validate that identity before consuming any request bytes.
    require_worker(&signing(&self_code()?)?)?;
    for (fd, mode) in [(0, libc::O_RDONLY), (1, libc::O_WRONLY)] {
        let mut stat = MaybeUninit::<libc::stat>::uninit();
        // SAFETY: fstat initializes stat; fcntl reads only the live descriptor.
        if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0
            || unsafe { stat.assume_init() }.st_mode & libc::S_IFMT != libc::S_IFIFO
            || unsafe { libc::fcntl(fd, libc::F_GETFL) } & libc::O_ACCMODE != mode
        {
            return Err(denied());
        }
    }
    for (resource, maximum) in [
        (libc::RLIMIT_CORE, 0),
        (libc::RLIMIT_CPU, 2),
        (libc::RLIMIT_NPROC, 0),
    ] {
        let mut limits = MaybeUninit::<libc::rlimit>::uninit();
        // SAFETY: getrlimit initializes limits on success; setrlimit gets a valid structure.
        unsafe {
            if libc::getrlimit(resource, limits.as_mut_ptr()) != 0 {
                return Err(io::Error::last_os_error());
            }
            let mut limits = limits.assume_init();
            limits.rlim_cur = limits.rlim_cur.min(maximum);
            limits.rlim_max = limits.rlim_max.min(maximum);
            if libc::setrlimit(resource, &limits) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
    }
    close_descriptors_from(2)
}

pub(crate) fn close_guardian_descriptors() -> io::Result<()> {
    close_descriptors_from(5)
}
pub(crate) fn close_tool_descriptors() -> io::Result<()> {
    close_descriptors_from(3)
}
fn close_descriptors_from(first: i32) -> io::Result<()> {
    // Enumerate actual descriptors instead of assuming RLIMIT_NOFILE covers
    // descriptors inherited before a caller lowered that limit.
    let mut descriptors = vec![
        libc::proc_fdinfo {
            proc_fd: 0,
            proc_fdtype: 0
        };
        4096
    ];
    // SAFETY: The buffer has the advertised size and the kernel initializes its entries.
    let bytes = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDLISTFDS,
            0,
            descriptors.as_mut_ptr().cast(),
            std::mem::size_of_val(descriptors.as_slice()) as i32,
        )
    };
    if bytes <= 0 || bytes as usize == std::mem::size_of_val(descriptors.as_slice()) {
        return Err(denied());
    }
    for descriptor in &descriptors[..bytes as usize / std::mem::size_of::<libc::proc_fdinfo>()] {
        if descriptor.proc_fd >= first {
            // SAFETY: This single-purpose worker no longer uses inherited descriptors.
            unsafe { libc::close(descriptor.proc_fd) };
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
