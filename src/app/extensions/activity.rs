//! Per-window command activity. The log accepts only invocation metadata.
use super::valid_command_id;
use eframe::egui;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const MAX_ACTIVITY: usize = 64;

pub(in crate::app) struct Invocation {
    command: String,
    started: Instant,
    scope: Scope,
    wait_started: std::cell::Cell<Option<Instant>>,
    pub(super) trace: crate::plugin_runtime::telemetry::Recorder,
}

#[derive(Clone, Copy)]
enum Scope {
    Selection,
    Document,
    DocumentAndStorage,
    Save,
    Provider,
    ScopedProvider,
    Broker,
}

impl Scope {
    fn label(self) -> &'static str {
        match self {
            Self::Selection => "Read and replace selection",
            Self::Document => "Read and edit current document",
            Self::DocumentAndStorage => "Current document and private command storage",
            Self::Save => "Read and replace document on save",
            Self::Provider => "Current-document provider callback",
            Self::ScopedProvider => "Scoped provider callback",
            Self::Broker => "Scoped workspace or integration command",
        }
    }
}

impl Invocation {
    pub(super) fn scoped_provider(command_id: &str) -> Self {
        Self {
            scope: Scope::ScopedProvider,
            ..Self::new(command_id)
        }
    }
    pub(in crate::app) fn broker(command_id: &str) -> Self {
        Self {
            scope: Scope::Broker,
            ..Self::new(command_id)
        }
    }
    pub(super) fn begin_wait(&self) {
        self.wait_started.set(Some(Instant::now()));
    }
    pub(super) fn end_wait(&self, phase: crate::plugin_runtime::telemetry::Phase) {
        if let Some(started) = self.wait_started.take() {
            self.trace.record(phase, started.elapsed());
        }
    }
    pub(in crate::app) fn new(command_id: &str) -> Self {
        Self {
            command: command_id.to_owned(),
            started: Instant::now(),
            scope: Scope::Selection,
            trace: Default::default(),
            wait_started: std::cell::Cell::new(None),
        }
    }

    pub(in crate::app) fn document(command_id: &str, private_storage: bool) -> Self {
        Self {
            scope: if private_storage {
                Scope::DocumentAndStorage
            } else {
                Scope::Document
            },
            ..Self::new(command_id)
        }
    }

    pub(in crate::app) fn provider(command_id: &str) -> Self {
        Self {
            scope: Scope::Provider,
            ..Self::new(command_id)
        }
    }
    pub(in crate::app) fn on_save(command_id: &str) -> Self {
        Self {
            scope: Scope::Save,
            ..Self::new(command_id)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum InvocationOutcome {
    Applied,
    Published,
    Discarded,
    Failed,
    LimitReached,
    Cancelled,
    Denied,
}

impl InvocationOutcome {
    pub(in crate::app) fn worker_failure(error: crate::plugin_runtime::Error) -> Self {
        if error.is_limit() {
            Self::LimitReached
        } else if error == crate::plugin_runtime::Error::Cancelled {
            Self::Cancelled
        } else {
            Self::Failed
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Applied => "Applied",
            Self::Published => "Result published",
            Self::Discarded => "Discarded",
            Self::Failed => "Failed",
            Self::LimitReached => "Limit reached",
            Self::Cancelled => "Canceled",
            Self::Denied => "Permission denied",
        }
    }
}

#[derive(Default)]
pub(super) struct Activity {
    recent: VecDeque<Run>,
}

struct Run {
    invocation: Invocation,
    duration: Duration,
    outcome: InvocationOutcome,
}

impl Activity {
    #[cfg(all(test, target_os = "macos"))]
    pub(in crate::app::extensions) fn measured(&self) -> Vec<serde_json::Value> {
        self.recent
            .iter()
            .map(|run| {
                serde_json::json!({
                    "duration_us":run.duration.as_micros(),
                    "outcome":run.outcome.label(),
                    "phases":run.invocation.trace.snapshot()
                })
            })
            .collect()
    }

    pub(super) fn record(&mut self, invocation: Invocation, outcome: InvocationOutcome) {
        if !valid_command_id(&invocation.command) {
            return;
        }
        if self.recent.len() == MAX_ACTIVITY {
            self.recent.pop_back();
        }
        self.recent.push_front(Run {
            duration: invocation.started.elapsed(),
            invocation,
            outcome,
        });
    }

    pub(super) fn show(&self, ui: &mut egui::Ui) {
        if self.recent.is_empty() {
            return;
        }
        ui.separator();
        ui.collapsing("Recent Lua activity", |ui| {
            ui.weak("This window · newest first");
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    for run in &self.recent {
                        ui.horizontal_wrapped(|ui| {
                            ui.monospace(&run.invocation.command);
                            ui.label(run.outcome.label());
                            ui.weak(format!(
                                "{} ms · {} · {}",
                                run.duration.as_millis(),
                                age(run.invocation.started.elapsed()),
                                run.invocation.scope.label(),
                            ));
                        });
                        let samples = run.invocation.trace.snapshot();
                        if !samples.is_empty() {
                            ui.collapsing("Phase timings", |ui| {
                                for sample in samples {
                                    ui.weak(format!(
                                        "{:?}: {} µs ({} samples)",
                                        sample.phase, sample.total_us, sample.count
                                    ));
                                }
                            });
                        }
                    }
                });
        });
    }
}

fn age(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    match seconds {
        0 => "just now".into(),
        1..=59 => format!("{seconds}s ago"),
        60..=3_599 => format!("{}m ago", seconds / 60),
        3_600..=86_399 => format!("{}h ago", seconds / 3_600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_keeps_only_the_latest_valid_command_ids_and_fixed_outcomes() {
        let mut activity = Activity::default();
        activity.record(Invocation::new("not a command"), InvocationOutcome::Failed);
        assert!(activity.recent.is_empty());
        for index in 0..=MAX_ACTIVITY {
            activity.record(
                Invocation::on_save(&format!("example.format-{index}")),
                InvocationOutcome::Applied,
            );
        }
        assert_eq!(activity.recent.len(), MAX_ACTIVITY);
        assert_eq!(activity.recent[0].invocation.command, "example.format-64");
        assert_eq!(activity.recent[63].invocation.command, "example.format-1");
        assert_eq!(activity.recent[0].outcome, InvocationOutcome::Applied);
        assert_eq!(
            InvocationOutcome::worker_failure(crate::plugin_runtime::Error::Deadline),
            InvocationOutcome::LimitReached
        );
        assert_eq!(
            InvocationOutcome::worker_failure(crate::plugin_runtime::Error::Execution),
            InvocationOutcome::Failed
        );
        assert_eq!(
            InvocationOutcome::worker_failure(crate::plugin_runtime::Error::Cancelled),
            InvocationOutcome::Cancelled
        );
    }
}
