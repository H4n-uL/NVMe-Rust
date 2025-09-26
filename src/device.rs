use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::hint::spin_loop;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::{Mutex, RwLock};

use crate::cmd::{Command, IdentifyType, FeatureId};
use crate::error::{Error, Result};
use crate::memory::{Allocator, Dma, PrpManager};
use crate::queues::{CompQueue, Completion, SubQueue};

/// Minimum size of an admin queue.
///
/// We'll use the maximum supported by the controller
/// to avoid queue full issues with many I/O queues.
const MIN_ADMIN_QUEUE_SIZE: usize = 2;

/// Default size of I/O queues.
const IO_QUEUE_SIZE: usize = 256;

/// Temperature threshold type.
#[derive(Debug, Clone, Copy)]
pub enum TempThresholdType {
    /// Over temperature threshold
    OverTemp,
    /// Under temperature threshold
    UnderTemp,
}

/// Self-test type.
#[derive(Debug, Clone, Copy)]
pub enum SelfTestType {
    /// Short device self-test (~ 2 minutes)
    Short,
    /// Extended device self-test (varies)
    Extended,
    /// Abort running self-test
    Abort,
}

/// Self-test result.
#[derive(Debug, Clone)]
pub struct SelfTestResult {
    /// Current self-test operation
    pub current_operation: u8,
    /// Current self-test completion percentage
    pub current_completion: u8,
    /// Historical test results
    pub results: Vec<u8>,
}

/// Error log entry.
#[derive(Debug, Clone)]
pub struct ErrorLogEntry {
    /// Error count
    pub error_count: u64,
    /// Submission queue ID
    pub sqid: u16,
    /// Command ID
    pub cmdid: u16,
    /// Status field
    pub status: u16,
    /// Parameter error location
    pub parameter_error_location: u16,
    /// LBA of error
    pub lba: u64,
    /// Namespace
    pub namespace: u32,
    /// Vendor specific info
    pub vendor_specific: u8,
    /// Transport type
    pub trtype: u8,
    /// Command specific info
    pub command_specific: u64,
    /// Transport specific
    pub transport_specific: u16,
}


/// Endurance group information.
#[derive(Debug, Clone)]
pub struct EnduranceGroupInfo {
    /// Critical warning
    pub critical_warning: u8,
    /// Available spare
    pub available_spare: u8,
    /// Available spare threshold
    pub available_spare_threshold: u8,
    /// Percentage used
    pub percentage_used: u8,
    /// Endurance estimate
    pub endurance_estimate: u128,
    /// Data units read
    pub data_units_read: u128,
    /// Data units written
    pub data_units_written: u128,
    /// Media units written
    pub media_units_written: u128,
    /// Host read commands
    pub host_read_commands: u128,
    /// Host write commands
    pub host_write_commands: u128,
    /// Media errors
    pub media_errors: u128,
    /// Number of error log entries
    pub num_error_entries: u128,
}

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
    /// Maximum number of I/O submission queues (0-based)
    pub max_io_sq: u16,
    /// Maximum number of I/O completion queues (0-based)
    pub max_io_cq: u16,
}

/// I/O queue pair representing submission and completion queues.
struct IoQueuePair {
    /// Queue ID (1-based for I/O queues)
    qid: u16,
    /// Submission queue
    sq: SubQueue,
    /// Completion queue
    cq: CompQueue,
    /// PRP manager for this queue
    prp_manager: PrpManager,
    /// Number of outstanding commands
    outstanding: AtomicUsize,
    /// Queue shutdown flag - when true, no new I/O accepted
    shutdown: AtomicBool,
}

/// Internal device state - uses spin::Mutex for thread-safe interior mutability
struct DeviceInner<A: Allocator> {
    allocator: Arc<A>,
    doorbell_helper: DoorbellHelper,
    data: Mutex<ControllerData>,
    ioq: Mutex<Vec<Arc<Mutex<IoQueuePair>>>>,
    queue_selector: AtomicUsize,
    next_queue_id: AtomicUsize,
    shutting_down: AtomicBool,
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

