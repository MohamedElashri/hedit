//! Settings for installing local extensions and approving individual Lua commands.
use super::catalog::{
    self, Catalog, ImportPreview, InstallChange, InstallReview, InstalledManifest, ManifestStatus,
    View,
};
use crate::app::{preferences::PreferencesPage, settings_scope::Scope, HeditApp};
use eframe::egui;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

#[derive(Default)]
pub(in crate::app) struct Manager {
    view: Option<View>,
    review: Option<Review>,
    pending: Option<Receiver<Result<Update, String>>>,
    feedback: Option<Feedback>,
    discard_file_review: bool,
}

enum Feedback {
    Success(&'static str),
    Error(String),
}

enum Update {
    Catalog {
        catalog: Catalog,
        feedback: Option<Feedback>,
    },
    File(Option<InstallReview>),
    FolderOpened,
}

enum Request {
    Reload,
    OpenFolder,
    ChooseFile(Option<String>),
    Review(Review),
    Confirm,
    SetEnabled(String, bool),
    Revoke(Approval),
}

enum Review {
    Install(InstallReview),
    Approve(Approval),
    Uninstall(InstalledManifest),
    ResetApprovals,
}

struct Approval {
    path: PathBuf,
    extension: String,
    command: String,
    title: String,
    digest: String,
}

impl Manager {
    fn hide(&mut self) {
        self.view = None;
        self.review = None;
        self.feedback = None;
        self.discard_file_review = true;
    }

    fn work(
        &mut self,
        ctx: &egui::Context,
        task: impl FnOnce() -> Result<Update, String> + Send + 'static,
    ) {
        let (sender, receiver) = mpsc::channel();
        // Wake the UI after receiving the result or a dropped worker.
        struct Wake(egui::Context);
        impl Drop for Wake {
            fn drop(&mut self) {
                self.0.request_repaint();
            }
        }
        let ctx = ctx.clone();
        match thread::Builder::new()
            .name("hedit-extensions".into())
            .spawn(move || {
                let _wake = Wake(ctx);
                let _ = sender.send(task());
            }) {
            Ok(_) => self.pending = Some(receiver),
            Err(error) => self.feedback = Some(Feedback::Error(error.to_string())),
        }
    }

    fn reload(&mut self, ctx: &egui::Context, config: PathBuf, manual: bool) {
        self.work(ctx, move || {
            Ok(Update::Catalog {
                catalog: catalog::read(&config),
                feedback: manual.then_some(Feedback::Success("Extensions reloaded.")),
            })
        });
    }

    fn mutate(
        &mut self,
        ctx: &egui::Context,
        config: PathBuf,
        success: &'static str,
        action: impl FnOnce(&Path) -> Result<Catalog, String> + Send + 'static,
    ) {
        self.work(ctx, move || {
            let (catalog, feedback) = match action(&config) {
                Ok(catalog) => (catalog, Feedback::Success(success)),
                // An on-disk change can make the review stale, or a failed
                // update can leave its prior approvals revoked.
                Err(error) => (catalog::read(&config), Feedback::Error(error)),
            };
            Ok(Update::Catalog {
                catalog,
                feedback: Some(feedback),
            })
        });
    }

    fn show(&mut self, ui: &mut egui::Ui, config: &Path) -> Option<Request> {
        if self.view.is_none() && self.pending.is_none() && self.feedback.is_none() {
            self.reload(ui.ctx(), config.to_owned(), false);
        }

        let pending = self.pending.is_some();
        let can_act = !pending && self.review.is_none();
        let mut request = None;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(can_act, egui::Button::new("Install from file…"))
                .clicked()
            {
                request = Some(Request::ChooseFile(None));
            }
            if ui
                .add_enabled(can_act, egui::Button::new("Reload"))
                .clicked()
            {
                request = Some(Request::Reload);
            }
            if self
                .view
                .as_ref()
                .is_some_and(|view| !view.files.is_empty())
                && ui
                    .add_enabled(can_act, egui::Button::new("Open extensions folder"))
                    .clicked()
            {
                request = Some(Request::OpenFolder);
            }
            if pending {
                ui.spinner();
                ui.weak("Working…");
            }
        });

        match &self.feedback {
            Some(Feedback::Error(error)) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            Some(Feedback::Success(status)) => {
                ui.label(*status);
            }
            None => {}
        }

