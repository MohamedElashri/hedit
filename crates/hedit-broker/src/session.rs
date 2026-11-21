//! Host-owned continuation state and aggregate work limits. All methods that
//! perform I/O run off the UI thread; review returns a plan without holding policy.
use crate::{
    cancel::Cancel,
    network::{Endpoint, Network},
    plan::Plan,
    resources::Resources,
    watch::Watch,
    workspace::Budget,
    Handle,
};
use hedit_plugin::{
    broker::{self, FailureCode, Intent, Response},
    sdk, service,
};
use std::{
    collections::BTreeMap,
    io,
    sync::Arc,
    time::{Duration, Instant},
};
static ADMISSION: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(16);
static ACTIVE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
const WORK: Duration = Duration::from_secs(10);
const BYTES: usize = 64 * 1024 * 1024;
enum Phase {
    Ready,
    Running(Instant),
    Review(Handle),
    Finished,
}
pub enum Effect {
    Continue,
    Review(Plan),
    Finished {
        transaction: Option<sdk::Transaction>,
        result: Option<Box<service::ResultSet>>,
        cache: sdk::Storage,
    },
}
pub struct Session {
    input: broker::Input,
    definition: Arc<crate::declaration::Definition>,
    resources: Option<Resources>,
    endpoints: BTreeMap<String, Arc<Endpoint>>,
    network: Arc<Network>,
    recovery: Arc<crate::recovery_store::RecoveryStore>,
    cancel: Arc<Cancel>,
    phase: Phase,
    spent: Duration,
    bytes: usize,
    changes: Vec<broker::Change>,
    watches: BTreeMap<Handle, Watch>,
    wake: Arc<dyn Fn() + Send + Sync>,
    watch_wake: Arc<tokio::sync::Notify>,
    permit: Option<tokio::sync::SemaphorePermit<'static>>,
    active: Option<tokio::sync::SemaphorePermit<'static>>,
}
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Broker request is unavailable in this session.",
    )
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "Invalid broker continuation.")
}
pub struct Config {
    pub package: sdk::Input,
    pub provider: Option<service::Request>,
    pub cache: sdk::Storage,
    pub definition: Arc<crate::declaration::Definition>,
    pub resources: Option<Resources>,
    pub endpoints: BTreeMap<String, Arc<Endpoint>>,
    pub network: Arc<Network>,
    pub recovery: Arc<crate::recovery_store::RecoveryStore>,
    pub cancel: Arc<Cancel>,
    pub wake: Arc<dyn Fn() + Send + Sync>,
}
impl Session {
    /// `Config` is built only after host policy resolution; none is deserialized.
    pub fn new(config: Config) -> io::Result<Self> {
        use crate::declaration::Capability;
        config
            .definition
            .validate(config.definition.has(Capability::Observe))
            .map_err(|_| invalid())?;
        let provider = config.definition.has(Capability::Observe);
        if (provider && config.provider.is_none())
            || (!provider
                && config.provider.as_ref().is_some_and(|request| {
                    request.kind != service::Kind::Presentation
                        || request.trigger != service::Trigger::Interaction
                }))
        {
            return Err(invalid());
        }
        if !config.definition.has(Capability::DocumentRead)
            && (!config.package.snapshot.text.is_empty()
                || config.package.snapshot.crlf
                || config.package.snapshot.revision != "0"
                || config.package.snapshot.language != "plain_text"
                || config.package.snapshot.selections != [sdk::Selection { anchor: 0, head: 0 }])
        {
            return Err(invalid());
        }
        if let Some(resources) = &config.resources {
            let root = resources.root(resources.scope())?;
            if !root.protected().durable() {
                return Err(invalid());
            }
            for (right, capability) in [
                (crate::workspace::Right::Discover, Capability::Discover),
                (crate::workspace::Right::Read, Capability::Read),
                (crate::workspace::Right::Edit, Capability::Edit),
                (crate::workspace::Right::Watch, Capability::Watch),
            ] {
                if root.require(right).is_ok() && !config.definition.has(capability) {
                    return Err(denied());
                }
            }
        }
        for (id, endpoint) in &config.endpoints {
            let requested = config
                .definition
                .endpoints
                .get(id)
                .ok_or_else(denied)?
                .endpoint()
                .map_err(|_| invalid())?;
            if !requested.same_policy(endpoint) {
                return Err(denied());
            }
        }
        if config.endpoints.len() > 8
            || config
                .endpoints
                .keys()
                .any(|key| !hedit_plugin::options::valid_id(key))
        {
            return Err(invalid());
        }
        let permit = ADMISSION.try_acquire().map_err(|_| {
            io::Error::new(io::ErrorKind::WouldBlock, "Broker session limit reached.")
        })?;
        let input = broker::Input {
            protocol_version: broker::VERSION,
            request: Handle::fresh()?,
            step: 0,
            scope: config.resources.as_ref().map(Resources::scope),
            package: config.package,
            may_edit: config.definition.has(Capability::DocumentEdit)
                && !config
                    .provider
                    .as_ref()
                    .is_some_and(|request| request.trigger == service::Trigger::Close),
            may_present: config.definition.has(Capability::Present),
            workspace_edit: config.resources.as_ref().is_some_and(|resources| {
                resources
                    .root(resources.scope())
                    .is_ok_and(|root| root.require(crate::workspace::Right::Edit).is_ok())
            }),
            provider: config.provider,
            response: None,
            cache: config.cache,
        };
        input.validate().map_err(|_| invalid())?;
        Ok(Self {
            input,
            definition: config.definition,
            resources: config.resources,
            endpoints: config.endpoints,
            network: config.network,
            recovery: config.recovery,
            cancel: config.cancel,
            phase: Phase::Ready,
            spent: Duration::ZERO,
            bytes: BYTES,
            changes: Vec::new(),
            watches: BTreeMap::new(),
            wake: config.wake,
            watch_wake: Arc::new(tokio::sync::Notify::new()),
            permit: Some(permit),
            active: None,
        })
    }
    fn live(&self) -> io::Result<()> {
        if self.cancel.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Broker session cancelled.",
            ));
        }
        if self.spent >= WORK {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Broker chain exhausted its work budget.",
            ));
        }
        Ok(())
    }
    pub fn begin_callback(&mut self) -> io::Result<broker::Input> {
        self.live()?;
        if !matches!(self.phase, Phase::Ready)
            || usize::from(self.input.step) >= broker::MAX_CALLBACKS
        {
            return Err(invalid());
        }
        self.active = Some(active()?);
        self.phase = Phase::Running(Instant::now());
        Ok(self.input.clone())
    }
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
    pub fn request(&self) -> Handle {
        self.input.request
    }
    pub(crate) fn charge_host_work(&mut self, elapsed: Duration) -> io::Result<()> {
        self.spent = self.spent.saturating_add(elapsed);
        self.live()
    }
    pub(crate) fn host_work(&self) -> io::Result<tokio::sync::SemaphorePermit<'static>> {
        self.live()?;
        active()
    }
    fn respond(&mut self, response: Response) -> io::Result<Effect> {
        response.validate().map_err(|_| invalid())?;
        self.input.response = Some(response);
        self.input.step = self.input.step.checked_add(1).ok_or_else(invalid)?;
        self.phase = Phase::Ready;
        self.active.take();
        self.live()?;
        Ok(Effect::Continue)
    }
    /// The callback's identity is checked before authorization or any effect.
    /// The returned guard must protect current grants; cancellation must be sent
    /// before exclusive revocation, so a transfer releases its disclosure guard.
    pub async fn accept<G>(
        &mut self,
        output: broker::Output,
        authority: impl FnMut() -> io::Result<G>,
    ) -> io::Result<Effect> {
        let result = self.accept_inner(output, authority).await;
        if result.is_err() || matches!(result, Ok(Effect::Finished { .. })) {
            self.finish(result.is_err());
        }
        result
    }
    fn finish(&mut self, failed: bool) {
        self.phase = Phase::Finished;
        if failed {
            self.cancel.cancel();
        }
        self.watches.clear();
        self.changes.clear();
        self.resources.take();
        self.endpoints.clear();
        self.input.response.take();
        self.input.cache.0.clear();
        self.input.package.snapshot.text.clear();
        self.permit.take();
        self.active.take();
    }
    async fn accept_inner<G>(
        &mut self,
        output: broker::Output,
        mut authority: impl FnMut() -> io::Result<G>,
    ) -> io::Result<Effect> {
        let Phase::Running(start) = self.phase else {
            return Err(invalid());
        };
        self.spent += start.elapsed();
        self.phase = Phase::Finished;
        self.live()?;
        output.validate(&self.input).map_err(|_| invalid())?;
        if !self
            .definition
            .has(crate::declaration::Capability::DocumentRead)
        {
            if let Intent::Finish {
                result: Some(result),
                ..
            } = &output.intent
            {
                if !matches!(result.as_ref(), service::ResultSet::Closed)
                    && !matches!(result.as_ref(),service::ResultSet::Presentation(presentation) if presentation.annotations.is_empty() && presentation.quick_list.as_ref().is_none_or(|list|list.items.iter().all(|item|matches!(item.action,service::Action::Callback{..}))))
                {
                    return Err(denied());
                }
            }
        }
        if usize::from(self.input.step) + 1 >= broker::MAX_CALLBACKS
            && !matches!(output.intent, Intent::Finish { .. })
        {
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "Broker continuation limit reached.",
            ));
        }
        self.input.cache = output.cache;
        let intent = match output.intent {
            Intent::WaitChanges { watch } => return self.wait_changes(watch, authority).await,
            intent => intent,
        };
        let started = Instant::now();
        let _guard = authority()?;
        let atomic = self.cancel.flag();
        let mut budget = Budget::new(atomic, WORK.saturating_sub(self.spent), self.bytes);
        let result = match intent {
            Intent::Finish {
                transaction,
                result,
            } => {
                return Ok(Effect::Finished {
                    transaction,
                    result,
                    cache: std::mem::take(&mut self.input.cache),
                })
            }
            Intent::Discover { scope, query } => {
                let listing = self
                    .resources
                    .as_ref()
                    .ok_or_else(denied)?
                    .root(scope)?
                    .discover(&query, &mut budget)?;
                let mut entries = Vec::new();
                let mut text = 0;
                let mut truncated = listing.truncated;
                for entry in listing.entries {
                    text += entry.relative.len();
                    if text > broker::MAX_CHUNK_BYTES {
                        truncated = true;
                        break;
                    }
                    entries.push(broker::Entry {
                        relative: entry.relative,
                        directory: entry.directory,
                        bytes: entry
                            .bytes
                            .min((crate::workspace::MAX_FILE_BYTES + 1) as u64),
                    });
                }
                Response::Discovered { entries, truncated }
            }
            Intent::Search { scope, query } => {
                let results = crate::search::search(
                    self.resources.as_ref().ok_or_else(denied)?.root(scope)?,
                    &query,
                    &mut budget,
                )?;
                let mut matches = Vec::new();
                let mut text = 0;
                let mut truncated = results.truncated;
                for item in results.matches {
                    text += item.relative.len() + item.excerpt.len();
                    if text > broker::MAX_CHUNK_BYTES {
                        truncated = true;
                        break;
                    }
                    matches.push(broker::SearchMatch {
                        relative: item.relative,
                        start: item.start,
                        finish: item.finish,
                        excerpt: item.excerpt,
                    });
                }
                Response::Searched { matches, truncated }
            }
            Intent::Open { scope, relative } => {
                let version = self.resources.as_mut().ok_or_else(denied)?.load(
                    scope,
                    &relative,
                    &mut budget,
                )?;
                Response::Opened(broker::Resource {
                    resource: version.resource,
                    version: version.version,
                    relative: version.relative,
                    bytes: version.bytes,
                })
            }
            Intent::OpenMany { scope, paths } => {
                let mut unique = std::collections::BTreeSet::new();
                let mut files = Vec::new();
                let mut bytes = 0;
                let resources = self.resources.as_mut().ok_or_else(denied)?;
                for relative in paths {
                    let resource = resources.load(scope, &relative, &mut budget)?;
                    let snapshot = resources.get(resource.resource, resource.version)?;
                    if !unique.insert(snapshot.snapshot().stamp.identity) {
                        return Err(invalid());
                    }
                    bytes += resource.relative.len() + resource.bytes;
                    if bytes > broker::MAX_CHUNK_BYTES {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Batch text exceeds 64 KiB. Use versioned chunks for larger files.",
                        ));
                    }
                    let text = resources
                        .chunk(
                            resource.resource,
                            resource.version,
                            0,
                            resource.bytes.max(1),
                            &mut budget,
                        )?
                        .text;
                    files.push(broker::ResourceText {
                        resource: broker::Resource {
                            resource: resource.resource,
                            version: resource.version,
                            relative: resource.relative,
                            bytes: resource.bytes,
                        },
                        text,
                    });
                }
                Response::OpenedMany { files }
            }
            Intent::Read {
                resource,
                version,
                offset,
                maximum,
            } => {
                let chunk = self.resources.as_ref().ok_or_else(denied)?.chunk(
                    resource,
                    version,
                    offset,
                    maximum,
                    &mut budget,
                )?;
                Response::Read {
                    text: chunk.text,
                    next: chunk.next,
                }
            }
            Intent::Release { resource } => {
                self.resources
                    .as_mut()
                    .ok_or_else(denied)?
                    .release(resource)?;
                self.changes.retain(|change| change.resource != resource);
                Response::Released {}
            }
            Intent::StageEdits { scope, change } => {
                let resources = self.resources.as_ref().ok_or_else(denied)?;
                resources
                    .root(scope)?
                    .require(crate::workspace::Right::Edit)?;
                if self.changes.len() >= crate::plan::MAX_PLAN_FILES
                    || self
                        .changes
                        .iter()
                        .any(|existing| existing.resource == change.resource)
                {
                    return Err(invalid());
                }
                // Validate immediately, then revalidate the complete plan at review.
                let _ = Plan::prepare(resources, scope, vec![change.clone()], &mut budget)?;
                let bytes: usize = self
                    .changes
                    .iter()
                    .chain(std::iter::once(&change))
                    .flat_map(|change| &change.edits)
                    .map(|edit| edit.text.len())
                    .sum();
                if bytes > crate::plan::MAX_REPLACEMENT_BYTES {
                    return Err(invalid());
                }
                self.changes.push(change);
                Response::Staged {
                    files: self.changes.len(),
                }
            }
            Intent::ReviewEdits { scope, changes } => {
                if let Some(changes) = changes {
                    if self.changes.len() + changes.len() > crate::plan::MAX_PLAN_FILES {
                        return Err(invalid());
                    }
                    self.changes.extend(changes);
                }
                let plan = Plan::prepare(
                    self.resources.as_ref().ok_or_else(denied)?,
                    scope,
                    std::mem::take(&mut self.changes),
                    &mut budget,
                )?;
                self.bytes = budget.remaining_bytes();
                self.spent += started.elapsed();
                self.phase = Phase::Review(plan.id());
                self.active.take();
                return Ok(Effect::Review(plan));
            }
            Intent::Watch { scope } => {
                if !self.watches.is_empty() {
                    return Err(invalid());
                }
                let notify = self.watch_wake.clone();
                let wake = self.wake.clone();
                let watch = Watch::start(
                    self.resources.as_ref().ok_or_else(denied)?.root(scope)?,
                    &mut budget,
                    Arc::new(move || {
                        notify.notify_one();
                        wake();
                    }),
                )?;
                let truncated = watch.truncated();
                let id = Handle::fresh()?;
                self.watches.insert(id, watch);
                Response::Watching {
                    watch: id,
                    truncated,
                }
            }
            Intent::Unwatch { watch } => {
                self.watches.remove(&watch).ok_or_else(denied)?;
                Response::Unwatched {}
            }
            Intent::Https { endpoint, body } => {
                let endpoint = self.endpoints.get(&endpoint).ok_or_else(denied)?;
                let response = tokio::time::timeout(
                    WORK.saturating_sub(self.spent),
                    self.network
                        .execute(endpoint, body, &self.cancel, &mut authority),
                )
                .await
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::TimedOut, "Broker request timed out.")
                })??;
                Response::Https {
                    status: response.status,
                    body: response.body,
                }
            }
            Intent::Tool {
                profile, arguments, ..
            } => {
                let tool = self.definition.tools.get(&profile).ok_or_else(denied)?;
                if tool.protocol != crate::declaration::ToolProtocol::Filter {
                    return Err(denied());
                }
                tool.argv(&arguments).map_err(|_| denied())?;
                Response::Failed {
                    code: FailureCode::Unavailable,
                }
            }
            Intent::LanguageServer { profile, .. } => {
                let tool = self.definition.tools.get(&profile).ok_or_else(denied)?;
                if tool.protocol != crate::declaration::ToolProtocol::LanguageServer {
                    return Err(denied());
                }
                Response::Failed {
                    code: FailureCode::Unavailable,
                }
            }
            Intent::WaitChanges { .. } => unreachable!("handled before acquiring policy"),
        };
        self.bytes = budget.remaining_bytes();
        self.spent += started.elapsed();
        self.respond(result)
    }
    async fn wait_changes<G>(
        &mut self,
        id: Handle,
        mut authority: impl FnMut() -> io::Result<G>,
    ) -> io::Result<Effect> {
        self.active.take();
        let waiting = async {
            loop {
                let watch = self.watches.get(&id).ok_or_else(denied)?;
                match watch.ready_after() {
                    Some(delay) if delay.is_zero() => break,
                    Some(delay) => tokio::time::sleep(delay).await,
                    None => self.watch_wake.notified().await,
                }
            }
            io::Result::Ok(())
        };
        // No worker, filesystem scan, policy guard or periodic polling while
        // idle. Unused subscriptions expire instead of occupying a host forever.
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => return Err(io::Error::new(io::ErrorKind::Interrupted,"Workspace watch cancelled.")),
            result = tokio::time::timeout(Duration::from_secs(30),waiting) => result.map_err(|_|io::Error::new(io::ErrorKind::TimedOut,"Workspace watch expired while idle."))??,
        }
        self.live()?;
        self.active = Some(active()?);
        let _guard = authority()?;
        let started = Instant::now();
        let mut budget = Budget::new(
            self.cancel.flag(),
            WORK.saturating_sub(self.spent),
            self.bytes,
        );
        let resources = self.resources.as_ref().ok_or_else(denied)?;
        let changes = self
            .watches
            .get_mut(&id)
            .ok_or_else(denied)?
            .drain(resources.root(resources.scope())?, &mut budget)?
            .ok_or_else(invalid)?;
        let mut paths = Vec::new();
        let mut bytes = 0;
        let mut truncated = changes.truncated;
        for path in changes.paths {
            bytes += path.len();
            if bytes > broker::MAX_CHUNK_BYTES {
                truncated = true;
                break;
            }
            paths.push(path);
        }
        self.bytes = budget.remaining_bytes();
        self.spent += started.elapsed();
        self.respond(Response::Changed { paths, truncated })
    }
    pub fn discard_review(&mut self, plan: &Plan) -> io::Result<Effect> {
        self.live()?;
        if !matches!(self.phase,Phase::Review(id) if id==plan.id()) {
            return Err(invalid());
        }
        self.respond(Response::Failed {
            code: FailureCode::Cancelled,
        })
    }
    /// Called off the UI only after the host accepts this exact preview and
    /// freezes matching clean buffers. The caller retains the journal on error.
    pub fn prepare_review<G>(
        &mut self,
        plan: &Plan,
        authority: impl FnMut() -> io::Result<G>,
    ) -> io::Result<crate::journal::Journal> {
        let _active = self.host_work()?;
        self.prepare_review_in_slot(plan, authority)
    }
    pub(crate) fn prepare_review_in_slot<G>(
        &mut self,
        plan: &Plan,
        mut authority: impl FnMut() -> io::Result<G>,
    ) -> io::Result<crate::journal::Journal> {
        self.live()?;
        if !matches!(self.phase,Phase::Review(id) if id==plan.id()) {
            return Err(invalid());
        }
        let _guard = authority()?;
        let started = Instant::now();
        let mut budget = Budget::new(
            self.cancel.flag(),
            WORK.saturating_sub(self.spent),
            self.bytes,
        );
        let journal = crate::journal::Journal::prepare(
            self.resources.as_ref().ok_or_else(denied)?,
            plan,
            &mut budget,
            &self.recovery,
        );
        self.bytes = budget.remaining_bytes();
        self.spent += started.elapsed();
        journal
    }
    pub fn apply_review<G>(
        &mut self,
        plan: &Plan,
        journal: &crate::journal::Journal,
        authority: impl FnMut() -> io::Result<G>,
    ) -> io::Result<Effect> {
        let _active = self.host_work()?;
        self.apply_review_in_slot(plan, journal, authority)
    }
    pub(crate) fn apply_review_in_slot<G>(
        &mut self,
        plan: &Plan,
        journal: &crate::journal::Journal,
        mut authority: impl FnMut() -> io::Result<G>,
    ) -> io::Result<Effect> {
        self.live()?;
        if !matches!(self.phase,Phase::Review(id) if id==plan.id())
            || !plan.live()
            || journal.plan_id() != plan.id()
        {
            return Err(invalid());
        }
        let started = Instant::now();
        let mut budget = Budget::new(
            self.cancel.flag(),
            WORK.saturating_sub(self.spent),
            self.bytes,
        );
        let receipts = (|| {
            journal.apply(
                self.resources
                    .as_ref()
                    .ok_or_else(denied)?
                    .root(plan.scope())?,
                &mut budget,
                &mut authority,
            )
        })();
        self.bytes = budget.remaining_bytes();
        self.spent += started.elapsed();
        let result = receipts.and_then(|receipts| {
            self.respond(Response::Edited {
                files: receipts.len(),
            })
        });
        if result.is_err() {
            self.finish(true);
        }
        result
    }
}
fn active() -> io::Result<tokio::sync::SemaphorePermit<'static>> {
    ACTIVE
        .try_acquire()
        .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "Extension broker is busy."))
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
#[cfg(test)]
mod tests;
