mod class;
pub use class::*;

pub mod pci2pci;
pub mod standard;

use crate::{Address, LittleEndian, LittleEndianU16, LittleEndianU32, LittleEndianU8, Physical};
use bit_field::BitField;
use core::{fmt, marker::PhantomData, mem::size_of, ptr::NonNull};

errorgen! {
    #[derive(Debug)]
    pub enum Error {
        InvalidKind { raw: u8 } => None,
        UnsupportedKind { raw: u8 } => None,
        InvalidBarSpace { value: u8 } => None,
        BarIndexOverflow { index: usize } => None
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct Command(u16);

// TODO impl command bits
// impl CommandRegister {
//     volatile_bitfield_getter_ro!(reg, io_space, 0);
//     volatile_bitfield_getter_ro!(reg, memory_space, 1);
//     volatile_bitfield_getter!(reg, bus_master, 2);
//     volatile_bitfield_getter_ro!(reg, special_cycle, 3);
//     volatile_bitfield_getter_ro!(reg, memory_w_and_i, 4);
//     volatile_bitfield_getter_ro!(reg, vga_palette_snoop, 5);
//     volatile_bitfield_getter!(reg, parity_error, 6);
//     volatile_bitfield_getter_ro!(reg, idsel_stepwait_cycle_ctrl, 7);
//     volatile_bitfield_getter!(reg, serr_num, 8);
//     volatile_bitfield_getter_ro!(reg, fast_b2b_transactions, 9);
//     volatile_bitfield_getter!(reg, interrupt_disable, 10);
// }

// impl Volatile for CommandRegister {}

// impl fmt::Debug for CommandRegister {
//     fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
//         formatter
//             .debug_struct("Command Register")
//             .field("IO Space", &self.get_io_space())
//             .field("Memory Space", &self.get_memory_space())
//             .field("Bus Master", &self.get_bus_master())
//             .field("Special Cycle", &self.get_special_cycle())
//             .field("Memory Write & Invalidate", &self.get_memory_w_and_i())
//             .field("VGA Palette Snoop", &self.get_vga_palette_snoop())
//             .field("Parity Error", &self.get_parity_error())
//             .field("IDSEL Stepping/Wait Cycle Control", &self.get_idsel_stepwait_cycle_ctrl())
//             .field("SERR#", &self.get_serr_num())
//             .field("Fast Back-to-Back Transactions", &self.get_fast_b2b_transactions())
//             .field("Interrupt Disable", &self.get_interrupt_disable())
//             .finish()
//     }
// }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevselTiming {
    Fast,
    Medium,
    Slow,
}

bitflags::bitflags! {
    #[repr(transparent)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Status : u16 {
        const INTERRUPT_STATUS = 1 << 3;
        const CAPABILITIES = 1 << 4;
        /// * Not applicable to PCIe.
        const CAPABILITITY_66MHZ = 1 << 5;
        /// * Not applicable to PCIe.
        const FAST_BACK2BACK_CAPABLE = 1 << 7;
        const MASTER_DATA_PARITY_ERROR = 1 << 8;
        /// * Not applicable to PCIe.
        const DEVSEL_TIMING = 3 << 9;
        const SIGNALED_TARGET_ABORT = 1 << 11;
        const RECEIVED_TARGET_ABORT = 1 << 12;
        const RECEIVED_MASTER_ABORT =  1 << 13;
        const SIGNALED_SYSTEM_ERROR = 1 << 14;
        const DETECTED_PARITY_ERROR = 1 << 15;
    }
}

impl Status {
    pub fn devsel_timing(self) -> DevselTiming {
        match self.bits().get_bits(9..11) {
            0b00 => DevselTiming::Fast,
            0b01 => DevselTiming::Medium,
            0b10 => DevselTiming::Slow,

            _ => unreachable!(),
        }
    }
}

#[repr(usize)]
enum Offset {
    VendorId = 0x0,
    DeviceId = 0x2,
    Command = 0x4,
    Status = 0x6,
    RevisionId = 0x8,
    ProgramInterface = 0x9,
    Subclass = 0xA,
    Class = 0xB,
    CacheLineSize = 0xC,
    LatencyTimer = 0xD,
    HeaderType = 0xE,
    BuiltinSelfTest = 0xF,
}

impl From<Offset> for usize {
    fn from(value: Offset) -> Self {
        value as usize
    }
}

pub trait Kind {
    const REGISTER_COUNT: usize;
}

