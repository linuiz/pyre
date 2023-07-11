pub mod exceptions;
pub mod traps;

mod controller;
pub use controller::*;

mod instructions;
pub use instructions::*;

use num_enum::TryFromPrimitive;

/// Delivery mode for IPIs.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[allow(clippy::upper_case_acronyms)]
pub enum InterruptDeliveryMode {
    Fixed = 0b000,
    LowPriority = 0b001,
    SMI = 0b010,
    NMI = 0b100,
    INIT = 0b101,
    StartUp = 0b110,
    ExtINT = 0b111,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[allow(clippy::upper_case_acronyms)]
pub enum DeliveryMode {
    Fixed = 0b000,
    LowPriority = 0b001,
    SMI = 0b010,
    NMI = 0b100,
    INIT = 0b101,
    StartUp = 0b110,
    ExtINT = 0b111,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationMode {
    Physical = 0,
    Logical = 1,
}

/// Provides access to the contained instance of `T`, ensuring interrupts are disabled before it is borrowed.
pub struct InterruptCell<T>(T);

impl<T> InterruptCell<T> {
    #[inline]
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    #[inline]
    pub fn with<U>(&self, func: impl FnOnce(&T) -> U) -> U {
        without(|| func(&self.0))
    }

    #[inline]
    pub fn with_mut<U>(&mut self, func: impl FnOnce(&mut T) -> U) -> U {
        without(|| func(&mut self.0))
    }
}
