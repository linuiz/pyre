#![no_std]
#![no_main]
#![feature(
    asm,
    abi_efiapi,
    abi_x86_interrupt,
    once_cell,
    const_mut_refs,
    raw_ref_op
)]

#[macro_use]
extern crate log;
extern crate alloc;
extern crate libstd;

mod block_malloc;
mod drivers;
mod logging;
mod timer;

use libstd::{
    acpi::SystemConfigTableEntry,
    cell::SyncOnceCell,
    memory::{falloc, malloc::MemoryAllocator, UEFIMemoryDescriptor},
    BootInfo, LinkerSymbol,
};

extern "C" {
    static __ap_trampoline_start: LinkerSymbol;
    static __ap_trampoline_end: LinkerSymbol;
    static __kernel_pml4: LinkerSymbol;

    static __kernel_stack_bottom: LinkerSymbol;
    static __kernel_stack_top: LinkerSymbol;

    static __ap_stack_bottom: LinkerSymbol;
    static __ap_stack_top: LinkerSymbol;

    static __text_start: LinkerSymbol;
    static __text_end: LinkerSymbol;

    static __rodata_start: LinkerSymbol;
    static __rodata_end: LinkerSymbol;

    static __data_start: LinkerSymbol;
    static __data_end: LinkerSymbol;

    static __bss_start: LinkerSymbol;
    static __bss_end: LinkerSymbol;
}

#[export_name = "__ap_stack_pointers"]
static mut AP_STACK_POINTERS: [*const core::ffi::c_void; 256] = [core::ptr::null(); 256];

#[cfg(debug_assertions)]
fn get_log_level() -> log::LevelFilter {
    log::LevelFilter::Trace
}

#[cfg(not(debug_assertions))]
fn get_log_level() -> log::LevelFilter {
    log::LevelFilter::Debug
}

static mut CON_OUT: drivers::io::Serial = drivers::io::Serial::new(drivers::io::COM1);
static TRACE_ENABLED_PATHS: [&str; 1] = ["libstd::structures::apic::icr"];
static BOOT_INFO: SyncOnceCell<BootInfo<UEFIMemoryDescriptor, SystemConfigTableEntry>> =
    SyncOnceCell::new();

macro_rules! print {
    () => {};
}

/// Clears the kernel stack by resetting `RSP`.
///
/// Safety: This method does *extreme* damage to the stack. It should only ever be used when
///         ABSOLUTELY NO dangling references to the old stack will exist (i.e. calling a
///         no-argument function directly after).
#[inline(always)]
unsafe fn clear_stack() {
    unsafe { libstd::registers::stack::RSP::write(__kernel_stack_top.as_page().base_addr()) };
}

#[no_mangle]
#[export_name = "_entry"]
unsafe extern "efiapi" fn _kernel_pre_init(
    boot_info: BootInfo<UEFIMemoryDescriptor, SystemConfigTableEntry>,
) -> ! {
    BOOT_INFO.set(boot_info);

    unsafe {
        clear_stack();
        kernel_init()
    }
}

unsafe fn kernel_init() -> ! {
    CON_OUT.init(drivers::io::SerialSpeed::S115200);

    match drivers::io::set_stdout(&mut CON_OUT, get_log_level(), &TRACE_ENABLED_PATHS) {
        Ok(()) => {
            info!("Successfully loaded into kernel, with logging enabled.");
        }
        Err(_) => libstd::instructions::interrupts::breakpoint(),
    }

    let boot_info = BOOT_INFO
        .get()
        .expect("Boot info hasn't been initialized in kernel memory");

    info!("Validating BootInfo struct.");
    boot_info.validate_magic();

    debug!(
        "Detected CPU features: {:?}",
        libstd::instructions::cpu_features()
    );

    debug!("Initializing kernel frame allocator.");
    falloc::load_new(boot_info.memory_map());
    reserve_system_frames(boot_info.memory_map());
    init_system_config_table(boot_info.config_table());

    clear_stack();
    kernel_mem_init()
}

