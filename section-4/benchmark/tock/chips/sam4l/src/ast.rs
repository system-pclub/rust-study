//! Implementation of a single hardware timer.
//!
//! - Author: Amit Levy <levya@cs.stanford.edu>
//! - Author: Philip Levis <pal@cs.stanford.edu>
//! - Date: July 16, 2015

use crate::pm::{self, PBDClock};
use kernel::common::cells::OptionalCell;
use kernel::common::registers::{register_bitfields, ReadOnly, ReadWrite, WriteOnly};
use kernel::common::StaticRef;
use kernel::hil::time::{self, Alarm, Freq16KHz, Time};
use kernel::hil::Controller;

/// Minimum number of clock tics to make sure ALARM0 register is synchronized
///
/// The datasheet has the following ominous language (Section 19.5.3.2):
///
/// > Because of synchronization, the transfer of the alarm value will not
/// > happen immediately. When changing/setting the alarm value, the user must
/// > make sure that the counter will not count the selected alarm value before
/// > the value is transferred to the register. In that case, the first alarm
/// > interrupt after the change will not be triggered.
///
/// In practice, we've observed that when the alarm is set for a counter value
/// less than or equal to four tics ahead of the current counter value, the
/// alarm interrupt doesn't fire. Thus, we simply round up to at least eight
/// tics. Seems safe enough and in practice has seemed to work.
const ALARM0_SYNC_TICS: u32 = 8;

#[repr(C)]
struct AstRegisters {
    cr: ReadWrite<u32, Control::Register>,
    cv: ReadWrite<u32, Value::Register>,
    sr: ReadOnly<u32, Status::Register>,
    scr: WriteOnly<u32, Interrupt::Register>,
    ier: WriteOnly<u32, Interrupt::Register>,
    idr: WriteOnly<u32, Interrupt::Register>,
    imr: ReadOnly<u32, Interrupt::Register>,
    wer: ReadWrite<u32, Event::Register>,
    // 0x20
    ar0: ReadWrite<u32, Value::Register>,
    ar1: ReadWrite<u32, Value::Register>,
    _reserved0: [u32; 2],
    pir0: ReadWrite<u32, PeriodicInterval::Register>,
    pir1: ReadWrite<u32, PeriodicInterval::Register>,
    _reserved1: [u32; 2],
    // 0x40
    clock: ReadWrite<u32, ClockControl::Register>,
    dtr: ReadWrite<u32, DigitalTuner::Register>,
    eve: WriteOnly<u32, Event::Register>,
    evd: WriteOnly<u32, Event::Register>,
    evm: ReadOnly<u32, Event::Register>,
    calv: ReadWrite<u32, Calendar::Register>, // we leave out parameter and version
}

register_bitfields![u32,
    Control [
        /// Prescalar Select
        PSEL OFFSET(16) NUMBITS(5) [],
        /// Clear on Alarm 1
        CA1  OFFSET(9) NUMBITS(1) [
            NoClearCounter = 0,
            ClearCounter = 1
        ],
        /// Clear on Alarm 0
        CA0  OFFSET(8) NUMBITS(1) [
            NoClearCounter = 0,
            ClearCounter = 1
        ],
        /// Calendar Mode
        CAL  OFFSET(2) NUMBITS(1) [
            CounterMode = 0,
            CalendarMode = 1
        ],
        /// Prescalar Clear
        PCLR OFFSET(1) NUMBITS(1) [],
        /// Enable
        EN   OFFSET(0) NUMBITS(1) [
            Disable = 0,
            Enable = 1
        ]
    ],

    Value [
        VALUE OFFSET(0) NUMBITS(32) []
    ],

    Status [
        /// Clock Ready
        CLKRDY 29,
        /// Clock Busy
        CLKBUSY 28,
        /// AST Ready
        READY 25,
        /// AST Busy
        BUSY 24,
        /// Periodic 0
        PER0 16,
        /// Alarm 0
        ALARM0 8,
        /// Overflow
        OVF 0
    ],

    Interrupt [
        /// Clock Ready
        CLKRDY 29,
        /// AST Ready
        READY 25,
        /// Periodic 0
        PER0 16,
        /// Alarm 0
        ALARM0 8,
        /// Overflow
        OVF 0
    ],

    Event [
        /// Periodic 0
        PER0 16,
        /// Alarm 0
        ALARM0 8,
        /// Overflow
        OVF 0
    ],

    PeriodicInterval [
        /// Interval Select
        INSEL OFFSET(0) NUMBITS(5) []
    ],

    ClockControl [
        /// Clock Source Selection
        CSSEL OFFSET(8) NUMBITS(3) [
            RCSYS = 0,
            OSC32 = 1,
            APBClock = 2,
            GCLK = 3,
            Clk1k = 4
        ],
        /// Clock Enable
        CEN   OFFSET(0) NUMBITS(1) [
            Disable = 0,
            Enable = 1
        ]
    ],

    DigitalTuner [
        VALUE OFFSET(8) NUMBITS(8) [],
        ADD   OFFSET(5) NUMBITS(1) [],
        EXP   OFFSET(0) NUMBITS(5) []
    ],

    Calendar [
        YEAR  OFFSET(26) NUMBITS(6) [],
        MONTH OFFSET(22) NUMBITS(4) [],
        DAY   OFFSET(17) NUMBITS(5) [],
        HOUR  OFFSET(12) NUMBITS(5) [],
        MIN   OFFSET( 6) NUMBITS(6) [],
        SEC   OFFSET( 0) NUMBITS(6) []
    ]
];

