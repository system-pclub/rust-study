use core::{mem, ptr};
use uefi::memory::{MemoryDescriptor, MemoryType};

static MM_BASE: u64 = 0x500;
static MM_SIZE: u64 = 0x4B00;

/// Memory does not exist
pub const MEMORY_AREA_NULL: u32 = 0;

/// Memory is free to use
pub const MEMORY_AREA_FREE: u32 = 1;

/// Memory is reserved
pub const MEMORY_AREA_RESERVED: u32 = 2;

/// Memory is used by ACPI, and can be reclaimed
pub const MEMORY_AREA_ACPI: u32 = 3;

/// A memory map area
#[derive(Copy, Clone, Debug, Default)]
#[repr(packed)]
pub struct MemoryArea {
    pub base_addr: u64,
    pub length: u64,
    pub _type: u32,
    pub acpi: u32
}

pub unsafe fn memory_map() -> usize {
    let uefi = std::system_table();

    ptr::write_bytes(MM_BASE as *mut u8, 0, MM_SIZE as usize);

    let mut map: [u8; 65536] = [0; 65536];
    let mut map_size = map.len();
    let mut map_key = 0;
    let mut descriptor_size = 0;
    let mut descriptor_version = 0;
    let _ = (uefi.BootServices.GetMemoryMap)(
        &mut map_size,
        map.as_mut_ptr() as *mut MemoryDescriptor,
        &mut map_key,
        &mut descriptor_size,
        &mut descriptor_version
    );

    if descriptor_size >= mem::size_of::<MemoryDescriptor>() {
        for i in 0..map_size/descriptor_size {
            let descriptor_ptr = map.as_ptr().offset((i * descriptor_size) as isize);
            let descriptor = & *(descriptor_ptr as *const MemoryDescriptor);
            let descriptor_type: MemoryType = mem::transmute(descriptor.Type);

            let bios_type = match descriptor_type {
                MemoryType::EfiLoaderCode |
                MemoryType::EfiLoaderData |
                MemoryType::EfiBootServicesCode |
                MemoryType::EfiBootServicesData |
                MemoryType::EfiConventionalMemory => {
                    MEMORY_AREA_FREE
                },
                _ => {
                    MEMORY_AREA_RESERVED
                }
            };

            let bios_area = MemoryArea {
                base_addr: descriptor.PhysicalStart.0,
                length: descriptor.NumberOfPages * 4096,
                _type: bios_type,
                acpi: 0,
            };

            ptr::write((MM_BASE as *mut MemoryArea).offset(i as isize), bios_area);
        }
    } else {
        println!("Unknown memory descriptor size: {}", descriptor_size);
    }

    map_key
}
