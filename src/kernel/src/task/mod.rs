mod context;
pub use context::*;

mod scheduling;
pub use scheduling::*;

mod address_space;
pub use address_space::*;

use alloc::{boxed::Box, string::String, vec::Vec};
use bit_field::BitField;
use core::num::NonZeroUsize;
use elf::{endian::AnyEndian, file::FileHeader, segment::ProgramHeader};
use libsys::{page_size, Address, Virtual};

#[allow(clippy::cast_possible_truncation)]
pub const STACK_SIZE: NonZeroUsize = NonZeroUsize::new((libsys::MIBIBYTE as usize) - page_size()).unwrap();
pub const STACK_PAGES: NonZeroUsize = NonZeroUsize::new(STACK_SIZE.get() / page_size()).unwrap();
pub const STACK_START: NonZeroUsize = NonZeroUsize::new(page_size()).unwrap();
pub const MIN_LOAD_OFFSET: usize = STACK_START.get() + STACK_SIZE.get();

pub const PT_FLAG_EXEC_BIT: usize = 0;
pub const PT_FLAG_WRITE_BIT: usize = 1;

pub fn segment_to_mmap_permissions(segment_ty: u32) -> MmapPermissions {
    match (segment_ty.get_bit(PT_FLAG_WRITE_BIT), segment_ty.get_bit(PT_FLAG_EXEC_BIT)) {
        (true, false) => MmapPermissions::ReadWrite,
        (false, true) => MmapPermissions::ReadExecute,
        (false, false) => MmapPermissions::ReadOnly,
        (true, true) => panic!("ELF section is WX"),
    }
}

errorgen! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Error {
        AlreadyMapped => None,
        AddressUnderrun { addr: Address<Virtual> } => None,
        UnhandledAddress { addr: Address<Virtual> } => None
    }
}

pub static TASK_LOAD_BASE: usize = 0x20000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Idle = 0,
    Low = 1,
    Normal = 2,
    High = 3,
    Critical = 4,
}

#[derive(Debug, Clone, Copy)]
pub struct ElfRela {
    pub address: Address<Virtual>,
    pub value: usize,
}

pub type Context = (State, Registers);

#[derive(Debug)]
pub enum ElfData {
    Memory(Box<[u8]>),
    File(String),
}

pub struct Task {
    id: uuid::Uuid,
    priority: Priority,

    address_space: AddressSpace,
    context: Context,
    load_offset: usize,

    elf_header: FileHeader<AnyEndian>,
    elf_segments: Box<[ProgramHeader]>,
    elf_relas: Vec<ElfRela>,
    elf_data: ElfData,
}

impl Task {
    pub fn new(
        priority: Priority,
        mut address_space: AddressSpace,
        load_offset: usize,
        elf_header: FileHeader<AnyEndian>,
        elf_segments: Box<[ProgramHeader]>,
        elf_relas: Vec<ElfRela>,
        elf_data: ElfData,
    ) -> Self {
        trace!("Generating a random ID for new task.");
        let id = uuid::Uuid::new_v4();

        trace!("Allocating userspace stack for task: {:?}.", id);
        let stack = address_space
            .mmap(Some(Address::new_truncate(STACK_START.get())), STACK_PAGES, MmapPermissions::ReadWrite)
            .unwrap();

        Self {
            id,
            priority,
            address_space,
            context: (
                State::user(
                    Address::new(load_offset + usize::try_from(elf_header.e_entry).unwrap()).unwrap(),
                    // Safety: Addition keeps the pointer within the bounds of the allocation, and the unit size is 1.
                    unsafe { Address::from_ptr(stack.as_non_null_ptr().as_ptr().add(stack.len())) },
                ),
                Registers::default(),
            ),
            load_offset,
            elf_header,
            elf_segments,
            elf_relas,
            elf_data,
        }
    }

    #[inline]
    pub const fn id(&self) -> uuid::Uuid {
        self.id
    }

    #[inline]
    pub const fn priority(&self) -> Priority {
        self.priority
    }

    #[inline]
    pub const fn address_space(&self) -> &AddressSpace {
        &self.address_space
    }

    #[inline]
    pub fn address_space_mut(&mut self) -> &mut AddressSpace {
        &mut self.address_space
    }

    #[inline]
    pub const fn load_offset(&self) -> usize {
        self.load_offset
    }

    #[inline]
    pub const fn elf_header(&self) -> &FileHeader<AnyEndian> {
        &self.elf_header
    }

    #[inline]
    pub const fn elf_segments(&self) -> &[ProgramHeader] {
        &self.elf_segments
    }

    #[inline]
    pub const fn elf_data(&self) -> &ElfData {
        &self.elf_data
    }

    #[inline]
    pub fn elf_relas(&mut self) -> &mut Vec<ElfRela> {
        &mut self.elf_relas
    }

