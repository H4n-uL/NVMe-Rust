use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::hint::spin_loop;

use crate::cmd::{Command, IdentifyType};
use crate::error::{NvmeError, Result};
use crate::memory::{Dma, NvmeAllocator};
use crate::nvme::{IoQueueId, IoQueuePair};
use crate::queues::{CompQueue, Completion, SubQueue};

/// Default size of an admin queue.
///
/// Here choose 64 which can exactly fit into a page,
/// which is usually enough for most cases.
const ADMIN_QUEUE_SIZE: usize = 64;

/// NVMe controller registers.
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

/// NVMe doorbell register.
#[derive(Clone, Debug)]
pub(crate) enum Doorbell {
    SubTail(u16),
    CompHead(u16),
}

/// A helper for calculating doorbell addresses.
///
/// It is separate so that the `IoQueuePair` can reference it
/// instead of the entire controller, thus not resulting into
/// the problem of creating mutable references multiple times.
#[derive(Clone, Debug)]
pub(crate) struct DoorbellHelper {
    address: usize,
    stride: u8,
}

impl DoorbellHelper {
    /// Create a new `DoorbellHelper` instance.
    pub fn new(address: usize, stride: u8) -> Self {
        Self { address, stride }
    }

    /// Write a value to specified doorbell register.
    pub fn write(&self, bell: Doorbell, val: u32) {
        let stride = 4 << self.stride;
        let base = self.address + 0x1000;
        let index = match bell {
            Doorbell::SubTail(qid) => qid * 2,
            Doorbell::CompHead(qid) => qid * 2 + 1,
        };

        let addr = base + (index * stride) as usize;
        unsafe { (addr as *mut u32).write_volatile(val) }
    }
}

/// NVMe namespace data structure.
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

/// A data structure that holds some
/// common information about some nvme controllers.
#[derive(Default, Debug, Clone)]
pub struct NvmeControllerData {
    /// Serial number
    pub serial_number: String,
    /// Model number
    pub model_number: String,
    /// Firmware revision
    pub firmware_revision: String,
    /// Maximum transfer size (in bytes)
    pub max_transfer_size: u64,
}

/// A structure representing an NVMe namespace.
#[derive(Debug, Clone)]
pub struct NvmeNamespace {
    /// Namespace ID
    pub id: u32,
    /// Block count
    pub block_count: u64,
    /// Block size (in bytes)
    pub block_size: u64,
}

/// A structure representing an NVMe controller device.
pub struct NvmeDevice<A> {
    address: *mut u8,
    pub(crate) allocator: Rc<A>,
    min_pagesize: usize,
    pub(crate) admin_sq: SubQueue<ADMIN_QUEUE_SIZE>,
    admin_cq: CompQueue<ADMIN_QUEUE_SIZE>,
    admin_buffer: Dma<[u8; 4096]>,
    doorbell_helper: DoorbellHelper,
    /// Some useful information of the controller
    pub controller_data: NvmeControllerData,
}

unsafe impl<A> Send for NvmeDevice<A> {}
unsafe impl<A> Sync for NvmeDevice<A> {}

impl<A: NvmeAllocator> NvmeDevice<A> {
    /// Initialize a NVMe controller device.
    ///
    /// The `address` is the base address of the controller and the
    /// `allocator` is the global DMA allocator for the entire NVMe device.
    pub fn init(address: usize, allocator: A) -> Result<Self> {
        let mut device = Self {
            address: address as _,
            admin_sq: SubQueue::new(&allocator),
            admin_cq: CompQueue::new(&allocator),
            admin_buffer: Dma::allocate(&allocator),
            doorbell_helper: DoorbellHelper::new(address, 0),
            controller_data: Default::default(),
            min_pagesize: Default::default(),
            allocator: Rc::new(allocator),
        };

        let cap = device.get_reg::<u64>(Register::CAP);
        let doorbell_stride = (cap >> 32) as u8 & 0xF;
        device.doorbell_helper = DoorbellHelper::new(address, doorbell_stride);
        device.min_pagesize = 1 << (((cap >> 48) as u8 & 0xF) + 12);

        let max_queues_entrys = (cap & 0x7FFF) as u16;
        log::info!("Max queues: {}", max_queues_entrys);

        device.set_reg::<u32>(Register::CC, device.get_reg::<u32>(Register::CC) & !1);
        while device.get_reg::<u32>(Register::CSTS) & 1 == 1 {
            spin_loop();
        }

        device.set_reg::<u64>(Register::ASQ, device.admin_sq.address() as u64);
        device.set_reg::<u64>(Register::ACQ, device.admin_cq.address() as u64);
        let aqa = (ADMIN_QUEUE_SIZE as u32 - 1) << 16 | (ADMIN_QUEUE_SIZE as u32 - 1);
        device.set_reg::<u32>(Register::AQA, aqa);

        let cc = device.get_reg::<u32>(Register::CC) & 0xFF00_000F;
        device.set_reg::<u32>(Register::CC, cc | (4 << 20) | (6 << 16));

        device.set_reg::<u32>(Register::CC, device.get_reg::<u32>(Register::CC) | 1);
        while device.get_reg::<u32>(Register::CSTS) & 1 == 0 {
            spin_loop();
        }

        device.controller_data = device.identify_controller()?;

        Ok(device)
    }
}

