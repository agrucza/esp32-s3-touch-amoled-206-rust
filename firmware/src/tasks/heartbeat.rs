use embassy_time::{Duration, Timer};

/// Logs a heartbeat message every second so we can confirm the async
/// runtime is alive and the chip is running. No GPIO needed for milestone 1.
///
/// A real blink task will be added once we've confirmed a safe output pin
/// from the schematic (no dedicated LED is broken out on this board).
#[embassy_executor::task]
pub async fn heartbeat() {
    let mut count: u32 = 0;
    loop {
        count += 1;
        log::info!("heartbeat #{}", count);
        Timer::after(Duration::from_secs(1)).await;
    }
}
