use crate::task::AddressSpace;
use alloc::collections::BTreeMap;
use uuid::Uuid;

pub struct VirtualMemoryManager {
    address_spaces: BTreeMap<Uuid, AddressSpace>,
}
