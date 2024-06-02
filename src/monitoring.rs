use defmt::info;
use embassy_stm32::adc::{Adc, SampleTime};
use embassy_stm32::peripherals::{ADC1, IWDG, PA0};
use embassy_stm32::wdg::IndependentWatchdog;
use embassy_stm32::{adc, bind_interrupts, Peripheral, PeripheralRef};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};

use fixed::types::I16F16;
use fixed_macro::types::I16F16;

static POKE_MEASURING: embassy_sync::signal::Signal<ThreadModeRawMutex, ()> =
    embassy_sync::signal::Signal::new();

pub fn poke_measuring() {
    POKE_MEASURING.signal(());
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Temp(pub I16F16);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Voltage(pub I16F16);

pub static TEMP: Mutex<ThreadModeRawMutex, Temp> = Mutex::new(Temp(I16F16!(0)));
pub static VOLTAGE: Mutex<ThreadModeRawMutex, Voltage> = Mutex::new(Voltage(I16F16!(0)));

bind_interrupts!(struct Irqs {
    ADC1_COMP => adc::InterruptHandler<ADC1>;
});

const VREF_CAL: *const u16 = 0x1FF8_0078 as _;
const TS_CAL1: *const u16 = 0x1FF8_007A as _;
const TS_CAL2: *const u16 = 0x1FF8_007E as _;

const BATTERY_VOLTAGE_FACTOR: I16F16 = I16F16!(4.2).unwrapped_div(I16F16!(2.4));

struct Factors {
    vref_scale: I16F16,
    volts_scale: I16F16,
    ts_cal_30: I16F16,
    ts_cal_130: I16F16,
}

impl Factors {
    async fn calculate<'a>(p: PeripheralRef<'a, ADC1>) -> Self {
        let mut adc = Adc::new(p, Irqs);
        adc.set_sample_time(SampleTime::CYCLES160_5);

        let mut vrefint = adc.enable_vref();
        let vrefint_sample = adc.read(&mut vrefint).await;

        let vrefint_sample = I16F16::from_num(vrefint_sample);
        let vref_cal = I16F16::from_num(unsafe { core::ptr::read_volatile(VREF_CAL) });

        let vref_scale = vref_cal / vrefint_sample;
        let batt_volts_scale = BATTERY_VOLTAGE_FACTOR * vref_scale * I16F16!(3.0) / I16F16!(4095);

        let ts_cal_30 = I16F16::from_num(unsafe { core::ptr::read_volatile(TS_CAL1) });
        let ts_cal_130 = I16F16::from_num(unsafe { core::ptr::read_volatile(TS_CAL2) });

        Self {
            vref_scale,
            volts_scale: batt_volts_scale,
            ts_cal_30,
            ts_cal_130,
        }
    }

    fn volts_from_raw(&self, raw: u16) -> Voltage {
        let v = I16F16::from_num(raw) * self.volts_scale;
        Voltage(v)
    }

    fn temp_from_raw(&self, raw: u16) -> Temp {
        let t = I16F16::from_num(raw) * self.vref_scale;
        let t = t - self.ts_cal_30;
        let t = t * I16F16!(100.0);
        let t = t / (self.ts_cal_130 - self.ts_cal_30);
        let t = t + I16F16!(30.0);

        Temp(t)
    }
}

struct Smoother(I16F16);

impl Smoother {
    pub fn update(&mut self, value: I16F16) {
        let diff = (value / I16F16!(8)) - (self.0 / I16F16!(8));

        self.0 = self.0.saturating_add(diff);
    }
}

struct Smoothers {
    temp: Smoother,
    voltage: Smoother,
}

