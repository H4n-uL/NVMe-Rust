use crate::error::{NvmeError, Result};
use alloc::{collections::vec_deque::VecDeque, vec::Vec};
use core::ops::{Deref, DerefMut};

/// Allocates physically contiguous memory mapped into virtual address space.
///
/// Used for DMA operations requiring contiguous physical memory.
pub trait NvmeAllocator {
    /// Translates a virtual address to a physical address.
    ///
    /// You may want to use your page table to translate the address
    /// instead of just subtracting an offset (e.g., `virt - HHDM_OFFSET`)
    /// if the address is allocated by a allocator based on virtual memory
    /// (e.g., kernel heap) rather than a frame allocator.
    fn translate(&self, addr: usize) -> usize;

    /// Allocates a `size` byte region of memory.
    ///
    /// Returns a virtual addresses of the allocated region's start.
    ///
    /// # Safety
    ///
    /// This is unsafe because:
    /// - Returns uninitialized memory
    /// - It must be a contiguous piece of memory at a physical address
    /// - It must be correctly mapped to virtual memory
    unsafe fn allocate(&self, size: usize) -> usize;

    /// Deallocates a previously allocated region of memory.
    ///
    /// The address must be the virtual address returned by `allocate`.
    ///
    /// # Safety
    ///
    /// This is unsafe because:
    /// - The memory should be returned by the allocator and not freed already
    unsafe fn deallocate(&self, addr: usize);
}

/// Represents a DMA (Direct Memory Access) buffer.
///
/// This structure is a wrapper for the generic type `T` and contains
/// a pointer to the virtual memory address of the allocated buffer
/// and the corresponding physical memory address.
///
/// The `T` stored in memory is page-aligned.
pub(crate) struct Dma<T> {
    pub addr: *mut T,
    pub phys_addr: usize,
}

impl<T> Deref for Dma<T> {
    type Target = T;

    /// Dereferences the DMA buffer to access the underlying value.
    ///
    /// # Safety
    /// This method assumes that the pointer is valid and properly aligned.
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.addr }
    }
}

impl<T> DerefMut for Dma<T> {
    /// Mutably dereferences the DMA buffer to access the underlying value.
    ///
    /// # Safety
    /// This method assumes that the pointer is valid and properly aligned.
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.addr }
    }
}

impl<T> Dma<T> {
    /// Allocates a new DMA buffer using the provided allocator.
    ///
    /// The allocated memory is page-aligned and sized to fit the type T,
    /// rounded up to the nearest page boundary.
    pub fn allocate<A: NvmeAllocator>(allocator: &A) -> Dma<T> {
        let addr = unsafe {
            let size = core::mem::size_of::<T>();
            allocator.allocate(size.div_ceil(4096) * 4096)
        } as *mut T;

        let phys_addr = allocator.translate(addr as usize);

        Self { addr, phys_addr }
    }

    /// Deallocates the DMA buffer using the provided allocator.
    ///
    /// # Safety
    ///
    /// This method assumes that the memory was allocated using the same allocator.
    /// After calling this method, the Dma instance should not be used anymore.
    pub fn deallocate<A: NvmeAllocator>(&self, allocator: &A) {
        unsafe {
            allocator.deallocate(self.addr as usize);
        }
    }
}

/// Represents the result of the creation of a PRP.
pub(crate) enum PrpResult {
    /// Address of PRP1
    Single(usize),
    /// Addresses of PRP1 and PRP2
    Double(usize, usize),
    /// Address of PRP1 and a list of PRP2s
    List(usize, Vec<Dma<[u64; 512]>>),
}

impl PrpResult {
    /// Get real address from the PRP result.
    ///
    /// Returns a tuple `(usize, usize)` containing the first and second PRP addresses.
    pub fn get_prp(&self) -> (usize, usize) {
        match self {
            Self::Single(prp) => (*prp, 0),
            Self::Double(prp1, prp2) => (*prp1, *prp2),
            Self::List(prp1, prp_lists) => (*prp1, prp_lists[0].phys_addr),
        }
    }
}

