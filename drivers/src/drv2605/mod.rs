//! TI DRV2605 haptic driver (I2C, ERM/LRA motor controller).
//!
//! Works with any I2C implementation that satisfies the `embedded-hal`
//! traits. Register map and procedures follow the TI datasheet
//! (SLOS825E); register/field names below match its Table 3 so the
//! code can be checked against the document line by line. The init
//! sequence additionally mirrors the vendor firmware proven on this
//! hardware (ERM open loop, ROM library A).
//!
//! Two usage modes are exposed:
//! - **RTP (real-time playback)** - continuous vibration with direct
//!   amplitude control; maps 1:1 onto the system's `buzz`/`buzz_stop`
//!   semantics (the model pulses the on/off pattern itself).
//! - **ROM library effects** - one-shot waveforms (clicks, ramps,
//!   alerts; IDs 1-123) via the 8-slot sequencer, for future
//!   tap/notification feedback.
//!
//! The EN pin is not managed here (on the T-Watch Ultra it hangs off
//! the XL9555 expander and is raised at board init). Idle power is
//! handled with the MODE register's STANDBY bit instead - [`buzz_off`]
//! re-enters software standby after every stop.
//!
//! The IN/TRIG-pin input modes (PWM, analog, audio-to-vibe, external
//! triggers) are configurable here for board portability, but the
//! T-Watch Ultra does not route IN/TRIG anywhere - on that board they
//! are dead config.
//!
//! Deliberately NOT implemented: OTP programming (CONTROL4
//! OTP_PROGRAM). It permanently burns registers 0x16-0x1A exactly
//! once per chip and requires VDD in the 4.0-4.4 V window - the
//! T-Watch runs the chip at 3.3 V, and an irreversible one-shot has
//! no place behind a casual driver call.
//! [`restore_calibration`](Drv2605::restore_calibration) covers the
//! persistence use case reversibly.
//!
//! [`buzz_off`]: Drv2605::buzz_off

use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::I2c as I2cTrait;

/// Fixed I2C address (datasheet 7.5.3.2; the DRV2605 has no address
/// straps). 0x58 is the TI haptic-broadcast address - unused here.
pub const DEFAULT_ADDRESS: u8 = 0x5A;

/// Full-scale RTP amplitude in the power-on signed data format
/// (datasheet Figure 24: 0x7F = 100% forward drive).
pub const RTP_MAX: u8 = 0x7F;

/// Register addresses and fields, named per datasheet Table 3.
mod reg {
    pub const STATUS:        u8 = 0x00; // DEVICE_ID[7:5], DIAG_RESULT[3], FB_STS[2], OVER_TEMP[1], OC_DETECT[0]
    pub const MODE:          u8 = 0x01; // DEV_RESET[7], STANDBY[6], MODE[2:0]
    pub const RTP_INPUT:     u8 = 0x02;
    pub const LIBRARY_SEL:   u8 = 0x03; // HI_Z[4], LIBRARY_SEL[2:0]
    // Waveform sequencer slots. WAIT[7]: value = pause of
    // WAV_FRM_SEQ[6:0] x 10 ms instead of an effect ID.
    pub const WAV_FRM_SEQ1:  u8 = 0x04;
    pub const WAV_FRM_SEQ2:  u8 = 0x05;
    pub const WAV_FRM_SEQ3:  u8 = 0x06;
    pub const WAV_FRM_SEQ4:  u8 = 0x07;
    pub const WAV_FRM_SEQ5:  u8 = 0x08;
    pub const WAV_FRM_SEQ6:  u8 = 0x09;
    pub const WAV_FRM_SEQ7:  u8 = 0x0A;
    pub const WAV_FRM_SEQ8:  u8 = 0x0B;
    /// The sequencer slots in playback order, for slot-indexed access.
    pub const WAV_FRM_SEQ: [u8; 8] = [
        WAV_FRM_SEQ1, WAV_FRM_SEQ2, WAV_FRM_SEQ3, WAV_FRM_SEQ4,
        WAV_FRM_SEQ5, WAV_FRM_SEQ6, WAV_FRM_SEQ7, WAV_FRM_SEQ8,
    ];
    pub const GO:            u8 = 0x0C;
    pub const ODT:           u8 = 0x0D; // overdrive time offset, 5 ms/LSB, 2s complement
    pub const SPT:           u8 = 0x0E; // sustain time offset positive
    pub const SNT:           u8 = 0x0F; // sustain time offset negative
    pub const BRT:           u8 = 0x10; // brake time offset
    pub const ATH_CTRL:      u8 = 0x11; // ATH_PEAK_TIME[3:2], ATH_FILTER[1:0]
    pub const ATH_MIN_INPUT: u8 = 0x12; // audio-to-vibe minimum input level
    pub const ATH_MAX_INPUT: u8 = 0x13; // audio-to-vibe full-scale input
    pub const ATH_MIN_DRIVE: u8 = 0x14; // audio-to-vibe minimum output drive
    pub const ATH_MAX_DRIVE: u8 = 0x15; // audio-to-vibe maximum output drive
    pub const RATED_VOLTAGE: u8 = 0x16; // closed-loop full-scale reference (default 0x3F)
    pub const OD_CLAMP:      u8 = 0x17; // overdrive clamp / open-loop full-scale ref (default 0x89)
    pub const A_CAL_COMP:    u8 = 0x18; // auto-cal compensation result
    pub const A_CAL_BEMF:    u8 = 0x19; // auto-cal back-EMF result
    pub const FEEDBACK_CTRL: u8 = 0x1A; // N_ERM_LRA[7], FB_BRAKE_FACTOR[6:4], LOOP_GAIN[3:2], BEMF_GAIN[1:0]
    pub const CONTROL1:      u8 = 0x1B; // STARTUP_BOOST[7], AC_COUPLE[5], DRIVE_TIME[4:0]
    pub const CONTROL2:      u8 = 0x1C; // BIDIR_INPUT[7], BRAKE_STABILIZER[6], SAMPLE_TIME[5:4],
                                        // BLANKING_TIME[3:2], IDISS_TIME[1:0]
    pub const CONTROL3:      u8 = 0x1D; // NG_THRESH[7:6], ERM_OPEN_LOOP[5], SUPPLY_COMP_DIS[4],
                                        // DATA_FORMAT_RTP[3], LRA_DRIVE_MODE[2], N_PWM_ANALOG[1], LRA_OPEN_LOOP[0]
    pub const CONTROL4:      u8 = 0x1E; // AUTO_CAL_TIME[5:4], OTP_STATUS[2], OTP_PROGRAM[0]
    pub const VBAT:          u8 = 0x21; // VDD = VBAT x 5.6 V / 255
    pub const LRA_PERIOD:    u8 = 0x22; // period = LRA_PERIOD x 98.46 us
}

