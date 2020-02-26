use core::ptr;
use coreboot_table::Framebuffer;

static VBE_BASE: u64 = 0x5200;

/// The info of the VBE mode
#[derive(Copy, Clone, Default, Debug)]
#[repr(packed)]
pub struct VBEModeInfo {
    attributes: u16,
    win_a: u8,
    win_b: u8,
    granularity: u16,
    winsize: u16,
    segment_a: u16,
    segment_b: u16,
    winfuncptr: u32,
    bytesperscanline: u16,
    pub xresolution: u16,
    pub yresolution: u16,
    xcharsize: u8,
    ycharsize: u8,
    numberofplanes: u8,
    bitsperpixel: u8,
    numberofbanks: u8,
    memorymodel: u8,
    banksize: u8,
    numberofimagepages: u8,
    unused: u8,
    redmasksize: u8,
    redfieldposition: u8,
    greenmasksize: u8,
    greenfieldposition: u8,
    bluemasksize: u8,
    bluefieldposition: u8,
    rsvdmasksize: u8,
    rsvdfieldposition: u8,
    directcolormodeinfo: u8,
    pub physbaseptr: u32,
    offscreenmemoryoffset: u32,
    offscreenmemsize: u16,
}

pub unsafe fn vesa(framebuffer_opt: Option<&Framebuffer>) {
    let mut mode_info = VBEModeInfo::default();

    if let Some(framebuffer) = framebuffer_opt {
        mode_info.xresolution = framebuffer.x_resolution as u16;
        mode_info.yresolution = framebuffer.y_resolution as u16;
        mode_info.physbaseptr = framebuffer.physical_address as u32;
    }

    ptr::write(VBE_BASE as *mut VBEModeInfo, mode_info);
}
