use defmt::{debug, info, trace};
use embassy_stm32::{
    dac::{Dac, DacCh1, Value},
    dma::NoDma,
    gpio::Output,
    peripherals::{DAC1, PA5},
    Peripheral,
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use embassy_time::Duration;
use fixed::types::U32F32;
use fixed_macro::types::{I16F16, I32F32};

use crate::{
    monitoring::{Temp, Voltage},
    pins,
};

static DESIRED_LEVEL: embassy_sync::mutex::Mutex<ThreadModeRawMutex, u8> = Mutex::new(0);
static GRADUAL_LEVEL: embassy_sync::mutex::Mutex<ThreadModeRawMutex, u8> = Mutex::new(0);

static POKE_POWER_CONTROLLER: embassy_sync::signal::Signal<ThreadModeRawMutex, ()> =
    embassy_sync::signal::Signal::new();

fn poke_power_controller() {
    POKE_POWER_CONTROLLER.signal(());
}

pub async fn blink(blinks: u8) {
    // TODO: calculate blink level off current level
    let current_level = *DESIRED_LEVEL.lock().await;

    for _ in 0..blinks {
        set_level(30).await;
        maitake::time::sleep(core::time::Duration::from_millis(100)).await;
        set_level(current_level).await;
        maitake::time::sleep(core::time::Duration::from_millis(100)).await;
    }
}

pub async fn set_level_gradual(level: u8) {
    let mut gradual_level = GRADUAL_LEVEL.lock().await;

    let needs_poking = *gradual_level == 0;

    *gradual_level = level;

    if needs_poking {
        poke_power_controller();
    }
}

pub async fn set_level(level: u8) {
    let mut desired_level = DESIRED_LEVEL.lock().await;
    *desired_level = level;

    set_level_gradual(level).await;
}

struct PowerPaths<'a> {
    dac: DacCh1<'a, DAC1>,
    hdr: Output<'a>,
    opamp_en: Output<'a>,
    boost_en: Output<'a>,
    shunt_select: Output<'a>,
}

impl<'a> PowerPaths<'a> {
    fn set_hdr(&mut self, high_range: bool) {
        self.hdr.set_level(high_range.into());
        self.shunt_select.set_level(high_range.into());
    }

    async fn set(&mut self, level: u8) {
        if level == 0 {
            debug!("Setting light level to {}", level);
            self.dac.set(embassy_stm32::dac::Value::Bit8(0));
            self.dac.disable();
            self.set_hdr(false);
            self.opamp_en.set_low();
            self.boost_en.set_low();
        } else {
            if self.boost_en.is_set_low() {
                self.dac.set(embassy_stm32::dac::Value::Bit8(0));
                self.dac.enable();
                self.set_hdr(false);
                self.opamp_en.set_high();
                maitake::time::sleep(core::time::Duration::from_millis(8)).await;
                self.boost_en.set_high();
                crate::monitoring::poke_measuring();
                debug!("Bringing up light");
            }

            let config = &crate::power_curve::POWER_LEVELS[(level - 1) as usize];
            debug!("hdr: {}, dac: {}", config.hdr, config.dac);
            self.dac
                .set(embassy_stm32::dac::Value::Bit12Right(config.dac));
            self.set_hdr(config.hdr.into());
        }
    }
}

const INSTANT_STOP_TEMP: Temp = Temp(I16F16!(50.0));
const MAX_TEMP: Temp = Temp(I16F16!(40.0));
const MIN_VOLTS: Voltage = Voltage(I16F16!(3.0));
const INSTANT_STOP_VOLTS: Voltage = Voltage(I16F16!(3.0));

