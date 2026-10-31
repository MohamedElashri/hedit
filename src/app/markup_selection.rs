use super::*;

impl HeditApp {
    pub(super) fn expand_markup_selection(&mut self, ctx: &egui::Context) {
        let buffer = self.active();
        let view = buffer.view.id;
        let revision = buffer.page.info.revision;
        let html = buffer.language() == syntax::Language::Html;
        let text = buffer.page.text.clone();
        let before = self.selections();
        let cancel = Arc::clone(&self.cancel);
        self.job(ctx, move || {
            let after = hedit::markup::expand(&text, &before, html, &cancel)?;
            Ok(Action::Quality(Box::new(quality::Event::TagSelections(
                view, revision, before, after,
            ))))
        });
    }
}
