//! System frame - persistent header and footer.
//!
//! Header is context-aware: on the Clock screen it renders a minimal
//! battery bar only (the screen itself owns the big clock). On every
//! other screen it renders a compact header with the small glance
//! clock alongside the battery meter.
//!
//! Footer: thin rule, screen name in small amber caps, dot carousel.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    mono_font::{ascii, MonoTextStyle},
    pixelcolor::Rgb565,
    text::{Baseline, Text},
    Drawable,
};

use super::big_digits::{self, DigitStyle};
use super::{primitives, theme};
use super::types::{ScreenId, SystemData};

/// Draw the top header. Layout depends on which screen is active: the
/// Clock screen gets a minimal battery-only header so its large clock
/// has the full content area to itself.
pub fn draw_header<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    data: &SystemData,
    active: ScreenId,
) {
    let w = theme::SCREEN_W as i32;

    // Battery meter is always present.
    draw_battery_meter(display, data.battery_percent, w);

    if active != ScreenId::Clock {
        // Compact glance clock using the small seven-segment style.
        let style = DigitStyle::SMALL;
        let tw = style.time_width();
        let tx = w / 2 - tw / 2;
        let ty = 26;
        big_digits::draw_time(display, tx, ty, data.hour, data.minute, theme::AMBER, &style);
    }

    // Header/content divider rule.
    primitives::section_rule(
        display,
        theme::MARGIN * 2,
        theme::CONTENT_TOP - 4,
        w - theme::MARGIN * 4,
        theme::AMBER_DIM,
    );
}

/// Thin flat battery meter with a small percentage label to the right.
fn draw_battery_meter<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, percent: Option<u8>, screen_w: i32,
) {
    let pct = percent.unwrap_or(0) as u16;
    let bar_y = 8;
    let bar_h = 4;
    let inset = 60; // stay clear of the top bezel curve
    let bar_w = screen_w - inset * 2 - 40;
    let bar_x = inset;

    let (fill, bg) = if pct > 50 {
        (theme::TEAL, theme::TEAL_DIM)
    } else if pct > 20 {
        (theme::AMBER, theme::AMBER_DIM)
    } else {
        (theme::RED, theme::AMBER_DIM)
    };

    primitives::flat_bar(display, bar_x, bar_y, bar_w, bar_h, pct, 100, fill, bg);

    // Percentage label
    let font = MonoTextStyle::new(&ascii::FONT_6X10, theme::AMBER);
    let mut buf = heapless::String::<8>::new();
    let _ = match percent {
        Some(p) => core::fmt::write(&mut buf, format_args!("{:>3}%", p)),
        None => core::fmt::write(&mut buf, format_args!("  -%")),
    };
    Text::with_baseline(
        &buf,
        Point::new(bar_x + bar_w + 6, bar_y - 1),
        font,
        Baseline::Top,
    )
    .draw(display).ok();
}

/// Draw the bottom footer (rule, screen name, dot carousel).
pub fn draw_footer<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    active_screen: ScreenId,
    screens: &[ScreenId],
) {
    let w = theme::SCREEN_W as i32;
    let y = theme::FOOTER_Y;

    // Divider rule at the top of the footer band.
    primitives::section_rule(
        display,
        theme::MARGIN * 2,
        y - 10,
        w - theme::MARGIN * 4,
        theme::AMBER_DIM,
    );

    // Screen name, centered, amber caps.
    let name = screen_name(active_screen);
    let font = MonoTextStyle::new(&ascii::FONT_10X20, theme::AMBER);
    let name_w = name.len() as i32 * 10;
    Text::with_baseline(
        name,
        Point::new(w / 2 - name_w / 2, y),
        font,
        Baseline::Top,
    )
    .draw(display).ok();

    // Dot carousel below the name.
    let active_idx = screens.iter().position(|s| *s == active_screen).unwrap_or(0);
    primitives::dot_carousel(
        display,
        w / 2,
        y + 34,
        screens.len(),
        active_idx,
        theme::AMBER,
        theme::AMBER_DIM,
    );
}

fn screen_name(id: ScreenId) -> &'static str {
    match id {
        ScreenId::Clock => "CLOCK",
        ScreenId::Status => "STATUS",
        ScreenId::CornerTest => "CORNER",
    }
}
