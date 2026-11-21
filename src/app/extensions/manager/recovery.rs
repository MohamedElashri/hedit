//! Trusted, explicit recovery; opening settings never resumes workspace effects.
use super::*;
use crate::app::extensions::{broker, presentation};
use hedit_broker::{
    journal::{Current, Journal, View as JournalView},
    recovery_store::Record,
    state::{SelectedScope, State},
    workspace::{Budget, Right},
};
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
static JOBS: AtomicUsize = AtomicUsize::new(0);
struct Job;
impl Job {
    fn reserve() -> Result<Self, String> {
        let mut used = JOBS.load(Ordering::Acquire);
        loop {
            if used >= 2 {
                return Err("Another recovery operation is in progress. Try again.".into());
            }
            match JOBS.compare_exchange_weak(used, used + 1, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Ok(Self),
                Err(actual) => used = actual,
            }
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        JOBS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Resume,
    Restore,
    Discard,
    DiscardUnprepared,
}
pub(super) enum Request {
    List,
    Choose(Record),
    Execute(Operation),
    Close,
}
pub(super) enum Review {
    List(Vec<Record>),
    Selected {
        record: Box<Record>,
        scope: SelectedScope,
        view: Option<Arc<JournalView>>,
        error: Option<String>,
        created: Instant,
        file: usize,
        before: usize,
        after: usize,
    },
}
impl Review {
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        ready: bool,
        feedback: Option<&str>,
    ) -> Option<Request> {
        let shown = egui::Modal::new(egui::Id::new("extension-workspace-recovery"))
            .show(ui.ctx(), |ui| self.contents(ui, ready, feedback));
        if shown.should_close() {
            Some(Request::Close)
        } else {
            shown.inner
        }
    }
    fn contents(
        &mut self,
        ui: &mut egui::Ui,
        ready: bool,
        feedback: Option<&str>,
    ) -> Option<Request> {
        let mut request = None;
        ui.set_width(640.0);
        ui.heading("Workspace recovery");
        if let Some(error) = feedback {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.label("Review retained edits before resuming, restoring originals, or deleting recovery history. These actions do not run plugin code.");
        match self {
            Self::List(records) => {
                if records.is_empty() {
                    ui.label("No retained workspace edits.");
                }
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        for record in records {
                            ui.horizontal(|ui| {
                                ui.monospace(record.id.to_string());
                                if ui
                                    .add_enabled(
                                        ready,
                                        egui::Button::new("Choose original directory…"),
                                    )
                                    .clicked()
                                {
                                    request = Some(Request::Choose(record.clone()));
                                }
                            });
                        }
                    });
            }
            Self::Selected {
                record,
                scope,
                view,
                error,
                created,
                file,
                before,
                after,
            } => {
                path_label(ui, "Selected directory", scope.path());
                ui.monospace(record.id.to_string());
                if let Some(error) = error {
                    ui.colored_label(ui.visuals().error_fg_color, error.as_str());
                    ui.label("Data is preserved. Cleanup below succeeds only for a verified incomplete preparation containing no captured originals or ledger. Damaged ledgers require manual inspection.");
                    if ui
                        .add_enabled(ready, egui::Button::new("Clean up incomplete preparation"))
                        .clicked()
                    {
                        request = Some(Request::Execute(Operation::DiscardUnprepared));
                    }
                }
                if let Some(view) = view {
                    if let Some(owner) = &view.owner {
                        ui.strong(&owner.command);
                        ui.monospace(format!("Source digest: {}", owner.digest));
                    } else {
                        ui.weak("Legacy journal: source attribution was not recorded.");
                    }
                    *file = (*file).min(view.files.len() - 1);
                    egui::ComboBox::from_id_salt("recovery-file")
                        .selected_text(broker::display_path(&view.files[*file].relative))
                        .show_ui(ui, |ui| {
                            for (index, item) in view.files.iter().enumerate() {
                                if ui
                                    .selectable_value(
                                        file,
                                        index,
                                        broker::display_path(&item.relative),
                                    )
                                    .clicked()
                                {
                                    *before = 0;
                                    *after = 0;
                                }
                            }
                        });
                    let item = &view.files[*file];
                    ui.label(match item.current {
                        Current::Original => "Current file: original",
                        Current::Replacement => "Current file: replacement",
                        Current::Missing => "Current file: missing; original retained",
                        Current::Conflict => "Current file: conflicting external changes",
                    });
                    ui.label("Original");
                    let (text, count) = item.before_page(*before);
                    presentation::indexed_page(ui, text, before, count);
                    ui.label("Replacement");
                    let (text, count) = item.after_page(*after);
                    presentation::indexed_page(ui, text, after, count);
                    let live = ready && created.elapsed() < Duration::from_secs(120);
                    let conflict = view.files.iter().any(|f| f.current == Current::Conflict);
                    ui.add_enabled_ui(live && !conflict, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    view.files.iter().all(|f| f.can_resume),
                                    egui::Button::new("Resume reviewed edit"),
                                )
                                .clicked()
                            {
                                request = Some(Request::Execute(Operation::Resume));
                            }
                            if ui.button("Restore originals").clicked() {
                                request = Some(Request::Execute(Operation::Restore));
                            }
                            if ui
                                .add_enabled(
                                    view.files.iter().all(|f| f.current != Current::Missing),
                                    egui::Button::new("Delete recovery history"),
                                )
                                .clicked()
                            {
                                request = Some(Request::Execute(Operation::Discard));
                            }
                        });
                    });
                    ui.weak("Deleting history keeps current files and removes retained versions. Resume and restore preserve history until you explicitly delete it.");
                    if !live && ready {
                        ui.label("Preview expired. Choose the directory again to refresh it.");
                    }
                }
                if ui
                    .add_enabled(ready, egui::Button::new("Choose directory again…"))
                    .clicked()
                {
                    request = Some(Request::Choose(record.as_ref().clone()));
                }
            }
        }
        if ui.add_enabled(ready, egui::Button::new("Close")).clicked() {
            request = Some(Request::Close);
        }
        request
    }
}

