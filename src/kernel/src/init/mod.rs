mod arch;
mod memory;

mod params;
pub use params::*;

pub mod boot;

use libsys::Address;

errorgen! {
    #[derive(Debug)]
    pub enum Error {
        Memory { err: memory::Error } => Some(err)
    }
}

pub static KERNEL_HANDLE: spin::Lazy<uuid::Uuid> = spin::Lazy::new(uuid::Uuid::new_v4);

#[allow(clippy::too_many_lines)]
pub unsafe extern "C" fn init() -> ! {
    use core::sync::atomic::{AtomicBool, Ordering};

    #[limine::limine_tag]
    static LIMINE_KERNEL_FILE: limine::KernelFileRequest = limine::KernelFileRequest::new(boot::LIMINE_REV);

    static INIT: AtomicBool = AtomicBool::new(false);
    assert!(!INIT.load(Ordering::Acquire), "`init()` has already been called!");
    INIT.store(true, Ordering::Release);

    setup_logging();
    arch::cpu_setup();
    print_boot_info();

    let kernel_file = LIMINE_KERNEL_FILE
        .get_response()
        .map(limine::KernelFileResponse::file)
        .expect("bootloader did not respond to kernel file request");

    params::parse(kernel_file.cmdline());
    crate::mem::alloc::pmm::init(boot::get_memory_map().unwrap()).unwrap();
    crate::panic::symbols::parse(kernel_file).unwrap();
    memory::setup(kernel_file).unwrap();

    crate::acpi::init_interface().unwrap();

    crate::mem::io::pci::init_devices().unwrap();

    load_drivers();

    setup_smp();

    crate::init::boot::reclaim_memory().unwrap();

    kernel_core_setup()
}

/// ### Safety
///
/// This function should only ever be called once per core.
pub(self) unsafe fn kernel_core_setup() -> ! {
    crate::cpu::state::init(1000);

    // Ensure we enable interrupts prior to enabling the scheduler.
    crate::interrupts::enable();
    crate::cpu::state::begin_scheduling().unwrap();

    // This interrupt wait loop is necessary to ensure the core can jump into the scheduler.
    crate::interrupts::wait_loop()
}

fn setup_logging() {
    if cfg!(debug_assertions) {
        // Logging isn't set up, so we'll just spin loop if we fail to initialize it.
        crate::logging::init().unwrap_or_else(|_| crate::interrupts::wait_loop());
    } else {
        // Logging failed to initialize, but just continue to boot (only in release).
        crate::logging::init().ok();
    }
}

fn print_boot_info() {
    #[limine::limine_tag]
    static BOOT_INFO: limine::BootInfoRequest = limine::BootInfoRequest::new(crate::init::boot::LIMINE_REV);

    if let Some(boot_info) = BOOT_INFO.get_response() {
        info!("Bootloader Info     {} v{} (rev {})", boot_info.name(), boot_info.version(), boot_info.revision());
    } else {
        info!("No bootloader info available.");
    }

    // Vendor strings from the CPU need to be enumerated per-platform.
    #[cfg(target_arch = "x86_64")]
    if let Some(vendor_info) = crate::arch::x86_64::cpuid::VENDOR_INFO.as_ref() {
        info!("Vendor              {}", vendor_info.as_str());
    } else {
        info!("Vendor              Unknown");
    }
}

