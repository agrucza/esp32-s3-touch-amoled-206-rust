extern crate alloc;

use crate::audio_hal::{self, SpeakerAmp};
use crate::bus::{AudioCommand, AUDIO_COMMAND, SharedI2c};
use drivers::es8311::Es8311;
use drivers::es7210::Es7210;
use embedded_hal::i2c::I2c;
use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Instant, Timer};
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
/// This full-duplex path (speaker + mic) is not yet wired to a
/// caller - it's kept for when microphone capture lands. The live
/// alarm-tone path is the TX-only [`run_audio_task`], which performs
/// the same settle-delay + codec bring-up for the speaker alone. Note
/// ALDO1 is already enabled at boot (touch shares it), so in practice
/// the rail is on before either path runs.
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

// ---- Alarm-tone path (TX-only speaker) --------------------------------------

/// Alert-tone pitch. A square wave near 880 Hz (A5) - bright and
/// audible over ambient noise on the small NS4150-driven speaker.
const TONE_HZ: u32 = 880;

/// Square-wave amplitude (i16 full-scale is 32767). Kept well below
/// full-scale: the codec output is already attenuated and the small
/// speaker distorts near the rails.
const TONE_AMPLITUDE: i16 = 0x1800;

/// Beep cadence: `BEEP_ON_MS` of tone then `BEEP_GAP_MS` of silence,
/// repeating. Deliberately distinct from the 200/100 ms haptic buzz it
/// sounds alongside.
const BEEP_ON_MS: u64 = 400;
const BEEP_GAP_MS: u64 = 200;

/// Fill `buf` (length a multiple of 4 = stereo i16 frames) with one
/// stretch of the tone. `phase` is the running sample index, carried
/// across calls so the waveform stays continuous between DMA chunks.
/// When `audible` is false the chunk is silence (the gap in the beep).
fn fill_tone(buf: &mut [u8], phase: &mut u32, audible: bool) {
    // Samples per half-period of the square wave.
    let half_period = (audio_hal::SAMPLE_RATE_HZ / (TONE_HZ * 2)).max(1);
    let frames = buf.len() / 4;
    for i in 0..frames {
        let sample: i16 = if !audible {
            0
        } else if (*phase / half_period) % 2 == 0 {
            TONE_AMPLITUDE
        } else {
            -TONE_AMPLITUDE
        };
        *phase = phase.wrapping_add(1);
        let [lo, hi] = sample.to_le_bytes();
        let base = i * 4;
        // Same sample on both channels (mono tone on a stereo bus).
        buf[base] = lo;
        buf[base + 1] = hi;
        buf[base + 2] = lo;
        buf[base + 3] = hi;
    }
}

/// Stream the beeping tone into the circular DMA buffer until a `Stop`
/// (or a redundant re-`PlayAlarm`) arrives on [`AUDIO_COMMAND`].
/// `push_with` blocks until the DMA frees space, so this paces itself
/// to the 16 kHz sample rate with no separate timer.
async fn play_until_stop(
    tx: &mut esp_hal::i2s::master::asynch::I2sWriteDmaTransferAsync<'_, &'static mut [u8]>,
    amp: &mut SpeakerAmp<'_>,
    phase: &mut u32,
) {
    let start = Instant::now();
    loop {
        let audible =
            (start.elapsed().as_millis() % (BEEP_ON_MS + BEEP_GAP_MS)) < BEEP_ON_MS;
        let fill = tx.push_with(|buf| {
            // DMA-available length isn't guaranteed frame-aligned; fill
            // only whole frames and leave any 1-3 trailing bytes for
            // the next push.
            let frames = buf.len() / 4;
            fill_tone(&mut buf[..frames * 4], phase, audible);
            frames * 4
        });
        match select(fill, AUDIO_COMMAND.wait()).await {
            Either::First(_) => {} // chunk enqueued; keep streaming
            Either::Second(AudioCommand::Stop) => {
                amp.disable();
                return;
            }
            Either::Second(AudioCommand::PlayAlarm) => {} // already playing
        }
    }
}