    pub fn demand_map(&mut self, address: Address<Virtual>) -> Result<()> {
        use crate::mem::paging::TableEntryFlags;
        use core::mem::MaybeUninit;
        use libsys::Page;

        let fault_page = Address::new_truncate(address.get());

        if self.address_space().is_mmapped(fault_page) {
            return Err(Error::AlreadyMapped);
        }

        let fault_unoffset =
            address.get().checked_sub(self.load_offset()).ok_or(Error::AddressUnderrun { addr: address })?;

        let segment = self
            .elf_segments()
            .iter()
            .filter(|phdr| phdr.p_type == elf::abi::PT_LOAD)
            .find(|phdr| {
                (phdr.p_vaddr..(phdr.p_vaddr + phdr.p_memsz)).contains(&u64::try_from(fault_unoffset).unwrap())
            })
            .copied()
            .ok_or(Error::UnhandledAddress { addr: address })?;

        // Small check to help ensure the segment alignments are page-fit.
        debug_assert_eq!(segment.p_align & (libsys::page_mask() as u64), 0);

        debug!("Demand mapping {:X?} from segment: {:X?}", Address::<Page>::new_truncate(address.get()), segment);

        let fault_unoffset_page: Address<Page> = Address::new_truncate(fault_unoffset);
        let fault_unoffset_page_addr = fault_unoffset_page.get().get();

        let fault_unoffset_end_page: Address<Page> = Address::new_truncate(fault_unoffset_page_addr + page_size());
        let fault_unoffset_end_page_addr = fault_unoffset_end_page.get().get();

        let segment_addr = usize::try_from(segment.p_vaddr).unwrap();
        let segment_size = usize::try_from(segment.p_filesz).unwrap();
        let segment_end_addr = segment_addr + segment_size;

        let fault_offset = fault_unoffset_page_addr.saturating_sub(segment_addr);
        let fault_end_pad = fault_unoffset_end_page_addr.saturating_sub(segment_end_addr);
        let fault_front_pad = segment_addr.saturating_sub(fault_unoffset_page_addr);
        let fault_size = ((fault_unoffset_end_page_addr - fault_unoffset_page_addr) - fault_front_pad) - fault_end_pad;

        trace!("Mapping the demand page RW so data can be copied.");
        let mapped_memory = self
            .address_space_mut()
            .mmap(Some(fault_page), core::num::NonZeroUsize::MIN, crate::task::MmapPermissions::ReadWrite)
            .unwrap();
        // Safety: Address space allocator fulfills all required invariants.
        let mapped_memory = unsafe { mapped_memory.as_uninit_slice_mut() };

        let (front_pad, remaining) = mapped_memory.split_at_mut(fault_front_pad);
        let (file_memory, end_pad) = remaining.split_at_mut(fault_size);

        debug_assert_eq!(fault_front_pad, front_pad.len(), "front padding");
        debug_assert_eq!(fault_end_pad, end_pad.len(), "end padding");
        debug_assert_eq!(fault_size, file_memory.len(), "file memory");

        trace!(
            "Copying memory into demand mapping: {:#X}..{:#X}..{:#X}.",
            front_pad.len(),
            file_memory.len(),
            end_pad.len()
        );
        front_pad.fill(MaybeUninit::uninit());
        end_pad.fill(MaybeUninit::uninit());

        if !file_memory.is_empty() {
            match self.elf_data() {
                ElfData::Memory(data) => {
                    let segment_data_offset = usize::try_from(segment.p_offset).unwrap();

                    let offset_segment_range =
                        (segment_data_offset + fault_offset)..(segment_data_offset + fault_offset + fault_size);

                    // Safety: Same-sized reinterpret for copying.
                    let (_, copy_data, _) = unsafe { data[offset_segment_range].align_to() };

                    file_memory.copy_from_slice(copy_data);
                }
                ElfData::File(_) => unimplemented!(),
            }
        }

        // Safety: Slice has been initialized with values.
        let _mapped_memory = unsafe { MaybeUninit::slice_assume_init_mut(mapped_memory) };

        trace!("Processing demand mapping relocations.");
        let load_offset = self.load_offset();
        let fault_page_as_range = fault_unoffset_page_addr..fault_unoffset_end_page_addr;

        self.elf_relas().retain(|rela| {
            if fault_page_as_range.contains(&rela.address.get()) {
                trace!("Processing relocation: {:X?}", rela);
                // Safety: Fault page is checked to contain the relocation's address, and the pointer is guaranteed after
                // offset to lie within the memory mapped region above.
                unsafe {
                    rela.address.as_ptr().add(load_offset).cast::<usize>().write(rela.value);
                }

                false
            } else {
                true
            }
        });

        trace!("Finalizing page's access attributes.");
        // Safety: Page is already mapped, permissions are being modified according to the segment access type.
        unsafe {
            self.address_space_mut()
                .set_flags(
                    fault_page,
                    core::num::NonZeroUsize::new(1).unwrap(),
                    TableEntryFlags::PRESENT
                        | TableEntryFlags::USER
                        | TableEntryFlags::from(crate::task::segment_to_mmap_permissions(segment.p_type)),
                )
                .unwrap();
        }

        trace!("Demand mapping complete.");

        Ok(())
    }
}

impl core::fmt::Debug for Task {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Task")
            .field("ID", &self.id)
            .field("Priority", &self.priority)
            .field("Address Space", &self.address_space)
            .field("Context", &self.context)
            .field("ELF Load Offset", &self.load_offset)
            .field("ELF Header", &self.elf_header)
            .finish_non_exhaustive()
    }
}
