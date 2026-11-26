use hedit::{document::Document, projects};
use std::fs;

#[test]
fn a_normal_document_edit_cannot_save_text_that_cannot_reopen() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("notes.txt");
    let mut doc = Document::empty(7);
    if doc.replace(0, 0, "before\0after".into(), 0).is_err()
        || doc.save(Some(path.clone())).is_err()
    {
        return;
    }
    let reopened = Document::open(8, &path).unwrap();
    assert_eq!(reopened.text(1024).unwrap(), "before\0after");
}

#[test]
fn accepted_configured_encoding_round_trips_its_own_output() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("notes.txt");
    let mut doc = Document::empty(7);
    doc.replace(0, 0, "café".into(), 0).unwrap();
    let configured = projects::Formatting {
        charset: Some("iso-2022-kr".into()),
        ..Default::default()
    };
    if configured.validate().is_err()
        || doc
            .set_encoding(configured.charset.as_deref().unwrap())
            .is_err()
    {
        return;
    }
    doc.save(Some(path.clone())).unwrap();
    let label = doc.info().encoding;
    let reopened = Document::open_with_encoding(8, &path, &label).unwrap();
    assert_eq!(reopened.text(1024).unwrap(), "café");
}

#[test]
fn replacing_a_paged_file_with_identical_text_keeps_it_clean() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("large.txt");
    fs::write(
        &path,
        format!("needle\n{}", "long filler line\n".repeat(130_000)),
    )
    .unwrap();
    let mut doc = Document::open(1, &path).unwrap();
    assert!(doc.info().paged && !doc.info().dirty);
    let revision = doc.revision();
    let (_, count) = doc
        .replace_all("needle", "needle", revision, |_, _| {})
        .unwrap();
    assert_eq!(count, 1);
    assert!(!doc.info().dirty);
    assert_eq!(doc.revision(), revision);
}
