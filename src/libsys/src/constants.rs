pub use x86_64::*;
mod x86_64 {
    use crate::Pow2Usize;
    use core::num::NonZeroU32;

    pub const fn page_shift() -> NonZeroU32 {
        NonZeroU32::new(12).unwrap()
    }

    pub const fn page_size() -> Pow2Usize {
        Pow2Usize::new(1 << page_shift().get()).unwrap()
    }

    pub const fn page_mask() -> usize {
        page_size().get().checked_sub(1).unwrap()
    }

    pub const fn table_index_shift() -> NonZeroU32 {
        NonZeroU32::new(9).unwrap()
    }

    pub const fn table_index_size() -> Pow2Usize {
        Pow2Usize::new(1 << table_index_shift().get()).unwrap()
    }

    pub const fn table_index_mask() -> usize {
        table_index_size().get().checked_sub(1).unwrap()
    }

    pub const fn phys_canonical_mask() -> usize {
        0x000F_FFFF_FFFF_FFFF
    }

    pub const fn checked_phys_canonical(address: usize) -> bool {
        (address & !phys_canonical_mask()) == 0
    }

    #[inline]
    pub fn virt_canonical_shift() -> NonZeroU32 {
        const CR4_LA57_BIT: usize = 1 << 12;

        // Safety: `asm!` is used safely, and `NonZeroU32` is guaranteed >0.
        unsafe {
            let cr4: usize;
            core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, pure));

            let paging_depth = if (cr4 & CR4_LA57_BIT) > 0 { 3 } else { 4 };
            NonZeroU32::new_unchecked((table_index_shift().get() * paging_depth) + page_shift().get())
        }
    }

    #[inline]
    pub fn virt_noncanonical_mask() -> usize {
        let shift = virt_canonical_shift().get();
        usize::MAX >> shift << shift
    }

    pub fn checked_virt_canonical(address: usize) -> bool {
        let canonical_extension_bits = virt_noncanonical_mask();
        let extension_bits = address >> virt_canonical_shift().get();

        extension_bits == 0 || extension_bits == canonical_extension_bits
    }
}
