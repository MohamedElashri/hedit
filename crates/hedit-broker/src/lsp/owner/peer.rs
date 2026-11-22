use super::super::{Connection, Document, Method, Notification, Transport};
use hedit_core::language_server::text::Encoding;
use hedit_plugin::{sdk, service::ResultSet};
use serde_json::{json, Value};
use std::{io, sync::atomic::AtomicBool, time::Instant};

pub(super) struct Peer<T: Transport> {
    pub connection: Connection<T>,
    document: Document,
    encoding: Encoding,
    capabilities: Value,
    incremental: bool,
    diagnostics: Option<ResultSet>,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid scoped language-server document state.",
    )
}
fn unavailable() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "The scoped language server does not support this method.",
    )
}
fn supports(value: &Value) -> bool {
    value == true || value.is_object()
}
impl<T: Transport> Peer<T> {
    pub(super) fn start(
        mut connection: Connection<T>,
        snapshot: sdk::Snapshot,
        uri: String,
        root: Option<String>,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> io::Result<Self> {
        connection.diagnostic_scope(&uri, 1);
        let result = connection.initialize(json!({
            "processId":null,"clientInfo":{"name":"hedit scoped plugin host"},
            "rootUri":root,"workspaceFolders":root.as_ref().map(|uri| json!([{"uri":uri,"name":"Approved snapshot"}])),
            "capabilities":{
                "general":{"positionEncodings":["utf-8","utf-16","utf-32"]},
                "workspace":{"applyEdit":false,"configuration":false,"workspaceFolders":false},
                "textDocument":{
                    "synchronization":{"dynamicRegistration":false,"didSave":false,"willSave":false},
                    "diagnostic":{"dynamicRegistration":false,"relatedDocumentSupport":false},
                    "completion":{"dynamicRegistration":false,"completionItem":{"snippetSupport":false,"commitCharactersSupport":false}},
                    "hover":{"dynamicRegistration":false,"contentFormat":["plaintext"]},
                    "codeAction":{"dynamicRegistration":false,"isPreferredSupport":false,"disabledSupport":false,"dataSupport":false},
                    "documentSymbol":{"dynamicRegistration":false,"hierarchicalDocumentSymbolSupport":true}
                }
            }
        }), cancel, deadline)?;
        let capabilities = result["capabilities"].clone();
        let encoding = Encoding::parse(
            capabilities
                .get("positionEncoding")
                .map(|value| value.as_str().ok_or_else(invalid))
                .transpose()?,
        )
        .map_err(|_| invalid())?;
        let sync = &capabilities["textDocumentSync"];
        let kind = if sync.is_object() {
            if sync.get("openClose").is_some_and(|value| value != true) {
                return Err(unavailable());
            }
            sync["change"].as_u64()
        } else {
            sync.as_u64()
        };
        let incremental = match kind {
            Some(1) => false,
            Some(2) => true,
            _ => return Err(unavailable()),
        };
        let document = Document::new(snapshot, uri, 1, encoding)?;
        let mut peer = Self {
            connection,
            document,
            encoding,
            capabilities,
            incremental,
            diagnostics: None,
        };
        peer.connection.notify(Notification::Open, json!({"textDocument":{"uri":peer.document.uri(),"version":1,"languageId":peer.document.snapshot().language,"text":peer.document.snapshot().text}}), cancel, deadline)?;
        peer.capture()?;
        Ok(peer)
    }
    fn capture(&mut self) -> io::Result<()> {
        for notification in self.connection.take_diagnostics() {
            self.diagnostics = Some(
                self.document
                    .published_diagnostics(&notification["params"])?,
            );
        }
        Ok(())
    }
    pub(super) fn idle(&mut self, cancel: &AtomicBool, deadline: Instant) -> io::Result<()> {
        self.connection.idle(cancel, deadline)?;
        self.capture()
    }
    fn update(
        &mut self,
        snapshot: sdk::Snapshot,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> io::Result<()> {
        let old = self.document.snapshot();
        if old.document != snapshot.document || old.language != snapshot.language {
            return Err(invalid());
        }
        let before = old.revision.parse::<u64>().map_err(|_| invalid())?;
        let after = snapshot.revision.parse::<u64>().map_err(|_| invalid())?;
        if after < before || (after == before && snapshot.text != old.text) {
            return Err(invalid());
        }
        if after == before {
            return Ok(());
        }
        let end = self.document.position(old.text.len())?;
        let version = self.document.version().checked_add(1).ok_or_else(invalid)?;
        let document = Document::new(snapshot, self.document.uri().into(), version, self.encoding)?;
        let change = if self.incremental {
            json!({"range":{"start":{"line":0,"character":0},"end":end},"text":document.snapshot().text})
        } else {
            json!({"text":document.snapshot().text})
        };
        self.connection.diagnostic_scope(document.uri(), version);
        self.connection.notify(Notification::Change, json!({"textDocument":{"uri":document.uri(),"version":version},"contentChanges":[change]}), cancel, deadline)?;
        self.document = document;
        self.diagnostics = None;
        self.capture()
    }
    pub(super) fn request(
        &mut self,
        snapshot: sdk::Snapshot,
        method: Method,
        position: usize,
        may_edit: bool,
        signals: (&AtomicBool, &AtomicBool),
        deadline: Instant,
    ) -> io::Result<ResultSet> {
        let (cancel, superseded) = signals;
        if superseded.load(std::sync::atomic::Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Scoped language-server callback superseded.",
            ));
        }
        self.update(snapshot, cancel, deadline)?;
        if superseded.load(std::sync::atomic::Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Scoped language-server callback superseded.",
            ));
        }
        let position_value = self.document.position(position)?;
        if !may_edit
            && matches!(
                method,
                Method::Completions | Method::Formatting | Method::CodeActions
            )
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Document edit approval is required.",
            ));
        }
        let capability = match method {
            Method::Diagnostics => "diagnosticProvider",
            Method::Completions => "completionProvider",
            Method::Hover => "hoverProvider",
            Method::Symbols => "documentSymbolProvider",
            Method::Formatting => "documentFormattingProvider",
            Method::CodeActions => "codeActionProvider",
        };
        if matches!(method, Method::Diagnostics) && !supports(&self.capabilities[capability]) {
            for _ in 0..1024 {
                if superseded.load(std::sync::atomic::Ordering::Acquire) {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "Scoped language-server callback superseded.",
                    ));
                }
                if let Some(result) = &self.diagnostics {
                    return Ok(result.clone());
                }
                self.idle(cancel, deadline)?;
                self.connection.transport_mut()?.wait(false)?;
            }
            return Err(invalid());
        }
        if !supports(&self.capabilities[capability]) {
            return Err(unavailable());
        }
        let document = json!({"uri":self.document.uri()});
        let params = match method {
            Method::Diagnostics | Method::Symbols => json!({"textDocument":document}),
            Method::Hover | Method::Completions => {
                json!({"textDocument":document,"position":position_value})
            }
            Method::Formatting => {
                json!({"textDocument":document,"options":{"tabSize":4,"insertSpaces":true}})
            }
            Method::CodeActions => {
                json!({"textDocument":document,"range":{"start":position_value,"end":position_value},"context":{"diagnostics":[]}})
            }
        };
        let value = self
            .connection
            .request_cancellable(method, params, cancel, superseded, deadline)?;
        self.capture()?;
        self.document.translate(method, &value, position, may_edit)
    }
}
