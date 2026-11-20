//! Preferences for local extensions and Lua plugin approvals.
use super::catalog::{self, Catalog, ImportPreview, View};
use crate::app::HeditApp;
use eframe::egui;
use std::{
    path::Path,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

#[derive(Default)]
pub(in crate::app) struct Manager {
    view: Option<View>,
    preview: Option<ImportPreview>,
    pending: Option<Receiver<Result<Update, String>>>,
    error: Option<String>,
    status: Option<&'static str>,
}

enum Update {
    View {
        catalog: Catalog,
        status: Option<&'static str>,
        installed: bool,
    },
    Preview(Option<ImportPreview>),
}

enum Request {
    Reload,
    ChooseFile,
    Install,
    SetEnabled(String, bool),
    SetLuaApproved {
        id: String,
        digest: String,
        approved: bool,
    },
}

impl Manager {
    fn work(
        &mut self,
        ctx: &egui::Context,
        task: impl FnOnce() -> Result<Update, String> + Send + 'static,
    ) {
        self.error = None;
        self.status = None;
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        // Wake the UI after the result or a dropped worker, including unwinding.
        struct Wake(egui::Context);
        impl Drop for Wake {
            fn drop(&mut self) {
                self.0.request_repaint();
            }
        }
        match thread::Builder::new()
            .name("hedit-extensions".into())
            .spawn(move || {
                let _wake = Wake(ctx);
                let sender = sender;
                let _ = sender.send(task());
            }) {
            Ok(_) => self.pending = Some(receiver),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn show(&mut self, ui: &mut egui::Ui, config: &Path) -> Option<Request> {
        if self.view.is_none() && self.pending.is_none() && self.error.is_none() {
            let config = config.to_owned();
            self.work(ui.ctx(), move || {
                Ok(Update::View {
                    catalog: catalog::read(&config),
                    status: None,
                    installed: false,
                })
            });
        }
        let pending = self.pending.is_some();
        let mut request = None;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!pending, egui::Button::new("Import manifest…"))
                .clicked()
            {
                request = Some(Request::ChooseFile);
            }
            if ui
                .add_enabled(!pending, egui::Button::new("Reload extensions"))
                .clicked()
            {
                request = Some(Request::Reload);
            }
            if pending {
                ui.spinner();
                ui.weak("Working…");
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if let Some(status) = self.status {
            ui.weak(status);
        }
        let mut cancel_preview = false;
        if let Some(preview) = &self.preview {
            ui.group(|ui| {
                ui.strong(format!("Import {}", preview.id));
                ui.weak(preview.source.display().to_string());
                command_rows(ui, &preview.commands);
                if preview.lua {
                    ui.label(LUA_ACCESS);
                    ui.weak("Approve this plugin after installing it to run its commands.");
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!pending, egui::Button::new("Install"))
                        .clicked()
                    {
                        request = Some(Request::Install);
                    }
                    if ui
                        .add_enabled(!pending, egui::Button::new("Cancel"))
                        .clicked()
                    {
                        cancel_preview = true;
                    }
                });
            });
            ui.add_space(8.0);
        }
        if cancel_preview {
            self.preview = None;
            self.error = None;
        }
        if let Some(view) = &self.view {
            for notice in &view.notices {
                ui.colored_label(ui.visuals().error_fg_color, notice);
            }
            if view.files.is_empty() && !pending {
                ui.label("No extensions installed.");
            }
            for entry in &view.files {
                ui.push_id(&entry.path, |ui| {
                    ui.separator();
                    match &entry.details {
                        Ok(details) => {
                            let mut enabled = details.enabled;
                            ui.horizontal_wrapped(|ui| {
                                ui.strong(&details.id)
                                    .on_hover_text(entry.path.display().to_string());
                                if ui
                                    .add_enabled(
                                        !pending && !view.state_unreadable,
                                        egui::Checkbox::new(&mut enabled, "Enabled"),
                                    )
                                    .changed()
                                {
                                    request =
                                        Some(Request::SetEnabled(details.id.clone(), enabled));
                                }
                            });
                            if let Some(lua) = &details.lua {
                                ui.label(LUA_ACCESS);
                                if !crate::plugin_runtime::SUPPORTED {
                                    ui.weak("Lua execution is unavailable on this platform.");
                                }
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(if lua.approved {
                                        "Approved"
                                    } else {
                                        "Approval required"
                                    })
                                    .on_hover_text(format!("Manifest SHA-256 {}", lua.digest));
                                    if ui
                                        .add_enabled(
                                            !pending
                                                && !view.grants_unreadable
                                                && (lua.approved
                                                    || crate::plugin_runtime::SUPPORTED),
                                            egui::Button::new(if lua.approved {
                                                "Revoke approval"
                                            } else {
                                                "Approve"
                                            }),
                                        )
                                        .clicked()
                                    {
                                        request = Some(Request::SetLuaApproved {
                                            id: details.id.clone(),
                                            digest: lua.digest.clone(),
                                            approved: !lua.approved,
                                        });
                                    }
                                });
                            }
                            ui.collapsing(format!("{} commands", details.commands.len()), |ui| {
                                command_rows(ui, &details.commands)
                            });
                        }
                        Err(error) => {
                            let name = entry.path.file_name().unwrap_or(entry.path.as_os_str());
                            ui.strong(name.to_string_lossy())
                                .on_hover_text(entry.path.display().to_string());
                            ui.colored_label(ui.visuals().error_fg_color, error);
                        }
                    }
                });
            }
        }
        request
    }
}

