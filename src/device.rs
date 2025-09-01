use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::hint::spin_loop;
use spin::Mutex;

use crate::cmd::{Command, IdentifyType};
use crate::error::{Error, Result};
use crate::memory::{Allocator, Dma, PrpManager};
use crate::queues::{CompQueue, Completion, SubQueue};

/// Default size of an admin queue.
///
/// Here choose 64 which can exactly fit into a page,
/// which is usually enough for most cases.
const ADMIN_QUEUE_SIZE: usize = 64;

/// Default size of I/O queues.
const IO_QUEUE_SIZE: usize = 256;

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

/// Controller data structure.
#[derive(Default, Debug, Clone)]
pub struct ControllerData {
    /// Serial number
    pub serial_number: String,
    /// Model number
    pub model_number: String,
    /// Firmware revision
    pub firmware_revision: String,
    /// Maximum transfer size (in bytes)
    pub max_transfer_size: usize,
    /// Minimum page size (in bytes)
    pub min_pagesize: usize,
    /// Maximum queue entries
    pub max_queue_entries: u16,
}

/// Internal I/O state with interior mutability.
struct IoState {
    io_sq: SubQueue,
    io_cq: CompQueue,
    prp_manager: PrpManager,
}

/// Internal device state - uses spin::Mutex for thread-safe interior mutability
struct DeviceInner<A: Allocator> {
    allocator: Arc<A>,
    doorbell_helper: Mutex<DoorbellHelper>,
    data: Mutex<ControllerData>,
    io_state: Mutex<IoState>,
}

/// A structure representing an NVMe namespace.
pub struct Namespace<A: Allocator> {
    id: u32,
    block_count: u64,
    block_size: u64,
    device: Arc<DeviceInner<A>>,
}

impl<A: Allocator> Namespace<A> {
    /// Get the namespace ID.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Get the block count.
    pub fn block_count(&self) -> u64 {
        self.block_count
    }

    /// Get the block size (in bytes).
    pub fn block_size(&self) -> u64 {
        self.block_size
    }

    /// Read from the namespace.
    pub fn read(&self, lba: u64, buf: &mut [u8]) -> Result<()> {
        if buf.len() as u64 % self.block_size != 0 {
            return Err(Error::InvalidBufferSize);
        }
        self.do_io(lba, buf.as_mut_ptr() as usize, buf.len(), false)
    }

    /// Write to the namespace.
    pub fn write(&self, lba: u64, buf: &[u8]) -> Result<()> {
        if buf.len() as u64 % self.block_size != 0 {
            return Err(Error::InvalidBufferSize);
        }
        self.do_io(lba, buf.as_ptr() as usize, buf.len(), true)
    }

    /// Perform I/O operation.
    fn do_io(&self, lba: u64, address: usize, bytes: usize, write: bool) -> Result<()> {
        let max_transfer_size = self.device.data.lock().max_transfer_size;
        if bytes > max_transfer_size {
            return Err(Error::IoSizeExceedsMdts);
        }

        let mut io_state = self.device.io_state.lock();

        // Create PRP list
        let prp_result = io_state.prp_manager.create(self.device.allocator.as_ref(), address, bytes)?;
        let prp = prp_result.get_prp();
        let blocks = bytes as u64 / self.block_size;

        // Create command
        let command = Command::read_write(
            io_state.io_sq.tail as u16,
            self.id,
            lba,
            blocks as u16 - 1,
            [prp.0 as u64, prp.1 as u64],
            write,
        );

        // Submit command
        let tail = io_state.io_sq.push(command);
        self.device.doorbell_helper.lock().write(Doorbell::SubTail(1), tail as u32);

        // Wait for completion
        let (head, entry) = io_state.io_cq.pop();
        self.device.doorbell_helper.lock().write(Doorbell::CompHead(1), head as u32);
        io_state.io_sq.head = entry.sq_head as usize;

        // Release PRP resources
        io_state.prp_manager.release(prp_result, self.device.allocator.as_ref());

        // Check status
        let status = (entry.status >> 1) & 0xff;
        if status != 0 {
            return Err(Error::CommandFailed(status));
        }

        Ok(())
    }
}

/// A structure representing an NVMe controller device.
pub struct Device<A: Allocator> {
    address: *mut u8,
    inner: Arc<DeviceInner<A>>,

    // Admin queues (only used during init)
    admin_sq: SubQueue,
    admin_cq: CompQueue,
    admin_buffer: Dma<u8>,

