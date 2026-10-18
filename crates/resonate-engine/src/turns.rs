use std::{
    collections::VecDeque,
    iter,
    time::{Duration, Instant},
};

use resonate_core::Gain;

const TURNS_REMEMBERED: usize = 8;
const TURNS_HELD_AT_MOST: usize = 512;
const ECHOES_ARRIVE_WITHIN: Duration = Duration::from_secs(2);
const ONE_LEVEL_WITHIN: f32 = 1e-5;
const A_DEVICES_HALF_STEP_DB: f32 = 0.75;

pub(crate) struct Turns {
    sent: VecDeque<(Gain, Instant)>,
}

impl Turns {
    pub(crate) fn new() -> Self {
        Self {
            sent: VecDeque::with_capacity(TURNS_REMEMBERED),
        }
    }

    pub(crate) fn sent(&mut self, gain: Gain) {
        self.sent.push_back((gain, Instant::now()));
        while self.sent.len() > TURNS_HELD_AT_MOST {
            self.sent.pop_front();
        }
        while self.sent.len() > TURNS_REMEMBERED
            && self
                .sent
                .front()
                .is_some_and(|(_, at)| at.elapsed() > ECHOES_ARRIVE_WITHIN)
        {
            self.sent.pop_front();
        }
    }

    pub(crate) fn echo(&self, heard: Gain, ours: Gain) -> bool {
        iter::once(ours)
            .chain(self.sent.iter().map(|(gain, _)| *gain))
            .any(|sent| rounds_to(heard, sent))
    }
}

fn rounds_to(heard: Gain, sent: Gain) -> bool {
    if heard.get() < ONE_LEVEL_WITHIN || sent.get() < ONE_LEVEL_WITHIN {
        return (heard.get() - sent.get()).abs() < ONE_LEVEL_WITHIN;
    }
    (heard.to_decibels().get() - sent.to_decibels().get()).abs() <= A_DEVICES_HALF_STEP_DB
}

#[cfg(test)]
mod tests {
    use resonate_core::Decibels;

    use super::*;

    fn at(db: f32) -> Gain {
        Decibels::new(db).expect("a finite level").to_gain()
    }

    #[test]
    fn a_level_rounded_to_the_devices_step_is_the_echo_of_the_one_sent() {
        let mut turns = Turns::new();
        turns.sent(at(-18.06));

        assert!(turns.echo(at(-18.5), at(-6.0)));
        assert!(!turns.echo(at(-21.0), at(-6.0)));
    }

    #[test]
    fn every_turn_of_a_fast_drag_is_still_known_when_its_echo_comes_back() {
        let mut turns = Turns::new();
        let first = at(-40.0);
        turns.sent(first);
        for step in 1..=TURNS_REMEMBERED * 4 {
            turns.sent(at(-40.0 + step as f32 * 0.5));
        }

        assert!(turns.echo(first, at(0.0)));
    }

    #[test]
    fn silence_is_the_echo_of_silence_alone() {
        let mut turns = Turns::new();
        turns.sent(Gain::SILENT);

        assert!(turns.echo(Gain::SILENT, at(-6.0)));
        assert!(!turns.echo(at(-90.0), at(-6.0)));
    }
}
