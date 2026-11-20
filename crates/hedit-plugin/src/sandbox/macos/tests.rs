use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

const PROBE: &str = "HEDIT_MACOS_SANDBOX_PROBE";
const TEST: &str = "sandbox::macos::tests::confines_native_file_network_and_inherited_descriptors";

fn prepare(directory: &std::path::Path) -> PathBuf {
    let output = directory.join("hedit-plugin.app");
    let result = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/macos_plugin_helper.py"
        ))
        .arg("--editor")
        .arg(std::env::current_exe().unwrap())
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    output.join("Contents/MacOS/hedit")
}

#[test]
fn confines_native_file_network_and_inherited_descriptors() {
    if std::env::var_os(PROBE).is_some() {
        if std::env::var(PROBE).as_deref() == Ok("wait") {
            let _ = io::stdin().read_exact(&mut [0]);
            unsafe { libc::_exit(0) }
        }
        if std::env::var(PROBE).as_deref() == Ok("memory") {
            check(enter().is_ok(), 30);
            let bytes = vec![0xa5u8; 300 * 1024 * 1024];
            check(io::stdout().write_all(b"allocated\n").is_ok(), 31);
            let _ = io::stdin().read_exact(&mut [0]);
            std::hint::black_box(bytes);
            unsafe { libc::_exit(0) }
        }
        native_probe();
    }
    let directory = tempfile::tempdir().unwrap();
    let secret = directory.path().join("private.txt");
    fs::write(&secret, "private fixture").unwrap();
    let file = fs::File::open(&secret).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let executable = prepare(directory.path());
    let mut command = Command::new(executable);
    command
        .args(["--exact", TEST, "--nocapture"])
        .env_clear()
        .env(PROBE, "1")
        .env("HEDIT_PROBE_PATH", &secret)
        .env(
            "HEDIT_PROBE_PORT",
            listener.local_addr().unwrap().port().to_string(),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Intentionally inherit a sensitive descriptor. The adapter must remove it
    // even though App Sandbox does not revoke an already-open descriptor.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(file.as_raw_fd(), 200) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    verify_child(&child).unwrap();
    child.stdin.take().unwrap().write_all(b"x").unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "native probe exited: {:?}: {}",
        result.status,
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(result.stdout.ends_with(b"native probe passed\n"));
    assert_eq!(fs::read_to_string(secret).unwrap(), "private fixture");
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

fn native_probe() -> ! {
    let path = std::ffi::CString::new(std::env::var("HEDIT_PROBE_PATH").unwrap()).unwrap();
    let port: u16 = std::env::var("HEDIT_PROBE_PORT").unwrap().parse().unwrap();
    check(unsafe { libc::fcntl(200, libc::F_GETFD) } != -1, 10);
    check(enter().is_ok(), 11);
    check(unsafe { libc::fcntl(200, libc::F_GETFD) } == -1, 12);
    let mut byte = [0];
    check(
        io::stdin().read_exact(&mut byte).is_ok() && byte == *b"x",
        13,
    );
    for (mode, status) in [(libc::O_RDONLY, 14), (libc::O_WRONLY, 15)] {
        check(
            unsafe { libc::open(path.as_ptr(), mode) } == -1 && errno_is_denied(),
            status,
        );
    }
    let socket = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    check(socket >= 0, 16);
    let address = libc::sockaddr_in {
        sin_len: std::mem::size_of::<libc::sockaddr_in>() as u8,
        sin_family: libc::AF_INET as u8,
        sin_port: port.to_be(),
        sin_addr: libc::in_addr {
            s_addr: u32::from_ne_bytes([127, 0, 0, 1]),
        },
        sin_zero: [0; 8],
    };
    check(
        unsafe {
            libc::connect(
                socket,
                (&address as *const libc::sockaddr_in).cast(),
                std::mem::size_of_val(&address) as u32,
            )
        } == -1
            && errno_is_denied(),
        17,
    );
    unsafe { libc::close(socket) };
    let page = unsafe {
        libc::mmap(
            ptr::null_mut(),
            4096,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    check(page != libc::MAP_FAILED, 18);
    // Hardened runtime forbids writable executable memory without an exception.
    // Merely changing an untouched anonymous page to read/execute can succeed;
    // that does not establish that unsigned instructions could execute there.
    check(
        unsafe {
            libc::mprotect(
                page,
                4096,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
            )
        } == -1
            && errno_is_denied(),
        19,
    );
    unsafe { libc::munmap(page, 4096) };
    let mut limits = MaybeUninit::<libc::rlimit>::uninit();
    check(
        unsafe { libc::getrlimit(libc::RLIMIT_CPU, limits.as_mut_ptr()) } == 0
            && unsafe { limits.assume_init() }.rlim_max <= 2,
        20,
    );
    let forked = unsafe { libc::fork() };
    if forked == 0 {
        unsafe { libc::_exit(99) }
    }
    let denied = forked == -1
        && matches!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::EAGAIN | libc::EPERM | libc::EACCES)
        );
    if forked > 0 {
        unsafe { libc::waitpid(forked, ptr::null_mut(), 0) };
        let _ = io::stdout().write_all(b"native fork unexpectedly succeeded\n");
    }
    check(denied, 22);
    let mut limits = MaybeUninit::<libc::rlimit>::uninit();
    check(
        unsafe { libc::getrlimit(libc::RLIMIT_NPROC, limits.as_mut_ptr()) } == 0
            && unsafe { limits.assume_init() }.rlim_max == 0,
        23,
    );
    check(io::stdout().write_all(b"native probe passed\n").is_ok(), 21);
    unsafe { libc::_exit(0) }
}

fn errno_is_denied() -> bool {
    matches!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::EPERM | libc::EACCES)
    )
}

fn check(condition: bool, code: i32) {
    if !condition {
        unsafe { libc::_exit(code) }
    }
}

#[test]
fn rejects_a_helper_with_additional_privileges_before_sending_input() {
    let directory = tempfile::tempdir().unwrap();
    let executable = prepare(directory.path());
    let entitlements = directory.path().join("entitlements.plist");
    fs::write(&entitlements, br#"<?xml version="1.0"?><plist version="1.0"><dict><key>com.apple.security.app-sandbox</key><true/><key>com.apple.security.network.client</key><true/></dict></plist>"#).unwrap();
    let result = Command::new("codesign")
        .args([
            "--force",
            "--sign",
            "-",
            "--options",
            "runtime",
            "--entitlements",
        ])
        .arg(entitlements)
        .arg(directory.path().join("hedit-plugin.app"))
        .output()
        .unwrap();
    assert!(result.status.success());
    let mut child = Command::new(executable)
        .args(["--exact", TEST, "--nocapture"])
        .env_clear()
        .env(PROBE, "wait")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let rejected = verify_child(&child).is_err();
    let live = child.try_wait().unwrap().is_none();
    let _ = child.kill();
    child.wait().unwrap();
    assert!(rejected);
    assert!(live, "signature rejection must inspect a live helper");
}

#[test]
fn detects_native_allocation_beyond_the_process_footprint_limit() {
    let directory = tempfile::tempdir().unwrap();
    let executable = prepare(directory.path());
    let mut child = Command::new(executable)
        .args(["--exact", TEST, "--nocapture"])
        .env_clear()
        .env(PROBE, "memory")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    verify_child(&child).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let exceeded = loop {
        if over_memory_limit(&child).unwrap() {
            break true;
        }
        if child.try_wait().unwrap().is_some() || std::time::Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let _ = child.kill();
    child.wait().unwrap();
    assert!(exceeded);
}
