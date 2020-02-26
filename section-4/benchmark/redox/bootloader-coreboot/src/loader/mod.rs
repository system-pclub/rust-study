use coreboot_table::{Mapper, PhysicalAddress, VirtualAddress, Table};

use self::memory_map::memory_map;
use self::paging::paging;
use self::vesa::vesa;

mod memory_map;
mod paging;
mod vesa;

struct IdentityMapper;

impl Mapper for IdentityMapper {
    unsafe fn map_aligned(&mut self, address: PhysicalAddress, _size: usize) -> Result<VirtualAddress, &'static str> {
        Ok(VirtualAddress(address.0))
    }

    unsafe fn unmap_aligned(&mut self, _address: VirtualAddress) -> Result<(), &'static str> {
        Ok(())
    }

    fn page_size(&self) -> usize {
        4096
    }
}

pub unsafe fn main() {
    extern "C" {
        fn startup() -> !;
    }

    let mut framebuffer_opt = None;
    coreboot_table::tables(|table| {
        match table {
            Table::Framebuffer(framebuffer) => {
                println!("{:?}", framebuffer);
                framebuffer_opt = Some(framebuffer.clone());
            },
            Table::Memory(memory) => {
                println!("{:?}", memory.ranges());
                memory_map(memory);
            },
            Table::Other(other) => println!("{:?}", other),
        }
        Ok(())
    }, &mut IdentityMapper).unwrap();

    if let Some(framebuffer) = framebuffer_opt {
        if framebuffer.bits_per_pixel == 32 {
            println!("Framebuffer of resolution {}x{}", framebuffer.x_resolution, framebuffer.y_resolution);
            vesa(Some(&framebuffer));
        } else {
            println!("Unsupported framebuffer bits per pixel {}", framebuffer.bits_per_pixel);
            vesa(None);
        }
    } else {
        println!("No framebuffer found");
        vesa(None);
    }

    println!("Paging");
    paging();

    println!("Startup");
    startup();
}
