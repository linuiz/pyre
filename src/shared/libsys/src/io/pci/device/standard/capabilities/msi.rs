#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[allow(non_camel_case_types)]
pub enum MultipleMessage {
    x1 = 0b000,
    x2 = 0b001,
    x4 = 0b010,
    x8 = 0b011,
    x16 = 0b100,
    x32 = 0b101,
}

#[repr(C)]
pub struct MessageControl(VolatileCell<u32, ReadWrite>);

impl MessageControl {
    pub fn get_msi_enable(&self) -> bool {
        self.0.read().get_bit(0)
    }

    pub fn set_msi_enable(&self, enable: bool) {
        self.0.write(self.0.read().set_bit(1, enable));
    }

    pub fn get_multi_msg_capable(&self) -> MultipleMessage {
        MultipleMessage::try_from_primitive(self.0.read().get_bits(1..4)).unwrap()
    }

    pub fn try_set_multi_msg_enable(&self, mme: MultipleMessage) -> Result<(), ()> {
        self.0.write(self.0.read().set_bits(4..7, mme as u32));
    }

    pub fn get_long_mode_capable(&self) -> bool {
        self.0.read().get_bit(7)
    }

    pub fn get_per_vector_masking(&self) -> bool {
        self.0.read().get_bit(8)
    }

    pub fn get_table_len(&self) -> usize {
        self.0.read().get_bits(16..27) as usize
    }

    volatile_bitfield_getter!(0, force_mask, 30);
    volatile_bitfield_getter!(0, enable, 31);
}