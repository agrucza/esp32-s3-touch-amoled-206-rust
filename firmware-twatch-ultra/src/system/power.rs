//! The T-Watch Ultra `system_core::board::Board` impl + PMU/expander
//! bring-up.
//!
//! Mirrors `firmware-s3/src/system/power.rs` in role. Board deltas:
//! no SYS_OUT latch and no motor GPIO - the AXP2101 manages long-press
//! shutdown internally (6 s), and haptics are a DRV2605 on I2C behind
//! an XL9555 enable, which the sync no-bus `Board::buzz` seam can't
//! drive (wire it up when the haptics effort lands). `arm_wake_sources`
//! arms BOOT (GPIO0), RTC INT (GPIO1), PMU IRQ (GPIO7 - readable on
//! this board, so the PWR button wakes from light sleep) and touch INT
//! (GPIO12).
//!
//! Rails ARE configured every boot (unlike the C6, which trusts the
//! AXP's persisted state): the map differs from any factory state and
//! the vendor firmware also sets it on every boot. Voltages per the
//! rail table in `board.rs` - everything 3.3 V except ALDO4 at 1.8 V
//! (the BHI260AP is a 1.8 V part; see board.rs).

use drivers::pmu::{Config as PmuConfig, Pmu};
use drivers::xl9555::{Config as ExpanderConfig, Xl9555};
use embedded_hal::i2c::I2c;
use esp_hal::gpio::{Input, Output};
use esp_hal::peripherals::GPIO;
use system_core::board::{Board, CpuFreq};

pub struct TwatchUltraBoard {
    /// PMU IRQ line (GPIO7). Not read directly - the shared power task
    /// polls the AXP2101 status registers - but held configured as an
    /// input so `arm_wake_sources` can make a PWR-button press wake
    /// the watch from light sleep.
    _pmu_irq: Input<'static>,
    /// Chip selects of the not-yet-driven shared-SPI peripherals,
    /// held high (deselected) so SD-card traffic on the shared bus
    /// can't address them.
    _lora_cs: Output<'static>,
    _nfc_cs: Output<'static>,
}

impl TwatchUltraBoard {
    /// Bring up the PMU rails and the XL9555 gates. Must run before
    /// any peripheral that hangs off the gated rails (display, touch,
    /// SD). Returns `(TwatchUltraBoard, Pmu)`; the caller wraps the `Pmu`
    /// in a `PowerTaskState` for the polling task.
    pub fn init(
        i2c: &mut impl I2c,
        pmu_irq: Input<'static>,
        lora_cs: Output<'static>,
        nfc_cs: Output<'static>,
    ) -> Result<(Self, Pmu), ()> {
        let pmu = Pmu::new(PmuConfig::default());
        log::info!("PMU: initializing AXP2101...");
        match pmu.check_device(i2c) {
            Ok(raw_id) => log::info!(
                "PMU: AXP2101 rev {} (0x{:02X})",
                (raw_id >> 4) & 0x03,
                raw_id,
            ),
            Err(_) => {
                log::error!("PMU: AXP2101 not responding");
                return Err(());
            }
        }

        // Boot rails (see the rail table in board.rs). Not
        // `Pmu::init()` - that helper bakes in another board's
        // voltages. DLDO1 (NFC) stays off until the NFC effort;
        // VBACKUP coin-cell charging is the RTC effort's call.
        pmu.set_aldo1_voltage(i2c, 3300).map_err(|_| ())?; // SD card
        pmu.set_aldo2_voltage(i2c, 3300).map_err(|_| ())?; // Display VCI
        pmu.set_aldo3_voltage(i2c, 3300).map_err(|_| ())?; // LoRa
        pmu.set_aldo4_voltage(i2c, 1800).map_err(|_| ())?; // BHI260AP - 1.8V part!
        pmu.set_bldo1_voltage(i2c, 3300).map_err(|_| ())?; // GPS
        pmu.set_bldo2_voltage(i2c, 3300).map_err(|_| ())?; // Speaker amp
        pmu.enable_all_rails(i2c).map_err(|_| ())?;
        pmu.enable_all_adc(i2c).map_err(|_| ())?;
        pmu.enable_battery_monitor(i2c).map_err(|_| ())?;

        // Discard PMU IRQ bits latched before boot: the >= 1 s PWRON
        // hold that powers the watch on latches PKEY press events,
        // and the power task's first poll would read them as a fresh
        // user action (observed as a phantom shutdown request right
        // after the first render). The vendor firmware clears IRQ
        // status at init for the same reason.
        if let Ok(status) = pmu.read_interrupts(i2c) {
            let _ = pmu.clear_interrupts(i2c, &status);
        }

        // XL9555 gates, vendor order: haptic enable (P06), display
        // VCI enable (P07), touch reset released high (P10). The
        // touch reset PULSE happens later in make_input.
        let expander = Xl9555::new(ExpanderConfig::default());
        if expander.probe(i2c).is_err() {
            log::error!("XL9555 not responding");
            return Err(());
        }
        for pin in [
            crate::board::EXP_DRV_EN,
            crate::board::EXP_DISP_EN,
            crate::board::EXP_TOUCH_RST,
        ] {
            expander.set_output(i2c, pin, true).map_err(|_| ())?;
        }
        log::info!("PMU: rails + expander gates up");

        Ok((
            Self {
                _pmu_irq: pmu_irq,
                _lora_cs: lora_cs,
                _nfc_cs: nfc_cs,
            },
            pmu,
        ))
    }
}

