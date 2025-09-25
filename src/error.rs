use core::fmt::{self, Display};

/// NVMe status code type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusCodeType {
    /// Generic command status
    Generic,
    /// Command specific status
    CommandSpecific,
    /// Media and data integrity errors
    MediaError,
    /// Path related errors
    PathError,
    /// Vendor specific
    VendorSpecific,
}

/// NVMe command status codes.
#[derive(Debug, Clone, Copy)]
pub struct StatusCode {
    /// Status code type
    pub sct: StatusCodeType,
    /// Status code value
    pub sc: u8,
}

impl StatusCode {
    /// Create a new status code.
    pub fn new(sct: StatusCodeType, sc: u8) -> Self {
        Self { sct, sc }
    }

    /// Parse from a raw status field.
    pub fn from_raw(status: u16) -> Self {
        let sc = ((status >> 1) & 0xFF) as u8;
        let sct_val = ((status >> 9) & 0x7) as u8;

        let sct = match sct_val {
            0 => StatusCodeType::Generic,
            1 => StatusCodeType::CommandSpecific,
            2 => StatusCodeType::MediaError,
            3 => StatusCodeType::PathError,
            7 => StatusCodeType::VendorSpecific,
            _ => StatusCodeType::Generic,
        };

        Self { sct, sc }
    }

