//! The same native executable acts as trusted bootstrap and hostile tool image.
//! Never bypass the production policy to exercise a probe.
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if let Some(ok) = hedit_plugin::tool::run_if_requested(&args) {
        std::process::exit(i32::from(!ok));
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    native::run(args);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    println!("Native tool adapter not enabled on this platform.");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod native {
    use hedit_plugin::tool::{self, Launch};
    use std::{
        fs,
        io::{self, Read, Write},
        net::{TcpListener, TcpStream},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{fs::PermissionsExt, process::CommandExt},
        },
        path::Path,
        process::{Command, Stdio},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::Duration,
    };

    pub fn run(args: Vec<std::ffi::OsString>) {
        if args.first().is_some_and(|arg| arg == "--probe") {
            probe(args);
            return;
        }
        #[cfg(target_os = "macos")]
        if args.first().is_some_and(|arg| arg == "--owner-probe") {
            let launch: Launch = serde_json::from_reader(io::stdin()).unwrap();
            let result = tool::run(
                &launch,
                b"private document",
                &AtomicBool::new(false),
                Duration::from_secs(10),
            );
            std::process::exit(i32::from(result.is_err()));
        }
        let directory = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(directory.path()).unwrap();
        let snapshot = root.join("snapshot \" café 東京");
        fs::create_dir(&snapshot).unwrap();
        fs::set_permissions(&snapshot, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(snapshot.join("input.txt"), "read-only café / 東京").unwrap();
        let image = root.join("tool-image");
        fs::copy(std::env::current_exe().unwrap(), &image).unwrap();
        fs::set_permissions(&image, fs::Permissions::from_mode(0o500)).unwrap();
        let secret = root.join("private.txt");
        fs::write(&secret, "private fixture").unwrap();
        std::os::unix::fs::symlink(&secret, snapshot.join("escape")).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let baseline = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        drop(listener.accept().unwrap());
        drop(baseline);
        listener.set_nonblocking(true).unwrap();
        // An inherited ambient descriptor must disappear before native exec.
        let file = fs::File::open(&secret).unwrap();
        let inherited = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, 256) };
        assert!(inherited >= 256);
        let inherited = unsafe { fs::File::from_raw_fd(inherited) };
        let launcher = std::env::current_exe().unwrap();
        let launch = Launch {
            executable: image,
            snapshot: snapshot.clone(),
            arguments: vec![
                "--probe".into(),
                "confined".into(),
                secret.to_str().unwrap().into(),
                listener.local_addr().unwrap().to_string(),
                inherited.as_raw_fd().to_string(),
            ],
        };
        for mode in ["confined", "reexec"] {
            let mut launch = launch.clone();
            launch.arguments[1] = mode.into();
            let result = tool::run(
                &launch,
                "document café / 東京".as_bytes(),
                &AtomicBool::new(false),
                Duration::from_secs(5),
            )
            .unwrap();
            assert_eq!(result.stdout, "document café / 東京".as_bytes());
            assert!(result.stderr.is_empty());
        }
        assert_eq!(fs::read_to_string(&secret).unwrap(), "private fixture");
        assert_eq!(
            fs::read_to_string(snapshot.join("input.txt")).unwrap(),
            "read-only café / 東京"
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        for mode in [
            "stdout-flood",
            "stderr-flood",
            "nonzero",
            "memory",
            "threads",
            "spin",
        ] {
            let mut launch = launch.clone();
            launch.arguments[1] = mode.into();
            let result = tool::run(
                &launch,
                &[0; tool::INPUT_BYTES],
                &AtomicBool::new(false),
                Duration::from_secs(5),
            );
            let error = result.unwrap_err();
            match mode {
                "stdout-flood" | "stderr-flood" => {
                    assert_eq!(error.to_string(), "Tool output limit exceeded.")
                }
                #[cfg(target_os = "macos")]
                "memory" => assert_eq!(error.to_string(), "Tool memory limit exceeded."),
                #[cfg(target_os = "macos")]
                "threads" => assert_eq!(error.to_string(), "Tool thread limit exceeded."),
                "spin" => assert_ne!(error.kind(), io::ErrorKind::TimedOut),
                _ => {}
            }
            assert_reaped();
        }
        let bytes = vec![b'x'; tool::INPUT_BYTES];
        assert_eq!(
            tool::run(
                &launch,
                &bytes,
                &AtomicBool::new(false),
                Duration::from_secs(5)
            )
            .unwrap()
            .stdout,
            bytes
        );
        let mut waiting = launch.clone();
        waiting.arguments[1] = "wait".into();
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&cancel);
        let trigger = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            signal.store(true, Ordering::Release);
        });
        assert_eq!(
            tool::run(
                &waiting,
                b"private document",
                &cancel,
                Duration::from_secs(5)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
        trigger.join().unwrap();
        assert_reaped();
        assert_eq!(
            tool::run(
                &waiting,
                b"private document",
                &AtomicBool::new(false),
                Duration::from_millis(100)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::TimedOut
        );
        assert_reaped();
        #[cfg(target_os = "macos")]
        for kill_guardian in [false, true] {
            guardian_lifecycle(&waiting, kill_guardian);
        }
        // Unknown and duplicate metadata fail before readiness.
        for header in [
            b"{}".as_slice(),
            b"{\"executable\":0,\"unexpected\":true}",
            b"{\"executable\":0,\"executable\":0}",
        ] {
            let mut child = Command::new(&launcher)
                .arg("--hedit-tool-worker")
                .env_clear()
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut input = child.stdin.take().unwrap();
            input
                .write_all(&(header.len() as u32).to_le_bytes())
                .unwrap();
            input.write_all(header).unwrap();
            drop(input);
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while child.try_wait().unwrap().is_none() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            if child.try_wait().unwrap().is_none() {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("invalid metadata did not terminate");
            }
            let result = child.wait_with_output().unwrap();
            assert!(!result.status.success());
            assert!(result.stdout.is_empty());
        }
        println!("Native tool confinement, inherited descriptors, reexec, output, memory, threads, CPU, cancellation and metadata probes passed.");
    }

    #[cfg(target_os = "macos")]
    fn children(pid: u32) -> Vec<u32> {
        let mut children = [0i32; 16];
        let count = unsafe {
            libc::proc_listchildpids(
                pid as i32,
                children.as_mut_ptr().cast(),
                std::mem::size_of_val(&children) as i32,
            )
        };
        assert!(count >= 0 && (count as usize) < children.len());
        children[..count as usize]
            .iter()
            .map(|pid| *pid as u32)
            .collect()
    }
    #[cfg(target_os = "macos")]
    fn image_path(pid: u32) -> Option<std::path::PathBuf> {
        use std::os::unix::ffi::OsStrExt;
        let mut path = [0u8; 4096];
        if unsafe { libc::proc_pidpath(pid as i32, path.as_mut_ptr().cast(), path.len() as u32) }
            <= 0
        {
            return None;
        }
        let length = path.iter().position(|byte| *byte == 0).unwrap();
        Some(Path::new(std::ffi::OsStr::from_bytes(&path[..length])).into())
    }
    #[cfg(target_os = "macos")]
    fn guardian_lifecycle(launch: &Launch, kill_guardian: bool) {
        let mut owner = Command::new(std::env::current_exe().unwrap())
            .arg("--owner-probe")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        owner
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(launch).unwrap())
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let (guardian, native) = loop {
            let ready = children(owner.id()).into_iter().find_map(|native| {
                if image_path(native).as_ref() != Some(&launch.executable) {
                    return None;
                }
                children(native)
                    .into_iter()
                    .next()
                    .map(|guardian| (guardian, native))
            });
            if let Some(ready) = ready {
                break ready;
            }
            if owner.try_wait().unwrap().is_some() || std::time::Instant::now() >= deadline {
                let _ = owner.kill();
                let result = owner.wait_with_output().unwrap();
                panic!(
                    "native owner did not launch the selected image: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        if kill_guardian {
            assert_eq!(unsafe { libc::kill(guardian as i32, libc::SIGKILL) }, 0);
        } else {
            owner.kill().unwrap();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while (unsafe { libc::kill(native as i32, 0) } == 0
            || unsafe { libc::kill(guardian as i32, 0) } == 0
            || owner.try_wait().unwrap().is_none())
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        if owner.try_wait().unwrap().is_none() {
            owner.kill().unwrap();
            owner.wait().unwrap();
            panic!("owner did not handle guardian exit");
        }
        assert_eq!(
            unsafe { libc::kill(native as i32, 0) },
            -1,
            "native tool survived editor/guardian exit"
        );
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
        assert_eq!(
            unsafe { libc::kill(guardian as i32, 0) },
            -1,
            "guardian survived editor/tool exit"
        );
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
        assert_reaped();
    }
    fn assert_reaped() {
        let result = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
        assert_eq!(result, -1, "tool child remained after the call");
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }
    fn denied<T: std::fmt::Debug>(result: io::Result<T>) {
        let error = result.unwrap_err();
        assert!(
            matches!(error.raw_os_error(), Some(libc::EPERM | libc::EACCES)),
            "{error:?}"
        );
    }
    fn probe(args: Vec<std::ffi::OsString>) {
        let mode = args[1].to_str().unwrap();
        match mode {
            "stdout-flood" => loop {
                io::stdout().write_all(&[0; 4096]).unwrap();
            },
            "stderr-flood" => loop {
                io::stderr().write_all(&[0; 4096]).unwrap();
            },
            "nonzero" => std::process::exit(19),
            "spin" => loop {
                std::hint::spin_loop();
            },
            "wait" => loop {
                std::thread::sleep(Duration::from_secs(1));
            },
            "threads" => {
                let mut threads = Vec::new();
                for _ in 0..64 {
                    threads.push(std::thread::spawn(|| {
                        std::thread::sleep(Duration::from_secs(30))
                    }));
                }
                for thread in threads {
                    thread.join().unwrap();
                }
                panic!("thread flood escaped supervision");
            }
            "memory" => {
                let mut blocks = Vec::new();
                loop {
                    let mut block = vec![0u8; 8 * 1024 * 1024];
                    // Dirty every page without making a debug-build byte loop
                    // consume the CPU budget before the memory probe finishes.
                    for (index, page) in block.chunks_mut(4096).enumerate() {
                        page[0] = (index % 251 + 1) as u8;
                    }
                    blocks.push(block);
                    std::hint::black_box(&blocks);
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            _ => {}
        }
        assert!(std::env::vars_os().next().is_none());
        #[cfg(target_os = "linux")]
        {
            let page = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    300 * 1024 * 1024,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                )
            };
            assert_eq!(
                page,
                libc::MAP_FAILED,
                "hard address-space cap was lost across exec"
            );
            assert_eq!(
                io::Error::last_os_error().raw_os_error(),
                Some(libc::ENOMEM)
            );
            assert_eq!(
                unsafe { libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) },
                1
            );
            assert_eq!(unsafe { libc::prctl(libc::PR_GET_SECCOMP, 0, 0, 0, 0) }, 2);
            for flags in [
                0,
                libc::CLONE_VM as u64,
                (libc::CLONE_VM | libc::CLONE_SIGHAND | libc::CLONE_THREAD) as u64 | (1u64 << 32),
            ] {
                let result = unsafe {
                    libc::syscall(libc::SYS_clone, flags, 0usize, 0usize, 0usize, 0usize)
                };
                if result == 0 {
                    unsafe { libc::_exit(33) }
                }
                assert_eq!(result, -1);
                assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM));
            }
            assert_eq!(
                unsafe { libc::fcntl(1, libc::F_SETOWN, libc::getppid()) },
                -1
            );
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM));
        }
        let snapshot = std::env::current_dir().unwrap();
        assert_eq!(
            fs::read_to_string(snapshot.join("input.txt")).unwrap(),
            "read-only café / 東京"
        );
        denied(fs::read(Path::new(&args[2])));
        denied(fs::read(snapshot.join("escape")));
        denied(fs::write(snapshot.join("input.txt"), "changed"));
        denied(fs::write(snapshot.join("created"), "changed"));
        denied(fs::remove_file(snapshot.join("input.txt")));
        denied(fs::rename(
            snapshot.join("input.txt"),
            snapshot.join("moved"),
        ));
        denied(fs::set_permissions(
            snapshot.join("input.txt"),
            fs::Permissions::from_mode(0o777),
        ));
        denied(fs::hard_link(Path::new(&args[2]), snapshot.join("link")));
        let inherited: i32 = args[4].to_str().unwrap().parse().unwrap();
        let mut byte = 0u8;
        assert_eq!(
            unsafe { libc::pread(inherited, (&mut byte as *mut u8).cast(), 1, 0) },
            -1
        );
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
        let address = args[3].to_str().unwrap().parse().unwrap();
        denied(TcpStream::connect_timeout(
            &address,
            Duration::from_millis(200),
        ));
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            unsafe { libc::_exit(31) }
        }
        assert_eq!(pid, -1);
        #[cfg(target_os = "macos")]
        {
            assert_eq!(
                unsafe { libc::setsid() },
                -1,
                "tool escaped the guardian's session"
            );
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM));
            assert_eq!(
                unsafe { libc::setpgid(0, 0) },
                -1,
                "tool escaped the guardian's process group"
            );
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM));
        }
        if let Ok(mut child) = Command::new(std::env::current_exe().unwrap())
            .args(["--probe", "wait"])
            .spawn()
        {
            let _ = child.kill();
            let _ = child.wait();
            panic!("native spawn escaped confinement");
        }
        // The exec allowlist is the selected image, not all readable system code.
        denied(Err::<(), _>(Command::new("/usr/bin/true").exec()));
        if mode == "reexec" {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command.arg("--probe").arg("confined").args(&args[2..]);
            panic!("same-image exec failed: {}", command.exec());
        }
        let mut document = Vec::new();
        io::stdin().read_to_end(&mut document).unwrap();
        io::stdout().write_all(&document).unwrap();
        assert!(std::thread::spawn(|| 7).join().unwrap() == 7);
    }
}
