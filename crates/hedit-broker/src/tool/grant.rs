//! Host-only execution authority produced by exact-source resource resolution.
use super::{Binding, Projection, Runs, Store};
use crate::{
    declaration::{ToolDefinition, ToolProtocol},
    resources::Resources,
    workspace::Budget,
    Handle,
};
use std::{
    io,
    sync::{atomic::AtomicBool, Arc},
    time::Instant,
};
pub struct Grant {
    image: Handle,
    binding: Binding,
    definition: ToolDefinition,
    images: Arc<Store>,
    runs: Arc<Runs>,
    bytes: u64,
}
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "The native tool is unavailable for this exact source and profile.",
    )
}
impl Grant {
    pub(crate) fn is_server(&self) -> bool {
        self.definition.protocol == ToolProtocol::LanguageServer
    }
    pub(crate) fn key(&self) -> (Binding, Handle) {
        (self.binding.clone(), self.image)
    }
    pub(crate) fn workspace(&self) -> bool {
        self.definition.workspace
    }
    pub(crate) fn workspace_identity(
        &self,
        resources: Option<&Resources>,
    ) -> io::Result<Option<crate::workspace::Identity>> {
        if self.definition.protocol != ToolProtocol::LanguageServer {
            return Err(denied());
        }
        if !self.definition.workspace {
            return Ok(None);
        }
        let resources = resources.ok_or_else(denied)?;
        let root = resources.root(resources.scope())?;
        root.require(crate::workspace::Right::Read)?;
        root.require(crate::workspace::Right::Discover)?;
        root.current()?;
        Ok(Some(root.identity()))
    }
    pub(crate) fn prepare_server(
        &self,
        request: &crate::lsp::Request<'_>,
        budget: &mut Budget<'_>,
    ) -> io::Result<(super::Run, String)> {
        if self.definition.protocol != ToolProtocol::LanguageServer {
            return Err(denied());
        }
        let arguments = self
            .definition
            .argv(&Default::default())
            .map_err(|_| denied())?;
        budget.check()?;
        let mut projection = if self.definition.workspace {
            Projection::workspace(request.resources.ok_or_else(denied)?, budget)?
        } else {
            Projection::empty()
        };
        let relative = projection.document(
            request.snapshot,
            if self.definition.workspace {
                request.relative
            } else {
                None
            },
            if self.definition.workspace {
                request.resources
            } else {
                None
            },
        )?;
        let image = self
            .images
            .resolve(self.image, &self.binding, request.cancel.flag())?;
        budget.check()?;
        let run = self
            .runs
            .prepare(image, projection, arguments, request.cancel.flag())?;
        budget.check()?;
        Ok((run, relative))
    }
    pub(crate) fn resolve(
        image: Handle,
        binding: Binding,
        definition: ToolDefinition,
        images: Arc<Store>,
        runs: Arc<Runs>,
    ) -> io::Result<Self> {
        let bytes = images.info(image, &binding)?.bytes;
        Ok(Self {
            image,
            binding,
            definition,
            images,
            runs,
            bytes,
        })
    }
    pub(crate) fn matches(
        &self,
        command: &str,
        token: Handle,
        profile: &str,
        definition: &ToolDefinition,
    ) -> bool {
        Binding::new(command, token, profile, definition)
            .is_ok_and(|binding| binding == self.binding)
    }
    pub(crate) fn image_work(&self) -> u64 {
        self.bytes.saturating_mul(3)
    }
    pub(crate) fn filter(
        &self,
        resources: Option<&Resources>,
        arguments: &hedit_plugin::sdk::Storage,
        input: &str,
        cancel: &AtomicBool,
        budget: &mut Budget<'_>,
        deadline: Instant,
    ) -> io::Result<String> {
        if self.definition.protocol != ToolProtocol::Filter {
            return Err(denied());
        }
        let arguments = self.definition.argv(arguments).map_err(|_| denied())?;
        budget.check()?;
        let projection = if self.definition.workspace {
            Projection::workspace(resources.ok_or_else(denied)?, budget)?
        } else {
            Projection::empty()
        };
        let image = self.images.resolve(self.image, &self.binding, cancel)?;
        budget.check()?;
        let run = self.runs.prepare(image, projection, arguments, cancel)?;
        budget.check()?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|time| !time.is_zero())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Tool preparation exhausted the callback deadline.",
                )
            })?;
        let output = run.filter(input.as_bytes(), cancel, remaining)?;
        String::from_utf8(output.stdout).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Native filter output is not UTF-8.",
            )
        })
    }
}
