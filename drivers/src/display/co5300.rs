//! CO5300 QSPI AMOLED display driver — HAL-agnostic.
//!
//! The Waveshare ESP32-S3-Touch-AMOLED-2.06 uses a CO5300 controller
//! (same MIPI DCS command set as RM67162, different vendor).
//!
//! Interface: QSPI — 4 data lines, no DC pin.
//!
//! Wire format — confirmed by CO5300 datasheet section 5.2.2:
//!   Control command: [0x02][0x00][CMD][0x00][params on 1 line]
//!   Pixel write start: [0x32][0x00][0x2C][0x00][pixels on 4 lines]
//!   Pixel write cont.: [0x32][0x00][0x3C][0x00][pixels on 4 lines]
//!
//! The CMD byte sits in AD[15:8] (the MIDDLE byte of the 24-bit address field).
//! AD[23:0] = {8'h00, CMD[7:0], 8'h00}  →  address value = (CMD as u32) << 8
//!
//! Do NOT send 0x38 (Enter Quad SPI) — that switches ALL communication to 4-wire,
//! breaking subsequent opcode-0x02 commands. The mixed approach (0x02 for config,
//! 0x32 for pixels) works without any mode-switch command.

use embedded_hal::digital::OutputPin;
use embedded_graphics_core::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::Rgb565,
    primitives::Rectangle,
    Pixel,
};

use super::qspi::QspiWrite;

/// Display resolution (panel H0198S005AMT005_V0_195)
pub const WIDTH:  u16 = 410;
pub const HEIGHT: u16 = 502;

/// Column offset applied by this specific panel inside the CO5300 frame buffer.
const COL_OFFSET: u16 = 22;
const ROW_OFFSET: u16 = 0;

/// MIPI DCS command bytes (standard across CO5300 / RM67162 family)
pub mod cmd {
    pub const SLPIN:      u8 = 0x10; // Enter Sleep Mode
    pub const SLPOUT:     u8 = 0x11; // Sleep Out — wait 120 ms after
    pub const DISPOFF:    u8 = 0x28; // Display Off
    pub const DISPON:     u8 = 0x29; // Display On
    pub const CASET:      u8 = 0x2A; // Column Address Set
    pub const RASET:      u8 = 0x2B; // Row Address Set
    pub const RAMWR:      u8 = 0x2C; // Memory Start Write
    pub const RAMWRC:     u8 = 0x3C; // Memory Continue Write
    pub const MADCTL:     u8 = 0x36; // Memory Access Control (rotation)
    pub const PIXFMT:     u8 = 0x3A; // Pixel Format
    pub const BRIGHTNESS: u8 = 0x51; // Write Display Brightness
}

/// RGB565 colour constants
pub mod color {
    pub const BLACK: u16 = 0x0000;
    pub const WHITE: u16 = 0xFFFF;
    pub const RED:   u16 = 0xF800;
    pub const GREEN: u16 = 0x07E0;
    pub const BLUE:  u16 = 0x001F;
}

/// Display orientation / mirroring. Sent as the MADCTL parameter (0x36).
///
/// The CO5300 MADCTL register only implements three bits:
///   MY  = bit 7 — mirror Y axis
///   MX  = bit 6 — mirror X axis
///   RGB = bit 3 — colour order (0 = RGB, 1 = BGR)
///
/// There is no MV (row/column swap) bit, so true 90° / 270° hardware
/// rotation is not available. Software pixel transposition would be needed
/// for landscape mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotation {
    /// Normal portrait orientation (no mirroring).
    #[default]
    Normal    = 0x00,   // MY=0 MX=0
    /// Mirror X axis (horizontal flip).
    MirrorX   = 0x40,   // MX=1
    /// Mirror Y axis (vertical flip).
    MirrorY   = 0x80,   // MY=1
    /// 180° rotation (mirror both axes).
    Deg180    = 0xC0,   // MY=1 MX=1
}

/// Pixel FIFO limit: ESP32-S3 blocking SPI = 64 bytes = 32 RGB565 pixels.
/// Kept conservative so this driver also works on MCUs with smaller FIFOs.
const FIFO_PIXELS: usize = 32;

/// CO5300 driver. Generic over the QSPI bus `B` and reset pin `RST`.
pub struct CO5300<B, RST> {
    bus:   B,
    reset: RST,
}

impl<B: QspiWrite, RST: OutputPin> CO5300<B, RST> {
    pub fn new(bus: B, reset: RST) -> Self {
        Self { bus, reset }
    }

