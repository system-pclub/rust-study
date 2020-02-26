#[cfg(target_arch = "x86")]
#[macro_use]
pub mod x86;
#[cfg(target_arch = "x86")]
pub use self::x86::*;
