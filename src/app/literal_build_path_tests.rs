use super::*;

#[test]
fn build_variables_treat_filename_characters_as_data() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.settings.build_system = "literal path".into();
    app.settings.build_systems.push(build::BuildSystem {
        name: "literal path".into(),
        shell_cmd: "printf '%s\\n' \"$file\"".into(),
        ..Default::default()
    });
    for name in [
        "literal$HOME.py",
        "literal$(printf EXPANDED).py",
        "literal\"quote.py",
        "literal$file_name.py",
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, "pass\n").unwrap();
        app.open(&ctx, Some(path.clone()));
        tests::finish_app_job(&mut app, &ctx);
        app.build(&ctx);
        let started = Instant::now();
        while app.build.running() {
            assert!(started.elapsed() < Duration::from_secs(5));
            app.poll_build();
            thread::sleep(Duration::from_millis(5));
        }
        let expected = path.canonicalize().unwrap().display().to_string();
        assert!(
            app.build.output.starts_with(&(expected + "\n")),
            "{}",
            app.build.output
        );
    }
}
