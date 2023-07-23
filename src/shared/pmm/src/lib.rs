#![cfg_attr(not(test), no_std)]
#![feature(
    error_in_core,  // #103765 <https://github.com/rust-lang/rust/issues/103765>
    if_let_guard,   // #51114 <https://github.com/rust-lang/rust/issues/51114>
    let_chains,     //#53667 <https://github.com/rust-lang/rust/issues/53667>
)]

use core::{fmt, marker::PhantomData, mem::size_of, ops::Range, ptr::NonNull, convert::Infallible};

pub trait Metadata: Default + Clone + Copy + PartialEq + Eq + From<usize> + Into<usize> {
    fn is_usable(&self) -> bool;
}

impl Metadata for usize {
    fn is_usable(&self) -> bool {
        false
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Region<M: Metadata> {
    metadata: usize,
    start: usize,
    end: usize,
    _reserved: [u8; size_of::<usize>()],
    _marker: PhantomData<M>,
}

impl<M: Metadata> Region<M> {
    #[inline]
    pub const fn extents(&self) -> Range<usize> {
        self.start..self.end
    }

    #[inline]
    pub fn metadata(&self) -> M {
        M::from(self.metadata)
    }
}

impl<M: Metadata> fmt::Debug for Region<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Region").field(&self.metadata).field(&self.extents()).finish()
    }
}

const REGION_TABLE_SIZE: usize = (libsys::page_size() / core::mem::size_of::<Region<usize>>()) - 1;

#[repr(C)]
pub struct RegionTable<M: Metadata> {
    table: [Region<M>; REGION_TABLE_SIZE],

    /* These two fields should be the same total size as a `Region<M>` */
    len: usize,
    next_table_ptr: Option<NonNull<Self>>,
    phantom: PhantomData<M>,
}

impl<M: Metadata> Default for RegionTable<M> {
    fn default() -> Self {
        Self { table: [Region::default(); REGION_TABLE_SIZE], len: 0, next_table_ptr: None, phantom: PhantomData }
    }
}

impl<M: Metadata> RegionTable<M> {
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub const fn is_full(&self) -> bool {
        self.len() == REGION_TABLE_SIZE
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    fn table(&self) -> &[Region<M>] {
        let len = self.len();
        &self.table[..len]
    }
    #[inline]
    fn table_mut(&mut self) -> &mut [Region<M>] {
        let len = self.len();
        &mut self.table[..len]
    }

    #[inline]
    fn next_table(&self) -> Option<&Self> {
        // Safety: If pointer is non-null, it's been allocated.
        self.next_table_ptr.map(|ptr| unsafe { ptr.as_ref() })
    }

    #[inline]
    fn next_table_mut(&mut self) -> Option<&mut Self> {
        // Safety: If pointer is non-null, it's been allocated.
        self.next_table_ptr.map(|mut ptr| unsafe { ptr.as_mut() })
    }

    /// Shuffles all elements at the given index down by one.
    fn shuffle_down(&mut self, index: usize) {
        assert!(index < self.len());

        unsafe {
            let copy_from = self.table.as_ptr().add(index);
            let copy_to = copy_from.sub(1).cast_mut();
            let copy_count = self.table().len() - index;
            core::ptr::copy(copy_from, copy_to, copy_count);
        }

        if let Some(_next_table) = self.next_table_mut() {
            todo!(
                "
                1. shuffle first item from next table
                2. check if we need to deallocate the next table
                "
            );
        } else {
            self.len -= 1;
        }
    }

    /// Shuffles all elements at the given index up by one.
    fn shuffle_up(&mut self, index: usize) {
        assert!(index < self.len);

        if self.is_full() {
            todo!(
                "
                1. allocate a new table
                2. place last item into next table
                "
            )
        } else {
            self.len += 1;
        }

        unsafe {
            let copy_from = self.table.as_ptr().add(index);
            let copy_to = copy_from.add(1).cast_mut();
            let copy_count = self.table().len() - index;
            core::ptr::copy(copy_from, copy_to, copy_count);
        }
    }

    pub fn insert(&mut self, region: Region<M>) {
        match self.table().iter().rposition(|region| region.end <= region.start) {
            Some(insert_at) => {
                let insert_at_region = self.table().get(insert_at).copied().unwrap();
                let insert_at_pre_region = self.table().get(insert_at - 1).copied();

                let collapse_at =
                    region.end == insert_at_region.start && region.metadata() == insert_at_region.metadata();
                let collapse_at_pre = insert_at_pre_region
                    .map(|pre_region| region.start == pre_region.end && region.metadata() == pre_region.metadata())
                    .unwrap_or(false);

                match (collapse_at, collapse_at_pre) {
                    (true, true) => {
                        let collapse_at = self.table_mut().get_mut(insert_at).unwrap();
                        collapse_at.start = insert_at_pre_region.unwrap().start;
                        self.shuffle_down(insert_at);
                    }

                    (true, false) => {
                        let collapse_at = self.table_mut().get_mut(insert_at).unwrap();
                        collapse_at.start = region.start;
                    }

                    (false, true) => {
                        let collapse_at_pre = self.table_mut().get_mut(insert_at - 1).unwrap();
                        collapse_at_pre.end = region.end;
                    }

                    (false, false) => {
                        self.shuffle_up(insert_at);
                        self.table_mut()[insert_at] = region;
                    }
                }
            }

            None if !self.is_full() => {
                self.table[self.len()] = region;
                self.len += 1;
            }

            None if let Some(next_table) = self.next_table_mut() => {
                next_table.insert(region);
            }

            None => {
                todo!("allocate next table");
            }
        }
    }

    fn allocate
}

impl<M: Metadata> fmt::Debug for RegionTable<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Region Table").field(&self.table()).field(&self.next_table()).finish()
    }
}

#[cfg(test)]
impl Metadata for usize {}

#[test]
pub fn test_push() {
    let mut table = RegionTable::<usize, 7>::default();

    const FACTOR: usize = 24;
    for idx in 0..FACTOR {
        table.insert({
            let mut d = Region::default();
            d.start = idx * FACTOR;
            d.end = (idx + 1) * FACTOR;

            d
        });

        println!("Current Table Status:");
        println!("{:?}", table);
    }
}
