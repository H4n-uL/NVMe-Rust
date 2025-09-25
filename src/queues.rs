use core::hint::spin_loop;

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
    /// The command slots
    slots: Dma<Command>,
    /// Current head position of the queue
    pub head: usize,
    /// Current tail position of the queue
    pub tail: usize,
    /// Length of the queue
    len: usize,
}

impl SubQueue {
    /// Creates a new submission queue.
    ///
    /// The allocator should implement the `Allocator` trait.
    pub fn new<A: Allocator>(len: usize, allocator: &A) -> Self {
        Self {
            slots: Dma::allocate(len, allocator),
            head: 0,
            tail: 0,
            len,
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
        if self.head == (self.tail + 1) % self.len {
            Err(Error::SubQueueFull)
        } else {
            self.slots[self.tail] = entry;
            self.tail = (self.tail + 1) % self.len;
            Ok(self.tail)
        }
    }
}

/// Represents an NVMe completion queue.
///
/// The completion queue holds completion entries that indicate the
/// status of processed commands from the submission queue.
pub(crate) struct CompQueue {
    /// The completion slots
    slots: Dma<Completion>,
    /// Current head position of the queue
    head: usize,
    /// Used to determine if an entry is valid
    phase: bool,
    /// Length of the queue
    len: usize,
}

impl CompQueue {
    /// Creates a new completion queue.
    ///
    /// The allocator should implement the `Allocator` trait.
    pub fn new<A: Allocator>(len: usize, allocator: &A) -> Self {
        Self {
            slots: Dma::allocate(len, allocator),
            head: 0,
            phase: true,
            len,
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
        if self.head >= self.len {
            self.phase = !self.phase;
        }
        self.head %= self.len;
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
            self.head = (self.head + 1) % self.len;
            if self.head == 0 {
                self.phase = !self.phase;
            }
            (self.head, entry.clone())
        })
    }
}
