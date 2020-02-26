//! Test reception on the virtualized UART by creating two readers that
//! read in parallel. To add this test, include the line
//! ```
//!    virtual_uart_rx_test::run_virtual_uart_receive(uart_mux);
//! ```
//! to the imix boot sequence, where `uart_mux` is a
//! `capsules::virtual_uart::MuxUart`.  There is a 3-byte and a 7-byte
//! read running in parallel. Test that they are both working by typing
//! and seeing that they both get all characters. If you repeatedly
//! type 'a', for example (0x61), you should see something like:
//! ```
//! Starting receive of length 3
//! Virtual uart read complete: CommandComplete:
//! 61
//! 61
//! 61
//! 61
//! 61
//! 61
//! 61
//! Starting receive of length 7
//! Virtual uart read complete: CommandComplete:
//! 61
//! 61
//! 61
//! Starting receive of length 3
//! Virtual uart read complete: CommandComplete:
//! 61
//! 61
//! 61
//! Starting receive of length 3
//! Virtual uart read complete: CommandComplete:
//! 61
//! 61
//! 61
//! 61
//! 61
//! 61
//! 61
//! Starting receive of length 7
//! Virtual uart read complete: CommandComplete:
//! 61
//! 61
//! 61
//! ```

use capsules::test::virtual_uart::TestVirtualUartReceive;
use capsules::virtual_uart::{MuxUart, UartDevice};
use kernel::debug;
use kernel::hil::uart::Receive;
use kernel::static_init;

pub unsafe fn run_virtual_uart_receive(mux: &'static MuxUart<'static>) {
    debug!("Starting virtual reads.");
    let small = static_init_test_receive_small(mux);
    let large = static_init_test_receive_large(mux);
    small.run();
    large.run();
}

unsafe fn static_init_test_receive_small(
    mux: &'static MuxUart<'static>,
) -> &'static TestVirtualUartReceive {
    static mut SMALL: [u8; 3] = [0; 3];
    let device = static_init!(UartDevice<'static>, UartDevice::new(mux, true));
    device.setup();
    let test = static_init!(
        TestVirtualUartReceive,
        TestVirtualUartReceive::new(device, &mut SMALL)
    );
    device.set_receive_client(test);
    test
}

unsafe fn static_init_test_receive_large(
    mux: &'static MuxUart<'static>,
) -> &'static TestVirtualUartReceive {
    static mut BUFFER: [u8; 7] = [0; 7];
    let device = static_init!(UartDevice<'static>, UartDevice::new(mux, true));
    device.setup();
    let test = static_init!(
        TestVirtualUartReceive,
        TestVirtualUartReceive::new(device, &mut BUFFER)
    );
    device.set_receive_client(test);
    test
}
