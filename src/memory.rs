use core::ops::{Deref, DerefMut};
use core::slice::{from_raw_parts, from_raw_parts_mut};

pub struct Dma<T> {
    count: usize,
    pub addr: *mut T,
    pub phys_addr: usize,
}

impl<T> Deref for Dma<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.addr }
    }
}

impl<T> DerefMut for Dma<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.addr }
    }
}

impl AsRef<[u8]> for Dma<u8> {
    fn as_ref(&self) -> &[u8] {
        unsafe { from_raw_parts(self.addr, self.count) }
    }
}

impl AsMut<[u8]> for Dma<u8> {
    fn as_mut(&mut self) -> &mut [u8] {
        unsafe { from_raw_parts_mut(self.addr, self.count) }
    }
}

/// Allocates physically contiguous memory mapped into virtual address space.
///
/// Used for DMA operations requiring contiguous physical memory.
pub trait Allocator {
    /// Allocates a `size` byte region of memory.
    ///
    /// Returns a tuple of physical and virtual addresses of the allocated region's start.
    ///
    /// # Safety
    ///
    /// This is unsafe because:
    /// - Returns uninitialized memory
    /// - Implementation must ensure physical contiguity and valid virtual mapping
    unsafe fn allocate(&self, size: usize) -> (usize, usize);
}

impl<T> Dma<T> {
    pub fn allocate<A: Allocator>(allocator: &A, count: usize) -> Dma<T> {
        let (phys, virt) = unsafe {
            let size = core::mem::size_of::<T>() * count;
            allocator.allocate(size.div_ceil(4096) * 4096)
        };

        Self {
            count,
            phys_addr: phys,
            addr: virt as *mut T,
        }
    }
}

// pub trait DmaSlice: AsRef<[u8]> + AsMut<[u8]> {
//     fn chunks(&self, bytes: usize) -> impl Iterator<Item = (&[u8], usize)>;
// }

// impl DmaSlice for Dma<u8> {
//     fn chunks(&self, bytes: usize) -> impl Iterator<Item = (&[u8], usize)> {
//         let addr = self.addr.cast_const();
//         (0..self.count).step_by(bytes).map(move |offset| {
//             let len = core::cmp::min(bytes, self.count - offset);
//             let slice = unsafe { from_raw_parts(addr.add(offset), len) };
//             (slice, self.phys_addr + offset)
//         })
//     }
// }
