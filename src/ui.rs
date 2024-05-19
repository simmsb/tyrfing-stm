use embassy_time::{with_timeout, Duration, Instant};

use embassy_futures::select;

use crate::{
    click::{ButtonEvent, ButtonState, BUTTON_EVENTS, LOCKOUT_BUTTON_STATES},
    power::blink,
};

const DEFAULT_LEVEL: u8 = 27;

#[embassy_executor::task]
pub async fn torch_ui_task() {
    let mut saved_level = DEFAULT_LEVEL;

    loop {
        let unlocked = crate::state::is_unlocked().await;

        if unlocked {
            let evt = with_timeout(
                Duration::from_secs(60 * 3),
                BUTTON_EVENTS.wait(),
            )
            .await;
            let Ok(evt) = evt else {
                blink(1).await;
                crate::state::set_unlocked(false).await;
                continue;
            };
            match evt {
                ButtonEvent::Click1 | ButtonEvent::Hold1 => {
                    saved_level = on_ramping(if evt == ButtonEvent::Click1 {
                        saved_level
                    } else {
                        DEFAULT_LEVEL
                    })
                    .await;
                }
                #[cfg(feature = "mode_fade")]
                ButtonEvent::Hold2 => {
                    on_fadeout().await;
                }
                #[cfg(feature = "mode_strobe")]
                ButtonEvent::Hold3 => {
                    on_strobe().await;
                }
                ButtonEvent::Click4 => {
                    blink(1).await;
                    crate::state::set_unlocked(false).await;
                }
                _ => {}
            }
        } else {
            let evt = select::select(BUTTON_EVENTS.wait(), LOCKOUT_BUTTON_STATES.wait()).await;
            match evt {
                select::Either::Second(ButtonState::Press) => {
                    crate::power::set_level(40).await;
                }
                select::Either::Second(ButtonState::Depress) => {
                    crate::power::set_level(0).await;
                }
                select::Either::First(ButtonEvent::Click3) => {
                    blink(1).await;
                    crate::state::set_unlocked(true).await;
                    saved_level = DEFAULT_LEVEL;
                }
                _ => {}
            }
        }
    }
}

#[cfg(feature = "mode_strobe")]
async fn on_strobe() {
    use core::cell::Cell;

    let level = Cell::new(DEFAULT_LEVEL);
    let period = Cell::new(Duration::from_hz(10));

    let strobe = async {
        let mut on = true;
        loop {
            embassy_time::Timer::after(period.get()).await;
            crate::power::set_level(if on { level.get() } else { 1 });
            crate::power::poke_power_controller();
            on = !on;
        }
    };

    let control = async {
        let mut last_hold_release = Instant::now();
        loop {
            match BUTTON_EVENTS.wait().await {
                crate::click::ButtonEvent::Click1 => {
                    return;
                }
                crate::click::ButtonEvent::Hold1 => {
                    let direction = if last_hold_release.elapsed() > Duration::from_millis(500) {
                        1
                    } else {
                        -1
                    };
                    loop {
                        if with_timeout(Some(Duration::from_millis(200)), BUTTON_EVENTS.wait())
                            .await
                            .is_err()
                        {
                            level.set(level.get().saturating_add_signed(direction * 4));
                        } else {
                            break;
                        }
                    }
                    if direction == 1 {
                        last_hold_release = Instant::now();
                    }
                }
                crate::click::ButtonEvent::Hold2 => loop {
                    if with_timeout(Some(Duration::from_millis(100)), BUTTON_EVENTS.wait())
                        .await
                        .is_err()
                    {
                        level.set(level.get().saturating_sub(4));
                    } else {
                        break;
                    }
                },
                crate::click::ButtonEvent::Hold3 => loop {
                    if with_timeout(Some(Duration::from_millis(100)), BUTTON_EVENTS.wait())
                        .await
                        .is_err()
                    {
                        period.set(Duration::from_ticks(
                            period.get().as_ticks().saturating_sub(10),
                        ));
                    } else {
                        break;
                    }
                },
                crate::click::ButtonEvent::Hold4 => loop {
                    if with_timeout(Some(Duration::from_millis(100)), BUTTON_EVENTS.wait())
                        .await
                        .is_err()
                    {
                        period.set(Duration::from_ticks(
                            period.get().as_ticks().saturating_add(10),
                        ));
                    } else {
                        break;
                    }
                },
                _ => {}
            }
        }
    };

    embassy_futures::select::select(strobe, control).await;

    crate::power::set_level_gradual(0).await;
}

