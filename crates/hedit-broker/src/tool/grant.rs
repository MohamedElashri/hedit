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
