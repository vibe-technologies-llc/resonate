use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use resonate_audio::{AudioSource, Learning, SinkId, StreamClock, Trial, TrialVerdict, Unplayed};
use resonate_core::{Frames, StreamSpec};

use crate::{
    Action, Asked, Carriage, Error, Feed, Renderer, Result, carriage,
    serve::{Server, Token},
    stream::{Hold, Holders},
    words,
};

pub const TRIAL_LASTS_AT_MOST: Duration = Duration::from_secs(10);

pub const PULLED_AT_LEAST: Duration = Duration::from_secs(2);

pub const CLOCK_MOVED_AT_LEAST: Duration = Duration::from_secs(1);

pub const TRIAL_ASKED_EVERY: Duration = Duration::from_millis(250);

pub const SETTLED_BETWEEN_TRIALS: Duration = Duration::from_millis(500);

pub struct Silence;

impl AudioSource for Silence {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        dst.fill(0);
        dst.len()
    }
}

pub(crate) fn learn(
    renderer: &Renderer,
    sink: SinkId,
    server: &Arc<Server>,
    holders: &Holders,
) -> Result<Learning> {
    let hold = holders.idle(renderer.udn()).ok_or(Error::InUse(sink))?;
    let learnt = trials(renderer, sink, server, &hold);
    hold.let_go(|| ());
    learnt
}

fn trials(
    renderer: &Renderer,
    sink: SinkId,
    server: &Arc<Server>,
    hold: &Hold,
) -> Result<Learning> {
    let mut learning = Learning::default();
    for trial in Trial::ALL {
        let wanted_unplayed = trial
            .wanting()
            .is_some_and(|wanted| learning.verdict_on(wanted) != Some(TrialVerdict::Played));
        let verdict = match carriage::tried_in(renderer.said(), trial) {
            Some((carriage, spec)) if !wanted_unplayed => {
                let tried = Tried {
                    renderer,
                    sink,
                    server,
                    hold,
                    carriage,
                    spec,
                };
                tried.verdict()?
            }
            Some(_) | None => TrialVerdict::Untried,
        };
        tracing::info!(renderer = %renderer.udn(), ?trial, ?verdict, "a renderer was tried");
        learning.heard(trial, verdict);
    }
    Ok(learning)
}

struct Tried<'a> {
    renderer: &'a Renderer,
    sink: SinkId,
    server: &'a Arc<Server>,
    hold: &'a Hold,
    carriage: Carriage,
    spec: StreamSpec,
}

