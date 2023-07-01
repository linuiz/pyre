#![cfg_attr(not(test), no_std)]
#![feature(
    error_in_core   // #103765 <https://github.com/rust-lang/rust/issues/103765>
)]

use core::{marker::PhantomData, mem::size_of, ops::Range, ptr::NonNull};

#[macro_use]
extern crate error;

errorgen! {
    #[derive(Debug)]
    pub enum Error {
        InvalidTableSize => None,
        RegionOverlap => None,
        Uninsertable => None,
    }
}

pub trait Metadata: PartialEq + Eq + From<usize> {}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Region {
    metadata: usize,
    start: usize,
    end: usize,
    _reserved: [u8; size_of::<usize>()],
}

impl Region {
    const fn extents(&self) -> Range<usize> {
        self.start..self.end
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct RegionTable<M: Metadata, const TABLE_LEN: usize> {
    regions: [Region; TABLE_LEN],
    len: usize,
    next_table_ptr: Option<NonNull<Self>>,
    phantom: PhantomData<M>,
}

impl<M: Metadata, const TABLE_LEN: usize> Default for RegionTable<M, TABLE_LEN> {
    fn default() -> Self {
        assert!((TABLE_LEN + 1).is_power_of_two());

        Self { regions: [Region::default(); TABLE_LEN], len: 0, next_table_ptr: None, phantom: PhantomData }
    }
}

impl<M: Metadata, const TABLE_LEN: usize> RegionTable<M, TABLE_LEN> {
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

    #[inline]
    fn increment_len_capped(&mut self) {
        self.len = core::cmp::min(TABLE_LEN, self.len + 1);
    }

    #[inline]
    pub const fn is_full(&self) -> bool {
        self.len == TABLE_LEN
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn extents(&self) -> Option<Range<usize>> {
        match self.len {
            0 => None,
            1 => Some(self.regions[0].extents()),
            _ => {
                let start_region = self.regions[0];
                let end_region = self.regions[self.len - 1];

                Some(start_region.start..end_region.end)
            }
        }
    }

    fn shuffle_in(&mut self, index: usize) {
        assert!(index < self.len);

        if self.len == TABLE_LEN {
            // check if we need to shuffle from the next table
        }

        unsafe {
            let copy_from = self.regions.as_ptr().add(index);
            let copy_to = copy_from.sub(1).cast_mut();
            let copy_count = self.len - index;
            core::ptr::copy(copy_from, copy_to, copy_count);
        }
    }

    fn shuffle_out(&mut self, index: usize) {
        assert!(index < self.len);

        if self.len == TABLE_LEN {
            // shuffle elements into next table
        }

        unsafe {
            let copy_from = self.regions.as_ptr().add(index);
            let copy_to = copy_from.add(1).cast_mut();
            let copy_count = self.len - index;
            core::ptr::copy(copy_from, copy_to, copy_count);
        }
    }

    pub fn insert(&mut self, new_region: Region) -> Result<()> {
        let new_metadata = M::from(new_region.metadata);

        match self.extents() {
            None => {
                debug_assert_eq!(self.len, 0, "len should be zero with no extents");

                self.regions[0] = new_region;

                self.increment_len_capped();
                Ok(())
            }

            Some(extents) => {
                use core::cmp::Ordering;

                match (new_region.start.cmp(&extents.start), new_region.end.cmp(&extents.end)) {
                    (Ordering::Less, Ordering::Less) => {
                        let cur_region = self.regions[0];

                        if new_region.end == cur_region.start && new_metadata == M::from(cur_region.metadata) {
                            self.regions[0].start = new_region.start;
                        } else {
                            self.shuffle_out(0);
                            self.regions[0] = new_region;
                            self.increment_len_capped();
                        }

                        Ok(())
                    }

                    (Ordering::Greater, Ordering::Less) => {
                        if let Some(insert_at) =
                            self.regions[..self.len].iter().rposition(|region| new_region.end <= region.start)
                        {
                            let cur_region = self.regions[insert_at];
                            let collapse_antecedent =
                                new_region.end == cur_region.start && new_metadata == M::from(cur_region.metadata);
                            let collapse_precedent = self
                                .regions
                                .get(insert_at - 1)
                                .map(|r| new_region.start == r.end && new_metadata == M::from(r.metadata));

                            match (collapse_precedent, collapse_antecedent) {
                                (Some(true), true) => {
                                    let start = self.regions[insert_at - 1].start;
                                    self.shuffle_in(insert_at);

                                    self.regions[insert_at].start = start;
                                    self.len -= 1;
                                }

                                (_, true) => {
                                    self.regions[insert_at].start = new_region.start;
                                    self.increment_len_capped();
                                }

                                (Some(true), _) => {
                                    self.regions[insert_at - 1].end = new_region.end;
                                    self.increment_len_capped();
                                }

                                (_, false) => {
                                    self.shuffle_out(insert_at);
                                    self.regions[insert_at] = new_region;
                                    self.increment_len_capped();
                                }
                            }

                            Ok(())
                        } else {
                            Err(Error::Uninsertable)
                        }
                    }

                    (Ordering::Greater, Ordering::Greater) if self.is_full() => {
                        if let Some(next_table) = self.next_table_mut() {
                            next_table.insert(new_region)
                        } else {
                            todo!("allocate next table")
                        }
                    }

                    (Ordering::Greater, Ordering::Greater) => {
                        let cur_region = self.regions[self.len - 1];

                        if new_region.start == cur_region.end && new_metadata == M::from(cur_region.metadata) {
                            self.regions[self.len - 1].end = new_region.end;
                        } else {
                            self.regions[self.len] = new_region;
                            self.increment_len_capped();
                        }

                        Ok(())
                    }

                    _ => Err(Error::RegionOverlap),
                }
            }
        }
    }
}

#[cfg(test)]
impl Metadata for usize {}

#[test]
pub fn test_push() {
    use std::time::{Duration, Instant};

    let mut table = RegionTable::<usize, 7>::default();

    let start = Instant::now();

    const FACTOR: usize = 1000000;
    for idx in 0..FACTOR {
        table.insert({
            let mut d = Region::default();
            d.start = idx * FACTOR;
            d.end = (idx + 1) * FACTOR;

            d
        });
    }

    let end = Instant::now();

    println!("completed in {}ms", end.duration_since(start).as_millis());
}
