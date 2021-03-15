pub mod express;
pub mod legacy;

use bitflags::bitflags;
use core::fmt::Debug;

bitflags! {
    pub struct PCIDeviceStatus: u16 {
        const CAPABILITIES = 1 << 4;
        const CAPABILITITY_66MHZ = 1 << 5;
        const FAST_BACK2BACK_CAPABLE = 1 << 7;
        const MASTER_DATA_PARITY_ERROR = 1 << 8;
        const SIGNALED_TARGET_ABORT = 1 << 11;
        const RECEIVED_TARGET_ABORT = 1 << 12;
        const RECEIVED_MASTER_ABORT =  1 << 13;
        const SIGNALED_SYSTEM_ERROR = 1 << 14;
        const DETECTED_PARITY_ERROR = 1 << 15;
    }
}

bitflags! {
    pub struct PCIDeviceSELTiming: u16 {
        const FAST = 0;
        const MEDIUM = 1;
        const SLOW = 1 << 1;
    }
}

impl PCIDeviceStatus {
    pub fn devsel_timing(&self) -> PCIDeviceSELTiming {
        PCIDeviceSELTiming::from_bits_truncate((self.bits() >> 9) & 0b11)
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PCIDeviceClass {
    Unclassified = 0x0,
    MassStorageController = 0x1,
    NetworkController = 0x2,
    DisplayController = 0x3,
    MultimediaController = 0x4,
    MemoryController = 0x5,
    Bridge = 0x6,
    CommunicationController = 0x7,
    GenericSystemPeripheral = 0x8,
    InputDeviceController = 0x9,
    DockingStation = 0xA,
    Processor = 0xB,
    SerialBusController = 0xC,
    WirelessController = 0xD,
    IntelligentController = 0xE,
    SatelliteCommunicationsController = 0xF,
    EncryptionController = 0x10,
    SignalProcessingController = 0x11,
    ProcessingAccelerators = 0x12,
    NonEssentialInstrumentation = 0x13,
    Coprocessor = 0x40,
    Unassigned = 0xFF,
}

#[repr(transparent)]
pub struct PCIBISTRegister {
    data: u8,
}

impl PCIBISTRegister {
    pub fn capable(&self) -> bool {
        (self.data & (1 << 7)) > 0
    }

    pub fn start(&self) -> bool {
        (self.data & (1 << 6)) > 0
    }

    pub fn completion_code(&self) -> u8 {
        self.data & 0b111
    }
}

impl core::fmt::Debug for PCIBISTRegister {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("BIST")
            .field("Capable", &self.capable())
            .field("Start", &self.start())
            .field("Completion Code", &self.completion_code())
            .finish()
    }
}

pub trait PCIDeviceHeaderType {}

pub enum Standard {}
impl PCIDeviceHeaderType for Standard {}

pub enum PCI2PCI {}
impl PCIDeviceHeaderType for PCI2PCI {}

pub enum PCI2CardBus {}
impl PCIDeviceHeaderType for PCI2CardBus {}

pub struct PCIDeviceHeader<T: PCIDeviceHeaderType> {
    vendor_id: u16,
    device_id: u16,
    command: u16,
    status: u16,
    revision_id: u8,
    prog_interface: u8,
    /// Subclass of the PCI device class.
    ///
    /// An explanation for this value can be found at:
    ///   https://pcisig.com/sites/default/files/files/PCI_Code-ID_r_1_11__v24_Jan_2019.pdf
    subclass: u8,
    class: PCIDeviceClass,
    cache_line_size: u8,
    latency_timer: u8,
    header_type: u8,
    bist: PCIBISTRegister,
    phantom: core::marker::PhantomData<T>,
}

impl<T: PCIDeviceHeaderType> PCIDeviceHeader<T> {
    pub fn is_invalid(&self) -> bool {
        self.vendor_id() == u16::MAX
    }

    pub fn vendor_id(&self) -> u16 {
        self.vendor_id
    }

    pub fn device_id(&self) -> u16 {
        self.device_id
    }

    pub fn status(&self) -> PCIDeviceStatus {
        PCIDeviceStatus::from_bits_truncate(self.status)
    }

    pub fn class(&self) -> PCIDeviceClass {
        self.class
    }

    pub fn subclass(&self) -> u8 {
        self.subclass
    }

    pub fn header_type(&self) -> u8 {
        self.header_type
    }

    pub fn bist(&self) -> &PCIBISTRegister {
        &self.bist
    }
}

impl core::fmt::Debug for PCIDeviceHeader {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("PCIDeviceHeader")
            .field("Vendor ID", &self.vendor_id())
            .field("Device ID", &self.device_id())
            .field("Class", &self.class())
            .field("Subclass", &self.subclass())
            .field("Status", &self.status())
            .field("Header Type", &self.header_type())
            .field("BIST", self.bist())
            .finish()
    }
}
