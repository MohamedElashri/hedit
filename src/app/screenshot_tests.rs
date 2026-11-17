//! README images rendered from the real application with a disposable project.
use super::tests::{finish_app_job, text_rect};
use super::*;
use std::collections::HashMap;

const SIZE: egui::Vec2 = egui::vec2(1280.0, 820.0);
const SCALE: f32 = 2.0;
const MAIN_RS: &str = r#"mod report;

use std::error::Error;
use std::fs;

/// Summarize a day's sensor readings.
fn main() -> Result<(), Box<dyn Error>> {
    let source = fs::read_to_string("data/readings.csv")?;
    let readings: Vec<f64> = source
        .lines()
        .skip(1)
        .filter_map(|line| line.split(',').nth(1)?.parse().ok())
        .collect();

    let total: f64 = readings.iter().sum();
    let average = total / readings.len() as f64;
    let peak = readings.iter().copied().fold(0.0, f64::max);

    println!("Atlas · daily report");
    println!("──────────────────────────");
    println!("Samples     {}", readings.len());
    println!("Average     {average:.1} °C");
    println!("Peak        {peak:.1} °C");
    report::print_summary(&readings);
    Ok(())
}
"#;
const REPORT_RS: &str = r#"/// Show how many readings need attention.
pub fn print_summary(readings: &[f64]) {
    let threshold = 24.0;
    let above = readings
        .iter()
        .filter(|&&value| value > threshold)
        .count();

    println!("Above 24 °C {above}");
    println!();
    println!("Analysis complete. Ready for review.");
}
"#;
const NOTES: &str = r#"# Atlas field notes

A small toolkit for **environmental data**.
Keep the analysis close to the observations.

## Daily review

- [x] Import the sensor readings
- [x] Check the summary statistics
- [ ] Review the afternoon peak

| Station | Samples | Status |
| :--- | ---: | :--- |
| North ridge | 128 | Complete |
| River bank | 96 | Complete |

## Analysis pipeline

```mermaid
flowchart LR
    A[Collect] --> B[Review]
```

The sample mean is $\bar{x} = \frac{1}{n}\sum_{i=1}^{n} x_i$.

> Record assumptions before interpreting results.
"#;