fn load_drivers() {
    use crate::task::{AddressSpace, Priority, Task};
    use elf::endian::AnyEndian;

    #[limine::limine_tag]
    static LIMINE_MODULES: limine::ModuleRequest = limine::ModuleRequest::new(crate::init::boot::LIMINE_REV);

    debug!("Unpacking kernel drivers...");

    let Some(modules) = LIMINE_MODULES.get_response()
    else {
        warn!("Bootloader provided no modules; skipping driver loading.");
        return
    };

    let modules = modules.modules();
    trace!("Found modules: {:X?}", modules);

    let Some(drivers_module) = modules.iter().find(|module| module.path().ends_with("drivers"))
    else {
        panic!("no drivers module found")
    };

    let archive = tar_no_std::TarArchiveRef::new(drivers_module.data());
    archive
        .entries()
        .filter_map(|entry| {
            debug!("Attempting to parse driver blob: {}", entry.filename());

            match elf::ElfBytes::<AnyEndian>::minimal_parse(entry.data()) {
                Ok(elf) => Some((entry, elf)),
                Err(err) => {
                    error!("Failed to parse driver blob into ELF: {:?}", err);
                    None
                }
            }
        })
        .for_each(|(entry, elf)| {
            // Get and copy the ELF segments into a small box.
            let Some(segments_copy) = elf.segments().map(|segments| segments.into_iter().collect())
            else {
                error!("ELF has no segments.");
                return
            };

            // Safety: In-place transmutation of initialized bytes for the purpose of copying safely.
            // let (_, archive_data, _) = unsafe { entry.data().align_to::<MaybeUninit<u8>>() };
            trace!("Allocating ELF data into memory...");
            let elf_data = alloc::boxed::Box::from(entry.data());
            trace!("ELF data allocated into memory.");

            let Ok((Some(shdrs), Some(_))) = elf.section_headers_with_strtab()
            else {
                panic!("Error retrieving ELF relocation metadata.")
            };

            let load_offset = crate::task::MIN_LOAD_OFFSET;

            trace!("Processing relocations localized to fault page.");
            let mut relas = alloc::vec::Vec::with_capacity(shdrs.len());

            shdrs
                .iter()
                .filter(|shdr| shdr.sh_type == elf::abi::SHT_RELA)
                .flat_map(|shdr| elf.section_data_as_relas(&shdr).unwrap())
                .for_each(|rela| {
                    use crate::task::ElfRela;

                    match rela.r_type {
                        elf::abi::R_X86_64_RELATIVE => relas.push(ElfRela {
                            address: Address::new(usize::try_from(rela.r_offset).unwrap()).unwrap(),
                            value: load_offset + usize::try_from(rela.r_addend).unwrap(),
                        }),

                        _ => unimplemented!(),
                    }
                });

            trace!("Finished processing relocations, pushing task.");

            let task = Task::new(
                Priority::Normal,
                AddressSpace::new_userspace(),
                load_offset,
                elf.ehdr,
                segments_copy,
                relas,
                crate::task::ElfData::Memory(elf_data),
            );

            crate::task::PROCESSES.lock().push_back(task);
        });
}

fn setup_smp() {
    #[limine::limine_tag]
    static LIMINE_SMP: limine::SmpRequest = limine::SmpRequest::new(crate::init::boot::LIMINE_REV)
        // Enable x2APIC mode if available.
        .flags(0b1);

    // Safety: `LIMINE_SMP` is only ever accessed within this individual context, and is effectively
    //          dropped as soon as this context goes out of scope.
    let limine_smp = unsafe { &mut *(&raw const LIMINE_SMP).cast_mut() };

    debug!("Detecting and starting additional cores.");

    limine_smp.get_response_mut().map(limine::SmpResponse::cpus).map_or_else(
        || debug!("Bootloader detected no additional CPU cores."),
        // Iterate all of the CPUs, and jump them to the SMP function.
        |cpus| {
            for cpu_info in cpus {
                trace!("Starting processor: ID P{}/L{}", cpu_info.processor_id(), cpu_info.lapic_id());

                if params::get().smp {
                    extern "C" fn _smp_entry(_: &limine::CpuInfo) -> ! {
                        arch::cpu_setup();

                        // Safety: All currently referenced memory should also be mapped in the kernel page tables.
                        crate::mem::with_kmapper(|kmapper| unsafe { kmapper.swap_into() });

                        // Safety: Function is called only once for this core.
                        unsafe { kernel_core_setup() }
                    }

                    // If smp is enabled, jump to the smp entry function.
                    cpu_info.jump_to(_smp_entry, None);
                } else {
                    extern "C" fn _idle_forever(_: &limine::CpuInfo) -> ! {
                        // Safety: Murder isn't legal. Is this?
                        unsafe { crate::interrupts::halt_and_catch_fire() }
                    }

                    // If smp is disabled, jump to the park function for the core.
                    cpu_info.jump_to(_idle_forever, None);
                }
            }
        },
    );
}
