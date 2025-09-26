use core::hint::spin_loop;

use spin::Mutex;

use crate::cmd::Command;
use crate::error::{Error, Result};
use crate::memory::{Dma, Allocator};

/// Completion entry in the NVMe completion queue.
#[derive(Debug, Clone)]
#[repr(C, packed)]
pub(crate) struct Completion {
    pub command_specific: u32,
    _rsvd: u32,
    pub sq_head: u16,
    pub sq_id: u16,
    pub cmd_id: u16,
    pub status: u16,
}

/// Represents an NVMe submission queue.
///
/// The submission queue holds commands that are
/// waiting to be processed by the NVMe controller.
pub(crate) struct SubQueue {
    /// Queue state protected by mutex
    inner: Mutex<SubQueueInner>,
    /// Length of the queue
    len: usize,
}

struct SubQueueInner {
    /// The command slots
    slots: Dma<Command>,
    /// Current head position of the queue
    head: usize,
    /// Current tail position of the queue
    tail: usize,
}

impl SubQueue {
    /// Creates a new submission queue.
    ///
    /// The allocator should implement the `Allocator` trait.
    pub fn new<A: Allocator>(len: usize, allocator: &A) -> Self {
        Self {
            inner: Mutex::new(SubQueueInner {
                slots: Dma::allocate(len, allocator),
                head: 0,
                tail: 0,
            }),
            len,
        }
    }

    /// Returns the physical address of the submission queue.
    ///
    /// It is usually used to configure the admin queues.
    pub fn address(&self) -> usize {
        self.inner.lock().slots.phys_addr
    }

    /// Get current tail position (for admin commands)
    pub fn tail(&self) -> usize {
        self.inner.lock().tail
    }

    /// Set head position (from completion entry)
    pub fn set_head(&self, head: usize) {
        self.inner.lock().head = head;
    }

    /// Pushes a command to the submission queue
    ///
    /// It blocks until there is space available in the queue.
    pub fn push(&self, entry: Command) -> usize {
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
    pub fn try_push(&self, entry: Command) -> Result<usize> {
        let mut inner = self.inner.lock();
        if inner.head == (inner.tail + 1) % self.len {
            Err(Error::SubQueueFull)
        } else {
            let tail = inner.tail;
            inner.slots[tail] = entry;
            inner.tail = (inner.tail + 1) % self.len;
            Ok(inner.tail)
        }
    }
}

/// Represents an NVMe completion queue.
///
/// The completion queue holds completion entries that indicate the
/// status of processed commands from the submission queue.
pub(crate) struct CompQueue {
    /// Queue state protected by mutex
    inner: Mutex<CompQueueInner>,
    /// Length of the queue
    len: usize,
}

struct CompQueueInner {
    /// The completion slots
    slots: Dma<Completion>,
    /// Current head position of the queue
    head: usize,
    /// Used to determine if an entry is valid
    phase: bool,
}

impl CompQueue {
    /// Creates a new completion queue.
    ///
    /// The allocator should implement the `Allocator` trait.
    pub fn new<A: Allocator>(len: usize, allocator: &A) -> Self {
        Self {
            inner: Mutex::new(CompQueueInner {
                slots: Dma::allocate(len, allocator),
                head: 0,
                phase: true,
            }),
            len,
        }
    }

    /// Returns the physical address of the completion queue.
    ///
    /// It is usually used to configure the admin queues.
    pub fn address(&self) -> usize {
        self.inner.lock().slots.phys_addr
    }

    /// Pops a completion entry from the queue.
    ///
    /// It blocks until there is a valid entry available.
    pub fn pop(&self) -> (usize, Completion) {
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
    pub fn pop_n(&self, step: usize) -> (usize, Completion) {
        let mut inner = self.inner.lock();
        inner.head += step - 1;
        if inner.head >= self.len {
            inner.phase = !inner.phase;
        }
        inner.head %= self.len;
        drop(inner); // Release lock before calling pop()
        self.pop()
    }

    /// Attempts to pop a completion entry from the queue.
    ///
    /// It does not block if the queue is empty.
    /// If the entry is valid (based on the phase), it returns the entry
    /// with the new head position.
    pub fn try_pop(&self) -> Option<(usize, Completion)> {
        let mut inner = self.inner.lock();
        let entry_clone = inner.slots[inner.head].clone();
        let status = entry_clone.status;

        (((status & 1) == 1) == inner.phase).then(|| {
            inner.head = (inner.head + 1) % self.len;
            if inner.head == 0 {
                inner.phase = !inner.phase;
            }
            (inner.head, entry_clone)
        })
    }
}
