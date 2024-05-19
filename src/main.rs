#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
// use embassy_stm32::{low_power, rcc::LsConfig, rtc::{DateTime, Rtc, RtcConfig}};
use embassy_time::Timer;
// use static_cell::StaticCell;
#[cfg(feature = "debug")]
use {defmt_rtt as _, panic_probe as _};
#[cfg(not(feature = "debug"))]
use panic_reset as _;

mod aux;
mod click;
mod monitoring;
mod power;
mod power_curve;
mod state;
mod ui;

//  _____________________________
// /\                            \
// \_| PINS                      |
//   | ----                      |
//   | PA0: Battery sense        |
//   | PA2: HDR                  |
//   | PA3: EN (dev)             |
//   | PA4: DAC out              |
//   | PA6: Red Aux   (c1: tim3) |
//   | PA7: Green Aux (c2: tim3) |
//   | PB0: Blue Aux  (c3: tim3) |
//   | PC15: Button LED          |
//   | PA8: Button               |
//   | PB5: EN (prod)            |
//   |   ________________________|_
//    \_/__________________________/

#[embassy_executor::task]
async fn vary_light() {
    let top_out_at = 40;

    loop {
        for i in 0..top_out_at {
            power::set_level(i).await;
            Timer::after_millis(30).await;
        }
        for i in (0..top_out_at).rev() {
            power::set_level(i).await;
            Timer::after_millis(30).await;
        }
        Timer::after_secs(4).await;
    }
}

// #[cortex_m_rt::entry]
// fn main() -> ! {
//     low_power::Executor::take().run(|spawner| {
//         spawner.must_spawn(async_main(spawner));
//     });
// }

// #[embassy_executor::task]
#[embassy_executor::main]
async fn async_main(spawner: Spawner) {
    let cfg = embassy_stm32::Config::default();
    // cfg.rcc.ls = LsConfig::default_lsi();
    // cfg.rcc.hsi = true;
    let p = embassy_stm32::init(cfg);

    // let rtc = Rtc::new(p.RTC, RtcConfig::default());

    // static RTC: StaticCell<Rtc> = StaticCell::new();
    // let rtc = RTC.init(rtc);
    // embassy_stm32::low_power::stop_with_rtc(rtc);

    info!("Hello World!");

    spawner.must_spawn(monitoring::monitoring_task(p.PA0, p.ADC1, p.IWDG));
    spawner.must_spawn(power::power_task(p.PA2, p.PA3, p.DAC1, p.PA4, p.PA5));
    spawner.must_spawn(aux::aux_task(p.TIM3, p.PA6, p.PA7, p.PB0));
    spawner.must_spawn(click::debouncer_task(p.PA8, p.EXTI8));
    spawner.must_spawn(click::event_generator_task());
    spawner.must_spawn(ui::torch_ui_task());

    spawner.must_spawn(vary_light());
}
