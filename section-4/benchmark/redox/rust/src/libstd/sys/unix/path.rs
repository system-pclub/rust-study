use crate::path::Prefix;
use crate::ffi::OsStr;

#[inline]
pub fn is_sep_byte(b: u8) -> bool {
    b == b'/'
}

#[inline]
pub fn is_verbatim_sep(b: u8) -> bool {
    b == b'/'
}

pub fn parse_prefix(path: &OsStr) -> Option<Prefix<'_>> {
    if cfg!(target_os = "redox") {
        if let Some(path_str) = path.to_str() {
            if let Some(i) = path_str.find(':') {
                return Some(Prefix::Scheme(OsStr::new(&path_str[..i])));
            }
        }
    }
    None
}

pub const MAIN_SEP_STR: &str = "/";
pub const MAIN_SEP: char = '/';
