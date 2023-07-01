use super::{Device, Pci2Pci};
use crate::{LittleEndianU16, LittleEndianU32, LittleEndianU8};

#[repr(usize)]
enum Offset {
    PrimaryBusNumber = 0x18,
    SecondaryBusNumber = 0x19,
    SubordinateBusNumber = 0x1A,
    SecondaryLatencyTimer = 0x1B,
    IoBase = 0x1C,
    IoLimit = 0x1D,
    SecondaryStatus = 0x1E,
    MemoryBase = 0x20,
    MemoryLimit = 0x22,
    PrefetchableMemoryBase = 0x24,
    PrefetchableMemoryLimit = 0x26,
    PrefetchableBaseUpper = 0x28,
    PrefetchableLimitUpper = 0x2C,
    IoBaseUpper = 0x30,
    IoLimitUpper = 0x32,
    CapabilityPtr = 0x34,
    ExpansionRomBaseAddress = 0x38,
    InterruptLine = 0x3C,
    InterruptPin = 0x3D,
    BridgeControl = 0x3E,
}

impl From<Offset> for usize {
    fn from(value: Offset) -> Self {
        value as usize
    }
}

impl Device<Pci2Pci> {
    pub fn get_primary_bus_number(&self) -> u8 {
        unsafe { self.read_offset::<_, LittleEndianU8>(Offset::PrimaryBusNumber) }
    }

    pub fn get_secondary_bus_number(&self) -> u8 {
        unsafe { self.read_offset::<_, LittleEndianU8>(Offset::SecondaryBusNumber) }
    }

    pub fn get_subordinate_bus_number(&self) -> u8 {
        unsafe { self.read_offset::<_, LittleEndianU8>(Offset::SubordinateBusNumber) }
    }
}
