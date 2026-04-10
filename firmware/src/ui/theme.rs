//! Cyberpunk color palette and layout constants.

use embedded_graphics::pixelcolor::Rgb565;

// -- Primary palette --
pub const BG:       Rgb565 = Rgb565::new(0, 0, 0);     // true black - AMOLED pixels off
pub const CYAN:     Rgb565 = Rgb565::new(0, 63, 31);    // #00FFFF
pub const RED:      Rgb565 = Rgb565::new(31, 0, 2);     // #F80010 - warning/danger
pub const YELLOW:   Rgb565 = Rgb565::new(31, 58, 0);    // #F8E800 - highlights/active
pub const DIM_CYAN: Rgb565 = Rgb565::new(0, 20, 10);    // #005050 - inactive/border dim
pub const DARK_RED: Rgb565 = Rgb565::new(10, 0, 0);     // #500000 - bar background
pub const PANEL_BG: Rgb565 = Rgb565::new(1, 1, 1);      // dark grey overlay background

// -- Screen geometry --
pub const SCREEN_W: u16 = 410;
pub const SCREEN_H: u16 = 502;

// -- Rounded corner safe zones --
pub const CORNER_R: i32 = 98;          // bezel corner radius in pixels

// -- Layout zones --
pub const HEADER_Y: i32 = 40;          // vertical center of header content
pub const CONTENT_TOP: i32 = CORNER_R; // main content starts here (full width safe)
pub const CONTENT_BOTTOM: i32 = (SCREEN_H as i32) - CORNER_R; // main content ends here
pub const CONTENT_H: i32 = CONTENT_BOTTOM - CONTENT_TOP; // 306px
pub const FOOTER_Y: i32 = (SCREEN_H as i32) - 58; // vertical center of footer content

pub const MARGIN: i32 = 8;             // side margin for content area
pub const CUT: i32 = 8;               // corner cut size for cyberpunk boxes
