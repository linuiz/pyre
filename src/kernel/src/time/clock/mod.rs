pub mod arch;

use crate::mem::alloc::eternal::EternalAllocator;
use alloc::boxed::Box;
use core::time::Duration;

static INITIAL_TIMESTAMP: spin::Once<u64> = spin::Once::new();

pub fn set_initial_timestamp() {
    assert!(INITIAL_TIMESTAMP.get().is_none(), "initial timestamp already set");

    INITIAL_TIMESTAMP.call_once(|| SYSTEM.get_timestamp());
}

#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(u64);

impl Instant {
    pub fn now() -> Self {
        Self(SYSTEM.get_timestamp() - INITIAL_TIMESTAMP.get().unwrap())
    }

    pub fn duration_since(self, other: Self) -> Duration {
        let elapsed_ticks = other.0.checked_sub(self.0).unwrap();
        let elapsed_secs = elapsed_ticks / SYSTEM.frequency();
        Duration::from_secs(elapsed_secs)
    }

    #[inline]
    pub fn as_secs(self) -> u64 {
        self.0 / SYSTEM.frequency()
    }

    #[inline]
    pub fn as_millis(self) -> u64 {
        self.0 / (SYSTEM.frequency() / 1000)
    }
}

pub static SYSTEM: spin::Lazy<Box<dyn Clock, EternalAllocator>> = spin::Lazy::new(|| {
    crate::interrupts::without(|| {
        let acpi_clock = arch::x86_64::Acpi::load().unwrap();
        Box::new_in(acpi_clock, EternalAllocator)
    })
});

pub trait Clock: Send + Sync {
    fn frequency(&self) -> u64;
    fn get_timestamp(&self) -> u64;
    fn spin_wait(&self, duration: Duration);
}
