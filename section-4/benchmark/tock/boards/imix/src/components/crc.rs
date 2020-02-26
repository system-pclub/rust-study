//! Component for CRC syscall interface on imix board.
//!
//! This provides one Component, CrcComponent, which implements a
//! userspace syscall interface to the CRC peripheral (CRCCU) on the
//! SAM4L.
//!
//! Usage
//! -----
//! ```rust
//! let crc = CrcComponent::new().finalize();
//! ```

// Author: Philip Levis <pal@cs.stanford.edu>
// Last modified: 6/20/2018

#![allow(dead_code)] // Components are intended to be conditionally included

use capsules::crc;
use kernel::capabilities;
use kernel::component::Component;
use kernel::create_capability;
use kernel::static_init;

pub struct CrcComponent {
    board_kernel: &'static kernel::Kernel,
}

impl CrcComponent {
    pub fn new(board_kernel: &'static kernel::Kernel) -> CrcComponent {
        CrcComponent {
            board_kernel: board_kernel,
        }
    }
}

impl Component for CrcComponent {
    type Output = &'static crc::Crc<'static, sam4l::crccu::Crccu<'static>>;

    unsafe fn finalize(&mut self) -> Self::Output {
        let grant_cap = create_capability!(capabilities::MemoryAllocationCapability);

        let crc = static_init!(
            crc::Crc<'static, sam4l::crccu::Crccu<'static>>,
            crc::Crc::new(
                &mut sam4l::crccu::CRCCU,
                self.board_kernel.create_grant(&grant_cap)
            )
        );

        crc
    }
}
