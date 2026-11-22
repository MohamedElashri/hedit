//! Settings for installing local extensions and approving individual Lua commands.
mod recovery;
mod resources;
use super::{
    activity::{Activity, Invocation, InvocationOutcome},
    catalog::{
        self, Catalog, ImportPreview, InstallChange, InstallReview, InstalledManifest,
        ManifestStatus, View,
    },
    manifest::{Contribution, ExtensionAction},
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
    one_time: Option<Contribution>,
    activity: Activity,
    recovery: Option<recovery::Review>,
    recovery_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    tool_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

enum Feedback {
    Success(&'static str),
    Error(String),
}

enum Update {
    Tool {
        command: String,
        token: hedit_broker::Handle,
        profile: String,
        selected: Option<(hedit_broker::Handle, hedit_broker::tool::Info)>,
    },
    Recovery(recovery::Review),
    Catalog {
        catalog: Catalog,
        feedback: Option<Feedback>,
        keep_one_time: bool,
    },
    File(Option<Box<InstallReview>>),
    OneTime(Contribution),
    FolderOpened,
    Scope {
        command: String,
        token: hedit_broker::Handle,
        selected: Option<hedit_broker::state::SelectedScope>,
    },
}

enum Request {
    Tool(resources::ToolAction),
    Recovery(recovery::Request),
    Reload,
    OpenFolder,
    ChooseFile(Option<String>),
    Review(Review),
    Confirm,
    SetEnabled(String, bool),
    Revoke(Approval),
    AllowOnce,
    CancelOnce,
    ChooseScope,
    RevokeResources(String),
}

enum Review {
    Install(Box<InstallReview>),
    Approve(Approval),
    Uninstall(InstalledManifest),
    ResetApprovals,
    ResetResourceApprovals,
    Resources(Box<resources::ResourceReview>),
}

#[derive(Clone)]
struct Approval {
    sdk: bool,
    storage: bool,
    provider: Option<crate::plugin_runtime::service::Definition>,
    broker: Option<hedit_broker::declaration::Definition>,
    path: PathBuf,
    extension: String,
    command: String,
    title: String,
    digest: String,
}

impl Manager {
    #[cfg(all(test, target_os = "macos"))]
    pub(in crate::app::extensions) fn measured_activity(&self) -> Vec<serde_json::Value> {
        self.activity.measured()
    }

    pub(in crate::app) fn one_time_command(&self) -> Option<&Contribution> {
        self.one_time.as_ref()
    }

    pub(in crate::app) fn consume_one_time(&mut self, id: &str) -> bool {
        if self
            .one_time
            .as_ref()
            .is_none_or(|command| command.id != id)
        {
            return false;
        }
        self.one_time.take().is_some_and(|command| {
            let ExtensionAction::Lua(plugin) = command.action else {
                return false;
            };
            plugin.one_time.is_some_and(|approval| approval.claim())
        })
    }

    pub(in crate::app) fn record(&mut self, invocation: Invocation, outcome: InvocationOutcome) {
        self.activity.record(invocation, outcome);
    }

    fn hide(&mut self) {
        self.recovery = None;
        if let Some(cancel) = self.tool_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        if let Some(cancel) = self.recovery_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Release);
        }
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
        let one_time = self.one_time.clone();
        self.work(ctx, move || {
            let catalog = catalog::read(&config);
            let keep_one_time = one_time
                .as_ref()
                .is_some_and(|command| catalog::one_time_current(&config, &catalog, command));
            Ok(Update::Catalog {
                catalog,
                feedback: manual.then_some(Feedback::Success("Extensions reloaded.")),
                keep_one_time,
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
        self.one_time = None;
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
                keep_one_time: false,
            })
        });
    }

    fn show(&mut self, ui: &mut egui::Ui, config: &Path) -> Option<Request> {
        if self.view.is_none() && self.pending.is_none() && self.feedback.is_none() {
            self.reload(ui.ctx(), config.to_owned(), false);
        }

        let pending = self.pending.is_some();
        let can_act = !pending && self.review.is_none() && self.recovery.is_none();
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
            if ui
                .add_enabled(can_act, egui::Button::new("Workspace recovery…"))
                .clicked()
            {
                request = Some(Request::Recovery(recovery::Request::List));
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
            if view.resources_unreadable
                && ui
                    .add_enabled(can_act, egui::Button::new("Reset resource approvals…"))
                    .clicked()
            {
                request = Some(Request::Review(Review::ResetResourceApprovals));
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
                    let once = self.one_time.as_ref().map(|command| command.id.as_str());
                    if let Some(action) = installed_row(ui, entry, view, can_act, once) {
                        request = Some(action);
                    }
                });
            }
        }

        self.activity.show(ui);
        if let Some(review) = &mut self.recovery {
            let error = match &self.feedback {
                Some(Feedback::Error(error)) => Some(error.as_str()),
                _ => None,
            };
            if let Some(action) = review.show(ui, !pending, error) {
                return Some(Request::Recovery(action));
            }
        }
        if let Some(review) = &mut self.review {
            let shown = egui::Modal::new(egui::Id::new(("review-extension", review.label())))
                .show(ui.ctx(), |ui| review.show(ui, !pending));
            if shown.should_close() || shown.inner == Some(ReviewChoice::Cancel) {
                self.review = None;
            } else {
                match shown.inner {
                    Some(ReviewChoice::Confirm) => return Some(Request::Confirm),
                    Some(ReviewChoice::Once) => return Some(Request::AllowOnce),
                    Some(ReviewChoice::ChooseScope) => return Some(Request::ChooseScope),
                    Some(ReviewChoice::Tool(action)) => return Some(Request::Tool(action)),
                    Some(ReviewChoice::Cancel) | None => {}
                }
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
    once: Option<&str>,
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
    if let Some(release) = entry
        .installed
        .as_ref()
        .and_then(|copy| copy.release.as_ref())
    {
        release_details(ui, release);
    }
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
                        if once == Some(id.as_str()) {
                            ui.weak("Allowed once");
                            if ui
                                .add_enabled(can_act, egui::Button::new("Cancel once"))
                                .clicked()
                            {
                                request = Some(Request::CancelOnce);
                            }
                            return;
                        }
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
                            let approval =
                                Approval::for_command(&entry.path, &details.id, command, lua);
                            request = Some(if command.approved {
                                Request::Revoke(approval)
                            } else {
                                Request::Review(Review::Approve(approval))
                            });
                        }
                        if let Some(token) = command.token.filter(|_| command.broker.is_some()) {
                            if ui
                                .add_enabled(can_act, egui::Button::new("Review resources…"))
                                .clicked()
                            {
                                request = Some(Request::Review(Review::Resources(Box::new(
                                    resources::ResourceReview::new(
                                        Approval::for_command(
                                            &entry.path,
                                            &details.id,
                                            command,
                                            lua,
                                        ),
                                        token,
                                        command.resources.as_ref(),
                                    ),
                                ))));
                            }
                            if command.resources.is_some()
                                && ui
                                    .add_enabled(
                                        can_act,
                                        egui::Button::new("Revoke resource access"),
                                    )
                                    .clicked()
                            {
                                request = Some(Request::RevokeResources(command.id.clone()));
                            }
                            if let Some(approval) = &command.resources {
                                resources::summary(ui, approval);
                            }
                        }
                    });
                });
            }
        });
    request
}

