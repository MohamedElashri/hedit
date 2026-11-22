//! Permission/source changes wake the UI; file I/O and watcher teardown stay off it.
use super::super::catalog;
use eframe::egui;
use notify::Watcher;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Condvar, Mutex, Weak,
    },
};
pub(super) struct Loaded {
    pub sequence: u64,
    pub catalog: Result<catalog::Catalog, String>,
}
pub(super) struct Controller {
    _commands: Arc<SyncSender<()>>, // Keep the native callback's weak sender live.
    pub sequence: Arc<AtomicU64>,
    pub loaded: Receiver<Loaded>,
    pub monitor: Monitor,
}
#[derive(Clone)]
pub(in crate::app::extensions) struct Monitor {
    state: Arc<MonitorState>,
    pub sequence: Arc<AtomicU64>,
}
struct MonitorState {
    listeners: Mutex<Vec<Weak<hedit_broker::cancel::Cancel>>>,
    ready: Mutex<Option<bool>>,
    changed: Condvar,
}
impl Monitor {
    fn invalidate(&self) {
        self.sequence.fetch_add(1, Ordering::AcqRel);
        let mut listeners = self
            .state
            .listeners
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        listeners.retain(|listener| {
            if let Some(cancel) = listener.upgrade() {
                cancel.cancel();
                true
            } else {
                false
            }
        });
    }
    fn ready(&self, success: bool) {
        *self.state.ready.lock().unwrap_or_else(|p| p.into_inner()) = Some(success);
        self.state.changed.notify_all();
        if !success {
            self.invalidate();
        }
    }
    /// Only the bounded host setup thread waits for native watcher readiness.
    pub(in crate::app::extensions) fn register(
        &self,
        cancel: &Arc<hedit_broker::cancel::Cancel>,
    ) -> std::io::Result<u64> {
        let ready = self.state.ready.lock().unwrap_or_else(|p| p.into_inner());
        let (ready, _) = self
            .state
            .changed
            .wait_timeout_while(ready, std::time::Duration::from_secs(2), |ready| {
                ready.is_none()
            })
            .unwrap_or_else(|p| p.into_inner());
        if *ready != Some(true) {
            return Err(std::io::Error::other(
                "The extension policy watcher is unavailable.",
            ));
        }
        let mut listeners = self
            .state
            .listeners
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        listeners.retain(|listener| listener.strong_count() != 0);
        if listeners
            .iter()
            .any(|listener| listener.ptr_eq(&Arc::downgrade(cancel)))
        {
            return Ok(self.sequence.load(Ordering::Acquire));
        }
        // Rotating host contexts and document-owned native registries have
        // separate lifetimes, while native process admission remains capped.
        if listeners.len() >= 32 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "Policy subscription limit reached.",
            ));
        }
        listeners.push(Arc::downgrade(cancel));
        Ok(self.sequence.load(Ordering::Acquire))
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.monitor.ready(false);
    }
}
fn relevant(config: &std::path::Path, event: &notify::Event) -> bool {
    use notify::{event::ModifyKind, EventKind};
    if event.need_rescan() {
        return true;
    }
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    let namespace = matches!(
        event.kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Name(_))
            | EventKind::Any
            | EventKind::Other
    );
    event.paths.iter().any(|path| {
        path.parent() == Some(config.join("extensions").as_path())
            || (path.parent() == Some(config.join("extensions-broker").as_path())
                && path
                    .file_name()
                    .is_some_and(|name| name == "approvals.json"))
            || (path.parent() == Some(config)
                && path.file_name().is_some_and(|name| {
                    [
                        "extensions.lock",
                        "extensions-lua-grants.json",
                        "extensions-disabled.json",
                        "extensions-stop.json",
                    ]
                    .iter()
                    .any(|file| name == *file)
                        || (namespace && (name == "extensions" || name == "extensions-broker"))
                }))
            || (path == config && namespace)
    })
}