/// MODE[2:0] values (plus the STANDBY bit pattern used for idling).
mod mode {
    pub const INTERNAL_TRIGGER: u8 = 0x00;
    pub const RTP:              u8 = 0x05;
    pub const DIAGNOSTICS:      u8 = 0x06;
    pub const AUTO_CAL:         u8 = 0x07;
    /// STANDBY bit set, MODE[2:0] = 0.
    pub const STANDBY:          u8 = 0x40;
}

/// STATUS register flag bits (datasheet Table 4). OVER_TEMP,
/// OC_DETECT and DIAG_RESULT are latching and clear on read.
pub mod status {
    pub const DIAG_RESULT: u8 = 0x08;
    pub const FB_STS:      u8 = 0x04;
    pub const OVER_TEMP:   u8 = 0x02;
    pub const OC_DETECT:   u8 = 0x01;
}

/// Driver error.
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<E> {
    /// An I2C transaction failed; the inner value is the HAL's own error.
    I2c(E),
    /// The STATUS DEVICE_ID field is not a DRV260x family member.
    DeviceNotFound,
}

/// Motor type behind the driver output (datasheet 7.5.4/7.5.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actuator {
    /// Eccentric rotating mass, open loop (this watch's motor).
    ErmOpenLoop,
    /// ERM with back-EMF closed-loop control (needs auto-calibration).
    ErmClosedLoop,
    /// Linear resonant actuator, auto-resonance closed loop (needs
    /// auto-calibration).
    Lra,
    /// LRA driven open loop at the selected frequency.
    LraOpenLoop,
}

/// One slot of the 8-deep waveform sequencer.
#[derive(Debug, Clone, Copy)]
pub enum SequenceStep {
    /// Play a ROM library effect (IDs 1-123).
    Effect(u8),
    /// Pause for `n * 10 ms` (n clamped to 127).
    WaitMs10(u8),
}

/// Inputs the auto-calibration engine requires (datasheet 7.5.6).
/// Compute the voltage register values per datasheet 7.5.2; the
/// remaining engine inputs use TI's "valid for most actuators"
/// recommendations (FB_BRAKE_FACTOR = 2, LOOP_GAIN = 2,
/// AUTO_CAL_TIME = 3).
#[derive(Debug, Clone, Copy)]
pub struct AutoCalInputs {
    /// RATED_VOLTAGE register value (closed-loop full-scale).
    pub rated_voltage: u8,
    /// OD_CLAMP register value (overdrive bound).
    pub od_clamp: u8,
    /// DRIVE_TIME[4:0] (datasheet 7.5.6 step 3g / Table 24: LRA
    /// wants ~0.5 x resonance period; ERM sets the back-EMF sample
    /// rate). `None` keeps the current register value - fine for
    /// most ERMs, compute it for LRA calibration.
    pub drive_time: Option<u8>,
}