pub(super) struct Context {
    pub editor: Arc<hedit::Editor>,
    pub session: PathBuf,
    pub following: BTreeSet<u64>,
    pub sender: std::sync::mpsc::Sender<Result<crate::app::Action, String>>,
}
pub(super) fn start(
    manager: &mut Manager,
    request: Request,
    ctx: &egui::Context,
    config: PathBuf,
    context: Context,
) {
    if matches!(request, Request::Close) {
        manager.recovery = None;
        if let Some(cancel) = manager.recovery_cancel.take() {
            cancel.store(true, Ordering::Release);
        }
        return;
    }
    let job = match Job::reserve() {
        Ok(job) => job,
        Err(error) => {
            manager.feedback = Some(Feedback::Error(error));
            return;
        }
    };
    let selected = match (&request, &manager.recovery) {
        (
            Request::Execute(operation),
            Some(Review::Selected {
                record,
                scope,
                view,
                created,
                ..
            }),
        ) => {
            if created.elapsed() >= Duration::from_secs(120) {
                manager.feedback = Some(Feedback::Error(
                    "Recovery preview expired. Choose the directory again.".into(),
                ));
                return;
            }
            Some((*operation, record.clone(), scope.clone(), view.clone()))
        }
        (Request::Execute(_), _) => return,
        _ => None,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    manager.recovery_cancel = Some(cancel.clone());
    let wake = ctx.clone();
    manager.work(ctx, move || {
        let _job = job;
        run(request, selected, config, context, cancel, wake).map_err(|error| {
            format!("{error} Recovery data was preserved unless cleanup completed. Refresh the review before retrying.")
        })
    });
}
type Selection = (
    Operation,
    Box<Record>,
    SelectedScope,
    Option<Arc<JournalView>>,
);
fn run(
    request: Request,
    selected: Option<Selection>,
    config: PathBuf,
    context: Context,
    cancel: Arc<AtomicBool>,
    wake: egui::Context,
) -> Result<Update, String> {
    let state = State::open(&config, [context.session]).map_err(|e| e.to_string())?;
    let store = state.recovery();
    let mut budget = Budget::new(&cancel, Duration::from_secs(10), 64 * 1024 * 1024);
    match request {
        Request::List => Ok(Update::Recovery(Review::List(
            store.records().map_err(|e| e.to_string())?,
        ))),
        Request::Choose(record) => {
            let Some(path) = rfd::FileDialog::new()
                .set_title("Choose the original workspace directory")
                .pick_folder()
            else {
                return Ok(Update::FolderOpened);
            };
            let mut budget = Budget::new(&cancel, Duration::from_secs(10), 64 * 1024 * 1024);
            budget.check().map_err(|e| e.to_string())?;
            let scope = SelectedScope::select(&path, vec![Right::Read, Right::Edit], &state)
                .map_err(|e| e.to_string())?;
            if scope.identity() != record.root {
                return Err("This is not the original physical workspace directory.".into());
            }
            let root = scope.resolve(&state).map_err(|e| e.to_string())?;
            let inspected = Journal::open(&root, &record.stage_name, &store)
                .and_then(|journal| journal.inspect(&root, &mut budget));
            let (view, error) = match inspected {
                Ok(view) => (Some(Arc::new(view)), None),
                Err(error) => (None, Some(error.to_string())),
            };
            Ok(Update::Recovery(Review::Selected {
                record: Box::new(record),
                scope,
                view,
                error,
                created: Instant::now(),
                file: 0,
                before: 0,
                after: 0,
            }))
        }
        Request::Execute(_) => {
            let (operation, record, scope, view) =
                selected.ok_or("No recovery preview selected.")?;
            // Cancel all extension sessions before taking the exclusive
            // policy guard, and retain it through reconciliation.
            let _policy = catalog::recovery_lock(&config)?;
            let root = scope.resolve(&state).map_err(|e| e.to_string())?;
            budget.check().map_err(|e| e.to_string())?;
            if matches!(operation, Operation::DiscardUnprepared) {
                Journal::discard_unprepared(&root, record.id, &store, &mut budget)
                    .map_err(|e| e.to_string())?;
            } else {
                let expected = view.ok_or("The recovery preview is unavailable.")?;
                let journal =
                    Journal::open(&root, &record.stage_name, &store).map_err(|e| e.to_string())?;
                let actual = journal
                    .inspect(&root, &mut budget)
                    .map_err(|e| e.to_string())?;
                if !same_view(&expected, &actual) {
                    return Err(
                        "Workspace recovery changed. Review it again before continuing.".into(),
                    );
                }
                let _buffers = broker::commit::prepare_recovery(
                    &context.editor,
                    expected,
                    scope.path().to_owned(),
                    &cancel,
                    context.sender,
                    wake,
                    &context.following,
                )
                .map_err(|e| e.to_string())?;
                budget.check().map_err(|e| e.to_string())?;
                match operation {
                    Operation::Resume => {
                        journal
                            .apply(&root, &mut budget, || Ok(()))
                            .map_err(|e| e.to_string())?;
                    }
                    Operation::Restore => {
                        journal
                            .restore(&root, &mut budget, || Ok(()))
                            .map_err(|e| e.to_string())?;
                    }
                    Operation::Discard => journal
                        .discard(&root, &store, &mut budget)
                        .map_err(|e| e.to_string())?,
                    Operation::DiscardUnprepared => unreachable!(),
                }
            }
            Ok(Update::Recovery(Review::List(
                store.records().map_err(|e| e.to_string())?,
            )))
        }
        Request::Close => unreachable!(),
    }
}

fn same_view(expected: &JournalView, actual: &JournalView) -> bool {
    expected.root == actual.root
        && expected.stage == actual.stage
        && expected.owner.as_ref().map(|o| (&o.command, &o.digest))
            == actual.owner.as_ref().map(|o| (&o.command, &o.digest))
        && expected.files.len() == actual.files.len()
        && expected.files.iter().zip(&actual.files).all(|(a, b)| {
            a.relative == b.relative
                && a.original == b.original
                && a.replacement == b.replacement
                && a.current == b.current
                && a.can_resume == b.can_resume
                && a.before() == b.before()
                && a.after() == b.after()
        })
}

#[cfg(test)]
mod tests;
