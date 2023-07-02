use crate::mem::{alloc::pmm, paging, HHDM};
use alloc::{collections::BTreeMap, vec::Vec};
use core::ptr::NonNull;
use libsys::{
    io::pci::{Class, Device, DeviceKind, Standard},
    Address, Frame, LittleEndian, LittleEndianU16,
};
use spin::Mutex;
use uuid::Uuid;

errorgen! {
    #[derive(Debug)]
    pub enum Error {
        NoninitTables => None,
        Acpi { err: acpi::AcpiError } => None,
        Paging { err: paging::Error } => Some(err)
    }
}

enum Ownership {
    Available,
    Owned(Uuid),
}

static DEVICES: Mutex<BTreeMap<Class, Vec<(Ownership, Device<Standard>)>>> = Mutex::new(BTreeMap::new());

pub fn get_device_base_address(base: usize, bus_index: u8, device_index: u8) -> Address<Frame> {
    let bus_index = usize::from(bus_index);
    let device_index = usize::from(device_index);

    Address::new(base | (bus_index << 20) | (device_index << 15)).unwrap()
}

pub fn init_devices() -> Result<()> {
    let mut devices = DEVICES.lock();

    let acpi_tables = crate::acpi::get_tables();
    let pci_regions = acpi::PciConfigRegions::new(acpi_tables, pmm::get()).map_err(|err| Error::Acpi { err })?;

    pci_regions
        .iter()
        .map(|entry| (entry.physical_address, entry.segment_group, entry.bus_range))
        .flat_map(|(base_address, segment_index, bus_range)| {
            bus_range.map(move |bus_index| (base_address, segment_index, bus_index))
        })
        .flat_map(|(base_address, segment_index, bus_index)| {
            (0u8..32u8).map(move |device_index| (base_address, segment_index, bus_index, device_index))
        })
        .try_for_each(|(base_address, segment_index, bus_index, device_index)| {
            let device_frame = get_device_base_address(base_address, bus_index, device_index);
            let device_page = HHDM.offset(device_frame).unwrap();

            // Safety: We should be reading known-good memory here, according to the PCI spec. The following `if` test will verify that.
            let vendor_id = unsafe { device_page.as_ptr().cast::<LittleEndianU16>().read_volatile() };
            if vendor_id.get() > u16::MIN && vendor_id.get() < u16::MAX {
                debug!(
                    "Configuring PCIe device: [{:0>2}:{:0>2}:{:0>2}.00@{:X?}]",
                    segment_index, bus_index, device_index, device_page
                );

                // Safety: Base pointer, at this point, has been verified as known-good.
                let device = unsafe { libsys::io::pci::new_device(NonNull::new(device_page.as_ptr()).unwrap()) };

                #[allow(clippy::single_match)]
                match device {
                    Ok(DeviceKind::Standard(device)) => {
                        trace!("{:#?}", device);

                        let class_devices = devices.entry(device.get_class()).or_insert(Vec::new());
                        class_devices.push((Ownership::Available, device));
                    }

                    // TODO handle PCI-to-PCI busses
                    _ => {}
                }
            }

            Ok(())
        })
}
