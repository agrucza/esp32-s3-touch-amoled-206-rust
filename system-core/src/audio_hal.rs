//! ESP32-S3 HAL glue for the audio I2S interface.
//!
//! ES8311 (DAC/playback) and ES7210 (ADC/capture) share the same I2S bus.
//! The ESP32-S3 is the I2S master.
//!
//! ## Pin assignments
//!
//! | Signal    | GPIO |
//! |-----------|------|
//! | MCLK      |  16  |
//! | SCLK/BCLK |  41  |
//! | LRCK/WS   |  45  |
//! | DSDIN     |  40  | (ESP32 TX -> ES8311 DAC input)
//! | ASDOUT    |  42  | (ES7210 ADC output -> ESP32 RX)
//! | PA_CTRL   |  46  | (NS4150B speaker amp enable, active HIGH)
//!
//! ## Sample format
//!
//! 16 kHz, 16-bit stereo, standard I2S (Philips/TDM).
//! DMA buffer layout: interleaved [left_ch, right_ch, ...] as i16 samples.

use esp_hal::{
    Async,
    dma::{DmaChannelFor, DmaDescriptor},
    gpio::{Output, interconnect::{PeripheralInput, PeripheralOutput}},
    i2s::master::{I2s, I2sRx, I2sTx, Config, DataFormat, Channels},
    time::Rate,
};

use esp_hal::i2s::AnyI2s;

pub const SAMPLE_RATE_HZ: u32 = 16_000;

/// Speaker amplifier control (NS4150B, active HIGH).
pub struct SpeakerAmp<'d> {
    ctrl: Output<'d>,
}

impl<'d> SpeakerAmp<'d> {
    pub fn new(pin: impl Into<Output<'d>>) -> Self {
        Self { ctrl: pin.into() }
    }

    pub fn enable(&mut self)  { self.ctrl.set_high(); }
    #[allow(dead_code)]
    pub fn disable(&mut self) { self.ctrl.set_low();  }
}

/// Build the I2S TX (playback) and RX (capture) channels.
///
/// Returns `(I2sTx, I2sRx)` both in async mode, ready for DMA transfers.
///
/// Descriptors are taken at lifetime `'d` rather than `'static` so the
/// caller can reborrow session-scoped slices into the I2S (and have the
/// reborrows release on session end - see `run_session`). The original
/// backing storage is still typically static (allocated by
/// `dma_circular_buffers!`), but is reborrowed at each call site so the
/// transfer's lifetime is bounded by the session, not by the static.
pub fn build_i2s<'d>(
    i2s:          impl esp_hal::i2s::master::Instance + 'd,
    dma_channel:  impl DmaChannelFor<AnyI2s<'d>>,
    mclk:         impl PeripheralOutput<'d>,
    bclk:         impl PeripheralOutput<'d>,
    ws:           impl PeripheralOutput<'d>,
    dout:         impl PeripheralOutput<'d>,
    din:          impl PeripheralInput<'d>,
    tx_desc:      &'d mut [DmaDescriptor],
    rx_desc:      &'d mut [DmaDescriptor],
) -> (I2sTx<'d, Async>, I2sRx<'d, Async>) {
    // SAFETY: esp-hal's `.build()` requires `&'static mut [DmaDescriptor]`
    // because the DMA hardware reads descriptors as raw pointers and the
    // chain has no statically-tracked stop. In our session-scoped use,
    // both the returned `I2sTx<'d>` / `I2sRx<'d>` and the descriptors are
    // bounded by `'d` (the caller's session scope) - `run_session` drops
    // the transfers BEFORE returning, which stops the DMA and releases
    // the descriptor borrow. The descriptor storage itself is `'static`
    // (allocated by `dma_circular_buffers!` in the bin); the `'d` borrow
    // is just a Rust-side reborrow of that static storage. Extending the
    // lifetime to `'static` here is sound because the underlying memory
    // really does live `'static`; the borrow checker just can't see it
    // through esp-hal's API.
    let tx_desc_static: &'static mut [DmaDescriptor] =
        unsafe { core::mem::transmute(tx_desc) };
    let rx_desc_static: &'static mut [DmaDescriptor] =
        unsafe { core::mem::transmute(rx_desc) };

    let i2s = I2s::new(
        i2s,
        dma_channel,
        Config::new_tdm_philips()
            .with_sample_rate(Rate::from_hz(SAMPLE_RATE_HZ))
            .with_data_format(DataFormat::Data16Channel16)
            .with_channels(Channels::STEREO),
    )
    .unwrap()
    .with_mclk(mclk)
    .into_async();

    let tx = i2s.i2s_tx
        .with_bclk(bclk)
        .with_ws(ws)
        .with_dout(dout)
        .build(tx_desc_static);

    // RX shares BCLK and WS driven by TX above; only DIN is a new pin.
    let rx = i2s.i2s_rx
        .with_din(din)
        .build(rx_desc_static);

    (tx, rx)
}
