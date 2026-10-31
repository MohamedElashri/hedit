use super::tests::frame;
use super::*;

#[test]
fn macos_accessibility_exposes_navigation_and_editor_text() {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    workspace.open[0].page.text = "Accessible audit note".into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    let tree = output
        .platform_output
        .accesskit_update
        .expect("macOS needs a native accessibility tree");
    for label in ["File", "Files", "Source Control"] {
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some(label)),
            "missing accessible control: {label}"
        );
    }
    assert!(
        tree.nodes.iter().any(|(_, node)| node
            .value()
            .is_some_and(|value| value.contains("Accessible audit note"))),
        "the editor contents must reach the accessibility tree"
    );
}
