#[cfg(target_arch = "x86_64")]
mod x86_64;
#[cfg(target_arch = "x86_64")]
pub use x86_64::*;

use core::num::NonZeroU16;

pub trait Controller {
    unsafe fn reset(&mut self);
    unsafe fn enable(&mut self);
    unsafe fn disable(&mut self);

    unsafe fn enable_timer(&mut self);
    unsafe fn disable_timer(&mut self);
    unsafe fn set_timer_frequency(&mut self, desired_frequency: NonZeroU16);
    unsafe fn set_timer_wait(&mut self, wait_factor: NonZeroU16);

    fn end_interrupt(&mut self);
}
