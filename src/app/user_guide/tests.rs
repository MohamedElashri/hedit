use super::*;
use crate::app::tests::{click_at, click_label, frame, frame_size, key, text_rect};

fn fixture() -> (tempfile::TempDir, egui::Context, HeditApp) {
    let directory = tempfile::tempdir().unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    ctx.style_mut(|s| s.scroll_animation = egui::style::ScrollAnimation::none());
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.workspace.open[0].page.text = "a note that must not change".into();
    app.workspace.edit(0, "").unwrap();
    frame(&ctx, &mut app, vec![]);
    (directory, ctx, app)
}

#[test]
fn every_local_document_link_is_bundled_and_every_fragment_exists() {
    let ctx = egui::Context::default();
    let mut guide = Guide::default();
    for (document, &(path, source)) in DOCUMENTS.iter().enumerate() {
        guide.load(document);
        let reader = guide.reader.as_ref().unwrap();
        assert!(
            !Parser::new(source).any(|e| matches!(e, Event::Start(Tag::Image { .. }))),
            "reader documents must not fetch images"
        );
        assert_eq!(reader.topics.len(), reader.prepared.headings.len());
        // Math and Mermaid examples in the manual must remain examples.
        assert!(!reader
            .prepared
            .rendered
            .contains("<!--hedit-markdown-figure:"));
        let targets = reader.prepared.targets.clone();
        for target in targets {
            if target.starts_with("https://") || target.starts_with("http://") {
                continue;
            }
            guide.load(document);
            let (destination, fragment) = guide
                .resolve(&target)
                .unwrap_or_else(|| panic!("{}: unbundled {target}", path));
            let reader = Reader::new(destination);
            assert!(
                fragment.is_empty() || reader.prepared.headings.contains_key(&fragment),
                "{}: invalid {target}",
                path
            );
            guide.navigate(&ctx, &target);
            assert!(guide.notice.is_none(), "{target}");
        }
    }
    assert!(
        ctx.output(|o| !o
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::OpenUrl(_)))),
        "local links must not open a browser"
    );
    guide.load(0);
    assert!(guide.resolve("../../secret.md").is_none());
    assert!(guide.resolve("file:///etc/passwd").is_none());
}

#[test]
fn help_about_palette_and_f1_open_the_same_offline_modal_without_changing_notes() {
    let (_directory, ctx, mut app) = fixture();
    let source = app.active().page.text.clone();
    let revision = app.active().page.info.revision;
    click_label(&ctx, &mut app, "Help");
    click_label(&ctx, &mut app, "User guide");
    frame(&ctx, &mut app, vec![]);
    assert!(app.guide.open);
    assert_eq!(app.guide.document, 0);
    // Editing shortcuts and text do not reach the document through the modal.
    frame(
        &ctx,
        &mut app,
        vec![
            key(Key::N, Modifiers::COMMAND),
            egui::Event::Text("accidental typing".into()),
        ],
    );
    assert_eq!(app.workspace.open.len(), 1);
    assert_eq!(app.active().page.text, source);
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(!app.guide.open);
    frame(&ctx, &mut app, vec![key(Key::F1, Modifiers::NONE)]);
    assert!(app.guide.open);
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    click_label(&ctx, &mut app, "Help");
    click_label(&ctx, &mut app, "About hedit");
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    click_label(&ctx, &mut app, "User guide");
    frame(&ctx, &mut app, vec![]);
    assert!(app.guide.open && !app.about_open);
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    app.execute(&ctx, Command::Palette);
    app.palette_query = "User guide".into();
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    frame(&ctx, &mut app, vec![]);
    assert!(app.guide.open && !app.palette);
    assert_eq!(app.active().page.info.revision, revision);
    assert_eq!(app.active().page.text, source);
    assert!(ctx.output(|o| !o
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::OpenUrl(_)))));
}

#[test]
fn contents_links_and_back_navigate_locally_and_restore_scroll() {
    let (_directory, ctx, mut app) = fixture();
    app.execute(&ctx, Command::UserGuide);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    click_label(&ctx, &mut app, "Markdown preview");
    for _ in 0..5 {
        frame(&ctx, &mut app, vec![]);
    }
    let at = app.guide.position;
    assert!(at > 1000.0, "section link did not scroll: {at}");
    // Use Contents to reach a different part of the same document.
    click_label(&ctx, &mut app, "Contents");
    click_label(&ctx, &mut app, "Installation");
    for _ in 0..5 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(app.guide.position < at);
    let before = app.guide.position;
    app.guide
        .navigate(&ctx, "DEVELOPER_GUIDE.md#build-and-install");
    for _ in 0..5 {
        frame(&ctx, &mut app, vec![]);
    }
    assert_eq!(app.guide.document, 1);
    click_label(&ctx, &mut app, "Back");
    for _ in 0..5 {
        frame(&ctx, &mut app, vec![]);
    }
    assert_eq!(app.guide.document, 0);
    assert!((app.guide.position - before).abs() < 1.0);
    assert!(ctx.output(|o| !o
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::OpenUrl(_)))));
}

#[test]
fn guide_fits_compact_windows_and_dismisses_without_clicking_through() {
    for size in [
        egui::vec2(720.0, 480.0),
        egui::vec2(420.0, 340.0),
        egui::vec2(1280.0, 820.0),
    ] {
        let (_directory, ctx, mut app) = fixture();
        app.execute(&ctx, Command::UserGuide);
        for _ in 0..3 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let output = frame_size(&ctx, &mut app, vec![], size);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let layer = output
            .shapes
            .iter()
            .find_map(|s| text_rect(&s.shape, "User guide").map(|rect| (s, rect)))
            .unwrap();
        assert!(screen.contains_rect(layer.1));
        let modal = egui::AreaState::load(&ctx, egui::Id::new("user-guide"))
            .unwrap()
            .rect();
        assert!(
            screen.contains_rect(modal),
            "modal overflow: {size:?}, {modal:?}"
        );
        click_at(&ctx, &mut app, egui::pos2(size.x - 3.0, size.y / 2.0), size);
        assert!(!app.guide.open);
        assert_eq!(app.workspace.open.len(), 1);
        frame_size(&ctx, &mut app, vec![], size);
        frame_size(
            &ctx,
            &mut app,
            vec![egui::Event::Text(" next".into())],
            size,
        );
        assert!(app.active().page.text.contains(" next"));
    }
}

#[test]
#[ignore = "writes offline guide screenshots to HEDIT_PREVIEW_DIR"]
fn export_user_guide_previews() {
    let directory =
        PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
    std::fs::create_dir_all(&directory).unwrap();
    for (name, theme) in [("dark", Theme::default()), ("light", Theme::paper())] {
        for (layout, size) in [
            ("wide", egui::vec2(1280.0, 820.0)),
            ("compact", egui::vec2(720.0, 480.0)),
        ] {
            let (_session, ctx, mut app) = fixture();
            app.set_theme(&ctx, theme.clone());
            app.execute(&ctx, Command::UserGuide);
            for _ in 0..4 {
                frame_size(&ctx, &mut app, vec![], size);
            }
            let output = frame_size(&ctx, &mut app, vec![], size);
            let ppm = crate::app::rtl_tests::rasterize(&ctx, output, size, 1.0);
            std::fs::write(directory.join(format!("guide-{name}-{layout}.ppm")), ppm).unwrap();
        }
    }
}
