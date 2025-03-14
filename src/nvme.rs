use core::hint::spin_loop;

use alloc::collections::btree_map::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use anyhow::{Result, anyhow};

use crate::cmd::{IdentifyType, QueueType};
use crate::memory::DmaSlice;
use crate::queues::{CompQueue, Completion};
use crate::queues::{QUEUE_LENGTH, SubQueue};

use super::cmd::Command;
use super::memory::{Allocator, Dma};

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct Namespace {
    pub id: u32,
    pub block_count: u64,
    pub block_size: u64,
}

#[derive(Debug, Default)]
pub struct Stats {
    completions: u64,
    submissions: u64,
}

#[derive(Debug)]
pub enum Register {
    /// Controller Capabilities
    CAP = 0x0,
    /// Version
    VS = 0x8,
    /// Interrupt Mask Set
    INTMS = 0xC,
    /// Interrupt Mask Clear
    INTMC = 0x10,
    /// Controller Configuration
    CC = 0x14,
    /// Controller Status
    CSTS = 0x1C,
    /// NVM Subsystem Reset
    NSSR = 0x20,
    /// Admin Queue Attributes
    AQA = 0x24,
    /// Admin Submission Queue Base Address
    ASQ = 0x28,
    /// Admin Completion Queue Base Address
    ACQ = 0x30,
}

#[derive(Clone, Debug)]
pub enum Doorbell {
    SubTail(u16),
    CompHead(u16),
}

#[derive(Debug, Clone)]
#[repr(C, packed)]
struct NamespaceData {
    _ignore1: u64,
    capacity: u64,
    _ignore2: [u8; 10],
    lba_size: u8,
    _ignore3: [u8; 101],
    lba_format_support: [u32; 16],
}

pub struct QueuePair<'a, A> {
    id: u16,
    device: &'a Device<A>,
    sub_queue: SubQueue,
    comp_queue: CompQueue,
}

impl<A: Allocator> QueuePair<'_, A> {
    pub fn quick_poll(&mut self) -> Result<()> {
        if let Some((tail, c_entry)) = self.comp_queue.try_pop() {
            self.device
                .write_doorbell(Doorbell::CompHead(self.id), tail as u32);
            self.sub_queue.head = c_entry.sq_head as usize;

            let status = c_entry.status >> 1;
            if status != 0 {
                anyhow::bail!(
                    "Status: 0x{:x}, Code 0x{:x}, Type: 0x{:x}",
                    status,
                    status & 0xFF,
                    (status >> 8) & 0x7
                );
            }
        }
        Ok(())
    }

    pub fn complete_io(&mut self, n: usize) -> Result<u16> {
        let (tail, c_entry) = self.comp_queue.pop_n(n);
        self.device
            .write_doorbell(Doorbell::CompHead(self.id), tail as u32);
        self.sub_queue.head = c_entry.sq_head as usize;

        let status = c_entry.status >> 1;
        if status != 0 {
            anyhow::bail!(
                "Status: 0x{:x}, Code 0x{:x}, Type: 0x{:x}",
                status,
                status & 0xFF,
                (status >> 8) & 0x7
            );
        }
        Ok(c_entry.sq_head)
    }

    pub fn submit_io(&mut self, data: &impl DmaSlice, mut lba: u64, write: bool) -> Result<usize> {
        let mut reqs = 0;

        for (chunk, phys_addr) in data.chunks(4096) {
            let blocks = (chunk.len() as u64).div_ceil(512);
            let addr = phys_addr as u64;

            let ptr1 = match chunk.len() {
                p if p <= 4096 => 0,
                _ => addr + 4096,
            };

            let entry = Command::read_write(
                self.id << 11 | self.sub_queue.tail as u16,
                1,
                lba,
                blocks as u16 - 1,
                [addr, ptr1],
                write,
            );

            match self.sub_queue.try_push(entry) {
                Some(tail) => {
                    self.device
                        .write_doorbell(Doorbell::SubTail(self.id), tail as u32);
                    lba += blocks;
                    reqs += 1;
                }
                None => {
                    anyhow::bail!("Queue full");
                }
            }
        }

        Ok(reqs)
    }
}

pub struct Device<A> {
    address: *mut u8,
    buffer: Dma<u8>,
    prp_list: Dma<[u64; 512]>,
    namespaces: BTreeMap<u32, Namespace>,
    stats: Stats,
    queue_id: u16,
    allocator: A,
    doorbell_stride: u16,
    admin_sq: SubQueue,
    admin_cq: CompQueue,
    io_sq: SubQueue,
    io_cq: CompQueue,
}

unsafe impl<A> Send for Device<A> {}
unsafe impl<A> Sync for Device<A> {}

