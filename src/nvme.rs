use core::ops::Deref;
use core::sync::atomic::{AtomicU16, Ordering};

use alloc::rc::Rc;

use crate::cmd::Command;
use crate::device::{Doorbell, DoorbellHelper, NvmeNamespace};
use crate::error::{NvmeError, Result};
use crate::memory::{NvmeAllocator, PrpManager, PrpResult};
use crate::queues::{CompQueue, SubQueue};

#[derive(Debug, Clone)]
pub struct IoQueueId(pub u16);

impl Deref for IoQueueId {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl IoQueueId {
    pub fn new() -> Self {
        static NEXT_ID: AtomicU16 = AtomicU16::new(1);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

pub struct IoQueuePair<'a, A: NvmeAllocator> {
    pub(crate) id: IoQueueId,
    allocator: Rc<A>,
    namespace: &'a NvmeNamespace,
    doorbell_helper: DoorbellHelper,
    sub_queue: SubQueue,
    comp_queue: CompQueue,
    prp_manager: PrpManager,
    max_transfer_size: u64,
}

impl<'a, A: NvmeAllocator> IoQueuePair<'a, A> {
    pub(crate) fn new(
        id: IoQueueId,
        namespace: &'a NvmeNamespace,
        doorbell_helper: DoorbellHelper,
        sub_queue: SubQueue,
        comp_queue: CompQueue,
        allocator: Rc<A>,
        max_transfer_size: u64,
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

impl<A: NvmeAllocator> IoQueuePair<'_, A> {
    fn submit_io(
        &mut self,
        blocks: u16,
        lba: u64,
        address: usize,
        write: bool,
    ) -> Result<PrpResult> {
        let bytes = blocks as usize * 512;

        let prp_result = self
            .prp_manager
            .create(self.allocator.as_ref(), address, bytes)?;

        let prp = prp_result.get_prp();

        let command = Command::read_write(
            *self.id << 10 | self.sub_queue.tail as u16,
            self.namespace.id,
            lba,
            blocks - 1,
            [prp.0 as u64, prp.1 as u64],
            write,
        );

        let tail = self.sub_queue.try_push(command)?;

        let doorbell = Doorbell::SubTail(*self.id);
        self.doorbell_helper.write(doorbell, tail as u32);

        Ok(prp_result)
    }

    fn complete_io(&mut self, step: u64) -> Result<u16> {
        let (tail, entry) = self.comp_queue.pop_n(step as usize);

        let doorbell = Doorbell::CompHead(*self.id);
        self.doorbell_helper.write(doorbell, tail as u32);

        let status = (entry.status >> 1) & 0xff;
        if status != 0 {
            return Err(NvmeError::CommandFailed(status));
        }

        Ok(entry.sq_head)
    }
}

impl<A: NvmeAllocator> IoQueuePair<'_, A> {
    fn handle_read_write(
        &mut self,
        bytes: u64,
        lba: u64,
        address: usize,
        write: bool,
    ) -> Result<()> {
        if bytes > self.max_transfer_size {
            return Err(NvmeError::IoSizeExceedsMdts);
        }
        if bytes % self.namespace.block_size != 0 {
            return Err(NvmeError::InvalidBufferSize);
        }

        let blocks = (bytes / self.namespace.block_size) as u16;
        let prp_result = self.submit_io(blocks, lba, address, write)?;
        self.sub_queue.head = self.complete_io(1)? as usize;
        self.prp_manager
            .release(prp_result, self.allocator.as_ref());

        Ok(())
    }
}

impl<A: NvmeAllocator> IoQueuePair<'_, A> {
    pub fn read(&mut self, dest: *mut u8, bytes: usize, lba: u64) -> Result<()> {
        self.handle_read_write(bytes as u64, lba, dest as usize, false)
    }

    pub fn write(&mut self, src: *const u8, bytes: usize, lba: u64) -> Result<()> {
        self.handle_read_write(bytes as u64, lba, src as usize, true)
    }
}
