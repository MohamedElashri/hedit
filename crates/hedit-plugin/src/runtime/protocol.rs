//! Private, bounded wire format shared by all native worker adapters.
use super::{Input, MAX_SELECTIONS, MAX_SELECTION_BYTES, MAX_SOURCE_BYTES};
use crate::{options as plugin_options, Error};
use serde::de::{self, DeserializeSeed, SeqAccess, Visitor};
use std::{cell::Cell, fmt, io::Read};

const MAX_LANGUAGE_BYTES: usize = 256;
pub(super) const MAX_REPLACEMENT_BYTES: usize = 256 * 1024;
const MAX_BATCH_REQUEST_BYTES: usize = 1024 * 1024;
pub(super) const MAX_BATCH_REPLY_BYTES: usize = 2 * 1024 * 1024;
const REQUEST_MAGIC: u32 = u32::MAX - 1;
const SDK_MAGIC: u32 = u32::MAX - 2;
const SERVICE_MAGIC: u32 = u32::MAX - 3;
const BROKER_MAGIC: u32 = u32::MAX - 4;

pub(super) enum Invocation {
    Legacy(Input),
    Sdk(crate::sdk::Input),
    Service(crate::service::Input),
    Broker(crate::broker::Input),
}

impl Input {
    fn validate(&self) -> Result<(), Error> {
        if self.source.len() > MAX_SOURCE_BYTES
            || self.language.len() > MAX_LANGUAGE_BYTES
            || !(1..=MAX_SELECTIONS).contains(&self.selections.len())
            || self.selections.iter().map(String::len).sum::<usize>() > MAX_SELECTION_BYTES
            || self.settings.len() > plugin_options::MAX_SETTINGS
            || self
                .settings
                .iter()
                .any(|(key, value)| !plugin_options::valid_id(key) || !value.valid())
        {
            return Err(Error::Request);
        }
        Ok(())
    }
}

pub(super) fn encode_request(input: &Input) -> Result<Vec<u8>, Error> {
    input.validate()?;
    let payload = serde_json::to_vec(input).map_err(|_| Error::Request)?;
    if payload.len() > MAX_BATCH_REQUEST_BYTES {
        return Err(Error::Request);
    }
    let mut bytes = Vec::with_capacity(payload.len() + 8);
    bytes.extend(REQUEST_MAGIC.to_le_bytes());
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(payload);
    Ok(bytes)
}

pub(super) fn encode_sdk(input: &crate::sdk::Input) -> Result<Vec<u8>, Error> {
    input.validate()?;
    let payload = serde_json::to_vec(input).map_err(|_| Error::Request)?;
    if payload.len() > MAX_BATCH_REQUEST_BYTES {
        return Err(Error::Request);
    }
    let mut bytes = SDK_MAGIC.to_le_bytes().to_vec();
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(payload);
    Ok(bytes)
}

pub(super) fn encode_service(input: &crate::service::Input) -> Result<Vec<u8>, Error> {
    input.validate()?;
    let payload = serde_json::to_vec(input).map_err(|_| Error::Request)?;
    if payload.len() > MAX_BATCH_REQUEST_BYTES {
        return Err(Error::Request);
    }
    let mut bytes = SERVICE_MAGIC.to_le_bytes().to_vec();
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(payload);
    Ok(bytes)
}

pub(super) fn encode_broker(input: &crate::broker::Input) -> Result<Vec<u8>, Error> {
    input.validate()?;
    let payload = serde_json::to_vec(input).map_err(|_| Error::Request)?;
    if payload.len() > crate::broker::MAX_MESSAGE_BYTES {
        return Err(Error::Request);
    }
    let mut bytes = BROKER_MAGIC.to_le_bytes().to_vec();
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(payload);
    Ok(bytes)
}

#[cfg(test)]
fn decode_request(input: impl Read) -> Result<Input, Error> {
    match decode_invocation(input)? {
        Invocation::Legacy(input) => Ok(input),
        Invocation::Sdk(_) | Invocation::Service(_) | Invocation::Broker(_) => Err(Error::Request),
    }
}

