use defmt::{info, trace};
use embassy_stm32::rtc::Rtc;
use embassy_time::{Duration, TICK_HZ};
use embassy_time_driver::{allocate_alarm, set_alarm, set_alarm_callback};
use maitake::{
    scheduler::{self, StaticScheduler},
    time::{Clock, Timer},
};

static SCHEDULER: StaticScheduler = scheduler::new_static!();

fn pend(_ctx: *mut ()) {
    cortex_m::asm::sev();
}

// /// Available Stop modes.
// #[non_exhaustive]
// #[derive(PartialEq, defmt::Format)]
// pub enum StopMode {
//     /// STOP 1
//     Stop1,
//     /// STOP 2
//     Stop2,
// }

// fn stop_mode() -> Option<StopMode> {
//     trace!(
//         "Sleep test: {}, {}",
//         unsafe { embassy_stm32::rcc::REFCOUNT_STOP2 },
//         unsafe { embassy_stm32::rcc::REFCOUNT_STOP1 },
//     );

//     if unsafe { embassy_stm32::rcc::REFCOUNT_STOP2 == 0 }
//         && unsafe { embassy_stm32::rcc::REFCOUNT_STOP1 == 0 }
//     {
//         Some(StopMode::Stop2)
//     } else if unsafe { embassy_stm32::rcc::REFCOUNT_STOP1 == 0 } {
//         Some(StopMode::Stop1)
//     } else {
//         None
//     }
// }

// fn configure_pwr() {
//     let mut scb = unsafe { cortex_m::Peripherals::steal().SCB };

//     scb.clear_sleepdeep();

//     compiler_fence(portable_atomic::Ordering::SeqCst);

//     let stop_mode = stop_mode();

//     let Some(stop_mode) = stop_mode else {
//         return;
//     };

//     trace!("Stop mode: {}", stop_mode);

//     if embassy_stm32::time_driver::get_driver()
//         .pause_time()
//         .is_err()
//     {
//         // embassy_stm32::pac::PWR.cr().modify(|w| {
//         //     w.set_lpsdsr(embassy_stm32::pac::pwr::vals::Mode::MAIN_MODE);
//         // });
//         trace!("Not entering deepsleep");
//         return;
//     }

//     // embassy_stm32::pac::PWR.cr().modify(|w| {
//     //     w.set_lpsdsr(embassy_stm32::pac::pwr::vals::Mode::LOW_POWER_MODE);
//     // });

//     trace!("entering deep sleep");

//     scb.set_sleepdeep();
// }

// #[export_name = "__on_wakeup_irq"]
// fn __on_wakeup_irq() {
//     info!("!!!!!!!!!!!!!!!!!!! Irq wakeup");
//     embassy_stm32::time_driver::get_driver().resume_time();
// }

pub fn run(_rtc: &'static mut Rtc) -> ! {
    info!("doing clock setup");

    // let driver = embassy_stm32::time_driver::get_driver();
    // driver.set_rtc(rtc);

    // rtc.enable_wakeup_line();

    let clock = Clock::new(Duration::from_hz(TICK_HZ).into(), || {
        embassy_time::Instant::now().as_ticks()
    });
    let timer = Timer::new(clock);

    let timer = &*static_cell::make_static!(timer);

    maitake::time::set_global_timer(timer).unwrap();

    let alarm = unsafe { allocate_alarm().unwrap() };

    loop {
        timer.turn();

        let tick = SCHEDULER.tick();

        let turn = timer.turn();

        if !tick.has_remaining {
            let _should_try_deepsleep = if let Some(next_turn) = turn.ticks_to_next_deadline() {
                trace!("now: {}", embassy_time::Instant::now().as_ticks());
                trace!("Next tick in: {}", turn.time_to_next_deadline());
                let ts = embassy_time::Instant::now().as_ticks() + next_turn;
                // info!("Asked for an alarm at {}", ts);
                set_alarm_callback(alarm, pend, core::ptr::null_mut());
                if !set_alarm(alarm, ts) {
                    continue;
                }

                next_turn > TICK_HZ / 2
            } else {
                trace!("No ticks: {}", defmt::Debug2Format(&tick));
                true
            };

            // this is an annoying race condition, because .pause_time() reads
            // from the current alarms, it's possible the alarm expires before
            // we try to deep sleep, resulting in deep sleep thinking there's no
            // alarms and that it should sleep for the max time.
            // if should_try_deepsleep {
            //     configure_pwr();
            // }

            trace!("WFE");
            // defmt::flush();
            cortex_m::asm::wfe();
        }
    }
}

pub fn scheduler() -> &'static StaticScheduler {
    &SCHEDULER
}
