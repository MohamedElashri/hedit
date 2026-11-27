use super::*;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Key {
    id: u64,
    revision: u64,
    language: syntax::Language,
    syntax_override: Option<String>,
    path: PathBuf,
}

#[derive(Default)]
pub(in crate::app) struct State {
    requested: Option<Key>,
    result: Option<(Key, Arc<Vec<text::InlayHint>>)>,
}

impl State {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(in crate::app) fn get(&self, buffer: &Buffer) -> Option<Arc<Vec<text::InlayHint>>> {
        let (key, hints) = self.result.as_ref()?;
        key.matches(buffer).then(|| hints.clone())
    }
}

impl Key {
    fn matches(&self, buffer: &Buffer) -> bool {
        buffer.view.id == self.id
            && buffer.page.info.revision == self.revision
            && buffer.language() == self.language
            && buffer.view.language_override == self.syntax_override
            && !buffer.follow
            && !buffer.page.info.paged
            && buffer
                .page
                .info
                .path
                .as_deref()
                .is_some_and(|path| Path::new(path) == self.path)
    }
}

impl HeditApp {
    pub(super) fn request_inlays(&mut self) {
        if !self.settings.show_inlay_hints
            || self.workspace.remote.is_some()
            || self.busy
            || self.closing
            || self.languages.pending.values().any(|intent| {
                matches!(intent, Intent::Feature(_) | Intent::Sync | Intent::Inlay(_))
            })
        {
            return;
        }
        let buffer = self.active();
        if self
            .languages
            .inlay
            .requested
            .as_ref()
            .is_some_and(|key| key.matches(buffer))
        {
            return;
        }
        let Some(path) = buffer.page.info.path.as_deref().map(Path::new) else {
            return;
        };
        let Some(snapshot) = self.languages.snapshots.iter().find(|snapshot| {
            snapshot.id == buffer.view.id
                && snapshot.revision == buffer.page.info.revision
                && snapshot.path == path
        }) else {
            return;
        };
        if buffer.follow
            || buffer.page.info.paged
            || buffer.page.text.len() > 512 * 1024
            || self
                .languages
                .observed
                .as_ref()
                .is_some_and(|(documents, _)| {
                    documents.iter().find(|document| document.id == snapshot.id) != Some(snapshot)
                })
            || !self
                .languages
                .statuses
                .values()
                .any(|status| status.state == "Running" && path.starts_with(&status.root))
        {
            return;
        }
        let key = Key {
            id: buffer.view.id,
            revision: buffer.page.info.revision,
            language: buffer.language(),
            syntax_override: buffer.view.language_override.clone(),
            path: path.to_owned(),
        };
        let op = Operation::Request {
            path: key.path.clone(),
            revision: key.revision,
            scalar: 0,
            feature: Feature::InlayHints,
            tab_size: 4,
            spaces: true,
        };
        if self.languages.send(op, Intent::Inlay(key.clone())).is_ok() {
            self.languages.inlay.requested = Some(key);
        }
    }

    pub(super) fn receive_inlays(&mut self, key: Key, reply: Result<lsp::Reply, String>) {
        let hints = reply
            .ok()
            .filter(|reply| {
                reply.path == key.path
                    && reply.revision == key.revision
                    && reply.feature == Feature::InlayHints
            })
            .and_then(|reply| serde_json::from_value::<Vec<text::InlayHint>>(reply.value).ok());
        if let Some(hints) = hints {
            self.languages.inlay.result = Some((key, Arc::new(hints)));
        }
    }
}
