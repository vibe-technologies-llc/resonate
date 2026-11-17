use std::{mem, time::Duration};

use gpui::{Context, EventEmitter, Task};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Beat;

#[derive(Default)]
pub(crate) struct Pulse {
    drawn: bool,
    beating: Option<Task<()>>,
}

impl Pulse {
    pub(crate) fn beats_while_drawn<T: EventEmitter<Beat> + 'static>(
        &mut self,
        every: Duration,
        within: fn(&mut T) -> &mut Self,
        cx: &mut Context<T>,
    ) {
        self.drawn = true;
        if self.beating.is_some() {
            return;
        }
        self.beating = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(every).await;
                let kept = this.update(cx, |this, cx| {
                    let pulse = within(this);
                    if mem::take(&mut pulse.drawn) {
                        cx.emit(Beat);
                        return true;
                    }
                    pulse.beating = None;
                    false
                });
                if !matches!(kept, Ok(true)) {
                    return;
                }
            }
        }));
    }

    pub(crate) fn stop(&mut self) {
        self.drawn = false;
        self.beating = None;
    }

    #[cfg(test)]
    pub(crate) const fn is_beating(&self) -> bool {
        self.beating.is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui::{AppContext as _, Context, Entity, EventEmitter, TestAppContext};

    use super::{Beat, Pulse};

    const EVERY: Duration = Duration::from_millis(500);

    struct Beaten {
        pulse: Pulse,
        redrawn: usize,
    }

    impl EventEmitter<Beat> for Beaten {}

    impl Beaten {
        fn drawn(&mut self, cx: &mut Context<Self>) {
            self.pulse
                .beats_while_drawn(EVERY, |beaten: &mut Self| &mut beaten.pulse, cx);
        }
    }

    fn counted(cx: &mut TestAppContext) -> Entity<Beaten> {
        let beaten = cx.new(|_| Beaten {
            pulse: Pulse::default(),
            redrawn: 0,
        });
        let counting = beaten.clone();
        cx.update(|cx| {
            cx.subscribe(&beaten, move |_, _: &Beat, cx| {
                counting.update(cx, |beaten, _| beaten.redrawn += 1);
            })
            .detach();
        });
        beaten
    }

    fn pass(cx: &mut TestAppContext, ticks: u32) {
        for _ in 0..ticks {
            cx.executor().advance_clock(EVERY);
            cx.run_until_parked();
        }
    }

    fn redrawn(beaten: &Entity<Beaten>, cx: &TestAppContext) -> usize {
        beaten.read_with(cx, |beaten, _| beaten.redrawn)
    }

    #[gpui::test]
    fn a_pulse_beats_while_it_is_drawn_each_beat_and_stops_after_the_first_beat_it_was_not(
        cx: &mut TestAppContext,
    ) {
        let beaten = counted(cx);

        beaten.update(cx, |beaten, cx| beaten.drawn(cx));
        pass(cx, 1);
        beaten.update(cx, |beaten, cx| beaten.drawn(cx));
        pass(cx, 1);
        beaten.update(cx, |beaten, cx| beaten.drawn(cx));

        assert_eq!(redrawn(&beaten, cx), 2);
        assert!(beaten.read_with(cx, |beaten, _| beaten.pulse.is_beating()));

        pass(cx, 1);

        assert_eq!(redrawn(&beaten, cx), 3);

        pass(cx, 4);

        assert_eq!(redrawn(&beaten, cx), 3);
        assert!(!beaten.read_with(cx, |beaten, _| beaten.pulse.is_beating()));
    }

    #[gpui::test]
    fn a_pulse_stopped_by_hand_beats_no_more_and_beats_again_when_drawn(cx: &mut TestAppContext) {
        let beaten = counted(cx);

        beaten.update(cx, |beaten, cx| beaten.drawn(cx));
        beaten.update(cx, |beaten, _| beaten.pulse.stop());
        pass(cx, 3);

        assert_eq!(redrawn(&beaten, cx), 0);

        beaten.update(cx, |beaten, cx| beaten.drawn(cx));
        pass(cx, 1);

        assert_eq!(redrawn(&beaten, cx), 1);
    }
}
