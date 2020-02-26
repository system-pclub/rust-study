use core::ptr;

static PT_BASE: u64 = 0x70000;

pub unsafe fn paging() {
    // Zero PML4, PDP, and 4 PD
    ptr::write_bytes(PT_BASE as *mut u8, 0, 6 * 4096);

    let mut base = PT_BASE;

    // Link first PML4 and second to last PML4 to PDP
    ptr::write(base as *mut u64, (PT_BASE + 0x1000) | 1 << 1 | 1);
    ptr::write((base + 510*8) as *mut u64, (PT_BASE + 0x1000) | 1 << 1 | 1);
    // Link last PML4 to PML4
    ptr::write((base + 511*8) as *mut u64, PT_BASE | 1 << 1 | 1);

    // Move to PDP
    base += 4096;

    // Link first four PDP to PD
    ptr::write(base as *mut u64, (PT_BASE + 0x2000) | 1 << 1 | 1);
    ptr::write((base + 8) as *mut u64, (PT_BASE + 0x3000) | 1 << 1 | 1);
    ptr::write((base + 16) as *mut u64, (PT_BASE + 0x4000) | 1 << 1 | 1);
    ptr::write((base + 24) as *mut u64, (PT_BASE + 0x5000) | 1 << 1 | 1);

    // Move to PD
    base += 4096;

    // Link all PD's (512 per PDP, 2MB each)
    let mut entry = 1 << 7 | 1 << 1 | 1;
    for i in 0..4*512 {
        ptr::write((base + i*8) as *mut u64, entry);
        entry += 0x200000;
    }
}
