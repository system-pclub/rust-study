//! Tock kernel for the Aconno ACD52832 board based on the Nordic nRF52832 MCU.

#![no_std]
#![no_main]
#![deny(missing_docs)]

use capsules::virtual_alarm::VirtualMuxAlarm;
use capsules::virtual_uart::{MuxUart, UartDevice};
use kernel::capabilities;
use kernel::hil;
use kernel::hil::entropy::Entropy32;
use kernel::hil::gpio::Pin;
use kernel::hil::rng::Rng;
#[allow(unused_imports)]
use kernel::{create_capability, debug, debug_gpio, static_init};
use nrf5x::rtc::Rtc;

const LED1_PIN: usize = 26;
const LED2_PIN: usize = 22;
const LED3_PIN: usize = 23;
const LED4_PIN: usize = 24;

const BUTTON1_PIN: usize = 25;
const BUTTON2_PIN: usize = 14;
const BUTTON3_PIN: usize = 15;
const BUTTON4_PIN: usize = 16;
const BUTTON_RST_PIN: usize = 19;

/// UART Writer
#[macro_use]
pub mod io;

// State for loading and holding applications.
// How should the kernel respond when a process faults.
const FAULT_RESPONSE: kernel::procs::FaultResponse = kernel::procs::FaultResponse::Panic;

// Number of concurrent processes this platform supports.
const NUM_PROCS: usize = 4;

#[link_section = ".app_memory"]
static mut APP_MEMORY: [u8; 32768] = [0; 32768];