/// Audio task body: owns the speaker hardware and drives the alarm /
/// timer alert tone in response to [`AUDIO_COMMAND`].
///
/// Spawned bin-side (the I2S peripheral, DMA channel and speaker pins
/// are board-specific) via a thin `#[embassy_executor::task]` wrapper
/// that hands this the board's concrete handles. The bring-up plus
/// synthesis is identical on every board, so it lives here.
///
/// Lazy bring-up: the codec / I2S stay dormant until the first
/// `PlayAlarm`, so nothing draws current while no alert sounds. After
/// the first bring-up the codec is left warm (DMA keeps running, the
/// amp is just muted) so a snooze re-fire is instant. Trade-off: that
/// warm idle draws current until the next power cycle - a teardown
/// path that silences MCLK when the alert ends is a power follow-up.
///
/// The ALDO1 analog rail the ES8311 needs is already enabled at boot
/// (the FT3168 touch controller shares it - see `Pmu::set_audio_rail`),
/// so no rail toggle happens here.
#[allow(clippy::too_many_arguments)]
pub async fn run_audio_task<'d>(
    i2c_bus: &'static SharedI2c,
    i2s: impl esp_hal::i2s::master::Instance + 'd,
    dma_ch: impl DmaChannelFor<AnyI2s<'d>>,
    mclk: impl PeripheralOutput<'d>,
    bclk: impl PeripheralOutput<'d>,
    ws: impl PeripheralOutput<'d>,
    dout: impl PeripheralOutput<'d>,
    pa_pin: Output<'static>,
    tx_buffer: &'static mut [u8],
    tx_descriptors: &'static mut [DmaDescriptor],
) -> ! {
    let mut amp = SpeakerAmp::new(pa_pin);
    amp.disable(); // start muted

    // Bring-up inputs, consumed once on the first PlayAlarm.
    let mut build = Some((i2s, dma_ch, mclk, bclk, ws, dout, tx_buffer, tx_descriptors));
    // Live transfer + codec, populated after first bring-up. `_codec`
    // is held only so its Drop doesn't deinit the DAC.
    let mut tx: Option<
        esp_hal::i2s::master::asynch::I2sWriteDmaTransferAsync<'d, &'static mut [u8]>,
    > = None;
    let mut _codec: Option<Es8311> = None;
    let mut phase: u32 = 0;

    loop {
        match AUDIO_COMMAND.wait().await {
            AudioCommand::Stop => amp.disable(),
            AudioCommand::PlayAlarm => {
                if tx.is_none() {
                    let (i2s, dma_ch, mclk, bclk, ws, dout, txbuf, txdesc) =
                        build.take().unwrap();
                    let i2s_tx = audio_hal::build_i2s_tx(
                        i2s, dma_ch, mclk, bclk, ws, dout, txdesc,
                    );
                    // Starting DMA begins MCLK/BCLK/WS; the initial
                    // buffer streams silence until we push tone.
                    let transfer = i2s_tx.write_dma_circular_async(txbuf).unwrap();
                    Timer::after(Duration::from_millis(10)).await; // MCLK settle
                    let codec = Es8311::new();
                    {
                        let mut i2c = i2c_bus.lock().await;
                        match codec.init(&mut *i2c) {
                            Ok(()) => log::info!("Audio: ES8311 ready"),
                            Err(_) => log::error!(
                                "Audio: ES8311 not found at 0x{:02X}",
                                drivers::es8311::ADDR,
                            ),
                        }
                        let _ = codec.set_volume(&mut *i2c, 0xAF);
                    }
                    tx = Some(transfer);
                    _codec = Some(codec);
                }
                amp.enable();
                if let Some(transfer) = tx.as_mut() {
                    play_until_stop(transfer, &mut amp, &mut phase).await;
                }
            }
        }
    }
}