#[inline(never)]
unsafe fn kernel_mem_init() -> ! {
    info!("Initializing kernel default allocator.");

    let memory_map = BOOT_INFO
        .get()
        .expect("Boot info struct has not been passed into kernel executable memory")
        .memory_map();

    let malloc = block_malloc::BlockAllocator::new(memory_map);

    debug!("Flagging `text` and `rodata` kernel sections as read-only.");
    for page in (__text_start.as_page()..__text_end.as_page())
        .chain(__rodata_start.as_page()..__rodata_end.as_page())
    {
        malloc.set_page_attribs(
            &page,
            libstd::memory::paging::PageAttributes::WRITABLE,
            libstd::memory::paging::AttributeModify::Remove,
        );
    }

    libstd::memory::malloc::set(alloc::boxed::Box::new(malloc));
    // Move the current (new) PML4 into the global processor reference.
    // TODO somehow ensure the PML4 frame is within the first 32KiB for the AP trampoline
    __kernel_pml4
        .as_mut_ptr::<u32>()
        .write(libstd::registers::CR3::read().0.as_usize() as u32);

    clear_stack();
    _startup()
}

#[no_mangle]
extern "C" fn _startup() -> ! {
    libstd::structures::gdt::init();
    libstd::structures::idt::load();
    libstd::lpu::init();
    init_apic();

    // If this is the BSP, wake other cores.
    if libstd::lpu::is_bsp() {
        use libstd::acpi::rdsp::xsdt::{
            madt::{InterruptDevice, MADT},
            XSDT,
        };

        // Initialize other CPUs
        info!("Beginning wake-up sequence for each enabled processor core.");
        let apic = libstd::lpu::get().apic();
        let icr = apic.interrupt_command_register();
        let ap_trampoline_page_index = unsafe { __ap_trampoline_start.as_page().index() } as u8;

        if let Ok(madt) = XSDT.find_sub_table::<MADT>() {
            for interrupt_device in madt.iter() {
                if let InterruptDevice::LocalAPIC(apic_other) = interrupt_device {
                    use libstd::acpi::rdsp::xsdt::madt::LocalAPICFlags;

                    // Ensure the CPU core can actually be enabled.
                    if apic_other.flags().intersects(
                        LocalAPICFlags::PROCESSOR_ENABLED | LocalAPICFlags::ONLINE_CAPABLE,
                    ) && apic.id() != apic_other.id()
                    {
                        const STACK_SIZE: usize = 32000 /* 32 KiB */;
                        unsafe {
                            AP_STACK_POINTERS[apic_other.id() as usize] =
                                libstd::memory::malloc::get()
                                    .alloc(STACK_SIZE, None)
                                    .expect("Failed to allocate stack for LPU")
                                    .into_parts()
                                    .0 as *mut _ // `c_void` has no alignment, so avoid overhead by direct casting
                                                 // (rather than using `.cast()`).
                        };

                        icr.send_init(apic_other.id());
                        icr.wait_pending();

                        icr.send_sipi(ap_trampoline_page_index, apic_other.id());
                        icr.wait_pending();
                        icr.send_sipi(ap_trampoline_page_index, apic_other.id());
                        icr.wait_pending();
                    }
                }
            }
        }
    }

    if libstd::lpu::is_bsp() {
        use crate::drivers::nvme::*;
        use libstd::{
            acpi::rdsp::xsdt::{mcfg::MCFG, XSDT},
            io::pci,
        };

        if let Ok(mcfg) = XSDT.find_sub_table::<MCFG>() {
            let bridges: alloc::vec::Vec<pci::PCIeHostBridge> = mcfg
                .iter()
                .filter_map(|entry| pci::configure_host_bridge(entry).ok())
                .collect();

            for device_variant in bridges
                .iter()
                .flat_map(|bridge| bridge.iter())
                .flat_map(|bus| bus.iter())
            {
                if let pci::DeviceVariant::Standard(device) = device_variant {
                    if device.class() == pci::DeviceClass::MassStorageController
                        && device.subclass() == 0x08
                    {
                        // // NVMe device
                        // let mut nvme = Controller::from_device(&device);

                        // let admin_sq = libstd::slice!(u8, 0x1000);
                        // let admin_cq = libstd::slice!(u8, 0x1000);

                        // let cc = nvme.controller_configuration();
                        // cc.set_iosqes(4);
                        // cc.set_iocqes(4);

                        // if unsafe { !nvme.safe_set_enable(true) } {
                        //     error!("NVMe controleler failed to safely enable.");
                        //     break;
                        // }
                    }
                }
            }
        }

        loop {}

        info!("Kernel has reached safe shutdown state.");
        unsafe { libstd::instructions::pwm::qemu_shutdown() }
    } else {
        libstd::instructions::hlt_indefinite()
    }
}

