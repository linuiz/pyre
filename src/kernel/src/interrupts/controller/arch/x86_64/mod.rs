mod apic;

use crate::interrupts::controller::{ArchController, ArchTimer};
use apic::*;

impl<'a> ArchTimer for LocalVector<'a, Timer> {
    unsafe fn enable(&mut self) {
        self.set_masked(true);
    }

    unsafe fn disable(&mut self) {
        self.set_masked(false);
    }

    unsafe fn set_frequency(&mut self, frequency: core::num::NonZeroUsize) {
        todo!()
    }
}

impl<'a> ArchController for Apic<'a> {
    type Timer = LocalVector<'a, Timer>;
    type Error = Error;

    fn new_init() -> core::result::Result<Self, Self::Error>
    where
        Self: Sized,
    {
        Apic::new_init()
    }

    unsafe fn enable(&mut self) {
        self.sw_enable();
    }

    unsafe fn disable(&mut self) {
        self.sw_disable();
    }

    fn timer(&mut self) -> &mut Self::Timer {
        self.timer()
    }

    fn end_interrupt(&mut self) {
        self.end_of_interrupt();
    }
}
