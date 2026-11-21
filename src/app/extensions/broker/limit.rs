use std::{
    io,
    sync::atomic::{AtomicUsize, Ordering},
};
pub(super) struct Reservation {
    counter: &'static AtomicUsize,
    amount: usize,
}
impl Reservation {
    pub(super) fn reserve(
        counter: &'static AtomicUsize,
        amount: usize,
        maximum: usize,
    ) -> io::Result<Self> {
        let mut used = counter.load(Ordering::Acquire);
        loop {
            let next = used
                .checked_add(amount)
                .filter(|next| *next <= maximum)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::OutOfMemory, "Extension host limit reached.")
                })?;
            match counter.compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Ok(Self { counter, amount }),
                Err(current) => used = current,
            }
        }
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.amount, Ordering::AcqRel);
    }
}
