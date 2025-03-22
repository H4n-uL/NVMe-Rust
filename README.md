# NVMe

A no-std compatible NVMe driver for embedded and operating system development.

## Usage

```rust
use alloc::boxed::Box;
use alloc::alloc::Layout;
use core::alloc::GlobalAlloc;
use nvme::{NvmeAllocator, NvmeDevice};

pub struct Allocator;

impl NvmeAllocator for Allocator {
    unsafe fn allocate(&self, size: usize) -> usize {
        DmaManager::allocate(size)
    }

    unsafe fn deallocate(&self, addr: usize) {
        DmaManager::deallocate(addr);
    }

    fn translate(&self, addr: usize) -> usize {
        DmaManager::translate_addr(addr)
    }
}

pub fn nvme_test() -> Result<(), Box<dyn core::error::Error>> {
    // Init the NVMe controller
    let controller = NvmeDevice::init(virtual_address, Allocator)?;

    // Identify all namespaces (base 0)
    let namespaces = controller.identify_namespaces(0)?;

    // Select the first namespace
    let namespace = &namespaces[0];

    // You can get the block size and count of the namespace
    let _disk_size = namespace.block_count * namespace.block_size;

    // Create a io queue pair to perform IO operations
    let mut qpair = controller.create_io_queue_pair(namespace, 64)?;

    // Should not be larger than controller.controller_data.max_transfer_size
    const TEST_LENGTH: usize = 524288;

    // Create a 4096 byte-aligned read buffer
    let layout = Layout::from_size_align(TEST_LENGTH, 4096)?;
    let read_buffer_ptr = unsafe { ALLOCATOR.alloc(layout) };
    let read_buffer = unsafe { core::slice::from_raw_parts_mut(read_buffer_ptr, TEST_LENGTH) };

    // Read `TEST_LENGTH` bytes starting from LBA 34
    qpair.read(read_buffer.as_mut_ptr(), read_buffer.len(), 34)?;

    // Create a 4096 byte-aligned write buffer
    let write_buffer_ptr = unsafe { ALLOCATOR.alloc(layout) };
    let write_buffer = unsafe { core::slice::from_raw_parts_mut(write_buffer_ptr, TEST_LENGTH) };

    // Fill the write buffer with data
    for i in 0..TEST_LENGTH {
        write_buffer[i] = (i % 256) as u8;
    }

    // Write the buffer to the disk starting from LBA 34
    qpair.write(write_buffer.as_ptr(), write_buffer.len(), 34)?;

    // Read back the data to verify correctness
    qpair.read(read_buffer.as_mut_ptr(), read_buffer.len(), 34)?;

    // Verify the data byte-by-byte
    for (i, (read, write)) in read_buffer.iter().zip(write_buffer.iter()).enumerate() {
        if read != write {
            eprintln!("Write test: Mismatch at index {i}: {read} != {write}");
            break;
        }
    }

    Ok(())
}
```