impl<A: NvmeAllocator> NvmeDevice<A> {
    /// Helper function to read a NVMe register.
    fn get_reg<T>(&self, reg: Register) -> T {
        let address = self.address as usize + reg as usize;
        unsafe { (address as *const T).read_volatile() }
    }

    /// Helper function to write a NVMe register.
    fn set_reg<T>(&self, reg: Register, value: T) {
        let address = self.address as usize + reg as usize;
        unsafe { (address as *mut T).write_volatile(value) }
    }
}

impl<A: NvmeAllocator> NvmeDevice<A> {
    fn identify_controller(&mut self) -> Result<NvmeControllerData> {
        self.exec_admin(Command::identify(
            self.admin_sq.tail as u16,
            self.admin_buffer.phys_addr,
            IdentifyType::Controller,
        ))?;

        let extract_string = |start: usize, end: usize| -> String {
            self.admin_buffer[start..end]
                .iter()
                .flat_map(|&b| char::from_u32(b as u32))
                .collect::<String>()
                .trim()
                .to_string()
        };

        let serial = extract_string(4, 24);
        let model = extract_string(24, 64);
        let firmware = extract_string(64, 72);

        let max_pages = 1 << self.admin_buffer.as_ref()[77];
        let max_transfer_size = (max_pages * self.min_pagesize) as u64;

        Ok(NvmeControllerData {
            serial_number: serial,
            model_number: model,
            firmware_revision: firmware,
            max_transfer_size,
        })
    }

    pub fn identify_namespaces(&mut self, base: u32) -> Result<Vec<NvmeNamespace>> {
        self.exec_admin(Command::identify(
            self.admin_sq.tail as u16,
            self.admin_buffer.phys_addr,
            IdentifyType::NamespaceList(base),
        ))?;

        let ids = self
            .admin_buffer
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
            .filter(|&id| id != 0)
            .collect::<Vec<u32>>();

        let get_namespace = |&id| {
            self.exec_admin(Command::identify(
                self.admin_sq.tail as u16,
                self.admin_buffer.phys_addr,
                IdentifyType::Namespace(id),
            ))?;

            let data = unsafe { &*(self.admin_buffer.addr as *const NamespaceData) };
            let flba_index = (data.lba_size & 0xF) as usize;
            let flba_data = (data.lba_format_support[flba_index] >> 16) & 0xFF;

            Ok(NvmeNamespace {
                id,
                block_size: 1 << flba_data,
                block_count: data.capacity,
            })
        };

        ids.iter().map(get_namespace).collect()
    }
}

impl<A: NvmeAllocator> NvmeDevice<A> {
    pub(crate) fn exec_admin(&mut self, cmd: Command) -> Result<Completion> {
        let tail = self.admin_sq.push(cmd);
        self.doorbell_helper
            .write(Doorbell::SubTail(0), tail as u32);

        let (head, entry) = self.admin_cq.pop();
        self.doorbell_helper
            .write(Doorbell::CompHead(0), head as u32);

        let status = (entry.status >> 1) & 0xff;
        if status != 0 {
            return Err(NvmeError::CommandFailed(status));
        }

        Ok(entry)
    }
}

impl<A: NvmeAllocator> NvmeDevice<A> {
    pub fn create_io_queue_pair<'a, const SIZE: usize>(
        &mut self,
        namespace: &'a NvmeNamespace,
    ) -> Result<IoQueuePair<'a, A, SIZE>> {
        let queue_id = IoQueueId::new();

        let comp_queue = CompQueue::new(self.allocator.as_ref());
        self.exec_admin(Command::create_completion_queue(
            self.admin_sq.tail as u16,
            *queue_id,
            comp_queue.address(),
            (SIZE - 1) as u16,
        ))?;

        let sub_queue = SubQueue::new(self.allocator.as_ref());
        self.exec_admin(Command::create_submission_queue(
            self.admin_sq.tail as u16,
            *queue_id,
            sub_queue.address(),
            (SIZE - 1) as u16,
            *queue_id,
        ))?;

        Ok(IoQueuePair::new(
            queue_id,
            namespace,
            self.doorbell_helper.clone(),
            sub_queue,
            comp_queue,
            self.allocator.clone(),
            self.controller_data.max_transfer_size,
        ))
    }

    pub fn delete_io_queue_pair<const SIZE: usize>(
        &mut self,
        qpair: IoQueuePair<A, SIZE>,
    ) -> Result<()> {
        let cmd_id = self.admin_sq.tail as u16;
        let command = Command::delete_submission_queue(cmd_id, *qpair.id);
        self.exec_admin(command)?;
        let command = Command::delete_completion_queue(cmd_id, *qpair.id);
        self.exec_admin(command)?;
        Ok(())
    }
}