impl<A: Allocator> Device<A> {
    pub fn init(address: usize, allocator: A) -> Result<Self> {
        let mut device = Self {
            address: address as _,
            queue_id: 1,
            buffer: Dma::allocate(&allocator, 4096),
            prp_list: Dma::allocate(&allocator, 1),
            namespaces: BTreeMap::new(),
            stats: Stats::default(),
            doorbell_stride: 0,
            admin_sq: SubQueue::new(QUEUE_LENGTH, &allocator),
            admin_cq: CompQueue::new(QUEUE_LENGTH, &allocator),
            io_sq: SubQueue::new(QUEUE_LENGTH, &allocator),
            io_cq: CompQueue::new(QUEUE_LENGTH, &allocator),
            allocator,
        };

        let cap = device.get_reg::<u64>(Register::CAP) >> 32;
        device.doorbell_stride = cap as u16 & 0xF;

        device.init_registers();
        device.init_io_queues()?;
        device.queue_id += 1;

        Ok(device)
    }

    fn init_io_queues(&mut self) -> Result<()> {
        self.exec_command(Command::create_queue(
            self.admin_sq.tail as u16,
            self.queue_id,
            self.io_cq.address(),
            (QUEUE_LENGTH - 1) as u16,
            QueueType::Completion,
            None,
        ))?;
        self.exec_command(Command::create_queue(
            self.admin_sq.tail as u16,
            self.queue_id,
            self.io_sq.address(),
            (QUEUE_LENGTH - 1) as u16,
            QueueType::Submission,
            Some(self.queue_id),
        ))?;
        Ok(())
    }

    fn init_registers(&mut self) {
        let prp_base = self.buffer.phys_addr + 4096;
        for (index, prp) in self.prp_list.iter_mut().enumerate() {
            *prp = (prp_base + index * 4096) as u64;
        }

        self.set_reg::<u32>(Register::CC, self.get_reg::<u32>(Register::CC) & !1);
        while self.get_reg::<u32>(Register::CSTS) & 1 == 1 {
            spin_loop();
        }

        // Configure Admin Queues
        self.set_reg::<u64>(Register::ASQ, self.admin_sq.address() as u64);
        self.set_reg::<u64>(Register::ACQ, self.admin_cq.address() as u64);
        let aqa = (QUEUE_LENGTH as u32 - 1) << 16 | (QUEUE_LENGTH as u32 - 1);
        self.set_reg::<u32>(Register::AQA, aqa);

        let cc = self.get_reg::<u32>(Register::CC) & 0xFF00_000F;
        self.set_reg::<u32>(Register::CC, cc | (4 << 20) | (6 << 16));

        self.set_reg::<u32>(Register::CC, self.get_reg::<u32>(Register::CC) | 1);
        while self.get_reg::<u32>(Register::CSTS) & 1 == 0 {
            spin_loop();
        }
    }
}

impl<A: Allocator> Device<A> {
    fn get_reg<T>(&self, reg: Register) -> T {
        let address = self.address as usize + reg as usize;
        unsafe { (address as *const T).read_volatile() }
    }

    fn set_reg<T>(&self, reg: Register, value: T) {
        let address = self.address as usize + reg as usize;
        unsafe { (address as *mut T).write_volatile(value) }
    }
}

impl<A: Allocator> Device<A> {
    pub fn identify_namespace(&mut self, id: u32) -> Result<Namespace> {
        self.exec_command(Command::identify(
            self.admin_sq.tail as u16,
            self.buffer.phys_addr,
            IdentifyType::Namespace(id),
        ))?;

        let data = unsafe { &*(self.buffer.addr as *const NamespaceData) };
        let flba_index = (data.lba_size & 0xF) as usize;
        let flba_data = (data.lba_format_support[flba_index] >> 16) & 0xFF;

        let block_size = match flba_data {
            9..=32 => 1 << flba_data,
            _ => anyhow::bail!("Block size < 512B is not supported"),
        };

        let namespace = Namespace {
            id,
            block_size,
            block_count: data.capacity,
        };
        self.namespaces.insert(id, namespace);

        Ok(namespace)
    }

    pub fn identify_controller(&mut self) -> Result<(String, String, String)> {
        self.exec_command(Command::identify(
            self.admin_sq.tail as u16,
            self.buffer.phys_addr,
            IdentifyType::Controller,
        ))?;

        let extract_string = |start: usize, end: usize| -> String {
            self.buffer.as_ref()[start..end]
                .iter()
                .take_while(|&&b| b != 0)
                .map(|&b| b as char)
                .collect::<String>()
                .trim()
                .to_string()
        };

        let serial = extract_string(4, 24);
        let model = extract_string(24, 64);
        let firmware = extract_string(64, 72);

        Ok((model, serial, firmware))
    }