pub(super) fn decode_invocation(mut input: impl Read) -> Result<Invocation, Error> {
    let mut header = [0; 8];
    input.read_exact(&mut header).map_err(|_| Error::Request)?;
    let magic = u32::from_le_bytes(header[..4].try_into().unwrap());
    if magic != REQUEST_MAGIC
        && magic != SDK_MAGIC
        && magic != SERVICE_MAGIC
        && magic != BROKER_MAGIC
    {
        return Err(Error::Request);
    }
    let size = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
    if size
        > if magic == BROKER_MAGIC {
            crate::broker::MAX_MESSAGE_BYTES
        } else {
            MAX_BATCH_REQUEST_BYTES
        }
    {
        return Err(Error::Request);
    }
    let mut payload = vec![0; size];
    input.read_exact(&mut payload).map_err(|_| Error::Request)?;
    if input.read(&mut [0; 1]).map_err(|_| Error::Request)? != 0 {
        return Err(Error::Request);
    }
    if magic == BROKER_MAGIC {
        let request: crate::broker::Input =
            serde_json::from_slice(&payload).map_err(|_| Error::Request)?;
        request.validate()?;
        Ok(Invocation::Broker(request))
    } else if magic == SERVICE_MAGIC {
        let request: crate::service::Input =
            serde_json::from_slice(&payload).map_err(|_| Error::Request)?;
        request.validate()?;
        Ok(Invocation::Service(request))
    } else if magic == SDK_MAGIC {
        let request: crate::sdk::Input =
            serde_json::from_slice(&payload).map_err(|_| Error::Request)?;
        request.validate()?;
        Ok(Invocation::Sdk(request))
    } else {
        let request: Input = serde_json::from_slice(&payload).map_err(|_| Error::Request)?;
        request.validate()?;
        Ok(Invocation::Legacy(request))
    }
}

#[cfg(any(unix, windows))]
pub(super) fn worker_reply(mut reply: Vec<u8>, delivered: bool) -> Result<Vec<u8>, Error> {
    if reply.len() > MAX_BATCH_REPLY_BYTES + 1 {
        return Err(Error::Worker);
    }
    let Some((&status, body)) = reply.split_first() else {
        return Err(Error::Worker);
    };
    if status != 0 {
        return Err(if body.is_empty() {
            Error::from_worker_code(status)
        } else {
            Error::Worker
        });
    }
    if !delivered {
        return Err(Error::Worker);
    }
    Ok(reply.split_off(1))
}

pub(super) fn validate_edits(edits: &[String]) -> Result<(), Error> {
    if edits.iter().map(String::len).sum::<usize>() > MAX_REPLACEMENT_BYTES {
        return Err(Error::ReturnSize);
    }
    if edits.iter().any(|edit| edit.contains('\0')) {
        return Err(Error::ReturnEncoding);
    }
    Ok(())
}

pub(super) fn decode_edits(payload: &[u8], count: usize) -> Result<Vec<String>, Error> {
    if payload.len() > MAX_BATCH_REPLY_BYTES || !(1..=MAX_SELECTIONS).contains(&count) {
        return Err(Error::Worker);
    }
    let failure = Cell::new(Error::Worker);
    let mut decoder = serde_json::Deserializer::from_slice(payload);
    let edits = Edits {
        count,
        failure: &failure,
    }
    .deserialize(&mut decoder)
    .map_err(|_| failure.get())?;
    decoder.end().map_err(|_| Error::Worker)?;
    Ok(edits)
}

struct Edits<'a> {
    count: usize,
    failure: &'a Cell<Error>,
}

impl<'de> DeserializeSeed<'de> for Edits<'_> {
    type Value = Vec<String>;

    fn deserialize<D: de::Deserializer<'de>>(self, decoder: D) -> Result<Self::Value, D::Error> {
        decoder.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for Edits<'_> {
    type Value = Vec<String>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("one replacement string per selected range")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        // Never reserve from an untrusted size hint or collect an unbounded array.
        let mut edits = Vec::with_capacity(self.count);
        let mut bytes = 0;
        for _ in 0..self.count {
            let edit: String = sequence
                .next_element()?
                .ok_or_else(|| de::Error::custom("missing replacement"))?;
            bytes += edit.len();
            let failure = if bytes > MAX_REPLACEMENT_BYTES {
                Some(Error::ReturnSize)
            } else if edit.contains('\0') {
                Some(Error::ReturnEncoding)
            } else {
                None
            };
            if let Some(failure) = failure {
                self.failure.set(failure);
                return Err(de::Error::custom("invalid replacement"));
            }
            edits.push(edit);
        }
        // Refuse an extra element before deserializing its type or contents.
        sequence.next_element_seed(NoExtraEdit)?;
        Ok(edits)
    }
}

struct NoExtraEdit;

impl<'de> DeserializeSeed<'de> for NoExtraEdit {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, _: D) -> Result<(), D::Error> {
        Err(de::Error::custom("unexpected replacement"))
    }
}

#[cfg(test)]
mod tests;
