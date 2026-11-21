//! Linux seccomp adapter for the common Lua worker.
use super::io;
use std::mem::MaybeUninit;

// Linux UAPI values from linux/audit.h, linux/magic.h and asm/unistd.h.
#[cfg(target_arch = "x86_64")]
pub(crate) const AUDIT_ARCH: u32 = 0xc000_003e;
#[cfg(target_arch = "aarch64")]
pub(crate) const AUDIT_ARCH: u32 = 0xc000_00b7;
#[cfg(target_arch = "x86_64")]
pub(crate) const X32_SYSCALL_BIT: u32 = 0x4000_0000;
const PIPEFS_MAGIC: u32 = 0x5049_5045;

// Layout of seccomp_data for the supported little-endian ABIs. The kernel
// consumes the low 32 bits for descriptors and protection flags.
pub(crate) const SYSCALL_NUMBER: u32 = 0;
pub(crate) const SYSCALL_ARCH: u32 = 4;
pub(crate) const SYSCALL_ARGS: u32 = 16;
pub(crate) const ARG_BYTES: u32 = 8;

pub(crate) const ALLOW: libc::sock_filter = return_value(libc::SECCOMP_RET_ALLOW);
pub(crate) const DENY: libc::sock_filter =
    return_value(libc::SECCOMP_RET_ERRNO | libc::EPERM as u32);
pub(crate) const KILL: libc::sock_filter = return_value(libc::SECCOMP_RET_KILL_PROCESS);

const ADDRESS_SPACE_BYTES: libc::rlim_t = 256 * 1024 * 1024;
const CPU_SECONDS: libc::rlim_t = 2;

