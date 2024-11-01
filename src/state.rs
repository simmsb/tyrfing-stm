use embassy_stm32::gpio::Output;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};

use crate::aux::poke_aux;

static ON: Mutex<ThreadModeRawMutex, bool> = Mutex::new(false);

pub async fn is_on() -> bool {
    *ON.lock().await
}

pub async fn set_on(on: bool) {
    *ON.lock().await = on;
}

static UNLOCKED: Mutex<ThreadModeRawMutex, bool> = Mutex::new(false);

pub async fn is_unlocked() -> bool {
    *UNLOCKED.lock().await
}

pub async fn set_unlocked(unlocked: bool) {
    *UNLOCKED.lock().await = unlocked;
    poke_aux();
}

pub fn emergency_stop() {
    let en_pin = unsafe { embassy_stm32::peripherals::PA3::steal() };
    let mut en_pin = Output::new(
        en_pin,
        embassy_stm32::gpio::Level::Low,
        embassy_stm32::gpio::Speed::Low,
    );
    en_pin.set_low();

    cortex_m::peripheral::SCB::sys_reset();
}
