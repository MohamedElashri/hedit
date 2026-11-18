//! Validate text-only workspace edits on the language worker before review.
use super::{
    text::{self, Encoding, TextEdit},
    Document,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileEdit {
    pub path: PathBuf,
    pub original: String,
    pub updated: String,
    pub revision: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preview {
    pub files: Vec<FileEdit>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuickFix {
    pub title: String,
    pub preview: Option<Preview>,
    pub reason: Option<String>,
}
pub const MAX_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
/// No resource operations, commands or writes occur while preparing a preview.
pub fn prepare(
    value: Value,
    encoding: Encoding,
    root: &Path,
    documents: &[Document],
) -> Result<Preview, String> {
    if value.is_null() {
        return Ok(Preview { files: vec![] });
    }
    let object = value.as_object().ok_or("Invalid workspace edit.")?;
    if object.contains_key("changes") && object.contains_key("documentChanges") {
        return Err("Ambiguous workspace edit contains two edit formats.".into());
    }
    let root = std::fs::canonicalize(root).map_err(super::error)?;
    let mut edits = BTreeMap::<PathBuf, (Option<u64>, Vec<TextEdit>)>::new();
    let mut add = |uri: &str, version: Option<u64>, items: Value| -> Result<(), String> {
        let path = std::fs::canonicalize(text::path(uri)?).map_err(super::error)?;
        if !path.starts_with(&root) {
            return Err("A language-server edit leaves the project root.".into());
        }
        if edits.contains_key(&path) {
            return Err("Duplicate document in workspace edit.".into());
        }
        if edits.len() >= 64 {
            return Err("Workspace edits are limited to 64 files.".into());
        }
        let items: Vec<TextEdit> = serde_json::from_value(items).map_err(super::error)?;
        if items.len() > 10_000 {
            return Err("Too many workspace edits.".into());
        }
        edits.insert(path, (version, items));
        Ok(())
    };
    if let Some(changes) = object.get("changes") {
        for (uri, items) in changes.as_object().ok_or("Invalid workspace changes.")? {
            add(uri, None, items.clone())?;
        }
    } else if let Some(changes) = object.get("documentChanges") {
        for change in changes.as_array().ok_or("Invalid document changes.")? {
            if change.get("kind").is_some() {
                return Err(
                    "File creation, rename and deletion are not supported by text refactoring."
                        .into(),
                );
            }
            let doc = &change["textDocument"];
            let version = if doc["version"].is_null() {
                None
            } else {
                Some(doc["version"].as_u64().ok_or("Invalid edit version.")?)
            };
            add(
                doc["uri"].as_str().ok_or("Missing edit URI.")?,
                version,
                change["edits"].clone(),
            )?;
        }
    } else {
        return Err("Language server returned no text edit set.".into());
    }
    let mut files = Vec::new();
    let mut bytes = 0usize;
    for (path, (version, edits)) in edits {
        let snapshot = documents.iter().find(|d| text::same_path(&d.path, &path));
        if version.is_some() && snapshot.map(|d| d.revision) != version {
            return Err(
                "A versioned workspace edit is stale or refers to a closed document.".into(),
            );
        }
        let original = if let Some(doc) = snapshot {
            doc.text.clone()
        } else {
            let doc = crate::document::Document::open(0, &path)?;
            if doc.info().path.as_ref().is_none_or(|path|!Path::new(path).starts_with(&root)){return Err("The edit source moved outside the project root.".into());}
            doc.text(crate::document::SMALL_LIMIT)?
        };
        if original.len() > crate::document::SMALL_LIMIT as usize {
            return Err("Refactoring requires a complete editor document of at most 2 MiB.".into());
        }
        let updated = text::apply(&original, &edits, encoding)?;
        if updated.len() > crate::document::SMALL_LIMIT as usize {
            return Err("A refactored file exceeds 2 MiB.".into());
        }
        bytes = bytes
            .saturating_add(original.len())
            .saturating_add(updated.len());
        if bytes > MAX_PREVIEW_BYTES {
            return Err("The refactor preview exceeds 8 MiB.".into());
        }
        if original != updated {
            files.push(FileEdit {
                path,
                original,
                updated,
                revision: snapshot.map(|d| d.revision),
            });
        }
    }
    Ok(Preview { files })
}
pub fn quick_fixes(
    value: Value,
    encoding: Encoding,
    root: &Path,
    documents: &[Document],
) -> Result<Vec<QuickFix>, String> {
    if value.is_null() {
        return Ok(vec![]);
    }
    let actions = value.as_array().ok_or("Invalid code action response.")?;
    let mut result = vec![];
    let mut bytes = 0usize;
    for action in actions.iter().take(32) {
        if action["kind"]
            .as_str()
            .is_some_and(|k| k != "quickfix" && !k.starts_with("quickfix."))
        {
            continue;
        }
        let title = action["title"]
            .as_str()
            .unwrap_or("Quick fix")
            .chars()
            .take(200)
            .collect();
        let preview = if action.get("disabled").is_some() {
            Err(action["disabled"]["reason"]
                .as_str()
                .unwrap_or("This fix is disabled.")
                .to_owned())
        } else if action.get("command").is_some() {
            Err("This action requires a server command, which is not supported.".into())
        } else if let Some(edit) = action.get("edit") {
            prepare(edit.clone(), encoding, root, documents)
        } else {
            Err("This action does not contain directly applicable text edits.".into())
        };
        let (preview, reason) = match preview {
            Ok(p) => {
                let size = p
                    .files
                    .iter()
                    .map(|f| f.original.len() + f.updated.len())
                    .sum::<usize>();
                if bytes.saturating_add(size) > MAX_PREVIEW_BYTES {
                    (None, Some("Quick-fix preview limit reached.".into()))
                } else {
                    bytes += size;
                    (Some(p), None)
                }
            }
            Err(e) => (None, Some(e)),
        };
        result.push(QuickFix {
            title,
            preview,
            reason:reason.map(|value|value.chars().take(1000).collect()),
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn edit(start: u32, end: u32, value: &str) -> Value {
        json!({"range":{"start":{"line":0,"character":start},"end":{"line":0,"character":end}},"newText":value})
    }
    fn fixture() -> (tempfile::TempDir, PathBuf, Document) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let path = root.join("main.rs");
        std::fs::write(&path, "disk text").unwrap();
        let doc = Document {
            path,
            root: root.clone(),
            language: "rust".into(),
            revision: 7,
            text: "a😀b\r\n".into(),
            dirty: true,
        };
        (dir, root, doc)
    }
    #[test]
    fn unicode_versioned_edits_use_unsaved_snapshots_and_keep_disk_untouched() {
        let (_dir, root, doc) = fixture();
        let value = json!({"documentChanges":[{"textDocument":{"uri":text::uri(&doc.path).unwrap(),"version":7},"edits":[edit(1,3,"ع")]}]});
        let preview = prepare(value, Encoding::Utf16, &root, std::slice::from_ref(&doc)).unwrap();
        assert_eq!(preview.files[0].updated, "aعb\r\n");
        assert_eq!(preview.files[0].revision, Some(7));
        assert_eq!(std::fs::read_to_string(&doc.path).unwrap(), "disk text");
    }
    #[test]
    fn stale_unknown_versions_overlaps_and_resource_operations_are_rejected() {
        let (_dir, root, doc) = fixture();
        let uri = text::uri(&doc.path).unwrap();
        for value in [
            json!({"documentChanges":[{"textDocument":{"uri":uri,"version":8},"edits":[edit(0,1,"x")]}]}),
            json!({"documentChanges":[{"textDocument":{"uri":uri,"version":7},"edits":[edit(0,1,"x")]}]}),
            json!({"documentChanges":[{"kind":"delete","uri":uri}]}),
            json!({"changes":{&uri:[edit(0,3,"x"),edit(1,3,"y")]}}),
        ] {
            assert!(prepare(value, Encoding::Utf16, &root, &[]).is_err());
        }
    }
    #[test]
    fn edits_cannot_leave_root_or_address_the_same_file_twice() {
        let (_dir, root, doc) = fixture();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let uri = text::uri(&doc.path).unwrap();
        assert!(prepare(
            json!({"changes":{text::uri(outside.path()).unwrap():[edit(0,0,"x")]}}),
            Encoding::Utf8,
            &root,
            &[]
        )
        .is_err());
        assert!(prepare(json!({"documentChanges":[{"textDocument":{"uri":uri},"edits":[]},{"textDocument":{"uri":uri},"edits":[]}]}),Encoding::Utf8,&root,&[]).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected() {
        let (_dir, root, _doc) = fixture();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let link = root.join("escape.rs");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        assert!(prepare(
            json!({"changes":{text::uri(&link).unwrap():[edit(0,0,"x")]}}),
            Encoding::Utf8,
            &root,
            &[]
        )
        .is_err());
    }
    #[test]
    fn quick_fixes_keep_disabled_and_command_actions_unavailable() {
        let (_dir, root, doc) = fixture();
        let uri = text::uri(&doc.path).unwrap();
        let fixes=quick_fixes(json!([{"title":"Text fix","kind":"quickfix","edit":{"changes":{uri:[edit(0,1,"X")]}}},{"title":"Command","command":"do.not.run"},{"title":"Disabled","disabled":{"reason":"Unavailable"},"edit":{"changes":{}}},{"title":"Other","kind":"source.organizeImports"}]),Encoding::Utf16,&root,&[doc]).unwrap();
        assert_eq!(fixes.len(), 3);
        assert!(fixes[0].preview.is_some());
        assert!(fixes[1].preview.is_none());
        assert!(fixes[1].reason.as_ref().unwrap().contains("command"));
        assert_eq!(fixes[2].reason.as_deref(), Some("Unavailable"));
    }
    #[test]
    fn absent_edits_and_oversized_documents_fail_without_writes() {
        let (_dir, root, mut doc) = fixture();
        assert!(prepare(json!({}), Encoding::Utf8, &root, &[]).is_err());
        doc.text = "x".repeat(crate::document::SMALL_LIMIT as usize + 1);
        assert!(prepare(
            json!({"changes":{text::uri(&doc.path).unwrap():[edit(0,0,"x")]}}),
            Encoding::Utf8,
            &root,
            &[doc]
        )
        .is_err());
    }
}
