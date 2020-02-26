use core::ptr;
use coreboot_table::{Memory, MemoryRangeKind};

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

pub unsafe fn memory_map(memory: &Memory) {
    ptr::write_bytes(MM_BASE as *mut u8, 0, MM_SIZE as usize);

    for (i, range) in memory.ranges().iter().enumerate() {
        let bios_type = match range.kind {
            MemoryRangeKind::Ram => {
                MEMORY_AREA_FREE
            },
            _ => {
                MEMORY_AREA_RESERVED
            }
        };

        let bios_area = MemoryArea {
            base_addr: range.start.unpack(),
            length: range.size.unpack(),
            _type: bios_type,
            acpi: 0,
        };

        ptr::write((MM_BASE as *mut MemoryArea).offset(i as isize), bios_area);
    }
}