    /// Select the optimal I/O queue for this operation.
    fn select_queue(&self) -> Option<Arc<Mutex<IoQueuePair>>> {
        let queues = self.device.ioq.lock();
        if queues.is_empty() {
            return None;
        }

        // Filter out shutdown queues
        let active_queues: Vec<_> = queues
            .iter()
            .filter(|q| !q.lock().shutdown.load(Ordering::Acquire))
            .cloned()
            .collect();

        if active_queues.is_empty() {
            return None;
        }

        if active_queues.len() == 1 {
            return Some(active_queues[0].clone());
        }

        // Try to find least loaded active queue
        let mut min_outstanding = usize::MAX;
        let mut selected_queue = None;

        for queue in active_queues.iter() {
            let outstanding = queue.lock().outstanding.load(Ordering::Relaxed);
            if outstanding < min_outstanding {
                min_outstanding = outstanding;
                selected_queue = Some(queue.clone());
            }
        }

        // If all queues are equally loaded, use round-robin
        selected_queue.or_else(|| {
            let idx = self.device.queue_selector.fetch_add(1, Ordering::Relaxed) % active_queues.len();
            Some(active_queues[idx].clone())
        })
    }

    /// TRIM/Discard - Essential for SSD performance and lifetime.
    /// Informs the controller that specified LBA ranges contain no valid data.
    pub fn trim(&self, lba: u64, block_count: u64) -> Result<()> {
        // Check if device is shutting down
        if self.device.shutting_down.load(Ordering::Acquire) {
            return Err(Error::DeviceShuttingDown);
        }

        let queue_arc = self.select_queue().ok_or(Error::NoActiveQueues)?;
        let mut queue = queue_arc.lock();
        queue.outstanding.fetch_add(1, Ordering::Relaxed);

        // Prepare dataset management ranges (up to 256 ranges)
        let range_data = [(lba as u32, (lba >> 32) as u32, block_count as u32)];
        let range_addr = range_data.as_ptr() as usize;

        let cmd = Command::dataset_management(
            queue.sq.tail() as u16,
            self.id,
            range_addr,
            0, // nr = 0 means 1 range
            true, // ad = true for deallocate (TRIM)
            false,
            false,
        );

        // Submit command with dynamic queue management
        let entry = self.submit_iocmd(&mut queue, cmd)?;
        queue.outstanding.fetch_sub(1, Ordering::Relaxed);

        let status = (entry.status >> 1) & 0xff;
        if status != 0 {
            return Err(Error::CommandFailed(status));
        }

        Ok(())
    }

    /// Write Zeroes - Efficient zeroing without data transfer.
    /// Much faster than writing actual zero buffers.
    pub fn write_zeroes(&self, lba: u64, block_count: u16) -> Result<()> {
        // Check if device is shutting down
        if self.device.shutting_down.load(Ordering::Acquire) {
            return Err(Error::DeviceShuttingDown);
        }

        let queue_arc = self.select_queue().ok_or(Error::NoActiveQueues)?;
        let queue = queue_arc.lock();
        queue.outstanding.fetch_add(1, Ordering::Relaxed);

        let cmd = Command::write_zeroes(
            queue.sq.tail() as u16,
            self.id,
            lba,
            block_count - 1,
            false, // deac = deallocate after write
        );

        let tail = queue.sq.push(cmd);
        self.device.doorbell_helper.write(Doorbell::SubTail(queue.qid), tail as u32);

        let (head, entry) = queue.cq.pop();
        self.device.doorbell_helper.write(Doorbell::CompHead(queue.qid), head as u32);
        queue.sq.set_head(entry.sq_head as usize);
        queue.outstanding.fetch_sub(1, Ordering::Relaxed);

        let status = (entry.status >> 1) & 0xff;
        if status != 0 {
            return Err(Error::CommandFailed(status));
        }

        Ok(())
    }

