//! Hold incoming buffer data during commit; reopen deferred disk reads off the UI.
use crate::app::{opening, Action, HeditApp};
use eframe::egui;
pub(in crate::app) struct Deferred {
    pub event: Result<Action, String>,
    pub request: Option<opening::Request>,
    pub refresh: bool,
}

impl HeditApp {
    pub(in crate::app) fn defer_broker_event(&self, event: &Result<Action, String>) -> bool {
        match event {
            Ok(Action::Add(..) | Action::AddTemplate(..) | Action::ReopenRead(..)) => {
                self.workspace.editor.workspace_commit_pending()
            }
            Ok(
                Action::Page(id, ..)
                | Action::Reloaded(id, ..)
                | Action::History(id, ..)
                | Action::Saved(id, ..)
                | Action::Discarded(id, ..),
            ) => self.workspace.editor.workspace_document_frozen(*id),
            _ => false,
        }
    }
    pub(in crate::app) fn reload_broker_deferred(
        &mut self,
        ctx: &egui::Context,
        deferred: Deferred,
    ) -> Option<Deferred> {
        if !deferred.refresh
            || !matches!(&deferred.event, Ok(Action::Add(document, ..)) if document.local_path().is_some() && !document.info().dirty)
        {
            return Some(deferred);
        }
        // Only one deferred disk read runs per window. The receive loop keeps
        // the remaining bounded events queued until its completion arrives.
        let sender = self.tx.clone();
        let wake = ctx.clone();
        self.broker_deferred_running = true;
        let shared = std::sync::Arc::new(std::sync::Mutex::new(Some(deferred)));
        let pending = shared.clone();
        let result = std::thread::Builder::new()
            .name("hedit-open-recheck".into())
            .spawn(move || {
                let Some(mut deferred) = pending.lock().unwrap_or_else(|p| p.into_inner()).take()
                else {
                    return;
                };
                let reloaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if let Ok(Action::Add(document, page, _)) = &mut deferred.event {
                        match document.reload(page.start) {
                            Ok(fresh) => *page = fresh,
                            Err(error) => deferred.event = Err(error),
                        }
                    }
                }));
                if reloaded.is_err() {
                    deferred.event = Err("The deferred file read stopped.".into());
                }
                deferred.refresh = false;
                let _ = sender.send(Ok(Action::ExtensionDeferred(Box::new(deferred))));
                wake.request_repaint();
            });
        if result.is_err() {
            self.broker_deferred_running = false;
            self.error = Some("The deferred file read could not start. Try again.".into());
            if let Some(deferred) = shared.lock().unwrap_or_else(|p| p.into_inner()).take() {
                self.broker_deferred.push_front(deferred);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests;