pub(crate) fn enter() -> io::Result<()> {
    validate_pipe(libc::STDIN_FILENO, libc::O_RDONLY)?;
    validate_pipe(libc::STDOUT_FILENO, libc::O_WRONLY)?;

    let parent = unsafe { libc::getppid() };
    set_process_flag(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong)?;
    if unsafe { libc::getppid() } != parent || parent <= 1 {
        return Err(io::Error::other("Lua plugin host already exited"));
    }

    set_process_flag(libc::PR_SET_DUMPABLE, 0)?;
    set_process_flag(libc::PR_SET_NO_NEW_PRIVS, 1)?;

    for (resource, maximum) in [
        (libc::RLIMIT_CORE, 0),
        (libc::RLIMIT_NICE, 0),
        (libc::RLIMIT_AS, ADDRESS_SPACE_BYTES),
        (libc::RLIMIT_CPU, CPU_SECONDS),
    ] {
        let mut limits = MaybeUninit::<libc::rlimit>::uninit();
        // SAFETY: getrlimit initializes the referenced rlimit on success.
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

    close_inherited_descriptors()?;
    install_filter()
}

pub(crate) fn validate_pipe(fd: libc::c_int, access_mode: libc::c_int) -> io::Result<()> {
    let mut filesystem = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: fstatfs initializes filesystem on success.
    if unsafe { libc::fstatfs(fd, filesystem.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    if u32::try_from(unsafe { filesystem.assume_init() }.f_type) != Ok(PIPEFS_MAGIC)
        || flags & libc::O_ACCMODE != access_mode
    {
        return Err(io::Error::other(
            "Lua plugin helper requires anonymous pipes",
        ));
    }
    Ok(())
}

pub(crate) fn set_process_flag(option: libc::c_int, value: libc::c_ulong) -> io::Result<()> {
    // SAFETY: These prctl options take scalar unsigned long arguments.
    if unsafe { libc::prctl(option, value, 0_u64, 0_u64, 0_u64) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn close_inherited_descriptors() -> io::Result<()> {
    // Close stderr as well. The parent gets structured errors over stdout.
    // The maximum possible descriptor is closed regardless of RLIMIT_NOFILE.
    const FIRST_CLOSED_FD: u32 = 2;
    if unsafe { libc::syscall(libc::SYS_close_range, FIRST_CLOSED_FD, u32::MAX, 0_u32) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn install_filter() -> io::Result<()> {
    let mut instructions = vec![
        load_word(SYSCALL_ARCH),
        equal(AUDIT_ARCH, 1, 0),
        KILL,
        load_word(SYSCALL_NUMBER),
    ];
    // x32 shares the Linux audit architecture value with x86_64.
    #[cfg(target_arch = "x86_64")]
    instructions.extend([has_bits(X32_SYSCALL_BIT, 0, 1), KILL]);

    allow_if(
        &mut instructions,
        libc::SYS_read,
        &[Check::Equal(0, libc::STDIN_FILENO as u32)],
    );
    allow_if(
        &mut instructions,
        libc::SYS_write,
        &[Check::Equal(0, libc::STDOUT_FILENO as u32)],
    );
    allow_if(
        &mut instructions,
        libc::SYS_mmap,
        &[
            Check::NoBits(2, !(libc::PROT_READ | libc::PROT_WRITE) as u32),
            Check::Equal(3, (libc::MAP_PRIVATE | libc::MAP_ANONYMOUS) as u32),
        ],
    );
    allow_if(
        &mut instructions,
        libc::SYS_mprotect,
        &[Check::NoBits(
            2,
            !(libc::PROT_READ | libc::PROT_WRITE) as u32,
        )],
    );
    allow_if(
        &mut instructions,
        libc::SYS_mremap,
        &[Check::NoBits(3, !libc::MREMAP_MAYMOVE as u32)],
    );
    allow_if(
        &mut instructions,
        libc::SYS_madvise,
        &[Check::Equal(2, libc::MADV_DONTNEED as u32)],
    );
    // Only CLOCK_REALTIME (0) and CLOCK_MONOTONIC (1). Other clock
    // identifiers can refer to file descriptors or other processes.
    allow_if(
        &mut instructions,
        libc::SYS_clock_gettime,
        &[Check::NoBits(0, !libc::CLOCK_MONOTONIC as u32)],
    );
    for syscall in [
        libc::SYS_brk,
        libc::SYS_munmap,
        libc::SYS_getrandom,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_time,
        libc::SYS_rt_sigreturn,
        libc::SYS_exit,
        libc::SYS_exit_group,
    ] {
        allow_if(&mut instructions, syscall, &[]);
    }
    instructions.push(DENY);
    enforce(instructions)
}

pub(crate) fn enforce(mut instructions: Vec<libc::sock_filter>) -> io::Result<()> {
    let program = libc::sock_fprog {
        len: instructions.len() as u16,
        filter: instructions.as_mut_ptr(),
    };

    // SAFETY: The live instruction buffer and its recorded length describe
    // a classic BPF program. The kernel copies it during the syscall.
    let result = unsafe {
        libc::syscall(
            libc::SYS_seccomp,
            libc::SECCOMP_SET_MODE_FILTER,
            libc::SECCOMP_FILTER_FLAG_TSYNC,
            &program,
        )
    };
    match result {
        0 => Ok(()),
        -1 => Err(io::Error::last_os_error()),
        _ => Err(io::Error::other(
            "Lua plugin isolation could not cover all threads",
        )),
    }
}

pub(crate) enum Check {
    Equal(u32, u32),
    NoBits(u32, u32),
    AllBits(u32, u32),
    ZeroHigh(u32),
}

pub(crate) fn allow_if(
    instructions: &mut Vec<libc::sock_filter>,
    syscall: libc::c_long,
    checks: &[Check],
) {
    // The initial jump skips this entire rule on a syscall mismatch. Each
    // matching syscall has one rule, so a bad argument can return directly.
    let length = checks
        .iter()
        .map(|check| {
            if matches!(check, Check::AllBits(..)) {
                4
            } else {
                3
            }
        })
        .sum::<usize>()
        + 1;
    instructions.push(equal(syscall as u32, 0, length as u8));
    for check in checks {
        match *check {
            Check::Equal(index, value) => {
                instructions.push(load_word(SYSCALL_ARGS + index * ARG_BYTES));
                instructions.push(equal(value, 1, 0));
            }
            Check::ZeroHigh(index) => {
                instructions.push(load_word(SYSCALL_ARGS + index * ARG_BYTES + 4));
                instructions.push(equal(0, 1, 0));
            }
            Check::AllBits(index, mask) => {
                instructions.push(load_word(SYSCALL_ARGS + index * ARG_BYTES));
                instructions.push(instruction(
                    libc::BPF_ALU | libc::BPF_AND | libc::BPF_K,
                    mask,
                    0,
                    0,
                ));
                instructions.push(equal(mask, 1, 0));
            }
            Check::NoBits(index, mask) => {
                instructions.push(load_word(SYSCALL_ARGS + index * ARG_BYTES));
                instructions.push(has_bits(mask, 0, 1));
            }
        }
        instructions.push(DENY);
    }
    instructions.push(ALLOW);
}

pub(crate) const fn instruction(code: u32, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter {
        code: code as u16,
        jt,
        jf,
        k,
    }
}

pub(crate) const fn load_word(offset: u32) -> libc::sock_filter {
    instruction(libc::BPF_LD | libc::BPF_W | libc::BPF_ABS, offset, 0, 0)
}

pub(crate) const fn return_value(value: u32) -> libc::sock_filter {
    instruction(libc::BPF_RET | libc::BPF_K, value, 0, 0)
}

pub(crate) const fn equal(value: u32, jt: u8, jf: u8) -> libc::sock_filter {
    instruction(libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K, value, jt, jf)
}

pub(crate) const fn has_bits(value: u32, jt: u8, jf: u8) -> libc::sock_filter {
    instruction(libc::BPF_JMP | libc::BPF_JSET | libc::BPF_K, value, jt, jf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::{Read as _, Write as _};
    use std::os::fd::IntoRawFd as _;
    use std::process::{Command, Stdio};

    #[test]
    fn confines_native_operations() {
        const PROBE: &str = "HEDIT_SANDBOX_NATIVE_PROBE";
        if std::env::var_os(PROBE).is_some() {
            native_probe();
        }
        for role in ["command", "service"] {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "sandbox::linux::tests::confines_native_operations",
                    "--nocapture",
                ])
                .env_clear()
                .env(PROBE, role)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(b"x").unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "probe exited with {:?}",
                output.status
            );
            assert!(output.stdout.ends_with(b"native probe passed\n"));
        }
    }

    fn native_probe() -> ! {
        let fd = File::open("/proc/self/stat").unwrap().into_raw_fd();
        assert!(fd >= 2);
        close_inherited_descriptors().unwrap();
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));

        let service = std::env::var("HEDIT_SANDBOX_NATIVE_PROBE").as_deref() == Ok("service");
        let entered = if service {
            super::super::enter_service()
        } else {
            enter()
        };
        if entered.is_err() {
            std::process::exit(11);
        }

        let mut byte = [0];
        verify(
            io::stdin().read_exact(&mut byte).is_ok() && byte == *b"x",
            12,
        );
        let fixture_path = c"/proc/self/stat";
        verify(
            denied(unsafe { libc::openat(libc::AT_FDCWD, fixture_path.as_ptr(), libc::O_RDONLY) }),
            13,
        );
        verify(
            denied(unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) }),
            14,
        );
        verify(denied(unsafe { libc::fork() }), 15);
        verify(
            denied(unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 0) }),
            40,
        );
        verify(
            denied(unsafe { libc::read(fd, byte.as_mut_ptr().cast(), 1) as i32 }),
            16,
        );
        verify(
            denied(unsafe { libc::write(0, byte.as_ptr().cast(), 1) as i32 }),
            17,
        );
        verify(
            denied(unsafe { libc::read(1, byte.as_mut_ptr().cast(), 1) as i32 }),
            18,
        );
        let page = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                4096,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        verify(page != libc::MAP_FAILED, 19);
        verify(
            denied(unsafe { libc::mprotect(page, 4096, libc::PROT_EXEC) }),
            20,
        );
        verify(unsafe { libc::munmap(page, 4096) } == 0, 21);
        let file_mapping = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                4096,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                fd,
                0,
            )
        };
        verify(
            file_mapping == libc::MAP_FAILED && last_errno() == Some(libc::EPERM),
            22,
        );
        let too_large = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                ADDRESS_SPACE_BYTES as usize + 4096,
                libc::PROT_READ,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        verify(
            too_large == libc::MAP_FAILED && last_errno() == Some(libc::ENOMEM),
            23,
        );

        let mut output = io::stdout().lock();
        verify(output.write_all(b"native probe passed\n").is_ok(), 24);
        verify(output.flush().is_ok(), 25);
        unsafe { libc::_exit(0) }
    }

    fn denied(result: i32) -> bool {
        result == -1 && last_errno() == Some(libc::EPERM)
    }

    fn last_errno() -> Option<i32> {
        io::Error::last_os_error().raw_os_error()
    }

    fn verify(condition: bool, status: i32) {
        if !condition {
            // The probe has closed stderr and restricted the test harness.
            // Return a distinct status for each assertion.
            unsafe { libc::_exit(status) }
        }
    }

    #[test]
    fn rejects_non_pipe_stdio() {
        const PROBE: &str = "HEDIT_SANDBOX_PIPE_PROBE";
        if std::env::var_os(PROBE).is_some() {
            assert!(enter().is_err());
            return;
        }
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sandbox::linux::tests::rejects_non_pipe_stdio"])
            .env_clear()
            .env(PROBE, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    }
}