    /// Compare - Atomically compare data without transferring to host.
    /// Essential for lock-free algorithms and database implementations.
    pub fn compare(&self, lba: u64, expected: &[u8]) -> Result<bool> {
        if expected.len() as u64 % self.block_size != 0 {
            return Err(Error::InvalidBufferSize);
        }

        // Check if device is shutting down
        if self.device.shutting_down.load(Ordering::Acquire) {
            return Err(Error::DeviceShuttingDown);
        }

        let queue_arc = self.select_queue().ok_or(Error::NoActiveQueues)?;
        let mut queue = queue_arc.lock();
        queue.outstanding.fetch_add(1, Ordering::Relaxed);

        // Create PRP for expected data
        let prp_result = queue.prp_manager.create(
            self.device.allocator.as_ref(),
            expected.as_ptr() as usize,
            expected.len()
        )?;
        let prp = prp_result.get_prp();
        let blocks = expected.len() as u64 / self.block_size;

        let cmd = Command::compare(
            queue.sq.tail() as u16,
            self.id,
            lba,
            blocks as u16 - 1,
            [prp.0 as u64, prp.1 as u64],
        );

        let tail = queue.sq.push(cmd);
        self.device.doorbell_helper.write(Doorbell::SubTail(queue.qid), tail as u32);

        let (head, entry) = queue.cq.pop();
        self.device.doorbell_helper.write(Doorbell::CompHead(queue.qid), head as u32);
        queue.sq.set_head(entry.sq_head as usize);

        // Release PRP resources
        queue.prp_manager.release(prp_result, self.device.allocator.as_ref());
        queue.outstanding.fetch_sub(1, Ordering::Relaxed);

        let status = (entry.status >> 1) & 0xff;
        if status == 0 {
            Ok(true) // Compare matched
        } else if status == 0x85 { // Compare Failure
            Ok(false) // Compare didn't match
        } else {
            Err(Error::CommandFailed(status))
        }
    }

    /// Verify - Check data integrity without transferring to host.
    /// Critical for data scrubbing and integrity verification.
    pub fn verify(&self, lba: u64, block_count: u16) -> Result<()> {
        // Check if device is shutting down
        if self.device.shutting_down.load(Ordering::Acquire) {
            return Err(Error::DeviceShuttingDown);
        }

        let queue_arc = self.select_queue().ok_or(Error::NoActiveQueues)?;
        let queue = queue_arc.lock();
        queue.outstanding.fetch_add(1, Ordering::Relaxed);

        let cmd = Command::verify(
            queue.sq.tail() as u16,
            self.id,
            lba,
            block_count - 1,
        );

        let tail = queue.sq.push(cmd);
        self.device.doorbell_helper.write(Doorbell::SubTail(queue.qid), tail as u32);

        let (head, entry) = queue.cq.pop();
        self.device.doorbell_helper.write(Doorbell::CompHead(queue.qid), head as u32);
        queue.sq.set_head(entry.sq_head as usize);
        queue.outstanding.fetch_sub(1, Ordering::Relaxed);

        let status = (entry.status >> 1) & 0xff;
        if status != 0 {
            return Err(Error::CommandFailed(status));
        }

        Ok(())
    }

    /// Copy - Server-side copy without host involvement.
    /// Essential for efficient data migration and backup.
    pub fn copy(&self, src_lba: u64, dst_lba: u64, block_count: u16) -> Result<()> {
        // Check if device is shutting down
        if self.device.shutting_down.load(Ordering::Acquire) {
            return Err(Error::DeviceShuttingDown);
        }

        let queue_arc = self.select_queue().ok_or(Error::NoActiveQueues)?;
        let queue = queue_arc.lock();
        queue.outstanding.fetch_add(1, Ordering::Relaxed);

        // Copy descriptor format 0 (simple copy)
        let copy_desc = [
            src_lba as u64,
            (src_lba >> 32) as u64 | ((block_count as u64 - 1) << 32),
        ];
        let desc_addr = copy_desc.as_ptr() as usize;

        let cmd = Command::copy(
            queue.sq.tail() as u16,
            self.id,
            desc_addr,
            dst_lba,
            0, // nr = 0 means 1 source range
            0, // desc_format = 0 for simple copy
        );

        let tail = queue.sq.push(cmd);
        self.device.doorbell_helper.write(Doorbell::SubTail(queue.qid), tail as u32);

        let (head, entry) = queue.cq.pop();
        self.device.doorbell_helper.write(Doorbell::CompHead(queue.qid), head as u32);
        queue.sq.set_head(entry.sq_head as usize);
        queue.outstanding.fetch_sub(1, Ordering::Relaxed);

        let status = (entry.status >> 1) & 0xff;
        if status != 0 {
            return Err(Error::CommandFailed(status));
        }

        Ok(())
    }

