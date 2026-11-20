use super::*;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::windows::io::AsRawHandle,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Memory::*;

pub(crate) const PROBE_ARGS: &[&str] = &[
    "--exact",
    "plugin_sandbox::windows::tests::native_probe",
    "--ignored",
    "--nocapture",
];

#[test]
#[ignore = "only run inside a parent-created AppContainer and job"]
fn native_probe() {
    if let Err(error) = enter() {
        println!("native confinement initialization failed: {error:?}");
        unsafe { ExitProcess(17) }
    }
    assert!(std::env::var_os("PATH").is_none());
    assert!(std::env::var_os("HEDIT_LUA_TEST_WORKER").is_none());
    assert!(std::env::var_os("SystemRoot").is_some());
    assert!(std::env::var_os("LOCALAPPDATA").is_some());
    let mut request = Vec::new();
    io::stdin().read_to_end(&mut request).unwrap();
    let request: serde_json::Value = serde_json::from_slice(&request).unwrap();
    match request["action"].as_str().unwrap() {
        "wait" => loop {
            std::thread::sleep(Duration::from_millis(50));
        },
        "spin" => {
            io::stdout().write_all(b"native spin ready\n").unwrap();
            loop {
                std::hint::spin_loop();
            }
        }
        "confinement" => {
            let path = Path::new(request["path"].as_str().unwrap());
            assert_eq!(
                std::fs::read(path).unwrap_err().kind(),
                io::ErrorKind::PermissionDenied
            );
            assert_eq!(
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(path)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::PermissionDenied
            );
            let inherited = request["inherited"].as_u64().unwrap() as usize as HANDLE;
            let mut flags = 0;
            assert_eq!(unsafe { GetHandleInformation(inherited, &mut flags) }, 0);
            let address = request["address"].as_str().unwrap().parse().unwrap();
            let error = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap_err();
            assert!(
                error.kind() == io::ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(10013),
                "{error:?}"
            );
            let page = unsafe {
                VirtualAlloc(ptr::null(), 4096, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE)
            };
            assert!(!page.is_null());
            let mut old = 0;
            assert_eq!(
                unsafe { VirtualProtect(page, 4096, PAGE_EXECUTE_READ, &mut old) },
                0
            );
            unsafe {
                VirtualFree(page, 0, MEM_RELEASE);
            }
            let excessive = unsafe {
                VirtualAlloc(
                    ptr::null(),
                    300 * 1024 * 1024,
                    MEM_RESERVE | MEM_COMMIT,
                    PAGE_READWRITE,
                )
            };
            assert!(
                excessive.is_null(),
                "job must deny native allocation beyond 256 MiB"
            );
            assert!(std::process::Command::new(std::env::current_exe().unwrap())
                .arg("--list")
                .spawn()
                .is_err());
        }
        _ => panic!("unknown native probe"),
    }
    io::stdout().write_all(b"native probe passed\n").unwrap();
    unsafe { ExitProcess(0) }
}

#[test]
fn native_file_network_handles_memory_code_and_children_are_confined() {
    let directory = tempfile::tempdir().unwrap();
    let secret = directory.path().join("private.txt");
    std::fs::write(&secret, "private fixture").unwrap();
    // Put the sensitive handle above the ordinary child startup handles so a
    // subsequently opened handle cannot accidentally reuse its numeric value.
    let events: Vec<_> = (0..512)
        .map(|_| unsafe { owned(CreateEventW(ptr::null(), 0, 0, ptr::null())).unwrap() })
        .collect();
    let file = File::open(&secret).unwrap();
    check(unsafe {
        SetHandleInformation(
            file.as_raw_handle(),
            HANDLE_FLAG_INHERIT,
            HANDLE_FLAG_INHERIT,
        )
    })
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let request = serde_json::to_vec(&serde_json::json!({"action":"confinement", "path":secret, "inherited":file.as_raw_handle() as usize, "address":listener.local_addr().unwrap().to_string()})).unwrap();
    let mut child = Worker::spawn(&std::env::current_exe().unwrap(), PROBE_ARGS).unwrap();
    let mut output = child.output.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output.read_to_end(&mut bytes).unwrap();
        bytes
    });
    child.input.take().unwrap().write_all(&request).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !child.finished(10).unwrap() && Instant::now() < deadline {}
    let completed = child.finished(0).unwrap();
    let success = child.success().unwrap();
    let mut exit_code = 0;
    check(unsafe { GetExitCodeProcess(child.process.as_raw_handle(), &mut exit_code) }).unwrap();
    child.terminate();
    let bytes = reader.join().unwrap();
    assert!(
        completed && success,
        "probe exited {exit_code:#010x}; output: {}",
        String::from_utf8_lossy(&bytes)
    );
    assert!(bytes.ends_with(b"native probe passed\n"));
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(std::fs::read_to_string(secret).unwrap(), "private fixture");
    drop(events);
}

#[test]
fn dropping_the_owner_kills_and_reaps_the_job() {
    let mut child = Worker::spawn(&std::env::current_exe().unwrap(), PROBE_ARGS).unwrap();
    let image = child._image.path().to_path_buf();
    let profile = child._profile.name.clone();
    let mut observed = ptr::null_mut();
    check(unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            child.process.as_raw_handle(),
            GetCurrentProcess(),
            &mut observed,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    })
    .unwrap();
    let observed = unsafe { owned(observed).unwrap() };
    child
        .input
        .take()
        .unwrap()
        .write_all(br#"{"action":"wait"}"#)
        .unwrap();
    drop(child);
    assert_eq!(
        unsafe { WaitForSingleObject(observed.as_raw_handle(), 1000) },
        WAIT_OBJECT_0
    );
    assert!(!image.exists(), "worker image must be removed after exit");
    let mut sid = ptr::null_mut();
    let status = unsafe {
        CreateAppContainerProfile(
            profile.as_ptr(),
            profile.as_ptr(),
            profile.as_ptr(),
            ptr::null(),
            0,
            &mut sid,
        )
    };
    if status >= 0 {
        drop(Profile { name: profile, sid });
    }
    assert_eq!(status, 0, "the exited worker's profile must be reusable");
}

#[test]
fn job_cpu_limit_terminates_a_native_busy_loop() {
    let mut child = Worker::spawn(&std::env::current_exe().unwrap(), PROBE_ARGS).unwrap();
    child
        .input
        .take()
        .unwrap()
        .write_all(br#"{"action":"spin"}"#)
        .unwrap();
    assert!(
        child.finished(10_000).unwrap(),
        "job CPU limit did not terminate the worker"
    );
    assert!(!child.success().unwrap());
    let mut output = String::new();
    child
        .output
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    assert!(
        output.contains("native spin ready"),
        "the probe must enter the native busy loop: {output}"
    );
}

#[test]
fn ordinary_editor_process_cannot_enter_the_worker() {
    assert!(enter().is_err());
}
