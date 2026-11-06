use hedit::build::{all, choose, defaults, locations, BuildSystem, Event, Running, Variables};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[test]
fn variables_expand_longest_names_first() {
    let vars = Variables {
        file: PathBuf::from("/work/src/kernel.cu"),
        folder: PathBuf::from("/work"),
    };
    assert_eq!(
        vars.expand(r#"nvcc "$file" -o "$file_base_name" in $file_path ($file_name, $file_extension) at $folder"#),
        r#"nvcc "/work/src/kernel.cu" -o "kernel" in /work/src ($file_name, cu) at /work"#
            .replace("$file_name", "kernel.cu")
    );
}

#[test]
fn errors_are_found_with_each_default_file_regex() {
    let regex = |name: &str| {
        defaults()
            .into_iter()
            .find(|s| s.name == name)
            .unwrap()
            .file_regex
    };
    let dir = Path::new("/work");
    let nvcc = locations(
        "kernel.cu(12): error: identifier \"x\" is undefined\n",
        &regex("CUDA (nvcc)"),
        dir,
        0,
    );
    assert_eq!(nvcc[0].file, PathBuf::from("/work/kernel.cu"));
    assert_eq!((nvcc[0].line, nvcc[0].column), (12, None));
    assert!(nvcc[0].message.starts_with("error: identifier"));
    let gcc = locations(
        "ok\nsrc/a.c:3:7: warning: unused\n",
        &regex("C (gcc)"),
        dir,
        10,
    );
    assert_eq!(gcc[0].output_line, 11);
    assert_eq!((gcc[0].line, gcc[0].column), (3, Some(7)));
    assert_eq!(gcc[0].message, "warning: unused");
    let rust = locations(
        "error[E0425]: x\n  --> src/main.rs:2:5\n",
        &regex("Rust (cargo build)"),
        dir,
        0,
    );
    assert_eq!(
        (rust[0].file.clone(), rust[0].line),
        (PathBuf::from("/work/src/main.rs"), 2)
    );
    let file = std::env::temp_dir().join("t.py");
    let python = locations(
        &format!("  File \"{}\", line 4, in <module>\n", file.display()),
        &regex("Python"),
        dir,
        0,
    );
    assert_eq!((python[0].file.clone(), python[0].line), (file, 4));
}

#[test]
fn systems_are_chosen_by_syntax_or_name_and_user_ones_replace_defaults() {
    let user = BuildSystem {
        name: "Python".into(),
        shell_cmd: "python3.12 \"$file\"".into(),
        selector: vec!["python".into()],
        ..Default::default()
    };
    let systems = all(std::slice::from_ref(&user));
    assert_eq!(choose(&systems, "automatic", "python"), Some(&user));
    assert_eq!(
        choose(&systems, "automatic", "cuda").unwrap().name,
        "CUDA (nvcc)"
    );
    assert_eq!(choose(&systems, "Make", "python").unwrap().name, "Make");
    assert!(choose(&systems, "automatic", "markdown").is_none());
    assert!(BuildSystem {
        name: "x".into(),
        shell_cmd: "y".into(),
        file_regex: "(".into(),
        ..Default::default()
    }
    .validate()
    .is_err());
}

fn collect(running: &Running) -> (String, Option<i32>) {
    let mut output = String::new();
    let started = Instant::now();
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "build did not finish"
        );
        match running
            .events
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
        {
            Event::Output(text) => output.push_str(&text),
            Event::Finished(code, _) => return (output, code),
        }
    }
}

#[test]
fn builds_stream_merged_output_and_report_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    let command = if cfg!(windows) {
        "echo out& echo err>&2& exit 3"
    } else {
        "echo out; echo err 1>&2; exit 3"
    };
    let running = Running::start(command, dir.path(), || {}).unwrap();
    let (output, code) = collect(&running);
    assert_eq!(output.replace("\r\n", "\n"), "out\nerr\n");
    assert_eq!(code, Some(3));
}

fn noisy_build() -> (tempfile::TempDir, Running) {
    let directory = tempfile::tempdir().unwrap();
    let python = if cfg!(windows) { "python" } else { "python3" };
    let command = format!(
        r#"{python} -c "import pathlib,sys; pathlib.Path('ready').touch(); sys.stdout.buffer.write(b'x'*4194304); sys.stdout.flush(); pathlib.Path('done').touch()""#
    );
    let running = Running::start(&command, directory.path(), || {}).unwrap();
    let started = Instant::now();
    while !directory.path().join("ready").exists() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "producer did not start"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    (directory, running)
}

#[test]
fn build_output_waits_for_the_consumer_instead_of_growing_an_unbounded_queue() {
    let (directory, running) = noisy_build();
    std::thread::sleep(Duration::from_millis(200));
    let finished_while_unread = directory.path().join("done").exists();
    // Drain before asserting so both fixed and unfixed builds exit cleanly.
    let (output, code) = collect(&running);
    assert_eq!(code, Some(0), "{output}");
    assert_eq!(output.len(), 4 * 1024 * 1024);
    assert!(
        !finished_while_unread,
        "all output was queued without a consumer"
    );
}

#[test]
fn dropping_the_build_receiver_does_not_leave_a_blocked_output_worker() {
    let (directory, running) = noisy_build();
    std::thread::sleep(Duration::from_millis(200));
    drop(running);
    let started = Instant::now();
    while !directory.path().join("done").exists() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the build stayed blocked after its receiver was dropped"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn cancellation_still_works_while_build_output_is_backpressured() {
    let (directory, running) = noisy_build();
    std::thread::sleep(Duration::from_millis(200));
    assert!(!directory.path().join("done").exists());
    let started = Instant::now();
    running.cancel();
    let (_, code) = collect(&running);
    assert_ne!(code, Some(0));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(!directory.path().join("done").exists());
}

