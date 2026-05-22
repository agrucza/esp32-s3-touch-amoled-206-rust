//! The C6 `system_core::board::Board` impl + AXP2101 sanity check.
//!
//! Mirrors `firmware-s3/src/system/power.rs` in role, but this board
//! has no SYS_OUT latch GPIO and no haptic motor: the AXP2101 manages
//! long-press shutdown internally and PWR is read via the inverting
//! MOSFET on GPIO18 (board.rs). So `shutdown` / `buzz` / `buzz_stop`
//! are no-ops. `arm_wake_sources` arms this board's wake GPIOs (touch
//! INT GPIO15, BOOT GPIO9); there is no RTC_INT pin here - periodic
//! RTC-cadence wake comes from the manager's internal timer source.
//!
//! AXP rails are NOT configured: the AXP retains rail state across
//! MCU resets and the reference firmware never enables them in
//! software. We `check_device` for reachability and `enable_all_adc`
//! (the board-independent ADC channels - vbus/vsys/die-temp - which
//! touch no rails), but skip the rail config. The returned `Pmu` goes
//! to the shared power task.

use drivers::pmu::{Config as PmuConfig, Pmu};
use embedded_hal::i2c::I2c;
use esp_hal::peripherals::GPIO;
use system_core::board::{Board, CpuFreq};

/// Zero-sized board glue for the C6. All board-specific manager
/// operations are no-ops or register pokes; there is no per-board
/// state to hold.
pub struct C6Board;

impl C6Board {
    /// Sanity-check the AXP2101 (no rail config - it retains state
    /// across resets). Returns `(C6Board, Pmu)`; the caller wraps the
    /// `Pmu` in the shared `PowerTaskState`.
    pub fn init(i2c: &mut impl I2c) -> (Self, Pmu) {
        let pmu = Pmu::new(PmuConfig::default());
        match pmu.check_device(i2c) {
            Ok(chip_id) => log::info!(
                "AXP2101 chip ID: 0x{:02X} (rev {:02b})",
                chip_id, (chip_id >> 4) & 0x03,
            ),
            Err(_) => log::error!("AXP2101 check_device failed"),
        }

        // Enable the AXP2101 ADC channels (vbus/vsys/die-temp/TS/batt).
        // Board-independent - the same channels the S3 enables - and it
        // touches no rails, so it's safe even though we otherwise trust
        // the AXP's persisted rail state here. Without it, vbus/vsys/
        // die-temp read 0.
        if pmu.enable_all_adc(i2c).is_err() {
            log::error!("AXP2101: enable_all_adc failed");
        }

        (Self, pmu)
    }
}

impl Board for C6Board {
    /// No haptic motor on this board.
    fn buzz(&mut self) {}

    /// No haptic motor on this board.
    fn buzz_stop(&mut self) {}

    /// No SYS_OUT latch GPIO: the AXP2101 handles long-press
    /// shutdown internally. Nothing for firmware to do.
    fn shutdown(&mut self) {
        log::info!("PWR: shutdown is AXP2101-managed on this board (no-op)");
    }

    /// Re-arm the C6 wake GPIOs (BOOT GPIO9, touch INT GPIO15). No
    /// RTC_INT pin on this board - the manager's `TimerWakeupSource`
    /// provides the periodic background-poll wake instead. `int_type
    /// = 4` is LowLevel (the INT lines are active-low), the only
    /// type esp-hal allows for wake-from-light-sleep.
    ///
    /// NOTE: C6 hardware light-sleep itself is not yet validated;
    /// this arms the right pins so it's correct when sleep is
    /// brought up. The manager only calls this on sleep entry.
    fn arm_wake_sources(&mut self) {
        for &gpio_num in &[crate::board::BTN_BOOT, crate::board::TOUCH_INT] {
            GPIO::regs().pin(gpio_num as usize).modify(|_, w| unsafe {
                w.wakeup_enable().set_bit();
                w.int_type().bits(4)
            });
        }
    }

    /// Switch CPU frequency at runtime. This chip (RISC-V) tops out
    /// at 160 MHz. With PLL as the root clock (left as configured at
    /// init - we do NOT touch the source) the hardware AUTODIV gives
    /// HP_ROOT_CLK = 160 MHz, and CPU_CLK = HP_ROOT / (cpu_hs_div_num
    /// + 1): `0` -> 160 MHz, `1` -> 80 MHz. Only the CPU high-speed
    /// divider changes (PLL/source stay up), which is the same
    /// low-risk class of poke esp-hal itself does in
    /// `configure_cpu_hs_div_impl` (esp-hal 1.1.0-rc.0,
    /// src/soc/esp32c6/clocks.rs) - a single `modify()`, no
    /// apply/poll bit, no flash-timing change. `Mhz240` is
    /// unreachable here; clamp to 160 (the manager never requests it
    /// at runtime).
    fn set_cpu_freq(&mut self, freq: CpuFreq) {
        use esp_hal::peripherals::PCR;
        let hs_div: u8 = match freq {
            CpuFreq::Mhz80 => 1,                          // 160 / 2 = 80
            CpuFreq::Mhz160 | CpuFreq::Mhz240 => 0,       // 160 / 1 = 160 (max)
        };
        PCR::regs()
            .cpu_freq_conf()
            .modify(|_, w| unsafe { w.cpu_hs_div_num().bits(hs_div) });
    }

    /// No chip-specific light-sleep tuning applied: the s3
    /// fpu/reject knobs don't exist on this chip's `RtcSleepConfig`,
    /// and C6 hardware light-sleep is not yet validated - the default
    /// config is the correct starting point until the C6 sleep
    /// recipe is brought up.
    fn tune_sleep_config(
        &self,
        _cfg: &mut esp_hal::rtc_cntl::sleep::RtcSleepConfig,
    ) {
    }
}
