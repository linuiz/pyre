use crate::{
    memory::io::pci::{DeviceType, Standard, BAR},
    num::LittleEndianU32,
};
use bit_field::BitField;
use core::fmt;
use libsys::{memory::VolatileCell, ReadWrite};

// #[repr(C)]
// pub struct MessageControl {
//     reg0: VolatileCell<u32, ReadWrite>,
// }

// impl MessageControl {
//     pub fn get_table_len(&self) -> usize {
//         self.reg0.read().get_bits(16..27) as usize
//     }

//     volatile_bitfield_getter!(reg0, force_mask, 30);
//     volatile_bitfield_getter!(reg0, enable, 31);
// }

// impl fmt::Debug for MessageControl {
//     fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
//         formatter
//             .debug_struct("Message Control")
//             .field("Enabled", &self.get_enable())
//             .field("Force Mask", &self.get_force_mask())
//             .field("Table Size", &self.get_table_len())
//             .finish()
//     }
// }

#[repr(C)]
pub struct Message {
    addr_low: VolatileCell<u32, ReadWrite>,
    addr_high: VolatileCell<u32, ReadWrite>,
    data: VolatileCell<u32, ReadWrite>,
    vector_control: VolatileCell<u32, ReadWrite>,
}

impl Message {
    pub fn get_masked(&self) -> bool {
        self.vector_control.read().get_bit(0)
    }
    pub fn set_masked(&self, masked: bool) {
        self.vector_control.write(
            // Modify the existing bits, as the MSI-X spec requires the
            // the reserved bits be preserved for compatibility.
            *self.vector_control.read().set_bit(0, masked),
        );
    }
    // TODO features gate this function behind x86, because its contents are arch-specific
    pub fn configure(&self, processor_id: u32, vector: u8, delivery_mode: crate::interrupts::InterruptDeliveryMode) {
        assert!(self.get_masked(), "cannot modify MSI-X message when it is unmasked");
        assert!(vector >= 0x20);
        let mut data = 0;
        data.set_bits(0..8, vector as u32);
        data.set_bits(8..11, delivery_mode as u32);
        data.set_bit(14, false);
        data.set_bit(15, false);
        data.set_bits(16..32, 0);
        self.data.write(data);

        // MSI-X spec requires the low 2 bits be zeroed, for proper DWORD alignment.
        let addr =
            ((crate::arch::x86_64::structures::apic::xAPIC_BASE_ADDR as u64) + ((processor_id << 12) as u64)) & !0b11;
        self.addr_low.write(addr as u32);
        // High address is reserved (zeroed) in x86.
        self.addr_high.write((addr >> 32) as u32);
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Message Table Entry")
            .field("Masked", &self.get_masked())
            .field(
                "Address",
                &format_args!("0x{:X}", ((self.addr_high.read() as u64) << 32) | (self.addr_low.read() as u64)),
            )
            .field("Data", &format_args!("0b{:b}", self.data.read()))
            .finish()
    }
}

// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
// pub enum PendingBit {
//     Unset,
//     Set,
// }

// impl From<usize> for PendingBit {
//     fn from(value: usize) -> Self {
//         match value {
//             0 => Self::Unset,
//             1 => Self::Set,
//             value => panic!("Invalid pending bit value: {}", value),
//         }
//     }
// }

// impl Into<usize> for PendingBit {
//     fn into(self) -> usize {
//         match self {
//             PendingBit::Unset => 0,
//             PendingBit::Set => 1,
//         }
//     }
// }

// impl crate::collections::bv_array::BitValue for PendingBit {
//     const BIT_WIDTH: usize = 0x1;
//     const MASK: usize = 0x1;
// }

// #[repr(C)]
// struct Data {
//     reg0: VolatileCell<u32, ReadWrite>,
//     reg1: VolatileCell<u32, ReadOnly>,
//     reg2: VolatileCell<u32, ReadOnly>,
// }

// impl Data {
//     pub fn get_enable(&self) -> bool {
//         self.reg0.read().get_bit(31)
//     }

//     pub fn set_enable(&self, enable: bool) {
//         self.reg0.write(*self.reg0.read().set_bit(31, enable));
//     }

//     pub fn get_function_mask(&self) -> bool {
//         self.reg0.read().get_bit(30)
//     }

//     pub fn set_function_mask(&self, mask_all: bool) {
//         self.reg0.write(*self.reg0.read().set_bit(30, mask_all));
//     }

//     fn get_table_len(&self) -> usize {
//         // Field is encoded as N-1, so add one to get N (table length).
//         (self.reg0.read().get_bits(16..27) as usize) + 1
//     }

//     fn get_table_info(&self) -> (StandardRegister, usize) {
//         let reg1 = self.reg1.read();

