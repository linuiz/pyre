use crate::mem::{
    alloc::{KernelAllocator, KMALLOC},
    HHDM,
};
use acpi::PhysicalMapping;
use port::{PortAddress, ReadWritePort};
use spin::{Lazy, Mutex};

errorgen! {
    #[derive(Debug)]
    pub enum Error {
        Acpi { err: acpi::AcpiError } => None,
        Boot { err: crate::init::boot::Error } => Some(err)
    }
}

pub enum Register<'a, T: port::PortReadWrite> {
    Io(ReadWritePort<T>),
    Mmio(&'a libkernel::mem::VolatileCell<T, libkernel::ReadWrite>),
}

impl<T: port::PortReadWrite> Register<'_, T> {
    pub const fn new(generic_address: &acpi::address::GenericAddress) -> Option<Self> {
        match generic_address.address_space {
            acpi::address::AddressSpace::SystemMemory => {
                Some(Self::Mmio(
                    // Safety: There's no meaningful way to validate the address provided by the `GenericAddress` structure.
                    unsafe { &*(generic_address.address as *const _) },
                ))
            }

            acpi::address::AddressSpace::SystemIo => {
                Some(Self::Io(
                    // Safety: There's no meaningful way to validate the port provided by the `GenericAddress` structure.
                    unsafe {
                        #[allow(clippy::cast_possible_truncation)]
                        ReadWritePort::<T>::new(generic_address.address as PortAddress)
                    },
                ))
            }

            _ => None,
        }
    }

    #[inline]
    pub fn read(&self) -> T {
        match self {
            Register::Io(port) => port.read(),
            Register::Mmio(addr) => addr.read(),
        }
    }

    #[inline]
    pub fn write(&mut self, value: T) {
        match self {
            Register::Io(port) => port.write(value),
            Register::Mmio(addr) => addr.write(value),
        }
    }
}

#[derive(Clone, Copy)]
#[allow(clippy::module_name_repetitions)]
pub struct AcpiHandler;

impl acpi::AcpiHandler for AcpiHandler {
    unsafe fn map_physical_region<T>(&self, address: usize, size: usize) -> acpi::PhysicalMapping<Self, T> {
        trace!("ACPI MAP: @{:#X}:{}", address, size);

        acpi::PhysicalMapping::new(
            address,
            core::ptr::NonNull::new(HHDM.ptr().add(address).cast()).unwrap(),
            size,
            size,
            Self,
        )
    }

    fn unmap_physical_region<T>(_: &acpi::PhysicalMapping<Self, T>) {
        // ... We don't actually need to unmap anything, since this utilizes the HHDM
    }
}

// #[allow(clippy::undocumented_unsafe_blocks)]
// impl aml::Handler for AcpiHandler {
//     fn read_u8(&self, address: usize) -> u8 {
//         unsafe { (address as *const u8).read() }
//     }

//     fn read_u16(&self, address: usize) -> u16 {
//         unsafe { (address as *const u16).read() }
//     }

//     fn read_u32(&self, address: usize) -> u32 {
//         unsafe { (address as *const u32).read() }
//     }

//     fn read_u64(&self, address: usize) -> u64 {
//         unsafe { (address as *const u64).read() }
//     }

//     fn write_u8(&mut self, address: usize, value: u8) {
//         unsafe { (address as *mut u8).write(value) };
//     }

//     fn write_u16(&mut self, address: usize, value: u16) {
//         unsafe { (address as *mut u16).write(value) };
//     }

//     fn write_u32(&mut self, address: usize, value: u32) {
//         unsafe { (address as *mut u32).write(value) };
//     }

//     fn write_u64(&mut self, address: usize, value: u64) {
//         unsafe { (address as *mut u64).write(value) };
//     }

//     fn read_io_u8(&self, port: u16) -> u8 {
//         unsafe { ReadOnlyPort::<u8>::new(port as PortAddress) }.read()
//     }

//     fn read_io_u16(&self, port: u16) -> u16 {
//         unsafe { ReadOnlyPort::<u16>::new(port as PortAddress) }.read()
//     }

//     fn read_io_u32(&self, port: u16) -> u32 {
//         unsafe { ReadOnlyPort::<u32>::new(port as PortAddress) }.read()
//     }

//     fn write_io_u8(&self, port: u16, value: u8) {
//         unsafe { WriteOnlyPort::<u8>::new(port as PortAddress) }.write(value);
//     }

//     fn write_io_u16(&self, port: u16, value: u16) {
//         unsafe { WriteOnlyPort::<u16>::new(port as PortAddress) }.write(value);
//     }