#[cfg(feature = "mode_fade")]
async fn on_fadeout() {
    use core::cell::Cell;

    let level = Cell::new(DEFAULT_LEVEL);
    let expiry = Cell::new(Instant::now() + Duration::from_secs(60 * 4));

    let fade = async {
        loop {
            embassy_time::Timer::after_millis(100).await;

            let Some(time_left) = expiry.get().checked_duration_since(Instant::now()) else {
                break;
            };

            let brightness = if time_left > Duration::from_secs(60 * 4) {
                level.get()
            } else {
                let remaining = fixed::types::U64F0::from_num(time_left.as_ticks())
                    .inv_lerp::<fixed::types::extra::U64>(
                        0u32.into(),
                        Duration::from_secs(60 * 4).as_ticks().into(),
                    )
                    .lerp(0u64.into(), 255u64.into())
                    .saturating_to_num::<u8>();
                cichlid::math::scale_u8(level.get(), remaining)
            };

            crate::power::set_level_gradual(brightness).await;
        }
    };

    let control = async {
        let mut last_hold_release = Instant::now();
        loop {
            match BUTTON_EVENTS.wait().await {
                crate::click::ButtonEvent::Click1 => {
                    return;
                }
                crate::click::ButtonEvent::Hold1 => {
                    let direction = if last_hold_release.elapsed() > Duration::from_millis(500) {
                        1
                    } else {
                        -1
                    };
                    loop {
                        if with_timeout(Duration::from_millis(16), BUTTON_EVENTS.wait())
                            .await
                            .is_err()
                        {
                            level.set(level.get().saturating_add_signed(direction));
                        } else {
                            break;
                        }
                    }
                    if direction == 1 {
                        last_hold_release = Instant::now();
                    }
                }
                crate::click::ButtonEvent::Hold2 => loop {
                    if with_timeout(Duration::from_millis(16), BUTTON_EVENTS.wait())
                        .await
                        .is_err()
                    {
                        level.set(level.get().saturating_sub(1));
                    } else {
                        break;
                    }
                },
                crate::click::ButtonEvent::Hold3 => loop {
                    if with_timeout(Duration::from_millis(500), BUTTON_EVENTS.wait())
                        .await
                        .is_err()
                    {
                        blink(1).await;
                        expiry.set(expiry.get() + Duration::from_secs(60));
                    } else {
                        break;
                    }
                },
                _ => {}
            }
        }
    };

    embassy_futures::select::select(fade, control).await;

    crate::power::set_level_gradual(0).await;
}

async fn on_ramping(level: u8) -> u8 {
    let mut level_before_boost = level;
    let mut level = level;

    let mut last_hold_release = Instant::now();

    loop {
        crate::power::set_level_gradual(level).await;

        match BUTTON_EVENTS.wait().await {
            crate::click::ButtonEvent::Click1 => {
                crate::power::set_level_gradual(0).await;
                return level;
            }
            crate::click::ButtonEvent::Click2 => {
                if level == 255 {
                    level = level_before_boost;
                } else {
                    level_before_boost = level;
                    level = 255;
                }
            }
            crate::click::ButtonEvent::Hold1 => {
                let direction = if last_hold_release.elapsed() > Duration::from_millis(500) {
                    1
                } else {
                    -1
                };
                loop {
                    if with_timeout(Duration::from_millis(16), BUTTON_EVENTS.wait())
                        .await
                        .is_err()
                    {
                        level = level.saturating_add_signed(direction);
                        crate::power::set_level_gradual(level).await;
                    } else {
                        break;
                    }
                }
                if direction == 1 {
                    last_hold_release = Instant::now();
                }
            }
            crate::click::ButtonEvent::Hold2 => loop {
                if with_timeout(Duration::from_millis(16), BUTTON_EVENTS.wait())
                    .await
                    .is_err()
                {
                    level = level.saturating_sub(1);
                    crate::power::set_level_gradual(level).await;
                } else {
                    break;
                }
            },
            _ => {}
        }
    }
}