impl Board for TwatchUltraBoard {
    /// Haptics are a DRV2605 on I2C (enable via XL9555) - not
    /// drivable from this sync no-bus seam. No-op until the haptics
    /// effort designs that path.
    fn buzz(&mut self) {}

    /// See `buzz`.
    fn buzz_stop(&mut self) {}

    /// No soft-power latch: the AXP2101 handles long-press (6 s)
    /// shutdown internally. Nothing for firmware to do.
    fn shutdown(&mut self) {
        log::info!("PWR: shutdown is AXP2101-managed on this board (no-op)");
    }

    /// Re-arm GPIO wake for BOOT (0), RTC INT (1), PMU IRQ (7) and
    /// touch INT (12). The embassy async GPIO drivers clear the
    /// `wakeup_enable` bits set at init on every wait, so the board
    /// sets them back immediately before `rtc.sleep()`. `int_type=4`
    /// is LowLevel - all four lines are active-low - and the only
    /// type esp-hal allows for wake-from-light-sleep.
    fn arm_wake_sources(&mut self) {
        for &gpio_num in &[
            crate::board::BTN_BOOT,
            crate::board::RTC_INT,
            crate::board::PMU_IRQ,
            crate::board::TOUCH_INT,
        ] {
            GPIO::regs().pin(gpio_num as usize).modify(|_, w| unsafe {
                w.wakeup_enable().set_bit();
                w.int_type().bits(4)
            });
        }
    }

    /// Switch CPU frequency at runtime via the `SYSTEM.cpu_per_conf`
    /// divider (PLL stays the source - APB stays 80 MHz so I2C/SPI
    /// are unaffected). Same silicon and same poke as the other S3
    /// board; see that impl for the esp-hal staleness caveat.
    fn set_cpu_freq(&mut self, freq: CpuFreq) {
        use esp_hal::peripherals::SYSTEM;
        let (period_sel, freq_mhz) = match freq {
            CpuFreq::Mhz80 => (0u8, 80u32),
            CpuFreq::Mhz160 => (1u8, 160u32),
            CpuFreq::Mhz240 => (2u8, 240u32),
        };
        SYSTEM::regs().cpu_per_conf().modify(|_, w| unsafe {
            w.pll_freq_sel().set_bit();
            w.cpuperiod_sel().bits(period_sel)
        });
        esp_hal::rom::ets_update_cpu_frequency_rom(freq_mhz);
    }

    /// The light-sleep recipe validated on the other S3 board (same
    /// chip family): RTC regulator kept powered, main XTAL allowed to
    /// power down, sleep-reject off so a latched INT can't silently
    /// cancel sleep entry. Re-validate on this board when sleep is
    /// first exercised here.
    fn tune_sleep_config(
        &self,
        cfg: &mut esp_hal::rtc_cntl::sleep::RtcSleepConfig,
    ) {
        cfg.set_rtc_regulator_fpu(true);
        cfg.set_xtal_fpu(false);
        cfg.set_light_slp_reject(false);
    }
}
