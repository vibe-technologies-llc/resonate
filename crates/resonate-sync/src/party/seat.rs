use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use parking_lot::RwLock;

use crate::{
    Error, Result,
    party::{
        follow::{Doing, Follow, Mine, Seen, Standing},
        said::{PartyNow, PartySong},
        service::{Party, PartyReach, PartyShown},
    },
};

const LOOK_EVERY: Duration = Duration::from_millis(200);

const TOLD_AT_LEAST_EVERY: Duration = Duration::from_secs(2);

const HOST_MOVED_PAST: Duration = Duration::from_millis(500);

const SONGS_FOUND_AHEAD: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeatHeard {
    pub mine: Mine,
    pub row: Option<u64>,
    pub position_nanos: Option<u64>,
    pub at_nanos: u64,
    pub coarse: bool,
}

pub trait PartySeat: Send + 'static {
    type Row: Clone + Send + 'static;

    fn heard(&self) -> SeatHeard;

    fn stamp_of(&self, row: &Self::Row) -> u64;

    fn songs_up_next(&self) -> (Option<PartySong>, Vec<PartySong>);

    fn added_here(&self, known: &[u64]) -> Vec<PartySong>;

    fn take_up(
        &self,
        rows: &[Self::Row],
        position_nanos: u64,
        at_nanos: u64,
        sounding: bool,
    ) -> bool;

    fn nudge(&self, position_nanos: u64, at_nanos: u64);

    fn seek(&self, to_nanos: u64);

    fn pause(&self);

    fn play(&self);

    fn put_back(&self);
}

pub trait PartyFinder: Send + Sync + 'static {
    type Row: Clone + Send + 'static;

    fn found(&self, song: &PartySong) -> Option<Self::Row>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartyFollowed {
    pub standing: Standing,
    pub song: Option<PartySong>,
}

