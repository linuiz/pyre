use core::{alloc::Layout, mem::size_of, num::NonZeroUsize};
use libstd::{
    addr_ty::{Physical, Virtual},
    align_up_div,
    memory::{
        falloc::{self, FrameType},
        malloc::{Alloc, AllocError, MemoryAllocator},
        paging::VirtualAddressor,
        Page,
    },
    Address,
};
use spin::{RwLock, RwLockWriteGuard};

/// Represents one page worth of memory blocks (i.e. 4096 bytes in blocks).
#[repr(transparent)]
#[derive(Clone)]
struct BlockPage(u64);

impl BlockPage {
    /// How many bits/block indexes in section primitive.
    const BLOCKS_PER: usize = size_of::<u64>() * 8;

    /// Whether the block page is empty.
    pub const fn is_empty(&self) -> bool {
        self.0 == u64::MIN
    }

    /// Whether the block page is full.
    pub const fn is_full(&self) -> bool {
        self.0 == u64::MAX
    }

    /// Unset all of the block page's blocks.
    pub const fn set_empty(&mut self) {
        self.0 = u64::MIN;
    }

    /// Set all of the block page's blocks.
    pub const fn set_full(&mut self) {
        self.0 = u64::MAX;
    }

    pub const fn value(&self) -> &u64 {
        &self.0
    }

    pub const fn value_mut(&mut self) -> &mut u64 {
        &mut self.0
    }
}

impl core::fmt::Debug for BlockPage {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_tuple("BlockPage")
            .field(&format_args!("0b{:b}", self.0))
            .finish()
    }
}

pub struct AllocatorMap<'map> {
    addressor: VirtualAddressor,
    pages: &'map mut [BlockPage],
}

/// Allocator utilizing blocks of memory, in size of 16 bytes per block, to
///  easily and efficiently allocate.
pub struct BlockAllocator<'map> {
    map: RwLock<AllocatorMap<'map>>,
}

impl<'map> BlockAllocator<'map> {
    /// The size of an allocator block.
    pub const BLOCK_SIZE: usize = 0x1000 / BlockPage::BLOCKS_PER;

    // TODO possibly move the initialization code from `init()` into this `new()` function.
    #[allow(const_item_mutation)]
    pub fn new() -> Self {
        const EMPTY: [BlockPage; 0] = [];

        let block_malloc = Self {
            // TODO make addressor use a RwLock
            map: RwLock::new(AllocatorMap {
                addressor: VirtualAddressor::null(),
                pages: &mut EMPTY,
            }),
        };

        {
            let mut map_write = block_malloc.map.write();

            unsafe {
                debug!("Initializing allocator's virtual addressor...");
                map_write.addressor = VirtualAddressor::new(Page::null());

                // TODO the addressors shouldn't mmap all reserved frames by default.
                //  It is, for insatnce, useless in userland addressors, where ACPI tables
                //  don't need to be mapped.
                debug!("Identity mapping all reserved global memory frames...");
                falloc::get()
                    .iter()
                    .enumerate()
                    .filter(|(_, (ty, _, _))| ty.eq(&falloc::FrameType::Reserved))
                    .for_each(|(index, _)| {
                        map_write
                            .addressor
                            .identity_map(&Page::from_index(index))
                            .unwrap();
                    });
            }

            // Since we're using physical offset mapping for our page table modification
            //  strategy, the memory needs to be identity mapped at the correct offset.
            let phys_mapping_addr = falloc::virtual_map_offset();
            debug!("Mapping physical memory at offset: {:?}", phys_mapping_addr);
            unsafe {
                map_write
                    .addressor
                    .modify_mapped_page(Page::from_addr(phys_mapping_addr));
            }

            info!("Writing kernel addressor's PML4 to the CR3 register.");
            unsafe { map_write.addressor.swap_into() };

            debug!("Allocating reserved physical memory frames...");
            falloc::get()
                .iter()
                .enumerate()
                .filter(|(_, (ty, _, _))| ty.eq(&FrameType::Reserved))
                .for_each(|(index, _)| {
                    while map_write.pages.len() <= index {
                        block_malloc
                            .grow(
                                usize::max(index - map_write.pages.len(), 1)
                                    * BlockPage::BLOCKS_PER,
                                &mut map_write,
                            )
                            .unwrap();
                    }

                    map_write.pages[index].set_full();
                });

            map_write.pages[0].set_full();

            info!("Finished block allocator initialization.");
        }

        block_malloc
    }

