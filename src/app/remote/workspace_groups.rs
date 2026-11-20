//! Remote Explorer hosts and their saved workspaces.
use super::*;
use std::collections::BTreeMap;

pub(super) enum Action {
    Open(RemoteConfig),
    Browse(RemoteConfig),
    Edit(RemoteConfig),
    Remove(usize),
    SshConfig,
}

struct Group<'a> {
    host: &'a str,
    listed_alias: bool,
    workspaces: Vec<(usize, &'a RemoteConfig, Option<usize>)>,
}

impl Group<'_> {
    fn last(&self) -> Option<&RemoteConfig> {
        self.workspaces.first().map(|(_, profile, _)| *profile)
    }

    fn show(
        &self,
        ui: &mut egui::Ui,
        query: &str,
        ssh_config: &Option<PathBuf>,
        ready: bool,
        local: bool,
    ) -> Option<Action> {
        let host_matches = self.host.to_lowercase().contains(query);
        let matches = |profile: &RemoteConfig| {
            host_matches
                || ConnectChoice::Workspace(profile)
                    .label()
                    .to_lowercase()
                    .contains(query)
        };
        if !host_matches
            && !self
                .workspaces
                .iter()
                .any(|(_, profile, _)| matches(profile))
        {
            return None;
        }
        let last = self.last();
        let home = || ConnectChoice::Host(self.host).config(ssh_config.clone());
        let browse = || {
            if self.listed_alias {
                home()
            } else {
                last.expect("a saved-only group has a workspace").clone()
            }
        };
        let mut choice = None;
        ui.push_id(self.host, |ui| {
            let row = ui
                .add_enabled(
                    ready && (local || last.is_some()),
                    egui::Button::new(format!("{}  →", self.host)).frame(false),
                )
                .on_hover_text(match last {
                    Some(profile) if !profile.root.is_empty() => profile.root.as_str(),
                    _ => "Open the remote home directory.",
                });
            if row.clicked() {
                choice = Some(Action::Open(last.cloned().unwrap_or_else(home)));
            }
            row.context_menu(|ui| {
                if self.listed_alias && ui.button("Connect to remote home").clicked() {
                    choice = Some(Action::Open(home()));
                    ui.close_menu();
                }
                if local && ui.button("Connection settings…").clicked() {
                    choice = Some(Action::Edit(browse()));
                    ui.close_menu();
                }
                if ui.button("SSH configuration…").clicked() {
                    choice = Some(Action::SshConfig);
                    ui.close_menu();
                }
            });
            ui.indent("remote-workspaces", |ui| {
                if let Some(profile) = last {
                    if ui
                        .add_enabled(ready, egui::Button::new("Reopen last workspace").wrap())
                        .on_hover_text(profile.root.as_str())
                        .clicked()
                    {
                        choice = Some(Action::Open(profile.clone()));
                    }
                }
                if local
                    && ui
                        .add_enabled(ready, egui::Button::new("Choose remote folder…").wrap())
                        .clicked()
                {
                    choice = Some(Action::Browse(browse()));
                }
                for (index, profile, _) in &self.workspaces {
                    if !matches(profile) {
                        continue;
                    }
                    let root = if profile.root.is_empty() {
                        "Remote home"
                    } else {
                        &profile.root
                    };
                    ui.push_id(index, |ui| {
                        let row = ui
                            .add_enabled(ready, egui::Button::new(root).wrap())
                            .on_hover_text(ConnectChoice::Workspace(profile).label());
                        if row.clicked() {
                            choice = Some(Action::Open((*profile).clone()));
                        }
                        row.context_menu(|ui| {
                            if ui
                                .add_enabled(local, egui::Button::new("Edit workspace…"))
                                .clicked()
                            {
                                choice = Some(Action::Edit((*profile).clone()));
                                ui.close_menu();
                            }
                            if ui.button("Remove saved workspace").clicked() {
                                choice = Some(Action::Remove(*index));
                                ui.close_menu();
                            }
                        });
                    });
                }
            });
        });
        choice
    }
}

fn groups(remote: &RemoteUi) -> Vec<Group<'_>> {
    let catalog = remote.catalog.as_ref();
    let aliases = remote
        .hosts
        .aliases
        .iter()
        .filter(|host| catalog.is_none_or(|catalog| catalog.imports.includes(host)));
    let mut groups = BTreeMap::<String, Group<'_>>::new();
    for host in aliases {
        groups
            .entry(host.to_ascii_lowercase())
            .or_insert_with(|| Group {
                host,
                listed_alias: true,
                workspaces: Vec::new(),
            });
    }
    if let Some(catalog) = catalog {
        for (index, profile) in catalog.workspaces.iter().enumerate() {
            let group = groups
                .entry(profile.target.host.to_ascii_lowercase())
                .or_insert_with(|| Group {
                    host: &profile.target.host,
                    listed_alias: false,
                    workspaces: Vec::new(),
                });
            group
                .workspaces
                .push((index, profile, catalog.open_order(profile)));
        }
    }
    let mut groups: Vec<_> = groups.into_values().collect();
    for group in &mut groups {
        group
            .workspaces
            .sort_unstable_by_key(|&(index, _, recent)| std::cmp::Reverse((recent, index)));
    }
    groups
}

pub(super) fn show(
    ui: &mut egui::Ui,
    remote: &RemoteUi,
    ready: bool,
    local: bool,
) -> Option<Action> {
    let groups = groups(remote);
    if remote.loading.is_none() && !groups.iter().any(|group| group.listed_alias) {
        if remote.hosts.aliases.is_empty() {
            ui.label("No SSH aliases found. Add a Host entry to your SSH config or connect to a hostname.");
        } else {
            ui.label("No imported SSH hosts.");
        }
    }
    let query = remote.query.to_lowercase();
    let ssh_config = remote.ssh_config();
    let mut choice = None;
    for group in groups {
        if let Some(action) = group.show(ui, &query, &ssh_config, ready, local) {
            choice = Some(action);
        }
    }
    choice
}

#[cfg(test)]
mod tests {
    use super::*;
    use hedit::remote::explorer::HostImports;

    #[test]
    fn groups_keep_saved_only_hosts_and_prefer_the_most_recent_connection() {
        let mut recent = ConnectChoice::Host("DEVBOX").config(None);
        recent.root = "/project".into();
        let mut saved = recent.clone();
        saved.root = "/saved-later".into();
        let mut hidden_saved = ConnectChoice::Host("hidden").config(None);
        hidden_saved.root = "/work".into();
        let mut catalog = Catalog::default();
        catalog.remember_opened(recent.clone(), false).unwrap();
        catalog.remember(saved.clone()).unwrap();
        catalog.remember(hidden_saved.clone()).unwrap();
        catalog
            .set_imports(HostImports::Selected(["devbox".into()].into()))
            .unwrap();
        let remote = RemoteUi {
            catalog: Some(catalog),
            hosts: ssh_config::Hosts {
                aliases: vec!["devbox".into(), "hidden".into(), "unused".into()],
                warnings: vec![],
            },
            ..Default::default()
        };
        let groups = groups(&remote);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].host, "devbox");
        assert_eq!(groups[0].last(), Some(&recent));
        assert_eq!(groups[0].workspaces[1].1, &saved);
        assert!(!groups[1].listed_alias);
        assert_eq!(groups[1].last(), Some(&hidden_saved));
    }
}