    /// Get human-readable description.
    pub fn description(&self) -> &'static str {
        match (self.sct, self.sc) {
            // Generic command status
            (StatusCodeType::Generic, 0x00) => "Success",
            (StatusCodeType::Generic, 0x01) => "Invalid Command Opcode",
            (StatusCodeType::Generic, 0x02) => "Invalid Field in Command",
            (StatusCodeType::Generic, 0x03) => "Command ID Conflict",
            (StatusCodeType::Generic, 0x04) => "Data Transfer Error",
            (StatusCodeType::Generic, 0x05) => "Commands Aborted due to Power Loss Notification",
            (StatusCodeType::Generic, 0x06) => "Internal Error",
            (StatusCodeType::Generic, 0x07) => "Command Abort Requested",
            (StatusCodeType::Generic, 0x08) => "Command Aborted due to SQ Deletion",
            (StatusCodeType::Generic, 0x09) => "Command Aborted due to Failed Fused Command",
            (StatusCodeType::Generic, 0x0A) => "Command Aborted due to Missing Fused Command",
            (StatusCodeType::Generic, 0x0B) => "Invalid Namespace or Format",
            (StatusCodeType::Generic, 0x0C) => "Command Sequence Error",
            (StatusCodeType::Generic, 0x0D) => "Invalid SGL Segment Descriptor",
            (StatusCodeType::Generic, 0x0E) => "Invalid Number of SGL Descriptors",
            (StatusCodeType::Generic, 0x0F) => "Data SGL Length Invalid",
            (StatusCodeType::Generic, 0x10) => "Metadata SGL Length Invalid",
            (StatusCodeType::Generic, 0x11) => "SGL Descriptor Type Invalid",
            (StatusCodeType::Generic, 0x12) => "Invalid Use of Controller Memory Buffer",
            (StatusCodeType::Generic, 0x13) => "PRP Offset Invalid",
            (StatusCodeType::Generic, 0x14) => "Atomic Write Unit Exceeded",
            (StatusCodeType::Generic, 0x15) => "Operation Denied",
            (StatusCodeType::Generic, 0x16) => "SGL Offset Invalid",
            (StatusCodeType::Generic, 0x17) => "Host Identifier Inconsistent Format",
            (StatusCodeType::Generic, 0x18) => "Keep Alive Timeout Expired",
            (StatusCodeType::Generic, 0x19) => "Keep Alive Timeout Invalid",
            (StatusCodeType::Generic, 0x1A) => "Command Aborted due to Preemption",
            (StatusCodeType::Generic, 0x1B) => "Sanitize Failed",
            (StatusCodeType::Generic, 0x1C) => "Sanitize In Progress",
            (StatusCodeType::Generic, 0x1D) => "SGL Data Block Granularity Invalid",
            (StatusCodeType::Generic, 0x1E) => "Command Not Supported for Queue in CMB",
            (StatusCodeType::Generic, 0x1F) => "Namespace is Write Protected",
            (StatusCodeType::Generic, 0x20) => "Command Interrupted",
            (StatusCodeType::Generic, 0x21) => "Transient Transport Error",

            // Command specific errors
            (StatusCodeType::CommandSpecific, 0x00) => "Completion Queue Invalid",
            (StatusCodeType::CommandSpecific, 0x01) => "Invalid Queue Identifier",
            (StatusCodeType::CommandSpecific, 0x02) => "Invalid Queue Size",
            (StatusCodeType::CommandSpecific, 0x03) => "Abort Command Limit Exceeded",
            (StatusCodeType::CommandSpecific, 0x04) => "Reserved",
            (StatusCodeType::CommandSpecific, 0x05) => "Asynchronous Event Request Limit Exceeded",
            (StatusCodeType::CommandSpecific, 0x06) => "Invalid Firmware Slot",
            (StatusCodeType::CommandSpecific, 0x07) => "Invalid Firmware Image",
            (StatusCodeType::CommandSpecific, 0x08) => "Invalid Interrupt Vector",
            (StatusCodeType::CommandSpecific, 0x09) => "Invalid Log Page",
            (StatusCodeType::CommandSpecific, 0x0A) => "Invalid Format",
            (StatusCodeType::CommandSpecific, 0x0B) => "Firmware Activation Requires Conventional Reset",
            (StatusCodeType::CommandSpecific, 0x0C) => "Invalid Queue Deletion",
            (StatusCodeType::CommandSpecific, 0x0D) => "Feature Identifier Not Saveable",
            (StatusCodeType::CommandSpecific, 0x0E) => "Feature Not Changeable",
            (StatusCodeType::CommandSpecific, 0x0F) => "Feature Not Namespace Specific",
            (StatusCodeType::CommandSpecific, 0x10) => "Firmware Activation Requires NVM Subsystem Reset",
            (StatusCodeType::CommandSpecific, 0x11) => "Firmware Activation Requires Reset",
            (StatusCodeType::CommandSpecific, 0x12) => "Firmware Activation Requires Maximum Time Violation",
            (StatusCodeType::CommandSpecific, 0x13) => "Firmware Activation Prohibited",
            (StatusCodeType::CommandSpecific, 0x14) => "Overlapping Range",
            (StatusCodeType::CommandSpecific, 0x15) => "Namespace Insufficient Capacity",
            (StatusCodeType::CommandSpecific, 0x16) => "Namespace Identifier Unavailable",
            (StatusCodeType::CommandSpecific, 0x18) => "Namespace Already Attached",
            (StatusCodeType::CommandSpecific, 0x19) => "Namespace Is Private",
            (StatusCodeType::CommandSpecific, 0x1A) => "Namespace Not Attached",
            (StatusCodeType::CommandSpecific, 0x1B) => "Thin Provisioning Not Supported",
            (StatusCodeType::CommandSpecific, 0x1C) => "Controller List Invalid",
            (StatusCodeType::CommandSpecific, 0x1D) => "Device Self-test In Progress",
            (StatusCodeType::CommandSpecific, 0x1E) => "Boot Partition Write Prohibited",
            (StatusCodeType::CommandSpecific, 0x1F) => "Invalid Controller Identifier",
            (StatusCodeType::CommandSpecific, 0x20) => "Invalid Secondary Controller State",
            (StatusCodeType::CommandSpecific, 0x21) => "Invalid Number of Controller Resources",
            (StatusCodeType::CommandSpecific, 0x22) => "Invalid Resource Identifier",
            (StatusCodeType::CommandSpecific, 0x23) => "Sanitize Prohibited While Persistent Memory Region is Enabled",
            (StatusCodeType::CommandSpecific, 0x24) => "ANA Group Identifier Invalid",
            (StatusCodeType::CommandSpecific, 0x25) => "ANA Attach Failed",

            // Media and data integrity errors
            (StatusCodeType::MediaError, 0x80) => "Write Fault",
            (StatusCodeType::MediaError, 0x81) => "Unrecovered Read Error",
            (StatusCodeType::MediaError, 0x82) => "End-to-End Guard Check Error",
            (StatusCodeType::MediaError, 0x83) => "End-to-End Application Tag Check Error",
            (StatusCodeType::MediaError, 0x84) => "End-to-End Reference Tag Check Error",
            (StatusCodeType::MediaError, 0x85) => "Compare Failure",
            (StatusCodeType::MediaError, 0x86) => "Access Denied",
            (StatusCodeType::MediaError, 0x87) => "Deallocated or Unwritten Logical Block",

            // Path related errors (NVMe 2.3)
            (StatusCodeType::PathError, 0x00) => "Internal Path Error",
            (StatusCodeType::PathError, 0x01) => "Asymmetric Access Persistent Loss",
            (StatusCodeType::PathError, 0x02) => "Asymmetric Access Inaccessible",
            (StatusCodeType::PathError, 0x03) => "Asymmetric Access Transition",
            (StatusCodeType::PathError, 0x60) => "Controller Pathing Error",
            (StatusCodeType::PathError, 0x70) => "Host Pathing Error",
            (StatusCodeType::PathError, 0x71) => "Command Aborted By Host",

            _ => "Unknown Error",
        }
    }
}