    /// Calculates the bit count and mask for a given set of block page parameters.
    fn calculate_bit_fields(
        map_index: usize,
        cur_block_index: usize,
        end_block_index: usize,
    ) -> (usize, u64) {
        let floor_blocks_index = map_index * BlockPage::BLOCKS_PER;
        let ceil_blocks_index = floor_blocks_index + BlockPage::BLOCKS_PER;
        let mask_bit_offset = cur_block_index - floor_blocks_index;
        let mask_bit_count = usize::min(ceil_blocks_index, end_block_index) - cur_block_index;

        (
            mask_bit_count,
            libstd::U64_BIT_MASKS[mask_bit_count - 1] << mask_bit_offset,
        )
    }

    pub fn grow(
        &self,
        required_blocks: usize,
        map_write: &mut RwLockWriteGuard<AllocatorMap>,
    ) -> Result<(), AllocError> {
        assert!(
            map_write.addressor.is_swapped_in(),
            "Cannot modify allocator state when addressor is not active."
        );
        assert!(required_blocks > 0, "calls to grow must be nonzero");

        trace!(
            "Allocator map requires growth: {} blocks required.",
            required_blocks
        );

        // Current length of our map, in indexes.
        let cur_map_len = map_write.pages.len();
        // Required length of our map, in indexes.
        let req_map_len = (map_write.pages.len()
            + libstd::align_up_div(required_blocks, BlockPage::BLOCKS_PER))
        .next_power_of_two();
        // Current page count of our map (i.e. how many pages the slice requires)
        let cur_map_pages = libstd::align_up_div(cur_map_len * size_of::<BlockPage>(), 0x1000);
        // Required page count of our map.
        let req_map_pages = libstd::align_up_div(req_map_len * size_of::<BlockPage>(), 0x1000);

        trace!(
            "Growth parameters: len {} => {}, pages {} => {}",
            cur_map_len,
            req_map_len,
            cur_map_pages,
            req_map_pages
        );

        // Attempt to find a run of already-mapped pages within our allocator
        // that can contain our required slice length.
        let mut current_run = 0;
        let start_index = core::lazy::OnceCell::new();
        for (index, block_page) in map_write.pages.iter().enumerate() {
            if block_page.is_empty() {
                current_run += 1;

                if current_run == req_map_pages {
                    start_index.set(index - (current_run - 1)).unwrap();
                    break;
                }
            } else {
                current_run = 0;
            }
        }

        let cur_map_page = Page::from_index((map_write.pages.as_ptr() as usize) / 0x1000);
        let new_map_page = Page::from_index(*start_index.get_or_init(|| {
            // When the map is zero-sized, this allows us to skip the first page in our
            // allocations (in order to keep the 0th page as null & unmapped).
            if cur_map_len == 0 {
                cur_map_len + 1
            } else {
                cur_map_len
            }
        }));

        trace!("Copy mapping current map to new pages.");
        for page_offset in 0..cur_map_pages {
            map_write
                .addressor
                .copy_by_map(
                    &cur_map_page.forward(page_offset).unwrap(),
                    &new_map_page.forward(page_offset).unwrap(),
                )
                .unwrap();
        }

        trace!("Allocating and mapping remaining pages of map.");
        for page_offset in cur_map_pages..req_map_pages {
            let mut new_page = new_map_page.forward(page_offset).unwrap();

            map_write.addressor.automap(&new_page);
            // Clear the newly allocated map page.
            unsafe { new_page.mem_clear() };
        }

        // Point to new map.
        map_write.pages = unsafe {
            core::slice::from_raw_parts_mut(
                new_map_page.as_mut_ptr(),
                libstd::align_up(req_map_len, 0x1000 / size_of::<BlockPage>()),
            )
        };

        map_write
            .pages
            .iter_mut()
            .skip(new_map_page.index())
            .take(req_map_pages)
            .for_each(|block_page| block_page.set_full());
        map_write
            .pages
            .iter_mut()
            .skip(cur_map_page.index())
            .take(cur_map_pages)
            .for_each(|block_page| block_page.set_empty());

        Ok(())
    }
}

