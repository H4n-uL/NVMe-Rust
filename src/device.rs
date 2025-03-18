use alloc::collections::btree_map::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use anyhow::Result;
use core::hint::spin_loop;

use crate::cmd::{Command, IdentifyType};
use crate::memory::{Allocator, Dma};
use crate::nvme::{IoQueueId, IoQueuePair, Namespace};
use crate::queues::{CompQueue, Completion};
use crate::queues::{QUEUE_LENGTH, SubQueue};

#[derive(Debug)]
#[allow(unused, clippy::upper_case_acronyms)]
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

pub struct Device<A> {
    address: *mut u8,
    namespaces: BTreeMap<u32, Namespace>,
    allocator: A,
    doorbell_stride: u16,
    admin_sq: SubQueue,
    admin_cq: CompQueue,
    admin_buffer: Dma<u8>,
}

unsafe impl<A> Send for Device<A> {}
unsafe impl<A> Sync for Device<A> {}

impl<A: Allocator> Device<A> {
    pub fn init(address: usize, allocator: A) -> Result<Self> {
        let mut device = Self {
            address: address as _,
            namespaces: BTreeMap::new(),
            doorbell_stride: 0,
            admin_sq: SubQueue::new(QUEUE_LENGTH, &allocator),
            admin_cq: CompQueue::new(QUEUE_LENGTH, &allocator),
            admin_buffer: Dma::allocate(&allocator, 4096),
            allocator,
        };

        let cap = device.get_reg::<u64>(Register::CAP) >> 32;
        device.doorbell_stride = cap as u16 & 0xF;

        device.set_reg::<u32>(Register::CC, device.get_reg::<u32>(Register::CC) & !1);
        while device.get_reg::<u32>(Register::CSTS) & 1 == 1 {
            spin_loop();
        }

        device.set_reg::<u64>(Register::ASQ, device.admin_sq.address() as u64);
        device.set_reg::<u64>(Register::ACQ, device.admin_cq.address() as u64);
        let aqa = (QUEUE_LENGTH as u32 - 1) << 16 | (QUEUE_LENGTH as u32 - 1);
        device.set_reg::<u32>(Register::AQA, aqa);

        let cc = device.get_reg::<u32>(Register::CC) & 0xFF00_000F;
        device.set_reg::<u32>(Register::CC, cc | (4 << 20) | (6 << 16));

        device.set_reg::<u32>(Register::CC, device.get_reg::<u32>(Register::CC) | 1);
        while device.get_reg::<u32>(Register::CSTS) & 1 == 0 {
            spin_loop();
        }
        Ok(device)
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
        self.exec_admin(Command::identify(
            self.admin_sq.tail as u16,
            self.admin_buffer.phys_addr,
            IdentifyType::Namespace(id),
        ))?;

        let data = unsafe { &*(self.admin_buffer.addr as *const NamespaceData) };
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
        self.namespaces.insert(id, namespace.clone());

        Ok(namespace)
    }

    pub fn identify_controller(&mut self) -> Result<(String, String, String)> {
        self.exec_admin(Command::identify(
            self.admin_sq.tail as u16,
            self.admin_buffer.phys_addr,
            IdentifyType::Controller,
        ))?;

        let extract_string = |start: usize, end: usize| -> String {
            self.admin_buffer.as_ref()[start..end]
                .iter()
                .flat_map(|&b| char::from_u32(b as u32))
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
        self.exec_admin(Command::identify(
            self.admin_sq.tail as u16,
            self.admin_buffer.phys_addr,
            IdentifyType::NamespaceList(base),
        ))?;

        let data = unsafe {
            core::slice::from_raw_parts(
                self.admin_buffer.addr as *const u32,
                4096 / core::mem::size_of::<u32>(),
            )
        };

        Ok(data.iter().take_while(|&&id| id != 0).copied().collect())
    }
}

impl<A: Allocator> Device<A> {
    pub fn write_doorbell(&self, bell: Doorbell, val: u32) {
        let stride = 4 << self.doorbell_stride;
        let base = self.address as usize + 0x1000;
        let index = match bell {
            Doorbell::SubTail(qid) => qid * 2,
            Doorbell::CompHead(qid) => qid * 2 + 1,
        };

        let addr = base + (index * stride) as usize;
        unsafe { (addr as *mut u32).write_volatile(val) }
    }

    pub fn exec_admin(&mut self, cmd: Command) -> Result<Completion> {
        let tail = self.admin_sq.push(cmd);
        self.write_doorbell(Doorbell::SubTail(0), tail as u32);

        let (head, entry) = self.admin_cq.pop();
        self.write_doorbell(Doorbell::CompHead(0), head as u32);

        let status = (entry.status >> 1) & 0xff;
        if status != 0 {
            anyhow::bail!("Command failed! Status: 0x{:x}", status);
        }

        Ok(entry)
    }
}

impl<A: Allocator> Device<A> {
    pub fn create_io_queue_pair(&mut self, len: usize) -> Result<IoQueuePair<A>> {
        let queue_id = IoQueueId::new();

        let comp_queue = CompQueue::new(len, &self.allocator);
        self.exec_admin(Command::create_completion_queue(
            self.admin_sq.tail as u16,
            *queue_id,
            comp_queue.address(),
            (len - 1) as u16,
        ))?;

        let sub_queue = SubQueue::new(len, &self.allocator);
        self.exec_admin(Command::create_submission_queue(
            self.admin_sq.tail as u16,
            *queue_id,
            sub_queue.address(),
            (len - 1) as u16,
            *queue_id,
        ))?;

        Ok(IoQueuePair {
            id: queue_id,
            device: self,
            sub_queue,
            comp_queue,
            buffer: Dma::allocate(&self.allocator, 4096),
        })
    }

    pub fn delete_io_queue_pair(&mut self, qpair: IoQueuePair<A>) -> Result<()> {
        let cmd_id = self.admin_sq.tail as u16;
        let command = Command::delete_submission_queue(cmd_id, *qpair.id);
        self.exec_admin(command)?;
        let command = Command::delete_completion_queue(cmd_id, *qpair.id);
        self.exec_admin(command)?;
        Ok(())
    }
}