const LUA_ACCESS: &str =
    "Lua access is limited to reading and replacing the active selection in a local editor.";

fn command_rows(ui: &mut egui::Ui, commands: &[(String, String)]) {
    for (id, title) in commands {
        ui.horizontal_wrapped(|ui| {
            ui.label(title);
            ui.weak(egui::RichText::new(id).monospace());
        });
    }
}

impl HeditApp {
    pub(in crate::app) fn poll_extension_manager(&mut self) {
        let manager = &mut self.extension_manager;
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
            Ok(Update::View {
                catalog,
                status,
                installed,
            }) => {
                self.extensions = catalog.contributions;
                self.chord = None;
                manager.view = Some(catalog.view);
                manager.status = status;
                if installed {
                    manager.preview = None;
                }
            }
            Ok(Update::Preview(preview)) => manager.preview = preview,
            Err(error) => manager.error = Some(error),
        }
    }

    pub(in crate::app) fn extension_preferences(&mut self, ui: &mut egui::Ui) {
        let Some(config) = self.settings_files.dir.clone() else {
            ui.label("Extension management requires a user configuration folder.");
            return;
        };
        let manager = &mut self.extension_manager;
        match manager.show(ui, &config) {
            Some(Request::Reload) => manager.work(ui.ctx(), move || {
                Ok(Update::View {
                    catalog: catalog::read(&config),
                    status: Some("Extensions reloaded."),
                    installed: false,
                })
            }),
            Some(Request::ChooseFile) => {
                manager.preview = None;
                manager.work(ui.ctx(), || {
                    let picked = rfd::FileDialog::new()
                        .set_title("Import extension manifest")
                        .add_filter("JSON manifest", &["json"])
                        .pick_file();
                    Ok(Update::Preview(
                        picked.map(ImportPreview::open).transpose()?,
                    ))
                });
            }
            Some(Request::Install) => {
                if let Some(preview) = manager.preview.clone() {
                    manager.work(ui.ctx(), move || {
                        Ok(Update::View {
                            catalog: preview.install(&config)?,
                            status: Some("Extension installed."),
                            installed: true,
                        })
                    });
                }
            }
            Some(Request::SetEnabled(id, enabled)) => manager.work(ui.ctx(), move || {
                Ok(Update::View {
                    catalog: catalog::set_enabled(&config, &id, enabled)?,
                    status: Some(if enabled {
                        "Extension enabled."
                    } else {
                        "Extension disabled."
                    }),
                    installed: false,
                })
            }),
            Some(Request::SetLuaApproved {
                id,
                digest,
                approved,
            }) => {
                manager.work(ui.ctx(), move || {
                    Ok(Update::View {
                        catalog: catalog::set_lua_approved(&config, &id, &digest, approved)?,
                        status: Some(if approved {
                            "Lua plugin approved for this version of its manifest."
                        } else {
                            "Lua plugin approval revoked."
                        }),
                        installed: false,
                    })
                });
            }
            None => {}
        }
    }
}