    // ---- Reset helpers (caller provides delays via Timer::after) ----------------

    pub fn reset_high(&mut self) { self.reset.set_high().ok(); }
    pub fn reset_low(&mut self)  { self.reset.set_low().ok();  }

    // ---- Init -------------------------------------------------------------------

    /// Configuration init — call order:
    ///   1. hardware reset  (RST low→high, 120 ms settle)
    ///   2. display.init()
    ///   3. display.wake()  + 120 ms
    ///   4. display.display_on() + 70 ms
    pub fn init(&mut self) {
        // 0xC4 enables QSPI pixel write via opcode 0x32 (Set_DSPi Mode).
        self.write_cmd(0xC4,         &[0x80]);
        self.write_cmd(cmd::PIXFMT,  &[0x55]); // RGB565 (16 bpp)
        self.write_cmd(0x35,         &[0x00]); // Tearing Effect On, mode 0
        self.write_cmd(0x53,         &[0x20]); // Write Control Display — BC_EN=1
        self.write_cmd(cmd::BRIGHTNESS, &[80]); // ~31% brightness
        self.write_cmd(0x63,         &[0xFF]); // HBM brightness max
        // NOTE: do NOT send 0x38 (Enter Quad SPI) — it breaks opcode-0x02 commands.
    }

    // ---- Power / backlight ------------------------------------------------------

    /// Put the display into sleep mode (low power).
    ///
    /// Timing requirements (datasheet §5.6.2 / §7.5.11):
    ///   - Wait ≥ 5 ms after this command before any subsequent command.
    ///   - Wait ≥ 120 ms after SLPOUT before issuing SLPIN.
    pub fn sleep(&mut self) {
        self.write_cmd(cmd::SLPIN, &[]);
    }

    /// Exit sleep mode.
    ///
    /// Timing requirements (datasheet §5.6.1 / §7.5.12):
    ///   - Wait ≥ 5 ms after this command before any subsequent command.
    ///   - Wait ≥ 120 ms after SLPIN before issuing SLPOUT.
    ///   - Caller must wait ≥ 120 ms before sending the first pixel write.
    pub fn wake(&mut self) {
        self.write_cmd(cmd::SLPOUT, &[]);
    }

    /// Turn the display output on (pixels visible).
    pub fn display_on(&mut self) {
        self.write_cmd(cmd::DISPON, &[]);
    }

    /// Turn the display output off (backlight stays on, panel goes blank).
    pub fn display_off(&mut self) {
        self.write_cmd(cmd::DISPOFF, &[]);
    }

    /// Set the display brightness (0 = off, 255 = maximum).
    pub fn set_brightness(&mut self, brightness: u8) {
        self.write_cmd(cmd::BRIGHTNESS, &[brightness]);
    }

    /// Set the display orientation (mirroring). See [`Rotation`] for available modes.
    /// The CO5300 does not support hardware 90°/270° rotation.
    pub fn set_rotation(&mut self, rotation: Rotation) {
        self.write_cmd(cmd::MADCTL, &[rotation as u8]);
    }

    // ---- Drawing primitives -----------------------------------------------------

    /// Fill a rectangle with a single RGB565 colour.
    pub fn fill_solid(&mut self, x: u16, y: u16, w: u16, h: u16, color: u16) {
        self.set_addr_window(x, y, w, h);
        let hi = (color >> 8) as u8;
        let lo = (color & 0xFF) as u8;
        let total = w as u32 * h as u32;
        self.write_pixel_chunks(total, |buf, n| {
            for i in 0..n {
                buf[i * 2]     = hi;
                buf[i * 2 + 1] = lo;
            }
        });
    }

    /// Write a rectangle of RGB565 pixels from a slice.
    ///
    /// `pixels` must contain exactly `w * h` values. If fewer are provided
    /// only those pixels are written; the remainder of the window is undefined.
    pub fn draw_raw(&mut self, x: u16, y: u16, w: u16, h: u16, pixels: &[u16]) {
        self.set_addr_window(x, y, w, h);
        let total = (w as u32 * h as u32).min(pixels.len() as u32);
        let mut offset = 0usize;
        self.write_pixel_chunks(total, |buf, n| {
            for i in 0..n {
                let px = pixels[offset + i];
                buf[i * 2]     = (px >> 8) as u8;
                buf[i * 2 + 1] = (px & 0xFF) as u8;
            }
            offset += n;
        });
    }

