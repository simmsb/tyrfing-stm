use cichlid::ColorRGB;
use embassy_stm32::{
    gpio::{Flex, OutputType, Pull},
    peripherals::{PA6, PA7, PB0, TIM3},
    time::Hertz,
    timer::{
        low_level::CountingMode,
        simple_pwm::{PwmPin, SimplePwm},
        Channel,
    },
    Peripheral as _,
};
use fixed::types::{I16F16, U32F32};
use fixed_macro::types::I16F16;

use crate::monitoring::Voltage;

#[derive(Clone, Copy)]
struct Rgb1Bit {
    r: bool,
    g: bool,
    b: bool,
}

impl Rgb1Bit {
    fn new(r: bool, g: bool, b: bool) -> Self {
        Self { r, g, b }
    }

    fn to_colorrgb(&self) -> ColorRGB {
        const ON_LEVEL: u8 = 40;

        ColorRGB::new(
            if self.r { ON_LEVEL } else { 0 },
            if self.g { ON_LEVEL } else { 0 },
            if self.b { ON_LEVEL } else { 0 },
        )
    }
}

struct AuxPwm<'a> {
    pwm: SimplePwm<'a, TIM3>,
}

impl<'a> AuxPwm<'a> {
    fn set(&mut self, c: ColorRGB) {
        let max_duty = self.pwm.get_max_duty();

        let calc_duty = |v: u8| {
            let r = fixed::types::U8F0::inv_lerp::<fixed::types::extra::U8>(
                v.into(),
                0.into(),
                255.into(),
            );

            U32F32::lerp(r.into(), U32F32::ZERO, max_duty.into())
                .int()
                .to_num()
        };

        for (c, v) in [
            (Channel::Ch1, c.r),
            (Channel::Ch2, c.g),
            (Channel::Ch3, c.b),
        ] {
            if v != 0 {
                self.pwm.enable(c);
            } else {
                self.pwm.disable(c);
            }
            self.pwm.set_duty(c, calc_duty(v));
        }
    }
}

struct AuxLow<'a> {
    r: Flex<'a>,
    g: Flex<'a>,
    b: Flex<'a>,
}

impl<'a> AuxLow<'a> {
    fn set(&mut self, c: Rgb1Bit) {
        self.r.set_as_input(if c.r { Pull::Up } else { Pull::Down });
        self.g.set_as_input(if c.g { Pull::Up } else { Pull::Down });
        self.b.set_as_input(if c.b { Pull::Up } else { Pull::Down });
    }
}

async fn transition_to_pwm<'a>(leds: &mut AuxPwm<'a>, prior: ColorRGB, target: ColorRGB) {
    leds.set(prior);

    for i in (0..255u8).step_by(20) {
        let mut c = prior;
        c.blend(target, i);
        leds.set(c);

        embassy_time::Timer::after_millis(16).await;
    }
}

fn hue_to_rgb(hue: u8) -> ColorRGB {
    let hue = cichlid::math::scale_u8(hue, 191);

    const HSV_SECTION_4: u8 = 0x40;

    let section = hue / HSV_SECTION_4;
    let offset = hue % HSV_SECTION_4;

    let brightness_floor = 0;

    let ramp_up = offset;
    let ramp_down = HSV_SECTION_4 - offset;

    let amplitude = 191_u16;

    let ramp_up_amp_adj = ((ramp_up as u16 * amplitude) / (256 / 4)) as u8;
    let ramp_down_amp_adj = ((ramp_down as u16 * amplitude) / (256 / 4)) as u8;

    let ramp_up = ramp_up.saturating_add(ramp_up_amp_adj);
    let ramp_down = ramp_down.saturating_add(ramp_down_amp_adj);

    match section {
        0 => ColorRGB::new(ramp_down, ramp_up, brightness_floor),
        1 => ColorRGB::new(brightness_floor, ramp_down, ramp_up),
        _ => ColorRGB::new(ramp_up, brightness_floor, ramp_down),
    }
}

async fn rainbow_aux<'a>(leds: &mut AuxPwm<'a>, prior: ColorRGB) -> ColorRGB {
    let mut h = 0u8;
    let target_startup_colour = hue_to_rgb(h);

    transition_to_pwm(leds, prior, target_startup_colour).await;

    loop {
        let rgb = hue_to_rgb(h);

        if !crate::state::is_on().await {
            return rgb;
        }

        leds.set(rgb);

        h = h.wrapping_add(1);

        embassy_time::Timer::after_millis(16).await;
    }
}