        if let Some(view) = &self.view {
            for notice in &view.notices {
                ui.colored_label(ui.visuals().error_fg_color, notice);
            }
            if view.grants_unreadable
                && ui
                    .add_enabled(can_act, egui::Button::new("Reset Lua approvals…"))
                    .clicked()
            {
                request = Some(Request::Review(Review::ResetApprovals));
            }
            if view.files.is_empty() {
                ui.label("No extensions installed.");
            }
            for entry in &view.files {
                ui.push_id(&entry.path, |ui| {
                    ui.separator();
                    if let Some(action) = installed_row(ui, entry, view, can_act) {
                        request = Some(action);
                    }
                });
            }
        }

        if let Some(review) = &self.review {
            let shown = egui::Modal::new(egui::Id::new(("review-extension", review.label())))
                .show(ui.ctx(), |ui| review.show(ui, !pending));
            if shown.should_close() || shown.inner == Some(ReviewChoice::Cancel) {
                self.review = None;
            } else if shown.inner == Some(ReviewChoice::Confirm) {
                return Some(Request::Confirm);
            }
        }
        request
    }
}

fn installed_row(
    ui: &mut egui::Ui,
    entry: &ManifestStatus,
    view: &View,
    can_act: bool,
) -> Option<Request> {
    let mut request = None;
    ui.horizontal_wrapped(|ui| {
        let id = match &entry.details {
            Ok(details) => Cow::Borrowed(details.id.as_str()),
            Err(_) => entry
                .path
                .file_name()
                .unwrap_or(entry.path.as_os_str())
                .to_string_lossy(),
        };
        ui.strong(id)
            .on_hover_text(entry.path.display().to_string());
        if let Ok(details) = &entry.details {
            let mut enabled = details.enabled;
            let can_toggle = can_act && !view.state_unreadable;
            let checkbox = ui.add_enabled(can_toggle, egui::Checkbox::new(&mut enabled, "Enabled"));
            let checkbox = if details.lua.is_some() {
                checkbox.on_hover_text("Disabling this extension revokes its Lua approvals.")
            } else {
                checkbox
            };
            if checkbox.changed() {
                request = Some(Request::SetEnabled(details.id.clone(), enabled));
            }
            if ui
                .add_enabled(
                    can_act && entry.installed.is_some(),
                    egui::Button::new("Update…"),
                )
                .clicked()
            {
                request = Some(Request::ChooseFile(Some(details.id.clone())));
            }
        }
        if let Some(installed) = &entry.installed {
            if ui
                .add_enabled(can_act, egui::Button::new("Uninstall…"))
                .clicked()
            {
                request = Some(Request::Review(Review::Uninstall(installed.clone())));
            }
        }
    });

    let details = match &entry.details {
        Ok(details) => details,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            return request;
        }
    };
    let supported = crate::plugin_runtime::SUPPORTED;
    if details.lua.is_some() && !supported {
        ui.weak("Lua execution is unavailable on this platform.");
    }
    egui::CollapsingHeader::new("Commands")
        .default_open(details.lua.is_some())
        .show(ui, |ui| {
            for (id, title) in &details.commands {
                ui.push_id(id, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        command_label(ui, id, title);
                        let Some(lua) = &details.lua else { return };
                        let Some(command) = lua.commands.iter().find(|command| command.id == *id)
                        else {
                            return;
                        };
                        let enabled = can_act
                            && !view.grants_unreadable
                            && (command.approved
                                || (supported && details.enabled && !view.state_unreadable));
                        if command.approved {
                            ui.weak("Approved");
                        }
                        let label = if command.approved {
                            "Revoke approval"
                        } else {
                            "Review permissions…"
                        };
                        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                            let approval = Approval {
                                path: entry.path.clone(),
                                extension: details.id.clone(),
                                command: command.id.clone(),
                                title: command.title.clone(),
                                digest: lua.digest.clone(),
                            };
                            request = Some(if command.approved {
                                Request::Revoke(approval)
                            } else {
                                Request::Review(Review::Approve(approval))
                            });
                        }
                    });
                });
            }
        });
    request
}

#[derive(PartialEq)]
enum ReviewChoice {
    Confirm,
    Cancel,
}

