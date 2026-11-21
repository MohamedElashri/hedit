use super::*;
use crate::workspace::{Policy, Protected, Right, Root};
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
fn fixture_config() -> (tempfile::TempDir, Config) {
    let outer = tempfile::tempdir().unwrap();
    std::fs::write(outer.path().join("text.txt"), "old café / 東京\r\n").unwrap();
    let ambient =
        cap_std::fs::Dir::open_ambient_dir(outer.path(), cap_std::ambient_authority()).unwrap();
    let private = crate::journal::native::private_directory(&ambient, ".test-config").unwrap();
    let protected = Protected::persistent(&private, []).unwrap();
    let store_dir = crate::journal::native::private_directory(&private, "recovery").unwrap();
    let recovery =
        Arc::new(crate::recovery_store::RecoveryStore::open(store_dir, &protected).unwrap());
    let root = Root::open_selected(
        outer.path(),
        Policy::new([Right::Read, Right::Discover, Right::Edit]).unwrap(),
        protected,
    )
    .unwrap();
    let package=serde_json::from_value(serde_json::json!({"sdk_version":1,"source":"return function(ctx) return {intent={release={resource='ffffffffffffffffffffffffffffffff'}}} end","snapshot":{"document":"0123456789abcdef0123456789abcdef","revision":"0","language":"plain_text","text":"","selections":[{"anchor":0,"head":0}],"crlf":false},"settings":{},"arguments":{}})).unwrap();
    let config = Config {
        source_token: None,
        tools: Default::default(),
        owner: None,
        package,
        provider: None,
        cache: Default::default(),
        definition: Arc::new(
            serde_json::from_str(
                r#"{"capabilities":["workspace.discover","workspace.read","workspace.edit"]}"#,
            )
            .unwrap(),
        ),
        resources: Some(Resources::new(root).unwrap()),
        endpoints: Default::default(),
        network: Arc::new(Network::default()),
        recovery,
        cancel: Arc::new(Cancel::default()),
        wake: Arc::new(|| {}),
    };
    (outer, config)
}
fn fixture() -> (tempfile::TempDir, Session) {
    let (outer, config) = fixture_config();
    (outer, Session::new(config).unwrap())
}
fn provider_config() -> (tempfile::TempDir, Config) {
    let (directory, mut config) = fixture_config();
    config.definition = Arc::new(serde_json::from_value(serde_json::json!({"capabilities":["workspace.discover","workspace.read","workspace.edit","events.observe","ui.present"]})).unwrap());
    config.provider = Some(service::Request {
        id: Handle::fresh().unwrap().to_string(),
        kind: service::Kind::Presentation,
        trigger: service::Trigger::Manual,
        position: 0,
        action: None,
        choice: None,
    });
    (directory, config)
}
#[tokio::test]
async fn active_background_work_leaves_a_foreground_slot_and_queued_work_cancels_without_ui_frames()
{
    let _suite = SERIAL.lock().await;
    let mut providers = Vec::new();
    for _ in 0..3 {
        let (directory, config) = provider_config();
        let mut session = Session::new(config).unwrap();
        session.begin_callback().unwrap();
        providers.push((directory, session));
    }
    let (_directory, config) = provider_config();
    let mut waiting = Session::new(config).unwrap();
    assert_eq!(
        waiting.begin_callback().err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    let (_directory, mut foreground) = fixture();
    foreground.begin_callback().unwrap();
    assert_eq!(ACTIVE.available_permits(), 0);
    let cancel = waiting.cancel.clone();
    let (result, ()) = tokio::join!(waiting.begin_callback_when_ready(), async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        cancel.cancel();
    });
    assert_eq!(result.err().unwrap().kind(), io::ErrorKind::Interrupted);
    assert!(waiting.active.is_none());
    drop(foreground);
    drop(providers);
    assert_eq!(ACTIVE.available_permits(), 4);
    assert_eq!(BACKGROUND_WORK.available_permits(), 3);
}
#[tokio::test]
async fn provider_contexts_leave_foreground_capacity_and_finish_releases_background_admission() {
    let _suite = SERIAL.lock().await;
    let mut directories = Vec::new();
    let mut providers = Vec::new();
    for _ in 0..crate::MAX_PROVIDER_CONTEXTS {
        let (directory, config) = provider_config();
        directories.push(directory);
        providers.push(Session::new(config).unwrap());
    }
    let (_directory, config) = provider_config();
    assert_eq!(
        Session::new(config).err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    let mut foreground = Vec::new();
    for _ in 0..4 {
        foreground.push(fixture());
    }
    let (_directory, config) = fixture_config();
    assert_eq!(
        Session::new(config).err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    providers[0].finish(false);
    assert!(providers[0].background.is_none());
    let (_directory, config) = provider_config();
    let resumed = Session::new(config).unwrap();
    drop(resumed);
    drop(providers);
    drop(foreground);
    assert_eq!(ADMISSION.available_permits(), 16);
    assert_eq!(PROVIDERS.available_permits(), crate::MAX_PROVIDER_CONTEXTS);
}
async fn send(session: &mut Session, intent: Intent) -> io::Result<Effect> {
    let input = session.begin_callback()?;
    session
        .accept(
            broker::Output {
                request: input.request,
                step: input.step,
                intent,
                cache: Default::default(),
            },
            || Ok(()),
        )
        .await
}
#[tokio::test]
async fn foreign_handles_and_replies_fail_before_authorization_or_disclosure() {
    let _suite = SERIAL.lock().await;
    let (_outer, mut session) = fixture();
    let input = session.begin_callback().unwrap();
    let output = broker::Output {
        request: Handle::fresh().unwrap(),
        step: input.step,
        intent: Intent::Discover {
            scope: input.scope.unwrap(),
            query: String::new(),
        },
        cache: Default::default(),
    };
    let mut checks = 0;
    assert!(session
        .accept(output, || {
            checks += 1;
            Ok(())
        })
        .await
        .is_err());
    assert_eq!(checks, 0);
    assert!(session.cancel.is_cancelled());
    assert!(session.resources.is_none());
    assert!(session.permit.is_none());
    assert!(session.begin_callback().is_err());
    let (_outer, mut session) = fixture();
    assert!(send(
        &mut session,
        Intent::Read {
            resource: Handle::fresh().unwrap(),
            version: Handle::fresh().unwrap(),
            offset: 0,
            maximum: 64
        }
    )
    .await
    .is_err());
}
#[tokio::test]
async fn workspace_disclosure_is_chunked_bound_to_versions_and_independent_of_document_edit() {
    let _suite = SERIAL.lock().await;
    let (_outer, mut config) = fixture_config();
    config.package.snapshot.crlf = true;
    assert!(Session::new(config).is_err());
    let (_outer, mut session) = fixture();
    let scope = session.input.scope.unwrap();
    assert!(!session.input.may_edit);
    assert!(session.input.workspace_edit);
    assert!(matches!(
        send(
            &mut session,
            Intent::Discover {
                scope,
                query: String::new()
            }
        )
        .await
        .unwrap(),
        Effect::Continue
    ));
    assert!(
        matches!(&session.input.response,Some(Response::Discovered{entries,..}) if entries.len()==1)
    );
    send(
        &mut session,
        Intent::Open {
            scope,
            relative: "text.txt".into(),
        },
    )
    .await
    .unwrap();
    let Some(Response::Opened(version)) = session.input.response.clone() else {
        panic!()
    };
    send(
        &mut session,
        Intent::Read {
            resource: version.resource,
            version: version.version,
            offset: 0,
            maximum: 64,
        },
    )
    .await
    .unwrap();
    assert!(
        matches!(&session.input.response,Some(Response::Read{text,next:None}) if text=="old café / 東京\r\n")
    );
    send(
        &mut session,
        Intent::StageEdits {
            scope,
            change: broker::Change {
                resource: version.resource,
                version: version.version,
                edits: vec![sdk::Edit {
                    start: 0,
                    finish: 3,
                    text: "new".into(),
                }],
            },
        },
    )
    .await
    .unwrap();
    let Effect::Review(plan) = send(
        &mut session,
        Intent::ReviewEdits {
            scope,
            changes: None,
        },
    )
    .await
    .unwrap() else {
        panic!()
    };
    assert_eq!(plan.files()[0].after(), "new café / 東京\r\n");
    assert!(session.begin_callback().is_err());
    session.discard_review(&plan).unwrap();
    assert!(session.begin_callback().is_ok());
}
#[tokio::test]
async fn revocation_exhausted_chains_and_unapproved_endpoints_never_run_io() {
    let _suite = SERIAL.lock().await;
    let (outer, mut session) = fixture();
    let scope = session.input.scope.unwrap();
    let input = session.begin_callback().unwrap();
    let output = broker::Output {
        request: input.request,
        step: input.step,
        intent: Intent::Open {
            scope,
            relative: "text.txt".into(),
        },
        cache: Default::default(),
    };
    session.cancel();
    let mut checked = false;
    assert!(session
        .accept(output, || {
            checked = true;
            Ok(())
        })
        .await
        .is_err());
    assert!(!checked);
    assert_eq!(
        std::fs::read_to_string(outer.path().join("text.txt")).unwrap(),
        "old café / 東京\r\n"
    );
    drop(session);
    let (_outer, mut session) = fixture();
    assert!(send(
        &mut session,
        Intent::Https {
            endpoint: "undeclared".into(),
            body: "fixture".into()
        }
    )
    .await
    .is_err());
    drop(session);
    let (_outer, mut session) = fixture();
    session.spent = WORK;
    assert!(session.begin_callback().is_err());
    session.spent = Duration::ZERO;
    session.input.step = 32;
    assert!(session.begin_callback().is_err());
}
#[tokio::test]
async fn batch_read_and_review_use_bounded_callbacks_for_multiple_files() {
    let _suite = SERIAL.lock().await;
    let (outer, mut session) = fixture();
    std::fs::write(outer.path().join("second.txt"), "old second\n").unwrap();
    let scope = session.input.scope.unwrap();
    send(
        &mut session,
        Intent::OpenMany {
            scope,
            paths: vec!["text.txt".into(), "second.txt".into()],
        },
    )
    .await
    .unwrap();
    let Some(Response::OpenedMany { files }) = session.input.response.clone() else {
        panic!()
    };
    assert_eq!(files.len(), 2);
    let changes = files
        .iter()
        .map(|file| broker::Change {
            resource: file.resource.resource,
            version: file.resource.version,
            edits: vec![sdk::Edit {
                start: 0,
                finish: 3,
                text: "new".into(),
            }],
        })
        .collect();
    let Effect::Review(plan) = send(
        &mut session,
        Intent::ReviewEdits {
            scope,
            changes: Some(changes),
        },
    )
    .await
    .unwrap() else {
        panic!()
    };
    assert_eq!(plan.files().len(), 2);
    assert_eq!(session.input.step, 1);
    assert_eq!(
        std::fs::read_to_string(outer.path().join("text.txt")).unwrap(),
        "old café / 東京\r\n"
    );
}

#[tokio::test]
async fn parked_previews_do_not_exhaust_active_callback_capacity() {
    let _suite = SERIAL.lock().await;
    let mut parked = Vec::new();
    for _ in 0..4 {
        let (outer, mut session) = fixture();
        let scope = session.input.scope.unwrap();
        send(
            &mut session,
            Intent::Open {
                scope,
                relative: "text.txt".into(),
            },
        )
        .await
        .unwrap();
        let Some(Response::Opened(resource)) = session.input.response.clone() else {
            panic!()
        };
        let effect = send(
            &mut session,
            Intent::ReviewEdits {
                scope,
                changes: Some(vec![broker::Change {
                    resource: resource.resource,
                    version: resource.version,
                    edits: vec![sdk::Edit {
                        start: 0,
                        finish: 3,
                        text: "new".into(),
                    }],
                }]),
            },
        )
        .await
        .unwrap();
        assert!(matches!(effect, Effect::Review(_)));
        assert!(session.active.is_none());
        parked.push((outer, session));
    }
    let mut running = Vec::new();
    for _ in 0..4 {
        let (outer, mut session) = fixture();
        session.begin_callback().unwrap();
        running.push((outer, session));
    }
    let (_outer, mut next) = fixture();
    assert_eq!(
        next.begin_callback().err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    running.pop();
    next.begin_callback().unwrap();
}

#[tokio::test]
async fn last_callback_rejects_effects_before_authorization_and_returns_final_cache() {
    let _suite = SERIAL.lock().await;
    let (_outer, mut session) = fixture();
    session.input.step = (broker::MAX_CALLBACKS - 1) as u8;
    let input = session.begin_callback().unwrap();
    let mut checked = false;
    assert!(session
        .accept(
            broker::Output {
                request: input.request,
                step: input.step,
                intent: Intent::Open {
                    scope: input.scope.unwrap(),
                    relative: "text.txt".into()
                },
                cache: Default::default(),
            },
            || {
                checked = true;
                Ok(())
            }
        )
        .await
        .is_err());
    assert!(!checked);
    let (_outer, mut session) = fixture();
    session.input.step = (broker::MAX_CALLBACKS - 1) as u8;
    let input = session.begin_callback().unwrap();
    let cache: sdk::Storage = serde_json::from_str(r#"{"counter":7}"#).unwrap();
    let Effect::Finished { cache: result, .. } = session
        .accept(
            broker::Output {
                request: input.request,
                step: input.step,
                intent: Intent::Finish {
                    transaction: None,
                    result: None,
                },
                cache,
            },
            || Ok(()),
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        result.0["counter"],
        hedit_plugin::options::Value::Integer(7)
    );
    assert!(session.input.cache.0.is_empty());
    assert!(session.active.is_none());
}

#[tokio::test]
async fn workspace_only_commands_cannot_return_document_navigation_or_annotations() {
    let _suite = SERIAL.lock().await;
    for result in [
        service::ResultSet::Diagnostics(vec![]),
        service::ResultSet::Presentation(service::Presentation {
            annotations: vec![service::Annotation {
                range: service::Range {
                    start: 0,
                    finish: 0,
                },
                text: "annotation".into(),
            }],
            ..Default::default()
        }),
        service::ResultSet::Presentation(service::Presentation {
            quick_list: Some(service::QuickList {
                title: "Navigation".into(),
                items: vec![service::ActionItem {
                    title: "Select".into(),
                    action: service::Action::Select(sdk::Selection { anchor: 0, head: 0 }),
                }],
            }),
            ..Default::default()
        }),
    ] {
        let (_outer, mut config) = fixture_config();
        config.definition = Arc::new(serde_json::from_str(r#"{"capabilities":["workspace.discover","workspace.read","workspace.edit","ui.present"]}"#).unwrap());
        let mut session = Session::new(config).unwrap();
        assert!(send(
            &mut session,
            Intent::Finish {
                transaction: None,
                result: Some(Box::new(result))
            }
        )
        .await
        .is_err());
    }
}

#[tokio::test]
async fn close_callbacks_remove_edit_authority_and_finish_releases_all_admission() {
    let _suite = SERIAL.lock().await;
    let (_outer, mut config) = fixture_config();
    config.resources = None;
    config.definition = Arc::new(
        serde_json::from_str(
            r#"{"capabilities":["document.read","document.edit","events.observe","ui.present"]}"#,
        )
        .unwrap(),
    );
    config.provider = Some(service::Request {
        id: Handle::fresh().unwrap().to_string(),
        kind: service::Kind::Presentation,
        trigger: service::Trigger::Close,
        position: 0,
        action: None,
        choice: None,
    });
    let mut session = Session::new(config).unwrap();
    assert!(!session.input.may_edit);
    assert!(matches!(
        send(
            &mut session,
            Intent::Finish {
                transaction: None,
                result: Some(Box::new(service::ResultSet::Closed))
            }
        )
        .await
        .unwrap(),
        Effect::Finished { .. }
    ));
    assert!(session.resources.is_none());
    assert!(session.permit.is_none());
    assert!(session.begin_callback().is_err());
}

#[tokio::test]
async fn watch_continuations_deliver_scoped_changes_without_holding_authority_while_idle() {
    let _suite = SERIAL.lock().await;
    let (outer, mut config) = fixture_config();
    let protected = config
        .resources
        .as_ref()
        .unwrap()
        .root(config.resources.as_ref().unwrap().scope())
        .unwrap()
        .protected()
        .clone();
    config.resources = Some(
        Resources::new(
            Root::open_selected(
                outer.path(),
                Policy::new([Right::Watch]).unwrap(),
                protected,
            )
            .unwrap(),
        )
        .unwrap(),
    );
    config.definition = Arc::new(
        serde_json::from_str(
            r#"{"capabilities":["document.read","events.observe","ui.present","workspace.watch"]}"#,
        )
        .unwrap(),
    );
    config.provider = Some(service::Request {
        id: Handle::fresh().unwrap().to_string(),
        kind: service::Kind::Presentation,
        trigger: service::Trigger::Manual,
        position: 0,
        action: None,
        choice: None,
    });
    let mut session = Session::new(config).unwrap();
    let scope = session.input.scope.unwrap();
    send(&mut session, Intent::Watch { scope }).await.unwrap();
    let Some(Response::Watching { watch, .. }) = session.input.response else {
        panic!()
    };
    std::fs::write(outer.path().join("text.txt"), "changed").unwrap();
    // Deterministically signal the coalesced notification as well, so semantic
    // coverage does not depend on the OS delivering its native event in time.
    session.watches.get_mut(&watch).unwrap().signal_for_test();
    let input = session.begin_callback().unwrap();
    let mut authorized = 0;
    tokio::time::timeout(
        Duration::from_secs(3),
        session.accept(
            broker::Output {
                request: input.request,
                step: input.step,
                intent: Intent::WaitChanges { watch },
                cache: Default::default(),
            },
            || {
                authorized += 1;
                Ok(())
            },
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(authorized, 1);
    assert!(
        matches!(&session.input.response,Some(Response::Changed { paths, .. }) if paths == &["text.txt"])
    );
    let input = session.begin_callback().unwrap();
    // Clear any delayed native event. No policy guard should be held if a
    // subscription is parked, and cancellation must wake it immediately.
    session.watches.get_mut(&watch).unwrap().clear_for_test();
    let cancel = session.cancel.clone();
    let cancel_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel.cancel();
    });
    let result = session
        .accept(
            broker::Output {
                request: input.request,
                step: input.step,
                intent: Intent::WaitChanges { watch },
                cache: Default::default(),
            },
            || {
                authorized += 1;
                Ok(())
            },
        )
        .await;
    cancel_task.await.unwrap();
    assert_eq!(result.err().unwrap().kind(), io::ErrorKind::Interrupted);
    assert_eq!(authorized, 1);
    assert!(session.watches.is_empty());
}