    // Namespaces
    namespaces: BTreeMap<u32, Namespace<A>>,
}

unsafe impl<A: Allocator> Send for Device<A> {}
unsafe impl<A: Allocator> Sync for Device<A> {}

impl<A: Allocator> Device<A> {
    /// Initialize a NVMe controller device.
    ///
    /// The `address` is the base address of the controller
    /// constructed by the PCI BAR 0 (lower 32 bits) and BAR 1 (upper 32 bits).
    ///
    /// The `allocator` is a DMA allocator that implements
    /// the `Allocator` trait used for the entire NVMe device.
    pub fn init(address: usize, allocator: A) -> Result<Self> {
        let allocator = Arc::new(allocator);
        let doorbell_helper = DoorbellHelper::new(address, 0);

        let inner = Arc::new(DeviceInner {
            allocator: allocator.clone(),
            doorbell_helper: Mutex::new(doorbell_helper),
            data: Mutex::new(Default::default()),
            io_state: Mutex::new(IoState {
                io_sq: SubQueue::new(IO_QUEUE_SIZE, allocator.as_ref()),
                io_cq: CompQueue::new(IO_QUEUE_SIZE, allocator.as_ref()),
                prp_manager: Default::default(),
            }),
        });

        let mut device = Self {
            address: address as _,
            inner: inner.clone(),
            admin_sq: SubQueue::new(ADMIN_QUEUE_SIZE, allocator.as_ref()),
            admin_cq: CompQueue::new(ADMIN_QUEUE_SIZE, allocator.as_ref()),
            admin_buffer: Dma::allocate(4096, allocator.as_ref()),
            namespaces: BTreeMap::new(),
        };

        // Read controller capabilities
        let cap = device.get_reg::<u64>(Register::CAP);
        let doorbell_stride = (cap >> 32) as u8 & 0xF;

        // Update inner fields safely using Mutex
        *device.inner.doorbell_helper.lock() = DoorbellHelper::new(address, doorbell_stride);
        {
            let mut data = device.inner.data.lock();
            data.min_pagesize = 1 << (((cap >> 48) as u8 & 0xF) + 12);
            data.max_queue_entries = (cap & 0x7FFF) as u16 + 1;
        }

        // Reset controller
        device.set_reg::<u32>(Register::CC, device.get_reg::<u32>(Register::CC) & !1);
        while device.get_reg::<u32>(Register::CSTS) & 1 == 1 {
            spin_loop();
        }

        // Configure admin queues
        device.set_reg::<u64>(Register::ASQ, device.admin_sq.address() as u64);
        device.set_reg::<u64>(Register::ACQ, device.admin_cq.address() as u64);
        let aqa = (ADMIN_QUEUE_SIZE as u32 - 1) << 16 | (ADMIN_QUEUE_SIZE as u32 - 1);
        device.set_reg::<u32>(Register::AQA, aqa);

        // Enable controller
        let cc = device.get_reg::<u32>(Register::CC) & 0xFF00_000F;
        device.set_reg::<u32>(Register::CC, cc | (4 << 20) | (6 << 16));

        device.set_reg::<u32>(Register::CC, device.get_reg::<u32>(Register::CC) | 1);
        while device.get_reg::<u32>(Register::CSTS) & 1 == 0 {
            spin_loop();
        }

        // Identify controller
        device.exec_admin(Command::identify(
            device.admin_sq.tail as u16,
            device.admin_buffer.phys_addr,
            IdentifyType::Controller,
        ))?;

        let extract_string = |start: usize, end: usize| -> String {
            device.admin_buffer[start..end]
                .iter()
                .flat_map(|&b| char::from_u32(b as u32))
                .collect::<String>()
                .trim()
                .to_string()
        };

        // Update controller data safely using Mutex
        {
            let mut data = device.inner.data.lock();
            data.serial_number = extract_string(4, 24);
            data.model_number = extract_string(24, 64);
            data.firmware_revision = extract_string(64, 72);

            let max_pages = 1 << device.admin_buffer.as_ref()[77];
            data.max_transfer_size = max_pages as usize * data.min_pagesize;
        }

        // Create I/O queues
        device.create_io_queues()?;

        // Identify all namespaces
        device.identify_all_namespaces()?;

        Ok(device)
    }

    /// Get a namespace by its ID.
    ///
    /// Returns `None` if the namespace doesn't exist.
    pub fn get_ns(&self, namespace_id: u32) -> Option<&Namespace<A>> {
        self.namespaces.get(&namespace_id)
    }

