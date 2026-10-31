//! Switch between headers and implementations using the shared command and picker.
use super::*;

impl HeditApp {
    pub(super) fn switch_counterpart(&mut self, ctx: &egui::Context) {
        let Some(path) = self.active().page.info.path.as_ref().map(PathBuf::from) else {
            return;
        };
        let view = self.active().view.id;
        let roots = self
            .workspace
            .folders
            .iter()
            .map(|f| PathBuf::from(&f.path))
            .collect::<Vec<_>>();
        let cancel = Arc::clone(&self.cancel);
        self.job(ctx, move || {
            let found = hedit::counterpart::find(&path, &roots, &cancel)?;
            Ok(Action::Quality(Box::new(quality::Event::Counterparts(
                view, path, found,
            ))))
        });
    }

    pub(super) fn show_counterparts(
        &mut self,
        ctx: &egui::Context,
        view: u64,
        source: PathBuf,
        found: Vec<PathBuf>,
    ) {
        // A tab switch, rename, or project switch invalidates this request.
        if self.active().view.id != view
            || self.active().page.info.path.as_ref().map(PathBuf::from) != Some(source.clone())
        {
            return;
        }
        match found.len() {
            0 => {
                self.notice = format!(
                    "No header or implementation found for {}.",
                    source.display()
                )
            }
            // Normal opening reuses existing buffers, including unsaved edits.
            1 => self.open_existing(ctx, found.into_iter().next().unwrap()),
            _ => {
                let items = found
                    .into_iter()
                    .map(|path| {
                        let name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        let detail = path.display().to_string();
                        (name, detail, commands::Pick::ExistingFile(path))
                    })
                    .collect();
                self.open_pick_list(
                    ctx,
                    "Switch header/implementation",
                    "Filter matching files…",
                    items,
                );
            }
        }
    }
}
