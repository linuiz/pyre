pub mod walker;
use bit_field::BitField;
use core::{fmt, iter::Step};
use libkernel::mem::{InteriorRef, Mut, Ref};
use libsys::{page_shift, table_index_mask, table_index_shift, table_index_size, Address, Frame, Page, Virtual};

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TableDepth(u32);

impl TableDepth {
    #[inline]
    pub const fn min() -> Self {
        Self(0)
    }

    #[inline]
    pub fn max() -> Self {
        Self({
            #[cfg(target_arch = "x86_64")]
            {
                use crate::arch::x86_64::registers::control;

                if control::CR4::read().contains(control::CR4Flags::LA57) {
                    5
                } else {
                    4
                }
            }
        })
    }

    #[inline]
    pub const fn min_align() -> usize {
        Self::min().align()
    }

    #[inline]
    pub fn max_align() -> usize {
        Self::max().align()
    }

    #[inline]
    pub fn new(depth: u32) -> Option<Self> {
        (Self::min().0..=Self::max().0).contains(&depth).then_some(Self(depth))
    }

    #[inline]
    pub const fn get(self) -> u32 {
        self.0
    }

    #[inline]
    pub const fn align(self) -> usize {
        libsys::page_size().checked_shl(libsys::table_index_shift().get() * self.get()).unwrap()
    }

    #[inline]
    pub fn next(self) -> Self {
        Step::forward(self, 1)
    }

    #[inline]
    pub fn next_checked(self) -> Option<Self> {
        Step::forward_checked(self, 1)
    }

    #[inline]
    pub fn is_min(self) -> bool {
        self == Self::min()
    }

    #[inline]
    pub fn is_max(self) -> bool {
        self == Self::max()
    }

    pub fn index_of(self, address: Address<Virtual>) -> Option<usize> {
        self.get()
            .checked_sub(1)
            .map(|depth| depth * table_index_shift().get())
            .map(|index_shift| (address.get() >> index_shift >> page_shift().get()) & table_index_mask())
    }
}

impl Step for TableDepth {
    fn steps_between(start: &Self, end: &Self) -> Option<usize> {
        Step::steps_between(&end.0, &start.0)
    }

    fn forward_checked(start: Self, count: usize) -> Option<Self> {
        let count = u32::try_from(count).expect("step count too large");
        let total = start.0.checked_sub(count).expect("step count overflowed");

        Self::new(total)
    }

    fn backward_checked(start: Self, count: usize) -> Option<Self> {
        let count = u32::try_from(count).expect("step count too large");
        let total = start.0.checked_add(count).expect("step count overflowed");

        Self::new(total)
    }
}

errorgen! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[allow(clippy::enum_variant_names)]
    pub enum Error {
        /// The underlying allocator is out of memory.
        AllocError => None,

        /// An attempted mapping sits outside the physical memory bounds.
        FrameBounds => None,

        /// Unexpected huge page was encountered.
        HugePage => None,

        /// The specified page is not mapped.
        NotMapped { addr: Address<Virtual> } => None
    }
}

#[cfg(target_arch = "x86_64")]
bitflags::bitflags! {
    #[repr(transparent)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct TableEntryFlags : u64 {
        const PRESENT = 1 << 0;
        const WRITABLE = 1 << 1;
        const USER = 1 << 2;
        const WRITE_THROUGH = 1 << 3;
        const UNCACHEABLE = 1 << 4;
        const ACCESSED = 1 << 5;
        const DIRTY = 1 << 6;
        const HUGE = 1 << 7;
        const GLOBAL = 1 << 8;
        const DEMAND = 1 << 9;
        const NO_EXECUTE = 1 << 63;

        const RO = Self::PRESENT.bits() | Self::NO_EXECUTE.bits();
        const RW = Self::PRESENT.bits() | Self::WRITABLE.bits() | Self::NO_EXECUTE.bits();
        const RX = Self::PRESENT.bits();
        const PTE = Self::PRESENT.bits() | Self::WRITABLE.bits() | Self::USER.bits();

        const MMIO = Self::RW.bits() | Self::UNCACHEABLE.bits();
    }
}

#[cfg(target_arch = "riscv64")]
bitflags::bitflags! {
    #[repr(transparent)]
    pub struct TableEntryFlags: u64 {
        const VALID = 1 << 0;
        const READ = 1 << 1;
        const WRITE = 1 << 2;
        const EXECUTE = 1 << 3;
        const USER = 1 << 4;
        const GLOBAL = 1 << 5;
        const ACCESSED = 1 << 6;
        const DIRTY = 1 << 7;

        const RO = Self::VALID.bits() | Self::READ.bits();
        const RW = Self::VALID.bits() | Self::READ.bits() | Self::WRITE.bits();
        const RX = Self::VALID.bits() | Self::READ.bits() | Self::EXECUTE.bits();
        const PTE = Self::VALID.bits() | Self::READ.bits() | Self::WRITE.bits();

        const MMIO = Self::RW.bits();
    }
}

