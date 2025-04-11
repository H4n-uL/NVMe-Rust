use alloc::sync::Arc;
use core::ops::Deref;
use core::sync::atomic::{AtomicU16, Ordering};

use crate::cmd::Command;
use crate::device::{Doorbell, DoorbellHelper, Namespace};
use crate::error::{Error, Result};
use crate::memory::{Allocator, PrpManager, PrpResult};
use crate::queues::{CompQueue, SubQueue};

/// A unique identifier for an I/O queue.
///
/// It self-increments starting from 1 and add each time
/// a new queue is created. The 0 is reserved for the admin queue pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct IoQueueId(u16);

impl Deref for IoQueueId {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[allow(clippy::new_without_default)]
impl IoQueueId {
    pub fn new() -> Self {
        static NEXT_ID: AtomicU16 = AtomicU16::new(1);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// A queue pair for handling NVMe I/O operations.
///
/// All your I/O operations should be done through this queue pair.
pub struct IoQueuePair<A: Allocator> {
    id: IoQueueId,
    allocator: Arc<A>,
    namespace: Namespace,
    doorbell_helper: DoorbellHelper,
    sub_queue: SubQueue,
    comp_queue: CompQueue,
    prp_manager: PrpManager,
    max_transfer_size: usize,
}

impl<A: Allocator> IoQueuePair<A> {
    pub(crate) fn new(
        id: IoQueueId,
        namespace: Namespace,
        doorbell_helper: DoorbellHelper,
        sub_queue: SubQueue,
        comp_queue: CompQueue,
        allocator: Arc<A>,
        max_transfer_size: usize,
    ) -> Self {
        Self {
            id,
            namespace,
            doorbell_helper,
            sub_queue,
            comp_queue,
            prp_manager: Default::default(),
            allocator,
            max_transfer_size,
        }
    }
}

impl<A: Allocator> IoQueuePair<A> {
    fn submit_io(
        &mut self,
        bytes: usize,
        lba: u64,
        address: usize,
        write: bool,
    ) -> Result<PrpResult> {
        let prp_result = self
            .prp_manager
            .create(self.allocator.as_ref(), address, bytes)?;

        let prp = prp_result.get_prp();
        let blocks = bytes as u64 / self.namespace.block_size();

        let command = Command::read_write(
            *self.id << 10 | self.sub_queue.tail as u16,
            self.namespace.id(),
            lba,
            blocks as u16 - 1,
            [prp.0 as u64, prp.1 as u64],
            write,
        );

        let tail = self.sub_queue.try_push(command)?;
        self.doorbell_helper
            .write(Doorbell::SubTail(*self.id), tail as u32);

        Ok(prp_result)
    }

    fn complete_io(&mut self, step: u64) -> Result<u16> {
        let (tail, entry) = self.comp_queue.pop_n(step as usize);
        self.doorbell_helper
            .write(Doorbell::CompHead(*self.id), tail as u32);

        let status = (entry.status >> 1) & 0xff;
        if status != 0 {
            return Err(Error::CommandFailed(status));
        }

        Ok(entry.sq_head)
    }
}

impl<A: Allocator> IoQueuePair<A> {
    fn handle_read_write(
        &mut self,
        bytes: usize,
        lba: u64,
        address: usize,
        write: bool,
    ) -> Result<()> {
        if bytes > self.max_transfer_size {
            return Err(Error::IoSizeExceedsMdts);
        }
        if bytes as u64 % self.namespace.block_size() != 0 {
            return Err(Error::InvalidBufferSize);
        }

        let prp_result = self.submit_io(bytes, lba, address, write)?;
        self.sub_queue.head = self.complete_io(1)? as usize;
        self.prp_manager
            .release(prp_result, self.allocator.as_ref());

        Ok(())
    }
}

impl<A: Allocator> IoQueuePair<A> {
    /// Returns the queue pair ID.
    ///
    /// This ID is globally unique as it is a static counter.
    pub fn id(&self) -> IoQueueId {
        self.id
    }

    /// Reads bytes from given LBA to the destination address.
    ///
    /// This function will block and the queue depth is always 1.
    pub fn read(&mut self, dest: *mut u8, bytes: usize, lba: u64) -> Result<()> {
        self.handle_read_write(bytes, lba, dest as usize, false)
    }

    /// Writes bytes from the source address to the given LBA.
    ///
    /// This function will block and the queue depth is always 1.
    pub fn write(&mut self, src: *const u8, bytes: usize, lba: u64) -> Result<()> {
        self.handle_read_write(bytes, lba, src as usize, true)
    }
}
