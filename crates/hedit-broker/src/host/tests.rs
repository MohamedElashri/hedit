use super::*;
use std::sync::mpsc;
static SERIAL: Mutex<()> = Mutex::new(());
#[test]
fn blocked_provider_setup_preserves_four_foreground_threads_and_releases_reservations() {
    let _suite = SERIAL.lock().unwrap();
    let mut hosts = Vec::new();
    let mut release = Vec::new();
    for provider in std::iter::repeat_n(true, crate::MAX_PROVIDER_CONTEXTS).chain([false; 4]) {
        let (sender, receiver) = mpsc::sync_channel(1);
        let setup = move |_| {
            receiver.recv().unwrap();
            Err(cancelled())
        };
        let authority: Arc<Authority> = Arc::new(|_| unreachable!());
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let host = if provider {
            Host::start_provider(setup, authority, Arc::new(|_, _| unreachable!()), wake)
        } else {
            Host::start(setup, authority, wake)
        }
        .unwrap();
        hosts.push(host);
        release.push(sender);
        if hosts.len() == crate::MAX_PROVIDER_CONTEXTS {
            let denied = Host::start_provider(
                |_| unreachable!(),
                Arc::new(|_| unreachable!()),
                Arc::new(|_, _| unreachable!()),
                Arc::new(|| {}),
            );
            assert_eq!(denied.err().unwrap().kind(), io::ErrorKind::WouldBlock);
        }
    }
    assert_eq!(
        Host::start(
            |_| unreachable!(),
            Arc::new(|_| unreachable!()),
            Arc::new(|| {})
        )
        .err()
        .unwrap()
        .kind(),
        io::ErrorKind::WouldBlock
    );
    for sender in release {
        sender.send(()).unwrap();
    }
    for mut host in hosts {
        host.thread.take().unwrap().join().unwrap();
    }
    assert_eq!(ADMISSION.available_permits(), 16);
    assert_eq!(PROVIDERS.available_permits(), crate::MAX_PROVIDER_CONTEXTS);
}
#[test]
fn setup_errors_and_panics_publish_fixed_failures_from_a_background_thread() {
    let _suite = SERIAL.lock().unwrap();
    for panic in [false, true] {
        let caller = std::thread::current().id();
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut host = Host::start(
            move |_| {
                assert_ne!(caller, std::thread::current().id());
                assert!(!panic, "synthetic setup failure");
                Err(io::Error::other(
                    "sensitive host error that must not reach Lua",
                ))
            },
            Arc::new(|_| panic!("authorization must not run")),
            Arc::new(move || {
                let _ = sender.try_send(());
            }),
        )
        .unwrap();
        receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(matches!(
            host.try_event(),
            Some(Event::Failed(FailureCode::Failed))
        ));
        assert!(host.try_event().is_none());
        let _ = host.thread.take().unwrap().join();
    }
}
#[test]
fn thread_admission_is_reserved_before_setup_and_slow_consumers_have_one_mailbox() {
    let _suite = SERIAL.lock().unwrap();
    let mut hosts = Vec::new();
    let mut release = Vec::new();
    let mut completed = Vec::new();
    for _ in 0..16 {
        let (sender, receiver) = mpsc::sync_channel(1);
        let (finish, finished) = mpsc::sync_channel(1);
        hosts.push(
            Host::start(
                move |_| {
                    receiver.recv().unwrap();
                    Err(cancelled())
                },
                Arc::new(|_| unreachable!()),
                Arc::new(move || {
                    let _ = finish.try_send(());
                }),
            )
            .unwrap(),
        );
        release.push(sender);
        completed.push(finished);
    }
    let fifth = Host::start(
        |_| unreachable!(),
        Arc::new(|_| unreachable!()),
        Arc::new(|| {}),
    );
    assert_eq!(fifth.err().unwrap().kind(), io::ErrorKind::WouldBlock);
    for sender in release {
        sender.send(()).unwrap();
    }
    for receiver in completed {
        receiver.recv_timeout(Duration::from_secs(3)).unwrap();
    }
    for mut host in hosts {
        assert!(matches!(
            host.try_event(),
            Some(Event::Failed(FailureCode::Cancelled))
        ));
        host.thread.take().unwrap().join().unwrap();
    }
    let mailbox = Mailbox {
        value: Mutex::new(None),
        wake: Arc::new(|| {}),
    };
    mailbox.publish(Event::Failed(FailureCode::Denied));
    mailbox.publish(Event::Failed(FailureCode::Changed));
    assert!(matches!(
        mailbox.value.lock().unwrap().take(),
        Some(Event::Failed(FailureCode::Changed))
    ));
}
