//! `icmp_lowpan_test.rs`: Test kernel space sending of
//! ICMP packets over 6LoWPAN
//!
//! Currently this file only tests sending messages.
//!
//! To use this test suite, allocate space for a new LowpanICMPTest structure, and
//! call the `initialize_all` function, which performs
//! the initialization routines for the 6LoWPAN, TxState, RxState, and Sixlowpan
//! structs. Insert the code into `boards/imix/src/main.rs` as follows:
//!
//! ...
//! // Radio initialization code
//! ...
//!    let icmp_lowpan_test = icmp_lowpan_test::initialize_all(
//!        mux_mac,
//!        mux_alarm as &'static MuxAlarm<'static, sam4l::ast::Ast>,
//!    );
//! ...
//! // Imix initialization
//! ...
//! icmp_lowpan_test.start();

use capsules::ieee802154::device::MacDevice;
use capsules::net::icmpv6::icmpv6::{ICMP6Header, ICMP6Type};
use capsules::net::icmpv6::icmpv6_send::{ICMP6SendStruct, ICMP6Sender};
use capsules::net::ieee802154::MacAddress;
use capsules::net::ipv6::ip_utils::IPAddr;
use capsules::net::ipv6::ipv6::{IP6Packet, IPPayload, TransportHeader};
use capsules::net::ipv6::ipv6_send::{IP6SendStruct, IP6Sender};
use capsules::net::sixlowpan::sixlowpan_compression;
use capsules::net::sixlowpan::sixlowpan_state::{Sixlowpan, SixlowpanState, TxState};
use capsules::virtual_alarm::{MuxAlarm, VirtualMuxAlarm};
use core::cell::Cell;
use kernel::debug;
use kernel::hil::radio;
use kernel::hil::time;
use kernel::hil::time::Frequency;
use kernel::static_init;
use kernel::ReturnCode;

pub const SRC_ADDR: IPAddr = IPAddr([
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
]);
pub const DST_ADDR: IPAddr = IPAddr([
    0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f,
]);

/* 6LoWPAN Constants */
const DEFAULT_CTX_PREFIX_LEN: u8 = 8;
static DEFAULT_CTX_PREFIX: [u8; 16] = [0x0 as u8; 16];
static mut RX_STATE_BUF: [u8; 1280] = [0x0; 1280];
const DST_MAC_ADDR: MacAddress = MacAddress::Short(0x802);
const SRC_MAC_ADDR: MacAddress = MacAddress::Short(0xf00f);

pub const TEST_DELAY_MS: u32 = 10000;
pub const TEST_LOOP: bool = false;

static mut ICMP_PAYLOAD: [u8; 10] = [0; 10];

pub static mut RF233_BUF: [u8; radio::MAX_BUF_SIZE] = [0 as u8; radio::MAX_BUF_SIZE];

//Use a global variable option, initialize as None, then actually initialize in initialize all

pub struct LowpanICMPTest<'a, A: time::Alarm> {
    alarm: A,
    test_counter: Cell<usize>,
    icmp_sender: &'a ICMP6Sender<'a>,
}

pub unsafe fn initialize_all(
    mux_mac: &'static capsules::ieee802154::virtual_mac::MuxMac<'static>,
    mux_alarm: &'static MuxAlarm<'static, sam4l::ast::Ast>,
) -> &'static LowpanICMPTest<
    'static,
    capsules::virtual_alarm::VirtualMuxAlarm<'static, sam4l::ast::Ast<'static>>,
