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
    let python = locations(
        "  File \"/tmp/t.py\", line 4, in <module>\n",
        &regex("Python"),
        dir,
        0,
    );
    assert_eq!(
        (python[0].file.clone(), python[0].line),
        (PathBuf::from("/tmp/t.py"), 4)
    );
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
    let running = Running::start("echo out; echo err 1>&2; exit 3", dir.path(), || {}).unwrap();
    let (output, code) = collect(&running);
    assert_eq!(output, "out\nerr\n");
    assert_eq!(code, Some(3));
}

#[test]
fn cancelling_stops_the_commands_a_build_started() {
    let dir = tempfile::tempdir().unwrap();
    // The sleep is a grandchild of the build's shell.
    let running = Running::start("echo started; sleep 30; echo late", dir.path(), || {}).unwrap();
    match running.events.recv_timeout(Duration::from_secs(5)).unwrap() {
        Event::Output(text) => assert_eq!(text, "started\n"),
        Event::Finished(..) => panic!("finished early"),
    }
    let started = Instant::now();
    running.cancel();
    // Temporary diagnostics for macOS.
    std::thread::sleep(Duration::from_millis(300));
    let ps = std::process::Command::new("ps").args(["-axo", "pid,ppid,pgid,stat,command"]).output().unwrap();
    for line in String::from_utf8_lossy(&ps.stdout).lines().filter(|l| l.contains("sleep 30") || l.contains("started;")) {
        eprintln!("DBG {line}");
    }
    let (output, code) = collect(&running);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(!output.contains("late"));
    assert_eq!(code, None);
}
