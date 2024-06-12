use core::{cell::Cell, future::Future, ops::ControlFlow};

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, signal::Signal};
use maitake::time::{timeout, Duration, Instant};

use embassy_futures::select;

use crate::{
    click::{ButtonEvent, ButtonState, BUTTON_EVENTS, LOCKOUT_BUTTON_STATES},
    power::blink,
};

const DEFAULT_LEVEL: u8 = 27;

enum Handled {
    Handled,
    Exit,
}

struct Handler<F> {
    inner: F,
}

impl Handler<()> {
    fn empty() -> Handler<impl Handle> {
        Handler {
            inner: |_| async { ControlFlow::Continue(()) },
        }
    }
}

trait Handle {
    async fn handle(&mut self, e: ButtonEvent) -> ControlFlow<Handled>;
}

impl<F, Fut> Handle for F
where
    F: FnMut(ButtonEvent) -> Fut,
    Fut: Future<Output = ControlFlow<Handled>>,
{
    async fn handle(&mut self, e: ButtonEvent) -> ControlFlow<Handled> {
        (self)(e).await
    }
}

impl<H0: Handle, H1: Handle> Handle for (H0, H1) {
    async fn handle(&mut self, e: ButtonEvent) -> ControlFlow<Handled> {
        self.0.handle(e).await?;
        self.1.handle(e).await
    }
}

impl<H: Handle> Handler<H> {
    async fn run(&mut self) {
        loop {
            let r = self.inner.handle(BUTTON_EVENTS.wait().await).await;
            if let ControlFlow::Break(Handled::Exit) = r {
                break;
            }
        }
    }

    fn and<Hother: Handle>(self, other: Hother) -> Handler<(H, Hother)> {
        Handler {
            inner: (self.inner, other),
        }
    }
}

struct Given<F> {
    needs: ButtonEvent,
    inner: F,
}

impl<F> Given<F> {
    fn new(needs: ButtonEvent, callback: F) -> Self {
        Self {
            needs,
            inner: callback,
        }
    }
}

impl<F, Fut> Handle for Given<F>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ControlFlow<Handled>>,
{
    async fn handle(&mut self, e: ButtonEvent) -> ControlFlow<Handled> {
        if e == self.needs {
            (self.inner)().await
        } else {
            ControlFlow::Continue(())
        }
    }
}

struct StandardAdjustment<Op> {
    last_hold_release: Instant,
    inner: Op,
}

impl<Op> StandardAdjustment<Op>
where
    Op: FnMut(i8),
{
    fn new(op: Op) -> Self {
        Self {
            last_hold_release: Instant::now(),
            inner: op,
        }
    }
}

impl<Op> Handle for StandardAdjustment<Op>
where
    Op: FnMut(i8),
{
    async fn handle(&mut self, e: ButtonEvent) -> ControlFlow<Handled> {
        match e {
            ButtonEvent::Click1 => ControlFlow::Break(Handled::Exit),
            ButtonEvent::Hold1 => {
                let direction = if self.last_hold_release.elapsed() > Duration::from_millis(500) {
                    1
                } else {
                    -1
                };
                loop {
                    if timeout(Duration::from_millis(16), BUTTON_EVENTS.wait())
                        .await
                        .is_err()
                    {
                        (self.inner)(direction);
                    } else {
                        break;
                    }
                }
                if direction == 1 {
                    self.last_hold_release = Instant::now();
                }

                ControlFlow::Break(Handled::Handled)
            }
            ButtonEvent::Hold2 => {
                loop {
                    if timeout(Duration::from_millis(16), BUTTON_EVENTS.wait())
                        .await
                        .is_err()
                    {
                        (self.inner)(-1);
                    } else {
                        break;
                    }
                }

                ControlFlow::Break(Handled::Handled)
            }

            _ => ControlFlow::Continue(()),
        }
    }
}

