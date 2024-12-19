use std::{io, marker::PhantomData};

use memmap2::{MmapMut, MmapOptions};

use super::{virt_to_phy::virt_to_phy, PAGE_SIZE, PAGE_SIZE_BITS};

use std::ops::{Deref, DerefMut};

/// A wrapper around mapped memory that ensures physical memory pages are consecutive.
pub(crate) struct ConscMem {
    /// Mmap handle
    inner: MmapMut,
}

impl ConscMem {
    /// Creates a new consecutive memory region of the specified number of pages.
    pub(crate) fn new(num_pages: usize) -> io::Result<Self> {
        /// TODO: implements allocating multiple consecutive pages
        assert_eq!(num_pages, 1, "currently only supports allocating one page");
        let inner = Self::try_reserve_consecutive(num_pages)?;
        Ok(Self { inner })
    }

    /// Attempts to reserve consecutive physical memory pages.
    fn try_reserve_consecutive(num_pages: usize) -> io::Result<MmapMut> {
        let mmap = Self::reserve(num_pages)?;
        if Self::ensure_consecutive(&mmap)? {
            return Ok(mmap);
        }

        Err(io::Error::from(io::ErrorKind::OutOfMemory))
    }

    /// Reserves memory pages using mmap.
    fn reserve(num_pages: usize) -> io::Result<MmapMut> {
        /// Number of bits representing a 4K page size
        const PAGE_SIZE_4K_BITS: u8 = 12;
        let len = PAGE_SIZE
            .checked_mul(num_pages)
            .ok_or(io::Error::from(io::ErrorKind::Unsupported))?;
        #[cfg(feature = "page_size_2m")]
        let mmap = MmapOptions::new()
            .len(len)
            .huge(Some(PAGE_SIZE_BITS))
            .map_anon()?;
        #[cfg(feature = "page_size_4k")]
        let mmap = MmapOptions::new().len(len).map_anon()?;

        mmap.lock()?;

        Ok(mmap)
    }

    /// Checks if the physical pages backing the memory mapping are consecutive.
    #[allow(clippy::as_conversions)] // casting usize ot u64 is safe
    fn ensure_consecutive(mmap: &MmapMut) -> io::Result<bool> {
        let virt_addrs = mmap.chunks(PAGE_SIZE).map(<[u8]>::as_ptr);
        let phy_addrs = virt_to_phy(virt_addrs)?;
        if phy_addrs.iter().any(Option::is_none) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "physical address not found",
            ));
        }
        let is_consec = phy_addrs
            .iter()
            .flatten()
            .skip(1)
            .zip(phy_addrs.iter().flatten())
            .all(|(a, b)| a.saturating_sub(*b) == PAGE_SIZE as u64);

        Ok(is_consec)
    }
}

impl Deref for ConscMem {
    type Target = MmapMut;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for ConscMem {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// A fixed-size slot allocator that manages memory slots within a consecutive memory region.
pub(crate) struct SlotAlloc<Mem, Slot> {
    /// The underlying consecutive memory allocation
    mem: Mem,
    /// List of free slot indices that can be allocated
    free_list: Vec<usize>,
    /// Phantom data to carry the Slot type parameter
    _marker: PhantomData<Slot>,
}

/// Trait for types that can specify their size requirements for memory slots.
pub(crate) trait SlotSize {
    /// Returns the size in bytes required for this slot type.
    fn size() -> usize;
}

#[allow(clippy::arithmetic_side_effects)]
impl<Mem, Slot: SlotSize> SlotAlloc<Mem, Slot> {
    /// Creates a new slot allocator with the given consecutive memory region.
    pub(crate) fn new(mem: Mem) -> Self {
        let num_slots = Self::num_slots_total();
        let alloc = (0..num_slots).collect();
        Self {
            mem,
            free_list: alloc,
            _marker: PhantomData,
        }
    }

    /// Returns true if there are free slots available.
    pub(crate) fn has_free_slot(&self) -> bool {
        !self.free_list.is_empty()
    }

    /// Returns the total number of slots that can be allocated.
    pub(crate) fn num_slots_total() -> usize {
        PAGE_SIZE / Self::slot_size()
    }

    /// Returns the maximum slot number that can be allocated.
    pub(crate) fn slot_num_max() -> usize {
        Self::num_slots_total().saturating_sub(1)
    }

    /// Returns the size of each slot in bytes.
    pub(crate) fn slot_size() -> usize {
        assert!(
            Slot::size() <= PAGE_SIZE && Slot::size() != 0,
            "invalid slot size"
        );
        Slot::size()
    }
}

#[allow(clippy::arithmetic_side_effects)]
impl<Mem, Slot> SlotAlloc<Mem, Slot>
where
    Mem: AsMut<[u8]>,
    Slot: SlotSize,
{
    /// Allocates a new memory slot if available.
    /// Returns None if no slots are available.
    pub(crate) fn alloc_one(&mut self) -> Option<&mut [u8]> {
        let slot_size = Self::slot_size();
        let sn = self.free_list.pop()?;
        let slot = self
            .mem
            .as_mut()
            .get_mut(sn * slot_size..sn * (slot_size + 1))
            .unwrap_or_else(|| unreachable!("range should always exists"));
        Some(slot)
    }

    /// Deallocates a previously allocated memory slot.
    /// Returns true if deallocation was successful, false otherwise.
    pub(crate) fn dealloc(&mut self, buf: &mut [u8]) -> bool {
        if buf.len() != Self::slot_size() {
            return false;
        }
        let addr = Self::slice_ptr_addr_usize(buf);
        let begin = Self::slice_ptr_addr_usize(self.mem.as_mut());
        let sn = begin.checked_sub(addr).map(|dlt| dlt / Self::slot_size());
        let Some(sn) = sn else {
            return false;
        };
        if sn > Self::slot_num_max() {
            return false;
        }
        buf.fill(0);
        self.free_list.push(sn);

        true
    }

    /// Converts a slice pointer to its address as usize.
    #[allow(clippy::as_conversions)]
    fn slice_ptr_addr_usize<T>(slice: &[T]) -> usize {
        slice.as_ptr() as usize
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn consc_mem_alloc_succ() {
        let mem = ConscMem::new(1).expect("failed to allocate");
    }

    #[test]
    fn slot_alloc_dealloc_ok() {
        struct Slot;
        impl SlotSize for Slot {
            fn size() -> usize {
                16
            }
        }
        let mut mem = [0u8; 1024];
        let mut alloc = SlotAlloc::<_, Slot>::new(&mut mem);
        let slot_size = SlotAlloc::<&mut [u8], Slot>::slot_size();
        let total = SlotAlloc::<&mut [u8], Slot>::num_slots_total();
        assert_eq!(slot_size, 16);
        assert_eq!(total, 1024 / 16);
        assert!(alloc.has_free_slot());
        let slot = alloc.alloc_one().unwrap();
        let slot1 = alloc.alloc_one().unwrap();
        //assert!(alloc.dealloc(slot));
    }
}