#[derive(PartialEq)]
enum ReviewChoice {
    Tool(resources::ToolAction),
    Confirm,
    Cancel,
    Once,
    ChooseScope,
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
            Self::ResetResourceApprovals => "Reset resource approvals",
            Self::Resources(_) => "Review extension resources",
        }
    }

    fn show(&mut self, ui: &mut egui::Ui, ready: bool) -> Option<ReviewChoice> {
        ui.set_width((ui.ctx().screen_rect().width() - 56.0).clamp(200.0, 520.0));
        ui.heading(self.label());
        let body_height = (ui.ctx().screen_rect().height() - 132.0).clamp(72.0, 390.0);
        let confirm = egui::ScrollArea::vertical()
            .max_height(body_height)
            .show(ui, |ui| self.details(ui))
            .inner;
        let ready = ready
            && match self {
                Self::Install(review) => {
                    !review.requires_publisher_confirmation() || review.publisher_confirmed
                }
                _ => true,
            };
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if confirm.is_some() { "Cancel" } else { "Close" })
                .clicked()
            {
                return Some(ReviewChoice::Cancel);
            }
            if matches!(self, Self::Approve(_))
                && ui
                    .add_enabled(
                        ready && !matches!(self,Self::Approve(approval) if approval.storage || approval.provider.is_some() || approval.broker.is_some()),
                        egui::Button::new("Allow once"),
                    )
                    .clicked()
            {
                return Some(ReviewChoice::Once);
            }
            if let Some(confirm) = confirm {
                if ui.add_enabled(ready, egui::Button::new(confirm)).clicked() {
                    return Some(ReviewChoice::Confirm);
                }
            }
            None
        })
        .inner.or_else(|| match self {
            Self::Resources(review) => if review.choose_scope { review.choose_scope = false; Some(ReviewChoice::ChooseScope) } else { review.tool_action.take().map(ReviewChoice::Tool) },
            _ => None
        })
    }
    fn details(&mut self, ui: &mut egui::Ui) -> Option<&'static str> {
        match self {
            Self::Install(review) => {
                ui.strong(&review.preview.id);
                path_label(ui, "From", &review.preview.source);
                if let Some(release) = &review.preview.release {
                    release_details(ui, release);
                }
                if review.requires_publisher_confirmation() {
                    ui.label("Compare this fingerprint with one supplied by the publisher.");
                    ui.checkbox(
                        &mut review.publisher_confirmed,
                        "Trust this publisher key for this extension",
                    );
                }
                if let InstallChange::Update {
                    installed,
                    previous_commands,
                    changed_scripts,
                } = &review.change
                {
                    path_label(ui, "Replacing", &installed.path);
                    if let Some(release) = &installed.release {
                        ui.label(format!("Installed version {}", release.version));
                    }
                    if !changed_scripts.is_empty() {
                        ui.label(format!("Changed scripts {}", changed_scripts.join(", ")));
                    }
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
                        if review.preview.brokers.is_empty() {
                            requested_permissions(ui, review.preview.sdk, review.preview.storage);
                        } else {
                            for (command, definition) in &review.preview.brokers {
                                ui.monospace(command);
                                resources::capabilities(ui, definition);
                            }
                        }
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
                if let Some(broker) = &approval.broker {
                    if let Some(provider) = &approval.provider {
                        ui.label(format!(
                            "Provider: {:?}; triggers: {:?}; debounce: {} ms",
                            provider.kind, provider.events, provider.debounce_ms
                        ));
                        ui.label(if broker.has(hedit_broker::declaration::Capability::DocumentRead) {
                            "Reads the eligible active local document. Automatic triggers are covered by this separate provider approval."
                        } else {
                            "Receives an empty document snapshot. Starts only on an explicit user action."
                        });
                    }
                    resources::capabilities(ui, broker);
                    ui.label("Source approval alone grants no folder, executable or endpoint. Review those resources separately.");
                } else if let Some(provider) = &approval.provider {
                    ui.label(format!(
                        "Provider: {:?}; triggers: {:?}; debounce: {} ms",
                        provider.kind, provider.events, provider.debounce_ms
                    ));
                    ui.label("Reads the eligible active local document and presents native results. Background triggers require this separate approval.");
                    ui.label(if provider.may_edit {
                        "May propose document edits for explicit user actions."
                    } else {
                        "Cannot edit document text."
                    });
                    ui.weak("Each callback uses a fresh isolated worker; private cache is scoped to this grant and active document.");
                } else {
                    requested_permissions(ui, approval.sdk, approval.storage);
                }
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
            Self::Resources(review) => {
                review.show(ui);
                Some("Approve resource access")
            }
            Self::ResetResourceApprovals => {
                ui.label("Back up the unreadable resource approval file. Workspace directories, HTTPS endpoints and tools will require fresh review. Command source approvals are retained.");
                Some("Reset resource approvals")
            }
        }
    }
}