#[cfg(target_arch = "riscv64")]
pub const PTE_FRAME_ADDRESS_MASK: u64 = 0x003FFFFF_FFFFFC00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagsModify {
    Set,
    Insert,
    Remove,
    Toggle,
}

// TODO impl table levels for attribute masking on x86
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    #[cfg(target_arch = "x86_64")]
    const FRAME_ADDRESS_RANGE: core::ops::Range<usize> = 12..51;

    /// Returns an empty `Self`. All bits of this entry will be 0.
    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn new(frame: Address<Frame>, attributes: TableEntryFlags) -> Self {
        let mut value = Self::empty();

        // Safety: Entry is fresh and has no dangling references.
        unsafe {
            value.set_frame(frame);
            value.set_attributes(attributes, FlagsModify::Set);
        }

        value
    }

    /// Gets the frame index of the page table entry.
    pub fn get_frame(self) -> Address<Frame> {
        Address::from_index(usize::try_from(self.0.get_bits(Self::FRAME_ADDRESS_RANGE)).unwrap()).unwrap()
    }

    /// Sets the entry's frame index.
    ///
    /// ### Safety
    ///
    /// Caller must ensure changing the attributes of this entry does not cause memory corruption.
    #[inline]
    pub unsafe fn set_frame(&mut self, frame: Address<Frame>) {
        self.0.set_bits(Self::FRAME_ADDRESS_RANGE, frame.index().try_into().unwrap());
    }

    /// Gets the attributes of this page table entry.
    #[inline]
    pub const fn get_attributes(self) -> TableEntryFlags {
        TableEntryFlags::from_bits_truncate(self.0)
    }

    /// Sets the attributes of this page table entry.
    ///
    /// ### Safety
    ///
    /// Caller must ensure changing the attributes of this entry does not cause any memory corruption side effects.
    pub unsafe fn set_attributes(&mut self, new_attributes: TableEntryFlags, modify_mode: FlagsModify) {
        // Use `::from_bits_retain()` to avoid needing to re-insert the frame address.
        let mut attributes = TableEntryFlags::from_bits_retain(self.0);

        match modify_mode {
            FlagsModify::Set => {
                attributes.remove(TableEntryFlags::all());
                attributes.insert(new_attributes);
            }
            FlagsModify::Insert => attributes.insert(new_attributes),
            FlagsModify::Remove => attributes.remove(new_attributes),
            FlagsModify::Toggle => attributes.toggle(new_attributes),
        }

        #[cfg(target_arch = "x86_64")]
        if !crate::arch::x86_64::registers::msr::IA32_EFER::get_nxe() {
            // This bit is reserved if NXE is not supported. For now, this means silently removing it for compatability.
            attributes.remove(TableEntryFlags::NO_EXECUTE);
        }

        self.0 = attributes.bits();
    }

    #[inline]
    pub const fn is_present(self) -> bool {
        self.get_attributes().contains(TableEntryFlags::PRESENT)
    }

    #[inline]
    pub const fn is_huge(self) -> bool {
        self.get_attributes().contains(TableEntryFlags::HUGE)
    }
}

impl fmt::Debug for PageTableEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Page Table Entry")
            .field(&self.get_frame())
            .field(&self.get_attributes())
            .field(&self.0)
            .finish()
    }
}

pub struct PageTable<'a, RefKind: InteriorRef> {
    depth: TableDepth,
    entry: <RefKind as InteriorRef>::RefType<'a, PageTableEntry>,
}

impl<RefKind: InteriorRef> core::ops::Deref for PageTable<'_, RefKind> {
    type Target = PageTableEntry;

    fn deref(&self) -> &Self::Target {
        RefKind::shared_ref(&self.entry)
    }
}

impl core::ops::DerefMut for PageTable<'_, Mut> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.entry
    }
}

impl<RefKind: InteriorRef> PageTable<'_, RefKind> {
    #[inline]
    pub const fn depth(&self) -> TableDepth {
        self.depth
    }

    fn table_ptr(&self) -> *mut PageTableEntry {
        crate::mem::HHDM.offset(self.get_frame()).unwrap().as_ptr().cast()
    }

    pub fn entries(&self) -> &[PageTableEntry] {
        // Safety: Type constructor requires the table pointer to be valid.
        unsafe { core::slice::from_raw_parts(self.table_ptr(), table_index_size()) }
    }
}

impl<'a> PageTable<'a, Ref> {
    /// ### Safety
    ///
    /// - Page table entry must point to a valid page table.
    /// - Page table depth must be correct for the provided table.
    pub const unsafe fn new(depth: TableDepth, entry: &'a PageTableEntry) -> Self {
        Self { depth, entry }
    }