    pub fn identify_namespace_list(&mut self, base: u32) -> Result<Vec<u32>> {
        self.exec_command(Command::identify(
            self.admin_sq.tail as u16,
            self.buffer.phys_addr,
            IdentifyType::NamespaceList(base),
        ))?;

        let data = unsafe {
            core::slice::from_raw_parts(
                self.buffer.addr as *const u32,
                4096 / core::mem::size_of::<u32>(),
            )
        };

        Ok(data.iter().take_while(|&&id| id != 0).copied().collect())
    }
}

impl<A: Allocator> Device<A> {
    fn get_doorbell(&self, bell: Doorbell) -> *mut u32 {
        let stride = 4 << self.doorbell_stride;
        let base = self.address as usize + 0x1000;
        let index = match bell {
            Doorbell::SubTail(qid) => qid * 2,
            Doorbell::CompHead(qid) => qid * 2 + 1,
        };
        (base + (index * stride) as usize) as *mut u32
    }

    fn write_doorbell(&self, bell: Doorbell, val: u32) {
        unsafe {
            self.get_doorbell(bell).write_volatile(val);
        }
    }
}

impl<A: Allocator> Device<A> {
    pub fn exec_command(&mut self, cmd: Command) -> Result<Completion> {
        let tail = self.admin_sq.push(cmd);
        self.write_doorbell(Doorbell::SubTail(0), tail as u32);

        let (head, entry) = self.admin_cq.pop();
        self.write_doorbell(Doorbell::CompHead(0), head as u32);

        let status = entry.status >> 1;
        if status != 0 {
            anyhow::bail!(
                "Status: 0x{:x}, Code: 0x{:x}, Type: 0x{:x}",
                status,
                status & 0xFF,
                (status >> 8) & 0x7
            );
        }

        Ok(entry)
    }
}

impl<A: Allocator> Device<A> {
    pub fn write(&mut self, data: &impl DmaSlice, lba: u64) -> Result<()> {
        let ns_id = 1;
        let ns = *self.namespaces.get(&ns_id).unwrap();
        for (offset, chunk) in data.chunks(ns.block_size as usize).enumerate() {
            let current = lba + offset as u64;
            self.namespace_io(ns_id, 1, current, chunk.1 as u64, true)?;
        }
        Ok(())
    }

    pub fn read(&mut self, dest: &impl DmaSlice, lba: u64) -> Result<()> {
        let ns_id = 1;
        let ns = *self.namespaces.get(&ns_id).unwrap();
        for (offset, chunk) in dest.chunks(ns.block_size as usize).enumerate() {
            let current = lba + offset as u64;
            self.namespace_io(ns_id, 1, current, chunk.1 as u64, false)?;
        }
        Ok(())
    }

    pub fn write_copied(&mut self, data: &[u8], lba: u64) -> Result<()> {
        let ns_id = 1;
        let ns = *self.namespaces.get(&ns_id).unwrap();
        for (offset, chunk) in data.chunks(4096).enumerate() {
            let blocks = (chunk.len() as u64).div_ceil(ns.block_size);
            let current = lba + offset as u64;
            self.buffer.as_mut()[..chunk.len()].copy_from_slice(chunk);
            self.namespace_io(ns_id, blocks, current, self.buffer.phys_addr as u64, true)?;
        }
        Ok(())
    }

    pub fn read_copied(&mut self, dest: &mut [u8], lba: u64) -> Result<()> {
        let ns_id = 1;
        let ns = *self.namespaces.get(&ns_id).unwrap();
        for (offset, chunk) in dest.chunks_mut(4096).enumerate() {
            let blocks = (chunk.len() as u64).div_ceil(ns.block_size);
            let current = lba + offset as u64;
            self.namespace_io(ns_id, blocks, current, self.buffer.phys_addr as u64, false)?;
            chunk.copy_from_slice(&self.buffer.as_ref()[..chunk.len()]);
        }
        Ok(())
    }
}

impl<A: Allocator> Device<A> {
    fn submit_io(
        &mut self,
        ns: &Namespace,
        addr: u64,
        blocks: u64,
        lba: u64,
        write: bool,
    ) -> Option<usize> {
        assert!(blocks > 0 && blocks <= 0x1_0000);

        let bytes = blocks * ns.block_size;
        let ptr1 = match bytes {
            0..=4096 => 0,
            4097..=8192 => addr + 4096,
            _ => {
                let offset = (addr - self.buffer.phys_addr as u64) / 8;
                self.prp_list.phys_addr as u64 + offset
            }
        };

        let entry = Command::read_write(
            self.io_sq.tail as u16,
            ns.id,
            lba,
            blocks as u16 - 1,
            [addr, ptr1],
            write,
        );

        self.io_sq.try_push(entry)
    }