/// Contains all possible errors that can occur in the NVMe driver.
#[derive(Debug)]
pub enum Error {
    /// The submission queue is full.
    SubQueueFull,
    /// Buffer size must be a multiple of the block size.
    InvalidBufferSize,
    /// Target address must be aligned to dword.
    NotAlignedToDword,
    /// Target address must be aligned to minimum page size.
    NotAlignedToPage,
    /// Single IO size should be less than maximum data transfer size (MDTS).
    IoSizeExceedsMdts,
    /// The queue size is less than 2.
    QueueSizeTooSmall,
    /// The queue size exceeds the maximum queue entry size (MQES).
    QueueSizeExceedsMqes,
    /// Command failed with a specific status code.
    CommandFailed(u16),
    /// Invalid namespace ID.
    InvalidNamespace,
    /// Feature configuration not set.
    InvalidFeatureConfig,
    /// Asynchronous event limit exceeded.
    AsyncEventLimitExceeded,
    /// Keep alive timeout.
    KeepAliveTimeout,
    /// Path failure detected.
    PathFailure,
    /// Power limit exceeded.
    PowerLimitExceeded,
    /// Sanitize operation in progress.
    SanitizeInProgress,
    /// Firmware update failed.
    FirmwareUpdateFailed,
    /// Security command failed.
    SecurityCommandFailed,
    /// NVMe status code error.
    NvmeStatus(StatusCode),
    /// Device is shutting down.
    DeviceShuttingDown,
    /// Failed to create I/O queues.
    QueueCreationFailed,
    /// Invalid queue ID specified.
    InvalidQueueId,
    /// Queue not found.
    QueueNotFound,
    /// Cannot remove the last queue.
    LastQueueCannotBeRemoved,
    /// Invalid queue count.
    InvalidQueueCount,
    /// Too many queues requested.
    TooManyQueues,
    /// No active queues available.
    NoActiveQueues,
}

impl core::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::SubQueueFull => {
                write!(f, "The submission queue is full")
            }
            Error::InvalidBufferSize => {
                write!(f, "Buffer size must be a multiple of the block size.")
            }
            Error::NotAlignedToDword => {
                write!(f, "Target address must be aligned to dword")
            }
            Error::NotAlignedToPage => {
                write!(f, "Target address must be aligned to minimum page size")
            }
            Error::IoSizeExceedsMdts => {
                write!(f, "Single IO size exceeds maximum data transfer size")
            }
            Error::QueueSizeTooSmall => {
                write!(f, "The queue size is less than 2")
            }
            Error::QueueSizeExceedsMqes => {
                write!(f, "The queue size exceeds the maximum queue entry size")
            }
            Error::CommandFailed(code) => {
                write!(f, "Command failed with status code: {}", code)
            }
            Error::InvalidNamespace => {
                write!(f, "Invalid namespace ID")
            }
            Error::InvalidFeatureConfig => {
                write!(f, "Feature configuration not set")
            }
            Error::AsyncEventLimitExceeded => {
                write!(f, "Asynchronous event limit exceeded")
            }
            Error::KeepAliveTimeout => {
                write!(f, "Keep alive timeout")
            }
            Error::PathFailure => {
                write!(f, "Path failure detected")
            }
            Error::PowerLimitExceeded => {
                write!(f, "Power limit exceeded")
            }
            Error::SanitizeInProgress => {
                write!(f, "Sanitize operation in progress")
            }
            Error::FirmwareUpdateFailed => {
                write!(f, "Firmware update failed")
            }
            Error::SecurityCommandFailed => {
                write!(f, "Security command failed")
            }
            Error::NvmeStatus(code) => {
                write!(f, "NVMe error: {}", code.description())
            }
            Error::DeviceShuttingDown => {
                write!(f, "Device is shutting down")
            }
            Error::QueueCreationFailed => {
                write!(f, "Failed to create I/O queues")
            }
            Error::InvalidQueueId => {
                write!(f, "Invalid queue ID specified")
            }
            Error::QueueNotFound => {
                write!(f, "Queue not found")
            }
            Error::LastQueueCannotBeRemoved => {
                write!(f, "Cannot remove the last I/O queue")
            }
            Error::InvalidQueueCount => {
                write!(f, "Invalid queue count")
            }
            Error::TooManyQueues => {
                write!(f, "Too many queues requested")
            }
            Error::NoActiveQueues => {
                write!(f, "No active I/O queues available")
            }
        }
    }
}

/// Result type for NVMe operations.
pub type Result<T> = core::result::Result<T, Error>;