//         (StandardRegister::try_from(reg1.get_bits(0..3) as usize).unwrap(), (reg1 & !0b111) as usize)
//     }

//     fn get_pending_info(&self) -> (StandardRegister, usize) {
//         let reg2 = self.reg2.read();

//         (StandardRegister::try_from(reg2.get_bits(0..3) as usize).unwrap(), (reg2 & !0b111) as usize)
//     }
// }

pub struct MSIX<'dev> {
    base_ptr: *mut LittleEndianU32,
    messages: &'dev [Message],
}

impl super::Capability for MSIX<'_> {
    const TYPE_CODE: u8 = 0x11;
    const BARS_USED: [bool; Standard::REGISTER_COUNT] = [false, true, true, false, false, false];

    unsafe fn from_base_ptr(capability_base_ptr: *mut LittleEndianU32, bars: [Option<BAR>; 6]) -> Self {
        use libsys::memory::Page;

        let bar1 = bars[1].expect("MSI-X capability utilizes BAR 1");
        // TODO support pending bits BAR
        // let bar2 = bars[2].expect("MSI-X capability utilizes BAR 2");

        /* BAR 1 */
        let messages = {
            let address = bar1.get_address();
            let size = bar1.get_size();

            assert!(
                address.is_aligned_to(core::num::NonZeroUsize::new(core::mem::size_of::<Message>()).unwrap()),
                "BAR address must be aligned"
            );
            assert_eq!(size & (core::mem::size_of::<Message>() - 1), 0, "BAR size must be aligned");

            // TODO maybe we shouldn't import kernel types? PCI may need to be moved back to libsys, for userspace compatibility.
            let frame_manager = crate::memory::get_kernel_frame_manager();
            let page_manager = crate::memory::get_kernel_page_manager();
            let hhdm_offset_address =
                libsys::Address::<libsys::Virtual>::new(crate::memory::get_hhdm_address().as_u64() + address.as_u64())
                    .unwrap();

            for size_offset in (0..size).step_by(0x1000) {
                page_manager
                    .map_mmio(
                        &Page::from_index((hhdm_offset_address.as_usize() + size_offset) / 0x1000),
                        address.frame_index() + (size / 0x1000),
                        frame_manager,
                    )
                    .unwrap();
            }

            core::slice::from_raw_parts(hhdm_offset_address.as_ptr(), size / core::mem::size_of::<Message>())
        };

        Self { base_ptr: capability_base_ptr, messages }
    }
}

impl<'dev> MSIX<'dev> {
    // pub fn new(device: &'dev PCIeDevice<Standard>, capability_base_ptr: *mut LittleEndianU32) -> Option<Self> {
    //     device.capabilities().find(|(_, capability)| *capability == super::Type::MSIX).map(|(addr, _)| {
    //         let data = unsafe { addr.as_ptr::<Data>().as_ref() }.unwrap();
    //         let (msg_register, msg_offset) = data.get_table_info();

    //         Self {
    //             data,
    //             messages: device
    //                 .get_register(msg_register)
    //                 .map(|mmio| unsafe { mmio.slice(msg_offset, data.get_table_len()).unwrap() })
    //                 .expect("Device does not have requisite BARs to construct MSIX capability."),
    //         }
    //     })
    // }

    fn get_table_size(&self) -> usize {
        // Safety: Type's constructor invariantly requires a valid base pointer.
        unsafe { self.base_ptr.read_volatile() }.get().get_bits(16..26) as usize
    }

    pub fn get_function_mask(&self) -> bool {
        // Safety: See `Self::get_table_size()`.
        unsafe { self.base_ptr.read_volatile() }.get().get_bit(30)
    }

    pub fn set_function_mask(&self, mask_all: bool) {
        // Safety: See `Self::get_table_size()`.
        unsafe {
            self.base_ptr
                .write_volatile(LittleEndianU32::new(*self.base_ptr.read_volatile().get().set_bit(30, mask_all)))
        };
    }

    pub fn get_enable(&self) -> bool {
        // Safety: See `Self::get_table_size()`.
        unsafe { self.base_ptr.read_volatile() }.get().get_bit(31)
    }

    pub fn set_enable(&self, enable: bool) {
        // Safety: See `Self::get_table_size()`.
        unsafe {
            self.base_ptr.write_volatile(LittleEndianU32::new(*self.base_ptr.read_volatile().get().set_bit(31, enable)))
        };
    }
}

impl fmt::Debug for MSIX<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MSI-X")
            .field("Enabled", &self.get_enable())
            .field("Function Mask", &self.get_function_mask())
            .field("Messages", &self.messages)
            .finish()
    }
}