//     fn write_io_u32(&self, port: u16, value: u32) {
//         unsafe { WriteOnlyPort::<u32>::new(port as PortAddress) }.write(value);
//     }

//     fn read_pci_u8(&self, _segment: u16, _bus: u8, _device: u8, _function: u8, _offset: u16) -> u8 {
//         todo!()
//     }

//     fn read_pci_u16(&self, _segment: u16, _bus: u8, _device: u8, _function: u8, _offset: u16) -> u16 {
//         todo!()
//     }

//     fn read_pci_u32(&self, _segment: u16, _bus: u8, _device: u8, _function: u8, _offset: u16) -> u32 {
//         todo!()
//     }

//     fn write_pci_u8(&self, _segment: u16, _bus: u8, _device: u8, _function: u8, _offset: u16, _value: u8) {
//         todo!()
//     }

//     fn write_pci_u16(&self, _segment: u16, _bus: u8, _device: u8, _function: u8, _offset: u16, _value: u16) {
//         todo!()
//     }

//     fn write_pci_u32(&self, _segment: u16, _bus: u8, _device: u8, _function: u8, _offset: u16, _value: u32) {
//         todo!()
//     }
// }

pub static TABLES: spin::Once<Mutex<acpi::AcpiTables<AcpiHandler>>> = spin::Once::new();

pub fn init_interface() -> Result<()> {
    debug!("Initializing ACPI interface...");

    TABLES.try_call_once(|| {
        let rsdp_address = crate::init::boot::get_rsdp_address().map_err(|err| Error::Boot { err })?;
        // Safety: Bootloader guarantees any address provided for RDSP will be valid.
        let acpi_tables = unsafe { acpi::AcpiTables::from_rsdp(AcpiHandler, rsdp_address.get()) }
            .map_err(|err| Error::Acpi { err })?;

        Ok(Mutex::new(acpi_tables))
    })?;

    debug!("Initialized ACPI interface.");

    Ok(())
}

pub static FADT: Lazy<Option<Mutex<PhysicalMapping<AcpiHandler, acpi::fadt::Fadt>>>> = Lazy::new(|| {
    TABLES.get().map(Mutex::lock).and_then(|tables| tables.find_table::<acpi::fadt::Fadt>().ok()).map(Mutex::new)
});

pub static MCFG: Lazy<Option<Mutex<PhysicalMapping<AcpiHandler, acpi::mcfg::Mcfg>>>> = Lazy::new(|| {
    TABLES.get().map(Mutex::lock).and_then(|tables| tables.find_table::<acpi::mcfg::Mcfg>().ok()).map(Mutex::new)
});

pub static PLATFORM_INFO: Lazy<Option<Mutex<acpi::PlatformInfo<&'static KernelAllocator>>>> = Lazy::new(|| {
    TABLES
        .get()
        .map(Mutex::lock)
        .and_then(|tables| acpi::PlatformInfo::new_in(&*tables, &*KMALLOC).ok())
        .map(Mutex::new)
});

// struct AmlContextWrapper(aml::AmlContext);
// // Safety: TODO
// unsafe impl Sync for AmlContextWrapper {}

// static AML_CONTEXT: Once<AmlContextWrapper> = Once::new();

// pub fn init_aml_context() {
//     AML_CONTEXT.call_once(|| {
//         AmlContextWrapper({
//             let mut aml_context = aml::AmlContext::new(alloc::boxed::Box::new(AcpiHandler), aml::DebugVerbosity::All);
//             let kernel_hhdm_address = crate::memory::get_hhdm_address().as_usize();
//             let rsdp = get_rsdp();

//             {
//                 let dsdt_table = rsdp.dsdt.as_ref().expect("machine has no DSDT");

//                 // Safety: We can be reasonably certain the provided base address and length are valid.
//                 let dsdt_stream = unsafe {
//                     core::slice::from_raw_parts(
//                         (dsdt_table.address + kernel_hhdm_address) as *const u8,
//                         dsdt_table.length as usize,
//                     )
//                 };

//                 aml_context.parse_table(dsdt_stream).expect("failed to parse DSDT");
//             }

//             {
//                 for sdst_table in &get_rsdp().ssdts {
//                     // Safety: We can be reasonably certain the provided base address and length are valid.
//                     let sdst_stream = unsafe {
//                         core::slice::from_raw_parts(
//                             (sdst_table.address + kernel_hhdm_address) as *const u8,
//                             sdst_table.length as usize,
//                         )
//                     };

//                     aml_context.parse_table(sdst_stream).expect("failed to parse SDST");
//                 }
//             }

//             aml_context.initialize_objects().expect("failed to initialize AML objects");

//             aml_context
//         })
//     });
// }