impl Tried<'_> {
    fn verdict(&self) -> Result<TrialVerdict> {
        let feed = Arc::new(Feed::new(
            self.spec,
            self.carriage.clone(),
            Box::new(Silence),
            Arc::new(StreamClock::behind_by(Frames::ZERO)),
        ));
        let token = self.server.offer(Arc::clone(&feed));
        let verdict = self.offered(&feed, token);
        feed.close();
        self.server.withdraw(token);
        if let Some(Err(error)) = self.hold.steer(|| self.renderer.stop()) {
            tracing::debug!(%error, renderer = %self.renderer.udn(), "a renderer did not stop after a trial");
        }
        thread::sleep(SETTLED_BETWEEN_TRIALS);
        verdict
    }

    fn offered(&self, feed: &Arc<Feed>, token: Token) -> Result<TrialVerdict> {
        let reached = self
            .renderer
            .transport_reached_at()
            .reached_at()
            .map_err(|source| Error::Network {
                asked: Asked::Action(Action::SetAvTransportUri),
                source,
            })?;
        let address = self
            .server
            .address_toward(reached, token, self.carriage.extension())?;
        let title = words::network::a_format_tried();
        let offered = self.steer(|| {
            self.renderer
                .set_uri(&self.carriage, &address, title, self.spec)
        })?;
        if let Some(refused) = refusal(offered)? {
            return Ok(refused);
        }
        feed.set_active(true);
        let played = self.steer(|| self.renderer.play())?;
        if let Some(refused) = refusal(played)? {
            return Ok(refused);
        }
        self.watched(feed)
    }

    fn watched(&self, feed: &Feed) -> Result<TrialVerdict> {
        let began = Instant::now();
        let mut signals = Signals::default();
        loop {
            thread::sleep(TRIAL_ASKED_EVERY);
            match self.steer(|| self.renderer.transport())? {
                Ok(transport) if transport.errored => {
                    return Ok(TrialVerdict::Unplayed(Unplayed::Errored));
                }
                Ok(transport) => signals.heard_playing(transport.playing),
                Err(error) => {
                    tracing::debug!(%error, renderer = %self.renderer.udn(), "a renderer did not say whether it plays");
                }
            }
            match self.steer(|| self.renderer.heard())? {
                Ok(Some(clock)) => signals.clocked(clock),
                Ok(None) => {}
                Err(error) => {
                    tracing::debug!(%error, renderer = %self.renderer.udn(), "a renderer did not say where it is");
                }
            }
            if signals.played(feed) {
                return Ok(TrialVerdict::Played);
            }
            if began.elapsed() >= TRIAL_LASTS_AT_MOST {
                return Ok(TrialVerdict::Unplayed(signals.unplayed(feed)));
            }
        }
    }

    fn steer<T>(&self, call: impl FnOnce() -> T) -> Result<T> {
        self.hold.steer(call).ok_or(Error::InUse(self.sink))
    }
}

fn refusal(answered: Result<()>) -> Result<Option<TrialVerdict>> {
    match answered {
        Ok(()) => Ok(None),
        Err(Error::Fault { .. }) => Ok(Some(TrialVerdict::Unplayed(Unplayed::Refused))),
        Err(error) => Err(error),
    }
}

#[derive(Debug, Default)]
pub struct Signals {
    playing: Option<bool>,
    first: Option<Duration>,
    last: Option<Duration>,
}

impl Signals {
    pub fn heard_playing(&mut self, playing: bool) {
        self.playing = Some(playing);
    }

    pub fn clocked(&mut self, clock: Duration) {
        if self.playing == Some(false) {
            return;
        }
        self.first.get_or_insert(clock);
        self.last = Some(clock);
    }

    pub fn played(&self, feed: &Feed) -> bool {
        pulled(feed) && self.sounding() && self.moved()
    }

    pub fn unplayed(&self, feed: &Feed) -> Unplayed {
        if feed.fetches() == 0 {
            Unplayed::NeverFetched
        } else if !pulled(feed) {
            Unplayed::StoppedFetching
        } else if !self.sounding() {
            Unplayed::NotPlaying
        } else {
            Unplayed::ClockStill
        }
    }

    fn sounding(&self) -> bool {
        self.playing != Some(false)
    }

    fn moved(&self) -> bool {
        match (self.first, self.last) {
            (Some(first), Some(last)) => last.saturating_sub(first) >= CLOCK_MOVED_AT_LEAST,
            _ => true,
        }
    }
}

fn pulled(feed: &Feed) -> bool {
    feed.is_fetched() && feed.served() >= feed.frames_of(PULLED_AT_LEAST)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clock_never_read_passes_and_one_read_must_move_a_second_while_playing() {
        let mut unread = Signals::default();
        unread.heard_playing(true);

        assert!(unread.moved());
        assert!(unread.sounding());

        let mut still = Signals::default();
        still.heard_playing(true);
        still.clocked(Duration::from_millis(200));
        still.clocked(Duration::from_millis(900));

        assert!(!still.moved());

        still.clocked(Duration::from_millis(1_300));

        assert!(still.moved());

        let mut stopped = Signals::default();
        stopped.heard_playing(false);
        stopped.clocked(Duration::from_secs(4));

        assert!(!stopped.sounding());
        assert_eq!(stopped.first, None);
    }
}
