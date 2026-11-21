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
    "sandbox::windows::tests::native_probe",
    "--ignored",
    "--nocapture",
];

const PIPE_TOKEN_PROBE_ARGS: &[&str] = &[
    "--exact",
    "sandbox::windows::tests::native_pipe_token_probe",
    "--ignored",
    "--nocapture",
];

#[test]
#[ignore = "only run inside a parent-created AppContainer and job"]
fn native_pipe_token_probe() {
    std::panic::set_hook(Box::new(|panic| {
        println!("native probe assertion: {panic}")
    }));
    assert!(enter().is_ok());
    // Unlike the ordinary EOF-framed probe, the parent keeps its client handle
    // live until this process exits. Reading first selects that client's token.
    let mut input = io::stdin().lock();
    let mut length = [0u8; 4];
    input.read_exact(&mut length).unwrap();
    let length = u32::from_le_bytes(length) as usize;
    assert!(length <= 4096);
    let mut request = vec![0; length];
    input.read_exact(&mut request).unwrap();
    let path: PathBuf = serde_json::from_slice(&request).unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    let impersonated = unsafe { ImpersonateNamedPipeClient(GetStdHandle(STD_INPUT_HANDLE)) } != 0;
    if impersonated {
        let result = std::fs::read(&path);
        // Restore the restricted process token before evaluating assertions.
        check(unsafe { RevertToSelf() }).unwrap();
        assert!(result.is_err(), "stdin exposed the editor's file authority");
    }
    assert_eq!(
        std::fs::read(&path).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    io::stdout()
        .write_all(b"native pipe token probe passed\n")
        .unwrap();
    unsafe { ExitProcess(0) }
}

#[test]
fn stdin_cannot_impersonate_the_live_editor_client() {
    let directory = tempfile::tempdir().unwrap();
    let secret = directory.path().join("private.txt");
    std::fs::write(&secret, "private fixture").unwrap();
    let request = serde_json::to_vec(&secret).unwrap();
    let (exit_code, bytes) = run_probe_inner(PIPE_TOKEN_PROBE_ARGS, &request, true);
    assert_eq!(
        exit_code,
        0,
        "probe output: {}",
        String::from_utf8_lossy(&bytes)
    );
    assert!(bytes.ends_with(b"native pipe token probe passed\n"));
    assert_eq!(std::fs::read_to_string(secret).unwrap(), "private fixture");
}

pub(crate) const UNREAD_PROBE_ARGS: &[&str] = &[
    "--exact",
    "sandbox::windows::tests::native_unresponsive_probe",
    "--ignored",
    "--nocapture",
];

#[test]
#[ignore = "only run inside a parent-created AppContainer and job"]
fn native_unresponsive_probe() {
    assert!(enter().is_ok());
    io::stdout().write_all(b"\0[]").unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "only run inside a parent-created AppContainer and job"]
fn native_probe() {
    // Production stderr is deliberately NUL; surface test assertions through
    // the bounded output pipe without changing the worker's handle policy.
    std::panic::set_hook(Box::new(|panic| {
        println!("native probe assertion: {panic}")
    }));
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
        "reply" => {
            io::stdout()
                .write_all(&[request["status"].as_u64().unwrap() as u8])
                .unwrap();
            io::stdout()
                .write_all(request["body"].as_str().unwrap().as_bytes())
                .unwrap();
            unsafe { ExitProcess(request["exit"].as_u64().unwrap() as u32) }
        }
        "flood" => {
            while io::stdout().write_all(&[0; 4096]).is_ok() {}
            unsafe { ExitProcess(0) }
        }
        "wait" => loop {
            std::thread::sleep(Duration::from_millis(50));
        },
        "spin" => {
            io::stdout().write_all(b"native spin ready\n").unwrap();
            loop {
                std::hint::spin_loop();
            }
        }
        "inherited-handle" => {
            let inherited = request["inherited"].as_u64().unwrap() as usize as HANDLE;
            // AppContainer strict-handle validation can terminate the process
            // instead of returning ERROR_INVALID_HANDLE. The parent accepts
            // that precise exception only after this marker, in its own probe.
            io::stdout()
                .write_all(b"checking inherited handle\n")
                .unwrap();
            io::stdout().flush().unwrap();
            let mut flags = 0;
            assert_eq!(unsafe { GetHandleInformation(inherited, &mut flags) }, 0);
        }
        "native-file-write" => {
            let allowed = request["allowed"].as_bool().unwrap();
            let mode = if allowed {
                native::Mode::ToolLow
            } else {
                native::Mode::ToolProbe
            };
            native::require_token(unsafe { GetCurrentProcess() }, mode).unwrap();
            if !allowed {
                assert!(
                    native::lower_integrity(unsafe { GetCurrentProcess() }, native::Mode::ToolLow)
                        .is_err(),
                    "native tool raised its integrity level"
                );
                let mut token_handle = ptr::null_mut();
                if unsafe {
                    OpenProcessToken(
                        GetCurrentProcess(),
                        TOKEN_QUERY | TOKEN_ADJUST_DEFAULT,
                        &mut token_handle,
                    )
                } != 0
                {
                    let token = unsafe { owned(token_handle).unwrap() };
                    let policy = TOKEN_MANDATORY_POLICY {
                        Policy: TOKEN_MANDATORY_POLICY_OFF,
                    };
                    assert_eq!(
                        unsafe {
                            SetTokenInformation(
                                token.as_raw_handle(),
                                TokenMandatoryPolicy,
                                (&policy as *const TOKEN_MANDATORY_POLICY).cast(),
                                mem::size_of_val(&policy) as u32,
                            )
                        },
                        0,
                        "native tool disabled mandatory integrity policy"
                    );
                }
                native::require_token(unsafe { GetCurrentProcess() }, mode).unwrap();
            }
            let result = std::fs::write(request["path"].as_str().unwrap(), "child fixture");
            if allowed {
                result.unwrap();
            } else {
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
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
            let address = request["address"].as_str().unwrap().parse().unwrap();
            let error = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap_err();
            assert!(
                matches!(
                    error.kind(),
                    io::ErrorKind::PermissionDenied | io::ErrorKind::TimedOut
                ) || error.raw_os_error() == Some(10013),
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
fn native_file_network_memory_code_and_children_are_confined() {
    let directory = tempfile::tempdir().unwrap();
    let secret = directory.path().join("private.txt");
    std::fs::write(&secret, "private fixture").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    // Establish that the listener is reachable before checking that the
    // capability-free worker cannot connect (Windows may drop or reject it).
    let baseline = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    drop(listener.accept().unwrap());
    drop(baseline);
    listener.set_nonblocking(true).unwrap();
    let request = serde_json::to_vec(&serde_json::json!({"action":"confinement", "path":secret, "address":listener.local_addr().unwrap().to_string()})).unwrap();
    let (exit_code, bytes) = run_probe(&request);
    assert_eq!(
        exit_code,
        0,
        "probe output: {}",
        String::from_utf8_lossy(&bytes)
    );
    assert!(bytes.ends_with(b"native probe passed\n"));
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(std::fs::read_to_string(secret).unwrap(), "private fixture");
}

fn run_probe(request: &[u8]) -> (u32, Vec<u8>) {
    run_probe_inner(PROBE_ARGS, request, false)
}

fn run_probe_inner(args: &[&str], request: &[u8], keep_client: bool) -> (u32, Vec<u8>) {
    let child = Worker::spawn(&std::env::current_exe().unwrap(), args).unwrap();
    collect_probe(child, request, keep_client)
}
fn collect_probe(mut child: Worker, request: &[u8], keep_client: bool) -> (u32, Vec<u8>) {
    let mut output = child.output.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let mut input = child.input.take().unwrap();
    if keep_client {
        input
            .write_all(&(request.len() as u32).to_le_bytes())
            .unwrap();
    }
    input.write_all(request).unwrap();
    let input = keep_client.then_some(input);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !child.finished(10).unwrap() && Instant::now() < deadline {}
    assert!(
        child.finished(0).unwrap(),
        "native probe exceeded its deadline"
    );
    let mut exit_code = 0;
    check(unsafe { GetExitCodeProcess(child.process.as_raw_handle(), &mut exit_code) }).unwrap();
    child.terminate();
    drop(input);
    let bytes = reader.join().unwrap();
    (exit_code, bytes)
}

#[test]
fn inherited_private_handle_is_unavailable() {
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
    let request = serde_json::to_vec(&serde_json::json!({
        "action": "inherited-handle", "inherited": file.as_raw_handle() as usize
    }))
    .unwrap();
    let (exit_code, bytes) = run_probe(&request);
    assert!(bytes
        .windows(b"checking inherited handle\n".len())
        .any(|part| part == b"checking inherited handle\n"));
    assert!(
        exit_code == 0 || exit_code == 0xc0000008,
        "unexpected exit {exit_code:#010x}: {}",
        String::from_utf8_lossy(&bytes)
    );
    if exit_code == 0 {
        assert!(bytes.ends_with(b"native probe passed\n"));
    }
    drop(events);
}

#[test]
fn dropping_the_owner_kills_and_reaps_the_job() {
    let mut child = Worker::spawn(&std::env::current_exe().unwrap(), PROBE_ARGS).unwrap();
    let image = child._image.as_ref().unwrap().path().to_path_buf();
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
    let api = ProfileApi::new().unwrap();
    let mut sid = ptr::null_mut();
    let status = unsafe {
        (api.create)(
            profile.as_ptr(),
            profile.as_ptr(),
            profile.as_ptr(),
            ptr::null(),
            0,
            &mut sid,
        )
    };
    if status >= 0 {
        drop(Profile {
            api,
            name: profile,
            sid,
        });
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

#[test]
fn confined_headless_worker_starts_without_a_console() {
    let executable = worker_executable().unwrap();
    let mut child = Worker::spawn(&executable, &["--version"]).unwrap();
    drop(child.input.take());
    assert!(child.finished(5000).unwrap());
    assert!(child.success().unwrap());
    let mut output = String::new();
    child
        .output
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    assert!(output.starts_with("hedit-plugin-worker "));
}

// The LPAC low-integrity control must be able to write the exact same kind of
// low-labelled, explicitly granted object that the untrusted token cannot write.
// This distinguishes the additional write restriction from a missing package
// DACL grant; AppContainer grants can override ordinary integrity expectations.
#[test]
fn native_tool_integrity_cannot_be_raised_or_disabled() {
    for (mode, allowed) in [
        (native::Mode::ToolLow, true),
        (native::Mode::ToolProbe, false),
    ] {
        let profile = Profile::new("tool-probe").unwrap();
        let image = tempfile::tempdir().unwrap();
        image_permissions(image.path(), profile.sid).unwrap();
        let copied = image.path().join("probe.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &copied).unwrap();
        image_permissions(&copied, profile.sid).unwrap();
        let fixture = tempfile::tempdir().unwrap();
        image_permissions(fixture.path(), profile.sid).unwrap();
        let path = fixture.path().join("low-write-control.txt");
        std::fs::write(&path, "parent fixture").unwrap();
        let acl = private_acl_rights(Some((profile.sid, FILE_ALL_ACCESS))).unwrap();
        let mut descriptor = ptr::null_mut();
        check(unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide("S:(ML;;NW;;;LW)").as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            )
        })
        .unwrap();
        let mut present = 0;
        let mut defaulted = 0;
        let mut sacl = ptr::null_mut();
        check(unsafe {
            GetSecurityDescriptorSacl(descriptor, &mut present, &mut sacl, &mut defaulted)
        })
        .unwrap();
        assert_ne!(present, 0);
        let status = unsafe {
            SetNamedSecurityInfoW(
                wide(&path).as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION
                    | PROTECTED_DACL_SECURITY_INFORMATION
                    | LABEL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl.0,
                sacl,
            )
        };
        unsafe { LocalFree(descriptor) };
        assert_eq!(status, 0);
        let child = Worker::launch(
            &copied,
            PROBE_ARGS,
            working_directory().unwrap(),
            profile,
            Some(image),
            mode,
        )
        .unwrap();
        let request = serde_json::to_vec(
            &serde_json::json!({"action":"native-file-write", "path":path, "allowed":allowed}),
        )
        .unwrap();
        let (exit, bytes) = collect_probe(child, &request, false);
        assert_eq!(
            exit,
            0,
            "native policy probe output: {}",
            String::from_utf8_lossy(&bytes)
        );
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            contents,
            if allowed {
                "child fixture"
            } else {
                "parent fixture"
            }
        );
    }
}

#[test]
fn ordinary_appcontainer_does_not_pass_native_lpac_verification() {
    let executable = worker_executable().unwrap();
    let child = Worker::spawn(&executable, &[]).unwrap();
    assert!(native::require_lpac(child.process.as_raw_handle()).is_err());
}
