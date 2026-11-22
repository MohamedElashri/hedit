use super::*;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
#[derive(Default)]
struct Stats {
    received: Vec<Value>,
    drops: AtomicUsize,
}
struct Wire {
    decoder: Decoder,
    output: VecDeque<u8>,
    stats: Arc<Mutex<Stats>>,
    chunk: usize,
    inject: Vec<Value>,
    duplicate: bool,
    wrong: bool,
    truncate: bool,
    supersede: Option<Arc<AtomicBool>>,
    supersede_partial: Option<Arc<AtomicBool>>,
    held: VecDeque<Vec<u8>>,
}
impl Wire {
    fn new(chunk: usize) -> Self {
        Self {
            decoder: Decoder::new(Limits::SCOPED),
            output: VecDeque::new(),
            stats: Arc::new(Mutex::new(Stats::default())),
            chunk,
            inject: vec![],
            duplicate: false,
            wrong: false,
            truncate: false,
            supersede: None,
            supersede_partial: None,
            held: VecDeque::new(),
        }
    }
}
impl Drop for Wire {
    fn drop(&mut self) {
        self.stats
            .lock()
            .unwrap()
            .drops
            .fetch_add(1, Ordering::Relaxed);
    }
}
impl Transport for Wire {
    fn poll(
        &mut self,
        input: &[u8],
        output: &mut [u8; READ_BYTES],
        _: &AtomicBool,
        _: Instant,
    ) -> io::Result<Progress> {
        let written = input.len().min(self.chunk);
        self.decoder.push(&input[..written])?;
        if written > 0 {
            if let Some(flag) = self.supersede_partial.take() {
                flag.store(true, Ordering::Release);
            }
        }
        while let Some(request) = self.decoder.next_message()? {
            self.stats.lock().unwrap().received.push(request.clone());
            if let Some(id) = request
                .get("id")
                .filter(|_| request.get("method").is_some())
            {
                let result = if request["method"] == "initialize" {
                    json!({"capabilities":{}})
                } else {
                    json!({"items":[]})
                };
                for notification in self.inject.drain(..) {
                    self.output
                        .extend(wire::encode(&notification, Limits::SCOPED)?);
                }
                let value = json!({"jsonrpc":"2.0", "id":if self.wrong { json!(999) } else { id.clone() }, "result":result});
                let bytes = wire::encode(&value, Limits::SCOPED)?;
                if let Some(flag) = self
                    .supersede
                    .as_ref()
                    .filter(|_| request["method"] != "initialize")
                {
                    flag.store(true, Ordering::Release);
                    self.held.push_back(bytes);
                    continue;
                }
                self.output.extend(&bytes);
                if self.duplicate {
                    self.output.extend(&bytes);
                }
                if self.truncate {
                    self.output.pop_back();
                }
            }
        }
        let count = output.len().min(self.chunk).min(self.output.len());
        for byte in &mut output[..count] {
            *byte = self.output.pop_front().unwrap();
        }
        if count == 0 && self.truncate {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "fixture EOF"));
        }
        Ok(Progress {
            written,
            read: count,
        })
    }
    fn wait(&self, _: bool) -> io::Result<()> {
        Ok(())
    }
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
fn ready(wire: Wire) -> Connection<Wire> {
    let mut connection = Connection::new(wire);
    connection
        .initialize(json!({}), &AtomicBool::new(false), deadline())
        .unwrap();
    connection
}

