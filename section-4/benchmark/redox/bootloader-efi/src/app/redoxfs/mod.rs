pub use self::disk::Disk;
pub use self::extent::Extent;
pub use self::filesystem::FileSystem;
pub use self::header::Header;
pub use self::node::Node;

mod disk;
mod extent;
mod filesystem;
mod header;
mod node;

pub const BLOCK_SIZE: u64 = 4096;
pub const SIGNATURE: &'static [u8; 8] = b"RedoxFS\0";
pub const VERSION: u64 = 4;
