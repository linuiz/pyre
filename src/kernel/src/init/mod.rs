mod arch;
mod memory;

mod params;
pub use params::*;

pub mod boot;

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use libsys::Address;

use crate::time::clock::Instant;

errorgen! {
    #[derive(Debug)]
    pub enum Error {
        Memory { err: memory::Error } => Some(err)
    }
}

pub static KERNEL_HANDLE: spin::Lazy<uuid::Uuid> = spin::Lazy::new(uuid::Uuid::new_v4);
static MEM_READY: AtomicBool = AtomicBool::new(false);

static SMP_COUNT: spin::Once<u32> = spin::Once::new();
static SMP_READY: AtomicU32 = AtomicU32::new(0);

#[doc(hidden)]
#[allow(clippy::too_many_lines)]
pub(super) unsafe extern "C" fn _init() -> ! {
    crate::acpi::init_interface().unwrap();
    crate::time::clock::set_initial_timestamp();

    setup_logging();
    debug!("Logging successfully setup.");

    arch::cpu_setup();
    print_boot_info();

    #[limine::limine_tag]
    static LIMINE_KERNEL_FILE: limine::KernelFileRequest = limine::KernelFileRequest::new(boot::LIMINE_REV);

    let kernel_file = LIMINE_KERNEL_FILE
        .get_response()
        .map(limine::KernelFileResponse::file)
        .expect("bootloader did not respond to kernel file request");

    params::parse(kernel_file.cmdline());

    // Setup SMP early to ensure the cores are parked in mapped regions.
    setup_smp();

    crate::mem::alloc::pmm::init(boot::get_memory_map().unwrap()).unwrap();
    crate::panic::symbols::parse(kernel_file).unwrap();
    memory::setup(kernel_file).unwrap();

    MEM_READY.store(true, Ordering::Relaxed);

    crate::mem::pcie::init_devices().unwrap();

    load_drivers();

    kernel_core_setup(true)
}

/// ### Safety
///
/// This function should only ever be called once per core.
pub(self) unsafe extern "sysv64" fn kernel_core_setup(is_bsp: bool) -> ! {
    SMP_READY.fetch_add(1, Ordering::Relaxed);

    let smp_count = *SMP_COUNT.get().unwrap();
    while SMP_READY.load(Ordering::Relaxed) < smp_count {
        core::hint::spin_loop();
    }

    if is_bsp {
        crate::init::boot::reclaim_memory().unwrap();
    }

    crate::cpu::state::init(1000);

    // Ensure we enable interrupts prior to enabling the scheduler.
    crate::interrupts::enable();
    crate::cpu::state::begin_scheduling().unwrap();

    // This interrupt wait loop is necessary to ensure the core can jump into the scheduler.
    crate::interrupts::wait_loop()
}

fn setup_logging() {
    #[cfg(debug_assertions)]
    {
        crate::logging::init().unwrap_or_else(|_| {
            // Logging isn't set up, so we just fucking die.

            #[cfg(target_arch = "x86_64")]
            {
                crate::arch::x86_64::instructions::breakpoint();
            }
        });
    }
    #[cfg(not(debug_assertions))]
    {
        // If logging fails to initialize in release we don't care (hopefully GUI works).
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

    let Some(cpus) = limine_smp.get_response_mut().map(limine::SmpResponse::cpus)
    else {
        debug!("Bootloader detected no additional CPU cores.");
        return
    };

    SMP_COUNT.call_once(|| (cpus.len() - /* don't count BSP */ 1).try_into().unwrap());
    for cpu_info in cpus {
        trace!("Starting processor: ID P{}/L{}", cpu_info.processor_id(), cpu_info.lapic_id());

        if params::get().smp {
            extern "C" fn _smp_entry(info: &limine::CpuInfo) -> ! {
                use crate::mem::StackUnit;
                use alloc::boxed::Box;

                arch::cpu_setup();

                while !MEM_READY.load(Ordering::Relaxed) {
                    core::hint::spin_loop();
                }

                // Safety: All currently referenced memory should also be mapped in the kernel page tables.
                crate::mem::with_kmapper(|kmapper| unsafe { kmapper.swap_into() });

                info!("Core {} is allocating fresh stack...", info.lapic_id());
                let stack: Box<[core::mem::MaybeUninit<StackUnit>]> =
                    Box::new_uninit_slice(crate::CORE_STACK_SIZE.try_into().unwrap());
                let stack_top = stack.as_ptr_range().end;
                info!("Core {} allocated fresh stack: @{:X?}", info.lapic_id(), stack_top);

                Box::leak(stack);

                // Safety: Memory will have been just allocated.
                unsafe {
                    #[cfg(target_arch = "x86_64")]
                    core::arch::asm!(
                        "
                            mov rsp, {}
                            xor rdi, rdi
                            call {}
                            ",
                        in(reg) stack_top,
                        sym kernel_core_setup,
                        options(nomem, nostack, noreturn)
                    )
                }
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
}
