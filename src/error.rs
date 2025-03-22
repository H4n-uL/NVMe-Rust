use core::error::Error;
use core::fmt::{self, Display};

/// Contains all possible errors that can occur in the NVMe driver.
#[derive(Debug)]
pub enum NvmeError {
    /// The submission queue is full.
    QueueFull,

    /// Length must be a multiple of block size.
    InvalidBufferSize,

    /// Target address must be aligned to dword.
    NotAlignedToDword,

    /// Target address must be aligned to minimum page size.
    NotAlignedToPage,

    /// Single IO size should be less than maximum data transfer size (MDTS).
    ///
    /// The transfer size limit can be found in the `NvmeControllerData`.
    IoSizeExceedsMdts,

    /// Command failed with a specific status code.
    CommandFailed(u16),
}

impl Error for NvmeError {}

impl Display for NvmeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NvmeError::QueueFull => {
                write!(f, "The submission queue is full")
            }
            NvmeError::InvalidBufferSize => {
                write!(f, "Length must be a multiple of block size")
            }
            NvmeError::NotAlignedToDword => {
                write!(f, "Target address must be aligned to dword")
            }
            NvmeError::NotAlignedToPage => {
                write!(f, "Target address must be aligned to minimum page size")
            }
            NvmeError::IoSizeExceedsMdts => {
                write!(f, "Single IO size should be less than MDTS")
            }
            NvmeError::CommandFailed(code) => {
                write!(f, "Command failed with status code: {}", code)
            }
        }
    }
}

/// Result type for NVMe operations.
pub type Result<T> = core::result::Result<T, NvmeError>;