impl MemoryAllocator for BlockAllocator<'_> {
    unsafe fn alloc(
        &self,
        size: usize,
        align: Option<NonZeroUsize>,
    ) -> Result<Alloc<u8>, AllocError> {
        let align = align.unwrap_or(NonZeroUsize::new_unchecked(1)).get();
        if !align.is_power_of_two() {
            return Err(AllocError::InvalidAlignment);
        }

        let align_shift = usize::max(align / Self::BLOCK_SIZE, 1);
        let size_in_blocks = libstd::align_up_div(size, Self::BLOCK_SIZE);
        let mut map_write = self.map.write();

        let end_map_index;
        let mut block_index;
        let mut current_run;

        'outer: loop {
            block_index = 0;
            current_run = 0;

            for (map_index, block_page) in map_write.pages.iter().enumerate() {
                if block_page.is_full() {
                    current_run = 0;
                    block_index += BlockPage::BLOCKS_PER;
                } else {
                    for bit_shift in 0..BlockPage::BLOCKS_PER {
                        if (block_page.value() & (1 << bit_shift)) > 0 {
                            current_run = 0;
                        } else if current_run > 0 || (bit_shift % align_shift) == 0 {
                            current_run += 1;

                            if current_run == size_in_blocks {
                                end_map_index = map_index + 1;
                                break 'outer;
                            }
                        }

                        block_index += 1;
                    }
                }
            }

            if let Err(alloc_err) = self.grow(size_in_blocks, &mut map_write) {
                return Err(alloc_err);
            }
        }

        let end_block_index = block_index + 1;
        block_index -= current_run - 1;
        let start_block_index = block_index;
        let start_map_index = start_block_index / BlockPage::BLOCKS_PER;

        for map_index in start_map_index..end_map_index {
            let block_page = &mut map_write.pages[map_index];
            let was_empty = block_page.is_empty();

            let block_index_floor = map_index * BlockPage::BLOCKS_PER;
            let low_offset = block_index - block_index_floor;
            let remaining_blocks_in_slice = usize::min(
                end_block_index - block_index,
                (block_index_floor + BlockPage::BLOCKS_PER) - block_index,
            );

            let mask_bits = libstd::U64_BIT_MASKS[remaining_blocks_in_slice];

            *block_page.value_mut() |= mask_bits << low_offset;
            block_index += remaining_blocks_in_slice;

            if was_empty {
                map_write.addressor.automap(&Page::from_index(map_index));
            }
        }

        Ok(Alloc::new(
            (start_block_index * Self::BLOCK_SIZE) as *mut _,
            size_in_blocks * Self::BLOCK_SIZE,
        ))
    }

    unsafe fn alloc_contiguous(
        &self,
        count: usize,
    ) -> Result<(Address<Physical>, Alloc<u8>), AllocError> {
        let mut map_write = self.map.write();
        let frame_index = match falloc::get().lock_next_many(count) {
            Ok(frame_index) => frame_index,
            Err(falloc_err) => {
                return Err(AllocError::FallocError(falloc_err));
            }
        };

        let mut start_index = 0;
        'outer: loop {
            let mut current_run = 0;

            for (map_index, block_page) in map_write.pages.iter_mut().enumerate().skip(start_index)
            {
                if !block_page.is_empty() {
                    current_run = 0;
                    start_index = map_index + 1;
                } else {
                    current_run += 1;

                    if current_run == count {
                        break 'outer;
                    }
                }
            }

            if let Err(alloc_err) = self.grow(count * BlockPage::BLOCKS_PER, &mut map_write) {
                return Err(alloc_err);
            }
        }

        for offset in 0..count {
            let page_index = start_index + offset;
            let frame_index = frame_index + offset;

            map_write.pages[page_index].set_full();
            map_write
                .addressor
                .map(&Page::from_index(page_index), frame_index, None)
                .unwrap();
        }

        Ok((
            Address::<Physical>::new(frame_index * 0x1000),
            Alloc::new((start_index * 0x1000) as *mut _, count * 0x1000),
        ))
    }

    unsafe fn alloc_against(
        &self,
        frame_index: usize,
        count: usize,
    ) -> Result<Alloc<u8>, AllocError> {
        let mut map_write = self.map.write();
        let mut start_index = 0;
        'outer: loop {
            let mut current_run = 0;

            for (map_index, block_page) in map_write.pages.iter_mut().enumerate().skip(start_index)
            {
                if !block_page.is_empty() {
                    current_run = 0;
                    start_index = map_index + 1;
                } else {
                    current_run += 1;

                    if current_run == count {
                        break 'outer;
                    }
                }
            }

            if let Err(alloc_err) = self.grow(count * BlockPage::BLOCKS_PER, &mut map_write) {
                return Err(alloc_err);
            }
        }

        for offset in 0..count {
            let page_index = start_index + offset;
            let frame_index = frame_index + offset;

            map_write.pages[page_index].set_full();
            map_write
                .addressor
                .map(&Page::from_index(page_index), frame_index, None)
                .unwrap();
        }

        Ok(Alloc::new((start_index * 0x1000) as *mut _, count * 0x1000))
    }

    unsafe fn alloc_identity(
        &self,
        frame_index: usize,
        count: usize,
    ) -> Result<Alloc<u8>, AllocError> {
        let mut map_write = self.map.write();

        if map_write.pages.len() < (frame_index + count) {
            self.grow(
                (frame_index + count) * BlockPage::BLOCKS_PER,
                &mut map_write,
            )
            .unwrap();
        }

        for page_index in frame_index..(frame_index + count) {
            if map_write.pages[page_index].is_empty() {
                map_write.pages[page_index].set_full();
                map_write
                    .addressor
                    .identity_map(&Page::from_index(page_index))
                    .unwrap();
            } else {
                for page_index in frame_index..page_index {
                    map_write.pages[page_index].set_empty();
                    map_write
                        .addressor
                        .unmap(&Page::from_index(page_index), false)
                        .unwrap();
                }

                return Err(AllocError::IdentityMappingOverlaps);
            }
        }

        Ok(Alloc::new((frame_index * 0x1000) as *mut _, count * 0x1000))
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let start_block_index = (ptr as usize) / Self::BLOCK_SIZE;
        let end_block_index = start_block_index + align_up_div(layout.size(), Self::BLOCK_SIZE);
        let mut block_index = start_block_index;
        trace!(
            "Deallocation requested: {}..{}",
            start_block_index,
            end_block_index
        );

        let start_map_index = start_block_index / BlockPage::BLOCKS_PER;
        let end_map_index = align_up_div(end_block_index, BlockPage::BLOCKS_PER);
        let mut map = self.map.write();
        for map_index in start_map_index..end_map_index {
            let (had_bits, has_bits) = {
                let block_page = &mut map.pages[map_index];

                let had_bits = !block_page.is_empty();

                let (bit_count, bit_mask) =
                    Self::calculate_bit_fields(map_index, block_index, end_block_index);
                assert_eq!(
                    *block_page.value() & bit_mask,
                    bit_mask,
                    "attempting to deallocate blocks that are already deallocated"
                );

                *block_page.value_mut() ^= bit_mask;
                block_index += bit_count;

                (had_bits, !block_page.is_empty())
            };

            if had_bits && !has_bits {
                // TODO we actually *don't know* if this page locked a frame or not...
                map.addressor
                    .unmap(&Page::from_index(map_index), true)
                    .unwrap();
            }
        }
    }

    fn get_page_attribs(&self, page: &Page) -> Option<libstd::memory::paging::PageAttributes> {
        self.map.read().addressor.get_page_attribs(page)
    }

    unsafe fn set_page_attribs(
        &self,
        page: &Page,
        attributes: libstd::memory::paging::PageAttributes,
        modify_mode: libstd::memory::paging::AttributeModify,
    ) {
        self.map
            .write()
            .addressor
            .set_page_attribs(page, attributes, modify_mode)
    }

    fn get_page_state(&self, page_index: usize) -> Option<bool> {
        self.map
            .read()
            .pages
            .get(page_index)
            .map(|block_page| !block_page.is_empty())
    }

    fn physical_memory(&self, addr: Address<Physical>) -> Address<Virtual> {
        self.map.read().addressor.mapped_offset() + addr.as_usize()
    }
}