fn write_project(root: &std::path::Path) {
    for (path, text) in [
        ("src/main.rs", MAIN_RS),
        ("src/report.rs", REPORT_RS),
        ("README.md", NOTES),
        (".gitignore", "/target\n"),
        ("Cargo.toml", "[package]\nname = \"atlas\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        ("data/readings.csv", "time,temperature\n08:00,18.4\n09:00,20.1\n10:00,22.8\n11:00,24.6\n12:00,26.3\n13:00,27.1\n14:00,25.7\n15:00,23.9\n"),
    ] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

struct Scene {
    _directory: tempfile::TempDir,
    root: PathBuf,
    ctx: egui::Context,
    app: HeditApp,
    textures: HashMap<egui::TextureId, egui::ColorImage>,
}
impl Scene {
    fn new(light: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("atlas");
        write_project(&root);
        let (workspace, errors) = Workspace::restore(directory.path().join("session")).unwrap();
        assert!(errors.is_empty());
        let mut scene = Self::from_workspace(directory, root, workspace, light);
        scene
            .app
            .set_folder(hedit::folder::read_directory(scene.root.clone()).unwrap());
        scene
            .app
            .project
            .expanded
            .insert(scene.root.join("src").to_string_lossy().into_owned());
        scene.app.sync_folder_view();
        for file in ["README.md", "src/report.rs", "src/main.rs"] {
            scene.app.open(&scene.ctx, Some(scene.root.join(file)));
            finish_app_job(&mut scene.app, &scene.ctx);
        }
        let blank = scene
            .app
            .workspace
            .open
            .iter()
            .position(|b| b.page.info.path.is_none())
            .unwrap();
        scene.app.workspace.close(blank).unwrap();
        scene.app.workspace.closed.clear();
        scene
            .app
            .set_selections(vec![hedit::advanced::Selection::caret(0)]);
        scene.settle();
        scene
    }
    fn from_workspace(
        directory: tempfile::TempDir,
        root: PathBuf,
        workspace: Workspace,
        light: bool,
    ) -> Self {
        settings_store::TEST_CONFIG_DIR.with(|dir| {
            *dir.borrow_mut() = Some(directory.path().join("config"));
        });
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(SCALE);
        ctx.style_mut(|s| s.scroll_animation = egui::style::ScrollAnimation::none());
        settings_store::fallback_fonts();
        settings_store::set_editor_font(&ctx, None);
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.set_theme(
            &ctx,
            hedit::theme::builtin(if light {
                hedit::theme::PAPER
            } else {
                hedit::theme::INK
            })
            .unwrap(),
        );
        app.settings.font_size = 16.0;
        app.settings.caret_blink = false;
        app.settings.terminal_onboarding_complete = true;
        Self {
            _directory: directory,
            root,
            ctx,
            app,
            textures: HashMap::new(),
        }
    }
    fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.app.project.poll();
        self.app.receive(&self.ctx);
        self.app.poll_terminal(&self.ctx);
        let output = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE)),
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| {
                self.app.poll_source_control(ctx);
                self.app.shortcuts(ctx);
                self.app.chrome(ctx);
                self.app.search_panel(ctx);
                self.app.workbench_panel(ctx);
                self.app.editor(ctx);
                self.app.quality_windows(ctx);
                self.app.preferences(ctx);
                self.app.buffer_tools(ctx);
                self.app.close_dialog(ctx);
            },
        );
        for (id, delta) in &output.textures_delta.set {
            let egui::ImageData::Color(image) = &delta.image else {
                continue;
            };
            if let Some([x, y]) = delta.pos {
                if let Some(target) = self.textures.get_mut(id) {
                    for row in 0..image.height() {
                        let start = (y + row) * target.width() + x;
                        target.pixels[start..start + image.width()].copy_from_slice(
                            &image.pixels[row * image.width()..(row + 1) * image.width()],
                        );
                    }
                }
            } else {
                self.textures.insert(*id, (**image).clone());
            }
        }
        output
    }
    fn settle(&mut self) {
        for _ in 0..4 {
            self.frame(vec![]);
        }
    }
    fn wait(&mut self, ready: impl Fn(&mut HeditApp) -> bool) {
        let started = Instant::now();
        loop {
            self.frame(vec![]);
            if ready(&mut self.app) {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "screenshot job timed out"
            );
            thread::sleep(Duration::from_millis(10));
        }
        self.settle();
    }
    fn click(&mut self, label: &str) {
        let output = self.frame(vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|s| text_rect(&s.shape, label))
            .unwrap_or_else(|| panic!("missing {label}"))
            .center();
        for pressed in [true, false] {
            self.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ]);
        }
        self.settle();
    }
    fn save(&mut self, output: &std::path::Path, name: &str) {
        if let Some(remote) = &self.app.workspace.remote {
            remote.flush().unwrap();
        }
        self.app.checkpoint(&self.ctx);
        self.wait(|app| !app.checkpoint_pending && app.persisted == app.generation);
        let started = Instant::now();
        loop {
            let frame = self.frame(vec![]);
            if !frame
                .shapes
                .iter()
                .any(|shape| text_rect(&shape.shape, "Loading…").is_some())
            {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "folder listing did not settle"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(self.app.error.is_none(), "{:?}", self.app.error);
        let frame = self.frame(vec![]);
        self.textures.extend(self.app.markdown.media.textures());
        std::fs::write(
            output.join(format!("{name}.ppm")),
            super::rtl_tests::rasterize_with_textures(
                &self.ctx,
                frame,
                SIZE,
                SCALE,
                &self.textures,
            ),
        )
        .unwrap();
    }
}
impl Drop for Scene {
    fn drop(&mut self) {
        settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
    }
}

#[test]
#[ignore = "writes README screenshots to HEDIT_PREVIEW_DIR; terminal scene starts a real shell"]
fn export_readme_screenshots() {
    let output =
        PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
    std::fs::create_dir_all(&output).unwrap();
    for light in [false, true] {
        let mut scene = Scene::new(light);
        scene.save(
            &output,
            if light {
                "screenshot-light"
            } else {
                "screenshot-dark"
            },
        );
    }
    let mut scene = Scene::new(true);
    scene
        .app
        .open(&scene.ctx, Some(scene.root.join("README.md")));
    finish_app_job(&mut scene.app, &scene.ctx);
    scene.app.settings.word_wrap = true;
    scene.app.execute(&scene.ctx, Command::MarkdownSplit);
    scene.wait(|app| !app.markdown.media.pending() && app.markdown.media.rendered_count() == 2);
    scene.save(&output, "markdown");
    drop(scene);

    let mut scene = Scene::new(false);
    scene.app.quality.folder_query = "readings".into();
    scene.app.quality.folders_open = true;
    scene.app.folder_search(&scene.ctx, false);
    scene.wait(|app| app.quality.folder_task.is_none());
    assert!(scene.app.quality.results.hits.len() >= 5);
    scene.save(&output, "search");
    drop(scene);

    let mut scene = Scene::new(false);
    scene.app.settings.word_wrap = true;
    scene.app.execute(&scene.ctx, Command::SplitColumns);
    scene.app.execute(&scene.ctx, Command::FocusGroup2);
    scene
        .app
        .open(&scene.ctx, Some(scene.root.join("src/report.rs")));
    finish_app_job(&mut scene.app, &scene.ctx);
    assert_eq!(scene.app.panes.groups(), 2);
    scene.save(&output, "split-panes");
    drop(scene);

    let mut scene = Scene::new(true);
    let git = hedit::git::Git::default();
    git.command(&scene.root, &["init", "-b", "main"]).unwrap();
    for (name, value) in [
        ("user.name", "Atlas contributors"),
        ("user.email", "atlas@example.invalid"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", "/nonexistent/hedit-screenshot-hooks"),
    ] {
        git.command(&scene.root, &["config", name, value]).unwrap();
    }
    git.command(&scene.root, &["add", "."]).unwrap();
    git.command(&scene.root, &["commit", "-m", "Add daily sensor report"])
        .unwrap();
    std::fs::write(scene.root.join("src/main.rs"), MAIN_RS.replace("    let total:", "    // Ignore invalid samples before computing the summary.\n    let total:").replace("    let average = total / readings.len() as f64;", "    let average = if readings.is_empty() {\n        0.0\n    } else {\n        total / readings.len() as f64\n    };" )).unwrap();
    scene.app.open_source_control(&scene.ctx);
    scene.wait(|app| !app.source_control.pending);
    scene.click("main.rs");
    scene.wait(|app| !app.source_control.pending);
    scene.click("Unified");
    scene.save(&output, "git");
    drop(scene);

    #[cfg(feature = "terminal")]
    {
        let mut scene = Scene::new(false);
        scene.app.choose_terminal(true);
        scene.app.toggle_terminal(&scene.ctx);
        scene.app.terminal.tabs[0].name = "Atlas · report".into();
        scene.settle();
        let session = scene.app.terminal.active_session_mut().unwrap();
        session
            .write(b"export PS1='atlas $ '; clear; cargo run --quiet\n".to_vec())
            .unwrap();
        // Readiness is checked against actual shell output, not injected VT cells.
        scene.wait(|app| {
            app.terminal
                .active_session_mut()
                .unwrap()
                .grid()
                .unwrap()
                .rows
                .iter()
                .any(|row| {
                    row.iter()
                        .map(|cell| cell.text.as_str())
                        .collect::<String>()
                        .contains("Ready for review.")
                })
        });
        scene.save(&output, "terminal");
    }
    let mut scene = Scene::new(false);
    scene.app.open_preferences(PreferencesPage::Appearance);
    scene.save(&output, "themes");
}

#[test]
#[ignore = "exports the workbench Settings scenes to HEDIT_PREVIEW_DIR"]
fn export_workbench_settings_screenshots() {
    let output =
        PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
    std::fs::create_dir_all(&output).unwrap();
    for light in [false, true] {
        Scene::new(light).save(
            &output,
            if light {
                "screenshot-light"
            } else {
                "screenshot-dark"
            },
        );
    }
    let mut scene = Scene::new(false);
    scene.app.open_preferences(PreferencesPage::Appearance);
    scene.save(&output, "themes");
    scene.app.settings_browser.query = "ui_font".into();
    scene.save(&output, "settings");
}

#[test]
#[ignore = "writes a real checkpoint/restart/closed-note recovery demo to HEDIT_PREVIEW_DIR"]
fn export_readme_recovery_demo() {
    let output =
        PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
    std::fs::create_dir_all(&output).unwrap();
    let mut scene = Scene::new(false);
    scene.app.execute(&scene.ctx, Command::New);
    scene.settle();
    let note = "# Afternoon observations\n\n## North ridge\n\n- Temperature peaked at 27.1 °C after lunch.\n- Recheck the sensor before the next collection.\n\n## River bank\n\n- Samples look consistent with yesterday.\n- Add the missing morning observation.\n\n## Next session\n\nCompare both stations before sharing the report.\n";
    scene.frame(vec![egui::Event::Text(note.into())]);
    scene
        .app
        .workspace
        .name_scratchpad(scene.app.workspace.active, "Field notebook")
        .unwrap();
    let index = scene.app.workspace.active;
    scene.app.workspace.open[index].set_language(Some(hedit::syntax::Language::Markdown));
    scene.app.changed();
    let id = scene.app.active().view.id;
    assert_eq!(scene.app.active().page.text, note);
    assert!(scene.app.active().page.info.path.is_none());
    scene.save(&output, "recovery-01");
    scene.app.execute(&scene.ctx, Command::Close);
    assert!(scene.app.close_flow.is_some());
    scene
        .app
        .choose_close(&scene.ctx, super::close::Choice::Discard);
    assert!(scene.app.workspace.closed.iter().any(|b| b.id == id));
    scene.save(&output, "recovery-closed");

    // Drop the actual app/store, then reopen the same session from disk.
    let directory = std::mem::replace(&mut scene._directory, tempfile::tempdir().unwrap());
    let root = scene.root.clone();
    drop(scene);
    let (workspace, errors) = Workspace::restore(directory.path().join("session")).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    let recovered_id = workspace
        .closed
        .iter()
        .find(|view| view.custom_name.as_deref() == Some("Field notebook"))
        .expect("the named unsaved note is recovered on the shelf")
        .id;
    let mut scene = Scene::from_workspace(directory, root, workspace, false);
    scene.app.execute(&scene.ctx, Command::Shelf);
    scene.save(&output, "recovery-02");
    scene.app.shelf_open = false;
    scene.app.execute(&scene.ctx, Command::Reopen);
    finish_app_job(&mut scene.app, &scene.ctx);
    assert_eq!(scene.app.active().view.id, recovered_id);
    assert_eq!(scene.app.active().page.text, note);
    assert!(scene.app.active().page.info.path.is_none());
    scene.save(&output, "recovery-03");
}

#[test]
#[ignore = "requires a disposable real SSH fixture in HEDIT_REMOTE_TEST_CONFIG; writes README image"]
fn export_readme_ssh_screenshot() {
    let fixture: serde_json::Value = serde_json::from_str(
        &std::env::var("HEDIT_REMOTE_TEST_CONFIG").expect("set HEDIT_REMOTE_TEST_CONFIG"),
    )
    .unwrap();
    let config = hedit::remote::Config {
        target: hedit_remote::client::SshTarget {
            host: "lab".into(),
            user: None,
            port: fixture["port"].as_u64().unwrap() as u16,
            config: Some(fixture["config"].as_str().unwrap().into()),
            helper: fixture["helper"].as_str().unwrap().into(),
            state: fixture["state"].as_str().unwrap().into(),
        },
        root: fixture["root"].as_str().unwrap().into(),
        takeover: false,
    };
    write_project(std::path::Path::new(&config.root));
    let directory = tempfile::tempdir().unwrap();
    let (workspace, errors) =
        Workspace::connect_remote(directory.path().join("client"), config).unwrap();
    assert!(errors.is_empty());
    let remote = workspace.remote.clone().unwrap();
    let mut scene = Scene::from_workspace(directory, PathBuf::new(), workspace, false);
    scene
        .app
        .set_folder(hedit::folder::read_directory(remote.root()).unwrap());
    scene
        .app
        .project
        .expanded
        .insert(remote.path("src").unwrap().to_string_lossy().into_owned());
    for file in ["README.md", "src/report.rs", "src/main.rs"] {
        scene.app.open(&scene.ctx, Some(remote.path(file).unwrap()));
        finish_app_job(&mut scene.app, &scene.ctx);
    }
    if let Some(blank) = scene
        .app
        .workspace
        .open
        .iter()
        .position(|b| b.page.info.path.is_none() && b.page.text.is_empty())
    {
        scene.app.workspace.close(blank).unwrap();
        scene.app.changed();
    }
    scene.app.sync_folder_view();
    scene.settle();
    scene
        .app
        .set_selections(vec![hedit::advanced::Selection::caret(0)]);
    scene.frame(vec![egui::Event::Text(
        "// Review the next collection with the field team.\n".into(),
    )]);
    remote.flush().unwrap();
    assert_eq!(remote.status().3, "Durable on SSH host");
    assert!(scene.app.active().page.info.dirty);
    assert!(scene
        .app
        .workspace
        .editor
        .get(scene.app.active().view.id)
        .unwrap()
        .lock()
        .unwrap()
        .is_ephemeral());
    let output =
        PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
    std::fs::create_dir_all(&output).unwrap();
    scene.save(&output, "ssh");
    assert!(
        !scene._directory.path().join("client/session.json").exists(),
        "remote document content never enters local recovery"
    );
}

#[test]
#[ignore = "exports persistent sidebar previews to HEDIT_PREVIEW_DIR"]
fn export_sidebar_previews() {
    let output =
        PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
    std::fs::create_dir_all(&output).unwrap();
    for light in [false, true] {
        let mut scene = Scene::new(light);
        scene.app.execute(&scene.ctx, Command::FindFiles);
        scene.app.quality.folder_query = "readings".into();
        scene.app.folder_search(&scene.ctx, false);
        let started = Instant::now();
        while scene.app.quality.folder_task.is_some() {
            scene.app.receive(&scene.ctx);
            assert!(started.elapsed() < Duration::from_secs(20));
            thread::sleep(Duration::from_millis(5));
        }
        scene.save(&output, if light { "search-light" } else { "search-dark" });
        scene.app.show_git_sidebar(&scene.ctx);
        scene.save(&output, if light { "scm-light" } else { "scm-dark" });
    }
}

#[test]
#[ignore = "exports unified panel scenes to HEDIT_PREVIEW_DIR"]
fn export_panel_previews() {
    let output =
        PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
    std::fs::create_dir_all(&output).unwrap();
    let mut scene = Scene::new(false);
    scene.app.build.name = "Project check".into();
    scene.app.build.status = "Finished with one warning".into();
    scene.app.build.output =
        "Compiling atlas v0.1.0\nsrc/main.rs:9:5: warning: unused variable\nFinished in 0.18s\n"
            .into();
    scene.app.build.locations = vec![hedit::build::Location {
        output_line: 1,
        file: scene.root.join("src/main.rs"),
        line: 9,
        column: Some(5),
        message: "warning: unused variable".into(),
    }];
    scene.app.execute(&scene.ctx, Command::BuildResults);
    scene.save(&output, "output");
    scene.app.execute(&scene.ctx, Command::LanguageProblems);
    scene.save(&output, "problems");
    #[cfg(feature = "terminal")]
    {
        scene.app.choose_terminal(true);
        scene.app.execute(&scene.ctx, Command::Terminal);
        scene.save(&output, "terminal");
    }
}

#[test]
#[ignore = "exports group-local tabs to HEDIT_PREVIEW_DIR"]
fn export_group_previews() {
    let output =
        PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
    std::fs::create_dir_all(&output).unwrap();
    for light in [false, true] {
        let mut scene = Scene::new(light);
        scene.app.split(1);
        scene.app.focus_group(1, false);
        scene
            .app
            .open(&scene.ctx, Some(scene.root.join("src/report.rs")));
        finish_app_job(&mut scene.app, &scene.ctx);
        scene.save(&output, if light { "groups-light" } else { "groups-dark" });
    }
}

#[test]
#[ignore = "exports breadcrumbs and Outline to HEDIT_PREVIEW_DIR"]
fn export_document_navigation_previews() {
    let output =
        PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
    std::fs::create_dir_all(&output).unwrap();
    for light in [false, true] {
        let mut scene = Scene::new(light);
        scene.app.execute(&scene.ctx, Command::Outline);
        scene.save(
            &output,
            if light {
                "navigation-light"
            } else {
                "navigation-dark"
            },
        );
    }
}

#[test]
#[ignore = "exports scoped settings to HEDIT_PREVIEW_DIR"]
fn export_scoped_settings_previews() {
    let output =
        PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
    std::fs::create_dir_all(&output).unwrap();
    for light in [false, true] {
        let mut scene = Scene::new(light);
        scene.app.execute(&scene.ctx, Command::ProjectSettings);
        scene.settle();
        scene.click("Resolved settings for the active file");
        scene.settle();
        scene.save(
            &output,
            if light {
                "project-light"
            } else {
                "project-dark"
            },
        );
        scene.app.open_preferences(PreferencesPage::Formatting);
        scene.save(&output, if light { "user-light" } else { "user-dark" });
    }
}