async fn handle_on_state<'a>(mut paths: PowerPaths<'a>) {
    let mut previous_level = 0u8;

    let mut accumulated_over_temp = U32F32::ZERO;
    let mut accumulated_under_volts = U32F32::ZERO;

    loop {
        let gradual_level = *GRADUAL_LEVEL.lock().await;
        let desired_level = *DESIRED_LEVEL.lock().await;

        let delta = if desired_level.abs_diff(gradual_level) > 50 {
            3
        } else {
            1
        };

        let desired_level = if desired_level < gradual_level {
            desired_level + delta
        } else if desired_level > gradual_level {
            desired_level - delta
        } else {
            desired_level
        };
        *DESIRED_LEVEL.lock().await = desired_level;

        let mut actual_level = desired_level;

        let volts = *crate::monitoring::VOLTAGE.lock().await;

        if volts < INSTANT_STOP_VOLTS {
            actual_level = 0;
        }

        let temp = *crate::monitoring::TEMP.lock().await;

        if temp > INSTANT_STOP_TEMP {
            actual_level = 0;
        }

        let temp_diff = temp.0 - MAX_TEMP.0;

        accumulated_over_temp =
            accumulated_over_temp.saturating_add_signed(temp_diff.saturating_to_num());

        trace!(
            "Accumulated over temp: {}",
            defmt::Display2Format(&accumulated_over_temp)
        );

        accumulated_under_volts =
            accumulated_under_volts.saturating_add_signed(if volts < MIN_VOLTS {
                I32F32!(1.0)
            } else {
                I32F32!(-1.0)
            });

        trace!(
            "Accumulated under volts: {}",
            defmt::Display2Format(&accumulated_under_volts)
        );

        // TODO: model this and tune the values correctly
        const TICKS_PER_SEC: u64 = 100;
        let power_decrease = (accumulated_under_volts / U32F32::from_num(TICKS_PER_SEC))
            .saturating_add(accumulated_over_temp / U32F32::from_num(TICKS_PER_SEC));

        actual_level = actual_level.saturating_sub(power_decrease.int().saturating_to_num());

        if actual_level != previous_level {
            previous_level = actual_level;

            paths.set(actual_level).await;
        }

        if actual_level == 0 && desired_level == 0 {
            return;
        }

        maitake::time::sleep(Duration::from_hz(TICKS_PER_SEC).into()).await;
    }
}

// #[embassy_executor::task]
pub async fn power_task(
    hdr: pins::hdr!(),
    opamp_en: pins::opamp_en!(),
    boost_en: pins::boost_en!(),
    shunt_select: pins::shunt_select!(),
    dac: DAC1,
    dac_out: pins::dac!(),
    pa5: PA5,
) {
    let mut hdr = hdr.into_ref();
    let mut opamp_en = opamp_en.into_ref();
    let mut boost_en = boost_en.into_ref();
    let mut shunt_select = shunt_select.into_ref();
    let mut dac = dac.into_ref();
    let mut dac_out = dac_out.into_ref();
    let mut pa5 = pa5.into_ref();
    loop {
        POKE_POWER_CONTROLLER.wait().await;

        info!("Power task coming online");

        let (mut dac_ch1, mut dac_ch2) = Dac::new(
            dac.reborrow(),
            NoDma,
            NoDma,
            dac_out.reborrow(),
            pa5.reborrow(),
        )
        .split();
        dac_ch2.set(Value::Bit8(0));
        dac_ch1.set(Value::Bit8(0));
        dac_ch2.set_enable(false);
        dac_ch1.set_enable(false);
        dac_ch1.set_output_buffer(false);

        let paths = PowerPaths {
            dac: dac_ch1,
            hdr: Output::new(
                hdr.reborrow(),
                embassy_stm32::gpio::Level::Low,
                embassy_stm32::gpio::Speed::Low,
            ),
            opamp_en: Output::new(
                opamp_en.reborrow(),
                embassy_stm32::gpio::Level::Low,
                embassy_stm32::gpio::Speed::Low,
            ),
            boost_en: Output::new(
                boost_en.reborrow(),
                embassy_stm32::gpio::Level::Low,
                embassy_stm32::gpio::Speed::Low,
            ),
            shunt_select: Output::new(
                shunt_select.reborrow(),
                embassy_stm32::gpio::Level::Low,
                embassy_stm32::gpio::Speed::Low,
            ),
        };

        handle_on_state(paths).await;
    }
}
