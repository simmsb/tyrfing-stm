use defmt::info;
use embassy_stm32::{
    exti::ExtiInput, gpio::Output, peripherals::{EXTI8, PA8, PC15}
};
#[derive(PartialEq)]
pub enum ButtonEvent {
    Click1,
    Click2,
    Click3,
    Click4,
    Click5,
    Click6,
    Click7,

    Hold1,
    Hold2,
    Hold3,
    Hold4,
    Hold5,
    Hold6,
    Hold7,

    HoldEnd,
}

impl ButtonEvent {
    pub fn click_from_count(n: u8) -> Self {
        match n {
            1 => Self::Click1,
            2 => Self::Click2,
            3 => Self::Click3,
            4 => Self::Click4,
            5 => Self::Click5,
            6 => Self::Click6,
            _ => Self::Click7,
        }
    }

    pub fn hold_from_count(n: u8) -> Self {
        match n {
            1 => Self::Hold1,
            2 => Self::Hold2,
            3 => Self::Hold3,
            4 => Self::Hold4,
            5 => Self::Hold5,
            6 => Self::Hold6,
            _ => Self::Hold7,
        }
    }
}

pub static BUTTON_EVENTS: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::ThreadModeRawMutex,
    ButtonEvent,
> = embassy_sync::signal::Signal::new();

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonState {
    Depress,
    Press,
}

static BUTTON_STATES: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::ThreadModeRawMutex,
    ButtonState,
> = embassy_sync::signal::Signal::new();

pub static LOCKOUT_BUTTON_STATES: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::ThreadModeRawMutex,
    ButtonState,
> = embassy_sync::signal::Signal::new();

#[derive(Clone, Copy)]
pub enum EventGenState {
    FirstClick,
    ForHigh { clicks: u8 },
    ForLow { clicks: u8 },
    HoldFinish,
}

// #[embassy_executor::task]
pub async fn debouncer_task(t: PA8, ch: EXTI8, led: PC15) {
    let mut t = ExtiInput::new(t, ch, embassy_stm32::gpio::Pull::Up);
    let mut led = Output::new(led, embassy_stm32::gpio::Level::High, embassy_stm32::gpio::Speed::Low);

    loop {
        led.set_high();

        info!("Button pin: {}", t.is_high());

        t.wait_for_low().await;
        let v = t.is_low();

        // if the button isn't pressed, abort
        if !v {
            continue;
        }

        led.set_low();

        maitake::time::sleep(core::time::Duration::from_millis(16)).await;

        // if the button is still pressed after 16ms, consider it debounced and pressed
        if t.is_low() {
            BUTTON_STATES.signal(ButtonState::Press);
            LOCKOUT_BUTTON_STATES.signal(ButtonState::Press);
        } else {
            continue;
        }

        // once pressed, we poll the button for depresses since sometimes the
        // edge interrupt can be missed
        loop {
            maitake::time::sleep(core::time::Duration::from_millis(16)).await;
            // if the button is still pressed, do nothing
            if t.is_low() {
                continue;
            }

            maitake::time::sleep(core::time::Duration::from_millis(16)).await;

            // if the button has been depressed for two cycles, consider it
            // debounced and depressed
            if t.is_high() {
                BUTTON_STATES.signal(ButtonState::Depress);
                LOCKOUT_BUTTON_STATES.signal(ButtonState::Depress);
                break;
            }
        }
    }
}

// #[embassy_executor::task]
pub async fn event_generator_task() {
    let mut state = EventGenState::FirstClick;
    loop {
        let (wait_until, expecting) = match state {
            EventGenState::FirstClick => (None, ButtonState::Press),
            EventGenState::ForHigh { .. } => (Some(core::time::Duration::from_millis(300)), ButtonState::Press),
            EventGenState::ForLow { .. } => {
                (Some(core::time::Duration::from_millis(300)), ButtonState::Depress)
            }
            EventGenState::HoldFinish => (None, ButtonState::Depress),
        };

        let r = if let Some(timeout) = wait_until {
            maitake::time::timeout(timeout,  BUTTON_STATES.wait()).await
        } else {
            Ok(BUTTON_STATES.wait().await)
        };

        let r = match r {
            Ok(state) if state == expecting => true,
            Ok(_) => {
                state = EventGenState::FirstClick;
                continue;
            }
            Err(_) => false,
        };

        // r: true if pressed, false if held

        let (state_, evt) = match state {
            EventGenState::FirstClick => (EventGenState::ForLow { clicks: 1 }, None),
            EventGenState::ForHigh { clicks } => {
                if r {
                    (EventGenState::ForLow { clicks: clicks + 1 }, None)
                } else {
                    (
                        EventGenState::FirstClick,
                        Some(ButtonEvent::click_from_count(clicks)),
                    )
                }
            }
            EventGenState::ForLow { clicks } => {
                if r {
                    (EventGenState::ForHigh { clicks }, None)
                } else {
                    (
                        EventGenState::HoldFinish,
                        Some(ButtonEvent::hold_from_count(clicks)),
                    )
                }
            }
            EventGenState::HoldFinish => (EventGenState::FirstClick, Some(ButtonEvent::HoldEnd)),
        };
        state = state_;
        if let Some(evt) = evt {
            BUTTON_EVENTS.signal(evt);
        }
    }
}
