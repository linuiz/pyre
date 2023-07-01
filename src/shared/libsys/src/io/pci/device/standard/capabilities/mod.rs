// mod msix;
// pub use msix::*;

use crate::mem::io::pci::Device;

use super::Kind;
use libkernel::{LittleEndianU32, LittleEndianU8};

pub trait Capability {
    const TYPE_CODE: u8;
    const BARS_USED: [bool; super::Standard::REGISTER_COUNT];

    unsafe fn from_base_ptr(capability_base_ptr: *mut LittleEndianU32, bars: [Option<super::Bar>; 6]) -> Self;
}

pub(super) struct CapablitiesIterator<'a, K: Kind> {
    device: &'a Device<K>,
    next_offset: u8,
}

impl<'a, K: Kind> CapablitiesIterator<'a, K> {
    pub(super) fn new(device: &'a Device<K>) -> Self {
        Self { device, next_offset: unsafe { device.read_offset::<LittleEndianU8>(Device::<K>::ROW_SIZE * 0xD) } }
    }
}

impl Iterator for CapablitiesIterator {
    type Item = (u8, *mut LittleEndianU32);

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_offset > 0 {
            unsafe {
                use bit_field::BitField;

                let capability_base_ptr =
                    (self.base_config_address + (self.next_offset as usize)) as *mut LittleEndianU32;
                let capability_reg0 = capability_base_ptr.read_volatile().get();
                self.next_offset = capability_reg0.get_bits(8..16) as u8;

                Some((capability_reg0.get_bits(0..8) as u8, capability_base_ptr))
            }
        } else {
            None
        }
    }
}