    pub fn with_entry<T>(
        &self,
        page: Address<Page>,
        to_depth: Option<TableDepth>,
        with_fn: impl FnOnce(&PageTableEntry) -> T,
    ) -> Result<T> {
        if self.depth() == to_depth.unwrap_or(TableDepth::min()) {
            Ok(with_fn(self.entry))
        } else if !self.is_huge() {
            let next_depth = self.depth().next_checked().unwrap();
            let entry_index = self.depth().index_of(page.get()).unwrap();
            let sub_entry = self.entries().get(entry_index).unwrap();

            if sub_entry.is_present() {
                // Safety: Since the state of the page tables can not be fully modelled or controlled within the kernel itself,
                //          we can't be 100% certain this is safe. However, in the case that it isn't, there's a near-certain
                //          chance that the entire kernel will explode shortly after reading bad data like this as a page table.
                (unsafe { PageTable::<Ref>::new(next_depth, sub_entry) }).with_entry(page, to_depth, with_fn)
            } else {
                Err(Error::NotMapped { addr: page.get() })
            }
        } else {
            Err(Error::HugePage)
        }
    }
}

impl<'a> PageTable<'a, Mut> {
    /// ### Safety
    ///
    /// - Page table entry must point to a valid page table.
    /// - Page table depth must be correct for the provided table.
    pub unsafe fn new(depth: TableDepth, entry: &'a mut PageTableEntry) -> Self {
        Self { depth, entry }
    }

    pub fn entries_mut(&mut self) -> &mut [PageTableEntry] {
        // Safety: Type constructor requires the table pointer to be valid.
        unsafe { core::slice::from_raw_parts_mut(self.table_ptr(), table_index_size()) }
    }

    pub fn with_entry_mut<T>(
        &mut self,
        page: Address<Page>,
        to_depth: Option<TableDepth>,
        with_fn: impl FnOnce(&mut PageTableEntry) -> T,
    ) -> Result<T> {
        if self.depth() == to_depth.unwrap_or(TableDepth::min()) {
            Ok(with_fn(self.entry))
        } else if !self.is_huge() {
            let next_depth = self.depth().next_checked().unwrap();
            let entry_index = self.depth().index_of(page.get()).unwrap();
            let sub_entry = self.entries_mut().get_mut(entry_index).unwrap();

            if sub_entry.is_present() {
                // Safety: Since the state of the page tables can not be fully modelled or controlled within the kernel itself,
                //          we can't be 100% certain this is safe. However, in the case that it isn't, there's a near-certain
                //          chance that the entire kernel will explode shortly after reading bad data like this as a page table.
                (unsafe { PageTable::<Mut>::new(next_depth, sub_entry) }).with_entry_mut(page, to_depth, with_fn)
            } else {
                Err(Error::NotMapped { addr: page.get() })
            }
        } else {
            Err(Error::HugePage)
        }
    }

    /// Attempts to get a mutable reference to the page table that lies in the given entry index's frame, or
    /// creates the sub page table if it doesn't exist. This function returns `None` if it was unable to allocate
    /// a frame for the requested page table.
    pub fn with_entry_create<T>(
        &mut self,
        page: Address<Page>,
        to_depth: TableDepth,
        with_fn: impl FnOnce(&mut PageTableEntry) -> T,
    ) -> Result<T> {
        if self.depth() == to_depth {
            Ok(with_fn(self.entry))
        } else if !self.is_huge() {
            if !self.is_present() {
                debug_assert!(
                    self.get_frame() == Address::default(),
                    "page table entry is non-present, but has a present frame address: {:?} {:?}",
                    self.depth(),
                    self.entry
                );

                let mut flags = TableEntryFlags::PTE;
                // Insert the USER bit in all non-leaf entries.
                // This is for compatibility with the x86 paging scheme.
                if !self.depth().is_min() {
                    flags.insert(TableEntryFlags::USER);
                }

                // Set the entry frame and set attributes to make a valid PTE.
                *self.entry = PageTableEntry::new(
                    crate::mem::alloc::pmm::get().next_frame().map_err(|_| Error::AllocError)?,
                    flags,
                );

                // Clear the table to avoid corrupted PTEs.
                self.entries_mut().fill(PageTableEntry::empty());
            }

            let next_depth = self.depth().next_checked().unwrap();
            let entry_index = self.depth().index_of(page.get()).unwrap();
            let sub_entry = self.entries_mut().get_mut(entry_index).unwrap();
            // Safety: If the page table entry is present, then it's a valid entry, all bits accounted.
            (unsafe { PageTable::<Mut>::new(next_depth, sub_entry) }).with_entry_create(page, to_depth, with_fn)
        } else {
            Err(Error::HugePage)
        }
    }
}
