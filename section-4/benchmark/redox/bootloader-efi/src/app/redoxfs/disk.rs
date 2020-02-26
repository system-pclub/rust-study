use std::proto::Protocol;
use uefi::guid::{Guid, BLOCK_IO_GUID};
use uefi::block_io::BlockIo as UefiBlockIo;
use uefi::status::Result;

use super::BLOCK_SIZE;

pub struct Disk(pub &'static mut UefiBlockIo);

impl Protocol<UefiBlockIo> for Disk {
    fn guid() -> Guid {
        BLOCK_IO_GUID
    }

    fn new(inner: &'static mut UefiBlockIo) -> Self {
        Self(inner)
    }
}

impl Disk {
    pub fn read_at(&self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        let block_size = self.0.Media.BlockSize as u64;

        let lba = block * BLOCK_SIZE / block_size;

        (self.0.ReadBlocks)(self.0, self.0.Media.MediaId, lba, buffer.len(), buffer.as_mut_ptr())?;
        Ok(buffer.len())
    }
}
