//! Exact-source authority and host setup shared by commands and providers.
use super::*;
pub(in crate::app::extensions) struct Context {
    pub(super) config: PathBuf,
    pub(super) command: String,
    pub(super) plugin: LuaPlugin,
    pub(super) package: sdk::Input,
    pub(super) target: Option<lua::Target>,
    pub(super) editor: Arc<hedit::Editor>,
    pub(super) monitor: Monitor,
    pub(super) generation: OnceLock<(Vec<u8>, u64)>,
    pub(super) directory: OnceLock<PathBuf>,
    pub(super) live: AtomicBool,
    native_services: OnceLock<Arc<hedit_broker::lsp::Registry>>,
    native_shared: AtomicBool,
    lifetime: Option<hedit::lifetime::Lease>,
    pub(super) commit: commit::Signals,
}
impl Context {
    pub(in crate::app::extensions) fn share_services(
        &self,
        services: Arc<hedit_broker::lsp::Registry>,
    ) -> io::Result<()> {
        self.native_services.set(services).map_err(|_| denied())?;
        self.native_shared.store(true, Ordering::Release);
        Ok(())
    }
    pub(in crate::app::extensions) fn snapshot(&self) -> &sdk::Snapshot {
        &self.package.snapshot
    }
    pub(in crate::app::extensions) fn directory(&self) -> Option<&std::path::Path> {
        self.directory.get().map(PathBuf::as_path)
    }
    pub(in crate::app::extensions) fn applied(&self) -> bool {
        self.commit.applied.load(Ordering::Acquire)
    }
    pub(in crate::app::extensions) fn committing(&self) -> bool {
        self.commit.committing.load(Ordering::Acquire)
    }
    pub(in crate::app::extensions) fn decide(
        &self,
        host: &Host,
        plan: hedit_broker::Handle,
        accept: bool,
    ) -> Result<(), String> {
        if !self.current() {
            return Err("The provider context changed.".into());
        }
        self.commit.committing.store(accept, Ordering::Release);
        if host.decide(host::Decision { plan, accept }).is_err() {
            self.commit.committing.store(false, Ordering::Release);
            return Err("This workspace preview expired.".into());
        }
        Ok(())
    }
    pub(in crate::app::extensions) fn result_authority(
        self: &Arc<Self>,
        ctx: &egui::Context,
    ) -> Result<Receiver<Option<Receipt>>, String> {
        let permit = limit::Reservation::reserve(&CHECKS, 1, 4)
            .map_err(|_| "Extension authorization is busy.")?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let context = self.clone();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("hedit-broker-result".into())
            .spawn(move || {
                let _permit = permit;
                let authorized = context
                    .authorize(&AtomicBool::new(false))
                    .ok()
                    .map(Receipt::new);
                match authorized {
                    Some((receipt, expiry)) => {
                        let _ = sender.send(Some(receipt));
                        ctx.request_repaint();
                        expiry.wait();
                    }
                    None => {
                        let _ = sender.send(None);
                        ctx.request_repaint();
                    }
                }
            })
            .map_err(|_| "Result authorization could not start.")?;
        Ok(receiver)
    }
}
impl Context {
    fn scope_current(&self) -> bool {
        self.live.load(Ordering::Acquire)
            && self.generation.get().is_none_or(|(_, sequence)| {
                self.monitor.sequence.load(Ordering::Acquire) == *sequence
            })
            && (self.commit.committing.load(Ordering::Acquire)
                || self.target.is_none()
                || self
                    .lifetime
                    .as_ref()
                    .is_some_and(hedit::lifetime::Lease::current))
    }
    fn revision_current(&self) -> Option<bool> {
        if self.commit.committing.load(Ordering::Acquire) {
            return Some(true);
        }
        let Some(target) = &self.target else {
            return Some(true);
        };
        let Ok(document) = self.editor.get(target.document()) else {
            return Some(false);
        };
        let result = match document.try_lock() {
            Ok(document) => Some(document.info().revision == target.revision()),
            Err(std::sync::TryLockError::WouldBlock) => None,
            Err(std::sync::TryLockError::Poisoned(_)) => Some(false),
        };
        result
    }
    /// Retain UI state during contention. This does not authorize disclosure or effects.
    pub(in crate::app::extensions) fn retained(&self) -> bool {
        self.scope_current() && self.revision_current() != Some(false)
    }
    /// A nonblocking revision check for publication and explicit UI decisions.
    pub(in crate::app::extensions) fn current(&self) -> bool {
        self.scope_current() && self.revision_current() == Some(true)
    }
    /// Called only on host/authorization threads, never while pumping the UI.
    fn verified_current(&self, cancelled: &AtomicBool) -> bool {
        let deadline = Instant::now() + Duration::from_millis(200);
        loop {
            if cancelled.load(Ordering::Acquire) || !self.scope_current() {
                return false;
            }
            match self.revision_current() {
                Some(current) => return current,
                None if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                None => return false,
            }
        }
    }
    pub(super) fn approved_plugin(&self) -> LuaPlugin {
        let mut plugin = self.plugin.clone();
        plugin.broker_epoch = self
            .generation
            .get()
            .map(|(epoch, _)| Arc::new(epoch.clone()));
        plugin
    }
    pub(in crate::app::extensions) fn authorize(&self, cancelled: &AtomicBool) -> io::Result<File> {
        if !self.verified_current(cancelled) {
            return Err(denied());
        }
        let guard = catalog::broker_authorization(&self.config, &self.approved_plugin())
            .ok_or_else(denied)?;
        if !self.verified_current(cancelled) {
            return Err(denied());
        }
        Ok(guard)
    }
}
impl Context {
    pub(in crate::app::extensions) fn new(
        config: PathBuf,
        command: String,
        plugin: LuaPlugin,
        package: sdk::Input,
        target: Option<lua::Target>,
        editor: Arc<hedit::Editor>,
        monitor: Monitor,
    ) -> Arc<Self> {
        let owner = Some(plugin.plugin_id.clone());
        let lifetime = target
            .as_ref()
            .and_then(|target| editor.document_lifetime(target.document()).ok());
        Arc::new(Self {
            config,
            command,
            plugin,
            package,
            target,
            editor,
            monitor,
            generation: OnceLock::new(),
            directory: OnceLock::new(),
            live: AtomicBool::new(true),
            native_services: OnceLock::new(),
            native_shared: AtomicBool::new(false),
            lifetime,
            commit: commit::Signals {
                owner,
                ..Default::default()
            },
        })
    }
    pub(in crate::app::extensions) fn close(&self) {
        self.live.store(false, Ordering::Release);
        if !self.native_shared.load(Ordering::Acquire) {
            if let Some(services) = self.native_services.get() {
                services.stop();
            }
        }
    }
    pub(in crate::app::extensions) fn start(
        self: &Arc<Self>,
        app: &HeditApp,
        ctx: &egui::Context,
        cache: sdk::Storage,
        request: Option<service::Request>,
        arguments: BTreeMap<String, hedit::plugin_options::Value>,
    ) -> io::Result<Host> {
        self.commit.applied.store(false, Ordering::Release);
        self.commit.incomplete.store(false, Ordering::Release);
        self.commit.committing.store(false, Ordering::Release);
        let setup = self.clone();
        let authority = self.clone();
        let editor = self.editor.clone();
        let config = self.config.clone();
        let private_directory = app.workspace.store.directory().to_owned();
        let sender = app.tx.clone();
        let wake = ctx.clone();
        let commit_authority = self.clone();
        let following: BTreeSet<_> = app
            .workspace
            .open
            .iter()
            .filter(|buffer| buffer.follow)
            .map(|buffer| buffer.page.info.id)
            .collect();
        let setup = move |cancel| {
            let state = State::open(&config, [private_directory])?;
            let sequence = setup.monitor.register(&cancel)?;
            let guard = setup.authorize(&AtomicBool::new(false))?;
            let epoch = catalog::broker_epoch(&config, &guard).map_err(|_| denied())?;
            if let Some((expected, known)) = setup.generation.get() {
                if expected != &epoch || *known != sequence {
                    return Err(denied());
                }
            } else {
                setup
                    .generation
                    .set((epoch, sequence))
                    .map_err(|_| denied())?;
            }
            let definition = setup.plugin.broker.clone().ok_or_else(denied)?;
            let token = serde_json::from_value(setup.plugin.grant.approval_id().into())
                .map_err(|_| denied())?;
            let resolved = state.resolve(&setup.command, token, &definition)?;
            let services = setup.native_services.get_or_init(Default::default).clone();
            let server = definition.tools.iter().any(|(profile, tool)| {
                tool.protocol == hedit_broker::declaration::ToolProtocol::LanguageServer
                    && resolved.tools.contains_key(profile)
            });
            if server
                && !request
                    .as_ref()
                    .is_some_and(|request| request.trigger == service::Trigger::Close)
            {
                services.bind_lifetime(setup.lifetime.as_ref().ok_or_else(denied)?)?;
                if setup.monitor.register(&services.cancellation())? != sequence {
                    services.stop();
                    return Err(denied());
                }
            }
            let document_relative = resolved.resources.as_ref().and_then(|resources| {
                let path = setup
                    .target
                    .as_ref()
                    .and_then(|target| setup.editor.get(target.document()).ok())
                    .and_then(|doc| {
                        doc.try_lock()
                            .ok()
                            .and_then(|doc| doc.local_path().map(std::path::Path::to_owned))
                    })?;
                let path = path.canonicalize().ok()?;
                let root = resources.root(resources.scope()).ok()?;
                let relative = path.strip_prefix(root.locator()).ok()?;
                let parts = relative
                    .components()
                    .map(|component| {
                        let std::path::Component::Normal(part) = component else {
                            return None;
                        };
                        part.to_str()
                            .filter(|part| !part.starts_with('.') && !part.contains('\\'))
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(parts.join("/"))
            });
            if let Some(resources) = &resolved.resources {
                let path = resources.root(resources.scope())?.locator().to_owned();
                if setup.directory.get().is_some_and(|known| known != &path) {
                    return Err(denied());
                }
                let _ = setup.directory.set(path);
            }
            let mut package = setup.package.clone();
            package.arguments = arguments;
            Ok(hedit_broker::session::Config {
                services,
                scope_epoch: setup.generation.get().map(|(epoch, _)| epoch.clone()),
                document_relative,
                source_token: Some(token),
                tools: resolved.tools,
                owner: Some(hedit_broker::journal::Owner {
                    command: setup.command.clone(),
                    digest: setup.plugin.grant.digest().into(),
                }),
                package,
                provider: request,
                cache,
                definition,
                resources: resolved.resources,
                endpoints: resolved.endpoints,
                network: NETWORK.get_or_init(|| Arc::new(Default::default())).clone(),
                recovery: state.recovery(),
                cancel,
                wake: Arc::new(|| {}),
            })
        };
        let authority: Arc<host::Authority> = Arc::new(move |cancel| authority.authorize(cancel));
        let prepare: Arc<host::PrepareCommit> = Arc::new(move |plan, cancel| {
            let _guard = commit_authority.authorize(cancel)?;
            let result = commit::prepare(
                &editor,
                plan,
                cancel,
                sender.clone(),
                wake.clone(),
                &following,
                commit_authority.commit.clone(),
            );
            if result.is_err() {
                commit_authority
                    .commit
                    .committing
                    .store(false, Ordering::Release);
            }
            result
        });
        let wake: Arc<dyn Fn() + Send + Sync> = {
            let ctx = ctx.clone();
            Arc::new(move || ctx.request_repaint())
        };
        if self.plugin.provider.is_some() {
            Host::start_provider(setup, authority, prepare, wake)
        } else {
            Host::start_scoped(setup, authority, prepare, wake)
        }
    }
}