const AST_ADDRESS: StaticRef<AstRegisters> =
    unsafe { StaticRef::new(0x400F0800 as *const AstRegisters) };

pub struct Ast<'a> {
    registers: StaticRef<AstRegisters>,
    callback: OptionalCell<&'a time::Client>,
}

pub static mut AST: Ast<'static> = Ast {
    registers: AST_ADDRESS,
    callback: OptionalCell::empty(),
};

impl Controller for Ast<'a> {
    type Config = &'static time::Client;

    fn configure(&self, client: &'a time::Client) {
        self.callback.set(client);

        pm::enable_clock(pm::Clock::PBD(PBDClock::AST));
        self.select_clock(Clock::ClockOsc32);
        self.disable();
        self.disable_alarm_irq();
        self.set_prescalar(0); // 32KHz / (2^(0 + 1)) = 16KHz
        self.enable_alarm_wake();
        self.clear_alarm();
    }
}

#[repr(usize)]
#[allow(dead_code)]
enum Clock {
    ClockRCSys = 0,
    ClockOsc32 = 1,
    ClockAPB = 2,
    ClockGclk2 = 3,
    Clock1K = 4,
}

impl Ast<'a> {
    fn clock_busy(&self) -> bool {
        let regs: &AstRegisters = &*self.registers;
        regs.sr.is_set(Status::CLKBUSY)
    }

    pub fn set_client(&self, client: &'a time::Client) {
        self.callback.set(client);
    }

    fn busy(&self) -> bool {
        let regs: &AstRegisters = &*self.registers;
        regs.sr.is_set(Status::BUSY)
    }

    /// Clears the alarm bit in the status register (indicating the alarm value
    /// has been reached).
    fn clear_alarm(&self) {
        let regs: &AstRegisters = &*self.registers;
        while self.busy() {}
        regs.scr.write(Interrupt::ALARM0::SET);
        while self.busy() {}
    }

    // Configure the clock to use to drive the AST
    fn select_clock(&self, clock: Clock) {
        let regs: &AstRegisters = &*self.registers;
        // Disable clock by setting first bit to zero
        while self.clock_busy() {}
        regs.clock.modify(ClockControl::CEN::CLEAR);
        while self.clock_busy() {}

        // Select clock
        regs.clock.write(ClockControl::CSSEL.val(clock as u32));
        while self.clock_busy() {}

        // Re-enable clock
        regs.clock.modify(ClockControl::CEN::SET);
        while self.clock_busy() {}
    }

    /// Enables the AST registers
    fn enable(&self) {
        let regs: &AstRegisters = &*self.registers;
        while self.busy() {}
        regs.cr.modify(Control::EN::SET);
        while self.busy() {}
    }

    /// Disable the AST registers
    fn disable(&self) {
        let regs: &AstRegisters = &*self.registers;
        while self.busy() {}
        regs.cr.modify(Control::EN::CLEAR);
        while self.busy() {}
    }

    /// Returns if an alarm is currently set
    fn is_alarm_enabled(&self) -> bool {
        let regs: &AstRegisters = &*self.registers;
        while self.busy() {}
        regs.sr.is_set(Status::ALARM0)
    }

    fn set_prescalar(&self, val: u8) {
        let regs: &AstRegisters = &*self.registers;
        while self.busy() {}
        regs.cr.modify(Control::PSEL.val(val as u32));
        while self.busy() {}
    }

    fn enable_alarm_irq(&self) {
        let regs: &AstRegisters = &*self.registers;
        regs.ier.write(Interrupt::ALARM0::SET);
    }

    fn disable_alarm_irq(&self) {
        let regs: &AstRegisters = &*self.registers;
        regs.idr.write(Interrupt::ALARM0::SET);
    }

    fn enable_alarm_wake(&self) {
        let regs: &AstRegisters = &*self.registers;
        while self.busy() {}
        regs.wer.modify(Event::ALARM0::SET);
        while self.busy() {}
    }

    fn get_counter(&self) -> u32 {
        let regs: &AstRegisters = &*self.registers;
        while self.busy() {}
        regs.cv.read(Value::VALUE)
    }

    pub fn handle_interrupt(&mut self) {
        self.clear_alarm();
        self.callback.map(|cb| {
            cb.fired();
        });
    }
}

impl Time for Ast<'a> {
    type Frequency = Freq16KHz;

    fn disable(&self) {
        self.disable_alarm_irq();
        self.clear_alarm();
    }

    fn is_armed(&self) -> bool {
        self.is_alarm_enabled()
    }
}

impl Alarm for Ast<'a> {
    fn now(&self) -> u32 {
        self.get_counter()
    }

    fn set_alarm(&self, mut tics: u32) {
        let regs: &AstRegisters = &*self.registers;
        let now = self.get_counter();
        if tics.wrapping_sub(now) <= ALARM0_SYNC_TICS {
            tics = now.wrapping_add(ALARM0_SYNC_TICS);
        }

        while self.busy() {}
        regs.ar0.write(Value::VALUE.val(tics));
        while self.busy() {}
        self.enable_alarm_irq();
        self.enable();
    }

    fn get_alarm(&self) -> u32 {
        let regs: &AstRegisters = &*self.registers;
        while self.busy() {}
        regs.ar0.read(Value::VALUE)
    }
}
