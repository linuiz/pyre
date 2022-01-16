use crate::{
    addr_ty::{Physical, Virtual},
    cell::SyncRefCell,
    memory::{
        paging::{AttributeModify, PageAttributes},
        Page,
    },
    Address,
};
use alloc::{boxed::Box, vec::Vec};
use core::{
    alloc::Layout,
    mem::{align_of, MaybeUninit},
};

#[derive(Debug, Clone, Copy)]
pub enum AllocError {
    OutOfMemory,
    InvalidAlignment(usize),
    IdentityMappingOverlaps,
    FallocError(crate::memory::falloc::FallocError),
}

pub struct Alloc<T> {
    ptr: *mut T,
    len: usize,
}

impl<T> Alloc<T> {
    pub const unsafe fn new(ptr: *mut T, len: usize) -> Self {
        Self { ptr, len }
    }

    pub const fn cast<U>(self) -> Result<Alloc<U>, Self> {
        if self.ptr.align_offset(align_of::<U>()) == 0 {
            Ok(Alloc::<U> {
                ptr: self.ptr as *mut U,
                len: self.len / core::mem::size_of::<U>(),
            })
        } else {
            Err(self)
        }
    }

    pub fn into_uninit_value(self) -> Result<Box<MaybeUninit<T>>, Self> {
        unsafe {
            // TODO this seems odd for some reason, may need to have this work differently.
            if self.len == 1 {
                Ok(Box::from_raw(self.ptr as *mut _))
            } else {
                Err(self)
            }
        }
    }

    pub unsafe fn into_value(self) -> Result<Box<T>, Self> {
        // TODO this seems odd for some reason, may need to have this work differently.
        if self.len == 1 {
            Ok(Box::from_raw(self.ptr))
        } else {
            Err(self)
        }
    }

    pub fn into_uninit_vec(self) -> Vec<MaybeUninit<T>> {
        unsafe { Vec::from_raw_parts(self.ptr as *mut _, self.len, self.len) }
    }

    pub unsafe fn into_vec(self) -> Vec<T> {
        Vec::from_raw_parts(self.ptr, self.len, self.len)
    }

    pub fn into_uninit_slice(self) -> Box<[MaybeUninit<T>]> {
        unsafe {
            Box::from_raw(core::ptr::slice_from_raw_parts_mut(
                self.ptr as *mut _,
                self.len,
            ))
        }
    }

    pub unsafe fn into_slice(self) -> Box<[T]> {
        Box::from_raw(core::ptr::slice_from_raw_parts_mut(self.ptr, self.len))
    }

    pub const fn into_parts(self) -> (*mut T, usize) {
        (self.ptr, self.len)
    }
}

impl<T: Default + Clone> Alloc<T> {
    pub fn clear(&mut self) {
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }.fill(T::default())
    }
}

impl<T> core::fmt::Debug for Alloc<T> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_tuple("Alloc")
            .field(&self.ptr)
            .field(&self.len)
            .finish()
    }
}

pub trait MemoryAllocator {
    // Returns the direct-mapped virtual address for the given physical address.
    fn physical_memory(&self, addr: Address<Physical>) -> Address<Virtual>;

    unsafe fn alloc(
        &self,
        size: usize,
        align: Option<core::num::NonZeroUsize>,
    ) -> Result<Alloc<u8>, AllocError>;

    unsafe fn alloc_contiguous(
        &self,
        count: usize,
    ) -> Result<(Address<Physical>, Alloc<u8>), AllocError>;

    unsafe fn alloc_against(
        &self,
        frame_index: usize,
        count: usize,
    ) -> Result<Alloc<u8>, AllocError>;

    /// Attempts to allocate a 1:1 mapping of virtual memory to its physical memory.
    ///
    /// REMARK:
    ///     This function is required only to offer the same guarantees as `VirtualAddressor::identity_map()`.
    unsafe fn alloc_identity(
        &self,
        frame_index: usize,
        count: usize,
    ) -> Result<Alloc<u8>, AllocError>;

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout);

    // Returns the page state of the given page index.
    // Option is whether it is mapped
    // `bool` is whether it is allocated to
    fn get_page_state(&self, page_index: usize) -> Option<bool>;

    fn get_page_attribs(&self, page: &Page) -> Option<PageAttributes>;
    unsafe fn set_page_attribs(
        &self,
        page: &Page,
        attribs: PageAttributes,
        modify_mode: AttributeModify,
    );
}

static DEFAULT_MALLOCATOR: SyncRefCell<&'static dyn MemoryAllocator> = SyncRefCell::empty();

pub unsafe fn set(allocator: &'static dyn MemoryAllocator) {
    DEFAULT_MALLOCATOR.set(allocator);
}

pub fn try_get() -> Option<&'static dyn MemoryAllocator> {
    DEFAULT_MALLOCATOR.borrow().map(|a| *a)
}
