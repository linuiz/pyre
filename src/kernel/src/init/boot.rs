use core::sync::atomic::{AtomicBool, Ordering};
use libsys::{Address, Virtual};

mod ignore {
    ///! This module is never exported. It is used for bootloader requests that should never be accessed in software.

    #[limine::limine_tag]
    static LIMINE_STACK: limine::StackSizeRequest = limine::StackSizeRequest::new(super::LIMINE_REV).stack_size({
        #[cfg(debug_assertions)]
        {
            0x1000000
        }

        #[cfg(not(debug_assertions))]
        {
            0x4000
        }
    });
}

errorgen! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Error {
        BootExpired => None,
        NoRsdpAddress => None,
        NoMemoryMap => None
    }
}

pub const LIMINE_REV: u64 = 0;

static BOOT_RECLAIM: AtomicBool = AtomicBool::new(false);

macro_rules! boot_only {
    ($code:block) => {{
        if !BOOT_RECLAIM.load(Ordering::Acquire) {
            Ok($code)
        } else {
            Err(Error::BootExpired)
        }
    }};
}

pub fn get_memory_map() -> Result<&'static [&'static limine::MemmapEntry]> {
    boot_only!({
        #[limine::limine_tag]
        static LIMINE_MMAP: limine::MemmapRequest = limine::MemmapRequest::new(LIMINE_REV);

        LIMINE_MMAP.get_response().map(limine::MemmapResponse::get_memmap).ok_or(Error::NoMemoryMap)
    })
    .flatten()
}

pub fn get_rsdp_address() -> Result<Address<Virtual>> {
    boot_only!({
        #[limine::limine_tag]
        static LIMINE_RSDP: limine::RsdpRequest = limine::RsdpRequest::new(LIMINE_REV);

        LIMINE_RSDP
            .get_response()
            .and_then(limine::RsdpResponse::address)
            .and_then(|ptr| {
                Address::new(
                    // Properly handle the bootloader's mapping of ACPI addresses in lower-half or higher-half memory space.
                    core::cmp::min(ptr.addr().get(), ptr.addr().get().wrapping_sub(crate::mem::HHDM.address().get())),
                )
            })
            .ok_or(Error::NoRsdpAddress)
    })
    .flatten()
}

#[derive(Debug, Clone, Copy)]
pub struct ReclaimMemoryError;

/// # Safety
///
/// No dangling references can remain to bootloader types or memory, as it may be concurrently overwritten.
pub unsafe fn reclaim_memory() -> core::result::Result<(), ReclaimMemoryError> {
    static BOOT_RECLAIM: AtomicBool = AtomicBool::new(false);
    assert!(!BOOT_RECLAIM.load(Ordering::Acquire));

    debug!("Reclaiming bootloader memory...");

    get_memory_map()
        .unwrap()
        .iter()
        .filter(|entry| entry.ty() == limine::MemoryMapEntryType::BootloaderReclaimable)
        .flat_map(|entry| entry.range().step_by(libsys::page_size()))
        .map(|address| Address::<libsys::Frame>::new(address.try_into().unwrap()).unwrap())
        .try_for_each(|frame| crate::mem::alloc::pmm::get().free_frame(frame).map_err(|_| ReclaimMemoryError))?;

    BOOT_RECLAIM.store(true, Ordering::Release);

    debug!("Bootloader memory reclaimed.");

    Ok(())
}
