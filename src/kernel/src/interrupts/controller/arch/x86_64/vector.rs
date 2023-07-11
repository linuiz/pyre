errorgen! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TryFromVectorError {
        ExceptionVector => None,
        PicVector => None,
        OutOfRange => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum Vector {
    Clock,
    Ps2Keyboard,
    /// Required for the primary PIC to cascade interrupts to the secondary PIC.
    /// **DO NOT USE**.
    PicCascade,
    Ps2Mouse,
    AutoEoi,
    Timer,
    Thermal,
    Performance,
    Error,

    Syscall,

    Other(u8),
}

impl From<Vector> for u8 {
    fn from(value: Vector) -> Self {
        match value {
            Vector::Clock => 0x20,
            Vector::Ps2Keyboard => 0x21,
            Vector::PicCascade => 0x22,
            Vector::Ps2Mouse => 0x23,
            Vector::AutoEoi => 0x24,
            Vector::Timer => 0x25,
            Vector::Thermal => 0x26,
            Vector::Performance => 0x27,
            Vector::Error => 0x28,

            Vector::Syscall => 0x80,

            Vector::Other(vector) => vector,
        }
    }
}

impl From<Vector> for u16 {
    fn from(value: Vector) -> Self {
        u8::from(value).into()
    }
}
impl From<Vector> for u32 {
    fn from(value: Vector) -> Self {
        u8::from(value).into()
    }
}
impl From<Vector> for u64 {
    fn from(value: Vector) -> Self {
        u8::from(value).into()
    }
}

impl TryFrom<u8> for Vector {
    type Error = TryFromVectorError;

    fn try_from(value: u8) -> core::result::Result<Self, <Self as TryFrom<u8>>::Error> {
        match value {
            0x0..0x20 => Err(TryFromVectorError::ExceptionVector),

            0x20 => Ok(Self::Clock),
            0x21..0x30 => Err(TryFromVectorError::PicVector),

            0x30 => Ok(Self::AutoEoi),
            0x31 => Ok(Self::Timer),
            0x32 => Ok(Self::Thermal),
            0x33 => Ok(Self::Performance),
            0x34 => Ok(Self::Error),

            0x80 => Ok(Self::Syscall),

            value => Ok(Self::Other(value)),
        }
    }
}

impl TryFrom<u16> for Vector {
    type Error = TryFromVectorError;

    fn try_from(value: u16) -> core::result::Result<Self, <Self as TryFrom<u16>>::Error> {
        u8::try_from(value).map_err(|_| TryFromVectorError::OutOfRange).and_then(Vector::try_from)
    }
}

impl TryFrom<u32> for Vector {
    type Error = TryFromVectorError;

    fn try_from(value: u32) -> core::result::Result<Self, <Self as TryFrom<u32>>::Error> {
        u8::try_from(value).map_err(|_| TryFromVectorError::OutOfRange).and_then(Vector::try_from)
    }
}

impl TryFrom<u64> for Vector {
    type Error = TryFromVectorError;

    fn try_from(value: u64) -> core::result::Result<Self, <Self as TryFrom<u64>>::Error> {
        u8::try_from(value).map_err(|_| TryFromVectorError::OutOfRange).and_then(Vector::try_from)
    }
}
