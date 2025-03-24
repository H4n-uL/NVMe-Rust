use core::hint::spin_loop;

use crate::cmd::Command;
use crate::error::{NvmeError, Result};
use crate::memory::{Dma, NvmeAllocator};

/// Completion entry in the NVMe completion queue.
#[derive(Debug, Clone)]
#[repr(C, packed)]
pub(crate) struct Completion {
    command_specific: u32,
    _rsvd: u32,
    pub sq_head: u16,
    sq_id: u16,
    cmd_id: u16,
    pub status: u16,
}

/// Maximum length of a NVMe queue.
///
/// Here we choose 4096, which is the maximum length of a admin
/// queue for simplification (I/O queue can be 65536 at most).
pub const MAX_QUEUE_LENGTH: usize = 4096;

/// Macro to assert the size of the queue.
///
/// This macro checks if the size of the queue is
/// smaller than 2 or larger than the maximum queue length
/// and panic if the size is invalid.
macro_rules! assert_size {
    ($name:ident) => {
        struct Assert<const N: usize>;
        impl<const N: usize> Assert<N> {
            const ASSERT_MIN: () = assert!(N >= 2, "queue size cannot be smaller than 2");
            const ASSERT_MAX: () = assert!(
                N <= MAX_QUEUE_LENGTH,
                "queue size cannot be larger than MAX_QUEUE_LENGTH(4096)"
            );
        }
        _ = Assert::<$name>::ASSERT_MAX;
        _ = Assert::<$name>::ASSERT_MIN;
    };
}

/// Represents an NVMe submission queue.
///
/// The submission queue holds commands that are
/// waiting to be processed by the NVMe controller.
///
/// # Const Generics
///
/// The `SIZE` parameter is a const generic that specifies the size
/// of the queue. It must be between 2 and `MAX_QUEUE_LENGTH(4096)`.
/// It will panic at compile time if the size is invalid.
///
/// Notice that the DMA allocation is fit to page size, so the
/// actual minimum size of the submission queue is 64 (4096 bytes).
pub(crate) struct SubQueue<const SIZE: usize> {
    /// The command slots
    slots: Dma<[Command; SIZE]>,
    /// Current head position of the queue
    pub head: usize,
    /// Current tail position of the queue
    pub tail: usize,
}

impl<const SIZE: usize> SubQueue<SIZE> {
    /// Creates a new submission queue.
    ///
    /// The allocator should implement the `NvmeAllocator` trait.
    pub fn new<A: NvmeAllocator>(allocator: &A) -> Self {
        assert_size!(SIZE);
        Self {
            slots: Dma::allocate(allocator),
            head: 0,
            tail: 0,
        }
    }

    /// Returns the physical address of the submission queue.
    ///
    /// It is usually used to configure the admin queues.
    pub fn address(&self) -> usize {
        self.slots.phys_addr
    }

    /// Pushes a command to the submission queue
    ///
    /// It blocks until there is space available in the queue.
    pub fn push(&mut self, entry: Command) -> usize {
        loop {
            if let Ok(tail) = self.try_push(entry) {
                return tail;
            }
            spin_loop();
        }
    }

    /// Attempts to push a command to the submission queue.
    ///
    /// It does not block if the queue is full.
    pub fn try_push(&mut self, entry: Command) -> Result<usize> {
        if self.head == (self.tail + 1) % SIZE {
            Err(NvmeError::QueueFull)
        } else {
            self.slots[self.tail] = entry;
            self.tail = (self.tail + 1) % SIZE;
            Ok(self.tail)
        }
    }
}

/// Represents an NVMe completion queue.
///
/// The completion queue holds completion entries that indicate the
/// status of processed commands from the submission queue.
///
/// # Const Generics
///
/// The `SIZE` parameter is a const generic that specifies the size
/// of the queue. It must be between 2 and `MAX_QUEUE_LENGTH(4096)`.
/// It will panic at compile time if the size is invalid.
///
/// Notice that the DMA allocation is fit to page size, so the
/// actual minimum size of the completion queue is 256 (4096 bytes).
/// However, the size of the completion queue should be same as
/// the submission queue, so the minimum size is 64 either.
pub(crate) struct CompQueue<const SIZE: usize> {
    /// The completion slots
    slots: Dma<[Completion; SIZE]>,
    /// Current head position of the queue
    head: usize,
    /// Used to determine if an entry is valid
    phase: bool,
}

impl<const SIZE: usize> CompQueue<SIZE> {
    /// Creates a new completion queue.
    ///
    /// The allocator should implement the `NvmeAllocator` trait.
    pub fn new<A: NvmeAllocator>(allocator: &A) -> Self {
        assert_size!(SIZE);
        Self {
            slots: Dma::allocate(allocator),
            head: 0,
            phase: true,
        }
    }

    /// Returns the physical address of the submission queue.
    ///
    /// It is usually used to configure the admin queues.
    pub fn address(&self) -> usize {
        self.slots.phys_addr
    }

    /// Pops a completion entry from the queue.
    ///
    /// It blocks until there is a valid entry available.
    pub fn pop(&mut self) -> (usize, Completion) {
        loop {
            if let Some(val) = self.try_pop() {
                return val;
            }
            spin_loop();
        }
    }

    /// Pops a step of completion entries from the queue.
    ///
    /// It returns the final head position and the completion entry.
    pub fn pop_n(&mut self, step: usize) -> (usize, Completion) {
        self.head += step - 1;
        if self.head >= SIZE {
            self.phase = !self.phase;
        }
        self.head %= SIZE;
        self.pop()
    }

    /// Attempts to pop a completion entry from the queue.
    ///
    /// It does not block if the queue is empty.
    /// If the entry is valid (based on the phase), it returns the entry
    /// with the new head position.
    pub fn try_pop(&mut self) -> Option<(usize, Completion)> {
        let entry = &self.slots[self.head];

        (((entry.status & 1) == 1) == self.phase).then(|| {
            self.head = (self.head + 1) % SIZE;
            if self.head == 0 {
                self.phase = !self.phase;
            }
            (self.head, entry.clone())
        })
    }
}