pub struct Standard;
impl Kind for Standard {
    const REGISTER_COUNT: usize = 6;
}

pub struct Pci2Pci;
impl Kind for Pci2Pci {
    const REGISTER_COUNT: usize = 2;
}

#[derive(Debug)]
pub enum DeviceKind {
    Standard(Device<Standard>),
    PCI2PCI(Device<Pci2Pci>),
}

pub struct Device<K: Kind>(NonNull<u8>, PhantomData<K>);

// Safety: PCI MMIO (and so, the pointers used for it) utilize the global HHDM, and so can be sent between threads.
unsafe impl<K: Kind> Send for Device<K> {}

/// ### Safety
///
/// Caller must ensure that the provided base pointer is a valid (and mapped) PCI MMIO header base.
pub unsafe fn new_device(ptr: NonNull<u8>) -> Result<DeviceKind> {
    let header_ty = unsafe { ptr.as_ptr().cast::<LittleEndianU8>().add(14).read_volatile() };

    match header_ty.get().get_bits(0..7) {
        0x0 => Ok(DeviceKind::Standard(Device::<Standard>(ptr, PhantomData))),
        0x1 => Ok(DeviceKind::PCI2PCI(Device(ptr, PhantomData))),
        0x2 => Err(Error::UnsupportedKind { raw: 0x2 }),
        raw => Err(Error::InvalidKind { raw }),
    }
}

impl<K: Kind> Device<K> {
    const ROW_SIZE: usize = core::mem::size_of::<LittleEndianU32>();

    unsafe fn read_offset<O: Into<usize>, U: LittleEndian>(&self, offset: O) -> U::NativeType {
        self.0.as_ptr().add(offset.into()).cast::<U>().read_volatile().get()
    }

    unsafe fn write_offset<O: Into<usize>, U: LittleEndian>(&mut self, offset: O, value: U::NativeType) {
        self.0.as_ptr().add(offset.into()).cast::<U>().write_volatile(U::from(value));
    }

    pub fn get_vendor_id(&self) -> Option<u16> {
        match unsafe { self.read_offset::<_, LittleEndianU16>(Offset::VendorId) } {
            0xFFFF => None,
            value => Some(value),
        }
    }

    pub fn get_device_id(&self) -> u16 {
        unsafe { self.read_offset::<_, LittleEndianU16>(Offset::DeviceId) }
    }

    pub fn get_command(&self) -> Command {
        Command(unsafe { self.read_offset::<_, LittleEndianU16>(Self::ROW_SIZE) })
    }

    pub fn set_command(&mut self, command: Command) {
        unsafe { self.write_offset::<_, LittleEndianU16>(Self::ROW_SIZE, command.0) }
    }

    pub fn get_status(&self) -> Status {
        Status::from_bits_retain(unsafe { self.read_offset::<_, LittleEndianU16>(Offset::Status) })
    }

    pub fn get_revision_id(&self) -> u8 {
        unsafe { self.read_offset::<_, LittleEndianU8>(Offset::RevisionId) }
    }

    pub fn get_class(&self) -> Class {
        let class = unsafe { self.read_offset::<_, LittleEndianU8>(Offset::Class) };
        let subclass = unsafe { self.read_offset::<_, LittleEndianU8>(Offset::Subclass) };
        let prog_if = unsafe { self.read_offset::<_, LittleEndianU8>(Offset::ProgramInterface) };

        Class::parse(class, subclass, prog_if)
    }

    pub fn get_cache_line_size(&self) -> u8 {
        unsafe { self.read_offset::<_, LittleEndianU8>(Offset::CacheLineSize) }
    }

    pub fn get_latency_timer(&self) -> u8 {
        unsafe { self.read_offset::<_, LittleEndianU8>(Offset::LatencyTimer) }
    }

    pub fn get_header_type(&self) -> u8 {
        unsafe { self.read_offset::<_, LittleEndianU8>(Offset::HeaderType) }.get_bits(0..7)
    }

    pub fn get_multi_function(&self) -> bool {
        unsafe { self.read_offset::<_, LittleEndianU8>(Offset::HeaderType) }.get_bit(7)
    }