pub struct Going {
    party: Arc<Party>,
    followed: Arc<RwLock<PartyFollowed>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Going {
    pub fn begin<S, F>(party: Party, seat: S, finder: Arc<F>) -> Result<Self>
    where
        S: PartySeat,
        F: PartyFinder<Row = S::Row>,
    {
        let party = Arc::new(party);
        let followed = Arc::new(RwLock::new(PartyFollowed {
            standing: Standing::WaitingForTheHost,
            song: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let running = {
            let party = Arc::clone(&party);
            let followed = Arc::clone(&followed);
            let stop = Arc::clone(&stop);
            move || {
                if party.shown().hosting {
                    hosted(&party, &seat, &stop);
                } else {
                    guested(&party, &seat, &finder, &followed, &stop);
                    seat.put_back();
                }
            }
        };
        let thread = thread::Builder::new()
            .name("resonate-party-seat".to_owned())
            .spawn(running)
            .map_err(Error::Thread)?;

        Ok(Self {
            party,
            followed,
            stop,
            thread: Some(thread),
        })
    }

    pub fn shown(&self) -> PartyShown {
        self.party.shown()
    }

    pub fn followed(&self) -> PartyFollowed {
        self.followed.read().clone()
    }

    pub const fn party(&self) -> &Arc<Party> {
        &self.party
    }

    pub fn leave(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Going {
    fn drop(&mut self) {
        self.leave();
    }
}

fn nanos_of(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn hosted<S: PartySeat>(party: &Party, seat: &S, stop: &AtomicBool) {
    let mut told: Option<(PartyNow, u64)> = None;

    while !stop.load(Ordering::Acquire) {
        thread::sleep(LOOK_EVERY);

        let heard = seat.heard();
        let Some(anchor) = party.to_server(heard.at_nanos) else {
            continue;
        };
        let (song, next) = seat.songs_up_next();
        let now = PartyNow {
            song,
            next,
            playing: heard.mine == Mine::Playing,
            position_nanos: heard.position_nanos.unwrap_or(0),
            anchor_nanos: anchor,
            coarse: heard.coarse,
        };

        if worth_telling(told.as_ref(), &now, heard.at_nanos) {
            party.tell(now.clone());
            told = Some((now, heard.at_nanos));
        }
    }
}

fn worth_telling(told: Option<&(PartyNow, u64)>, now: &PartyNow, at_nanos: u64) -> bool {
    let Some((before, told_at)) = told else {
        return true;
    };

    if before.song != now.song
        || before.next != now.next
        || before.playing != now.playing
        || before.coarse != now.coarse
    {
        return true;
    }
    if at_nanos.saturating_sub(*told_at) >= nanos_of(TOLD_AT_LEAST_EVERY) {
        return true;
    }

    before
        .position_at(now.anchor_nanos)
        .abs_diff(now.position_nanos)
        > nanos_of(HOST_MOVED_PAST)
}

fn songs_of(now: &PartyNow) -> Vec<PartySong> {
    now.song
        .iter()
        .chain(now.next.iter().take(SONGS_FOUND_AHEAD))
        .cloned()
        .collect()
}

fn guested<S, F>(
    party: &Party,
    seat: &S,
    finder: &Arc<F>,
    followed: &RwLock<PartyFollowed>,
    stop: &AtomicBool,
) where
    S: PartySeat,
    F: PartyFinder<Row = S::Row>,
{
    let mut follow = Follow::default();
    let mut rows: Vec<(u64, u64)> = Vec::new();
    let (found_in, found) = mpsc::channel::<(u64, Vec<(u64, Option<S::Row>)>)>();

    while !stop.load(Ordering::Acquire) {
        thread::sleep(LOOK_EVERY);

        let shown = party.shown();
        if matches!(
            shown.reach,
            PartyReach::Ended | PartyReach::NoSuchParty | PartyReach::TurnedAway
        ) {
            return;
        }

        let heard = seat.heard();
        let there = party.to_server(heard.at_nanos);
        let now = shown.now.clone();

        while let Ok((row, songs)) = found.try_recv() {
            if now
                .as_ref()
                .and_then(|now| now.song.as_ref())
                .map(|song| song.row)
                != Some(row)
            {
                follow.found(row, true);
                continue;
            }
            taken_up(party, seat, &mut follow, &mut rows, row, songs);
        }

        let mine_row = heard
            .row
            .and_then(|stamp| rows.iter().find(|(held, _)| *held == stamp))
            .map(|(_, host)| *host);
        let next: Vec<u64> = now
            .as_ref()
            .map(|now| now.next.iter().map(|song| song.row).collect())
            .unwrap_or_default();
        let seen = Seen {
            now_nanos: heard.at_nanos,
            host_row: now
                .as_ref()
                .and_then(|now| now.song.as_ref())
                .map(|song| song.row),
            host_next: &next,
            host_playing: now.as_ref().is_some_and(|now| now.playing),
            host_position: now
                .as_ref()
                .zip(there)
                .map(|(now, there)| now.position_at(there)),
            coarse: heard.coarse || now.as_ref().is_some_and(|now| now.coarse),
            mine: heard.mine,
            mine_row,
            mine_position: heard.position_nanos,
        };

        if !rows.is_empty() {
            let known: Vec<u64> = rows.iter().map(|(stamp, _)| *stamp).collect();
            for song in seat.added_here(&known) {
                party.ask_for(song);
            }
        }

        match follow.step(&seen) {
            Doing::Nothing => {}
            Doing::Find { row } => {
                if let Some(now) = &now {
                    let songs = songs_of(now);
                    let finder = Arc::clone(finder);
                    let found_in = found_in.clone();
                    let looking = thread::Builder::new()
                        .name("resonate-party-find".to_owned())
                        .spawn(move || {
                            let rows = songs
                                .iter()
                                .map(|song| (song.row, finder.found(song)))
                                .collect();
                            let _ = found_in.send((row, rows));
                        });
                    if let Err(error) = looking {
                        tracing::warn!(%error, "a party's song could not be looked for");
                        follow.found(row, false);
                    }
                }
            }
            Doing::Pause => seat.pause(),
            Doing::Play => seat.play(),
            Doing::Nudge => {
                if let Some(position) = seen.host_position {
                    seat.nudge(position, heard.at_nanos);
                }
            }
            Doing::Seek { to_nanos } => seat.seek(to_nanos),
        }

        *followed.write() = PartyFollowed {
            standing: follow.standing(),
            song: now.and_then(|now| now.song),
        };
    }
}

fn taken_up<S: PartySeat>(
    party: &Party,
    seat: &S,
    follow: &mut Follow,
    rows: &mut Vec<(u64, u64)>,
    row: u64,
    songs: Vec<(u64, Option<S::Row>)>,
) {
    let held_first = songs.first().is_some_and(|(_, held)| held.is_some());
    follow.found(row, held_first);
    if !held_first {
        return;
    }

    let run: Vec<(u64, S::Row)> = songs
        .into_iter()
        .map_while(|(host, held)| held.map(|held| (host, held)))
        .collect();
    let in_order: Vec<S::Row> = run.iter().map(|(_, held)| held.clone()).collect();
    let shown = party.shown();
    let Some(now) = shown.now else {
        return;
    };
    let at_nanos = seat.heard().at_nanos;
    let Some(there) = party.to_server(at_nanos) else {
        follow.cannot_start_mid_song(row);
        return;
    };
    let position = now.position_at(there);

    if seat.take_up(&in_order, position, at_nanos, now.playing) {
        *rows = run
            .iter()
            .map(|(host, held)| (seat.stamp_of(held), *host))
            .collect();
        follow.loaded(row, now.playing, at_nanos);
    } else {
        follow.cannot_start_mid_song(row);
    }
}
