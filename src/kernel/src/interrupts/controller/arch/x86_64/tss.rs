#![allow(clippy::module_name_repetitions)]

use crate::arch::x86_64::structures::gdt;
use ia32utils::{instructions::tables::load_tss, structures::tss::TaskStateSegment};

pub fn ptr_as_descriptor(tss_ptr: core::ptr::NonNull<TaskStateSegment>) -> gdt::Descriptor {
    use bit_field::BitField;

    let tss_ptr_u64 = tss_ptr.addr().get() as u64;

    let mut low = gdt::DescriptorFlags::PRESENT.bits();
    // base
    low.set_bits(16..40, tss_ptr_u64.get_bits(0..24));
    low.set_bits(56..64, tss_ptr_u64.get_bits(24..32));
    // limit (the `-1` is needed since the bound is inclusive, not exclusive)
    low.set_bits(0..16, (core::mem::size_of::<TaskStateSegment>() - 1) as u64);
    // type (0b1001 = available 64-bit tss)
    low.set_bits(40..44, 0b1001);

    // high 32 bits of base
    let mut high = 0;
    high.set_bits(0..32, tss_ptr_u64.get_bits(32..64));

    gdt::Descriptor::SystemSegment(low, high)
}

/// Safety
///
/// * Descriptor must be valid as the core's task state segment.
/// * Caller must ensure loading a new TSS will not result in undefined behaviour.
pub unsafe fn load_local(descriptor: gdt::Descriptor) {
    crate::interrupts::without(|| {
        // Store current GDT pointer to restore later.
        let cur_gdt = gdt::sgdt();
        // Create temporary kernel GDT to avoid a GPF on switching to it.
        let mut temp_gdt = gdt::GlobalDescriptorTable::new();
        temp_gdt.add_entry(gdt::Descriptor::kernel_code_segment());
        temp_gdt.add_entry(gdt::Descriptor::kernel_data_segment());
        let tss_selector = temp_gdt.add_entry(descriptor);

        // Load temp GDT ...
        temp_gdt.load_unsafe();
        // ... load TSS from temporary GDT ...
        load_tss(tss_selector);
        // ... and restore cached GDT.
        gdt::lgdt(&cur_gdt);
    });
}
