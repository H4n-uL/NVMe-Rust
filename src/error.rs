use core::fmt::{self, Display};

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
        }
    }
}

/// Result type for NVMe operations.
pub type Result<T> = core::result::Result<T, Error>;
