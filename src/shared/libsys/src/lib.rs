#![no_std]
#![feature(
    error_in_core,              // #103765 <https://github.com/rust-lang/rust/issues/103765>
    strict_provenance,          // #95228 <https://github.com/rust-lang/rust/issues/95228>
    try_trait_v2,               // #84277 <https://github.com/rust-lang/rust/issues/84277>
    exclusive_range_pattern,    // #37854 <https://github.com/rust-lang/rust/issues/37854>
    const_option,
)]

mod macros;

mod address;
pub use address::*;

mod constants;
pub use constants::*;

mod little_endian;
pub use little_endian::*;

// pub mod sync;
pub mod io;
pub mod syscall;

extern crate alloc;
#[macro_use]
extern crate static_assertions;
#[macro_use]
extern crate error;

use core::num::NonZeroU32;

pub const KIBIBYTE: u64 = 0x400; // 1024
pub const MIBIBYTE: u64 = KIBIBYTE * KIBIBYTE;
pub const GIBIBYTE: u64 = MIBIBYTE * MIBIBYTE;

#[inline]
pub const fn to_kibibytes(value: u64) -> u64 {
    value / KIBIBYTE
}

#[inline]
pub const fn to_mibibytes(value: u64) -> u64 {
    value / MIBIBYTE
}

#[inline]
pub const fn align_up(value: usize, alignment_bits: NonZeroU32) -> usize {
    (value.wrapping_neg() & (1usize << alignment_bits.get()).wrapping_neg()).wrapping_neg()
}

#[inline]
pub const fn align_up_div(value: usize, alignment_bits: NonZeroU32) -> usize {
    align_up(value, alignment_bits) / (1usize << alignment_bits.get())
}

#[inline]
pub const fn align_down(value: usize, alignment_bits: NonZeroU32) -> usize {
    value & !((1usize << alignment_bits.get()) - 1)
}

#[inline]
pub const fn align_down_div(value: usize, alignment_bits: NonZeroU32) -> usize {
    align_down(value, alignment_bits) / (1usize << alignment_bits.get())
}

pub use cpu_types::*;
mod cpu_types {
    #![allow(non_camel_case_types)]

    #[cfg(target_pointer_width = "128")]
    pub type uptr = u128;
    #[cfg(target_pointer_width = "64")]
    pub type uptr = u64;
    #[cfg(target_pointer_width = "32")]
    pub type uptr = u32;

    #[cfg(any(target_arch = "x86_64", target_arch = "riscv64", target_arch = "aarch64"))]
    pub type ureg = u64;
    #[cfg(any(target_arch = "x86", target_arch = "riscv32"))]
    pub type ureg = u32;
}

pub trait Truncate {
    type Into;

    fn truncate_into(self) -> Self::Into;
}

impl Truncate for ureg {
    type Into = usize;

    fn truncate_into(self) -> Self::Into {
        self as usize
    }
}

impl Truncate for usize {
    type Into = ureg;

    fn truncate_into(self) -> Self::Into {
        self as ureg
    }
}