#[cfg(unix)]
#[test]
fn cancellation_stops_a_build_that_has_already_closed_its_output() {
    let directory = tempfile::tempdir().unwrap();
    let running = Running::start(
        r#"printf '%s' "$$" > pid; touch ready; exec 1>&- 2>&-; sleep 30"#,
        directory.path(),
        || {},
    )
    .unwrap();
    let started = Instant::now();
    while !directory.path().join("ready").exists() {
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(200));
    running.cancel();
    let result = running.events.recv_timeout(Duration::from_secs(2));
    // Always stop our fixture even when the original cancellation bug occurs.
    if matches!(result, Err(std::sync::mpsc::RecvTimeoutError::Timeout)) {
        let pid: i32 = std::fs::read_to_string(directory.path().join("pid"))
            .unwrap()
            .parse()
            .unwrap();
        if let Some(pid) = rustix::process::Pid::from_raw(pid) {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
    }
    assert!(
        matches!(result, Ok(Event::Finished(..))),
        "cancellation waited for a process that had closed its output"
    );
}

#[cfg(unix)]
#[test]
fn build_output_keeps_an_incomplete_utf8_tail_visible() {
    let directory = tempfile::tempdir().unwrap();
    let running = Running::start(r"printf '\303'", directory.path(), || {}).unwrap();
    let (output, code) = collect(&running);
    assert_eq!(code, Some(0));
    assert_eq!(output, "\u{fffd}");
}

fn slow_build(late: bool) -> &'static str {
    match (cfg!(windows), late) {
        (true, true) => "echo started& ping -n 31 127.0.0.1 >nul& echo late",
        (true, false) => "echo started& ping -n 31 127.0.0.1 >nul",
        (false, true) => "echo started; sleep 30; echo late",
        (false, false) => "echo started; sleep 30",
    }
}

#[test]
fn cancelling_stops_the_commands_a_build_started() {
    let dir = tempfile::tempdir().unwrap();
    // The sleep is a grandchild of the build's shell.
    let running = Running::start(slow_build(true), dir.path(), || {}).unwrap();
    match running.events.recv_timeout(Duration::from_secs(5)).unwrap() {
        Event::Output(text) => assert_eq!(text.replace("\r\n", "\n"), "started\n"),
        Event::Finished(..) => panic!("finished early"),
    }
    let started = Instant::now();
    running.cancel();
    let (output, code) = collect(&running);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(!output.contains("late"));
    assert_ne!(code, Some(0));
}

#[test]
fn cancelling_while_the_shell_starts_a_command_still_stops_it() {
    // Cancel right after the first output, while the shell may be starting
    // `sleep`; a process created as the signal lands must not survive.
    let dir = tempfile::tempdir().unwrap();
    for _ in 0..20 {
        let running = Running::start(slow_build(false), dir.path(), || {}).unwrap();
        let _ = running.events.recv_timeout(Duration::from_secs(5)).unwrap();
        let started = Instant::now();
        running.cancel();
        loop {
            match running.events.recv_timeout(Duration::from_secs(3)) {
                Ok(Event::Finished(..)) => break,
                Ok(Event::Output(_)) => {}
                Err(_) => panic!("cancel did not stop the build"),
            }
        }
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}

#[test]
fn gcc_errors_accept_windows_drive_letters() {
    let regex = defaults()
        .into_iter()
        .find(|s| s.name == "C (gcc)")
        .unwrap()
        .file_regex;
    let found = locations(
        r"C:\work\file.c:12:3: error: missing symbol",
        &regex,
        Path::new("."),
        0,
    );
    assert_eq!((found[0].line, found[0].column), (12, Some(3)));
    assert!(found[0].file.to_string_lossy().ends_with(r"C:\work\file.c"));
}

#[cfg(windows)]
#[test]
fn windows_builds_preserve_quoted_executable_paths_and_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("build script.cmd");
    std::fs::write(&script, "@echo %~1\r\n@echo problem>&2\r\n@exit /b 7\r\n").unwrap();
    let command = format!("\"{}\" \"argument with spaces\"", script.display());
    let running = Running::start(&command, dir.path(), || {}).unwrap();
    let (output, code) = collect(&running);
    assert_eq!(
        output.replace("\r\n", "\n"),
        "argument with spaces\nproblem\n"
    );
    assert_eq!(code, Some(7));
}

#[test]
fn path_variables_are_not_recursively_expanded() {
    let vars = Variables {
        file: PathBuf::from("/work/$folder.py"),
        folder: PathBuf::from("/work"),
    };
    assert_eq!(vars.expand("$file"), "/work/$folder.py");
    assert_eq!(vars.expand("$folder_suffix"), "$folder_suffix");
}

#[test]
fn quoted_build_paths_reach_the_shell_without_reinterpreting_the_filename() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("literal$folder & %NAME% file.py");
    std::fs::write(&file, "pass\n").unwrap();
    let vars = Variables {
        file,
        folder: dir.path().to_owned(),
    };
    let command = if cfg!(windows) {
        "if exist \"$file\" (echo FILE_OK) else (exit /b 7)"
    } else {
        "test -f \"$file\" && printf 'FILE_OK\\n'"
    };
    let running = Running::start_with_variables(command, dir.path(), &vars, || {}).unwrap();
    let (output, code) = collect(&running);
    assert_eq!(code, Some(0), "{output}");
    assert_eq!(output.trim(), "FILE_OK");
}
