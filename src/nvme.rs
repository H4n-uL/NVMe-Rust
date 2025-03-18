use anyhow::{Result, anyhow};
use core::ops::Deref;
use core::sync::atomic::{AtomicU16, Ordering};

use crate::cmd::Command;
use crate::device::{Device, Doorbell};
use crate::memory::{Allocator, Dma};
use crate::queues::{CompQueue, SubQueue};

#[derive(Debug, Clone)]
pub struct Namespace {
    pub id: u32,
    pub block_count: u64,
    pub block_size: u64,
}

#[derive(Debug, Clone)]
pub struct IoQueueId(pub u16);

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

pub struct IoQueuePair<'a, A> {
    pub id: IoQueueId,
    pub buffer: Dma<u8>,
    pub device: &'a Device<A>,
    pub sub_queue: SubQueue,
    pub comp_queue: CompQueue,
}

impl<A: Allocator> IoQueuePair<'_, A> {
    fn submit_io(
        &mut self,
        namespace_id: u32,
        blocks: u64,
        lba: u64,
        addr: u64,
        write: bool,
    ) -> Result<()> {
        let command = Command::read_write(
            *self.id << 10 | self.sub_queue.tail as u16,
            namespace_id,
            lba,
            blocks as u16 - 1,
            [addr, 0],
            write,
        );

        let tail = self
            .sub_queue
            .try_push(command)
            .ok_or(anyhow!("Submission queue is full"))?;

        let doorbell = Doorbell::SubTail(*self.id);
        self.device.write_doorbell(doorbell, tail as u32);

        Ok(())
    }

    fn complete_io(&mut self, step: u64) -> Result<u16> {
        let (tail, entry) = self.comp_queue.pop_n(step as usize);

        let doorbell = Doorbell::CompHead(*self.id);
        self.device.write_doorbell(doorbell, tail as u32);

        let status = (entry.status >> 1) & 0xff;
        if status != 0 {
            anyhow::bail!("Command failed! Status: 0x{:x}", status);
        }

        Ok(entry.sq_head)
    }
}

impl<A: Allocator> IoQueuePair<'_, A> {
    pub fn write_copied(&mut self, data: &[u8], lba: u64) -> Result<()> {
        let ns_id = 1;
        // let ns = *self.namespaces.get(&ns_id).unwrap();
        for (offset, chunk) in data.chunks(4096).enumerate() {
            let blocks = (chunk.len() as u64).div_ceil(512);
            let current = lba + offset as u64;
            self.buffer.as_mut()[..chunk.len()].copy_from_slice(chunk);

            self.submit_io(ns_id, blocks, current, self.buffer.phys_addr as u64, true)?;
            self.sub_queue.head = self.complete_io(1)? as usize;
        }
        Ok(())
    }

    pub fn read_copied(&mut self, dest: &mut [u8], lba: u64) -> Result<()> {
        let ns_id = 1;
        // let ns = *self.namespaces.get(&ns_id).unwrap();
        for (offset, chunk) in dest.chunks_mut(4096).enumerate() {
            let blocks = (chunk.len() as u64).div_ceil(512);
            let current = lba + offset as u64;

            self.submit_io(ns_id, blocks, current, self.buffer.phys_addr as u64, false)?;
            self.sub_queue.head = self.complete_io(1)? as usize;

            chunk.copy_from_slice(&self.buffer.as_ref()[..chunk.len()]);
        }
        Ok(())
    }
}