fn release_details(ui: &mut egui::Ui, release: &super::distribution::Release) {
    ui.horizontal_wrapped(|ui| {
        ui.weak(format!("Version {}", release.version));
        if let Some(publisher) = &release.publisher {
            ui.label(format!("Signed by {}", publisher.name));
            ui.label(&publisher.fingerprint)
                .on_hover_text("Publisher key SHA-256");
        } else {
            ui.weak("Unsigned bundle");
        }
    });
}

fn requested_permissions(ui: &mut egui::Ui, sdk: bool, storage: bool) {
    ui.strong("Requested permissions");
    if sdk {
        ui.label("Read the whole current local document, up to 64 KiB.");
        ui.label("Propose edits and selections anywhere in that document.");
        if storage {
            ui.label("Read and update private command storage. Revoking approval clears it.");
        }
        return;
    }
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
    fn for_command(
        path: &Path,
        extension: &str,
        command: &catalog::LuaCommandStatus,
        lua: &catalog::LuaStatus,
    ) -> Self {
        Self {
            sdk: lua.sdk,
            storage: command.storage,
            provider: command.provider.clone(),
            broker: command.broker.clone(),
            path: path.into(),
            extension: extension.into(),
            command: command.id.clone(),
            title: command.title.clone(),
            digest: lua.digest.clone(),
        }
    }
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
        manager.recovery_cancel = None;
        manager.tool_cancel = None;
        match result {
            Ok(Update::Tool {
                command,
                token,
                profile,
                selected,
            }) => {
                if showing {
                    if let Some(Review::Resources(review)) = &mut manager.review {
                        if review.source.command == command && review.token == token {
                            if let Some((image, info)) = selected {
                                review.tools.insert(profile.clone(), image);
                                review.tool_info.insert(profile, info);
                            }
                        }
                    }
                }
            }
            Ok(Update::Recovery(review)) => {
                self.plugin_services.invalidate();
                if showing {
                    manager.recovery = Some(review);
                }
            }
            Ok(Update::Catalog {
                catalog,
                feedback,
                keep_one_time,
            }) => {
                if !keep_one_time {
                    manager.one_time = None;
                }
                self.extensions = catalog.contributions;
                self.plugin_services.invalidate();
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
            Ok(Update::OneTime(command)) => {
                manager.one_time = Some(command);
                self.chord = None;
                if showing {
                    manager.feedback = Some(Feedback::Success(
                        "Allowed for the next run in this window.",
                    ));
                }
            }
            Ok(Update::FolderOpened) => {}
            Ok(Update::Scope {
                command,
                token,
                selected,
            }) => {
                if showing {
                    if let Some(Review::Resources(review)) = &mut manager.review {
                        if review.source.command == command && review.token == token {
                            if let Some(scope) = selected {
                                review.scope = Some(scope);
                            }
                        }
                    }
                }
            }
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
        if matches!(
            &request,
            Some(
                Request::SetEnabled(..)
                    | Request::Revoke(_)
                    | Request::Confirm
                    | Request::RevokeResources(_)
                    | Request::Tool(resources::ToolAction::ClearUnused)
            )
        ) {
            self.broker_command.take();
            self.plugin_services.suspend();
        }
        match request {
            Some(Request::Tool(action)) => {
                if action == resources::ToolAction::ClearUnused {
                    manager.review = None;
                    manager.mutate(ctx, config, "Unused tool imports removed.", catalog::clear_unused_tools);
                } else if let Some(Review::Resources(review)) = &manager.review {
                    let source = review.source.clone();
                    let token = review.token;
                    let (profile, pick) = match action { resources::ToolAction::Pick(profile) => (profile, true), resources::ToolAction::Inspect(profile) => (profile, false), _ => unreachable!() };
                    let current = review.tools.get(&profile).copied();
                    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                    manager.tool_cancel = Some(cancel.clone());
                    manager.work(ctx, move || {
                        let selection = catalog::ToolSelection { config: &config, path: &source.path, extension: &source.extension, command: &source.command, digest: &source.digest, token, profile: &profile };
                        let selected = if pick {
                            rfd::FileDialog::new().set_title("Choose a standalone native executable").pick_file().map(|path| catalog::import_tool(selection, &path, &cancel)).transpose()?
                        } else {
                            let image = current.ok_or("No tool image is selected.")?;
                            Some((image, catalog::tool_info(selection, image)?))
                        };
                        Ok(Update::Tool { command: source.command, token, profile, selected })
                    });
                }
            }
            Some(Request::Recovery(request)) => {
                let context = recovery::Context {
                    editor: self.workspace.editor.clone(),
                    session: self.workspace.store.directory().to_owned(),
                    following: self.workspace.open.iter().filter(|b| b.follow).map(|b| b.page.info.id).collect(),
                    sender: self.tx.clone(),
                };
                recovery::start(manager, request, ctx, config, context);
            }
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
                        .add_filter("Extension or bundle", &["json", "hedit-plugin"])
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
                    Ok(Update::File(Some(Box::new(review))))
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
            Some(Request::ChooseScope) => {
                if let Some(Review::Resources(review))=&manager.review {
                    let rights=review.rights();
                    let command=review.source.command.clone();let token=review.token;
                    let private_directory = self.workspace.store.directory().to_owned();
                    manager.work(ctx,move || {
                        let selected=rfd::FileDialog::new().set_title("Select a local extension workspace").pick_folder();
                        let scope=selected.map(|path| {let state=hedit_broker::state::State::open(&config,[private_directory]).map_err(|e|e.to_string())?;hedit_broker::state::SelectedScope::select(&path,rights,&state).map_err(|e|e.to_string())}).transpose()?;
                        Ok(Update::Scope {command,token,selected:scope})
                    });
                }
            }
            Some(Request::RevokeResources(command)) => manager.mutate(ctx,config,"Resource access revoked.",move |config|catalog::revoke_resources(config,&command)),
            Some(Request::Revoke(approval)) => {
                manager.mutate(ctx, config, "Command approval revoked.", move |config| {
                    approval.change(config, false)
                });
            }
            Some(Request::AllowOnce) => {
                if let Some(Review::Approve(approval)) = manager.review.take() {
                    manager.work(ctx, move || {
                        let command = catalog::prepare_one_time(
                            &config,
                            &approval.path,
                            &approval.extension,
                            &approval.command,
                            &approval.digest,
                        )?;
                        Ok(Update::OneTime(command))
                    });
                }
            }
            Some(Request::CancelOnce) => {
                manager.one_time = None;
                self.chord = None;
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
                Some(Review::Resources(review)) => manager.mutate(ctx,config,"Resource access updated.",move |config|review.approve(config)),
                Some(Review::ResetResourceApprovals) => manager.mutate(ctx, config, "Resource approvals reset. The previous file is backed up in extensions-broker.", catalog::reset_resource_approvals),
                None => {}
            },
            None => {}
        }
    }
}

#[cfg(test)]
mod tests;
