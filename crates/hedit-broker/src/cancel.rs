//! Wakeable cancellation without a polling thread or lost notifications.
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Default)]
pub struct Cancel {
    flag: AtomicBool,
    notify: tokio::sync::Notify,
}
impl Cancel {
    pub(crate) fn flag(&self) -> &AtomicBool {
        &self.flag
    }
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }
    pub async fn cancelled(&self) {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}