> {
    let radio_mac = static_init!(
        capsules::ieee802154::virtual_mac::MacUser<'static>,
        capsules::ieee802154::virtual_mac::MacUser::new(mux_mac)
    );
    mux_mac.add_user(radio_mac);
    let sixlowpan = static_init!(
        Sixlowpan<'static, sam4l::ast::Ast<'static>, sixlowpan_compression::Context>,
        Sixlowpan::new(
            sixlowpan_compression::Context {
                prefix: DEFAULT_CTX_PREFIX,
                prefix_len: DEFAULT_CTX_PREFIX_LEN,
                id: 0,
                compress: false,
            },
            &sam4l::ast::AST
        )
    );

    let sixlowpan_state = sixlowpan as &SixlowpanState;
    let sixlowpan_tx = TxState::new(sixlowpan_state);

    let icmp_hdr = ICMP6Header::new(ICMP6Type::Type128); // Echo Request

    let ip_pyld: IPPayload = IPPayload {
        header: TransportHeader::ICMP(icmp_hdr),
        payload: &mut ICMP_PAYLOAD,
    };

    let ip6_dg = static_init!(IP6Packet<'static>, IP6Packet::new(ip_pyld));

    let ipsender_virtual_alarm = static_init!(
        VirtualMuxAlarm<'static, sam4l::ast::Ast>,
        VirtualMuxAlarm::new(mux_alarm)
    );

    let ip6_sender = static_init!(
        IP6SendStruct<'static, VirtualMuxAlarm<'static, sam4l::ast::Ast<'static>>>,
        IP6SendStruct::new(
            ip6_dg,
            ipsender_virtual_alarm,
            &mut RF233_BUF,
            sixlowpan_tx,
            radio_mac,
            DST_MAC_ADDR,
            SRC_MAC_ADDR
        )
    );
    radio_mac.set_transmit_client(ip6_sender);

    let icmp_send_struct = static_init!(
        ICMP6SendStruct<
            'static,
            IP6SendStruct<'static, VirtualMuxAlarm<'static, sam4l::ast::Ast<'static>>>,
        >,
        ICMP6SendStruct::new(ip6_sender)
    );

    let icmp_lowpan_test = static_init!(
        LowpanICMPTest<'static, VirtualMuxAlarm<'static, sam4l::ast::Ast>>,
        LowpanICMPTest::new(
            //sixlowpan_tx,
            //radio_mac,
            VirtualMuxAlarm::new(mux_alarm),
            icmp_send_struct
        )
    );

    ip6_sender.set_client(icmp_send_struct);
    icmp_send_struct.set_client(icmp_lowpan_test);
    icmp_lowpan_test.alarm.set_client(icmp_lowpan_test);
    ipsender_virtual_alarm.set_client(ip6_sender);

    icmp_lowpan_test
}

impl<'a, A: time::Alarm> capsules::net::icmpv6::icmpv6_send::ICMP6SendClient
    for LowpanICMPTest<'a, A>
{
    fn send_done(&self, result: ReturnCode) {
        match result {
            ReturnCode::SUCCESS => {
                debug!("ICMP Echo Request Packet Sent!");
                match self.test_counter.get() {
                    2 => debug!("Test completed successfully."),
                    _ => self.schedule_next(),
                }
            }
            _ => debug!("Failed to send ICMP Packet!"),
        }
    }
}

impl<A: time::Alarm> LowpanICMPTest<'a, A> {
    pub fn new(alarm: A, icmp_sender: &'a ICMP6Sender<'a>) -> LowpanICMPTest<'a, A> {
        LowpanICMPTest {
            alarm: alarm,
            test_counter: Cell::new(0),
            icmp_sender: icmp_sender,
        }
    }

    pub fn start(&self) {
        self.schedule_next();
    }

    fn schedule_next(&self) {
        let delta = (A::Frequency::frequency() * TEST_DELAY_MS) / 1000;
        let next = self.alarm.now().wrapping_add(delta);
        self.alarm.set_alarm(next);
    }

    fn run_test_and_increment(&self) {
        let test_counter = self.test_counter.get();
        self.run_test(test_counter);
        match TEST_LOOP {
            true => self.test_counter.set((test_counter + 1) % self.num_tests()),
            false => self.test_counter.set(test_counter + 1),
        };
    }

    fn num_tests(&self) -> usize {
        2
    }

    fn run_test(&self, test_id: usize) {
        debug!("Running test {}:", test_id);
        match test_id {
            0 => self.ipv6_send_packet_test(),
            1 => self.ipv6_send_packet_test(),
            _ => {}
        }
    }

    fn ipv6_send_packet_test(&self) {
        unsafe {
            self.send_ipv6_packet();
        }
    }

    unsafe fn send_ipv6_packet(&self) {
        self.send_next();
    }

    fn send_next(&self) {
        let icmp_hdr = ICMP6Header::new(ICMP6Type::Type128); // Echo Request
        unsafe { self.icmp_sender.send(DST_ADDR, icmp_hdr, &ICMP_PAYLOAD) };
    }
}

impl<'a, A: time::Alarm> time::Client for LowpanICMPTest<'a, A> {
    fn fired(&self) {
        self.run_test_and_increment();
    }
}
