//! Component for USB on the imix board.
//!
//! This provides one Component, UsbComponent, which implements
//! a userspace syscall interface to the USB peripheral on the SAM4L.
//!
//! Usage
//! -----
//! ```rust
//! let usb = UsbComponent::new().finalize();
//! ```

// Author: Philip Levis <pal@cs.stanford.edu>
// Last modified: 6/20/2018

#![allow(dead_code)] // Components are intended to be conditionally included

use kernel::capabilities;
use kernel::component::Component;
use kernel::create_capability;
use kernel::static_init;

pub struct UsbComponent {
    board_kernel: &'static kernel::Kernel,
}

type UsbDevice = capsules::usb_user::UsbSyscallDriver<
    'static,
    capsules::usbc_client::Client<'static, sam4l::usbc::Usbc<'static>>,
>;

impl UsbComponent {
    pub fn new(board_kernel: &'static kernel::Kernel) -> UsbComponent {
        UsbComponent {
            board_kernel: board_kernel,
        }
    }
}

impl Component for UsbComponent {
    type Output = &'static UsbDevice;

    unsafe fn finalize(&mut self) -> Self::Output {
        let grant_cap = create_capability!(capabilities::MemoryAllocationCapability);

        // Configure the USB controller
        let usb_client = static_init!(
            capsules::usbc_client::Client<'static, sam4l::usbc::Usbc<'static>>,
            capsules::usbc_client::Client::new(&sam4l::usbc::USBC)
        );
        sam4l::usbc::USBC.set_client(usb_client);

        // Configure the USB userspace driver
        let usb_driver = static_init!(
            capsules::usb_user::UsbSyscallDriver<
                'static,
                capsules::usbc_client::Client<'static, sam4l::usbc::Usbc<'static>>,
            >,
            capsules::usb_user::UsbSyscallDriver::new(
                usb_client,
                self.board_kernel.create_grant(&grant_cap)
            )
        );

        usb_driver
    }
}