    /// Submit I/O command to hardware queue
    fn submit_iocmd(&self, queue: &mut IoQueuePair, cmd: Command) -> Result<Completion> {
        // Push command to submission queue (will spin if full)
        let tail = queue.sq.push(cmd);
        self.device.doorbell_helper.write(Doorbell::SubTail(queue.qid), tail as u32);

        // Wait for completion
        let (head, entry) = queue.cq.pop();
        self.device.doorbell_helper.write(Doorbell::CompHead(queue.qid), head as u32);

        // Update submission queue head from completion entry
        queue.sq.set_head(entry.sq_head as usize);

        Ok(entry)
    }

    /// Perform I/O operation.
    fn do_io(&self, lba: u64, address: usize, bytes: usize, write: bool) -> Result<()> {
        // Check if device is shutting down
        if self.device.shutting_down.load(Ordering::Acquire) {
            return Err(Error::DeviceShuttingDown);
        }

        let max_transfer_size = self.device.data.lock().max_transfer_size;
        if bytes > max_transfer_size {
            return Err(Error::IoSizeExceedsMdts);
        }

        // Select queue and perform I/O
        let queue_arc = self.select_queue().ok_or(Error::NoActiveQueues)?;
        let mut queue = queue_arc.lock();
        queue.outstanding.fetch_add(1, Ordering::Relaxed);

        // Create PRP list
        let prp_result = queue.prp_manager.create(self.device.allocator.as_ref(), address, bytes)?;
        let prp = prp_result.get_prp();
        let blocks = bytes as u64 / self.block_size;

        // Create command
        let command = Command::read_write(
            queue.sq.tail() as u16,
            self.id,
            lba,
            blocks as u16 - 1,
            [prp.0 as u64, prp.1 as u64],
            write,
        );

        // Submit command with dynamic queue management
        let entry = self.submit_iocmd(&mut queue, command)?;

        // Release PRP resources
        queue.prp_manager.release(prp_result, self.device.allocator.as_ref());
        queue.outstanding.fetch_sub(1, Ordering::Relaxed);

        // Check status
        let status = (entry.status >> 1) & 0xff;
        if status != 0 {
            return Err(Error::CommandFailed(status));
        }

        Ok(())
    }
}

/// A structure representing an NVMe controller device.
pub struct NVMeDevice<A: Allocator> {
    address: *mut u8,
    inner: Arc<DeviceInner<A>>,

    // Namespaces
    namespaces: RwLock<BTreeMap<u32, Arc<Namespace<A>>>>,

    // Admin queues
    admin_sq: SubQueue,
    admin_cq: CompQueue,
    admin_buffer: Dma<u8>,
    // Mutex to serialize admin commands
    admin_lock: Mutex<()>,
}

unsafe impl<A: Allocator> Send for NVMeDevice<A> {}
unsafe impl<A: Allocator> Sync for NVMeDevice<A> {}

