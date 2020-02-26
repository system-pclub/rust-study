use core::ops::{Deref, DerefMut};
use core::{mem, slice};
use crate::flag::{EventFlags, MapFlags, PtraceFlags, SigActionFlags};

#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
pub struct Event {
    pub id: usize,
    pub flags: EventFlags,
    pub data: usize
}

impl Deref for Event {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const Event as *const u8, mem::size_of::<Event>())
        }
    }
}

impl DerefMut for Event {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self as *mut Event as *mut u8, mem::size_of::<Event>())
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
pub struct ITimerSpec {
    pub it_interval: TimeSpec,
    pub it_value: TimeSpec,
}

impl Deref for ITimerSpec {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const ITimerSpec as *const u8,
                                  mem::size_of::<ITimerSpec>())
        }
    }
}

impl DerefMut for ITimerSpec {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self as *mut ITimerSpec as *mut u8,
                                      mem::size_of::<ITimerSpec>())
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
pub struct Map {
    pub offset: usize,
    pub size: usize,
    pub flags: MapFlags,
}

impl Deref for Map {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const Map as *const u8, mem::size_of::<Map>())
        }
    }
}

impl DerefMut for Map {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self as *mut Map as *mut u8, mem::size_of::<Map>())
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
pub struct Packet {
    pub id: u64,
    pub pid: usize,
    pub uid: u32,
    pub gid: u32,
    pub a: usize,
    pub b: usize,
    pub c: usize,
    pub d: usize
}

impl Deref for Packet {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const Packet as *const u8, mem::size_of::<Packet>())
        }
    }
}

impl DerefMut for Packet {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self as *mut Packet as *mut u8, mem::size_of::<Packet>())
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C)]
pub struct SigAction {
    pub sa_handler: Option<extern "C" fn(usize)>,
    pub sa_mask: [u64; 2],
    pub sa_flags: SigActionFlags,
}

#[allow(dead_code)]
unsafe fn _assert_size_of_function_is_sane() {
    // Transmuting will complain *at compile time* if sizes differ.
    // Rust forbids a fn-pointer from being 0 so to allow SIG_DFL to
    // exist, we use Option<extern "C" fn(usize)> which will mean 0
    // becomes None
    let _ = mem::transmute::<Option<extern "C" fn(usize)>, usize>(None);
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_mode: u16,
    pub st_nlink: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub st_size: u64,
    pub st_blksize: u32,
    pub st_blocks: u64,
    pub st_mtime: u64,
    pub st_mtime_nsec: u32,
    pub st_atime: u64,
    pub st_atime_nsec: u32,
    pub st_ctime: u64,
    pub st_ctime_nsec: u32,
}

impl Deref for Stat {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const Stat as *const u8,
                                  mem::size_of::<Stat>())
        }
    }
}

impl DerefMut for Stat {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self as *mut Stat as *mut u8,
                                      mem::size_of::<Stat>())
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C)]
pub struct StatVfs {
    pub f_bsize: u32,
    pub f_blocks: u64,
    pub f_bfree: u64,
    pub f_bavail: u64,
}

impl Deref for StatVfs {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const StatVfs as *const u8,
                                  mem::size_of::<StatVfs>())
        }
    }
}

impl DerefMut for StatVfs {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self as *mut StatVfs as *mut u8,
                                      mem::size_of::<StatVfs>())
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
#[repr(C)]
pub struct TimeSpec {
    pub tv_sec: i64,
    pub tv_nsec: i32,
}

impl Deref for TimeSpec {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const TimeSpec as *const u8,
                                  mem::size_of::<TimeSpec>())
        }
    }
}

impl DerefMut for TimeSpec {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self as *mut TimeSpec as *mut u8,
                                      mem::size_of::<TimeSpec>())
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
#[cfg(target_arch = "x86_64")]
pub struct IntRegisters {
    // TODO: Some of these don't get set by Redox yet. Should they?

    pub r15: usize,
    pub r14: usize,
    pub r13: usize,
    pub r12: usize,
    pub rbp: usize,
    pub rbx: usize,
    pub r11: usize,
    pub r10: usize,
    pub r9: usize,
    pub r8: usize,
    pub rax: usize,
    pub rcx: usize,
    pub rdx: usize,
    pub rsi: usize,
    pub rdi: usize,
    // pub orig_rax: usize,
    pub rip: usize,
    pub cs: usize,
    pub rflags: usize,
    pub rsp: usize,
    pub ss: usize,
    // pub fs_base: usize,
    // pub gs_base: usize,
    // pub ds: usize,
    // pub es: usize,
    pub fs: usize,
    // pub gs: usize
}

impl Deref for IntRegisters {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const IntRegisters as *const u8, mem::size_of::<IntRegisters>())
        }
    }
}

impl DerefMut for IntRegisters {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self as *mut IntRegisters as *mut u8, mem::size_of::<IntRegisters>())
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(packed)]
#[cfg(target_arch = "x86_64")]
pub struct FloatRegisters {
    pub fcw: u16,
    pub fsw: u16,
    pub ftw: u8,
    pub _reserved: u8,
    pub fop: u16,
    pub fip: u64,
    pub fdp: u64,
    pub mxcsr: u32,
    pub mxcsr_mask: u32,
    pub st_space: [u128; 8],
    pub xmm_space: [u128; 16]
}

impl Deref for FloatRegisters {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const FloatRegisters as *const u8, mem::size_of::<FloatRegisters>())
        }
    }
}

impl DerefMut for FloatRegisters {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self as *mut FloatRegisters as *mut u8, mem::size_of::<FloatRegisters>())
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct PtraceEvent {
    pub cause: PtraceFlags,
    pub a: usize,
    pub b: usize,
    pub c: usize,
    pub d: usize,
    pub e: usize,
    pub f: usize
}

impl Deref for PtraceEvent {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const PtraceEvent as *const u8, mem::size_of::<PtraceEvent>())
        }
    }
}

impl DerefMut for PtraceEvent {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self as *mut PtraceEvent as *mut u8, mem::size_of::<PtraceEvent>())
        }
    }
}

#[macro_export]
macro_rules! ptrace_event {
    ($cause:expr $(, $a:expr $(, $b:expr $(, $c:expr)?)?)?) => {
        $crate::data::PtraceEvent {
            cause: $cause,
            $(a: $a,
              $(b: $b,
                $(c: $c,)?
              )?
            )?
            ..Default::default()
        }
    }
}
