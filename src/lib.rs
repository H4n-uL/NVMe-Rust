//! A no-std compatible NVMe driver for embedded and operating system development.
//!
//! This crate provides functionality for interacting with NVMe
//! (Non-Volatile Memory Express) storage devices in environments without
//! the standard library, such as kernels, bootloaders, or embedded systems.
#![no_std]

extern crate alloc;

mod cmd;
mod device;
mod error;
mod nvme;
mod memory;
mod queues;

pub use device::{NvmeControllerData, NvmeDevice};
pub use error::NvmeError;
pub use memory::NvmeAllocator;
