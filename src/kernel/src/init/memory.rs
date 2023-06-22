use crate::mem::paging::{self, TableDepth, TableEntryFlags};
use core::ops::Range;
use libsys::{page_size, Address};

errorgen! {
    #[derive(Debug)]
    pub enum Error {
        KernelAddress => None,
        KernelElf { err: elf::ParseError } => Some(err),
        Paging { err: paging::Error } => Some(err),
        Boot { err: crate::init::boot::Error } => Some(err)
    }
}

struct KernelAddresses {
    phys: usize,
    virt: usize,
}

fn get_kernel_addresses() -> Result<KernelAddresses> {
    #[limine::limine_tag]
    static LIMINE_KERNEL_ADDR: limine::KernelAddressRequest =
        limine::KernelAddressRequest::new(crate::init::boot::LIMINE_REV);

    LIMINE_KERNEL_ADDR
        .get_response()
        .map(|response| KernelAddresses {
            phys: usize::try_from(response.physical_base()).unwrap(),
            virt: usize::try_from(response.virtual_base()).unwrap(),
        })
        .ok_or(Error::KernelAddress)
}

#[allow(clippy::too_many_lines)]
pub fn setup(kernel_file: &limine::File) -> Result<()> {
    // Extract kernel address information.
    let kernel_addresses = get_kernel_addresses()?;

    debug!("Preparing kernel memory system.");

    // Safety: Bootloader guarantees the provided information to be correct.
    let kernel_elf = elf::ElfBytes::<elf::endian::AnyEndian>::minimal_parse(kernel_file.data())
        .map_err(|err| Error::KernelElf { err })?;

    /* load and map segments */

    debug!("Mapping the higher-half direct map.");
    crate::mem::with_kmapper(|kmapper| {
        let mmap_iter =
            &mut crate::init::boot::get_memory_map().map_err(|err| Error::Boot { err })?.iter().map(|entry| {
                let range = entry.range();
                (usize::try_from(range.start).unwrap()..usize::try_from(range.end).unwrap(), entry.ty())
            });

        let mut last_end = 0;
        while let Some((mut acc_range, acc_ty)) = mmap_iter.next() {
            if let Some((end_range, _)) =
                mmap_iter.take_while(|(range, ty)| acc_range.end == range.start && acc_ty.eq(ty)).last()
            {
                acc_range.end = end_range.end;
            }

            if acc_range.start > last_end {
                map_hhdm_range(kmapper, last_end..acc_range.start, TableEntryFlags::RW, true)?;
            }

            last_end = acc_range.end;

            let mmap_args = {
                use limine::MemoryMapEntryType;

                match acc_ty {
                    MemoryMapEntryType::Usable => Some((TableEntryFlags::RW, false)),

                    MemoryMapEntryType::AcpiNvs
                    | MemoryMapEntryType::AcpiReclaimable
                    | MemoryMapEntryType::BootloaderReclaimable
                    | MemoryMapEntryType::Framebuffer => Some((TableEntryFlags::RW, true)),

                    MemoryMapEntryType::Reserved | MemoryMapEntryType::KernelAndModules => {
                        Some((TableEntryFlags::RO, true))
                    }

                    MemoryMapEntryType::BadMemory => None,
                }
            };

            if let Some((flags, lock_frames)) = mmap_args {
                map_hhdm_range(kmapper, acc_range, flags, lock_frames)?;
            } else {
                trace!("HHDM Map (!! bad memory !!) @{:#X?}", acc_range);
            }
        }

        /* load kernel segments */
        kernel_elf
            .segments()
            .expect("kernel file has no segments")
            .into_iter()
            .filter(|ph| ph.p_type == elf::abi::PT_LOAD)
            .try_for_each(|phdr| {
                extern "C" {
                    static KERNEL_BASE: libkernel::LinkerSymbol;
                }

                debug!("{:X?}", phdr);

                // Safety: `KERNEL_BASE` is a linker symbol to an in-executable memory location, so it is guaranteed to be valid (and is never written to).
                let base_offset = usize::try_from(phdr.p_vaddr).unwrap() - unsafe { KERNEL_BASE.as_usize() };
                let base_offset_end = base_offset + usize::try_from(phdr.p_memsz).unwrap();
                let flags = TableEntryFlags::from(crate::task::segment_to_mmap_permissions(phdr.p_flags));

                (base_offset..base_offset_end)
                    .step_by(page_size())
                    // Attempt to map the page to the frame.
                    .try_for_each(|offset| {
                        let phys_addr = Address::new(kernel_addresses.phys + offset).unwrap();
                        let virt_addr = Address::new(kernel_addresses.virt + offset).unwrap();

                        trace!("Map  {:X?} -> {:X?}   {:?}", virt_addr, phys_addr, flags);
                        kmapper
                            .map(virt_addr, TableDepth::min(), phys_addr, true, flags)
                            .map_err(|err| Error::Paging { err })
                    })
            })?;

        debug!("Switching to kernel page tables...");
        // Safety: Kernel mappings should be identical to the bootloader mappings.
        unsafe { kmapper.swap_into() };
        debug!("Kernel has finalized control of page tables.");

        Ok(())
    })
}

fn map_hhdm_range(
    mapper: &mut crate::mem::mapper::Mapper,
    mut range: Range<usize>,
    flags: TableEntryFlags,
    lock_frames: bool,
) -> Result<()> {
    use crate::mem::HHDM;

    let huge_page_depth = TableDepth::new(1).unwrap();

    trace!("HHDM Map  {:#X?}  {:?}   lock {}", range, flags, lock_frames);

    while !range.is_empty() {
        if range.len() > huge_page_depth.align()
            && range.start.trailing_zeros() >= huge_page_depth.align().trailing_zeros()
        {
            // Map a huge page

            let frame = Address::new(range.start).unwrap();
            let page = HHDM.offset(frame).unwrap();
            range.advance_by(huge_page_depth.align()).unwrap();

            mapper
                .map(page, huge_page_depth, frame, lock_frames, flags | TableEntryFlags::HUGE)
                .map_err(|err| Error::Paging { err })?;
        } else {
            // Map a standard page

            let frame = Address::new(range.start).unwrap();
            let page = HHDM.offset(frame).unwrap();
            range.advance_by(page_size()).unwrap();

            mapper.map(page, TableDepth::min(), frame, lock_frames, flags).map_err(|err| Error::Paging { err })?;
        }
    }

    Ok(())
}