impl Controller {
    pub fn start(config: PathBuf, ctx: egui::Context) -> Result<Self, String> {
        let (commands, incoming) = mpsc::sync_channel(4);
        let commands = Arc::new(commands);
        let weak: Weak<SyncSender<()>> = Arc::downgrade(&commands);
        let sequence = Arc::new(AtomicU64::new(0));
        let monitor = Monitor {
            state: Arc::new(MonitorState {
                listeners: Mutex::new(Vec::new()),
                ready: Mutex::new(None),
                changed: Condvar::new(),
            }),
            sequence: sequence.clone(),
        };
        let thread_monitor = monitor.clone();
        let changed = sequence.clone();
        let (sender, loaded) = mpsc::sync_channel(1);
        std::thread::Builder::new().name("hedit-provider-policy".into()).spawn(move|| {
            let watch_config=match config.canonicalize() { Ok(path)=>path,Err(_)=>{thread_monitor.ready(false);let _=sender.send(Loaded{sequence:0,catalog:Err("The extension configuration directory is unavailable.".into())});ctx.request_repaint();return;} };
            let event_config=watch_config.clone();
            let events=thread_monitor.clone();let wake=ctx.clone();let failed=Arc::new(AtomicBool::new(false));let fault=failed.clone();
            let mut watcher=match notify::recommended_watcher(move|event:Result<notify::Event,notify::Error>| {
                let relevant=match event {
                    Ok(event)=>{if event.need_rescan(){fault.store(true,Ordering::Release);events.ready(false);}relevant(&event_config,&event)},Err(_)=>{fault.store(true,Ordering::Release);events.ready(false);true},
                };
                if relevant {events.invalidate();if let Some(commands)=weak.upgrade(){let _=commands.try_send(());}wake.request_repaint();}
            }) {
                Ok(watcher)=>watcher,
                Err(_)=>{thread_monitor.ready(false);let _=sender.send(Loaded{sequence:0,catalog:Err("Automatic providers require a working permission watcher. Reload extensions to retry.".into())});ctx.request_repaint();return;}
            };
            if watcher.watch(&watch_config,notify::RecursiveMode::NonRecursive).is_err()||watcher.watch(&watch_config.join("extensions"),notify::RecursiveMode::NonRecursive).is_err() {
                thread_monitor.ready(false);let _=sender.send(Loaded{sequence:0,catalog:Err("Automatic providers require a working permission watcher. Reload extensions to retry.".into())});ctx.request_repaint();return;
            }
            let broker_dir=watch_config.join("extensions-broker");
            let mut broker_watched=false;
            loop {
                if !broker_watched && broker_dir.exists() {
                    if watcher.watch(&broker_dir,notify::RecursiveMode::NonRecursive).is_err() {
                        failed.store(true,Ordering::Release);thread_monitor.ready(false);
                    } else { broker_watched=true; }
                }
                if !failed.load(Ordering::Acquire) { thread_monitor.ready(true); }
                let mut attempts=0;
                loop {
                    attempts+=1;let before=changed.load(Ordering::Acquire);
                    let catalog=if failed.load(Ordering::Acquire){Err("The permission watcher failed. Stop and reload extensions to retry.".into())}else{catalog::read_consistent(&config)};
                    if changed.load(Ordering::Acquire)!=before && attempts<8 {continue;}
                    let catalog=if changed.load(Ordering::Acquire)==before{catalog}else{Err("Extension policy is changing too quickly. Reload extensions to retry.".into())};
                    if sender.send(Loaded{sequence:before,catalog}).is_err(){return;}ctx.request_repaint();break;
                }
                if incoming.recv().is_err(){return;}
                while incoming.try_recv().is_ok(){}
            }
        }).map_err(|_|"The provider policy monitor could not start.")?;
        Ok(Self {
            _commands: commands,
            sequence,
            loaded,
            monitor,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistent_scope_subscriptions_do_not_grow_with_callbacks() {
        let monitor = Monitor {
            state: Arc::new(MonitorState {
                listeners: Mutex::new(vec![]),
                ready: Mutex::new(Some(true)),
                changed: Condvar::new(),
            }),
            sequence: Arc::new(AtomicU64::new(0)),
        };
        let cancel = Arc::new(hedit_broker::cancel::Cancel::default());
        for _ in 0..128 {
            assert_eq!(monitor.register(&cancel).unwrap(), 0);
        }
        assert_eq!(monitor.state.listeners.lock().unwrap().len(), 1);
        monitor.invalidate();
        assert!(cancel.is_cancelled());
    }
    #[test]
    fn journal_metadata_is_not_policy_and_renamed_policy_namespaces_are() {
        use notify::{
            event::{DataChange, MetadataKind, ModifyKind, RenameMode},
            Event, EventKind,
        };
        let config = std::path::Path::new("config");
        let metadata = EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any));
        for path in [
            "config/extensions-broker",
            "config/extensions-broker/protected-identities",
            "config/extensions-broker/recovery/record.json",
        ] {
            assert!(!relevant(
                config,
                &Event::new(metadata).add_path(path.into())
            ));
        }
        for path in [
            "config/extensions-stop.json",
            "config/extensions-broker/approvals.json",
            "config/extensions/plugin.json",
        ] {
            assert!(relevant(
                config,
                &Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Any)))
                    .add_path(path.into())
            ));
        }
        assert!(relevant(
            config,
            &Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                .add_path("config/extensions-broker".into())
        ));
    }
}