impl<A: Allocator> NVMeDevice<A> {
    /// Set the number of I/O queue pairs.
    /// Will add or remove queues to match the target count.
    /// When removing queues, it will:
    /// 1. Mark queues for shutdown (no new I/O accepted)
    /// 2. Wait for outstanding I/O to complete
    /// 3. Remove the queues from hardware
    pub fn set_ioq_count(&self, target: usize) -> Result<()> {
        if target == 0 {
            return Err(Error::InvalidQueueCount);
        }

        let hw_limit = {
            let data = self.inner.data.lock();
            data.max_io_sq.min(data.max_io_cq) as usize
        };

        if target > hw_limit {
            return Err(Error::TooManyQueues);
        }

        let current = self.ioq_count();

        if target > current {
            // Add queues
            for _ in current..target {
                self.add_ioq_internal()?;
            }
        } else if target < current {
            // Remove queues safely
            self.rm_ioq_internal(current - target)?;
        }

        Ok(())
    }

    /// Get the current number of I/O queue pairs.
    pub fn ioq_count(&self) -> usize {
        self.inner.ioq.lock().len()
    }

    /// Get the current number of active (non-shutdown) I/O queue pairs.
    pub fn active_ioq_count(&self) -> usize {
        self.inner.ioq.lock()
            .iter()
            .filter(|q| !q.lock().shutdown.load(Ordering::Acquire))
            .count()
    }

    /// Get statistics for each queue.
    pub fn queue_stats(&self) -> Vec<(u16, usize, bool)> {
        self.inner.ioq.lock()
            .iter()
            .map(|q| {
                let queue = q.lock();
                (
                    queue.qid,
                    queue.outstanding.load(Ordering::Relaxed),
                    queue.shutdown.load(Ordering::Relaxed)
                )
            })
            .collect()
    }

    /// Internal method to add a new I/O queue pair.
    fn add_ioq_internal(&self) -> Result<u16> {
        let max_queue_entries = self.inner.data.lock().max_queue_entries;
        // Use a reasonable I/O queue size, but ensure at least 2 entries
        let queue_size = IO_QUEUE_SIZE.min(max_queue_entries as usize).max(2);

        let qid = self.inner.next_queue_id.fetch_add(1, Ordering::SeqCst) as u16;
        // No artificial limit - only hardware limits apply!

        // Create queue structures
        let sq = SubQueue::new(queue_size, self.inner.allocator.as_ref());
        let cq = CompQueue::new(queue_size, self.inner.allocator.as_ref());
        let sq_addr = sq.address();
        let cq_addr = cq.address();

        // Create completion queue first
        self.exec_admin(Command::create_completion_queue(
            self.admin_sq.tail() as u16,
            qid,
            cq_addr,
            (queue_size - 1) as u16,
        ))?;

        // Create submission queue
        self.exec_admin(Command::create_submission_queue(
            self.admin_sq.tail() as u16,
            qid,
            sq_addr,
            (queue_size - 1) as u16,
            qid, // Use same ID for CQ
        ))?;

        // Add to queue list
        let queue_pair = Arc::new(Mutex::new(IoQueuePair {
            qid,
            sq,
            cq,
            prp_manager: Default::default(),
            outstanding: AtomicUsize::new(0),
            shutdown: AtomicBool::new(false),
        }));

        self.inner.ioq.lock().push(queue_pair);
        Ok(qid)
    }