#[test]
fn fragmented_requests_correlate_without_reusing_ids() {
    for chunk in [1, 3, 7, READ_BYTES] {
        let wire = Wire::new(chunk);
        let stats = wire.stats.clone();
        let mut connection = ready(wire);
        for id in 2..20 {
            assert_eq!(
                connection
                    .request(
                        Method::Hover,
                        json!({"text":"café 東京 😀"}),
                        &AtomicBool::new(false),
                        deadline()
                    )
                    .unwrap(),
                json!({"items":[]})
            );
            assert_eq!(stats.lock().unwrap().received.last().unwrap()["id"], id);
        }
        drop(connection);
        assert_eq!(stats.lock().unwrap().drops.load(Ordering::Relaxed), 1);
    }
}
#[test]
fn server_requests_are_denied_even_when_interleaved_with_results() {
    let mut connection = ready(Wire::new(1));
    let stats = connection.transport.as_ref().unwrap().stats.clone();
    connection.transport.as_mut().unwrap().inject = vec![
        json!({"jsonrpc":"2.0","id":"server","method":"workspace/applyEdit","params":{"edit":{"changes":{"file:///secret":[]}}}}),
        json!({"jsonrpc":"2.0","id":32,"method":"workspace/executeCommand","params":{"command":"shell"}}),
        json!({"jsonrpc":"2.0","id":33,"method":"client/registerCapability","params":{}}),
    ];
    connection
        .request(
            Method::Symbols,
            json!({}),
            &AtomicBool::new(false),
            deadline(),
        )
        .unwrap();
    let stats = stats.lock().unwrap();
    let denied: Vec<_> = stats
        .received
        .iter()
        .filter(|value| value.get("error").is_some())
        .collect();
    assert_eq!(denied.len(), 3);
    for value in denied {
        assert_eq!(value["error"]["code"], -32601);
    }
}
#[test]
fn duplicate_foreign_and_truncated_responses_destroy_owner() {
    for mode in 0..3 {
        let mut connection = ready(Wire::new(READ_BYTES));
        let transport = connection.transport.as_mut().unwrap();
        let stats = transport.stats.clone();
        match mode {
            0 => transport.duplicate = true,
            1 => transport.wrong = true,
            _ => transport.truncate = true,
        }
        assert!(connection
            .request(
                Method::Hover,
                json!({}),
                &AtomicBool::new(false),
                deadline()
            )
            .is_err());
        assert_eq!(stats.lock().unwrap().drops.load(Ordering::Relaxed), 1);
        assert!(connection
            .request(
                Method::Hover,
                json!({}),
                &AtomicBool::new(false),
                deadline()
            )
            .is_err());
    }
}
#[test]
fn cancellation_and_deadlines_never_leave_a_reusable_connection() {
    for cancelled in [false, true] {
        let mut connection = ready(Wire::new(1));
        let stats = connection.transport.as_ref().unwrap().stats.clone();
        let before = stats.lock().unwrap().received.len();
        assert!(connection
            .request(
                Method::Hover,
                json!({}),
                &AtomicBool::new(cancelled),
                if cancelled {
                    deadline()
                } else {
                    Instant::now()
                }
            )
            .is_err());
        assert_eq!(stats.lock().unwrap().received.len(), before);
        assert_eq!(stats.lock().unwrap().drops.load(Ordering::Relaxed), 1);
    }
}
#[test]
fn floods_retention_and_lifetime_request_limits_fail_closed() {
    for diagnostics in [true, false] {
        let mut connection = ready(Wire::new(READ_BYTES));
        connection.transport.as_mut().unwrap().inject = (0..65).map(|_| json!({"jsonrpc":"2.0","method":if diagnostics {"textDocument/publishDiagnostics"} else {"window/logMessage"},"params":{}})).collect();
        assert!(connection
            .request(
                Method::Diagnostics,
                json!({}),
                &AtomicBool::new(false),
                deadline()
            )
            .is_err());
        assert!(connection.take_diagnostics().is_empty());
    }
    let mut connection = ready(Wire::new(READ_BYTES));
    for _ in 1..REQUESTS {
        connection
            .request(
                Method::Hover,
                json!({}),
                &AtomicBool::new(false),
                deadline(),
            )
            .unwrap();
    }
    assert!(connection
        .request(
            Method::Hover,
            json!({}),
            &AtomicBool::new(false),
            deadline()
        )
        .is_err());
}
#[test]
fn unsolicited_responses_and_role_confusion_fail_before_features() {
    let mut connection = ready(Wire::new(READ_BYTES));
    let bytes = wire::encode(
        &json!({"jsonrpc":"2.0","id":2,"result":null}),
        Limits::SCOPED,
    )
    .unwrap();
    connection.transport.as_mut().unwrap().output.extend(bytes);
    assert!(connection
        .idle(&AtomicBool::new(false), deadline())
        .is_err());
    let mut connection = Connection::new(Wire::new(READ_BYTES));
    assert!(connection
        .request(
            Method::Hover,
            json!({}),
            &AtomicBool::new(false),
            deadline()
        )
        .is_err());
}

#[test]
fn idle_flood_limits_do_not_reset_on_each_poll() {
    let mut connection = ready(Wire::new(READ_BYTES));
    let message = wire::encode(
        &json!({"jsonrpc":"2.0","method":"window/logMessage","params":{}}),
        Limits::SCOPED,
    )
    .unwrap();
    for _ in 0..2 {
        for _ in 0..32 {
            connection
                .transport
                .as_mut()
                .unwrap()
                .output
                .extend(&message);
        }
        connection
            .idle(&AtomicBool::new(false), deadline())
            .unwrap();
    }
    connection
        .transport
        .as_mut()
        .unwrap()
        .output
        .extend(&message);
    assert!(connection
        .idle(&AtomicBool::new(false), deadline())
        .is_err());
    assert!(connection.transport.is_none());
}