impl Review {
    fn label(&self) -> &'static str {
        match self {
            Self::Install(review) => match review.change {
                InstallChange::New => "Install extension",
                InstallChange::Update { .. } => "Update extension",
                InstallChange::Unchanged => "Already installed",
            },
            Self::Approve(_) => "Approve Lua command",
            Self::Uninstall(_) => "Uninstall extension",
            Self::ResetApprovals => "Reset Lua approvals",
        }
    }

    fn show(&self, ui: &mut egui::Ui, ready: bool) -> Option<ReviewChoice> {
        ui.set_width((ui.ctx().screen_rect().width() - 56.0).clamp(200.0, 520.0));
        ui.heading(self.label());
        let body_height = (ui.ctx().screen_rect().height() - 132.0).clamp(72.0, 390.0);
        let confirm = egui::ScrollArea::vertical()
            .max_height(body_height)
            .show(ui, |ui| self.details(ui))
            .inner;
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if confirm.is_some() { "Cancel" } else { "Close" })
                .clicked()
            {
                return Some(ReviewChoice::Cancel);
            }
            if let Some(confirm) = confirm {
                if ui.add_enabled(ready, egui::Button::new(confirm)).clicked() {
                    return Some(ReviewChoice::Confirm);
                }
            }
            None
        })
        .inner
    }
    fn details(&self, ui: &mut egui::Ui) -> Option<&'static str> {
        match self {
            Self::Install(review) => {
                ui.strong(&review.preview.id);
                path_label(ui, "From", &review.preview.source);
                if let InstallChange::Update {
                    installed,
                    previous_commands,
                } = &review.change
                {
                    path_label(ui, "Replacing", &installed.path);
                    ui.label("Updating clears existing Lua approvals.");
                    egui::CollapsingHeader::new("Currently installed commands").show(ui, |ui| {
                        command_list(ui, previous_commands);
                    });
                }
                let button = match review.change {
                    InstallChange::New => Some("Install"),
                    InstallChange::Update { .. } => Some("Update"),
                    InstallChange::Unchanged => None,
                };
                if button.is_some() {
                    ui.label("Commands in this manifest");
                    command_list(ui, &review.preview.commands);
                    if review.preview.lua {
                        requested_permissions(ui);
                        ui.weak("Approve each Lua command separately after installation.");
                    }
                } else {
                    ui.label("This exact manifest is already installed.");
                }
                button
            }
            Self::Approve(approval) => {
                ui.strong(&approval.title);
                ui.monospace(&approval.command);
                path_label(ui, "Manifest", &approval.path);
                ui.label("Approval applies to this command and these exact manifest contents.");
                requested_permissions(ui);
                ui.collapsing("Manifest SHA-256", |ui| {
                    ui.monospace(&approval.digest);
                });
                Some("Approve command")
            }
            Self::Uninstall(installed) => {
                if let Some(id) = &installed.id {
                    ui.strong(id);
                }
                path_label(ui, "Installed manifest", &installed.path);
                ui.label("Remove the installed manifest and clear its Lua approvals.");
                Some("Uninstall")
            }
            Self::ResetApprovals => {
                ui.label("Back up the unreadable approval file in the configuration folder. Every Lua command will need fresh approval.");
                Some("Reset approvals")
            }
        }
    }
}

fn requested_permissions(ui: &mut egui::Ui) {
    ui.strong("Requested permissions");
    ui.label("Read the selected text in a local editor.");
    ui.label("Replace that selection when this command is run.");
}

fn path_label(ui: &mut egui::Ui, label: &str, path: &Path) {
    let display = path.display().to_string();
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::Label::new(&display).truncate())
            .on_hover_text(display);
    });
}

fn command_label(ui: &mut egui::Ui, id: &str, title: &str) {
    ui.label(title);
    ui.weak(egui::RichText::new(id).monospace());
}

fn command_list(ui: &mut egui::Ui, commands: &[(String, String)]) {
    for (id, title) in commands {
        ui.horizontal_wrapped(|ui| command_label(ui, id, title));
    }
}

impl Approval {
    fn change(self, config: &Path, approved: bool) -> Result<Catalog, String> {
        catalog::set_lua_approved(
            config,
            &self.path,
            &self.extension,
            &self.command,
            &self.digest,
            approved,
        )
    }
}

impl HeditApp {
    pub(in crate::app) fn refresh_extensions_on_focus(&mut self, ctx: &egui::Context) {
        let manager = &mut self.extension_manager;
        if manager.pending.is_none() && manager.review.is_none() {
            if let Some(config) = self.settings_files.dir.clone() {
                manager.reload(ctx, config, false);
            }
        }
    }

