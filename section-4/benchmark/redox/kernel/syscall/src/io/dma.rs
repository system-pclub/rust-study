use core::{mem, ptr};
use core::ops::{Deref, DerefMut};

use crate::Result;

struct PhysBox {
    address: usize,
    size: usize
}

impl PhysBox {
    fn new(size: usize) -> Result<PhysBox> {
        let address = unsafe { crate::physalloc(size)? };
        Ok(PhysBox {
            address: address,
            size: size
        })
    }
}

impl Drop for PhysBox {
    fn drop(&mut self) {
        let _ = unsafe { crate::physfree(self.address, self.size) };
    }
}

pub struct Dma<T> {
    phys: PhysBox,
    virt: *mut T
}

impl<T> Dma<T> {
    pub fn new(value: T) -> Result<Dma<T>> {
        let phys = PhysBox::new(mem::size_of::<T>())?;
        let virt = unsafe { crate::physmap(phys.address, phys.size, crate::PHYSMAP_WRITE)? } as *mut T;
        unsafe { ptr::write(virt, value); }
        Ok(Dma {
            phys: phys,
            virt: virt
        })
    }

    pub fn zeroed() -> Result<Dma<T>> {
        let phys = PhysBox::new(mem::size_of::<T>())?;
        let virt = unsafe { crate::physmap(phys.address, phys.size, crate::PHYSMAP_WRITE)? } as *mut T;
        unsafe { ptr::write_bytes(virt as *mut u8, 0, phys.size); }
        Ok(Dma {
            phys: phys,
            virt: virt
        })
    }

    pub fn physical(&self) -> usize {
        self.phys.address
    }
}

impl<T> Deref for Dma<T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.virt }
    }
}

impl<T> DerefMut for Dma<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.virt }
    }
}

impl<T> Drop for Dma<T> {
    fn drop(&mut self) {
        unsafe { drop(ptr::read(self.virt)); }
        let _ = unsafe { crate::physunmap(self.virt as usize) };
    }
}