    /// Get controller data.
    pub fn controller_data(&self) -> ControllerData {
        self.inner.data.lock().clone()
    }

    /// Create I/O queues.
    fn create_io_queues(&mut self) -> Result<()> {
        let max_queue_entries = self.inner.data.lock().max_queue_entries;
        let queue_size = IO_QUEUE_SIZE.min(max_queue_entries as usize);
        let (io_cq_addr, io_sq_addr) = {
            let io_state = self.inner.io_state.lock();
            (io_state.io_cq.address(), io_state.io_sq.address())
        };

        // Create completion queue
        self.exec_admin(Command::create_completion_queue(
            self.admin_sq.tail as u16,
            1, // Queue ID 1
            io_cq_addr,
            (queue_size - 1) as u16,
        ))?;

        // Create submission queue
        self.exec_admin(Command::create_submission_queue(
            self.admin_sq.tail as u16,
            1, // Queue ID 1
            io_sq_addr,
            (queue_size - 1) as u16,
            1, // Completion queue ID
        ))?;

        Ok(())
    }

    /// Destroy I/O queues.
    fn destroy_io_queues(&mut self) -> Result<()> {
        // Delete submission queue first (spec requirement)
        self.exec_admin(Command::delete_submission_queue(
            self.admin_sq.tail as u16,
            1, // Queue ID 1
        ))?;

        // Then delete completion queue
        self.exec_admin(Command::delete_completion_queue(
            self.admin_sq.tail as u16,
            1, // Queue ID 1
        ))?;

        Ok(())
    }

    /// Identify all namespaces on the device.
    fn identify_all_namespaces(&mut self) -> Result<()> {
        // Get namespace list
        self.exec_admin(Command::identify(
            self.admin_sq.tail as u16,
            self.admin_buffer.phys_addr,
            IdentifyType::NamespaceList(0),
        ))?;

        let ids = self.admin_buffer
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
            .filter(|&id| id != 0)
            .collect::<Vec<u32>>();

        // Identify each namespace
        for id in ids {
            self.exec_admin(Command::identify(
                self.admin_sq.tail as u16,
                self.admin_buffer.phys_addr,
                IdentifyType::Namespace(id),
            ))?;

            let data = unsafe { &*(self.admin_buffer.addr as *const NamespaceData) };
            let flba_index = (data.lba_size & 0xF) as usize;
            let flba_data = (data.lba_format_support[flba_index] >> 16) & 0xFF;

            let namespace = Namespace {
                id,
                block_size: 1 << flba_data,
                block_count: data.capacity,
                device: self.inner.clone(),
            };

            self.namespaces.insert(id, namespace);
        }

        Ok(())
    }

    /// Get the list of all namespaces on the device.
    pub fn list_namespaces(&self) -> Vec<u32> {
        self.namespaces.keys().cloned().collect()
    }

    /// Helper function to read a NVMe register.
    fn get_reg<T>(&self, reg: Register) -> T {
        let address = self.address as usize + reg as usize;
        unsafe { (address as *const T).read_volatile() }
    }

    /// Helper function to write a NVMe register.
    fn set_reg<T>(&mut self, reg: Register, value: T) {
        let address = self.address as usize + reg as usize;
        unsafe { (address as *mut T).write_volatile(value) }
    }

    /// Execute an admin command.
    fn exec_admin(&mut self, cmd: Command) -> Result<Completion> {
        let tail = self.admin_sq.push(cmd);
        self.inner.doorbell_helper.lock().write(Doorbell::SubTail(0), tail as u32);

        let (head, entry) = self.admin_cq.pop();
        self.inner.doorbell_helper.lock().write(Doorbell::CompHead(0), head as u32);

        let status = (entry.status >> 1) & 0xff;
        if status != 0 {
            return Err(Error::CommandFailed(status));
        }

        Ok(entry)
    }
}

impl<A: Allocator> Device<A> {
    /// Get the version of the NVMe controller.
    pub fn nvme_version(&self) -> (u16, u8, u8) {
        let version = self.get_reg::<u32>(Register::VS);
        let major = (version >> 16) as u16;
        let minor = (version >> 8) as u8;
        let tertiary = version as u8;
        (major, minor, tertiary)
    }
}

impl<A: Allocator> Drop for Device<A> {
    fn drop(&mut self) {
        // Best effort cleanup - ignore errors during drop
        let _ = self.destroy_io_queues();

        // Reset controller
        self.set_reg::<u32>(Register::CC,
            self.get_reg::<u32>(Register::CC) & !1);
    }
}