// #[embassy_executor::task]
pub async fn monitoring_task(mut bat_level: PA0, adc: ADC1, wd: IWDG) {
    let mut adc = adc.into_ref();

    let mut watchdog = IndependentWatchdog::new(wd, 6_000_000);
    // watchdog.unleash();

    let mut smoothers = Smoothers {
        temp: Smoother(I16F16!(20.0)),
        voltage: Smoother(I16F16!(4.0)),
    };

    let factors = Factors::calculate(adc.reborrow()).await;

    loop {
        if crate::state::is_on().await {
            measure_while_on(
                &mut watchdog,
                &mut bat_level,
                adc.reborrow(),
                &factors,
                &mut smoothers,
            )
            .await;
        } else {
            measure_while_off(
                &mut watchdog,
                &mut bat_level,
                adc.reborrow(),
                &factors,
                &mut smoothers,
            )
            .await;
        }
    }
}

async fn measure_and_update(
    watchdog: &mut IndependentWatchdog<'_, IWDG>,
    bat_level: &mut PA0,
    tempsense: &mut adc::Temperature,
    adc: &mut Adc<'_, ADC1>,
    factors: &Factors,
    smoothers: &mut Smoothers,
) {
    let v = adc.read(bat_level).await;
    let v = factors.volts_from_raw(v);
    smoothers.voltage.update(v.0);

    *VOLTAGE.lock().await = Voltage(smoothers.voltage.0);

    let t = adc.read(tempsense).await;
    let t = factors.temp_from_raw(t);
    smoothers.temp.update(t.0);

    *TEMP.lock().await = Temp(smoothers.temp.0);

    if t.0 > I16F16!(60.0) {
        crate::state::emergency_stop();
    }

    watchdog.pet();

    info!(
        "v: {}, t: {}",
        defmt::Display2Format(&smoothers.voltage.0),
        defmt::Display2Format(&smoothers.temp.0)
    );
}

async fn measure_while_on(
    watchdog: &mut IndependentWatchdog<'_, IWDG>,
    bat_level: &mut PA0,
    p: PeripheralRef<'_, ADC1>,
    factors: &Factors,
    smoothers: &mut Smoothers,
) {
    let mut adc = Adc::new(p, Irqs);
    adc.set_sample_time(SampleTime::CYCLES160_5);

    let mut tempsense = adc.enable_temperature();

    loop {
        measure_and_update(
            watchdog,
            bat_level,
            &mut tempsense,
            &mut adc,
            factors,
            smoothers,
        )
        .await;

        if !crate::state::is_on().await {
            return;
        }

        maitake::time::sleep(core::time::Duration::from_millis(250)).await;
    }
}

async fn measure_while_off(
    watchdog: &mut IndependentWatchdog<'_, IWDG>,
    bat_level: &mut PA0,
    mut p: PeripheralRef<'_, ADC1>,
    factors: &Factors,
    smoothers: &mut Smoothers,
) {
    loop {
        // this scope is important, we must drop the adc before we sleep
        {
            let mut adc = Adc::new(p.reborrow(), Irqs);
            adc.set_sample_time(SampleTime::CYCLES160_5);

            let mut tempsense = adc.enable_temperature();

            measure_and_update(
                watchdog,
                bat_level,
                &mut tempsense,
                &mut adc,
                factors,
                smoothers,
            )
            .await;

            if crate::state::is_on().await {
                return;
            }
        }

        let _ =
            maitake::time::timeout(core::time::Duration::from_secs(4), POKE_MEASURING.wait()).await;
    }
}

// #[allow(non_snake_case)]
// struct TemperatureSmoother {
//     u: I16F16,
//     std_dev_a: I16F16,
//     std_dev_m: I16F16,
//     A: nalgebra::SMatrix<I16F16, 2, 2>,
//     B: nalgebra::SMatrix<I16F16, 2, 1>,
//     H: nalgebra::SMatrix<I16F16, 1, 2>,
//     Q: nalgebra::SMatrix<I16F16, 2, 2>,
//     R: nalgebra::SMatrix<I16F16, 1, 1>,
//     P: nalgebra::SMatrix<I16F16, 2, 2>,
//     x: nalgebra::SVector<I16F16, 2>,
// }

// impl TemperatureSmoother {}
