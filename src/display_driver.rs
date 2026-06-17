use core::u8;
use embedded_hal::delay::DelayNs;
use embedded_hal::spi::SpiDevice;
use esp_hal::delay::Delay;
use esp_hal::gpio::Input;
use esp_hal::gpio::InputConfig;
use esp_hal::gpio::Output;
use esp_hal::gpio::OutputConfig;
use esp_hal::gpio::Pull;
use esp_hal::time::Duration;
use esp_hal::timer::Timer;

// INKPLATE PORT - - -
pub fn reset_panel(rst_pin: &mut Output, delay: &mut Delay) {
    rst_pin.set_low();
    delay.delay_ms(100u32);
    rst_pin.set_high();
    delay.delay_ms(100u32);
}

pub fn send_command(
    spi_dev: &mut impl SpiDevice,
    dc_pin: &mut Output,
    command: u8,
    delay: &mut Delay,
) {
    dc_pin.set_low();
    delay.delay_us(10u32);
    spi_dev.write(&[command]);
    dc_pin.set_high();
    delay.delay_ms(1u32);
}

pub fn send_data(
    spi_dev: &mut impl SpiDevice,
    dc_pin: &mut Output,
    data: &[u8],
    delay: &mut Delay,
) {
    dc_pin.set_high();
    delay.delay_us(10u32);
    spi_dev.write(data);
    delay.delay_ms(1u32);
}

pub fn wait_for_epd(
    busy_pin: &mut Input,
    timeout: u64,
    timer: &mut impl Timer,
    delay: &mut Delay,
) -> bool {
    timer.load_value(Duration::from_millis(timeout)).unwrap();
    timer.start();
    while busy_pin.is_low() && !timer.is_interrupt_set() {
        continue;
    }
    if busy_pin.is_low() {
        timer.clear_interrupt();
        return false;
    }
    timer.clear_interrupt();
    delay.delay_ms(200u32);
    true
}
