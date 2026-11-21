use hedit_plugin::tool::{self, Launch};
use std::{
    fs,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::*,
    System::{Memory::*, Threading::*},
};

pub fn run(args: Vec<std::ffi::OsString>) {
    if args.first().is_some_and(|arg| arg == "--probe") {
        probe(args);
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(directory.path()).unwrap();
    let snapshot = root.join("snapshot café 東京");
    fs::create_dir(&snapshot).unwrap();
    fs::write(snapshot.join("input.txt"), "read-only café / 東京").unwrap();
    let image = root.join("tool-image.exe");
    fs::copy(std::env::current_exe().unwrap(), &image).unwrap();
    let secret = root.join("private.txt");
    fs::write(&secret, "private fixture").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let baseline = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    drop(listener.accept().unwrap());
    drop(baseline);
    listener.set_nonblocking(true).unwrap();
    let launch = Launch {
        executable: image,
        snapshot: snapshot.clone(),
        arguments: vec![
            "--probe".into(),
            "confined".into(),
            secret.to_str().unwrap().into(),
            listener.local_addr().unwrap().to_string(),
            "".into(),
            "two words".into(),
            "quote \" here".into(),
            "trailing\\".into(),
            "slash\\\"quote".into(),
            "東京".into(),
        ],
    };
    let result = tool::run(
        &launch,
        "document café / 東京".as_bytes(),
        &AtomicBool::new(false),
        Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(result.stdout, "document café / 東京".as_bytes());
    assert!(result.stderr.is_empty());
    let bytes = vec![b'x'; tool::INPUT_BYTES];
    assert_eq!(
        tool::run(
            &launch,
            &bytes,
            &AtomicBool::new(false),
            Duration::from_secs(10)
        )
        .unwrap()
        .stdout,
        bytes
    );
    assert_eq!(fs::read_to_string(&secret).unwrap(), "private fixture");
    assert_eq!(
        fs::read_to_string(snapshot.join("input.txt")).unwrap(),
        "read-only café / 東京"
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    for mode in ["stdout-flood", "stderr-flood", "nonzero", "threads", "spin"] {
        let mut launch = launch.clone();
        launch.arguments[1] = mode.into();
        let error = tool::run(
            &launch,
            &bytes,
            &AtomicBool::new(false),
            Duration::from_secs(10),
        )
        .unwrap_err();
        match mode {
            "stdout-flood" | "stderr-flood" => {
                assert_eq!(error.to_string(), "Tool output limit exceeded.")
            }
            "threads" => assert_eq!(error.to_string(), "Tool thread limit exceeded."),
            "spin" => assert_ne!(error.kind(), io::ErrorKind::TimedOut),
            _ => {}
        }
    }
    let mut waiting = launch.clone();
    waiting.arguments[1] = "wait".into();
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&cancel);
    let trigger = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        signal.store(true, Ordering::Release);
    });
    assert_eq!(
        tool::run(&waiting, &bytes, &cancel, Duration::from_secs(10))
            .unwrap_err()
            .kind(),
        io::ErrorKind::Interrupted
    );
    trigger.join().unwrap();
    assert_eq!(
        tool::run(
            &waiting,
            &bytes,
            &AtomicBool::new(false),
            Duration::from_millis(100)
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::TimedOut
    );
    println!("Windows native token, snapshot, argv, overlapped transport, output, threads, CPU and cancellation probes passed.");
}

fn information(class: TOKEN_INFORMATION_CLASS) -> Vec<usize> {
    let mut token = std::ptr::null_mut();
    assert_ne!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) },
        0
    );
    if class == TokenIsAppContainer || class == TokenMandatoryPolicy {
        let mut value = 0u32;
        let mut count = std::mem::size_of_val(&value) as u32;
        assert_ne!(
            unsafe {
                GetTokenInformation(
                    token,
                    class,
                    (&mut value as *mut u32).cast(),
                    std::mem::size_of_val(&value) as u32,
                    &mut count,
                )
            },
            0
        );
        assert_eq!(count, std::mem::size_of_val(&value) as u32);
        unsafe { CloseHandle(token) };
        return vec![value as usize];
    }
    let mut count = 0;
    unsafe { GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut count) };
    assert!(count > 0 && count <= 65536);
    let mut bytes = vec![0usize; (count as usize).div_ceil(std::mem::size_of::<usize>())];
    assert_ne!(
        unsafe { GetTokenInformation(token, class, bytes.as_mut_ptr().cast(), count, &mut count) },
        0
    );
    unsafe { CloseHandle(token) };
    bytes
}
fn probe(args: Vec<std::ffi::OsString>) {
    match args[1].to_str().unwrap() {
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
            let threads: Vec<_> = (0..64)
                .map(|_| std::thread::spawn(|| std::thread::sleep(Duration::from_secs(30))))
                .collect();
            for thread in threads {
                thread.join().unwrap();
            }
            panic!("thread flood survived supervision");
        }
        _ => {}
    }
    assert_eq!(information(TokenIsAppContainer)[0], 1);
    assert_ne!(
        information(TokenMandatoryPolicy)[0] & TOKEN_MANDATORY_POLICY_NO_WRITE_UP as usize,
        0
    );
    let label = information(TokenIntegrityLevel);
    let sid = unsafe { &*label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() }
        .Label
        .Sid;
    assert_eq!(unsafe { *GetSidSubAuthorityCount(sid) }, 1);
    assert_eq!(unsafe { *GetSidSubAuthority(sid, 0) }, 0);
    assert!(std::env::var_os("PATH").is_none());
    assert!(std::env::var_os("SystemRoot").is_some());
    assert_eq!(
        args[4..],
        [
            "",
            "two words",
            "quote \" here",
            "trailing\\",
            "slash\\\"quote",
            "東京"
        ]
    );
    let snapshot = std::env::current_dir().unwrap();
    assert_eq!(
        fs::read_to_string(snapshot.join("input.txt")).unwrap(),
        "read-only café / 東京"
    );
    assert_eq!(
        fs::read(Path::new(&args[2])).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        fs::write(snapshot.join("input.txt"), "changed")
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        fs::write(snapshot.join("created"), "changed")
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    let address = args[3].to_str().unwrap().parse().unwrap();
    let error = TcpStream::connect_timeout(&address, Duration::from_millis(200)).unwrap_err();
    assert!(
        matches!(
            error.kind(),
            io::ErrorKind::PermissionDenied | io::ErrorKind::TimedOut
        ) || error.raw_os_error() == Some(10013)
    );
    let allocation = unsafe {
        VirtualAlloc(
            std::ptr::null(),
            300 * 1024 * 1024,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        )
    };
    assert!(allocation.is_null(), "job memory cap was lost");
    assert!(std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--probe", "wait"])
        .spawn()
        .is_err());
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input).unwrap();
    io::stdout().write_all(&input).unwrap();
}