    /// Internal method to remove specified number of I/O queues safely.
    fn rm_ioq_internal(&self, count: usize) -> Result<()> {
        let queues_to_remove = {
            let queues = self.inner.ioq.lock();

            // Don't remove if it would leave us with no queues
            if queues.len() <= count {
                return Err(Error::LastQueueCannotBeRemoved);
            }

            // Select queues to remove (prefer queues with least outstanding I/O)
            let mut queue_stats: Vec<_> = queues.iter()
                .map(|q| {
                    let queue = q.lock();
                    (q.clone(), queue.qid, queue.outstanding.load(Ordering::Relaxed))
                })
                .collect();

            // Sort by outstanding I/O count
            queue_stats.sort_by_key(|&(_, _, outstanding)| outstanding);

            // Take the last 'count' queues (highest load)
            queue_stats.into_iter()
                .rev()
                .take(count)
                .map(|(arc, qid, _)| (arc, qid))
                .collect::<Vec<_>>()
        };

        // Phase 1: Mark queues for shutdown
        for (queue_arc, _) in &queues_to_remove {
            queue_arc.lock().shutdown.store(true, Ordering::Release);
        }

        // Phase 2: Flush and wait for outstanding I/O to complete
        // This is important for controlled queue removal to ensure data integrity
        for (queue_arc, qid) in &queues_to_remove {
            // Send flush command to ensure all writes are committed
            for &ns_id in self.namespaces.read().keys() {
                let queue = queue_arc.lock();

                // Flush only shutdown queues, but ensure completion
                if queue.shutdown.load(Ordering::Acquire) {
                    let flush_cmd = Command::flush(queue.sq.tail() as u16, ns_id);

                    // Push flush command (blocking is OK here - controlled removal)
                    let tail = queue.sq.push(flush_cmd);
                    self.inner.doorbell_helper.write(Doorbell::SubTail(*qid), tail as u32);

                    // MUST wait for flush completion for data safety
                    let (head, _entry) = queue.cq.pop();
                    self.inner.doorbell_helper.write(Doorbell::CompHead(*qid), head as u32);
                    queue.sq.set_head(_entry.sq_head as usize);
                }
            }

            // Wait for all outstanding I/O to complete
            // This is necessary for controlled removal to avoid data loss
            let mut wait_count = 0;
            const MAX_WAIT: usize = 10000; // Prevent infinite wait

            loop {
                let outstanding = queue_arc.lock().outstanding.load(Ordering::Acquire);
                if outstanding == 0 {
                    break;
                }

                wait_count += 1;
                if wait_count > MAX_WAIT {
                    // Log warning or handle timeout
                    break;
                }

                core::hint::spin_loop();
            }
        }

        // Phase 3: Delete queues from hardware and remove from list
        for (_, qid) in &queues_to_remove {
            // Delete submission queue first (NVMe spec requirement)
            self.exec_admin(Command::delete_submission_queue(
                self.admin_sq.tail() as u16,
                *qid,
            ))?;

            // Then delete completion queue
            self.exec_admin(Command::delete_completion_queue(
                self.admin_sq.tail() as u16,
                *qid,
            ))?;
        }

        // Phase 4: Remove from the queue list
        let mut queues = self.inner.ioq.lock();
        queues.retain(|q| {
            let qid = q.lock().qid;
            !queues_to_remove.iter().any(|(_, rm_qid)| *rm_qid == qid)
        });

        Ok(())
    }

    /// Initialize a NVMe controller device.
    ///
    /// The `address` is the base address of the controller
    /// constructed by the PCI BAR 0 (lower 32 bits) and BAR 1 (upper 32 bits).
    ///
    /// The `allocator` is a DMA allocator that implements
    /// the `Allocator` trait used for the entire NVMe device.
    pub fn init(address: usize, allocator: A) -> Result<Self> {
        let allocator = Arc::new(allocator);
        // Need to read capabilities first to get the doorbell stride and max queue entries
        let cap = unsafe { ((address + Register::CAP as usize) as *const u64).read_volatile() };
        let doorbell_stride = (cap >> 32) as u8 & 0xF;
        let max_queue_entries = (cap & 0x7FFF) as usize + 1;
        let min_pagesize = 1 << (((cap >> 48) as u8 & 0xF) + 12);

        // Use hardware maximum for admin queue - software queue handles overflow efficiently
        // No artificial limits - let hardware capabilities determine the size
        let admin_queue_size = max_queue_entries.max(MIN_ADMIN_QUEUE_SIZE);

        let doorbell_helper = DoorbellHelper::new(address, doorbell_stride);

        let inner = Arc::new(DeviceInner {
            allocator: allocator.clone(),
            doorbell_helper: doorbell_helper,
            data: Mutex::new(Default::default()),
            ioq: Mutex::new(Vec::new()),
            queue_selector: AtomicUsize::new(0),
            next_queue_id: AtomicUsize::new(1),
            shutting_down: AtomicBool::new(false),
        });

        let device = Self {
            address: address as _,
            inner: inner.clone(),
            namespaces: RwLock::new(BTreeMap::new()),
            admin_sq: SubQueue::new(admin_queue_size, allocator.as_ref()),
            admin_cq: CompQueue::new(admin_queue_size, allocator.as_ref()),
            admin_buffer: Dma::allocate(4096, allocator.as_ref()),
            admin_lock: Mutex::new(()),
        };

        // Update controller data with capability values
        {
            let mut data = device.inner.data.lock();
            data.min_pagesize = min_pagesize;
            data.max_queue_entries = max_queue_entries as u16;
        }

        // Reset controller
        device.set_reg::<u32>(Register::CC, device.get_reg::<u32>(Register::CC) & !1);
        while device.get_reg::<u32>(Register::CSTS) & 1 == 1 {
            spin_loop();
        }

        // Configure admin queues
        device.set_reg::<u64>(Register::ASQ, device.admin_sq.address() as u64);
        device.set_reg::<u64>(Register::ACQ, device.admin_cq.address() as u64);
        let aqa = (admin_queue_size as u32 - 1) << 16 | (admin_queue_size as u32 - 1);
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
            device.admin_sq.tail() as u16,
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

            // Note: SQES (bytes 512) and CQES (byte 513) are queue entry sizes, not queue counts
            // We'll get the actual maximum I/O queue counts via Set Features
        }