struct StateHandler<const GRADUAL: bool, S> {
    state: Cell<S>,
    signal: Signal<ThreadModeRawMutex, ()>,
}

impl<S> StateHandler<true, S> {
    fn gradual(state: S) -> Self {
        StateHandler {
            state: Cell::new(state),
            signal: Signal::new(),
        }
    }
}

impl<S> StateHandler<false, S> {
    fn instant(state: S) -> Self {
        StateHandler {
            state: Cell::new(state),
            signal: Signal::new(),
        }
    }
}

#[allow(unused)]
impl<const GRADUAL: bool, S: Copy> StateHandler<GRADUAL, S> {
    fn set(&self, state: S) {
        self.state.set(state);
        self.signal.signal(());
    }

    fn modify(&self, f: impl FnOnce(S) -> S) {
        self.state.set(f(self.state.get()));
        self.signal.signal(());
    }

    fn tick(&self) {
        self.signal.signal(());
    }

    fn get(&self) -> S {
        self.state.get()
    }

    async fn run(&self, mapper: impl Fn(S) -> u8) {
        loop {
            let level = mapper(self.state.get());

            if GRADUAL {
                crate::power::set_level_gradual(level).await;
            } else {
                crate::power::set_level(level).await;
            }

            let _ = self.signal.wait().await;
        }
    }
}

async fn with_torch_on<O>(fut: impl Future<Output = O>) -> O {
    crate::state::set_on(true).await;

    let r = fut.await;

    crate::power::set_level_gradual(0).await;
    crate::state::set_on(false).await;

    r
}

