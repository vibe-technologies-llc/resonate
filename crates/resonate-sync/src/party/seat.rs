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
        ahead::{Ahead, SONGS_FOUND_AHEAD},
        follow::{Doing, Follow, Mine, Seen, Standing},
        said::{PartyNow, PartySong},
        service::{Party, PartyReach, PartyShown},
    },
};

const LOOK_EVERY: Duration = Duration::from_millis(200);

const TOLD_AT_LEAST_EVERY: Duration = Duration::from_secs(2);

const HOST_MOVED_PAST: Duration = Duration::from_millis(500);

const NANOS_A_MILLISECOND: f64 = 1_000_000.0;

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

    fn keep_ahead(&self, rows: &[Self::Row]) -> bool;

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
                    hosted(&party, &seat, &stop, &[]);
                    return;
                }
                match guested(&party, &seat, &finder, &followed, &stop) {
                    Guested::Left => seat.put_back(),
                    Guested::TookOver { rows, inherited } => {
                        hosted_after(&party, &seat, &stop, rows, inherited);
                    }
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

type Inherited<R> = mpsc::Receiver<Vec<(u64, R)>>;

enum Guested<R> {
    Left,
    TookOver {
        rows: Vec<(u64, u64)>,
        inherited: Option<Inherited<R>>,
    },
}

fn inherit<F: PartyFinder>(finder: &Arc<F>, songs: Vec<PartySong>) -> Option<Inherited<F::Row>> {
    let (found_in, found) = mpsc::channel();
    let finder = Arc::clone(finder);
    let looking = thread::Builder::new()
        .name("resonate-party-find".to_owned())
        .spawn(move || {
            let held = songs
                .iter()
                .filter_map(|song| finder.found(song).map(|held| (song.row, held)))
                .collect();
            let _ = found_in.send(held);
        });
    if let Err(error) = looking {
        tracing::warn!(%error, "the songs a party had up next could not be looked for");
        return None;
    }

    Some(found)
}

fn hosted_after<S: PartySeat>(
    party: &Party,
    seat: &S,
    stop: &AtomicBool,
    mut rows: Vec<(u64, u64)>,
    inherited: Option<Inherited<S::Row>>,
) {
    if let Some(inherited) = inherited {
        while !stop.load(Ordering::Acquire) {
            match inherited.recv_timeout(LOOK_EVERY) {
                Ok(held) => {
                    let in_order: Vec<S::Row> = held.iter().map(|(_, row)| row.clone()).collect();
                    if seat.keep_ahead(&in_order) {
                        for (host, row) in &held {
                            let stamp = seat.stamp_of(row);
                            if !rows.iter().any(|(kept, _)| *kept == stamp) {
                                rows.push((stamp, *host));
                            }
                        }
                    }
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    hosted(party, seat, stop, &rows);
}

fn as_the_party_knows(rows: &[(u64, u64)], mut song: PartySong) -> PartySong {
    if let Some((_, host)) = rows.iter().find(|(stamp, _)| *stamp == song.row) {
        song.row = *host;
    }
    song
}

fn hosted<S: PartySeat>(party: &Party, seat: &S, stop: &AtomicBool, rows: &[(u64, u64)]) {
    let mut told: Option<(PartyNow, u64)> = None;

    while !stop.load(Ordering::Acquire) {
        thread::sleep(LOOK_EVERY);

        let heard = seat.heard();
        let Some(anchor) = party.to_server(heard.at_nanos) else {
            continue;
        };
        let (song, next) = seat.songs_up_next();
        let now = PartyNow {
            song: song.map(|song| as_the_party_knows(rows, song)),
            next: next
                .into_iter()
                .map(|song| as_the_party_knows(rows, song))
                .collect(),
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
) -> Guested<S::Row>
where
    S: PartySeat,
    F: PartyFinder<Row = S::Row>,
{
    let mut follow = Follow::default();
    let mut ahead = Ahead::default();
    let mut rows: Vec<(u64, u64)> = Vec::new();
    let mut followed_last: Option<PartyNow> = None;
    let (found_in, found) = mpsc::channel::<Found<S::Row>>();

    while !stop.load(Ordering::Acquire) {
        thread::sleep(LOOK_EVERY);

        let shown = party.shown();
        if shown.hosting {
            let inherited = followed_last.and_then(|now| inherit(finder, now.next));
            return Guested::TookOver { rows, inherited };
        }
        if matches!(
            shown.reach,
            PartyReach::Ended | PartyReach::NoSuchParty | PartyReach::TurnedAway
        ) {
            return Guested::Left;
        }

        let mut heard = seat.heard();
        let mut there = party.to_server(heard.at_nanos);
        let now = shown.now.clone();
        if now.is_some() {
            followed_last.clone_from(&now);
        }

        while let Ok(arrived) = found.try_recv() {
            match arrived {
                Found::Song { row, songs } => {
                    if now
                        .as_ref()
                        .and_then(|now| now.song.as_ref())
                        .map(|song| song.row)
                        != Some(row)
                    {
                        follow.found(row, true);
                        continue;
                    }
                    ahead.forget();
                    if taken_up(party, seat, &mut follow, &mut rows, row, songs) {
                        heard = seat.heard();
                        there = party.to_server(heard.at_nanos);
                    }
                }
                Found::Ahead { wanted, songs } => {
                    let next = host_next_of(now.as_ref());
                    if ahead.found(&wanted, &next) && !kept_ahead(seat, &mut rows, songs) {
                        ahead.not_kept();
                    }
                }
            }
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

        if let (Some(mine), Some(host)) = (seen.mine_position, seen.host_position)
            && seen.mine == Mine::Playing
            && seen.mine_row == seen.host_row
        {
            tracing::trace!(
                apart_ms = (i128::from(mine) - i128::from(host)) as f64 / NANOS_A_MILLISECOND,
                "this seat against the host"
            );
        }

        if !rows.is_empty() {
            let known: Vec<u64> = rows.iter().map(|(stamp, _)| *stamp).collect();
            for song in seat.added_here(&known) {
                party.ask_for(song);
            }
        }

        if let Some(now) = &now
            && seen.mine_row.is_some()
            && seen.mine_row == seen.host_row
            && let Some(wanted) = ahead.step(&next, &standing_ahead(&rows, heard.row))
        {
            let songs: Vec<PartySong> = now.next.iter().take(SONGS_FOUND_AHEAD).cloned().collect();
            let finder = Arc::clone(finder);
            let found_in = found_in.clone();
            let looking = thread::Builder::new()
                .name("resonate-party-find".to_owned())
                .spawn(move || {
                    let songs = songs
                        .iter()
                        .map(|song| (song.row, finder.found(song)))
                        .collect();
                    let _ = found_in.send(Found::Ahead { wanted, songs });
                });
            if let Err(error) = looking {
                tracing::warn!(%error, "a party's queue ahead could not be looked for");
                ahead.not_found();
                ahead.forget();
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
                            let songs = songs
                                .iter()
                                .map(|song| (song.row, finder.found(song)))
                                .collect();
                            let _ = found_in.send(Found::Song { row, songs });
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

    Guested::Left
}

enum Found<R> {
    Song {
        row: u64,
        songs: Vec<(u64, Option<R>)>,
    },
    Ahead {
        wanted: Vec<u64>,
        songs: Vec<(u64, Option<R>)>,
    },
}

fn host_next_of(now: Option<&PartyNow>) -> Vec<u64> {
    now.map(|now| now.next.iter().map(|song| song.row).collect())
        .unwrap_or_default()
}

fn standing_ahead(rows: &[(u64, u64)], heard: Option<u64>) -> Vec<u64> {
    let Some(at) = rows.iter().position(|(stamp, _)| Some(*stamp) == heard) else {
        return Vec::new();
    };

    rows.iter().skip(at + 1).map(|(_, host)| *host).collect()
}

fn kept_ahead<S: PartySeat>(
    seat: &S,
    rows: &mut Vec<(u64, u64)>,
    songs: Vec<(u64, Option<S::Row>)>,
) -> bool {
    let heard = seat.heard();
    let Some(at) = rows.iter().position(|(stamp, _)| Some(*stamp) == heard.row) else {
        return false;
    };
    let run: Vec<(u64, S::Row)> = songs
        .into_iter()
        .map_while(|(host, held)| held.map(|held| (host, held)))
        .collect();
    let in_order: Vec<S::Row> = run.iter().map(|(_, held)| held.clone()).collect();

    if !seat.keep_ahead(&in_order) {
        return false;
    }

    rows.truncate(at + 1);
    rows.extend(run.iter().map(|(host, held)| (seat.stamp_of(held), *host)));

    true
}

fn taken_up<S: PartySeat>(
    party: &Party,
    seat: &S,
    follow: &mut Follow,
    rows: &mut Vec<(u64, u64)>,
    row: u64,
    songs: Vec<(u64, Option<S::Row>)>,
) -> bool {
    let held_first = songs.first().is_some_and(|(_, held)| held.is_some());
    follow.found(row, held_first);
    if !held_first {
        return false;
    }

    let run: Vec<(u64, S::Row)> = songs
        .into_iter()
        .map_while(|(host, held)| held.map(|held| (host, held)))
        .collect();
    let in_order: Vec<S::Row> = run.iter().map(|(_, held)| held.clone()).collect();
    let shown = party.shown();
    let Some(now) = shown.now else {
        return false;
    };
    let at_nanos = seat.heard().at_nanos;
    let Some(there) = party.to_server(at_nanos) else {
        follow.cannot_start_mid_song(row);
        return false;
    };
    let position = now.position_at(there);

    if seat.take_up(&in_order, position, at_nanos, now.playing) {
        *rows = run
            .iter()
            .map(|(host, held)| (seat.stamp_of(held), *host))
            .collect();
        follow.loaded(row, now.playing, at_nanos);

        true
    } else {
        follow.cannot_start_mid_song(row);

        false
    }
}