        // Negotiate maximum number of I/O queues with the controller
        // Request a reasonable number of queues (e.g., 64 of each type)
        // The controller will respond with the actual number it can support
        let requested_queues = 63;  // 0-based value (63 means 64 queues)
        let queue_config = (requested_queues << 16) | requested_queues;

        let result = device.exec_admin(Command::set_features(
            device.admin_sq.tail() as u16,
            FeatureId::NumberOfQueues,
            queue_config,
            false,
        ))?;

        // Extract actual allocated queue counts from completion entry
        // Bits 31:16 = Number of I/O Completion Queues Allocated (0-based)
        // Bits 15:0 = Number of I/O Submission Queues Allocated (0-based)
        let allocated_sq = (result.command_specific & 0xFFFF) + 1;
        let allocated_cq = ((result.command_specific >> 16) & 0xFFFF) + 1;

        {
            let mut data = device.inner.data.lock();
            data.max_io_sq = allocated_sq as u16;
            data.max_io_cq = allocated_cq as u16;
        }

        // Create I/O queues
        device.create_ioq()?;

        // Identify all namespaces
        device.ident_namespaces_all()?;

        Ok(device)
    }

    /// Get a namespace by its ID.
    ///
    /// Returns `None` if the namespace doesn't exist.
    pub fn get_ns(&self, namespace_id: u32) -> Option<Arc<Namespace<A>>> {
        self.namespaces.read().get(&namespace_id).cloned()
    }

    /// Get controller data.
    pub fn data(&self) -> ControllerData {
        self.inner.data.lock().clone()
    }

    /// Create initial I/O queues.
    fn create_ioq(&self) -> Result<()> {
        // Start with one I/O queue pair
        self.add_ioq_internal()?;
        Ok(())
    }

    /// Destroy all I/O queues.
    /// Ensures all data is flushed before deletion.
    fn destroy_ioq(&self) -> Result<()> {
        let queue_count = self.inner.ioq.lock().len();
        if queue_count > 0 {
            // Phase 1: Mark all queues for shutdown
            {
                let queues = self.inner.ioq.lock();
                for queue in queues.iter() {
                    queue.lock().shutdown.store(true, Ordering::Release);
                }
            }

            // Phase 2: Flush all namespaces and wait for completion
            // This is critical - we MUST ensure flushes complete for data safety
            for &ns_id in self.namespaces.read().keys() {
                let queues = self.inner.ioq.lock().clone();
                for queue_arc in queues.iter() {
                    let queue = queue_arc.lock();
                    let flush_cmd = Command::flush(queue.sq.tail() as u16, ns_id);

                    // Push flush command
                    let tail = queue.sq.push(flush_cmd);
                    self.inner.doorbell_helper.write(Doorbell::SubTail(queue.qid), tail as u32);

                    // Wait for flush completion - this is essential
                    let (head, _entry) = queue.cq.pop();
                    self.inner.doorbell_helper.write(Doorbell::CompHead(queue.qid), head as u32);
                    queue.sq.set_head(_entry.sq_head as usize);
                }
            }

            // Phase 3: Delete all queues from hardware
            // Controller reset will handle any remaining I/O
            let queues = self.inner.ioq.lock().clone();
            for queue_arc in queues.iter().rev() {
                let qid = queue_arc.lock().qid;

                // Delete submission queue first (spec requirement)
                self.exec_admin(Command::delete_submission_queue(
                    self.admin_sq.tail() as u16,
                    qid,
                ))?;

                // Then delete completion queue
                self.exec_admin(Command::delete_completion_queue(
                    self.admin_sq.tail() as u16,
                    qid,
                ))?;
            }
        }

        self.inner.ioq.lock().clear();
        self.inner.next_queue_id.store(1, Ordering::SeqCst);
        Ok(())
    }

    /// Identify all namespaces on the device.
    fn ident_namespaces_all(&self) -> Result<()> {
        // Get namespace list
        self.exec_admin(Command::identify(
            self.admin_sq.tail() as u16,
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
                self.admin_sq.tail() as u16,
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

            self.namespaces.write().insert(id, Arc::new(namespace));
        }

        Ok(())
    }

    /// Get the list of all namespaces on the device.
    pub fn list_ns(&self) -> Vec<u32> {
        self.namespaces.read().keys().cloned().collect()
    }

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

    /// Execute an admin command.
    fn exec_admin(&self, cmd: Command) -> Result<Completion> {
        // Serialize admin commands to prevent race conditions
        let _guard = self.admin_lock.lock();

        // Push command to submission queue (will spin if full)
        let tail = self.admin_sq.push(cmd);
        self.inner.doorbell_helper.write(Doorbell::SubTail(0), tail as u32);

        // Wait for completion
        let (head, entry) = self.admin_cq.pop();
        self.inner.doorbell_helper.write(Doorbell::CompHead(0), head as u32);

        // Update submission queue head from completion entry
        self.admin_sq.set_head(entry.sq_head as usize);

        let status = (entry.status >> 1) & 0xff;
        if status != 0 {
            return Err(Error::CommandFailed(status));
        }

        Ok(entry)
    }
}