    fn complete_io(&mut self, step: u64) -> Result<u16> {
        let queue_id = 1;

        let (tail, c_entry) = self.io_cq.pop_n(step as usize);
        self.write_doorbell(Doorbell::CompHead(queue_id), tail as u32);

        let status = c_entry.status >> 1;
        if status != 0 {
            anyhow::bail!(
                "Status: 0x{:x}, Code 0x{:x}, Type: 0x{:x}",
                status,
                status & 0xFF,
                (status >> 8) & 0x7
            );
        }
        self.stats.completions += 1;
        Ok(c_entry.sq_head)
    }

    fn namespace_io(
        &mut self,
        namespace_id: u32,
        blocks: u64,
        lba: u64,
        addr: u64,
        write: bool,
    ) -> Result<()> {
        let queue_id = 1;
        let bytes = blocks * 512;

        let ptr1 = match bytes {
            b if b <= 4096 => 0,
            b if b <= 8192 => addr + 4096,
            _ => self.prp_list.phys_addr as u64,
        };

        let entry = Command::read_write(
            self.io_sq.tail as u16,
            namespace_id,
            lba,
            blocks as u16 - 1,
            [addr, ptr1],
            write,
        );

        let tail = self.io_sq.push(entry);
        self.stats.submissions += 1;

        self.write_doorbell(Doorbell::SubTail(queue_id), tail as u32);
        self.io_sq.head = self.complete_io(1)? as usize;

        Ok(())
    }
}

impl<A: Allocator> Device<A> {
    pub fn batched_write(
        &mut self,
        namespace_id: u32,
        data: &[u8],
        mut lba: u64,
        batch_len: u64,
    ) -> Result<()> {
        let q_id = 1;
        let ns = *self.namespaces.get(&namespace_id).unwrap();
        for chunk in data.chunks(4096) {
            self.buffer.as_mut()[..chunk.len()].copy_from_slice(chunk);
            self.process_io_batch(&ns, q_id, chunk.len(), batch_len, &mut lba, true)?;
        }
        Ok(())
    }

    pub fn batched_read(
        &mut self,
        namespace_id: u32,
        data: &mut [u8],
        mut lba: u64,
        batch_len: u64,
    ) -> Result<()> {
        let q_id = 1;
        let ns = *self.namespaces.get(&namespace_id).unwrap();
        for chunk in data.chunks_mut(4096) {
            self.process_io_batch(&ns, q_id, chunk.len(), batch_len, &mut lba, false)?;
            chunk.copy_from_slice(&self.buffer.as_ref()[..chunk.len()]);
        }
        Ok(())
    }

    fn process_io_batch(
        &mut self,
        ns: &Namespace,
        queue_id: u16,
        chunk_len: usize,
        batch_len: u64,
        lba: &mut u64,
        is_write: bool,
    ) -> Result<()> {
        let block_size = ns.block_size;
        let batch_len = batch_len.min(chunk_len as u64 / block_size);
        let batch_size = chunk_len as u64 / batch_len;
        let blocks = batch_size / block_size;

        for index in 0..batch_len {
            let addr = self.buffer.phys_addr as u64 + index * batch_size;
            let tail = self
                .submit_io(ns, addr, blocks, *lba, is_write)
                .ok_or(anyhow!("Failed to submit IO command"))?;

            self.stats.submissions += 1;
            self.write_doorbell(Doorbell::SubTail(queue_id), tail as u32);
            *lba += blocks;
        }

        self.io_sq.head = self.complete_io(batch_len)? as usize;
        Ok(())
    }
}

impl<A: Allocator> Device<A> {
    pub fn create_io_queue_pair(&mut self, len: usize) -> Result<QueuePair<A>> {
        let comp_queue = CompQueue::new(len, &self.allocator);
        self.exec_command(Command::create_queue(
            self.admin_sq.tail as u16,
            self.queue_id,
            comp_queue.address(),
            (len - 1) as u16,
            QueueType::Completion,
            None,
        ))?;

        let sub_queue = SubQueue::new(len, &self.allocator);
        self.exec_command(Command::create_queue(
            self.admin_sq.tail as u16,
            self.queue_id,
            sub_queue.address(),
            (len - 1) as u16,
            QueueType::Submission,
            Some(self.queue_id),
        ))?;

        self.queue_id += 1;
        Ok(QueuePair {
            id: self.queue_id,
            device: self,
            sub_queue,
            comp_queue,
        })
    }

    pub fn delete_io_queue_pair(&mut self, qpair: QueuePair<A>) -> Result<()> {
        let cmd_id = self.admin_sq.tail as u16;
        let command = Command::delete_queue(cmd_id, qpair.id, QueueType::Submission);
        self.exec_command(command)?;
        let command = Command::delete_queue(cmd_id, qpair.id, QueueType::Completion);
        self.exec_command(command)?;
        Ok(())
    }
}
