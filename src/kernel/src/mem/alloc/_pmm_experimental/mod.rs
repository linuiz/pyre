mod region;

use core::alloc::Allocator;
use limine::{MemmapEntry, MemoryMapEntryType};
pub use region::*;

use core::{alloc::Layout, fmt, num::NonZeroU32, ops::Range, ptr::NonNull};

const MAX_TABLE_SIZE: usize = (libsys::page_size() / core::mem::size_of::<Region>()) - 1;
const LAST_TABLE_INDEX: usize = MAX_TABLE_SIZE - 1;
const FIRST_TABLE_INDEX: usize = 0;

#[repr(C)]
pub struct PhysicalMemoryManager {
    table: [Region; MAX_TABLE_SIZE],
    len: usize,
    total_memory: usize,
    total_usable_memory: usize,

    next_table_ptr: Option<NonNull<Self>>,
}

impl PhysicalMemoryManager {
    pub unsafe fn new(regions: &[&MemmapEntry]) -> Self {
        let mut region_table = Self {
            table: [Region::undefined(); MAX_TABLE_SIZE],
            len: 0,
            total_memory: 0,
            total_usable_memory: 0,
            next_table_ptr: None,
        };

        for region in regions {
            let region_range = region.range();
            region_table.total_memory += region_range.len();

            let region_kind = match region.ty() {
                MemoryMapEntryType::Reserved | MemoryMapEntryType::AcpiNvs | MemoryMapEntryType::Framebuffer => {
                    Kind::Reserved
                }

                MemoryMapEntryType::Usable => {
                    region_table.total_usable_memory += region_range.len();

                    Kind::Generic
                }

                MemoryMapEntryType::BadMemory => Kind::Unusable,
                MemoryMapEntryType::AcpiReclaimable => Kind::AcpiReclaim,
                MemoryMapEntryType::BootloaderReclaimable => Kind::BootReclaim,
                MemoryMapEntryType::KernelAndModules => Kind::BootReclaim,
            };

            let region = Region::new(region_kind, region_range.start.try_into().unwrap(), region_range.len());
            region_table.insert(region);
        }

        Ok(region_table)
    }

    #[inline]
    const fn is_full(&self) -> bool {
        self.len == MAX_TABLE_SIZE
    }

    #[inline]
    const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    fn table(&self) -> &[Region] {
        &self.table[..self.len]
    }
    #[inline]
    fn table_mut(&mut self) -> &mut [Region] {
        &mut self.table[..self.len]
    }

    #[inline]
    fn next_table(&self) -> Option<&Self> {
        // Safety: If pointer is non-null, it's been allocated.
        self.next_table_ptr.map(|ptr| unsafe { ptr.as_ref() })
    }

    #[inline]
    fn next_table_mut(&mut self) -> Option<&mut Self> {
        // Safety: If pointer is non-null, it's been allocated.
        self.next_table_ptr.map(|mut ptr| unsafe { ptr.as_mut() })
    }

    /// Shuffles all elements at the given index down by one.
    fn shuffle_out(&mut self, index: usize) -> Option<Region> {
        trace!("shuffling out of index {}", index);

        assert!(index <= self.len && index < MAX_TABLE_SIZE);

        let old_len = self.len;
        let shuffled_item = self.table().first().copied();
        let next_shuffled_item = self.next_table_mut().and_then(|next_table| next_table.shuffle_out(FIRST_TABLE_INDEX));

        // Shuffle all the elements down by 1.
        trace!("copying {:?} to {}", (index + 1)..old_len, index);
        self.table.copy_within((index + 1)..old_len, index);

        if let Some(next_shuffled_item) = next_shuffled_item {
            self.table_mut()[LAST_TABLE_INDEX] = next_shuffled_item;

            let next_table = self.next_table_mut().unwrap();
            // Deallocate the next table only when the table after the next is also empty.
            //
            // This keeps a buffer table on hand to ensure we aren't allocating and deallocating
            // a new table every time we shuffle up or down when full.
            if next_table.is_empty() && let Some(next_next_table) = next_table.next_table() && next_next_table.is_empty() {
                todo!("deallocate table only if next table's next table is emtpy (keep 1 buffer table)");
            }
        } else {
            self.len -= 1;
        }

        shuffled_item
    }

    fn shuffle_in(&mut self, index: usize, item: Region) {
        trace!("shuffling into index {}", index);

        assert!(index <= self.len && index < MAX_TABLE_SIZE);

        let old_len = self.len;

        // If the last possible index exists, we need to shuffle that item into the next table.
        if let Some(shuffled_item) = self.table().get(LAST_TABLE_INDEX).copied() {
            let next_table = self.next_table_mut().expect("table is full but no next table exists for shuffling");

            next_table.shuffle_in(FIRST_TABLE_INDEX, shuffled_item);

        // If the last possible index does not exist, no shuffling occurs, and we simply increment our length.
        } else {
            self.len += 1;
        }

        trace!("copying {:?} to {}", index..old_len, index + 1);
        self.table.copy_within(index..old_len, index + 1); // Shuffle all the elements up from the index.
        self.table_mut()[index] = item;
    }

