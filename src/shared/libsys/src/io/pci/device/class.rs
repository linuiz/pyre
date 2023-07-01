#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Unclassified(Unclassified),
    MassStorageController(MassStorageController),
    DisplayController(DisplayController),
    Bridge(Bridge),

    ProcessingAccelerator { subclass: u8, prog_if: u8 },
    NonEssentialInstrumentation { subclass: u8, prog_if: u8 },
    Coprocessor { subclass: u8, prog_if: u8 },
    VendorSpecific { subclass: u8, prog_if: u8 },

    Unknown { class: u8, subclass: u8, prog_if: u8 },
}

impl Class {
    pub const fn parse(class: u8, subclass: u8, prog_if: u8) -> Self {
        match (class, subclass, prog_if) {
            // Unclassified
            (0x0, 0x0, 0x0) => Class::Unclassified(Unclassified::NonVgaCompatible),
            (0x0, 0x1, 0x0) => Class::Unclassified(Unclassified::VgaCompatible),

            // Mass Storage
            (0x01, 0x00, 0x0) => Class::MassStorageController(MassStorageController::Scsi),
            (0x01, 0x01, 0x0) => Class::MassStorageController(MassStorageController::Ide(IdeController::IsaOnly)),
            (0x01, 0x01, 0x5) => Class::MassStorageController(MassStorageController::Ide(IdeController::PciNativeOnly)),
            (0x01, 0x01, 0xA) => {
                Class::MassStorageController(MassStorageController::Ide(IdeController::IsaSupportingPciNative))
            }
            (0x01, 0x01, 0xF) => {
                Class::MassStorageController(MassStorageController::Ide(IdeController::PciNativeSupportingIsa))
            }
            (0x01, 0x01, 0x80) => {
                Class::MassStorageController(MassStorageController::Ide(IdeController::IsaBusMastering))
            }
            (0x01, 0x01, 0x85) => {
                Class::MassStorageController(MassStorageController::Ide(IdeController::PciNativeBusMastering))
            }
            (0x01, 0x01, 0x8A) => {
                Class::MassStorageController(MassStorageController::Ide(IdeController::IsaFullSupport))
            }
            (0x01, 0x01, 0x8F) => {
                Class::MassStorageController(MassStorageController::Ide(IdeController::PciFullSupport))
            }
            (0x01, 0x02, 0x0) => Class::MassStorageController(MassStorageController::Floppy),
            (0x01, 0x03, 0x0) => Class::MassStorageController(MassStorageController::Ipi),
            (0x01, 0x04, 0x0) => Class::MassStorageController(MassStorageController::Raid),
            (0x01, 0x05, 0x20) => Class::MassStorageController(MassStorageController::AtaSingleStep),
            (0x01, 0x05, 0x30) => Class::MassStorageController(MassStorageController::AtaContinuous),
            (0x01, 0x06, 0x0) => Class::MassStorageController(MassStorageController::SataVendorSpecific),
            (0x01, 0x06, 0x1) => Class::MassStorageController(MassStorageController::SataAhci),
            (0x01, 0x07, 0x0) => Class::MassStorageController(MassStorageController::Sas),
            (0x01, 0x80, 0x0) => Class::MassStorageController(MassStorageController::Other),

            // Display
            (0x3, 0x0, 0x0) => Class::DisplayController(DisplayController::Vga),
            (0x3, 0x1, 0x0) => Class::DisplayController(DisplayController::Xga),
            (0x3, 0x2, 0x0) => Class::DisplayController(DisplayController::NonVga3D),
            (0x3, 0x80, 0x0) => Class::DisplayController(DisplayController::Other),

            // Bridge
            (0x6, 0x0, 0x0) => Class::Bridge(Bridge::Host),
            (0x6, 0x1, 0x0) => Class::Bridge(Bridge::Isa),
            (0x6, 0x2, 0x0) => Class::Bridge(Bridge::Eisa),
            (0x6, 0x3, 0x0) => Class::Bridge(Bridge::Mca),
            (0x6, 0x4, 0x0) => Class::Bridge(Bridge::Pci2Pci(Pci2PciBridge::NormalDecode)),
            (0x6, 0x4, 0x1) => Class::Bridge(Bridge::Pci2Pci(Pci2PciBridge::SubtractiveDecode)),
            (0x6, 0x9, 0x40) => Class::Bridge(Bridge::Pci2Pci(Pci2PciBridge::SemiTransparentPrimaryBus)),
            (0x6, 0x9, 0x80) => Class::Bridge(Bridge::Pci2Pci(Pci2PciBridge::SemiTransparentSecondaryBus)),
            (0x6, 0x5, 0x0) => Class::Bridge(Bridge::Pcmcia),
            (0x6, 0x6, 0x0) => Class::Bridge(Bridge::NuBus),
            (0x6, 0x7, 0x0) => Class::Bridge(Bridge::CardBus),
            (0x6, 0x8, 0x0) => Class::Bridge(Bridge::RACEway(RACEwayBridge::TransparentMode)),
            (0x6, 0x8, 0x1) => Class::Bridge(Bridge::RACEway(RACEwayBridge::EndpointMode)),
            (0x6, 0x9, 0x0) => Class::Bridge(Bridge::InfiniBand2Pci),
            (0x6, 0x80, 0x0) => Class::Bridge(Bridge::Other),

            (0x12, subclass, prog_if) => Class::ProcessingAccelerator { subclass, prog_if },
            (0x13, subclass, prog_if) => Class::NonEssentialInstrumentation { subclass, prog_if },
            (0x40, subclass, prog_if) => Class::Coprocessor { subclass, prog_if },
            (0xFF, subclass, prog_if) => Class::VendorSpecific { subclass, prog_if },

            (class, subclass, prog_if) => Class::Unknown { class, subclass, prog_if },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Unclassified {
    NonVgaCompatible,
    VgaCompatible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MassStorageController {
    Scsi,
    Ide(IdeController),
    Floppy,
    Ipi,
    Raid,
    AtaSingleStep,
    AtaContinuous,
    SataVendorSpecific,
    SataAhci,
    Sas,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IdeController {
    IsaOnly,
    PciNativeOnly,
    IsaSupportingPciNative,
    PciNativeSupportingIsa,
    IsaBusMastering,
    PciNativeBusMastering,
    IsaFullSupport,
    PciFullSupport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DisplayController {
    Vga,
    Xga,
    NonVga3D,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bridge {
    Host,
    Isa,
    Eisa,
    Mca,
    Pci2Pci(Pci2PciBridge),
    Pcmcia,
    NuBus,
    CardBus,
    RACEway(RACEwayBridge),
    InfiniBand2Pci,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pci2PciBridge {
    NormalDecode,
    SubtractiveDecode,
    SemiTransparentPrimaryBus,
    SemiTransparentSecondaryBus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RACEwayBridge {
    TransparentMode,
    EndpointMode,
}
