//! Revocable host lifetime without callbacks, I/O or waiting in Drop.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
pub struct Lifetime(Arc<AtomicBool>);
#[derive(Clone)]
pub struct Lease(Arc<AtomicBool>);
impl Default for Lifetime {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
}
impl Lifetime {
    pub fn lease(&self) -> Lease {
        Lease(self.0.clone())
    }
    pub fn close(&self) {
        self.0.store(false, Ordering::Release);
    }
}
impl Drop for Lifetime {
    fn drop(&mut self) {
        self.close();
    }
}
impl Lease {
    pub fn current(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn close_and_reopen_never_revive_old_leases() {
        let owner = Lifetime::default();
        let old = owner.lease();
        let copy = old.clone();
        assert!(old.same(&copy) && copy.current());
        owner.close();
        assert!(!old.current() && !copy.current());
        let new = Lifetime::default();
        assert!(new.lease().current() && !new.lease().same(&old));
        let lease = new.lease();
        drop(new);
        assert!(!lease.current());
    }
}
