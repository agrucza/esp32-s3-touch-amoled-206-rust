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
/// The caller must provide static DMA descriptor slices - use `dma_descriptors!`
/// or `dma_buffers!` macros in the call site.
pub fn build_i2s<'d>(
    i2s:          impl esp_hal::i2s::master::Instance + 'd,
    dma_channel:  impl DmaChannelFor<AnyI2s<'d>>,
    mclk:         impl PeripheralOutput<'d>,
    bclk:         impl PeripheralOutput<'d>,
    ws:           impl PeripheralOutput<'d>,
    dout:         impl PeripheralOutput<'d>,
    din:          impl PeripheralInput<'d>,
    tx_desc:      &'static mut [DmaDescriptor],
    rx_desc:      &'static mut [DmaDescriptor],
) -> (I2sTx<'d, Async>, I2sRx<'d, Async>) {
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
        .build(tx_desc);

    // RX shares BCLK and WS driven by TX above; only DIN is a new pin.
    let rx = i2s.i2s_rx
        .with_din(din)
        .build(rx_desc);

    (tx, rx)
}

/// Build only the I2S TX (playback) channel - the speaker path.
///
/// Same master config as [`build_i2s`] but without the RX/mic side.
/// Used by the alarm-tone path, where capture isn't needed: it claims
/// one fewer GPIO (no DSDOUT/`din`) and one fewer descriptor set. The
/// full-duplex [`build_i2s`] is retained for when microphone capture
/// lands.
pub fn build_i2s_tx<'d>(
    i2s:          impl esp_hal::i2s::master::Instance + 'd,
    dma_channel:  impl DmaChannelFor<AnyI2s<'d>>,
    mclk:         impl PeripheralOutput<'d>,
    bclk:         impl PeripheralOutput<'d>,
    ws:           impl PeripheralOutput<'d>,
    dout:         impl PeripheralOutput<'d>,
    tx_desc:      &'static mut [DmaDescriptor],
) -> I2sTx<'d, Async> {
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

    i2s.i2s_tx
        .with_bclk(bclk)
        .with_ws(ws)
        .with_dout(dout)
        .build(tx_desc)
}
