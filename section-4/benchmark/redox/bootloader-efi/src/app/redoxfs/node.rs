use core::{fmt, mem, ops, slice, str};

use super::{Extent, BLOCK_SIZE};

/// A file/folder node
#[repr(packed)]
pub struct Node {
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub ctime: u64,
    pub ctime_nsec: u32,
    pub mtime: u64,
    pub mtime_nsec: u32,
    pub atime: u64,
    pub atime_nsec: u32,
    pub name: [u8; 226],
    pub parent: u64,
    pub next: u64,
    pub extents: [Extent; (BLOCK_SIZE as usize - 288)/16],
}

impl Node {
    pub const MODE_TYPE: u16 = 0xF000;
    pub const MODE_FILE: u16 = 0x8000;
    pub const MODE_DIR: u16 = 0x4000;
    pub const MODE_SYMLINK: u16 = 0xA000;

    pub const MODE_PERM: u16 = 0x0FFF;
    pub const MODE_EXEC: u16 = 0o1;
    pub const MODE_WRITE: u16 = 0o2;
    pub const MODE_READ: u16 = 0o4;

    pub fn default() -> Node {
        Node {
            mode: 0,
            uid: 0,
            gid: 0,
            ctime: 0,
            ctime_nsec: 0,
            mtime: 0,
            mtime_nsec: 0,
            atime: 0,
            atime_nsec: 0,
            name: [0; 226],
            parent: 0,
            next: 0,
            extents: [Extent::default(); (BLOCK_SIZE as usize - 288)/16],
        }
    }

    pub fn name(&self) -> Result<&str, str::Utf8Error> {
        let mut len = 0;

        for &b in self.name.iter() {
            if b == 0 {
                break;
            }
            len += 1;
        }

        str::from_utf8(&self.name[..len])
    }

    pub fn is_dir(&self) -> bool {
        self.mode & Node::MODE_TYPE == Node::MODE_DIR
    }

    pub fn is_file(&self) -> bool {
        self.mode & Node::MODE_TYPE == Node::MODE_FILE
    }

    pub fn is_symlink(&self) -> bool {
        self.mode & Node::MODE_TYPE == Node::MODE_SYMLINK
    }

    pub fn owner(&self, uid: u32) -> bool {
        uid == 0 || self.uid == uid
    }

    pub fn permission(&self, uid: u32, gid: u32, op: u16) -> bool {
        let mut perm = self.mode & 0o7;
        if self.uid == uid {
            perm |= (self.mode >> 6) & 0o7;
        }
        if self.gid == gid || gid == 0 {
            perm |= (self.mode >> 3) & 0o7;
        }
        if uid == 0 {
            perm |= 0o7;
        }
        perm & op == op
    }

    pub fn size(&self) -> u64 {
        self.extents.iter().fold(0, |size, extent| size + extent.length)
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let extents: Vec<&Extent> = self.extents.iter().filter(|extent| -> bool { extent.length > 0 }).collect();
        unsafe {
            f.debug_struct("Node")
                .field("mode", &self.mode)
                .field("uid", &self.uid)
                .field("gid", &self.gid)
                .field("ctime", &self.ctime)
                .field("ctime_nsec", &self.ctime_nsec)
                .field("mtime", &self.mtime)
                .field("mtime_nsec", &self.mtime_nsec)
                .field("atime", &self.atime)
                .field("atime_nsec", &self.atime_nsec)
                .field("name", &self.name())
                .field("next", &self.next)
                .field("extents", &extents)
                .finish()
        }
    }
}

impl ops::Deref for Node {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const Node as *const u8, mem::size_of::<Node>()) as &[u8]
        }
    }
}

impl ops::DerefMut for Node {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(self as *mut Node as *mut u8, mem::size_of::<Node>()) as &mut [u8]
        }
    }
}

#[test]
fn node_size_test() {
    assert_eq!(mem::size_of::<Node>(), BLOCK_SIZE as usize);
}
