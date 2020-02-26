use core::cmp::min;

use uefi::status::{Error, Result};

use super::{BLOCK_SIZE, Disk, Extent, Header, Node};

/// A file system
pub struct FileSystem {
    pub disk: Disk,
    pub block: u64,
    pub header: (u64, Header)
}

impl FileSystem {
    /// Open a file system on a disk
    pub fn open(disk: Disk) -> Result<Self> {
        for block in 0..65536 {
            let mut header = (0, Header::default());
            disk.read_at(block + header.0, &mut header.1)?;

            if header.1.valid() {
                let mut root = (header.1.root, Node::default());
                disk.read_at(block + root.0, &mut root.1)?;

                let mut free = (header.1.free, Node::default());
                disk.read_at(block + free.0, &mut free.1)?;

                return Ok(FileSystem {
                    disk: disk,
                    block: block,
                    header: header
                });
            }
        }

        Err(Error::NotFound)
    }

    pub fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        self.disk.read_at(self.block + block, buffer)
    }

    pub fn node(&mut self, block: u64) -> Result<(u64, Node)> {
        let mut node = Node::default();
        self.read_at(block, &mut node)?;
        Ok((block, node))
    }

    pub fn child_nodes(&mut self, children: &mut Vec<(u64, Node)>, parent_block: u64) -> Result<()> {
        if parent_block == 0 {
            return Ok(());
        }

        let parent = self.node(parent_block)?;
        for extent in parent.1.extents.iter() {
            for (block, size) in extent.blocks() {
                if size >= BLOCK_SIZE {
                    children.push(self.node(block)?);
                }
            }
        }

        self.child_nodes(children, parent.1.next)
    }

    pub fn find_node(&mut self, name: &str, parent_block: u64) -> Result<(u64, Node)> {
        if parent_block == 0 {
            return Err(Error::NotFound);
        }

        let parent = self.node(parent_block)?;
        for extent in parent.1.extents.iter() {
            for (block, size) in extent.blocks() {
                if size >= BLOCK_SIZE {
                    let child = self.node(block)?;

                    let mut matches = false;
                    if let Ok(child_name) = child.1.name() {
                        if child_name == name {
                            matches = true;
                        }
                    }

                    if matches {
                        return Ok(child);
                    }
                }
            }
        }

        self.find_node(name, parent.1.next)
    }

    fn node_extents(&mut self, block: u64, mut offset: u64, mut len: usize, extents: &mut Vec<Extent>) -> Result<()> {
        if block == 0 {
            return Ok(());
        }

        let node = self.node(block)?;
        for extent in node.1.extents.iter() {
            let mut push_extent = Extent::default();
            for (block, size) in extent.blocks() {
                if offset == 0 {
                    if push_extent.block == 0 {
                        push_extent.block = block;
                    }
                    if len as u64 >= size {
                        push_extent.length += size;
                        len -= size as usize;
                    } else if len > 0 {
                        push_extent.length += len as u64;
                        len = 0;
                        break;
                    } else {
                        break;
                    }
                } else {
                    offset -= 1;
                }
            }
            if push_extent.length > 0 {
                extents.push(push_extent);
            }
            if len == 0 {
                break;
            }
        }

        if len > 0 {
            self.node_extents(node.1.next, offset, len, extents)
        } else {
            Ok(())
        }
    }

    pub fn read_node(&mut self, block: u64, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let block_offset = offset / BLOCK_SIZE;
        let mut byte_offset = (offset % BLOCK_SIZE) as usize;

        let mut extents = Vec::new();
        self.node_extents(block, block_offset, byte_offset + buf.len(), &mut extents)?;

        let mut i = 0;
        for extent in extents.iter() {
            let mut block = extent.block;
            let mut length = extent.length;

            if byte_offset > 0 && length > 0 {
                let mut sector = [0; BLOCK_SIZE as usize];
                self.read_at(block, &mut sector)?;

                let sector_size = min(sector.len() as u64, length) as usize;
                for (s_b, b) in sector[byte_offset..sector_size].iter().zip(buf[i..].iter_mut()) {
                    *b = *s_b;
                    i += 1;
                }

                block += 1;
                length -= sector_size as u64;

                byte_offset = 0;
            }

            let length_aligned = ((min(length, (buf.len() - i) as u64)/BLOCK_SIZE) * BLOCK_SIZE) as usize;

            if length_aligned > 0 {
                let extent_buf = &mut buf[i..i + length_aligned];
                self.read_at(block, extent_buf)?;
                i += length_aligned;
                block += (length_aligned as u64)/BLOCK_SIZE;
                length -= length_aligned as u64;
            }

            if length > 0 {
                let mut sector = [0; BLOCK_SIZE as usize];
                self.read_at(block, &mut sector)?;

                let sector_size = min(sector.len() as u64, length) as usize;
                for (s_b, b) in sector[..sector_size].iter().zip(buf[i..].iter_mut()) {
                    *b = *s_b;
                    i += 1;
                }

                block += 1;
                length -= sector_size as u64;
            }

            assert_eq!(length, 0);
            assert_eq!(block, extent.block + (extent.length + BLOCK_SIZE - 1)/BLOCK_SIZE);
        }

        Ok(i)
    }

    pub fn node_len(&mut self, block: u64) -> Result<u64> {
        if block == 0 {
            return Err(Error::NotFound);
        }

        let mut size = 0;

        let node = self.node(block)?;
        for extent in node.1.extents.iter() {
            size += extent.length;
        }

        if node.1.next > 0 {
            size += self.node_len(node.1.next)?;
            Ok(size)
        } else {
            Ok(size)
        }
    }
}