    pub(in crate::app) fn poll_extension_manager(&mut self) {
        let showing = self.preferences_open
            && self.preferences_page == PreferencesPage::Extensions
            && self.settings_scope.scope == Scope::User
            && !self.settings_search_active();
        let manager = &mut self.extension_manager;
        if !showing {
            manager.hide();
        }
        let Some(receiver) = &manager.pending else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("The extension operation stopped.".into()),
        };
        manager.pending = None;
        match result {
            Ok(Update::Catalog { catalog, feedback }) => {
                self.extensions = catalog.contributions;
                self.chord = None;
                if showing {
                    manager.view = Some(catalog.view);
                    if feedback.is_some() {
                        manager.feedback = feedback;
                    }
                }
            }
            Ok(Update::File(review)) => {
                if showing && !manager.discard_file_review {
                    manager.review = review.map(Review::Install);
                }
            }
            Ok(Update::FolderOpened) => {}
            Err(error) => {
                if showing {
                    manager.feedback = Some(Feedback::Error(error));
                }
            }
        }
    }

    pub(in crate::app) fn extension_preferences(&mut self, ui: &mut egui::Ui) {
        let Some(config) = self.settings_files.dir.clone() else {
            ui.label("Extension management requires a user configuration folder.");
            return;
        };
        let manager = &mut self.extension_manager;
        let request = manager.show(ui, &config);
        let ctx = ui.ctx();
        if request.is_some() {
            manager.feedback = None;
        }
        match request {
            Some(Request::Reload) => manager.reload(ctx, config, true),
            Some(Request::OpenFolder) => manager.work(ctx, move || {
                let folder = config.join("extensions");
                hedit::folder::reveal_in_file_manager(folder.to_string_lossy().into_owned())?;
                Ok(Update::FolderOpened)
            }),
            Some(Request::ChooseFile(expected)) => {
                manager.discard_file_review = false;
                manager.work(ctx, move || {
                    let picked = rfd::FileDialog::new()
                        .set_title(if expected.is_some() {
                            "Choose an updated extension manifest"
                        } else {
                            "Install extension from file"
                        })
                        .add_filter("Extension manifest", &["json"])
                        .pick_file();
                    let Some(source) = picked else {
                        return Ok(Update::File(None));
                    };
                    let preview = ImportPreview::open(source)?;
                    if let Some(id) = &expected {
                        if preview.id != *id {
                            return Err(format!(
                                "This manifest is for {:?}. Choose a manifest for {id:?} to update it.",
                                preview.id
                            ));
                        }
                    }
                    let review = preview.review(&config)?;
                    if expected.is_some() && matches!(review.change, InstallChange::New) {
                        return Err("The extension is no longer installed. Use Install from file to add it again.".into());
                    }
                    Ok(Update::File(Some(review)))
                });
            }
            Some(Request::SetEnabled(id, enabled)) => manager.mutate(
                ctx,
                config,
                if enabled {
                    "Extension enabled."
                } else {
                    "Extension disabled."
                },
                move |config| catalog::set_enabled(config, &id, enabled),
            ),
            Some(Request::Review(review)) => {
                manager.review = Some(review);
                ctx.request_repaint();
            }
            Some(Request::Revoke(approval)) => {
                manager.mutate(ctx, config, "Command approval revoked.", move |config| {
                    approval.change(config, false)
                });
            }
            Some(Request::Confirm) => match manager.review.take() {
                Some(Review::Install(review)) => {
                    let message = match review.change {
                        InstallChange::New => "Extension installed.",
                        InstallChange::Update { .. } => "Extension updated.",
                        InstallChange::Unchanged => return,
                    };
                    manager.mutate(ctx, config, message, move |config| review.install(config));
                }
                Some(Review::Approve(approval)) => {
                    manager.mutate(ctx, config, "Lua command approved.", move |config| {
                        approval.change(config, true)
                    });
                }
                Some(Review::Uninstall(installed)) => {
                    manager.mutate(ctx, config, "Extension uninstalled.", move |config| {
                        catalog::uninstall(config, &installed)
                    });
                }
                Some(Review::ResetApprovals) => manager.mutate(
                    ctx,
                    config,
                    "Lua approvals reset. The previous file is backed up in the configuration folder.",
                    catalog::reset_unreadable_approvals,
                ),
                None => {}
            },
            None => {}
        }
    }
}

#[cfg(test)]
mod tests;
