use core::hint::spin_loop;

use super::cmd::Command;
use super::memory::{Dma, NvmeAllocator};

#[derive(Debug, Clone)]
#[repr(C, packed)]
pub struct Completion {
    command_specific: u32,
    _rsvd: u32,
    pub sq_head: u16,
    sq_id: u16,
    cmd_id: u16,
    pub status: u16,
}

pub const QUEUE_LENGTH: usize = 64;

pub struct SubQueue {
    slots: Dma<[Command; QUEUE_LENGTH]>,
    pub head: usize,
    pub tail: usize,
    len: usize,
}

impl SubQueue {
    pub fn new<A: NvmeAllocator>(len: usize, allocator: &A) -> Self {
        Self {
            slots: Dma::allocate(allocator),
            head: 0,
            tail: 0,
            len: len.min(QUEUE_LENGTH),
        }
    }

    pub fn address(&self) -> usize {
        self.slots.phys_addr
    }

    pub fn is_full(&self) -> bool {
        self.head == (self.tail + 1) % self.len
    }

    pub fn push(&mut self, entry: Command) -> usize {
        self.slots[self.tail] = entry;
        self.tail = (self.tail + 1) % self.len;
        self.tail
    }

    pub fn try_push(&mut self, entry: Command) -> Option<usize> {
        if self.is_full() {
            None
        } else {
            Some(self.push(entry))
        }
    }
}

pub struct CompQueue {
    slots: Dma<[Completion; QUEUE_LENGTH]>,
    head: usize,
    phase: bool,
    len: usize,
}

impl CompQueue {
    pub fn new<A: NvmeAllocator>(len: usize, allocator: &A) -> Self {
        Self {
            slots: Dma::allocate(allocator),
            head: 0,
            phase: true,
            len: len.min(QUEUE_LENGTH),
        }
    }

    pub fn address(&self) -> usize {
        self.slots.phys_addr
    }

    pub fn pop(&mut self) -> (usize, Completion) {
        loop {
            if let Some(val) = self.try_pop() {
                return val;
            }
            spin_loop();
        }
    }

    pub fn pop_n(&mut self, commands: usize) -> (usize, Completion) {
        self.head += commands - 1;
        if self.head >= self.len {
            self.phase = !self.phase;
        }
        self.head %= self.len;
        self.pop()
    }

    pub fn try_pop(&mut self) -> Option<(usize, Completion)> {
        let entry = &self.slots[self.head];
        let phase_match = ((entry.status & 1) == 1) == self.phase;

        phase_match.then(|| {
            self.head = (self.head + 1) % self.len;
            if self.head == 0 {
                self.phase = !self.phase;
            }
            (self.head, entry.clone())
        })
    }
}
