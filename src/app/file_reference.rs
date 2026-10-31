use super::*;
use hedit::{advanced::Selection, file_reference};

impl HeditApp {
    pub(super) fn open_file_reference(&mut self, ctx: &egui::Context) {
        let b = self.active();
        let Some(reference) = file_reference::at(
            &b.page.text,
            Selection {
                anchor: b.view.anchor,
                head: b.view.head,
            },
        ) else {
            self.notice = "Place the cursor in a file path, or select a single-line path.".into();
            return;
        };
        let view = b.view.id;
        let document = b.page.info.path.as_ref().map(PathBuf::from);
        let roots = self
            .workspace
            .folders
            .iter()
            .map(|f| PathBuf::from(&f.path))
            .collect::<Vec<_>>();
        self.job(ctx, move || {
            let found = file_reference::resolve(&reference, document.as_deref(), &roots);
            Ok(Action::Quality(Box::new(quality::Event::FileReferences(
                view, document, reference, found,
            ))))
        });
    }

    pub(super) fn show_file_references(
        &mut self,
        ctx: &egui::Context,
        view: u64,
        document: Option<PathBuf>,
        reference: String,
        found: Vec<PathBuf>,
    ) {
        if self.active().view.id != view
            || self.active().page.info.path.as_ref().map(PathBuf::from) != document
        {
            return;
        }
        match found.len() {
            0 => self.notice = format!("No existing file found for “{reference}”."),
            1 => self.open_existing(ctx, found.into_iter().next().unwrap()),
            _ => {
                let items = found
                    .into_iter()
                    .map(|path| {
                        let label = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        let detail = path.display().to_string();
                        (label, detail, commands::Pick::ExistingFile(path))
                    })
                    .collect();
                self.open_pick_list(
                    ctx,
                    "Open file under cursor",
                    "Filter matching paths…",
                    items,
                );
            }
        }
    }
}
