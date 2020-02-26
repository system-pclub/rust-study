use crate::paging::ActivePageTable;

pub mod cpu;
pub mod local_apic;
pub mod pic;
pub mod pit;
pub mod rtc;
pub mod serial;
#[cfg(feature = "acpi")]
pub mod hpet;

pub unsafe fn init(active_table: &mut ActivePageTable){
    pic::init();
    local_apic::init(active_table);
}

#[cfg(feature = "acpi")]
unsafe fn init_hpet() -> bool {
    use crate::acpi::ACPI_TABLE;
    if let Some(ref mut hpet) = *ACPI_TABLE.hpet.write() {
        hpet::init(hpet)
    } else {
        false
    }
}

#[cfg(not(feature = "acpi"))]
unsafe fn init_hpet() -> bool {
    false
}

pub unsafe fn init_noncore() {
    if ! init_hpet() {
        pit::init();
    }

    rtc::init();
    serial::init();
}

pub unsafe fn init_ap() {
    local_apic::init_ap();
}