/// Auto-calibration output (datasheet Figure 23). Persist and restore
/// via [`Drv2605::restore_calibration`] to skip the audible
/// calibration buzz on later boots (the chip loses these in hardware
/// shutdown only if OTP is unprogrammed; rewriting them at boot is
/// the datasheet-recommended flow).
#[derive(Debug, Clone, Copy)]
pub struct Calibration {
    /// A_CAL_COMP - drive-gain compensation for resistive losses.
    pub compensation: u8,
    /// A_CAL_BEMF - back-EMF level at rated voltage.
    pub back_emf: u8,
    /// BEMF_GAIN field (FEEDBACK_CTRL[1:0]) chosen by the engine.
    pub bemf_gain: u8,
}

/// DRV2605 driver.
///
/// Holds only the I2C address. The I2C bus is passed by mutable
/// reference on every call so it can be freely shared with other
/// peripherals on the same bus.
pub struct Drv2605 {
    addr: u8,
}

/// Driver configuration.
pub struct Config {
    /// I2C device address (default: [`DEFAULT_ADDRESS`]).
    pub address: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self { address: DEFAULT_ADDRESS }
    }
}

impl Drv2605 {
    /// Create a new driver instance.
    pub fn new(config: Config) -> Self {
        Self { addr: config.address }
    }

    /// Verify the device and configure it for an ERM motor in open
    /// loop with ROM library A - the vendor-proven sequence for this
    /// board's motor. Leaves the chip in software standby;
    /// [`buzz_on`] and [`play_effect`] wake it per use.
    ///
    /// Returns DEVICE_ID (datasheet Table 4: 3 = DRV2605,
    /// 4 = DRV2604, 6 = DRV2604L, 7 = DRV2605L).
    ///
    /// [`buzz_on`]: Drv2605::buzz_on
    /// [`play_effect`]: Drv2605::play_effect
    pub fn init<I2C, E>(&self, i2c: &mut I2C) -> Result<u8, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let device_id = self.read_register(i2c, reg::STATUS)? >> 5;
        if !matches!(device_id, 3 | 4 | 6 | 7) {
            return Err(Error::DeviceNotFound);
        }

        self.write_register(i2c, reg::MODE, mode::INTERNAL_TRIGGER)?; // clear STANDBY
        self.write_register(i2c, reg::RTP_INPUT, 0x00)?;
        self.write_register(i2c, reg::WAV_FRM_SEQ1, 1)?;
        self.write_register(i2c, reg::WAV_FRM_SEQ2, 0)?;
        self.write_register(i2c, reg::ODT, 0)?;
        self.write_register(i2c, reg::SPT, 0)?;
        self.write_register(i2c, reg::SNT, 0)?;
        self.write_register(i2c, reg::BRT, 0)?;
        self.write_register(i2c, reg::ATH_MAX_INPUT, 0x64)?;

        self.set_actuator(i2c, Actuator::ErmOpenLoop)?;
        self.select_library(i2c, 1)?;

        // Idle in standby until first use.
        self.write_register(i2c, reg::MODE, mode::STANDBY)?;