fn volts_to_rgb(volts: Voltage) -> ColorRGB {
    let max = Voltage(I16F16!(4.2));
    let min = Voltage(I16F16!(3.4));

    // red
    let min_hue = 0u8;
    // magenta
    let max_hue = 212u8;

    let hue = if volts < min {
        min_hue
    } else if volts > max {
        max_hue
    } else {
        fixed::types::I16F16::inv_lerp::<fixed::types::extra::U16>(volts.0, min.0, max.0)
            .lerp(I16F16::from_num(min_hue), I16F16::from_num(max_hue))
            .to_num()
    };

    hue_to_rgb(hue)
}

fn volts_to_1bit_rgb(volts: Voltage) -> Rgb1Bit {
    if volts > Voltage(I16F16!(4.1)) {
        Rgb1Bit::new(true, false, true)
    } else if volts > Voltage(I16F16!(3.9)) {
        Rgb1Bit::new(false, false, true)
    } else if volts > Voltage(I16F16!(3.7)) {
        Rgb1Bit::new(false, true, true)
    } else if volts > Voltage(I16F16!(3.5)) {
        Rgb1Bit::new(false, true, false)
    } else if volts > Voltage(I16F16!(3.3)) {
        Rgb1Bit::new(true, true, false)
    } else {
        Rgb1Bit::new(true, false, false)
    }
}

async fn voltage_high_aux<'a>(leds: &mut AuxPwm<'a>, prior: ColorRGB) -> ColorRGB {
    let volts = *crate::monitoring::VOLTAGE.lock().await;
    let target_startup_colour = volts_to_rgb(volts);

    transition_to_pwm(leds, prior, target_startup_colour).await;

    loop {
        let volts = *crate::monitoring::VOLTAGE.lock().await;
        let rgb = volts_to_rgb(volts);

        if crate::state::is_on().await {
            return rgb;
        }

        leds.set(rgb);

        embassy_time::Timer::after_millis(64).await;
    }
}

async fn transition_to_low_voltage_aux<'a>(leds: &mut AuxPwm<'a>, prior: ColorRGB) -> ColorRGB {
    let volts = *crate::monitoring::VOLTAGE.lock().await;
    let target_startup_colour = volts_to_1bit_rgb(volts).to_colorrgb();

    transition_to_pwm(leds, prior, target_startup_colour).await;

    target_startup_colour
}

async fn voltage_low_aux<'a>(leds: &mut AuxLow<'a>) -> ColorRGB {
    loop {
        let volts = *crate::monitoring::VOLTAGE.lock().await;
        let rgb = volts_to_1bit_rgb(volts);
        leds.set(rgb);

        if crate::state::is_unlocked().await {
            return rgb.to_colorrgb();
        }

        embassy_time::Timer::after_secs(1).await;
    }
}

#[embassy_executor::task]
pub async fn aux_task(timer: TIM3, r: PA6, g: PA7, b: PB0) {
    let mut timer = timer.into_ref();
    let mut r = r.into_ref();
    let mut g = g.into_ref();
    let mut b = b.into_ref();
    let mut prior_colour = ColorRGB::Black;

    loop {
        if crate::state::is_unlocked().await {
            let pwm = SimplePwm::new(
                timer.reborrow(),
                Some(PwmPin::new_ch1(r.reborrow(), OutputType::PushPull)),
                Some(PwmPin::new_ch2(g.reborrow(), OutputType::PushPull)),
                Some(PwmPin::new_ch3(b.reborrow(), OutputType::PushPull)),
                None,
                Hertz::khz(20),
                CountingMode::EdgeAlignedUp,
            );
            let mut aux = AuxPwm { pwm };

            loop {
                if !crate::state::is_unlocked().await {
                    break;
                }

                if crate::state::is_on().await {
                    prior_colour = rainbow_aux(&mut aux, prior_colour).await;
                } else {
                    prior_colour = voltage_high_aux(&mut aux, prior_colour).await;
                }
            }

            prior_colour = transition_to_low_voltage_aux(&mut aux, prior_colour).await;
        } else {
            let mut aux = AuxLow {
                r: Flex::new(r.reborrow()),
                g: Flex::new(g.reborrow()),
                b: Flex::new(b.reborrow()),
            };

            prior_colour = voltage_low_aux(&mut aux).await;
        }
    }
}