fn reserve_system_frames(memory_map: &[UEFIMemoryDescriptor]) {
    let falloc = falloc::get();

    let mut last_frame_end = 0;
    for descriptor in memory_map {
        let frame_index = descriptor.phys_start.frame_index();
        let frame_count = descriptor.page_count as usize;

        if !descriptor.phys_start.is_aligned(0x1000) {
            warn!("Found unaligned UEFI memory descriptor! Refusing to process.");
            continue;

        // Checks for 'holes' in system memory which we shouldn't try to allocate to.
        } else if last_frame_end < frame_index {
            for frame_index in last_frame_end..frame_index {
                unsafe { falloc.try_modify_type(frame_index, falloc::FrameType::Unusable) };
            }
        };

        if descriptor.should_reserve() {
            falloc.lock_many(frame_index, frame_count);
        }

        for frame_index in frame_index..(frame_index + frame_count) {
            unsafe {
                falloc.try_modify_type(
                    frame_index,
                    if descriptor.should_reserve() {
                        falloc::FrameType::Reserved
                    } else {
                        falloc::FrameType::Usable
                    },
                );
            }
        }

        last_frame_end = frame_index + frame_count;
    }
}

fn init_system_config_table(config_table: &[SystemConfigTableEntry]) {
    info!("Initializing system configuration table.");
    let config_table_ptr = config_table.as_ptr();
    let config_table_entry_len = config_table.len();

    let frame_index = (config_table_ptr as usize) / 0x1000;
    let frame_count =
        (config_table_entry_len * core::mem::size_of::<SystemConfigTableEntry>()) / 0x1000;

    unsafe {
        // Assign system configuration table prior to reserving frames to ensure one doesn't already exist.
        libstd::acpi::init_system_config_table(config_table_ptr, config_table_entry_len);

        let frame_range = frame_index..(frame_index + frame_count);
        debug!("System configuration table: {:?}", frame_range);
        let falloc = falloc::get();
        for frame_index in frame_index..(frame_index + frame_count) {
            falloc.borrow(frame_index);
        }
    }
}

fn init_apic() {
    use libstd::structures::idt;

    let apic = &libstd::lpu::get().apic();

    apic.auto_configure_timer_frequency();

    idt::set_interrupt_handler(32, timer::apic_tick_handler);
    apic.timer().set_vector(32);
    idt::set_interrupt_handler(58, apic_error_handler);
    apic.error().set_vector(58);

    apic.timer()
        .set_mode(libstd::structures::apic::TimerMode::Periodic);
    apic.timer().set_masked(false);
    apic.sw_enable();

    info!("Core-local APIC configured and enabled.");
}

extern "x86-interrupt" fn apic_error_handler(_: libstd::structures::idt::InterruptStackFrame) {
    let apic = &libstd::lpu::get().apic();

    error!("APIC ERROR INTERRUPT");
    error!("--------------------");
    error!("DUMPING APIC ERROR REGISTER:");
    error!("  {:?}", apic.error_status());

    apic.end_of_interrupt();
}