static mut PROCESSES: [Option<&'static kernel::procs::ProcessType>; NUM_PROCS] = [None; NUM_PROCS];

/// Dummy buffer that causes the linker to reserve enough space for the stack.
#[no_mangle]
#[link_section = ".stack_buffer"]
pub static mut STACK_MEMORY: [u8; 0x1000] = [0; 0x1000];

/// Supported drivers by the platform
pub struct Platform {
    ble_radio: &'static capsules::ble_advertising_driver::BLE<
        'static,
        nrf52::radio::Radio,
        VirtualMuxAlarm<'static, Rtc>,
    >,
    button: &'static capsules::button::Button<'static, nrf5x::gpio::GPIOPin>,
    console: &'static capsules::console::Console<'static>,
    gpio: &'static capsules::gpio::GPIO<'static, nrf5x::gpio::GPIOPin>,
    led: &'static capsules::led::LED<'static, nrf5x::gpio::GPIOPin>,
    rng: &'static capsules::rng::RngDriver<'static>,
    temp: &'static capsules::temperature::TemperatureSensor<'static>,
    ipc: kernel::ipc::IPC,
    alarm:
        &'static capsules::alarm::AlarmDriver<'static, VirtualMuxAlarm<'static, nrf5x::rtc::Rtc>>,
    gpio_async:
        &'static capsules::gpio_async::GPIOAsync<'static, capsules::mcp230xx::MCP230xx<'static>>,
    light: &'static capsules::ambient_light::AmbientLight<'static>,
    buzzer: &'static capsules::buzzer_driver::Buzzer<
        'static,
        capsules::virtual_alarm::VirtualMuxAlarm<'static, nrf5x::rtc::Rtc>,
    >,
}

impl kernel::Platform for Platform {
    fn with_driver<F, R>(&self, driver_num: usize, f: F) -> R
    where
        F: FnOnce(Option<&kernel::Driver>) -> R,
    {
        match driver_num {
            capsules::console::DRIVER_NUM => f(Some(self.console)),
            capsules::gpio::DRIVER_NUM => f(Some(self.gpio)),
            capsules::alarm::DRIVER_NUM => f(Some(self.alarm)),
            capsules::led::DRIVER_NUM => f(Some(self.led)),
            capsules::button::DRIVER_NUM => f(Some(self.button)),
            capsules::rng::DRIVER_NUM => f(Some(self.rng)),
            capsules::ble_advertising_driver::DRIVER_NUM => f(Some(self.ble_radio)),
            capsules::temperature::DRIVER_NUM => f(Some(self.temp)),
            capsules::gpio_async::DRIVER_NUM => f(Some(self.gpio_async)),
            capsules::ambient_light::DRIVER_NUM => f(Some(self.light)),
            capsules::buzzer_driver::DRIVER_NUM => f(Some(self.buzzer)),
            kernel::ipc::DRIVER_NUM => f(Some(&self.ipc)),
            _ => f(None),
        }
    }
}

/// Entry point in the vector table called on hard reset.
#[no_mangle]
pub unsafe fn reset_handler() {
    // Loads relocations and clears BSS
    nrf52::init();

    // Create capabilities that the board needs to call certain protected kernel
    // functions.
    let process_management_capability =
        create_capability!(capabilities::ProcessManagementCapability);
    let main_loop_capability = create_capability!(capabilities::MainLoopCapability);
    let memory_allocation_capability = create_capability!(capabilities::MemoryAllocationCapability);

    let board_kernel = static_init!(kernel::Kernel, kernel::Kernel::new(&PROCESSES));

    // GPIOs
    let gpio_pins = static_init!(
        [&'static nrf5x::gpio::GPIOPin; 14],
        [
            &nrf5x::gpio::PORT[3], // Bottom right header on DK board
            &nrf5x::gpio::PORT[4],
            &nrf5x::gpio::PORT[28],
            &nrf5x::gpio::PORT[29],
            &nrf5x::gpio::PORT[30],
            // &nrf5x::gpio::PORT[31], // -----
            &nrf5x::gpio::PORT[12], // Top mid header on DK board
            &nrf5x::gpio::PORT[11], // -----
            &nrf5x::gpio::PORT[27], // Top left header on DK board
            &nrf5x::gpio::PORT[26],
            &nrf5x::gpio::PORT[2],
            &nrf5x::gpio::PORT[25],
            &nrf5x::gpio::PORT[24],
            &nrf5x::gpio::PORT[23],
            &nrf5x::gpio::PORT[22], // -----
        ]
    );

    // LEDs
    let led_pins = static_init!(
        [(&'static nrf5x::gpio::GPIOPin, capsules::led::ActivationMode); 4],
        [
            (
                &nrf5x::gpio::PORT[LED1_PIN],
                capsules::led::ActivationMode::ActiveLow
            ),
            (
                &nrf5x::gpio::PORT[LED2_PIN],
                capsules::led::ActivationMode::ActiveLow
            ),
            (
                &nrf5x::gpio::PORT[LED3_PIN],
                capsules::led::ActivationMode::ActiveLow
            ),
            (
                &nrf5x::gpio::PORT[LED4_PIN],
                capsules::led::ActivationMode::ActiveLow
            ),
        ]
    );

    // Setup GPIO pins that correspond to buttons
    let button_pins = static_init!(
        [(&'static nrf5x::gpio::GPIOPin, capsules::button::GpioMode); 4],
        [
            // 13
            (
                &nrf5x::gpio::PORT[BUTTON1_PIN],
                capsules::button::GpioMode::LowWhenPressed
            ),
            // 14
            (
                &nrf5x::gpio::PORT[BUTTON2_PIN],
                capsules::button::GpioMode::LowWhenPressed
            ),
            // 15
            (
                &nrf5x::gpio::PORT[BUTTON3_PIN],
                capsules::button::GpioMode::LowWhenPressed
            ),
            // 16
            (
                &nrf5x::gpio::PORT[BUTTON4_PIN],
                capsules::button::GpioMode::LowWhenPressed
            ),
        ]
    );

    // Make non-volatile memory writable and activate the reset button
    let uicr = nrf52::uicr::Uicr::new();
    nrf52::nvmc::NVMC.erase_uicr();
    nrf52::nvmc::NVMC.configure_writeable();
    while !nrf52::nvmc::NVMC.is_ready() {}
    uicr.set_psel0_reset_pin(BUTTON_RST_PIN);
    while !nrf52::nvmc::NVMC.is_ready() {}
    uicr.set_psel1_reset_pin(BUTTON_RST_PIN);

    // Configure kernel debug gpios as early as possible
    kernel::debug::assign_gpios(
        Some(&nrf5x::gpio::PORT[LED2_PIN]),
        Some(&nrf5x::gpio::PORT[LED3_PIN]),
        Some(&nrf5x::gpio::PORT[LED4_PIN]),
    );

    //
    // GPIO Pins
    //
    let gpio = static_init!(
        capsules::gpio::GPIO<'static, nrf5x::gpio::GPIOPin>,
        capsules::gpio::GPIO::new(
            gpio_pins,
            board_kernel.create_grant(&memory_allocation_capability)
        )
    );
    for pin in gpio_pins.iter() {
        pin.set_client(gpio);
    }

    //
    // LEDs
    //
    let led = static_init!(
        capsules::led::LED<'static, nrf5x::gpio::GPIOPin>,
        capsules::led::LED::new(led_pins)
    );

    //
    // Buttons
    //
    let button = static_init!(
        capsules::button::Button<'static, nrf5x::gpio::GPIOPin>,
        capsules::button::Button::new(
            button_pins,
            board_kernel.create_grant(&memory_allocation_capability)
        )
    );
    for &(btn, _) in button_pins.iter() {
        use kernel::hil::gpio::PinCtl;
        btn.set_input_mode(kernel::hil::gpio::InputMode::PullUp);
        btn.set_client(button);
    }

    //
    // RTC for Timers
    //
    let rtc = &nrf5x::rtc::RTC;
    rtc.start();
    let mux_alarm = static_init!(
        capsules::virtual_alarm::MuxAlarm<'static, nrf5x::rtc::Rtc>,
        capsules::virtual_alarm::MuxAlarm::new(&nrf5x::rtc::RTC)
    );
    rtc.set_client(mux_alarm);

    //
    // Timer/Alarm
    //

    // Virtual alarm for the userspace timers
    let alarm_driver_virtual_alarm = static_init!(
        capsules::virtual_alarm::VirtualMuxAlarm<'static, nrf5x::rtc::Rtc>,
        capsules::virtual_alarm::VirtualMuxAlarm::new(mux_alarm)
    );

    // Userspace timer driver
    let alarm = static_init!(
        capsules::alarm::AlarmDriver<
            'static,
            capsules::virtual_alarm::VirtualMuxAlarm<'static, nrf5x::rtc::Rtc>,
        >,
        capsules::alarm::AlarmDriver::new(
            alarm_driver_virtual_alarm,
            board_kernel.create_grant(&memory_allocation_capability)
        )
    );
    alarm_driver_virtual_alarm.set_client(alarm);

    //
    // RTT and Console and `debug!()`
    //

    // Virtual alarm for the Segger RTT communication channel
    let virtual_alarm_rtt = static_init!(
        capsules::virtual_alarm::VirtualMuxAlarm<'static, nrf5x::rtc::Rtc>,
        capsules::virtual_alarm::VirtualMuxAlarm::new(mux_alarm)
    );

    // RTT communication channel
    let rtt_memory = static_init!(
        capsules::segger_rtt::SeggerRttMemory,
        capsules::segger_rtt::SeggerRttMemory::new(
            b"Terminal\0",
            &mut capsules::segger_rtt::UP_BUFFER,
            b"Terminal\0",
            &mut capsules::segger_rtt::DOWN_BUFFER
        )
    );
    let rtt = static_init!(
        capsules::segger_rtt::SeggerRtt<VirtualMuxAlarm<'static, nrf5x::rtc::Rtc>>,
        capsules::segger_rtt::SeggerRtt::new(
            virtual_alarm_rtt,
            rtt_memory,
            &mut capsules::segger_rtt::UP_BUFFER,
            &mut capsules::segger_rtt::DOWN_BUFFER
        )
    );
    virtual_alarm_rtt.set_client(rtt);

    //
    // Virtual UART
    //

    // Create a shared UART channel for the console and for kernel debug.
    let uart_mux = static_init!(
        MuxUart<'static>,
        MuxUart::new(rtt, &mut capsules::virtual_uart::RX_BUF, 115200)
    );
    kernel::hil::uart::Transmit::set_transmit_client(rtt, uart_mux);
    kernel::hil::uart::Receive::set_receive_client(rtt, uart_mux);

    // Create a UartDevice for the console.
    let console_uart = static_init!(UartDevice, UartDevice::new(uart_mux, true));
    console_uart.setup();

    // Create the console object for apps to printf()
    let console = static_init!(
        capsules::console::Console,
        capsules::console::Console::new(
            console_uart,
            &mut capsules::console::WRITE_BUF,
            &mut capsules::console::READ_BUF,
            board_kernel.create_grant(&memory_allocation_capability)
        )
    );
    kernel::hil::uart::Transmit::set_transmit_client(console_uart, console);
    kernel::hil::uart::Receive::set_receive_client(console_uart, console);

    // Create virtual device for kernel debug.
    let debugger_uart = static_init!(UartDevice, UartDevice::new(uart_mux, false));
    debugger_uart.setup();

    // Create the debugger object that handles calls to `debug!()`
    let debugger = static_init!(
        kernel::debug::DebugWriter,
        kernel::debug::DebugWriter::new(
            debugger_uart,
            &mut kernel::debug::OUTPUT_BUF,
            &mut kernel::debug::INTERNAL_BUF,
        )
    );
    hil::uart::Transmit::set_transmit_client(debugger_uart, debugger);

    // Create the wrapper which helps with rust ownership rules.
    let debug_wrapper = static_init!(
        kernel::debug::DebugWriterWrapper,
        kernel::debug::DebugWriterWrapper::new(debugger)
    );
    kernel::debug::set_debug_writer_wrapper(debug_wrapper);

    //
    // I2C Devices
    //

    // Create shared mux for the I2C bus
    let i2c_mux = static_init!(
        capsules::virtual_i2c::MuxI2C<'static>,
        capsules::virtual_i2c::MuxI2C::new(&nrf52::i2c::TWIM0)
    );
    nrf52::i2c::TWIM0.configure(
        nrf5x::pinmux::Pinmux::new(21),
        nrf5x::pinmux::Pinmux::new(20),
    );
    nrf52::i2c::TWIM0.set_client(i2c_mux);

    // Configure the MCP23017. Device address 0x20.
    let mcp23017_i2c = static_init!(
        capsules::virtual_i2c::I2CDevice,
        capsules::virtual_i2c::I2CDevice::new(i2c_mux, 0x40)
    );
    let mcp23017 = static_init!(
        capsules::mcp230xx::MCP230xx<'static>,
        capsules::mcp230xx::MCP230xx::new(
            mcp23017_i2c,
            Some(&nrf5x::gpio::PORT[11]),
            Some(&nrf5x::gpio::PORT[12]),
            &mut capsules::mcp230xx::BUFFER,
            8,
            2
        )
    );
    mcp23017_i2c.set_client(mcp23017);
    nrf5x::gpio::PORT[11].set_client(mcp23017);
    nrf5x::gpio::PORT[12].set_client(mcp23017);

    //
    // GPIO Extenders
    //

    // Create an array of the GPIO extenders so we can pass them to an
    // administrative layer that provides a single interface to them all.
    let async_gpio_ports = static_init!([&'static capsules::mcp230xx::MCP230xx; 1], [mcp23017]);

    // `gpio_async` is the object that manages all of the extenders.
    let gpio_async = static_init!(
        capsules::gpio_async::GPIOAsync<'static, capsules::mcp230xx::MCP230xx<'static>>,
        capsules::gpio_async::GPIOAsync::new(async_gpio_ports)
    );
    // Setup the clients correctly.
    for port in async_gpio_ports.iter() {
        port.set_client(gpio_async);
    }

    //
    // BLE
    //

    // Virtual alarm for the BLE stack
    let ble_radio_virtual_alarm = static_init!(
        capsules::virtual_alarm::VirtualMuxAlarm<'static, nrf5x::rtc::Rtc>,
        capsules::virtual_alarm::VirtualMuxAlarm::new(mux_alarm)
    );

    // Setup the BLE radio object that implements the BLE stack
    let ble_radio = static_init!(
        capsules::ble_advertising_driver::BLE<
            'static,
            nrf52::radio::Radio,
            VirtualMuxAlarm<'static, Rtc>,
        >,
        capsules::ble_advertising_driver::BLE::new(
            &mut nrf52::radio::RADIO,
            board_kernel.create_grant(&memory_allocation_capability),
            &mut capsules::ble_advertising_driver::BUF,
            ble_radio_virtual_alarm
        )
    );
    kernel::hil::ble_advertising::BleAdvertisementDriver::set_receive_client(
        &nrf52::radio::RADIO,
        ble_radio,
    );
    kernel::hil::ble_advertising::BleAdvertisementDriver::set_transmit_client(
        &nrf52::radio::RADIO,
        ble_radio,
    );
    ble_radio_virtual_alarm.set_client(ble_radio);

    //
    // Temperature
    //

    // Setup internal temperature sensor
    let temp = static_init!(
        capsules::temperature::TemperatureSensor<'static>,
        capsules::temperature::TemperatureSensor::new(
            &mut nrf5x::temperature::TEMP,
            board_kernel.create_grant(&memory_allocation_capability)
        )
    );
    kernel::hil::sensors::TemperatureDriver::set_client(&nrf5x::temperature::TEMP, temp);

    //
    // RNG
    //

    // Convert hardware RNG to the Random interface.
    let entropy_to_random = static_init!(
        capsules::rng::Entropy32ToRandom<'static>,
        capsules::rng::Entropy32ToRandom::new(&nrf5x::trng::TRNG)
    );
    nrf5x::trng::TRNG.set_client(entropy_to_random);

    // Setup RNG for userspace
    let rng = static_init!(
        capsules::rng::RngDriver<'static>,
        capsules::rng::RngDriver::new(
            entropy_to_random,
            board_kernel.create_grant(&memory_allocation_capability)
        )
    );
    entropy_to_random.set_client(rng);

    //
    // Light Sensor
    //

    // Setup Analog Light Sensor
    let analog_light_sensor = static_init!(
        capsules::analog_sensor::AnalogLightSensor<'static, nrf52::adc::Adc>,
        capsules::analog_sensor::AnalogLightSensor::new(
            &nrf52::adc::ADC,
            &nrf52::adc::AdcChannel::AnalogInput5,
            capsules::analog_sensor::AnalogLightSensorType::LightDependentResistor,
        )
    );
    nrf52::adc::ADC.set_client(analog_light_sensor);

    // Create userland driver for ambient light sensor
    let light = static_init!(
        capsules::ambient_light::AmbientLight<'static>,
        capsules::ambient_light::AmbientLight::new(
            analog_light_sensor,
            board_kernel.create_grant(&memory_allocation_capability)
        )
    );
    hil::sensors::AmbientLight::set_client(analog_light_sensor, light);

    //
    // PWM
    //
    let mux_pwm = static_init!(
        capsules::virtual_pwm::MuxPwm<'static, nrf52::pwm::Pwm>,
        capsules::virtual_pwm::MuxPwm::new(&nrf52::pwm::PWM0)
    );
    let virtual_pwm_buzzer = static_init!(
        capsules::virtual_pwm::PwmPinUser<'static, nrf52::pwm::Pwm>,
        capsules::virtual_pwm::PwmPinUser::new(mux_pwm, nrf5x::pinmux::Pinmux::new(31))
    );
    virtual_pwm_buzzer.add_to_mux();

    //
    // Buzzer
    //
    let virtual_alarm_buzzer = static_init!(
        capsules::virtual_alarm::VirtualMuxAlarm<'static, nrf5x::rtc::Rtc>,
        capsules::virtual_alarm::VirtualMuxAlarm::new(mux_alarm)
    );
    let buzzer = static_init!(
        capsules::buzzer_driver::Buzzer<
            'static,
            capsules::virtual_alarm::VirtualMuxAlarm<'static, nrf5x::rtc::Rtc>,
        >,
        capsules::buzzer_driver::Buzzer::new(
            virtual_pwm_buzzer,
            virtual_alarm_buzzer,
            capsules::buzzer_driver::DEFAULT_MAX_BUZZ_TIME_MS,
            board_kernel.create_grant(&memory_allocation_capability)
        )
    );
    virtual_alarm_buzzer.set_client(buzzer);

    // Start all of the clocks. Low power operation will require a better
    // approach than this.
    nrf52::clock::CLOCK.low_stop();
    nrf52::clock::CLOCK.high_stop();

    nrf52::clock::CLOCK.low_set_source(nrf52::clock::LowClockSource::XTAL);
    nrf52::clock::CLOCK.low_start();
    nrf52::clock::CLOCK.high_set_source(nrf52::clock::HighClockSource::XTAL);
    nrf52::clock::CLOCK.high_start();
    while !nrf52::clock::CLOCK.low_started() {}
    while !nrf52::clock::CLOCK.high_started() {}

    let platform = Platform {
        button: button,
        ble_radio: ble_radio,
        console: console,
        led: led,
        gpio: gpio,
        rng: rng,
        temp: temp,
        alarm: alarm,
        gpio_async: gpio_async,
        light: light,
        buzzer: buzzer,
        ipc: kernel::ipc::IPC::new(board_kernel, &memory_allocation_capability),
    };

    let chip = static_init!(nrf52::chip::NRF52, nrf52::chip::NRF52::new());

    nrf5x::gpio::PORT[31].make_output();
    nrf5x::gpio::PORT[31].clear();

    debug!("Initialization complete. Entering main loop\r");
    debug!("{}", &nrf52::ficr::FICR_INSTANCE);

    extern "C" {
        /// Beginning of the ROM region containing app images.
        static _sapps: u8;
    }
    kernel::procs::load_processes(
        board_kernel,
        chip,
        &_sapps as *const u8,
        &mut APP_MEMORY,
        &mut PROCESSES,
        FAULT_RESPONSE,
        &process_management_capability,
    );

    board_kernel.kernel_loop(&platform, chip, Some(&platform.ipc), &main_loop_capability);
}
