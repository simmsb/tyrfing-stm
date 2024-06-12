#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_fn_trait_return)]
#![feature(async_closure)]
#![no_std]
#![no_main]

use defmt::*;
use embassy_stm32::rtc::{Rtc, RtcConfig};
use portable_atomic::AtomicUsize;
use static_cell::{make_static, StaticCell};
// use static_cell::StaticCell;
#[cfg(not(feature = "debug"))]
use panic_reset as _;
#[cfg(feature = "debug")]
use {defmt_rtt as _, panic_probe as _};

mod aux;
mod battery_level;
mod click;
mod monitoring;
mod power;
mod power_curve;
mod state;
mod ui;

#[cfg(feature = "use_maitake_executor")]
mod executor;

static CNT: AtomicUsize = AtomicUsize::new(0);

defmt::timestamp! {"{}", CNT.fetch_add(1, portable_atomic::Ordering::Relaxed) }

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

#[cfg(feature = "use_maitake_executor")]
mod maitake_stuff {
    use core::future::Future;

    pub struct StaticStorage;

    impl<S: 'static, F: Future + 'static> maitake::task::Storage<S, F> for StaticStorage {
        type StoredTask = &'static mut maitake::task::Task<S, F, StaticStorage>;

        fn into_raw(task: Self::StoredTask) -> core::ptr::NonNull<maitake::task::Task<S, F, Self>> {
            core::ptr::NonNull::from(task)
        }

        fn from_raw(
            mut ptr: core::ptr::NonNull<maitake::task::Task<S, F, Self>>,
        ) -> Self::StoredTask {
            unsafe { ptr.as_mut() }
        }
    }

    impl StaticStorage {
        pub fn allocate<S: maitake::scheduler::Schedule + 'static, F: Future + 'static>(
            fut: F,
        ) -> maitake::task::Task<S, F, Self> {
            maitake::task::Task::new(fut)
        }
    }

    #[repr(transparent)]
    pub struct SurelySend<T>(pub T);
    unsafe impl<T> Send for SurelySend<T> {}
    impl<T: Future> Future for SurelySend<T> {
        type Output = <T as Future>::Output;

        fn poll(
            self: core::pin::Pin<&mut Self>,
            cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Self::Output> {
            unsafe { T::poll(core::mem::transmute(self), cx) }
        }
    }
}

#[cfg(feature = "use_maitake_executor")]
#[cortex_m_rt::entry]
fn main() -> ! {
    use maitake_stuff::*;

    macro_rules! spawn {
        ($f:expr) => {{
            let task = make_static!(StaticStorage::allocate(SurelySend($f)));
            task.bind(crate::executor::scheduler());
            crate::executor::scheduler()
                .build_task()
                .spawn_allocated::<StaticStorage, _>(task)
        }};
    }

    info!("Hello world");

    let cfg = embassy_stm32::Config::default();
    let p = embassy_stm32::init(cfg);
    let rtc = Rtc::new(p.RTC, RtcConfig::default());

    static RTC: StaticCell<Rtc> = StaticCell::new();
    let rtc = RTC.init(rtc);

    spawn!(monitoring::monitoring_task(p.PA0, p.ADC1, p.IWDG));
    // spawn!(power::power_task(p.PA2, p.PA3, p.DAC1, p.PA4, p.PA5));
    spawn!(power::power_task(p.PA2, p.PB5, p.DAC1, p.PA4, p.PA5));
    spawn!(aux::aux_task(p.TIM3, p.PA6, p.PA7, p.PB0));
    spawn!(click::debouncer_task(p.PA8, p.EXTI8, p.PC15));
    spawn!(click::event_generator_task());
    spawn!(ui::torch_ui_task());

    crate::executor::run(rtc);
}

// TODO(ben) add back in support for using the embassy executor, check binary sizes

// #[embassy_executor::task]
// #[embassy_executor::main]
// async fn async_main(spawner: Spawner) {
//     let cfg = embassy_stm32::Config::default();
//     // cfg.rcc.ls = LsConfig::default_lsi();
//     // cfg.rcc.hsi = true;
//     let p = embassy_stm32::init(cfg);

//     // let rtc = Rtc::new(p.RTC, RtcConfig::default());

//     // static RTC: StaticCell<Rtc> = StaticCell::new();
//     // let rtc = RTC.init(rtc);
//     // embassy_stm32::low_power::stop_with_rtc(rtc);

//     info!("Hello World!");

//     spawner.must_spawn(monitoring::monitoring_task(p.PA0, p.ADC1, p.IWDG));
//     spawner.must_spawn(power::power_task(p.PA2, p.PA3, p.DAC1, p.PA4, p.PA5));
//     spawner.must_spawn(aux::aux_task(p.TIM3, p.PA6, p.PA7, p.PB0));
//     spawner.must_spawn(click::debouncer_task(p.PA8, p.EXTI8));
//     spawner.must_spawn(click::event_generator_task());
//     spawner.must_spawn(ui::torch_ui_task());

//     spawner.must_spawn(vary_light());
// }
