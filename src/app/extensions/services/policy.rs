//! Permission/source changes wake the UI; file I/O and watcher teardown stay off it.
use super::super::catalog;
use eframe::egui;
use notify::Watcher;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Weak,
    },
};
pub(super) struct Loaded {
    pub sequence: u64,
    pub catalog: Result<catalog::Catalog, String>,
}
pub(super) struct Controller {
    commands: Arc<SyncSender<()>>,
    pub sequence: Arc<AtomicU64>,
    pub loaded: Receiver<Loaded>,
}
impl Controller {
    pub fn start(config: PathBuf, ctx: egui::Context) -> Result<Self, String> {
        let (commands, incoming) = mpsc::sync_channel(4);
        let commands = Arc::new(commands);
        let weak: Weak<SyncSender<()>> = Arc::downgrade(&commands);
        let sequence = Arc::new(AtomicU64::new(0));
        let changed = sequence.clone();
        let (sender, loaded) = mpsc::sync_channel(1);
        std::thread::Builder::new().name("hedit-provider-policy".into()).spawn(move|| {
            let events=changed.clone();let wake=ctx.clone();let failed=Arc::new(AtomicBool::new(false));let fault=failed.clone();
            let mut watcher=match notify::recommended_watcher(move|event:Result<notify::Event,notify::Error>| {
                let relevant=match event {
                    Ok(event)=>!matches!(event.kind,notify::EventKind::Access(_))&&event.paths.iter().any(|path| {
                        path.parent().is_some_and(|p|p.file_name().is_some_and(|n|n=="extensions")) || path.file_name().is_some_and(|name|["extensions.lock","extensions-lua-grants.json","extensions-disabled.json","extensions"].iter().any(|v|name==*v))
                    }),Err(_)=>{fault.store(true,Ordering::Release);true},
                };
                if relevant {events.fetch_add(1,Ordering::AcqRel);if let Some(commands)=weak.upgrade(){let _=commands.try_send(());}wake.request_repaint();}
            }) {
                Ok(watcher)=>watcher,
                Err(_)=>{let _=sender.send(Loaded{sequence:0,catalog:Err("Automatic providers require a working permission watcher. Reload extensions to retry.".into())});ctx.request_repaint();return;}
            };
            if watcher.watch(&config,notify::RecursiveMode::NonRecursive).is_err()||watcher.watch(&config.join("extensions"),notify::RecursiveMode::NonRecursive).is_err() {
                let _=sender.send(Loaded{sequence:0,catalog:Err("Automatic providers require a working permission watcher. Reload extensions to retry.".into())});ctx.request_repaint();return;
            }
            loop {
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
            commands,
            sequence,
            loaded,
        })
    }
    pub fn reload(&self) {
        self.sequence.fetch_add(1, Ordering::AcqRel);
        let _ = self.commands.try_send(());
    }
}