/// A simple fixed-size queue.
///
/// This queue is used to store PRP lists for reuse.
struct FixedSizeQueue<T> {
    queue: VecDeque<T>,
}

impl<T> FixedSizeQueue<T> {
    /// Creates a new `FixedSizeQueue`.
    fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::with_capacity(capacity),
        }
    }

    /// Checks if the queue is full.
    fn is_full(&self) -> bool {
        self.queue.len() == self.queue.capacity()
    }

    /// Pops an item from the queue.
    fn pop(&mut self) -> Option<T> {
        self.queue.pop_front()
    }

    /// Pushes an item into the queue.
    fn push(&mut self, item: T) -> Result<()> {
        if self.queue.len() < self.queue.capacity() {
            self.queue.push_back(item);
            Ok(())
        } else {
            Err(NvmeError::QueueFull)
        }
    }
}

/// Manages the creation and release of PRP results.
///
/// It will cache a number of PRP lists to avoid frequent allocations.
pub(crate) struct PrpManager {
    list_pool: FixedSizeQueue<Dma<[u64; 512]>>,
}

impl Default for PrpManager {
    /// Creates a new `PrpManager` with a default list pool size.
    ///
    /// The default size is 32, which can be adjusted based on the expected workload.
    fn default() -> Self {
        Self {
            list_pool: FixedSizeQueue::new(32),
        }
    }
}

impl PrpManager {
    /// Creates a PRP result for the given address and byte count.
    ///
    /// The NVMe controller will read or write data starting from this address directly.
    ///
    /// # Arguments
    ///
    /// The start address must be aligned to a 4-byte boundary in all situations.
    ///
    /// And it must be aligned to a page boundary if read or write
    /// more than a page (currently always 4096 bytes) because the NVMe controller
    /// reads or writes data in block size which will cause unexpected memory access.
    pub(crate) fn create<A: NvmeAllocator>(
        &mut self,
        allocator: &A,
        address: usize,
        bytes: usize,
    ) -> Result<PrpResult> {
        let count = ((address & 0xfff) + bytes).div_ceil(4096);

        let prp1 = allocator.translate(address);
        let prp2_start = allocator.translate(address + 4096);

        if (address & 0x3) != 0 {
            return Err(NvmeError::NotAlignedToDword);
        }
        if count == 1 {
            return Ok(PrpResult::Single(prp1));
        }
        if (address & 0xfff) != 0 {
            return Err(NvmeError::NotAlignedToPage);
        }
        if count == 2 {
            return Ok(PrpResult::Double(prp1, prp2_start));
        }

        let remaining = count - 1;
        let lists_needed = (remaining - 1).div_ceil(511);
        let mut prp_lists = Vec::with_capacity(lists_needed);

        for list_idx in 0..lists_needed {
            let entries = if list_idx == lists_needed - 1 {
                remaining - list_idx * 511
            } else {
                511
            };
            let mut prp_list = self
                .list_pool
                .pop()
                .unwrap_or_else(|| Dma::allocate(allocator));
            for i in 0..entries {
                prp_list[i] = (prp2_start + (list_idx * 511 + i) * 4096) as u64;
            }
            prp_lists.push(prp_list);
        }

        for index in 0..prp_lists.len() - 1 {
            prp_lists[index][511] = prp_lists[index + 1].phys_addr as u64;
        }

        Ok(PrpResult::List(prp1, prp_lists))
    }

    /// Releases the resources associated with a PRP result.
    ///
    /// All PRP results created by this manager should be released using this method.
    ///
    /// If the result contains PRP lists, it will attempt to return them to the
    /// list cache pool and if the pool is full, the lists will be deallocated.
    pub(crate) fn release<A: NvmeAllocator>(&mut self, prp_result: PrpResult, allocator: &A) {
        if let PrpResult::List(_, prp_lists) = prp_result {
            for prp in prp_lists {
                if self.list_pool.is_full() {
                    prp.deallocate(allocator);
                } else {
                    let _ = self.list_pool.push(prp);
                }
            }
        }
    }
}