// #[embassy_executor::task]
pub async fn torch_ui_task() {
    let mut saved_level = DEFAULT_LEVEL;

    loop {
        let unlocked = crate::state::is_unlocked().await;

        if unlocked {
            let evt = timeout(Duration::from_secs(60 * 3), BUTTON_EVENTS.wait()).await;
            let Ok(evt) = evt else {
                blink(1).await;
                crate::state::set_unlocked(false).await;
                continue;
            };
            match evt {
                ButtonEvent::Click1 | ButtonEvent::Hold1 => {
                    saved_level = with_torch_on(on_ramping(if evt == ButtonEvent::Click1 {
                        saved_level
                    } else {
                        DEFAULT_LEVEL
                    }))
                    .await;
                }
                #[cfg(feature = "mode_fade")]
                ButtonEvent::Hold2 => {
                    with_torch_on(on_fadeout()).await;
                }
                #[cfg(feature = "mode_strobe")]
                ButtonEvent::Hold3 => {
                    with_torch_on(on_strobe()).await;
                }
                #[cfg(feature = "mode_croak")]
                ButtonEvent::Hold4 => {
                    with_torch_on(on_croak()).await;
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
                    crate::power::set_level(30).await;
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
            maitake::time::sleep(period.get().into()).await;
            crate::power::set_level(if on { level.get() } else { 1 });
            crate::power::poke_power_controller();
            on = !on;
        }
    };

    let control = async {
        let mut last_hold_release = Instant::now();
        loop {
            match BUTTON_EVENTS.wait().await {
                ButtonEvent::Click1 => {
                    return;
                }
                ButtonEvent::Hold1 => {
                    let direction = if last_hold_release.elapsed() > Duration::from_millis(500) {
                        1
                    } else {
                        -1
                    };
                    loop {
                        if timeout(Some(Duration::from_millis(200)), BUTTON_EVENTS.wait())
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
                ButtonEvent::Hold2 => loop {
                    if timeout(Some(Duration::from_millis(100)), BUTTON_EVENTS.wait())
                        .await
                        .is_err()
                    {
                        level.set(level.get().saturating_sub(4));
                    } else {
                        break;
                    }
                },
                ButtonEvent::Hold3 => loop {
                    if timeout(Some(Duration::from_millis(100)), BUTTON_EVENTS.wait())
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
                ButtonEvent::Hold4 => loop {
                    if timeout(Some(Duration::from_millis(100)), BUTTON_EVENTS.wait())
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
    #[derive(Copy, Clone)]
    struct State {
        level: u8,
        expiry: Instant,
    }

    let state = StateHandler::gradual(State {
        level: DEFAULT_LEVEL,
        expiry: Instant::now() + Duration::from_secs(60 * 4),
    });

    let fade = async {
        loop {
            maitake::time::sleep(core::time::Duration::from_millis(100)).await;

            if state
                .get()
                .expiry
                .checked_duration_since(Instant::now())
                .is_none()
            {
                break;
            };

            state.tick();
        }
    };

    let control = async {
        Handler::empty()
            .and(StandardAdjustment::new(|d| {
                state.modify(|State { level, expiry }| State {
                    level: level.saturating_add_signed(d),
                    expiry,
                })
            }))
            .and(Given::new(ButtonEvent::Hold3, || async {
                loop {
                    if timeout(Duration::from_millis(500), BUTTON_EVENTS.wait())
                        .await
                        .is_err()
                    {
                        blink(1).await;
                        state.modify(|State { level, expiry }| State {
                            level,
                            expiry: expiry + Duration::from_secs(60),
                        })
                    } else {
                        break;
                    }
                }

                ControlFlow::Break(Handled::Handled)
            }))
            .run()
            .await;
    };

    let level_fut = state.run(|State { level, expiry }| {
        let Some(time_left) = expiry.checked_duration_since(Instant::now()) else {
            return 0;
        };

        if time_left > Duration::from_secs(60 * 4) {
            level
        } else {
            ((time_left.as_millis() as u32 * level as u32)
                / Duration::from_secs(60 * 4).as_millis() as u32) as u8
        }
    });

    embassy_futures::select::select3(fade, control, level_fut).await;
}

#[cfg(feature = "mode_croak")]
async fn on_croak() {
    #[derive(Copy, Clone)]
    struct State {
        level: u8,
        on: bool,
    }

    let state = StateHandler::instant(State {
        level: DEFAULT_LEVEL,
        on: false,
    });

    let croaker = async {
        loop {
            for x in small_morse::encode("croak") {
                state.modify(|State { level, .. }| State {
                    level,
                    on: x.state == small_morse::State::On,
                });

                maitake::time::sleep(core::time::Duration::from_millis(x.duration as u64 * 300))
                    .await;
            }
        }
    };

    let control = async {
        Handler::empty()
            .and(StandardAdjustment::new(|d| {
                state.modify(|State { level, on }| State {
                    level: level.saturating_add_signed(d),
                    on,
                })
            }))
            .run()
            .await;
    };

    let level_fut = state.run(|State { level, on }| if on { level } else { 1 });

    embassy_futures::select::select3(croaker, control, level_fut).await;
}

async fn on_ramping(level: u8) -> u8 {
    #[derive(Copy, Clone)]
    struct State {
        level: u8,
        level_before_boost: u8,
    }

    let state = StateHandler::gradual(State {
        level,
        level_before_boost: level,
    });

    let control = async {
        Handler::empty()
            .and(StandardAdjustment::new(|d| {
                state.modify(
                    |State {
                         level,
                         level_before_boost,
                     }| State {
                        level: level.saturating_add_signed(d),
                        level_before_boost,
                    },
                )
            }))
            .and(Given::new(ButtonEvent::Click2, || async {
                state.modify(
                    |State {
                         level,
                         level_before_boost,
                     }| {
                        if level == 255 {
                            State {
                                level: level_before_boost,
                                level_before_boost,
                            }
                        } else {
                            State {
                                level: 255,
                                level_before_boost: level,
                            }
                        }
                    },
                );

                ControlFlow::Break(Handled::Handled)
            }))
            .run()
            .await;
    };

    let level_fut = state.run(|State { level, .. }| level);

    embassy_futures::select::select(control, level_fut).await;

    state.get().level
}
