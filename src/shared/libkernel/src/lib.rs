#![no_std]
#![feature(
    extern_types,                   // #43467 <https://github.com/rust-lang/rust/issues/43467>
    exclusive_range_pattern,        // #37854 <https://github.com/rust-lang/rust/issues/37854>
)]

pub mod mem;

pub struct ReadOnly;
pub struct WriteOnly;
pub struct ReadWrite;

extern "C" {
    pub type LinkerSymbol;
}

impl LinkerSymbol {
    #[inline]
    pub fn as_ptr<T>(&'static self) -> *const T {
        self as *const _ as *const T
    }

    pub fn as_usize(&'static self) -> usize {
        self as *const Self as usize
    }
}

pub struct IndexRing {
    current: usize,
    max: usize,
}

impl IndexRing {
    pub fn new(max: usize) -> Self {
        Self { current: 0, max }
    }

    pub fn index(&self) -> usize {
        self.current
    }

    pub fn increment(&mut self) {
        self.current = self.next_index();
    }

    pub fn next_index(&self) -> usize {
        (self.current + 1) % self.max
    }
}

impl core::fmt::Debug for IndexRing {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_tuple("Index Ring").field(&format_args!("{}/{}", self.current, self.max - 1)).finish()
    }
}

#[macro_export]
macro_rules! asm_marker {
    ($marker:literal) => {
        core::arch::asm!("push r8", concat!("mov r8, ", $marker), "pop r8", options(nostack, nomem));
    };
}
