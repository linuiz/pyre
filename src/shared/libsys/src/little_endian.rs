#![allow(dead_code)]

pub trait LittleEndian: From<Self::NativeType> {
    type NativeType;

    fn get(&self) -> Self::NativeType;
}

macro_rules! little_endian {
    ($Name:ident, $Type:ty) => {
        #[repr(transparent)]
        #[derive(Clone, Copy)]
        pub struct $Name([u8; core::mem::size_of::<$Type>()]);

        impl Eq for $Name {}
        impl PartialEq for $Name {
            fn eq(&self, other: &Self) -> bool {
                self.get() == other.get()
            }
        }

        impl core::fmt::Debug for $Name {
            fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                formatter.debug_tuple("$Name").field(&self.get()).finish()
            }
        }

        impl LittleEndian for $Name {
            type NativeType = $Type;

            fn get(&self) -> Self::NativeType {
                <$Type>::from_le_bytes(self.0)
            }
        }

        impl From<$Type> for $Name {
            fn from(value: $Type) -> Self {
                Self(value.to_le_bytes())
            }
        }
    };
}

little_endian!(LittleEndianU8, u8);
little_endian!(LittleEndianU16, u16);
little_endian!(LittleEndianU32, u32);
little_endian!(LittleEndianU64, u64);
