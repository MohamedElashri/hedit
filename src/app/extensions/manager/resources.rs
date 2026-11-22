use super::*;
use hedit_broker::{
    declaration::{Capability, Definition},
    state::{Approval as ResourceApproval, SelectedScope},
    workspace::Right,
    Handle,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, PartialEq, Eq)]
pub(super) enum ToolAction {
    Pick(String),
    Inspect(String),
    ClearUnused,
}
pub(super) struct ResourceReview {
    pub(super) source: Approval,
    pub(super) token: Handle,
    pub(super) scope: Option<SelectedScope>,
    rights: BTreeSet<Right>,
    endpoints: BTreeSet<String>,
    pub(super) tools: BTreeMap<String, Handle>,
    pub(super) tool_info: BTreeMap<String, hedit_broker::tool::Info>,
    pub(super) tool_action: Option<ToolAction>,
    pub(super) choose_scope: bool,
}
impl ResourceReview {
    pub(super) fn new(source: Approval, token: Handle, record: Option<&ResourceApproval>) -> Self {
        let definition = source.broker.as_ref().unwrap();
        Self {
            token,
            scope: record.and_then(|record| record.scope.clone()),
            rights: record
                .and_then(|record| record.scope.as_ref())
                .map(|scope| scope.rights().iter().copied().collect())
                .unwrap_or_else(|| {
                    definition
                        .capabilities
                        .iter()
                        .filter_map(|capability| capability.workspace())
                        .collect()
                }),
            endpoints: record
                .map(|record| record.endpoints.iter().cloned().collect())
                .unwrap_or_default(),
            tools: record
                .map(|record| record.tools.clone())
                .unwrap_or_default(),
            source,
            tool_info: BTreeMap::new(),
            tool_action: None,
            choose_scope: false,
        }
    }
    pub(super) fn rights(&self) -> Vec<Right> {
        self.rights.iter().copied().collect()
    }
    pub(super) fn show(&mut self, ui: &mut egui::Ui) {
        let definition = self.source.broker.as_ref().unwrap();
        ui.strong(&self.source.title);
        ui.monospace(&self.source.command);
        ui.label("Resource access applies only to this exact source approval. Revoke it at any time in Extensions.");
        let requested: Vec<_> = definition
            .capabilities
            .iter()
            .filter_map(|capability| capability.workspace())
            .collect();
        if !requested.is_empty() {
            ui.separator();
            ui.strong("Local workspace");
            for right in requested {
                let mut selected = self.rights.contains(&right);
                let can_select = right != Right::Edit || self.rights.contains(&Right::Read);
                if ui
                    .add_enabled(
                        can_select,
                        egui::Checkbox::new(&mut selected, right_label(right)),
                    )
                    .changed()
                {
                    if selected {
                        self.rights.insert(right);
                    } else {
                        self.rights.remove(&right);
                        if right == Right::Read {
                            self.rights.remove(&Right::Edit);
                        }
                    }
                }
            }
            if let Some(scope) = &self.scope {
                path_label(ui, "Selected directory", scope.path());
            } else {
                ui.weak("No directory selected. Workspace operations will be denied.");
            }
            if ui
                .add_enabled(
                    !self.rights.is_empty(),
                    egui::Button::new("Choose directory…"),
                )
                .clicked()
            {
                self.choose_scope = true;
            }
            if self.scope.is_some() && ui.button("Remove directory access").clicked() {
                self.scope = None;
            }
            ui.weak("Hidden files, links, binary files and private editor data remain excluded. Edit access covers existing text files and requires a complete preview.");
        }
        if !definition.endpoints.is_empty() {
            ui.separator();
            ui.strong("HTTPS endpoints");
            for (id, endpoint) in &definition.endpoints {
                let mut selected = self.endpoints.contains(id);
                if ui.checkbox(&mut selected, id).changed() {
                    if selected {
                        self.endpoints.insert(id.clone());
                    } else {
                        self.endpoints.remove(id);
                    }
                }
                let normalized = endpoint
                    .endpoint()
                    .map(|endpoint| endpoint.url().to_owned())
                    .unwrap_or_else(|_| "Unavailable endpoint".into());
                ui.label(format!("{:?} {normalized}", endpoint.method));
                ui.weak(format!(
                    "{:?}; upload {} bytes; response {} bytes; ten-second maximum",
                    endpoint.content_type, endpoint.upload, endpoint.response
                ));
            }
            if !self.endpoints.is_empty()
                && (definition.has(Capability::DocumentRead) || self.rights.contains(&Right::Read))
            {
                ui.label("Granted document and workspace text can be uploaded to these endpoints.");
            }
        }
        if !definition.tools.is_empty() {
            ui.separator();
            ui.strong("Native tools");
            ui.label("Selected tools can receive text granted to this command. Changes take effect only when you approve this review.");
            for (id, tool) in &definition.tools {
                ui.push_id(id, |ui| {
                    ui.strong(id);
                    if let Some(info) = self.tool_info.get(id) {
                        ui.label(format!("{} ({} bytes)", info.origin.escape_debug(), info.bytes));
                        ui.monospace(format!("SHA-256 {}", info.sha256));
                    } else if self.tools.contains_key(id) {
                        ui.label("An imported executable is selected.");
                        if ui.button("Review selected image").clicked() { self.tool_action = Some(ToolAction::Inspect(id.clone())); }
                    } else { ui.weak("No executable selected. This tool is denied."); }
                    if tool.workspace { ui.label("Receives a read-only copy of eligible files in the separately selected workspace."); }
                    else { ui.label("Receives text supplied by the command, with an empty private working directory."); }
                    let recipe: Vec<_> = tool.arguments.iter().map(|argument| match argument {
                        hedit_broker::declaration::Argument::Literal(value) => format!("{value:?}"),
                        hedit_broker::declaration::Argument::Choice(key) => format!("<{key}: {}>", tool.choices[key].iter().map(|value| serde_json::to_string(value).unwrap_or_default()).collect::<Vec<_>>().join(", ")),
                    }).collect();
                    ui.monospace(format!("Arguments: {}", recipe.join(" ")));
                    if tool.protocol == hedit_broker::declaration::ToolProtocol::Filter {
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(self.tools.contains_key(id) || self.tools.len() < 8, egui::Button::new("Choose native executable…")).clicked() { self.tool_action = Some(ToolAction::Pick(id.clone())); }
                            if self.tools.contains_key(id) && ui.button("Remove tool access").clicked() { self.tools.remove(id); self.tool_info.remove(id); }
                        });
                        ui.weak("64 KiB input/output; ten-second wall deadline; two-second CPU budget. Network, child processes and file writes are blocked.");
                        ui.weak("256 MiB memory and 32 threads. Monitoring and OS enforcement can briefly overshoot their configured limits.");
                    } else { ui.weak("Language servers are unavailable in this build."); }
                    ui.separator();
                });
            }
            if ui.button("Clear unused tool imports").clicked() {
                self.tool_action = Some(ToolAction::ClearUnused);
            }
            ui.weak("Imports keep a private copy and preserve execution trust marks. Updating the original executable does not replace an approved copy.");
        }
    }
    pub(super) fn approve(self, config: &Path) -> Result<Catalog, String> {
        let rights = self.rights();
        let scope = if rights.is_empty() {
            None
        } else {
            self.scope
                .as_ref()
                .map(|scope| scope.with_rights(rights).map_err(|error| error.to_string()))
                .transpose()?
        };
        catalog::approve_resources(
            config,
            &self.source.path,
            &self.source.extension,
            &self.source.digest,
            ResourceApproval {
                command: self.source.command,
                token: self.token,
                scope,
                endpoints: self.endpoints.into_iter().collect(),
                tools: self.tools,
            },
        )
    }
}
fn right_label(right: Right) -> &'static str {
    match right {
        Right::Discover => "Discover file and directory names",
        Right::Read => "Read eligible text files",
        Right::Edit => "Propose edits to existing files",
        Right::Watch => "Observe coalesced workspace changes",
    }
}
pub(super) fn capabilities(ui: &mut egui::Ui, definition: &Definition) {
    ui.strong("Requested capabilities");
    for capability in definition.canonical() {
        ui.label(match capability {
            Capability::DocumentRead => "Read the whole current local document (up to 64 KiB).",
            Capability::DocumentEdit => "Propose edits in that document.",
            Capability::Observe => "Run on the separately declared background events.",
            Capability::Present => "Show bounded native results and interactions.",
            Capability::Discover => "Request scoped workspace discovery.",
            Capability::Read => "Request scoped workspace text reads.",
            Capability::Edit => "Request reviewed multi-file edit plans.",
            Capability::Watch => "Request scoped workspace notifications.",
            Capability::Tool => "Request separately reviewed native executables.",
            Capability::Https => "Request separately reviewed HTTPS endpoints.",
        });
    }
}
pub(super) fn summary(ui: &mut egui::Ui, approval: &ResourceApproval) {
    if let Some(scope) = &approval.scope {
        path_label(ui, "Resource scope", scope.path());
        for right in scope.rights() {
            ui.weak(right_label(*right));
        }
    }
    if !approval.endpoints.is_empty() {
        ui.weak(format!(
            "Approved endpoints: {}",
            approval.endpoints.join(", ")
        ));
    }
    if !approval.tools.is_empty() {
        ui.weak(format!(
            "Approved tools: {}",
            approval
                .tools
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}
