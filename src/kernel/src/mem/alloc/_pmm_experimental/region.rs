#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Kind {
    Unusable,
    Generic,
    Reserved,
    BootReclaim,
    AcpiReclaim,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Region {
    kind: Kind,
    start: usize,
    size: usize,
}

impl Region {
    #[inline]
    pub const fn undefined() -> Self {
        Self::new(Kind::Unusable, usize::MAX, 0)
    }

    #[inline]
    pub const fn new(kind: Kind, start: usize, size: usize) -> Self {
        Self { kind, start, size }
    }

    #[inline]
    pub const fn start(&self) -> usize {
        self.start
    }

    #[inline]
    pub const fn end(&self) -> usize {
        self.start + self.size
    }

    #[inline]
    pub const fn size(&self) -> usize {
        self.size
    }

    #[inline]
    pub fn kind(&self) -> Kind {
        self.kind
    }
}

impl core::fmt::Debug for Region {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Region")
            .field("Kind", &self.kind())
            .field("Start", &self.start())
            .field("Size", &self.size())
            .finish()
    }
}

impl PartialEq for Region {
    fn eq(&self, other: &Self) -> bool {
        self.start.eq(&other.start)
    }
}

impl Eq for Region {}

impl PartialOrd for Region {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Region {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.start.cmp(&other.start)
    }
}
