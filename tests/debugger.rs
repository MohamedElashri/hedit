use hedit::debugger::{decode_stream, quote, DebugSession};
#[cfg(target_os = "linux")]
use hedit::debugger::{State, OUTPUT_LIMIT};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};
use std::{ffi::OsString, path::Path};

#[test]
fn mi_console_decodes_c_escapes_and_octal_utf8() {
    assert_eq!(
        decode_stream(r#""line\n\t\"quoted\" \\ \316\273""#).unwrap(),
        "line\n\t\"quoted\" \\ λ"
    );
    assert!(decode_stream("unquoted").is_none());
    assert_eq!(quote("a\"b\\c\nλ"), "\"a\\\"b\\\\c\\nλ\"");
}

#[test]
fn cli_keeps_literal_paths_and_parses_locations() {
    let opts = hedit::cli::Options::parse(
        [
            "--session-dir",
            "/tmp/session",
            "--new",
            "-",
            "--",
            "-literal",
        ]
        .map(OsString::from),
    )
    .unwrap();
    assert!(opts.stdin && opts.new_buffer);
    assert_eq!(opts.files, [Path::new("-literal")]);
    let (path, line, col) = hedit::cli::location(Path::new("kernel.cu:42:9"));
    assert_eq!(path, Path::new("kernel.cu"));
    assert_eq!((line, col), (42, 9));
    assert!(hedit::cli::Options::parse([OsString::from("--typo")]).is_err());
}

#[test]
fn cuda_highlighting_preserves_every_byte() {
    let source = "__global__ void f(float* α) { // comment\n α[threadIdx.x] = 1; /* end */ }\n";
    let job = hedit::syntax::highlight(source, 14.0, 800.0, true);
    assert_eq!(job.text, source);
    assert!(job.sections.len() > 10);
}

#[cfg(target_os = "linux")]
#[test]
fn absent_tools_report_actionable_errors() {
    let result = DebugSession::launch("/no/such/cuda-gdb", Path::new("Cargo.toml"), &[], None);
    assert!(result.err().unwrap().contains("Install the NVIDIA tool"));
    assert!(DebugSession::sanitizer(Path::new("Cargo.toml"), &[], "unknown").is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn real_process_transport_handles_commands_state_and_bounded_output() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let fake = directory.path().join("fake-cuda-gdb");
    std::fs::write(
        &fake,
        r##"#!/usr/bin/env python3
import sys
print('(gdb)', flush=True)
for command in sys.stdin:
    if '-gdb-exit' in command:
        print('^exit', flush=True)
        break
    if '-exec-run' in command:
        print('*running,thread-id="all"', flush=True)
        print('*stopped,reason="breakpoint-hit"', flush=True)
    if 'flood' in command:
        for i in range(50):
            print('x'*20000, flush=True)
    print('^done', flush=True)
"##,
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut session = DebugSession::launch(
        fake.to_str().unwrap(),
        Path::new("Cargo.toml"),
        &["a b".into(), "$(touch forbidden)".into()],
        None,
    )
    .unwrap();
    assert!(session.send("-exec-run\n-gdb-exit").is_err());
    session.breakpoint("file with spaces.cu:42").unwrap();
    session.send("-exec-run").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while session.state != State::Stopped && Instant::now() < deadline {
        session.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(session.state, State::Stopped);
    session.console("flood").unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        session.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(session.transcript.len() <= OUTPUT_LIMIT);
    session.stop();
    let deadline = Instant::now() + Duration::from_secs(3);
    while session.state != State::Exited && Instant::now() < deadline {
        session.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(session.state, State::Exited);
    assert!(!Path::new("forbidden").exists());
}

#[test]
fn unsupported_platforms_disable_tools_without_disabling_cuda_highlighting() {
    assert_eq!(
        hedit::debugger::LOCAL_TOOLS_SUPPORTED,
        cfg!(target_os = "linux")
    );
    #[cfg(not(target_os = "linux"))]
    {
        let executable = std::path::Path::new("Cargo.toml");
        assert!(DebugSession::launch("cuda-gdb", executable, &[], None)
            .err()
            .unwrap()
            .contains("supported on Linux only"));
        assert!(DebugSession::sanitizer(executable, &[], "memcheck").is_err());
        assert!(DebugSession::gpu_info().is_err());
    }
    assert_eq!(
        hedit::syntax::Language::detect("kernel.cu"),
        hedit::syntax::Language::Cuda
    );
}