#[test]
fn superseded_replies_are_fenced_and_cancellation_does_not_reuse_ids() {
    let mut connection = ready(Wire::new(READ_BYTES));
    let superseded = Arc::new(AtomicBool::new(false));
    connection.transport.as_mut().unwrap().supersede = Some(superseded.clone());
    assert_eq!(
        connection
            .request_cancellable(
                Method::Hover,
                json!({}),
                &AtomicBool::new(false),
                &superseded,
                deadline()
            )
            .unwrap_err()
            .kind(),
        io::ErrorKind::Interrupted
    );
    assert!(connection.open());
    assert_eq!(connection.retired.iter().copied().collect::<Vec<_>>(), [2]);
    let wire = connection.transport.as_mut().unwrap();
    wire.supersede = None;
    let old = wire.held.pop_front().unwrap();
    wire.output.extend(old);
    let stats = wire.stats.clone();
    assert_eq!(
        connection
            .request(
                Method::Hover,
                json!({}),
                &AtomicBool::new(false),
                deadline()
            )
            .unwrap(),
        json!({"items":[]})
    );
    assert!(connection.retired.is_empty());
    let received = &stats.lock().unwrap().received;
    assert!(received
        .iter()
        .any(|value| value["method"] == "$/cancelRequest" && value["params"]["id"] == 2));
    assert_eq!(received.last().unwrap()["id"], 3);
}

#[test]
fn unsent_and_partially_sent_superseded_requests_keep_framing_consistent() {
    for partial in [false, true] {
        let mut connection = ready(Wire::new(if partial { 1 } else { READ_BYTES }));
        let superseded = Arc::new(AtomicBool::new(!partial));
        if partial {
            connection.transport.as_mut().unwrap().supersede_partial = Some(superseded.clone());
        }
        assert_eq!(
            connection
                .request_cancellable(
                    Method::Hover,
                    json!({"text":"café / 東京"}),
                    &AtomicBool::new(false),
                    &superseded,
                    deadline()
                )
                .unwrap_err()
                .kind(),
            io::ErrorKind::Interrupted
        );
        assert!(connection.open());
        assert_eq!(
            connection
                .request(
                    Method::Hover,
                    json!({}),
                    &AtomicBool::new(false),
                    deadline()
                )
                .unwrap(),
            json!({"items":[]})
        );
    }
}

#[test]
fn abandoned_id_budget_and_hard_cancellation_still_destroy_the_owner() {
    let mut connection = ready(Wire::new(READ_BYTES));
    let superseded = Arc::new(AtomicBool::new(false));
    for _ in 0..16 {
        superseded.store(false, Ordering::Release);
        connection.transport.as_mut().unwrap().supersede = Some(superseded.clone());
        assert_eq!(
            connection
                .request_cancellable(
                    Method::Hover,
                    json!({}),
                    &AtomicBool::new(false),
                    &superseded,
                    deadline()
                )
                .unwrap_err()
                .kind(),
            io::ErrorKind::Interrupted
        );
        assert!(connection.open());
    }
    superseded.store(false, Ordering::Release);
    assert!(connection
        .request_cancellable(
            Method::Hover,
            json!({}),
            &AtomicBool::new(false),
            &superseded,
            deadline()
        )
        .is_err());
    assert!(!connection.open());
    let mut connection = ready(Wire::new(READ_BYTES));
    assert!(connection
        .request_cancellable(
            Method::Hover,
            json!({}),
            &AtomicBool::new(true),
            &AtomicBool::new(true),
            deadline()
        )
        .is_err());
    assert!(!connection.open());
}

#[test]
fn retired_error_responses_are_discarded_once_and_duplicates_fail_closed() {
    let mut connection = ready(Wire::new(READ_BYTES));
    let superseded = Arc::new(AtomicBool::new(false));
    connection.transport.as_mut().unwrap().supersede = Some(superseded.clone());
    assert!(connection
        .request_cancellable(
            Method::Hover,
            json!({}),
            &AtomicBool::new(false),
            &superseded,
            deadline()
        )
        .is_err());
    let error = wire::encode(
        &json!({"jsonrpc":"2.0","id":2,"error":{"code":-32800,"message":"Cancelled"}}),
        Limits::SCOPED,
    )
    .unwrap();
    let wire = connection.transport.as_mut().unwrap();
    wire.supersede = None;
    wire.held.clear();
    wire.output.extend(&error);
    connection
        .idle(&AtomicBool::new(false), deadline())
        .unwrap();
    assert!(connection.retired.is_empty() && connection.open());
    connection.transport.as_mut().unwrap().output.extend(error);
    assert!(connection
        .idle(&AtomicBool::new(false), deadline())
        .is_err());
    assert!(!connection.open());
}
