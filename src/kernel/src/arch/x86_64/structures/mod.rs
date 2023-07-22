pub mod gdt;
pub mod idt;
pub mod ioapic;

// pub fn load_static_tables() {
//     use crate::arch::x86_64::structures::idt::InterruptDescriptorTable;

//     // Always initialize GDT prior to configuring IDT.
//     crate::arch::x86_64::structures::gdt::load();

//     // Due to the fashion in which the `x86_64` crate initializes the IDT entries,
//     // it must be ensured that the handlers are set only *after* the GDT has been
//     // properly initialized and loaded—otherwise, the `CS` value for the IDT entries
//     // is incorrect, and this causes very confusing GPFs.
//     crate::interrupts::without(|| {
//         static STATIC_IDT: spin::Lazy<InterruptDescriptorTable> = spin::Lazy::new(|| {
//             let mut idt = InterruptDescriptorTable::new();
//             crate::arch::x86_64::structures::idt::set_exception_handlers(&mut idt);
//             crate::arch::x86_64::structures::idt::set_stub_handlers(&mut idt);
//             idt
//         });

//         STATIC_IDT.load();
//     });
// }