    /// Write a single RGB565 pixel at (x, y).
    pub fn draw_pixel(&mut self, x: u16, y: u16, color: u16) {
        self.set_addr_window(x, y, 1, 1);
        let buf = [(color >> 8) as u8, (color & 0xFF) as u8];
        self.bus.write_pixels(true, &buf).ok();
    }

    // ---- Raw DCS command --------------------------------------------------------

    /// Send one MIPI DCS command with optional parameters (single-wire).
    pub fn write_cmd(&mut self, command: u8, params: &[u8]) {
        self.bus.write_cmd(command, params).ok();
    }

    // ---- Private helpers --------------------------------------------------------

    fn set_addr_window(&mut self, x: u16, y: u16, w: u16, h: u16) {
        let x0 = x + COL_OFFSET;
        let x1 = x + w - 1 + COL_OFFSET;
        let y0 = y + ROW_OFFSET;
        let y1 = y + h - 1 + ROW_OFFSET;

        self.bus.write_cmd(cmd::CASET, &[
            (x0 >> 8) as u8, (x0 & 0xFF) as u8,
            (x1 >> 8) as u8, (x1 & 0xFF) as u8,
        ]).ok();
        self.bus.write_cmd(cmd::RASET, &[
            (y0 >> 8) as u8, (y0 & 0xFF) as u8,
            (y1 >> 8) as u8, (y1 & 0xFF) as u8,
        ]).ok();
    }

    fn write_pixel_chunks<F>(&mut self, total: u32, mut fill: F)
    where
        F: FnMut(&mut [u8], usize),
    {
        let mut buf = [0u8; FIFO_PIXELS * 2];
        let mut left = total;
        let mut first = true;

        while left > 0 {
            let n = left.min(FIFO_PIXELS as u32) as usize;
            fill(&mut buf, n);
            self.bus.write_pixels(first, &buf[..n * 2]).ok();
            first = false;
            left -= n as u32;
        }
    }
}

// ---- embedded-graphics DrawTarget -------------------------------------------

impl<B: QspiWrite, RST: OutputPin> OriginDimensions for CO5300<B, RST> {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl<B: QspiWrite, RST: OutputPin> DrawTarget for CO5300<B, RST> {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        use embedded_graphics_core::pixelcolor::RgbColor;
        for Pixel(pt, color) in pixels {
            if pt.x >= 0 && pt.y >= 0
                && (pt.x as u16) < WIDTH
                && (pt.y as u16) < HEIGHT
            {
                let rgb565 = ((color.r() as u16) << 11)
                    | ((color.g() as u16) << 5)
                    | (color.b() as u16);
                self.draw_pixel(pt.x as u16, pt.y as u16, rgb565);
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(
        &mut self,
        area: &Rectangle,
        colors: I,
    ) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Rgb565>,
    {
        use embedded_graphics_core::pixelcolor::RgbColor;

        // Clip the area to the display bounds.
        let x0 = area.top_left.x.max(0) as u16;
        let y0 = area.top_left.y.max(0) as u16;
        let x1 = ((area.top_left.x + area.size.width  as i32 - 1).min(WIDTH  as i32 - 1)) as u16;
        let y1 = ((area.top_left.y + area.size.height as i32 - 1).min(HEIGHT as i32 - 1)) as u16;

        if x0 > x1 || y0 > y1 {
            return Ok(());
        }

        let w = x1 - x0 + 1;
        let h = y1 - y0 + 1;

        self.set_addr_window(x0, y0, w, h);

        let total = w as u32 * h as u32;
        let mut colors = colors.into_iter();
        let mut left = total;
        let mut first = true;

        let mut buf = [0u8; FIFO_PIXELS * 2];
        while left > 0 {
            let n = left.min(FIFO_PIXELS as u32) as usize;
            for i in 0..n {
                let c = colors.next().unwrap_or(Rgb565::BLACK);
                let rgb = ((c.r() as u16) << 11)
                    | ((c.g() as u16) << 5)
                    | (c.b() as u16);
                buf[i * 2]     = (rgb >> 8) as u8;
                buf[i * 2 + 1] = (rgb & 0xFF) as u8;
            }
            self.bus.write_pixels(first, &buf[..n * 2]).ok();
            first = false;
            left -= n as u32;
        }

        Ok(())
    }

    fn clear(&mut self, color: Rgb565) -> Result<(), Self::Error> {
        use embedded_graphics_core::pixelcolor::RgbColor;
        let rgb565 = ((color.r() as u16) << 11)
            | ((color.g() as u16) << 5)
            | (color.b() as u16);
        self.fill_solid(0, 0, WIDTH, HEIGHT, rgb565);
        Ok(())
    }
}
