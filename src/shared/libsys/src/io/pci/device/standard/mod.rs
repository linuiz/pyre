use super::{Device, Standard};
use crate::{LittleEndianU16, LittleEndianU32, LittleEndianU8};

#[repr(usize)]
enum Offset {
    CardbusCisPtr = 0x28,
    SubsystemVendorId = 0x2C,
    SubsystemId = 0x2E,
    ExpansionRomBaseAddress = 0x30,
    CapabilitiesPtr = 0x34,
    InterruptLine = 0x3C,
    InterruptPin = 0x3D,
    MinGrant = 0x3E,
    MaxLatency = 0x3F,
}

impl From<Offset> for usize {
    fn from(value: Offset) -> Self {
        value as Self
    }
}

impl Device<Standard> {
    pub fn get_cardbus_cis_ptr(&self) -> usize {
        let value = unsafe { self.read_offset::<_, LittleEndianU32>(Offset::CardbusCisPtr) };
        debug_assert_eq!(value, 0, "PCIe hardwired to 0");

        value.try_into().unwrap()
    }

    pub fn get_subsystem_vendor_id(&self) -> u16 {
        unsafe { self.read_offset::<_, LittleEndianU16>(Offset::SubsystemVendorId) }
    }

    pub fn get_subsystem_id(&self) -> u16 {
        unsafe { self.read_offset::<_, LittleEndianU16>(Offset::SubsystemId) }
    }

    pub fn get_expansion_rom_base_addr(&self) -> Option<usize> {
        match unsafe { self.read_offset::<_, LittleEndianU32>(Offset::ExpansionRomBaseAddress) } {
            0x0 => None,
            value => Some(value as usize),
        }
    }

    // pub(self) fn capabilities(&self) -> CapablitiesIterator {
    //     CapablitiesIterator::new(&self.mmio, unsafe { (self.mmio.read::<u8>(0x34).assume_init() & !0b11) as usize })
    // }

    // pub fn get_capability<T: capabilities::Capability>(&self) -> Option<T> {
    //     let initial_capability_offset = unsafe { self.read_offset::<_, LittleEndianU8>(Self::ROW_SIZE * 0xD) };
    //     let capabilities_iterator = CapablitiesIterator::new(self);

    //     for (capability_type, capability_base_ptr) in capabilities_iterator {
    //         if capability_type == T::TYPE_CODE {
    //             return Some(unsafe {
    //                 T::from_base_ptr(
    //                     capability_base_ptr,
    //                     [
    //                         self.get_bar(0),
    //                         self.get_bar(1),
    //                         self.get_bar(2),
    //                         self.get_bar(3),
    //                         self.get_bar(4),
    //                         self.get_bar(5),
    //                     ],
    //                 )
    //             });
    //         }
    //     }

    //     None
    // }

    pub fn get_interrupt_line(&self) -> Option<u8> {
        match unsafe { self.read_offset::<_, LittleEndianU8>(Offset::InterruptLine) } {
            0xFF => None,
            value => Some(value),
        }
    }

    pub fn get_interrupt_pin(&self) -> Option<u8> {
        match unsafe { self.read_offset::<_, LittleEndianU8>(Offset::InterruptPin) } {
            0x0 => None,
            value @ 0x1..=0x4 => Some(value),
            _ => unimplemented!(),
        }
    }

    pub fn get_min_grant(&self) -> u8 {
        let value = unsafe { self.read_offset::<_, LittleEndianU8>(Offset::MinGrant) };
        debug_assert_eq!(value, 0, "PCIe hardwired to 0");

        value
    }

    pub fn get_max_latency(&self) -> u8 {
        let value = unsafe { self.read_offset::<_, LittleEndianU8>(Offset::MaxLatency) };
        debug_assert_eq!(value, 0, "PCIe hardwired to 0");

        value
    }
}

impl core::fmt::Debug for Device<Standard> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let debug_struct = &mut formatter.debug_struct("PCIe Device (Standard)");

        self.generic_debug_fmt(debug_struct);
        debug_struct
            .field("Cardbus CIS Pointer", &self.get_cardbus_cis_ptr())
            .field("Subsystem Vendor ID", &self.get_subsystem_vendor_id())
            .field("Subsystem ID", &self.get_subsystem_id())
            .field("Expansion ROM Base Address", &self.get_expansion_rom_base_addr())
            .field("Interrupt Line", &self.get_interrupt_line())
            .field("Interrupt Pin", &self.get_interrupt_pin())
            .field("Min Grant", &self.get_min_grant())
            .field("Max Latency", &self.get_max_latency())
            .finish()
    }
}
