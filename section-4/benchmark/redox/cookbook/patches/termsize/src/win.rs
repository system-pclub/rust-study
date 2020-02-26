extern crate winapi;
extern crate kernel32;

use self::super::Size;
use std::ptr;

/// Gets the current terminal size
pub fn get() -> Option<Size> {
    // http://rosettacode.org/wiki/Terminal_control/Dimensions#Windows
    use self::kernel32::{self, GetConsoleScreenBufferInfo};
    use self::winapi::{self, CONSOLE_SCREEN_BUFFER_INFO, HANDLE,
                       INVALID_HANDLE_VALUE};
    let handle: HANDLE = unsafe {
        kernel32::CreateFileA(
            b"CONOUT$\0".as_ptr() as *const i8,
            winapi::GENERIC_READ | winapi::GENERIC_WRITE,
            winapi::FILE_SHARE_WRITE,
            ptr::null_mut(),
            winapi::OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let info = unsafe {
        // https://msdn.microsoft.com/en-us/library/windows/desktop/ms683171(v=vs.85).aspx
        let mut info = ::std::mem::uninitialized();
        if GetConsoleScreenBufferInfo(handle, &mut info) == 0 {
            None
        } else {
            Some(info)
        }
    };
    info.map(|inf| {
        Size {
            rows: (inf.srWindow.Bottom - inf.srWindow.Top + 1) as u16,
            cols: (inf.srWindow.Right - inf.srWindow.Left + 1) as u16,
        }
    })
}