    pub fn insert(&mut self, new_region: Region) {
        trace!("Attempting to insert item: {:?}", new_region);

        match self.table.binary_search(&new_region) {
            Ok(_) => panic!("attempted to insert region that already exists"),

            Err(insert_index) if insert_index > LAST_TABLE_INDEX => {
                trace!("Item does not belong in table; trying next...");

                if self.next_table().is_none() {
                    self.allocate_next_table();
                }

                self.next_table_mut().unwrap().insert(new_region);
            }

            Err(insert_index) => {
                trace!("Found insertion index: {}", insert_index);

                let before_index = insert_index.checked_sub(1).unwrap_or(usize::MAX);
                let after_index = insert_index.checked_add(1).unwrap_or(usize::MAX);

                let collapse_before = self
                    .table_mut()
                    .get_mut(before_index)
                    .map(|before_item| {
                        new_region.kind() == before_item.kind() && new_region.start() == before_item.end()
                    })
                    .unwrap_or(false);

                let collapse_after = self
                    .table_mut()
                    .get_mut(after_index)
                    .map(|after_item| new_region.kind() == after_item.kind() && new_region.end() == after_item.start())
                    .unwrap_or(false);

                match (collapse_before, collapse_after) {
                    (true, false) => {
                        trace!("Before region is collapsable.");

                        let region = &mut self.table()[insert_index];
                        *region = Region::new(region.kind(), region.start(), region.size() + new_region.size());
                    }

                    (false, true) => {
                        trace!("After region is collapsable.");

                        let after_region = &mut self.table_mut()[after_index];
                        *after_region = Region::new(
                            after_region.kind(),
                            new_region.start(),
                            after_region.size() + new_region.size(),
                        );
                    }

                    (true, true) => {
                        trace!("Before and after regions are collapsable.");

                        let before_region = &mut self.table_mut()[before_index];
                        *before_region = Region::new(
                            before_region.kind(),
                            before_region.start(),
                            before_region.size() + new_region.size() + self.table()[after_index].size(),
                        );
                        self.shuffle_out(after_index);
                    }

                    (false, false) => {
                        trace!("Cannot collapse any regions. Inserting instead.");

                        self.shuffle_in(insert_index, new_region);
                    }
                }
            }
        }
    }

    fn exsert(&mut self, layout: Layout) -> Result<Range<usize>, ()> {
        fn align_region_to_layout(region: &Region, layout: Layout) -> Option<Region> {}

        let layout = layout.pad_to_align();
        let find_region =
            self.table().iter().copied().enumerate().filter(|(_, region)| region.kind() == Kind::Generic).find_map(
                |(index, region)| {
                    let aligned_start =
                        libsys::align_up(region.start(), NonZeroU32::new(layout.align().trailing_zeros()).unwrap());
                    let aligned_size = region.size() - aligned_start;
                    let region_aligned = Region::new(region.kind(), aligned_start, aligned_size);

                    (region_aligned.size() >= layout.size()).then_some((index, region, region_aligned))
                },
            );

        if let Some((index, region, region_aligned)) = find_region {
            // If there's padding, we need to add a region in to cover that.
            if region_aligned.start() > region.start() {
                self.shuffle_in(
                    index,
                    Region::new(region.kind(), region.start(), region_aligned.start() - region.start()),
                );
            }

            // Now we simply push up the current region's bounds based on the layout size.
            self.table_mut()[index] = Region::new(
                region_aligned.kind(),
                region_aligned.start() + layout.size(),
                region_aligned.size() - layout.size(),
            );
        } else if let Some(next_table) = self.next_table_mut() {
            next_table.exsert(layout)
        } else {
            todo!("no available regions for allocation: {:X?}, need to allow reducing memory pressure", layout)
        }
    }

    fn allocate_next_table(&mut self) {
        todo!("allocate next table")
    }
}

impl fmt::Debug for PhysicalMemoryManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Region Table").field(&self.table()).field(&self.next_table()).finish()
    }
}

// Safety: Trust me.
unsafe impl Allocator for PhysicalMemoryManager {
    fn allocate(&self, layout: core::alloc::Layout) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        let layout = layout.align_to(libsys::page_size()).unwrap();
        self.exsert(layout)
    }

    unsafe fn deallocate(&self, ptr: core::ptr::NonNull<u8>, layout: core::alloc::Layout) {
        let layout = layout.align_to(libsys::page_size()).unwrap();
        let region = Region::new(Kind::Generic, ptr.addr().get(), layout.size());
        self.insert(Region::from_ptr_layout(ptr, layout));
    }
}
