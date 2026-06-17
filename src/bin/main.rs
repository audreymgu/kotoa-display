#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]
// #![deny(warnings)]

use alloc::vec;
use alloc::vec::Vec;
use core::prelude::v1::*;
use core::u8;
use embedded_hal::delay::DelayNs;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::Input;
use esp_hal::gpio::InputConfig;
use esp_hal::gpio::Output;
use esp_hal::gpio::OutputConfig;
use esp_hal::gpio::Pull;
use esp_hal::main;
use esp_hal::spi::Mode;
use esp_hal::spi::master::Config;
use esp_hal::spi::master::Spi;
use esp_hal::time::Rate;
use esp_hal::time::{Duration, Instant};
use esp_hal::timer::timg::TimerGroup;
use kotoa_display::display_driver::*;
use log::info;

use embedded_graphics::{
    prelude::*,
    primitives::{Line, PrimitiveStyle, PrimitiveStyleBuilder},
};
use epd_waveshare::{epd2in13bc::*, prelude::*};

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[main]
fn main() -> ! {
    // generator version: 1.3.0
    // generator parameters: --chip esp32 -o esp32-wrover-e -o unstable-hal -o alloc -o wifi -o esp-backtrace -o log -o vscode -o esp

    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // The following pins are used to bootstrap the chip. They are available
    // for use, but check the datasheet of the module for more information on them.
    // - GPIO0
    // - GPIO2
    // - GPIO5
    // - GPIO12
    // - GPIO15
    // These GPIO pins are in use by some feature of the module and should not be used.
    let _ = peripherals.GPIO6;
    let _ = peripherals.GPIO7;
    let _ = peripherals.GPIO8;
    let _ = peripherals.GPIO9;
    let _ = peripherals.GPIO10;
    let _ = peripherals.GPIO11;
    let _ = peripherals.GPIO16;
    let _ = peripherals.GPIO17;
    let _ = peripherals.GPIO20;

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 98768);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);
    let (mut _wifi_controller, _interfaces) =
        esp_radio::wifi::new(peripherals.WIFI, Default::default())
            .expect("Failed to initialize Wi-Fi controller");

    // PIN SETUP - - -
    // set chip select pin
    let cs = peripherals.GPIO27;
    let cs_output = Output::new(
        cs,
        esp_hal::gpio::Level::Low,
        OutputConfig::default().with_drive_mode(esp_hal::gpio::DriveMode::PushPull),
    );
    // set dc pin
    let dc = peripherals.GPIO33;
    let mut dc_out = Output::new(
        dc,
        esp_hal::gpio::Level::Low,
        OutputConfig::default().with_drive_mode(esp_hal::gpio::DriveMode::PushPull),
    );
    // set rst pin
    let rst = peripherals.GPIO19;
    let mut rst_out = Output::new(
        rst,
        esp_hal::gpio::Level::Low,
        OutputConfig::default().with_drive_mode(esp_hal::gpio::DriveMode::PushPull),
    );
    // set busy pin
    let busy = peripherals.GPIO32;
    let config = InputConfig::default().with_pull(Pull::Up);
    let mut busy_in = Input::new(busy, config);

    // RESET PANEL - - -
    let mut delay = Delay::new();
    let mut timer = timg0.timer1;
    delay.delay_ms(10u32);
    delay.delay_ms(2000u32);
    reset_panel(&mut rst_out, &mut delay);
    // INITIALIZE SPI - - -
    let mut spi = Spi::new(
        peripherals.SPI2,
        Config::default()
            .with_frequency(Rate::from_khz(100))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_sck(peripherals.GPIO18)
    .with_mosi(peripherals.GPIO23);
    let mut spi_device = ExclusiveDevice::new_no_delay(spi, cs_output).unwrap();

    // CONFIGURE PANEL - - -
    send_command(&mut spi_device, &mut dc_out, 0x04u8, &mut delay);
    // TODO: replace with waitForEpd
    delay.delay_ms(2000);
    send_command(&mut spi_device, &mut dc_out, 0x00u8, &mut delay); // Enter panel setting
    send_data(&mut spi_device, &mut dc_out, &[0x0fu8], &mut delay);
    send_data(&mut spi_device, &mut dc_out, &[0x89u8], &mut delay);
    send_command(&mut spi_device, &mut dc_out, 0x61u8, &mut delay); // Enter panel resolution setting
    // set WIDTH and HEIGHT
    const WIDTH: u8 = 104;
    const HEIGHT: u16 = 212;
    const MID: usize = (WIDTH as usize * HEIGHT as usize) / 8;
    send_data(&mut spi_device, &mut dc_out, &[WIDTH], &mut delay);
    send_data(
        &mut spi_device,
        &mut dc_out,
        &[(HEIGHT >> 8) as u8],
        &mut delay,
    );
    send_data(
        &mut spi_device,
        &mut dc_out,
        &[(HEIGHT & 0xff) as u8],
        &mut delay,
    );
    send_command(&mut spi_device, &mut dc_out, 0x50u8, &mut delay); // VCOM and data interval setting
    send_data(&mut spi_device, &mut dc_out, &[0x77u8], &mut delay);

    // BLANK OUT PANEL - - -
    let mut buffer = [0xFF; (WIDTH as usize * HEIGHT as usize) / 4];

    // BLANK BW PIXELS
    send_command(&mut spi_device, &mut dc_out, 0x10u8, &mut delay);
    send_data(&mut spi_device, &mut dc_out, &buffer[..MID], &mut delay);

    // BLANK RED PIXELS
    send_command(&mut spi_device, &mut dc_out, 0x13u8, &mut delay);
    send_data(&mut spi_device, &mut dc_out, &buffer[MID..], &mut delay);

    // STOP DATA TRANSFER - - -
    send_command(&mut spi_device, &mut dc_out, 0x11u8, &mut delay); // VCOM and data interval setting
    send_data(&mut spi_device, &mut dc_out, &[0x00u8], &mut delay);

    // SEND DISPLAY REFRESH COMMAND - - -
    send_command(&mut spi_device, &mut dc_out, 0x12u8, &mut delay);
    delay.delay_micros(500u32);
    // TODO: replace with waitForEpd
    delay.delay_ms(10000u32);

    // EXAMPLE CODE BEGINS - - -

    // // Setup EPD Waveshare
    // let mut epd =
    //     Epd2in13bc::new(&mut spi_device, busy_in, dc_out, rst_out, &mut delay, None).unwrap();

    // // Use display graphics from embedded-graphics
    // // This display is for the black/white/chromatic pixels
    // let mut tricolor_display = Display2in13bc::default();

    // // Use embedded graphics for drawing a black line
    // let _ = Line::new(Point::new(0, 0), Point::new(220, 0))
    //     .into_styled(PrimitiveStyle::with_stroke(TriColor::Black, 1))
    //     .draw(&mut tricolor_display);

    // let _ = Line::new(Point::new(100, 120), Point::new(100, 200))
    //     .into_styled(PrimitiveStyle::with_stroke(TriColor::Chromatic, 1))
    //     .draw(&mut tricolor_display);

    // let _ = Line::new(Point::new(100, 120), Point::new(0, 200))
    //     .into_styled(PrimitiveStyle::with_stroke(TriColor::Chromatic, 1))
    //     .draw(&mut tricolor_display);

    // // We use `chromatic` but it will be shown as red/yellow
    // let _ = Line::new(Point::new(15, 120), Point::new(15, 200))
    //     .into_styled(PrimitiveStyle::with_stroke(TriColor::Chromatic, 1))
    //     .draw(&mut tricolor_display);

    // // Display updated frame
    // epd.update_color_frame(
    //     &mut spi_device,
    //     &mut delay,
    //     &tricolor_display.bw_buffer(),
    //     &tricolor_display.chromatic_buffer(),
    // )
    // .unwrap();
    // epd.display_frame(&mut spi_device, &mut delay).unwrap();

    // // Set the EPD to sleep
    // epd.sleep(&mut spi_device, &mut delay).unwrap();

    // EXAMPLE CODE ENDS - - -

    loop {
        info!("Hello world!");
        let delay_start = Instant::now();
        while delay_start.elapsed() < Duration::from_millis(500) {}
    }
}
