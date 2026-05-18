extern crate alloc;

use crate::audio_hal::{self, SpeakerAmp};
use drivers::es8311::Es8311;
use drivers::es7210::Es7210;
use embedded_hal::i2c::I2c;
use embassy_time::{Duration, Timer};
use esp_hal::{
    dma::{DmaChannelFor, DmaDescriptor},
    gpio::{Output, interconnect::{PeripheralInput, PeripheralOutput}},
    i2s::AnyI2s,
};

/// Owns the hardware drivers for the audio subsystem. The fields are
/// held here purely so their Drop impls don't run (which would
/// deinitialize the DAC/ADC and disable the amplifier). We don't read
/// them yet, but removing them would silently break audio output.
#[allow(dead_code)]
pub struct AudioSystem<'d> {
    pub speaker_amp: SpeakerAmp<'d>,
    pub codec: Es8311,
    pub adc_mic: Es7210,
}

/// Initialize the full audio subsystem.
///
/// Starts I2S DMA (which begins MCLK output), then configures the ES8311 DAC
/// and ES7210 ADC over I2C, enables the speaker amp, and drains stale mic data.
///
/// Returns `(AudioSystem, tx_transfer, rx_transfer)`. The DMA transfer objects
/// are returned separately because their types are too complex to store in a struct.
///
/// The `dma_circular_buffers!` macro must be called in the caller's scope and
/// the resulting buffers/descriptors passed here.
///
/// # Power rail prerequisite
///
/// The ES8311 and ES7210 analog supplies live on AXP2101 ALDO1
/// (net name `A3V3`), which is held OFF at boot by `Pmu::init` to
/// save idle current while audio is dormant. **Before calling this
/// function the caller MUST enable ALDO1 via
/// `drivers::pmu::Pmu::set_audio_rail(i2c, true)` and wait at
/// least 10 ms for the rail to stabilise.** Skipping that step will
/// cause the first codec / ADC I²C transactions here to silently
/// NAK or corrupt register writes.
///
/// `SystemManager::start_audio` already handles the rail-enable +
/// settle-delay sequence and then calls this function; prefer that
/// entry point to direct calls.
pub async fn init_audio<'d>(
    i2s: impl esp_hal::i2s::master::Instance + 'd,
    dma_ch: impl DmaChannelFor<AnyI2s<'d>>,
    mclk_pin: impl PeripheralOutput<'d>,
    bclk_pin: impl PeripheralOutput<'d>,
    ws_pin: impl PeripheralOutput<'d>,
    dout_pin: impl PeripheralOutput<'d>,
    din_pin: impl PeripheralInput<'d>,
    pa_pin: Output<'d>,
    tx_buffer: &'static mut [u8],
    rx_buffer: &'static mut [u8],
    tx_descriptors: &'static mut [DmaDescriptor],
    rx_descriptors: &'static mut [DmaDescriptor],
    i2c: &mut impl I2c,
) -> (
    AudioSystem<'d>,
    esp_hal::i2s::master::asynch::I2sWriteDmaTransferAsync<'d, &'static mut [u8]>,
    esp_hal::i2s::master::asynch::I2sReadDmaTransferAsync<'d, &'static mut [u8]>,
) {
    let mut speaker_amp = SpeakerAmp::new(pa_pin);

    let (i2s_tx, i2s_rx) = audio_hal::build_i2s(
        i2s, dma_ch, mclk_pin, bclk_pin, ws_pin, dout_pin, din_pin,
        tx_descriptors, rx_descriptors,
    );

    // Start DMA - this begins MCLK/BCLK/WS output
    let tx_transfer = i2s_tx.write_dma_circular_async(tx_buffer).unwrap();
    let mut rx_transfer = i2s_rx.read_dma_circular_async(rx_buffer).unwrap();
    log::info!("Audio: I2S DMA started");

    // Let MCLK stabilize before configuring codecs
    Timer::after(Duration::from_millis(10)).await;

    // --- ES8311 DAC ---
    log::info!("Audio: initializing ES8311...");
    let codec = Es8311::new();
    match codec.init(i2c) {
        Ok(()) => {
            match codec.read_ids(i2c) {
                Ok(ids) => log::info!("Audio: ES8311 ids [{:02X} {:02X} {:02X}]", ids[0], ids[1], ids[2]),
                Err(_) => log::warn!("Audio: ES8311 init OK but failed to read IDs"),
            }
        }
        Err(_) => log::error!("Audio: ES8311 not found at I2C address 0x{:02X}", drivers::es8311::ADDR),
    }

    // --- ES7210 ADC (three-step init with delays) ---
    log::info!("Audio: initializing ES7210...");
    let adc_mic = Es7210::new();
    match adc_mic.init(i2c) {
        Ok(()) => {
            Timer::after(Duration::from_millis(10)).await;
            match adc_mic.init_after_delay(i2c) {
                Ok(()) => {
                    Timer::after(Duration::from_millis(10)).await;
                    match adc_mic.finalize(i2c) {
                        Ok(()) => log::info!("Audio: ES7210 ready"),
                        Err(_) => log::error!("Audio: ES7210 finalize failed"),
                    }
                }
                Err(_) => log::error!("Audio: ES7210 config failed"),
            }
        }
        Err(_) => log::error!("Audio: ES7210 not found at I2C address 0x{:02X}", drivers::es7210::ADDR),
    }

    // Enable speaker amp after codecs are configured
    speaker_amp.enable();

    // Reduce volume to prevent mic feedback
    let _ = codec.set_volume(i2c, 0xAF);

    // Drain stale mic data accumulated during init
    {
        let buf_len = 16384;
        let mut drain = alloc::vec![0u8; buf_len];
        match rx_transfer.pop(&mut drain).await {
            Ok(n) => log::info!("Audio: drained {} bytes", n),
            Err(_) => log::info!("Audio: drain skipped"),
        }
    }

    log::info!("Audio: ready");

    let system = AudioSystem {
        speaker_amp,
        codec,
        adc_mic,
    };

    (system, tx_transfer, rx_transfer)
}