    pub fn get_bar(&mut self, index: usize) -> Result<Bar> {
        if index >= K::REGISTER_COUNT {
            return Err(Error::BarIndexOverflow { index });
        }

        let bar_offset = 0x10 + (index * size_of::<LittleEndianU32>());
        let bar = unsafe { self.read_offset::<_, LittleEndianU32>(bar_offset) };

        if bar.get_bit(0) {
            Ok(Bar::IOSpace { address: bar & !0b11, size: 0 })
        } else {
            match bar.get_bits(1..3) {
                0b00 => {
                    // Safety: See above about PCI spec.
                    let size = unsafe {
                        self.write_offset::<_, LittleEndianU32>(bar_offset, u32::MAX);
                        let size = !(self.read_offset::<_, LittleEndianU32>(bar_offset) & !0xF) + 1;
                        self.write_offset::<_, LittleEndianU32>(bar_offset, bar);
                        size
                    };

                    Ok(Bar::MemorySpace32 {
                        address: Address::new(usize::try_from(bar).unwrap()).unwrap(),
                        size,
                        prefetch: bar.get_bit(3),
                    })
                }

                0b10 => {
                    let high_bar_offset = bar_offset + size_of::<LittleEndianU32>();
                    let high_bar = unsafe { self.read_offset::<_, LittleEndianU32>(high_bar_offset) };

                    // Safety: See above about PCI spec.
                    let size = unsafe {
                        self.write_offset::<_, LittleEndianU32>(bar_offset, u32::MAX);
                        self.write_offset::<_, LittleEndianU32>(high_bar_offset, u32::MAX);

                        let size_low = u64::from(self.read_offset::<_, LittleEndianU32>(bar_offset) & !0xF);
                        let size_high = u64::from(self.read_offset::<_, LittleEndianU32>(high_bar_offset));
                        let size = ((size_high << 32) | size_low) + 1;

                        self.write_offset::<_, LittleEndianU32>(bar_offset, bar);
                        self.write_offset::<_, LittleEndianU32>(high_bar_offset, high_bar);

                        size
                    };

                    let address = (u64::from(high_bar) << 32) | (u64::from(bar) & !0xF);

                    Ok(Bar::MemorySpace64 {
                        address: Address::new(address.try_into().unwrap()).unwrap(),
                        size,
                        prefetch: address.get_bit(3),
                    })
                }

                invalid_space => Err(Error::InvalidBarSpace { value: invalid_space.try_into().unwrap() }),
            }
        }
    }

    pub fn generic_debug_fmt(&self, debug_struct: &mut fmt::DebugStruct) {
        debug_struct
            .field("ID", &format_args!("{:4X?}:{:4X}", self.get_vendor_id(), self.get_device_id()))
            .field("Command", &format_args!("{:?}", self.get_command()))
            .field("Status", &self.get_status())
            .field("Revision ID", &self.get_revision_id())
            .field("Class", &format_args!("{:?}", self.get_class()))
            .field("Cache Line Size", &self.get_cache_line_size())
            .field("Master Latency Timer", &self.get_latency_timer())
            .field("Header Type", &self.get_header_type());
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Bar {
    MemorySpace32 { address: Address<Physical>, size: u32, prefetch: bool },
    MemorySpace64 { address: Address<Physical>, size: u64, prefetch: bool },
    IOSpace { address: u32, size: u32 },
}

#[allow(clippy::match_same_arms)]
impl Bar {
    pub fn is_unused(&self) -> bool {
        match self {
            Bar::MemorySpace32 { address, size: _, prefetch: _ } => address.get() == 0,
            Bar::MemorySpace64 { address, size: _, prefetch: _ } => address.get() == 0,
            Bar::IOSpace { address, size: _ } => address.get_bits(2..32) == 0,
        }
    }

    pub fn get_size(&self) -> usize {
        match self {
            Bar::MemorySpace32 { address: _, size, prefetch: _ } => usize::try_from(*size).unwrap(),
            Bar::MemorySpace64 { address: _, size, prefetch: _ } => usize::try_from(*size).unwrap(),
            Bar::IOSpace { address: _, size } => usize::try_from(*size).unwrap(),
        }
    }

    pub fn get_address(&self) -> Address<Physical> {
        match self {
            Bar::MemorySpace32 { address, size: _, prefetch: _ } => *address,
            Bar::MemorySpace64 { address, size: _, prefetch: _ } => *address,
            Bar::IOSpace { address, size: _ } => Address::new(usize::try_from(*address).unwrap()).unwrap(),
        }
    }
}

impl core::fmt::Debug for Device<Pci2Pci> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_tuple("Not Implemented").finish()
    }
}
