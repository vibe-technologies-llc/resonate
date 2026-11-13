use std::{sync::Arc, thread, time::Instant};

use resonate_audio::{Learning, SinkId, StreamClock, Trial, TrialVerdict, Unplayed};
use resonate_core::{Frames, SampleFormat, StreamSpec};
use resonate_upnp::{
    Carriage, Feed, SETTLED_BETWEEN_TRIALS, Server, Signals, Silence, TRIAL_ASKED_EVERY,
    TRIAL_LASTS_AT_MOST, Token, Wrapping,
};

use crate::{
    Asked, Device, Error, Result, device,
    link::Link,
    stream::{self, Hold, Idle, Lanes, Offered, Played, entries_in},
    words,
};

pub(crate) fn learn(
    device: &Device,
    sink: SinkId,
    server: &Arc<Server>,
    lanes: &Lanes,
) -> Result<Learning> {
    let hold = lanes.idle(device.at()).ok_or(Error::InUse(sink))?;
    let learnt = trials(device, sink, server, &hold);
    hold.let_go(|| ());
    learnt
}

fn trials(device: &Device, sink: SinkId, server: &Arc<Server>, hold: &Hold) -> Result<Learning> {
    let link = Link::connect(device.at())?;
    let session = device::launched(&link)?;
    let mut learning = Learning::default();
    for trial in Trial::ALL {
        let wanted_unplayed = trial.is_wanting_in(&learning);
        let verdict = if wanted_unplayed {
            TrialVerdict::Untried
        } else {
            let tried = Tried {
                device,
                sink,
                server,
                hold,
                link: &link,
                transport: &session.transport,
                spec: trial.spec(SampleFormat::S24),
            };
            tried.verdict()?
        };
        tracing::info!(receiver = %device.id(), ?trial, ?verdict, "a Cast receiver was tried");
        learning.heard(trial, verdict);
    }
    Ok(learning)
}

struct Tried<'a> {
    device: &'a Device,
    sink: SinkId,
    server: &'a Arc<Server>,
    hold: &'a Hold,
    link: &'a Link,
    transport: &'a str,
    spec: StreamSpec,
}

impl Tried<'_> {
    fn verdict(&self) -> Result<TrialVerdict> {
        let carriage = Carriage::new(
            Wrapping::Flac,
            vec![self.spec.rate],
            vec![self.spec.channels],
        );
        let feed = Arc::new(Feed::new(
            self.spec,
            carriage.clone(),
            Box::new(Silence),
            Arc::new(StreamClock::behind_by(Frames::ZERO)),
        ));
        let token = self.server.offer(Arc::clone(&feed));
        let mut played = None;
        let verdict = self.offered(&feed, token, &carriage, &mut played);
        feed.close();
        self.server.withdraw(token);
        if let Some(played) = &played
            && let Some(Err(error)) = self
                .hold
                .steer(|| stream::media(self.link, played, Asked::Stop))
        {
            tracing::debug!(%error, receiver = %self.device.id(), "a Cast receiver did not stop after a trial");
        }
        thread::sleep(SETTLED_BETWEEN_TRIALS);
        verdict
    }

    fn offered(
        &self,
        feed: &Feed,
        token: Token,
        carriage: &Carriage,
        played: &mut Option<Played>,
    ) -> Result<TrialVerdict> {
        let address = self
            .server
            .address_toward(self.device.at(), token, carriage.extension())
            .map_err(Error::served)?;
        let offered = Offered {
            address,
            kind: carriage.kind_of(self.spec),
            title: words::casting::a_format_tried().to_owned(),
        };
        feed.set_active(true);
        let entry = match self.steer(|| stream::loaded(self.link, self.transport, &offered))? {
            Ok(entry) => entry,
            Err(Error::Refused { .. }) => return Ok(TrialVerdict::Unplayed(Unplayed::Refused)),
            Err(error) => return Err(error),
        };
        let mine = Played {
            transport: self.transport.to_owned(),
            session: entry.session,
        };
        *played = Some(mine.clone());
        self.watched(feed, &mine)
    }

    fn watched(&self, feed: &Feed, played: &Played) -> Result<TrialVerdict> {
        let began = Instant::now();
        let mut signals = Signals::default();
        loop {
            thread::sleep(TRIAL_ASKED_EVERY);
            let answer = self.steer(|| stream::media(self.link, played, Asked::MediaStatus))?;
            let entry = answer
                .ok()
                .as_ref()
                .and_then(entries_in)
                .and_then(|entries| {
                    entries
                        .into_iter()
                        .find(|entry| entry.session == played.session)
                });
            match entry {
                Some(entry) if entry.idle == Some(Idle::Errored) => {
                    return Ok(TrialVerdict::Unplayed(Unplayed::Errored));
                }
                Some(entry) => {
                    signals.heard_playing(entry.playing);
                    if let Some(heard) = entry.heard {
                        signals.clocked(heard);
                    }
                }
                None => signals.heard_playing(false),
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