        Ok(device_id)
    }

    /// Start continuous vibration at `amplitude` (0..=[`RTP_MAX`];
    /// values above signed full scale are clamped). Wakes the chip
    /// into RTP mode. Runs until [`buzz_off`](Drv2605::buzz_off).
    pub fn buzz_on<I2C, E>(&self, i2c: &mut I2C, amplitude: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, reg::MODE, mode::RTP)?;
        self.write_register(i2c, reg::RTP_INPUT, amplitude.min(RTP_MAX))
    }

    /// Stop vibration and drop back into software standby.
    pub fn buzz_off<I2C, E>(&self, i2c: &mut I2C) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, reg::RTP_INPUT, 0x00)?;
        self.write_register(i2c, reg::MODE, mode::STANDBY)
    }

    /// Fire a one-shot ROM library effect (IDs 1-123; e.g. 1 = strong
    /// click). Returns immediately - the waveform plays out in
    /// hardware.
    pub fn play_effect<I2C, E>(&self, i2c: &mut I2C, effect: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.play_sequence(i2c, &[SequenceStep::Effect(effect)])
    }

    /// Program the waveform sequencer and trigger it: effects and
    /// 10 ms-granular pauses, e.g. double-click = `[Effect(1),
    /// WaitMs10(10), Effect(1)]`. Longer inputs are truncated to the
    /// hardware's 8 slots. Playback stops at the first zero slot or
    /// after slot 8. Returns immediately; poll
    /// [`is_playing`](Drv2605::is_playing) or fire and forget.
    pub fn play_sequence<I2C, E>(
        &self,
        i2c: &mut I2C,
        steps: &[SequenceStep],
    ) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, reg::MODE, mode::INTERNAL_TRIGGER)?;
        let mut slot = 0usize;
        for step in steps.iter().take(reg::WAV_FRM_SEQ.len()) {
            let value = match step {
                SequenceStep::Effect(id) => *id & 0x7F,
                SequenceStep::WaitMs10(n) => 0x80 | (*n & 0x7F),
            };
            self.write_register(i2c, reg::WAV_FRM_SEQ[slot], value)?;
            slot += 1;
        }
        if slot < reg::WAV_FRM_SEQ.len() {
            // Terminate the sequence.
            self.write_register(i2c, reg::WAV_FRM_SEQ[slot], 0)?;
        }
        self.write_register(i2c, reg::GO, 1)
    }

    /// `true` while the sequencer is still playing (GO self-clears
    /// when the sequence completes).
    pub fn is_playing<I2C, E>(&self, i2c: &mut I2C) -> Result<bool, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        Ok(self.read_register(i2c, reg::GO)? & 0x01 != 0)
    }

    /// Abort a playing sequence (clearing GO during playback cancels
    /// the waveform, datasheet Table 9).
    pub fn stop<I2C, E>(&self, i2c: &mut I2C) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, reg::GO, 0)
    }

    // ---- Configuration beyond the boot defaults ------------------------------

    /// Select the actuator type and matching loop mode (datasheet
    /// 7.5.4: N_ERM_LRA in FEEDBACK_CTRL + the per-type open-loop bit
    /// in CONTROL3). `init` selects ERM open loop; closed-loop modes
    /// need an [`auto_calibrate`](Drv2605::auto_calibrate) pass (or
    /// restored results) to track properly.
    pub fn set_actuator<I2C, E>(&self, i2c: &mut I2C, actuator: Actuator) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let feedback = self.read_register(i2c, reg::FEEDBACK_CTRL)?;
        let control3 = self.read_register(i2c, reg::CONTROL3)?;
        let (fb, c3) = match actuator {
            Actuator::ErmOpenLoop => (feedback & 0x7F, control3 | 0x20),
            Actuator::ErmClosedLoop => (feedback & 0x7F, control3 & !0x20),
            Actuator::Lra => (feedback | 0x80, control3 & !0x01),
            Actuator::LraOpenLoop => (feedback | 0x80, control3 | 0x01),
        };
        self.write_register(i2c, reg::FEEDBACK_CTRL, fb)?;
        self.write_register(i2c, reg::CONTROL3, c3)
    }

    /// Select the ROM effect library (datasheet Table 7: 1-5 =
    /// TS2200 libraries A-E for ERM, 6 = LRA library; 0 = empty).
    /// Preserves the HI_Z bit sharing this register.
    pub fn select_library<I2C, E>(&self, i2c: &mut I2C, library: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let cur = self.read_register(i2c, reg::LIBRARY_SEL)?;
        self.write_register(i2c, reg::LIBRARY_SEL, (cur & !0x07) | (library & 0x07))
    }

    /// Put the output driver into (or out of) true high-impedance
    /// (LIBRARY_SEL register HI_Z bit).
    pub fn set_hi_z<I2C, E>(&self, i2c: &mut I2C, hi_z: bool) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let cur = self.read_register(i2c, reg::LIBRARY_SEL)?;
        let new = if hi_z { cur | 0x10 } else { cur & !0x10 };
        self.write_register(i2c, reg::LIBRARY_SEL, new)
    }

    /// RATED_VOLTAGE register - closed-loop full-scale reference.
    /// Compute per datasheet 7.5.2.1.
    pub fn set_rated_voltage<I2C, E>(&self, i2c: &mut I2C, value: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, reg::RATED_VOLTAGE, value)
    }

    /// OD_CLAMP register - overdrive bound (closed loop) / full-scale
    /// reference (open loop). Compute per datasheet 7.5.2.2.
    pub fn set_overdrive_clamp<I2C, E>(&self, i2c: &mut I2C, value: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, reg::OD_CLAMP, value)
    }

    /// Switch RTP_INPUT interpretation between the power-on signed
    /// format (0x7F full scale) and unsigned (0xFF full scale) -
    /// CONTROL3 DATA_FORMAT_RTP. Unsigned is the datasheet
    /// recommendation for closed-loop unidirectional mode.
    pub fn set_rtp_unsigned<I2C, E>(&self, i2c: &mut I2C, unsigned: bool) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let control3 = self.read_register(i2c, reg::CONTROL3)?;
        let new = if unsigned { control3 | 0x08 } else { control3 & !0x08 };
        self.write_register(i2c, reg::CONTROL3, new)
    }

    /// Explicit software-standby control (init/buzz_off already
    /// manage this for the common paths).
    pub fn set_standby<I2C, E>(&self, i2c: &mut I2C, standby: bool) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let value = if standby { mode::STANDBY } else { mode::INTERNAL_TRIGGER };
        self.write_register(i2c, reg::MODE, value)
    }

    /// Software device reset (MODE DEV_RESET): equivalent to a power
    /// cycle, all registers return to defaults. Blocks until the
    /// self-clearing bit drops (or ~100 ms timeout - `false` means
    /// the reset never confirmed). Re-run [`init`](Drv2605::init)
    /// afterwards.
    pub fn reset<I2C, E, D>(&self, i2c: &mut I2C, delay: &mut D) -> Result<bool, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
        D: DelayNs,
    {
        self.write_register(i2c, reg::MODE, 0x80)?;
        let mut remaining_ms = 100u32;
        while self.read_register(i2c, reg::MODE)? & 0x80 != 0 {
            if remaining_ms == 0 {
                return Ok(false);
            }
            delay.delay_ms(5);
            remaining_ms = remaining_ms.saturating_sub(5);
        }
        Ok(true)
    }

    // ---- Library-waveform time offsets (datasheet 7.6.7-7.6.10) --------------
    // All four are 5 ms/LSB, two's complement, applied to the ROM
    // library waveforms in open-loop mode.

    /// ODT - overdrive portion time offset.
    pub fn set_overdrive_time_offset<I2C, E>(&self, i2c: &mut I2C, value: i8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, reg::ODT, value as u8)
    }

    /// SPT - positive sustain portion time offset.
    pub fn set_sustain_time_offset_positive<I2C, E>(&self, i2c: &mut I2C, value: i8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, reg::SPT, value as u8)
    }

    /// SNT - negative sustain portion time offset.
    pub fn set_sustain_time_offset_negative<I2C, E>(&self, i2c: &mut I2C, value: i8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, reg::SNT, value as u8)
    }

    /// BRT - braking portion time offset.
    pub fn set_brake_time_offset<I2C, E>(&self, i2c: &mut I2C, value: i8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, reg::BRT, value as u8)
    }

    // ---- Loop / drive tuning (datasheet 7.6.20-7.6.23) ------------------------

    /// DRIVE_TIME[4:0] (CONTROL1). LRA: initial drive-time guess,
    /// (value x 0.1 ms + 0.5 ms) ~ half the resonance period. ERM:
    /// back-EMF sample rate, (value x 0.2 ms + 1 ms).
    pub fn set_drive_time<I2C, E>(&self, i2c: &mut I2C, value: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let control1 = self.read_register(i2c, reg::CONTROL1)?;
        self.write_register(i2c, reg::CONTROL1, (control1 & !0x1F) | (value & 0x1F))
    }

    /// STARTUP_BOOST (CONTROL1): higher loop gain during overdrive
    /// for faster actuator start. On by default.
    pub fn set_startup_boost<I2C, E>(&self, i2c: &mut I2C, enabled: bool) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let control1 = self.read_register(i2c, reg::CONTROL1)?;
        let new = if enabled { control1 | 0x80 } else { control1 & !0x80 };
        self.write_register(i2c, reg::CONTROL1, new)
    }

    /// FB_BRAKE_FACTOR[2:0] (FEEDBACK_CTRL): braking-vs-driving gain
    /// ratio, 0-6 = 1x..16x, 7 = braking disabled.
    pub fn set_fb_brake_factor<I2C, E>(&self, i2c: &mut I2C, value: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let feedback = self.read_register(i2c, reg::FEEDBACK_CTRL)?;
        self.write_register(i2c, reg::FEEDBACK_CTRL, (feedback & !0x70) | ((value & 0x07) << 4))
    }

    /// LOOP_GAIN[1:0] (FEEDBACK_CTRL): 0 = low .. 3 = very high.
    pub fn set_loop_gain<I2C, E>(&self, i2c: &mut I2C, value: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let feedback = self.read_register(i2c, reg::FEEDBACK_CTRL)?;
        self.write_register(i2c, reg::FEEDBACK_CTRL, (feedback & !0x0C) | ((value & 0x03) << 2))
    }

    /// BIDIR_INPUT (CONTROL2): bidirectional (default, open-loop
    /// compatible) vs unidirectional input interpretation (closed
    /// loop only, one extra bit of resolution).
    pub fn set_bidir_input<I2C, E>(&self, i2c: &mut I2C, bidirectional: bool) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let control2 = self.read_register(i2c, reg::CONTROL2)?;
        let new = if bidirectional { control2 | 0x80 } else { control2 & !0x80 };
        self.write_register(i2c, reg::CONTROL2, new)
    }

    /// BRAKE_STABILIZER (CONTROL2): reduce loop gain when braking is
    /// nearly complete, for stability. On by default.
    pub fn set_brake_stabilizer<I2C, E>(&self, i2c: &mut I2C, enabled: bool) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let control2 = self.read_register(i2c, reg::CONTROL2)?;
        let new = if enabled { control2 | 0x40 } else { control2 & !0x40 };
        self.write_register(i2c, reg::CONTROL2, new)
    }

    /// SAMPLE_TIME[1:0] (CONTROL2): LRA auto-resonance sampling time,
    /// 0-3 = 150/200/250/300 us. Advanced use.
    pub fn set_sample_time<I2C, E>(&self, i2c: &mut I2C, value: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let control2 = self.read_register(i2c, reg::CONTROL2)?;
        self.write_register(i2c, reg::CONTROL2, (control2 & !0x30) | ((value & 0x03) << 4))
    }

    /// BLANKING_TIME[1:0] (CONTROL2): pre-conversion back-EMF
    /// blanking. Advanced use.
    pub fn set_blanking_time<I2C, E>(&self, i2c: &mut I2C, value: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let control2 = self.read_register(i2c, reg::CONTROL2)?;
        self.write_register(i2c, reg::CONTROL2, (control2 & !0x0C) | ((value & 0x03) << 2))
    }

    /// IDISS_TIME[1:0] (CONTROL2): flyback current-dissipation time.
    /// Advanced use.
    pub fn set_idiss_time<I2C, E>(&self, i2c: &mut I2C, value: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let control2 = self.read_register(i2c, reg::CONTROL2)?;
        self.write_register(i2c, reg::CONTROL2, (control2 & !0x03) | (value & 0x03))
    }

    /// NG_THRESH[1:0] (CONTROL3): PWM/analog noise-gate threshold,
    /// 0 = disabled, 1-3 = 2/4/8 %.
    pub fn set_noise_gate_threshold<I2C, E>(&self, i2c: &mut I2C, value: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let control3 = self.read_register(i2c, reg::CONTROL3)?;
        self.write_register(i2c, reg::CONTROL3, (control3 & !0xC0) | ((value & 0x03) << 6))
    }

    /// SUPPLY_COMP_DIS (CONTROL3), inverted to a positive API:
    /// `enabled = true` keeps the chip's constant-drive-vs-VDD
    /// compensation active (the default).
    pub fn set_supply_compensation<I2C, E>(&self, i2c: &mut I2C, enabled: bool) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let control3 = self.read_register(i2c, reg::CONTROL3)?;
        let new = if enabled { control3 & !0x10 } else { control3 | 0x10 };
        self.write_register(i2c, reg::CONTROL3, new)
    }

    /// LRA_DRIVE_MODE (CONTROL3): drive-amplitude update cadence,
    /// once (default) or twice per LRA cycle.
    pub fn set_lra_drive_twice_per_cycle<I2C, E>(&self, i2c: &mut I2C, twice: bool) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let control3 = self.read_register(i2c, reg::CONTROL3)?;
        let new = if twice { control3 | 0x04 } else { control3 & !0x04 };
        self.write_register(i2c, reg::CONTROL3, new)
    }

    // ---- IN/TRIG input modes (datasheet 7.5.8.2.1-7.5.8.2.6) ------------------
    // NOTE: dead config on the T-Watch Ultra - IN/TRIG is not routed
    // there. Provided for boards that wire the pin.

    /// External trigger: IN/TRIG edge fires the sequencer (MODE 1)
    /// or level-follows the GO bit (MODE 2).
    pub fn set_mode_external_trigger<I2C, E>(&self, i2c: &mut I2C, level: bool) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, reg::MODE, if level { 0x02 } else { 0x01 })
    }

    /// PWM drive from IN/TRIG (MODE 3, N_PWM_ANALOG = 0).
    pub fn set_mode_pwm_input<I2C, E>(&self, i2c: &mut I2C) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let control3 = self.read_register(i2c, reg::CONTROL3)?;
        self.write_register(i2c, reg::CONTROL3, control3 & !0x02)?;
        self.write_register(i2c, reg::MODE, 0x03)
    }

    /// Analog-voltage drive from IN/TRIG (MODE 3, N_PWM_ANALOG = 1;
    /// 1.8 V full scale).
    pub fn set_mode_analog_input<I2C, E>(&self, i2c: &mut I2C) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let control3 = self.read_register(i2c, reg::CONTROL3)?;
        self.write_register(i2c, reg::CONTROL3, control3 | 0x02)?;
        self.write_register(i2c, reg::MODE, 0x03)
    }

    /// Audio-to-vibe from an AC-coupled source on IN/TRIG (MODE 4 +
    /// AC_COUPLE + N_PWM_ANALOG, datasheet 7.5.8.2.4). Tune the
    /// conversion window via
    /// [`set_audio_to_vibe_levels`](Drv2605::set_audio_to_vibe_levels).
    pub fn set_mode_audio_to_vibe<I2C, E>(&self, i2c: &mut I2C) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let control1 = self.read_register(i2c, reg::CONTROL1)?;
        self.write_register(i2c, reg::CONTROL1, control1 | 0x20)?; // AC_COUPLE
        let control3 = self.read_register(i2c, reg::CONTROL3)?;
        self.write_register(i2c, reg::CONTROL3, control3 | 0x02)?; // N_PWM_ANALOG
        self.write_register(i2c, reg::MODE, 0x04)
    }

    /// Audio-to-vibe input window and output drive range
    /// (ATH_MIN/MAX_INPUT: x 1.8 V / 255; ATH_MIN/MAX_DRIVE: % of
    /// full scale x 255 / 100).
    pub fn set_audio_to_vibe_levels<I2C, E>(
        &self,
        i2c: &mut I2C,
        min_input: u8,
        max_input: u8,
        min_drive: u8,
        max_drive: u8,
    ) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, reg::ATH_MIN_INPUT, min_input)?;
        self.write_register(i2c, reg::ATH_MAX_INPUT, max_input)?;
        self.write_register(i2c, reg::ATH_MIN_DRIVE, min_drive)?;
        self.write_register(i2c, reg::ATH_MAX_DRIVE, max_drive)
    }

    /// Audio-to-vibe peak-detect time (0-3 = 10/20/30/40 ms) and
    /// low-pass filter (0-3 = 100/125/150/200 Hz).
    pub fn set_audio_to_vibe_ctrl<I2C, E>(
        &self,
        i2c: &mut I2C,
        peak_time: u8,
        filter: u8,
    ) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(
            i2c,
            reg::ATH_CTRL,
            ((peak_time & 0x03) << 2) | (filter & 0x03),
        )
    }

    // ---- Calibration / diagnostics / telemetry --------------------------------

    /// Run hardware auto-calibration per datasheet 7.5.6 (audibly
    /// buzzes the motor; up to ~1.2 s at the AUTO_CAL_TIME=3 setting
    /// used here). Set the actuator type first; rated/clamp voltages
    /// come from `inputs`, and the engine's remaining knobs use TI's
    /// most-actuators recommendations (FB_BRAKE_FACTOR=2, LOOP_GAIN=2,
    /// AUTO_CAL_TIME=3).
    ///
    /// Returns `Ok(Some(_))` with the results on convergence -
    /// persist them and apply via
    /// [`restore_calibration`](Drv2605::restore_calibration) on later
    /// boots to skip the buzz. `Ok(None)` = the routine ran but did
    /// not converge (DIAG_RESULT set) or timed out.
    pub fn auto_calibrate<I2C, E, D>(
        &self,
        i2c: &mut I2C,
        delay: &mut D,
        inputs: &AutoCalInputs,
    ) -> Result<Option<Calibration>, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
        D: DelayNs,
    {
        // Engine inputs (7.5.6 step 3). FB_BRAKE_FACTOR=2, LOOP_GAIN=2
        // via read-modify-write preserving N_ERM_LRA and BEMF_GAIN;
        // AUTO_CAL_TIME=3 preserving the CONTROL4 OTP bits.
        let feedback = self.read_register(i2c, reg::FEEDBACK_CTRL)?;
        self.write_register(i2c, reg::FEEDBACK_CTRL, (feedback & 0x83) | (2 << 4) | (2 << 2))?;
        self.write_register(i2c, reg::RATED_VOLTAGE, inputs.rated_voltage)?;
        self.write_register(i2c, reg::OD_CLAMP, inputs.od_clamp)?;
        let control4 = self.read_register(i2c, reg::CONTROL4)?;
        self.write_register(i2c, reg::CONTROL4, (control4 & !0x30) | (3 << 4))?;
        if let Some(dt) = inputs.drive_time {
            self.set_drive_time(i2c, dt)?;
        }

        self.write_register(i2c, reg::MODE, mode::AUTO_CAL)?;
        self.write_register(i2c, reg::GO, 1)?;
        // AUTO_CAL_TIME=3 bounds the run at 1000-1200 ms.
        let mut remaining_ms = 1500u32;
        while self.read_register(i2c, reg::GO)? & 0x01 != 0 {
            if remaining_ms == 0 {
                self.write_register(i2c, reg::MODE, mode::STANDBY)?;
                return Ok(None);
            }
            delay.delay_ms(20);
            remaining_ms = remaining_ms.saturating_sub(20);
        }
        // DIAG_RESULT: 0 = converged. Latching, clears on this read.
        let converged = self.read_register(i2c, reg::STATUS)? & status::DIAG_RESULT == 0;
        let result = if converged {
            Some(Calibration {
                compensation: self.read_register(i2c, reg::A_CAL_COMP)?,
                back_emf: self.read_register(i2c, reg::A_CAL_BEMF)?,
                bemf_gain: self.read_register(i2c, reg::FEEDBACK_CTRL)? & 0x03,
            })
        } else {
            None
        };
        self.write_register(i2c, reg::MODE, mode::STANDBY)?;
        Ok(result)
    }

    /// Apply previously saved auto-calibration results (datasheet
    /// 7.5.6 step 6b: rewrite on subsequent power-ups).
    pub fn restore_calibration<I2C, E>(
        &self,
        i2c: &mut I2C,
        cal: &Calibration,
    ) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, reg::A_CAL_COMP, cal.compensation)?;
        self.write_register(i2c, reg::A_CAL_BEMF, cal.back_emf)?;
        let feedback = self.read_register(i2c, reg::FEEDBACK_CTRL)?;
        self.write_register(
            i2c,
            reg::FEEDBACK_CTRL,
            (feedback & !0x03) | (cal.bemf_gain & 0x03),
        )
    }

    /// Run the hardware diagnostic (briefly actuates the motor).
    /// Returns `Ok(true)` when the actuator passes - present, not
    /// shorted, back-EMF in range (datasheet Table 4, diagnostic
    /// mode).
    pub fn run_diagnostics<I2C, E, D>(
        &self,
        i2c: &mut I2C,
        delay: &mut D,
    ) -> Result<bool, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
        D: DelayNs,
    {
        self.write_register(i2c, reg::MODE, mode::DIAGNOSTICS)?;
        self.write_register(i2c, reg::GO, 1)?;
        let mut remaining_ms = 1000u32;
        while self.read_register(i2c, reg::GO)? & 0x01 != 0 {
            if remaining_ms == 0 {
                self.write_register(i2c, reg::MODE, mode::STANDBY)?;
                return Ok(false);
            }
            delay.delay_ms(10);
            remaining_ms = remaining_ms.saturating_sub(10);
        }
        let passed = self.read_register(i2c, reg::STATUS)? & status::DIAG_RESULT == 0;
        self.write_register(i2c, reg::MODE, mode::STANDBY)?;
        Ok(passed)
    }

    /// Raw STATUS byte (see the [`status`] bit constants). Note the
    /// latching flags (over-temp, over-current, diag result) clear
    /// on read.
    pub fn read_status<I2C, E>(&self, i2c: &mut I2C) -> Result<u8, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.read_register(i2c, reg::STATUS)
    }

    /// Supply voltage at the chip in millivolts (datasheet Table 28:
    /// VDD = VBAT x 5.6 V / 255). Only valid while actively driving
    /// a waveform.
    pub fn vbat_mv<I2C, E>(&self, i2c: &mut I2C) -> Result<u16, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let raw = self.read_register(i2c, reg::VBAT)? as u32;
        Ok((raw * 5600 / 255) as u16)
    }

    /// Measured LRA resonance frequency in Hz (datasheet Table 29:
    /// period = LRA_PERIOD x 98.46 us). Only valid while actively
    /// driving an LRA; returns 0 when unmeasured.
    pub fn lra_resonance_hz<I2C, E>(&self, i2c: &mut I2C) -> Result<u16, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let period = self.read_register(i2c, reg::LRA_PERIOD)? as u32;
        if period == 0 {
            return Ok(0);
        }
        // 1 / 98.46 us = 10157 Hz at period = 1.
        Ok((10_157 / period) as u16)
    }

    // ---- Register access ----------------------------------------------------

    fn read_register<I2C, E>(&self, i2c: &mut I2C, register: u8) -> Result<u8, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let mut buf = [0u8; 1];
        i2c.write_read(self.addr, &[register], &mut buf)
            .map_err(Error::I2c)?;
        Ok(buf[0])
    }

    fn write_register<I2C, E>(&self, i2c: &mut I2C, register: u8, value: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        i2c.write(self.addr, &[register, value]).map_err(Error::I2c)
    }
}
