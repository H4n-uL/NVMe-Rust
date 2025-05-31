# NVMe

A no-std compatible NVMe driver for embedded and operating system development.

## Usage

You need create a allocator that implements the `NVMeAllocator` trait.

```rust
pub struct NVMeAllocator;

impl Allocator for NVMeAllocator {
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
```

Here is a complete example of a full routine that initializes the NVMe controller, identifies namespaces, and performs read/write operations.

```rust
pub fn nvme_test() -> Result<(), Box<dyn core::error::Error>> {
    // Init the NVMe controller
    let controller = Device::init(virtual_address, Allocator)?;

    // Some useful data you may want to see
    let _controller_data = controller.controller_data();

    // Select the first namespace
    let namespace = controller.get_ns(1)?;

    // You can get the block size and count of the namespace
    let _disk_size = namespace.total_size();

    // Should not be larger than controller_data.max_transfer_size
    const TEST_LENGTH: usize = 524288;

    // Create a 4096 byte aligned read buffer
    let layout = Layout::from_size_align(TEST_LENGTH, 4096)?;
    let read_buffer_ptr = unsafe { ALLOCATOR.alloc(layout) };
    let read_buffer = unsafe { core::slice::from_raw_parts_mut(read_buffer_ptr, TEST_LENGTH) };

    // Read `TEST_LENGTH` bytes starting from LBA 34
    namespace.read(34, &mut read_buffer)?;

    // Create a 4096 byte aligned write buffer
    let write_buffer_ptr = unsafe { ALLOCATOR.alloc(layout) };
    let write_buffer = unsafe { core::slice::from_raw_parts_mut(write_buffer_ptr, TEST_LENGTH) };

    // Fill the write buffer with data
    for i in 0..TEST_LENGTH {
        write_buffer[i] = (i % 256) as u8;
    }

    // Write the buffer to the disk starting from LBA 34
    namespace.write(34, &write_buffer)?;

    // Read back the data to verify correctness
    namespace.read(34, &mut read_buffer)?;

    // Verify the data byte-by-byte
    for (i, (read, write)) in read_buffer.iter().zip(write_buffer.iter()).enumerate() {
        if read != write {
            eprintln!("Write test: Mismatch at index {i}: {read} != {write}");
            break;
        }
    }

    // Don't forget to free the buffer
    unsafe {
        ALLOCATOR.dealloc(read_buffer_ptr, layout);
        ALLOCATOR.dealloc(write_buffer_ptr, layout);
    }

    Ok(())
}
```