impl<A: Allocator> NVMeDevice<A> {
    /// Get the version of the NVMe controller.
    pub fn nvme_version(&self) -> (u16, u8, u8) {
        let version = self.get_reg::<u32>(Register::VS);
        let major = (version >> 16) as u16;
        let minor = (version >> 8) as u8;
        let tertiary = version as u8;
        (major, minor, tertiary)
    }
}

impl<A: Allocator> Drop for NVMeDevice<A> {
    fn drop(&mut self) {
        // 1. Set global shutdown flag
        self.inner.shutting_down.store(true, Ordering::Release);

        // 2. Flush each namespace on each queue
        for &ns_id in self.namespaces.read().keys() {
            let queues = self.inner.ioq.lock().clone();
            for queue_arc in queues.iter() {
                let queue = queue_arc.lock();

                // Mark shutdown and send flush
                queue.shutdown.store(true, Ordering::Release);

                let flush_cmd = Command::flush(queue.sq.tail() as u16, ns_id);
                let tail = queue.sq.push(flush_cmd);
                self.inner.doorbell_helper.write(Doorbell::SubTail(queue.qid), tail as u32);

                // Wait for flush completion
                let (head, entry) = queue.cq.pop();
                self.inner.doorbell_helper.write(Doorbell::CompHead(queue.qid), head as u32);
                queue.sq.set_head(entry.sq_head as usize);
            }
        }

        // 3. Destroy queues
        let _ = self.destroy_ioq();

        // 4. Reset controller
        self.set_reg::<u32>(Register::CC,
            self.get_reg::<u32>(Register::CC) & !1);
    }
}
