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
#[ignore = "manual startup diagnosis with a trusted --version-only executable"]
fn bootstrap_loader_diagnostics() {
    let executable = std::path::PathBuf::from(
        std::env::var_os("HEDIT_LUA_TEST_WORKER").expect("set the built worker path"),
    );
    let mut strict_succeeded = false;
    // These variants never receive source or selections. The sole command is
    // the trusted worker's --version path, before any Lua or request handling.
    for (label, container, mitigation, child_policy, inherit_env) in [
        ("strict", true, MITIGATIONS, true, false),
        ("without-mitigations", true, 0, true, false),
        ("dynamic-code-only", true, 1u64 << 36, true, false),
        ("extension-points-only", true, 1u64 << 32, true, false),
        ("without-child-policy", true, MITIGATIONS, false, false),
        ("inherited-environment", true, MITIGATIONS, true, true),
        ("without-container", false, MITIGATIONS, true, false),
    ] {
        let profile = Profile::new().unwrap();
        let image = tempfile::tempdir().unwrap();
        image_permissions(image.path(), profile.sid).unwrap();
        let copied = image.path().join("hedit-worker.exe");
        std::fs::copy(&executable, &copied).unwrap();
        image_permissions(&copied, profile.sid).unwrap();
        let job = job().unwrap();
        let (input, input_writer) = pipe().unwrap();
        let (output_reader, output) = pipe().unwrap();
        drop(input_writer);
        check(unsafe {
            SetHandleInformation(output_reader.as_raw_handle(), HANDLE_FLAG_INHERIT, 0)
        })
        .unwrap();
        let security = SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: 1,
        };
        let stderr = unsafe {
            owned(CreateFileW(
                wide("NUL").as_ptr(),
                GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                &security,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                ptr::null_mut(),
            ))
            .unwrap()
        };
        let handles = [
            input.as_raw_handle(),
            output.as_raw_handle(),
            stderr.as_raw_handle(),
        ];
        let capabilities = SECURITY_CAPABILITIES {
            AppContainerSid: profile.sid,
            Capabilities: ptr::null_mut(),
            CapabilityCount: 0,
            Reserved: 0,
        };
        let jobs = [job.as_raw_handle()];
        let policy = 1u32;
        let mut attributes = Attributes::new(5).unwrap();
        attributes
            .set(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &handles)
            .unwrap();
        attributes
            .set(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs)
            .unwrap();
        if container {
            attributes
                .set(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, &capabilities)
                .unwrap();
        }
        if mitigation != 0 {
            attributes
                .set(PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY, &mitigation)
                .unwrap();
        }
        if child_policy {
            attributes
                .set(PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY, &policy)
                .unwrap();
        }
        let startup = STARTUPINFOEXW {
            StartupInfo: STARTUPINFOW {
                cb: mem::size_of::<STARTUPINFOEXW>() as u32,
                dwFlags: STARTF_USESTDHANDLES,
                hStdInput: input.as_raw_handle(),
                hStdOutput: output.as_raw_handle(),
                hStdError: stderr.as_raw_handle(),
                ..Default::default()
            },
            lpAttributeList: attributes.pointer(),
        };
        let mut command = wide(format!("\"{}\" --version", copied.display()));
        let environment = environment().unwrap();
        let current_directory = working_directory().unwrap();
        let mut information = PROCESS_INFORMATION::default();
        let created = unsafe {
            CreateProcessW(
                wide(&copied).as_ptr(),
                command.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                1,
                CREATE_SUSPENDED
                    | CREATE_NO_WINDOW
                    | CREATE_UNICODE_ENVIRONMENT
                    | EXTENDED_STARTUPINFO_PRESENT,
                if inherit_env {
                    ptr::null()
                } else {
                    environment.as_ptr().cast()
                },
                current_directory.as_ptr(),
                &startup.StartupInfo,
                &mut information,
            )
        };
        if created == 0 {
            println!(
                "BOOTSTRAP {label}: CreateProcessW {:?}",
                io::Error::last_os_error()
            );
            continue;
        }
        let process = unsafe { owned(information.hProcess).unwrap() };
        let thread = unsafe { owned(information.hThread).unwrap() };
        assert_ne!(unsafe { ResumeThread(thread.as_raw_handle()) }, u32::MAX);
        drop(input);
        drop(output);
        drop(stderr);
        let wait = unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) };
        let mut code = 0;
        check(unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) }).unwrap();
        unsafe {
            TerminateJobObject(job.as_raw_handle(), 1);
            WaitForSingleObject(process.as_raw_handle(), 1000);
        }
        let mut text = String::new();
        File::from(output_reader)
            .take(4096)
            .read_to_string(&mut text)
            .unwrap();
        println!("BOOTSTRAP {label}: wait={wait:#x} exit={code:#010x} output={text:?}");
        if label == "strict" {
            strict_succeeded =
                wait == WAIT_OBJECT_0 && code == 0 && text.starts_with("hedit-plugin-worker ");
        }
    }
    assert!(
        strict_succeeded,
        "strict startup must succeed before enabling Lua"
    );
}
