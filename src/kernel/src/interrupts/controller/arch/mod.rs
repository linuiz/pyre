#[cfg(target_arch = "x86_64")]
mod x86_64;
#[cfg(target_arch = "x86_64")]
pub use x86_64::*;

pub trait ArchTimer {
    unsafe fn enable(&mut self);
    unsafe fn disable(&mut self);
    unsafe fn set_frequency(&mut self, frequency: core::num::NonZeroUsize);
}

pub trait ArchController {
    type Timer: ArchTimer;
    type Error;

    fn new_init() -> core::result::Result<Self, Self::Error>
    where
        Self: Sized;

    unsafe fn enable(&mut self);
    unsafe fn disable(&mut self);
    fn timer(&mut self) -> &mut Self::Timer;

    fn end_interrupt(&mut self);
}
