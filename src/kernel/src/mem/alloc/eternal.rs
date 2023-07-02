use core::{alloc::Allocator, num::NonZeroU32, ptr::NonNull, sync::atomic::AtomicUsize};

#[link_section = ".bss"]
static MEMORY: [u8; 0x8000] = [0u8; 0x8000];
static OFFSET: AtomicUsize = AtomicUsize::new(0);

#[allow(clippy::module_name_repetitions)]
pub struct EternalAllocator;

// Safety: We'll see :)
unsafe impl Allocator for EternalAllocator {
    fn allocate(&self, layout: core::alloc::Layout) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        assert!(layout.align() <= 32, "eternal allocator cannot serve object with alignments greater than 32");

        let size = layout.pad_to_align().size();
        let offset = OFFSET.fetch_add(size, core::sync::atomic::Ordering::Relaxed);

        assert!(offset < MEMORY.len(), "eternal allocate is full");

        let aligned_offset = libsys::align_up(offset, NonZeroU32::new(layout.align().trailing_zeros()).unwrap());
        let allocated_memory = MEMORY.get(aligned_offset..layout.size()).unwrap();

        #[allow(clippy::ptr_cast_constness)]
        Ok(NonNull::new(allocated_memory as *const [u8] as *mut [u8]).unwrap())
    }

    unsafe fn deallocate(&self, _: core::ptr::NonNull<u8>, _: core::alloc::Layout) {
        unimplemented!("eternal allocator cannot deallocate")
    }
}
