pub mod alloc;
pub mod io;
pub mod mapper;
pub mod paging;
pub mod vmm;

use self::mapper::Mapper;
use crate::interrupts::InterruptCell;
use core::ptr::NonNull;
use libsys::{table_index_size, Address, Frame, Page, Virtual};
use spin::{Lazy, Mutex};

#[repr(align(0x10))]
pub struct Stack<const SIZE: usize>([u8; SIZE]);

impl<const SIZE: usize> Stack<SIZE> {
    #[inline]
    pub const fn new() -> Self {
        Self([0u8; SIZE])
    }

    pub fn top(&self) -> NonNull<u8> {
        // Safety: Pointer is valid for the length of the slice.
        NonNull::new(unsafe { self.0.as_ptr().add(self.0.len()).cast_mut() }).unwrap()
    }
}

impl<const SIZE: usize> core::ops::Deref for Stack<SIZE> {
    type Target = [u8; SIZE];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub static HHDM: spin::Lazy<Hhdm> = spin::Lazy::new(|| {
    #[limine::limine_tag]
    static LIMINE_HHDM: limine::HhdmRequest = limine::HhdmRequest::new(crate::init::boot::LIMINE_REV);

    let hhdm_address = LIMINE_HHDM
        .get_response()
        .expect("bootloader provided no higher-half direct mapping")
        .offset()
        .try_into()
        .unwrap();

    debug!("HHDM address: {:X?}", hhdm_address);

    Hhdm(Address::<Page>::new(hhdm_address).unwrap())
});

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hhdm(Address<Page>);

impl Hhdm {
    #[inline]
    pub const fn page(self) -> Address<Page> {
        self.0
    }

    #[inline]
    pub fn address(self) -> Address<Virtual> {
        self.0.get()
    }

    #[inline]
    pub fn ptr(self) -> *mut u8 {
        self.address().as_ptr()
    }

    #[inline]
    pub fn offset(self, frame: Address<Frame>) -> Option<Address<Page>> {
        self.address().get().checked_add(frame.get().get()).and_then(Address::new)
    }
}

pub fn with_kmapper<T>(func: impl FnOnce(&mut Mapper) -> T) -> T {
    static KERNEL_MAPPER: Lazy<InterruptCell<Mutex<Mapper>>> = Lazy::new(|| {
        debug!("Creating kernel-space address mapper.");

        InterruptCell::new(Mutex::new(Mapper::new(paging::TableDepth::max()).unwrap()))
    });

    KERNEL_MAPPER.with(|mapper| {
        let mut mapper = mapper.lock();
        func(&mut mapper)
    })
}

pub fn copy_kernel_page_table() -> alloc::pmm::Result<Address<Frame>> {
    let table_frame = alloc::pmm::get().next_frame()?;

    // Safety: Frame is provided by allocator, and so guaranteed to be within the HHDM, and is frame-sized.
    let new_table = unsafe {
        core::slice::from_raw_parts_mut(
            HHDM.offset(table_frame).unwrap().as_ptr().cast::<paging::PageTableEntry>(),
            table_index_size(),
        )
    };
    new_table.fill(paging::PageTableEntry::empty());
    with_kmapper(|kmapper| new_table.copy_from_slice(kmapper.view_page_table()));

    Ok(table_frame)
}

#[cfg(target_arch = "x86_64")]
pub struct PagingRegister(pub Address<Frame>, pub crate::arch::x86_64::registers::control::CR3Flags);
#[cfg(target_arch = "riscv64")]
pub struct PagingRegister(pub Address<Frame>, pub u16, pub crate::arch::rv64::registers::satp::Mode);

impl PagingRegister {
    pub fn read() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            let args = crate::arch::x86_64::registers::control::CR3::read();
            Self(args.0, args.1)
        }

        #[cfg(target_arch = "riscv64")]
        {
            let args = crate::arch::rv64::registers::satp::read();
            Self(args.0, args.1, args.2)
        }
    }

    /// Safety
    ///
    /// Writing to this register has the chance to externally invalidate memory references.
    pub unsafe fn write(args: &Self) {
        #[cfg(target_arch = "x86_64")]
        crate::arch::x86_64::registers::control::CR3::write(args.0, args.1);

        #[cfg(target_arch = "riscv64")]
        crate::arch::rv64::registers::satp::write(args.0.as_usize(), args.1, args.2);
    }

    #[inline]
    pub const fn frame(&self) -> Address<Frame> {
        self.0
    }
}
