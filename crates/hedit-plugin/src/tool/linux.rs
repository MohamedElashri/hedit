//! Landlock confines file contents and exec; seccomp denies ambient operations.
use super::Launch;
use crate::sandbox::linux::*;
use std::{
    fs::File,
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::ffi::OsStrExt,
    },
    path::Path,
};

pub(super) fn prepare(role: super::Role) -> io::Result<()> {
    for (fd, mode) in [
        (0, libc::O_RDONLY),
        (1, libc::O_WRONLY),
        (2, libc::O_WRONLY),
    ] {
        validate_pipe(fd, mode)?;
    }
    if unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let parent = unsafe { libc::getppid() };
    set_process_flag(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong)?;
    if parent <= 1 || parent != unsafe { libc::getppid() } {
        return Err(io::Error::other("Tool host exited."));
    }
    set_process_flag(libc::PR_SET_DUMPABLE, 0)?;
    set_process_flag(libc::PR_SET_NO_NEW_PRIVS, 1)?;
    for (resource, maximum) in [
        (libc::RLIMIT_CORE, 0),
        (libc::RLIMIT_CPU, role.cpu_seconds()),
        (libc::RLIMIT_NICE, 0),
        (libc::RLIMIT_AS, 256 * 1024 * 1024),
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

pub(super) fn check_resources(pid: u32) -> io::Result<std::time::Duration> {
    // The final ')' delimits comm, which may itself contain spaces/parens.
    use std::io::Read;
    let mut bytes = Vec::new();
    File::open(format!("/proc/{pid}/stat"))?
        .take(4097)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(io::Error::other("Tool accounting failed."));
    }
    let status =
        std::str::from_utf8(&bytes).map_err(|_| io::Error::other("Tool accounting failed."))?;
    let tail = status
        .rsplit_once(") ")
        .ok_or_else(|| io::Error::other("Tool accounting failed."))?
        .1;
    let mut fields = tail.split_ascii_whitespace();
    let user = fields.nth(11).and_then(|n| n.parse::<u64>().ok());
    let system = fields.next().and_then(|n| n.parse::<u64>().ok());
    let threads = fields
        .nth(4)
        .and_then(|n| n.parse::<u32>().ok())
        .ok_or_else(|| io::Error::other("Tool accounting failed."))?;
    if threads > 32 {
        return Err(io::Error::other("Tool thread limit exceeded."));
    }
    let ticks = user
        .zip(system)
        .and_then(|(a, b)| a.checked_add(b))
        .ok_or_else(|| io::Error::other("Tool accounting failed."))?;
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if hz <= 0 {
        return Err(io::Error::other("Tool accounting failed."));
    }
    let nanos = u128::from(ticks) * 1_000_000_000 / hz as u128;
    Ok(std::time::Duration::from_nanos(
        u64::try_from(nanos).map_err(|_| io::Error::other("Tool accounting overflow."))?,
    ))
}

// Linux UAPI ABI 3: all filesystem actions through TRUNCATE, including REFER.
// Require ABI 3 rather than silently losing write/truncate confinement.
const EXECUTE: u64 = 1;
const READ_FILE: u64 = 1 << 2;
const READ_DIR: u64 = 1 << 3;
#[repr(C)]
struct Ruleset {
    handled_access_fs: u64,
}
#[repr(C, packed)]
struct Beneath {
    allowed_access: u64,
    parent_fd: i32,
}

fn landlock(launch: &Launch) -> io::Result<()> {
    let abi = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            std::ptr::null::<u8>(),
            0usize,
            1u32,
        )
    };
    if abi < 3 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Landlock ABI 3 is required.",
        ));
    }
    let attributes = Ruleset {
        handled_access_fs: (1 << 15) - 1,
    };
    let fd = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            &attributes,
            std::mem::size_of_val(&attributes),
            0u32,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let rules = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    add(&rules, &launch.executable, EXECUTE | READ_FILE)?;
    add(&rules, &launch.snapshot, READ_FILE | READ_DIR)?;
    for path in ["/lib", "/lib64", "/usr/lib", "/usr/lib64"] {
        let path = Path::new(path);
        match std::fs::canonicalize(path) {
            Ok(path) => add(&rules, &path, READ_FILE | READ_DIR)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    // The kernel opens an ELF interpreter for execution too. Grant only the
    // architecture's fixed system loader, never executable library directories.
    #[cfg(target_arch = "x86_64")]
    let loaders = ["/lib64/ld-linux-x86-64.so.2", "/lib/ld-musl-x86_64.so.1"];
    #[cfg(target_arch = "aarch64")]
    let loaders = ["/lib/ld-linux-aarch64.so.1", "/lib/ld-musl-aarch64.so.1"];
    for path in loaders {
        match std::fs::canonicalize(path) {
            Ok(path) => add(&rules, &path, EXECUTE | READ_FILE)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    match std::fs::symlink_metadata("/etc/ld.so.cache") {
        Ok(metadata) if metadata.is_file() => {
            add(&rules, Path::new("/etc/ld.so.cache"), READ_FILE)?
        }
        Ok(_) => return Err(super::invalid()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if unsafe { libc::syscall(libc::SYS_landlock_restrict_self, rules.as_raw_fd(), 0u32) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
fn add(rules: &OwnedFd, path: &Path, rights: u64) -> io::Result<()> {
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())?;
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_PATH | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { OwnedFd::from_raw_fd(fd) };
    let attributes = Beneath {
        allowed_access: rights,
        parent_fd: file.as_raw_fd(),
    };
    if unsafe {
        libc::syscall(
            libc::SYS_landlock_add_rule,
            rules.as_raw_fd(),
            1u32,
            &attributes,
            0u32,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn values(
    instructions: &mut Vec<libc::sock_filter>,
    syscall: libc::c_long,
    index: u32,
    values: &[u32],
) {
    instructions.push(equal(syscall as u32, 0, (values.len() + 3) as u8));
    instructions.push(load_word(SYSCALL_ARGS + index * ARG_BYTES));
    for (index, value) in values.iter().enumerate() {
        instructions.push(equal(*value, (values.len() - index) as u8, 0));
    }
    instructions.extend([DENY, ALLOW]);
}
pub(super) fn enter(launch: &Launch) -> io::Result<()> {
    landlock(launch)?;
    let mut instructions = vec![
        load_word(SYSCALL_ARCH),
        equal(AUDIT_ARCH, 1, 0),
        KILL,
        load_word(SYSCALL_NUMBER),
    ];
    #[cfg(target_arch = "x86_64")]
    instructions.extend([has_bits(X32_SYSCALL_BIT, 0, 1), KILL]);
    // Modern libc retries pthread creation with clone when clone3 is absent.
    instructions.extend([
        equal(libc::SYS_clone3 as u32, 0, 1),
        return_value(libc::SECCOMP_RET_ERRNO | libc::ENOSYS as u32),
    ]);
    let required = (libc::CLONE_VM | libc::CLONE_SIGHAND | libc::CLONE_THREAD) as u32;
    let allowed = required
        | (libc::CLONE_FS
            | libc::CLONE_FILES
            | libc::CLONE_SYSVSEM
            | libc::CLONE_SETTLS
            | libc::CLONE_PARENT_SETTID
            | libc::CLONE_CHILD_SETTID
            | libc::CLONE_CHILD_CLEARTID) as u32;
    allow_if(
        &mut instructions,
        libc::SYS_clone,
        &[
            Check::ZeroHigh(0),
            Check::AllBits(0, required),
            Check::NoBits(0, !allowed),
        ],
    );
    values(
        &mut instructions,
        libc::SYS_prctl,
        0,
        &[
            libc::PR_GET_NO_NEW_PRIVS as u32,
            libc::PR_GET_SECCOMP as u32,
        ],
    );
    values(
        &mut instructions,
        libc::SYS_fcntl,
        1,
        &[
            libc::F_GETFD as u32,
            libc::F_SETFD as u32,
            libc::F_GETFL as u32,
            libc::F_SETFL as u32,
            libc::F_DUPFD as u32,
            libc::F_DUPFD_CLOEXEC as u32,
        ],
    );
    #[cfg(target_arch = "x86_64")]
    values(
        &mut instructions,
        libc::SYS_arch_prctl,
        0,
        &[0x1002, 0x1003],
    );
    allow_if(
        &mut instructions,
        libc::SYS_prlimit64,
        &[Check::Equal(0, 0), Check::Equal(2, 0), Check::ZeroHigh(2)],
    );
    allow_if(
        &mut instructions,
        libc::SYS_tgkill,
        &[Check::Equal(0, unsafe { libc::getpid() } as u32)],
    );
    allow_if(
        &mut instructions,
        libc::SYS_sched_getaffinity,
        &[Check::Equal(0, 0)],
    );
    // Read only realtime, monotonic and the current process/thread CPU clocks.
    // Negative encoded clocks can address other processes or file descriptors.
    for syscall in [libc::SYS_clock_gettime, libc::SYS_clock_getres] {
        values(&mut instructions, syscall, 0, &[0, 1, 2, 3]);
    }
    allow_if(
        &mut instructions,
        libc::SYS_clock_nanosleep,
        &[Check::NoBits(0, !1)],
    );
    for syscall in [
        libc::SYS_read,
        libc::SYS_readv,
        libc::SYS_pread64,
        libc::SYS_write,
        libc::SYS_writev,
        libc::SYS_close,
        libc::SYS_openat,
        libc::SYS_fstat,
        libc::SYS_newfstatat,
        libc::SYS_statx,
        libc::SYS_getdents64,
        libc::SYS_readlinkat,
        libc::SYS_faccessat,
        libc::SYS_faccessat2,
        libc::SYS_mmap,
        libc::SYS_munmap,
        libc::SYS_mprotect,
        libc::SYS_mremap,
        libc::SYS_madvise,
        libc::SYS_brk,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigreturn,
        libc::SYS_sigaltstack,
        libc::SYS_futex,
        libc::SYS_set_tid_address,
        libc::SYS_set_robust_list,
        libc::SYS_rseq,
        libc::SYS_getpid,
        libc::SYS_getppid,
        libc::SYS_gettid,
        libc::SYS_getuid,
        libc::SYS_geteuid,
        libc::SYS_getgid,
        libc::SYS_getegid,
        libc::SYS_getrandom,
        libc::SYS_getcwd,
        libc::SYS_chdir,
        libc::SYS_nanosleep,
        libc::SYS_sched_yield,
        libc::SYS_ppoll,
        libc::SYS_pselect6,
        libc::SYS_epoll_create1,
        libc::SYS_epoll_ctl,
        libc::SYS_epoll_pwait,
        libc::SYS_epoll_pwait2,
        libc::SYS_eventfd2,
        libc::SYS_pipe2,
        libc::SYS_dup,
        libc::SYS_dup3,
        libc::SYS_execve,
        libc::SYS_exit,
        libc::SYS_exit_group,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_open,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_poll,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_epoll_wait,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_dup2,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_readlink,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_access,
    ] {
        allow_if(&mut instructions, syscall, &[]);
    }
    instructions.push(DENY);
    enforce(instructions)
}
