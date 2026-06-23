use core::u8;
use embedded_graphics::Pixel;
use embedded_graphics::pixelcolor::PixelColor;
use embedded_graphics::prelude::DrawTarget;
use embedded_graphics::prelude::OriginDimensions;
use embedded_graphics::prelude::Size;
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

const MASK_LUT: [u8; 8] = [0x1, 0x2, 0x4, 0x8, 0x10, 0x20, 0x40, 0x80];
// set WIDTH and HEIGHT
// this is hard-coded in landscape orientation
const WIDTH: u8 = 104;
const HEIGHT: u16 = 212;
const MID: usize = (WIDTH as usize * HEIGHT as usize) / 8;

pub struct DisplayDriver {
    buffer: [u8; (WIDTH as usize * HEIGHT as usize) / 4],
}

impl DisplayDriver {
    pub fn new() -> Self {
        DisplayDriver {
            buffer: [0xFF; (WIDTH as usize * HEIGHT as usize) / 4],
        }
    }

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

    pub fn wake_panel(spi_device: &mut impl SpiDevice, dc_out: &mut Output, delay: &mut Delay) {
        DisplayDriver::send_command(spi_device, dc_out, 0x04u8, delay);
        // TODO: replace with waitForEpd
        delay.delay_ms(2000);
        DisplayDriver::send_command(spi_device, dc_out, 0x00u8, delay); // Enter panel setting
        DisplayDriver::send_data(spi_device, dc_out, &[0x0fu8], delay);
        DisplayDriver::send_data(spi_device, dc_out, &[0x89u8], delay);
        DisplayDriver::send_command(spi_device, dc_out, 0x61u8, delay); // Enter panel resolution setting
        DisplayDriver::send_data(spi_device, dc_out, &[WIDTH], delay);
        DisplayDriver::send_data(spi_device, dc_out, &[(HEIGHT >> 8) as u8], delay);
        DisplayDriver::send_data(spi_device, dc_out, &[(HEIGHT & 0xff) as u8], delay);
        DisplayDriver::send_command(spi_device, dc_out, 0x50u8, delay); // VCOM and data interval setting
        DisplayDriver::send_data(spi_device, dc_out, &[0x77u8], delay);
    }

    pub fn display(&self, spi_device: &mut impl SpiDevice, dc_out: &mut Output, delay: &mut Delay) {
        // update bw pixels
        DisplayDriver::send_command(spi_device, dc_out, 0x10u8, delay);
        DisplayDriver::send_data(spi_device, dc_out, &self.buffer[..MID], delay);

        // update red pixels
        DisplayDriver::send_command(spi_device, dc_out, 0x13u8, delay);
        DisplayDriver::send_data(spi_device, dc_out, &self.buffer[MID..], delay);

        // stop data transfer
        DisplayDriver::send_command(spi_device, dc_out, 0x11u8, delay); // VCOM and data interval setting
        DisplayDriver::send_data(spi_device, dc_out, &[0x00u8], delay);

        // send display refresh command
        DisplayDriver::send_command(spi_device, dc_out, 0x12u8, delay);
        delay.delay_micros(500u32);
        // TODO: replace with waitForEpd
        delay.delay_ms(10000u32);
    }

    pub fn write_pixel_internal(&mut self, x0: u16, y0: u16, color: u8) {
        // early return if request is outside pixel bounds
        // note this is hard-coded for LANDSCAPE orientation.
        // TODO: implement rotation
        if y0 > WIDTH as u16 - 1 || x0 > HEIGHT - 1 {
            return;
        }
        let mut x_rot = y0;
        let y_rot = x0;
        x_rot = WIDTH as u16 - x_rot - 1;
        // locate column of bytes in row
        let x_byte = x_rot / 8;
        // locate bit in byte
        let x_bit = x_rot % 8;
        // collapse x and y positions to byte position in bit-packed array
        // you can think of this as drilling down rows (via y0), then advancing by column (with x_byte)
        // note that this does not handle the specific bit, that comes later
        let byte_pos = WIDTH as u16 / 8 * y_rot + x_byte;
        // clear bw bit (set to 1)
        self.buffer[byte_pos as usize] |= MASK_LUT[(7 - x_bit) as usize];
        // clear red bit (set to 1)
        self.buffer[byte_pos as usize + MID] |= MASK_LUT[(7 - x_bit) as usize];
        // write bw bit
        // if color is 0, bit remains unchanged
        // example transformation for bit 0x80
        // 00000001 (Color) > 10000000 (Shifted) > 01111111 (Flipped) > MSB changed, all other bits left unchanged
        if color < 2 {
            self.buffer[byte_pos as usize] &= !(color << (7 - x_bit));
        } else {
            self.buffer[byte_pos as usize + MID] &= !(MASK_LUT[(7 - x_bit) as usize]);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayColor {
    White,
    Black,
    Red,
}

impl PixelColor for DisplayColor {
    type Raw = ();
}

impl OriginDimensions for DisplayDriver {
    fn size(&self) -> Size {
        // remember, we are hardcoding a logical landscape ratio, hence the flip
        Size::new(HEIGHT as u32, WIDTH as u32)
    }
}

impl DrawTarget for DisplayDriver {
    type Color = DisplayColor;
    type Error = core::convert::Infallible;
    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            let raw_color = match color {
                DisplayColor::White => 0,
                DisplayColor::Black => 1,
                DisplayColor::Red => 2,
            };
            self.write_pixel_internal(point.x as u16, point.y as u16, raw_color);
        }
        Ok(())
    }
}
