//! Local connection metadata only. No document content or authentication secrets.
use super::Config;
use std::path::{Path, PathBuf};

#[derive(Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Catalog {
    pub config: Option<PathBuf>,
    pub workspaces: Vec<Config>,
}
impl Catalog {
    pub fn load(path: &Path) -> Result<Self, String> {
        let value: Self = match crate::projects::read_json(path, 1024 * 1024) {
            Ok(value) => value,
            Err(_)
                if std::fs::symlink_metadata(path)
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(Self::default())
            }
            Err(error) => return Err(format!("Remote Explorer: {error}")),
        };
        if value.workspaces.len() > 64 {
            return Err("Remote Explorer supports at most 64 saved workspaces.".into());
        }
        for workspace in &value.workspaces {
            workspace.validate()?;
        }
        if value
            .config
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            return Err("Use an absolute SSH config path.".into());
        }
        Ok(value)
    }
    pub fn remember(&mut self, mut config: Config) -> Result<(), String> {
        config.validate()?;
        config.takeover = false;
        let existing = self.workspaces.iter().position(|w| {
            w.target.host == config.target.host
                && w.target.config == config.target.config
                && w.target.user == config.target.user
                && w.target.port == config.target.port
                && (w.root == config.root || w.managed && config.managed && w.root.is_empty())
        });
        if let Some(index) = existing {
            self.workspaces.remove(index);
            self.workspaces.push(config);
        } else if self.workspaces.len() < 64 {
            self.workspaces.push(config);
        } else {
            return Err("Remove a saved workspace before adding another (limit 64).".into());
        }
        Ok(())
    }
    pub fn save(&self, path: &Path, expected: &Self) -> Result<(), String> {
        if serde_json::to_vec_pretty(self)
            .map_err(|e| e.to_string())?
            .len()
            > 1024 * 1024
        {
            return Err("Saved SSH workspaces exceed the 1 MiB limit.".into());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // Serialize writers across editor windows and reject stale snapshots.
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock().map_err(|_| {
            "SSH workspaces are being updated in another window. Refresh and retry.".to_owned()
        })?;
        if Self::load(path)? != *expected {
            return Err("SSH workspaces changed in another window. Refresh before saving.".into());
        }
        crate::projects::atomic_json(path, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Config {
        Config {
            target: hedit_remote::client::SshTarget {
                host: "devbox".into(),
                user: None,
                port: 0,
                config: None,
                helper: "/opt/hedit-remote".into(),
                state: "/private/state".into(),
            },
            root: "/work/project".into(),
            managed: false,
            takeover: true,
        }
    }
    #[test]
    fn saved_workspaces_deduplicate_and_never_retain_takeover() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ssh-workspaces.json");
        let mut catalog = Catalog::load(&path).unwrap();
        catalog.remember(config()).unwrap();
        let mut updated = config();
        updated.target.helper = "/opt/new-helper".into();
        catalog.remember(updated).unwrap();
        assert_eq!(catalog.workspaces.len(), 1);
        assert!(!catalog.workspaces[0].takeover);
        catalog.save(&path, &Catalog::default()).unwrap();
        let loaded = Catalog::load(&path).unwrap();
        assert_eq!(loaded.workspaces[0].target.port, 0);
        assert_eq!(loaded.workspaces[0].target.helper, "/opt/new-helper");
        assert!(!loaded.workspaces[0].takeover);
        let bytes = std::fs::read_to_string(&path).unwrap();
        assert!(!bytes.contains("takeover"));
    }
    #[test]
    fn a_stale_window_cannot_overwrite_another_windows_saved_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ssh-workspaces.json");
        let original = Catalog::default();
        let mut first = original.clone();
        first.remember(config()).unwrap();
        first.save(&path, &original).unwrap();
        let mut second = original.clone();
        let mut other = config();
        other.target.host = "other".into();
        second.remember(other).unwrap();
        assert!(second.save(&path, &original).is_err());
        assert_eq!(
            Catalog::load(&path).unwrap().workspaces[0].target.host,
            "devbox"
        );
    }
    #[test]
    fn corrupt_metadata_is_reported_and_invalid_paths_cannot_be_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ssh-workspaces.json");
        std::fs::write(&path, "broken").unwrap();
        assert!(Catalog::load(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken");
        let mut invalid = config();
        invalid.root = "relative".into();
        assert!(Catalog::default().remember(invalid).is_err());
    }
    #[test]
    fn managed_profiles_resolve_defaults_without_duplicating_saved_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ssh-workspaces.json");
        let mut automatic = config();
        automatic.managed = true;
        automatic.root.clear();
        automatic.target.helper.clear();
        automatic.target.state.clear();
        let mut catalog = Catalog::default();
        catalog.remember(automatic.clone()).unwrap();
        catalog.save(&path, &Catalog::default()).unwrap();
        let loaded = Catalog::load(&path).unwrap();
        assert!(loaded.workspaces[0].managed);
        automatic.root = "/home/me".into();
        automatic.target.helper = "/home/me/.local/helper".into();
        automatic.target.state = "/home/me/.local/state".into();
        catalog.remember(automatic).unwrap();
        assert_eq!(catalog.workspaces.len(), 1);
        assert_eq!(catalog.workspaces[0].root, "/home/me");
    }
}
