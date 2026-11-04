use std::time::Duration;

pub const OUT_OF_STEP_PAST: Duration = Duration::from_millis(40);

pub const FAR_ENOUGH_TO_SEEK: Duration = Duration::from_secs(2);

pub const SAMPLES_BEFORE_A_NUDGE: u32 = 5;

pub const NUDGES_APART: Duration = Duration::from_secs(5);

pub const HANDED_ON_WITHIN: Duration = Duration::from_secs(2);

const PAUSES_SEEN_BEFORE_SITTING_OUT: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mine {
    Playing,
    Paused,
    Stopped,
}

#[derive(Clone, Debug)]
pub struct Seen<'host> {
    pub now_nanos: u64,
    pub host_row: Option<u64>,
    pub host_next: &'host [u64],
    pub host_playing: bool,
    pub host_position: Option<u64>,
    pub coarse: bool,
    pub mine: Mine,
    pub mine_row: Option<u64>,
    pub mine_position: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Doing {
    Nothing,
    Find { row: u64 },
    Pause,
    Play,
    Nudge,
    Seek { to_nanos: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SittingOut {
    NotHeld,
    CannotStartMidSong,
    AskedTo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    WaitingForTheHost,
    Finding,
    SittingOut(SittingOut),
    HostPaused,
    InStep,
}

#[derive(Clone, Debug, Default)]
pub struct Follow {
    finding: Option<u64>,
    out_for: Option<(u64, SittingOut)>,
    asked_out: bool,
    pauses_seen: u32,
    meant_to_play: bool,
    over: u32,
    last_moved_at: Option<u64>,
    handing_on_since: Option<u64>,
    last_host_row: Option<u64>,
    last_host_next: Vec<u64>,
    aligned_row: Option<u64>,
    standing: Option<Standing>,
}

impl Follow {
    pub fn standing(&self) -> Standing {
        self.standing.unwrap_or(Standing::WaitingForTheHost)
    }

    pub fn found(&mut self, row: u64, held: bool) {
        if self.finding == Some(row) {
            self.finding = None;
        }
        if !held {
            self.out_for = Some((row, SittingOut::NotHeld));
        }
    }

    pub fn loaded(&mut self, row: u64, sounding: bool, now_nanos: u64) {
        self.meant_to_play = sounding;
        self.over = 0;
        self.aligned_row = Some(row);
        self.last_moved_at = Some(now_nanos);
    }

    pub fn cannot_start_mid_song(&mut self, row: u64) {
        self.out_for = Some((row, SittingOut::CannotStartMidSong));
    }

    fn stood(&mut self, standing: Standing, doing: Doing) -> Doing {
        self.standing = Some(standing);
        if doing == Doing::Pause {
            self.meant_to_play = false;
        }
        doing
    }

    fn handing_on(&mut self, seen: &Seen<'_>, host_row: u64) -> bool {
        let handed = seen.mine_row.is_some_and(|mine| {
            seen.host_next.contains(&mine)
                || (self.last_host_row == Some(mine) && self.last_host_next.contains(&host_row))
        });
        if !handed {
            self.handing_on_since = None;
            return false;
        }
        let since = *self.handing_on_since.get_or_insert(seen.now_nanos);
        let within = u64::try_from(HANDED_ON_WITHIN.as_nanos()).unwrap_or(u64::MAX);

        seen.now_nanos.saturating_sub(since) < within
    }

    pub fn step(&mut self, seen: &Seen<'_>) -> Doing {
        if self.meant_to_play && seen.mine == Mine::Paused {
            self.pauses_seen = self.pauses_seen.saturating_add(1);
            if self.pauses_seen < PAUSES_SEEN_BEFORE_SITTING_OUT {
                return Doing::Nothing;
            }
            self.pauses_seen = 0;
            self.asked_out = true;
            self.meant_to_play = false;
        } else {
            self.pauses_seen = 0;
        }
        if self.asked_out {
            if seen.mine != Mine::Playing {
                return self.stood(Standing::SittingOut(SittingOut::AskedTo), Doing::Nothing);
            }
            self.asked_out = false;
            self.over = SAMPLES_BEFORE_A_NUDGE;
            self.last_moved_at = None;
            self.meant_to_play = true;
        }

        let Some(host_row) = seen.host_row else {
            return self.stood(Standing::WaitingForTheHost, Doing::Nothing);
        };
        let doing = self.step_with_a_song(seen, host_row);
        if self.last_host_row != Some(host_row) {
            self.last_host_row = seen.host_row;
            self.last_host_next = seen.host_next.to_vec();
        }

        doing
    }

    fn step_with_a_song(&mut self, seen: &Seen<'_>, host_row: u64) -> Doing {
        if let Some((row, why)) = self.out_for {
            if row == host_row {
                let doing = if seen.mine == Mine::Playing {
                    Doing::Pause
                } else {
                    Doing::Nothing
                };
                return self.stood(Standing::SittingOut(why), doing);
            }
            self.out_for = None;
        }

        if seen.mine_row != Some(host_row) {
            if self.handing_on(seen, host_row) {
                return self.stood(Standing::InStep, Doing::Nothing);
            }
            if self.finding == Some(host_row) {
                return self.stood(Standing::Finding, Doing::Nothing);
            }
            self.finding = Some(host_row);
            return self.stood(Standing::Finding, Doing::Find { row: host_row });
        }
        self.handing_on_since = None;

        if !seen.host_playing {
            let doing = if seen.mine == Mine::Playing {
                Doing::Pause
            } else {
                Doing::Nothing
            };
            return self.stood(Standing::HostPaused, doing);
        }
        if seen.mine != Mine::Playing {
            self.meant_to_play = true;
            self.over = SAMPLES_BEFORE_A_NUDGE;
            self.last_moved_at = None;
            return self.stood(Standing::InStep, Doing::Play);
        }

        let doing = self.keeping_step(seen, host_row);
        self.stood(Standing::InStep, doing)
    }

    fn keeping_step(&mut self, seen: &Seen<'_>, host_row: u64) -> Doing {
        let (Some(mine), Some(host)) = (seen.mine_position, seen.host_position) else {
            return Doing::Nothing;
        };
        let apart = mine.abs_diff(host);
        let threshold = u64::try_from(OUT_OF_STEP_PAST.as_nanos()).unwrap_or(u64::MAX);
        if apart <= threshold {
            self.over = 0;
            return Doing::Nothing;
        }

        self.over = self.over.saturating_add(1);
        if self.over < SAMPLES_BEFORE_A_NUDGE {
            return Doing::Nothing;
        }
        let apart_at_least = u64::try_from(NUDGES_APART.as_nanos()).unwrap_or(u64::MAX);
        if self
            .last_moved_at
            .is_some_and(|at| seen.now_nanos.saturating_sub(at) < apart_at_least)
        {
            return Doing::Nothing;
        }
        if seen.coarse && self.aligned_row == Some(host_row) {
            return Doing::Nothing;
        }

        self.over = 0;
        self.last_moved_at = Some(seen.now_nanos);
        self.aligned_row = Some(host_row);

        let far = u64::try_from(FAR_ENOUGH_TO_SEEK.as_nanos()).unwrap_or(u64::MAX);
        if apart > far {
            return Doing::Seek { to_nanos: host };
        }

        Doing::Nudge
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: u64 = 1_000_000_000;

    const MS: u64 = 1_000_000;

    fn seen(now_nanos: u64, mine: Mine, mine_row: Option<u64>) -> Seen<'static> {
        Seen {
            now_nanos,
            host_row: Some(1),
            host_next: &[2],
            host_playing: true,
            host_position: Some(10 * SECOND),
            coarse: false,
            mine,
            mine_row,
            mine_position: Some(10 * SECOND),
        }
    }

    fn in_step() -> Follow {
        let mut follow = Follow::default();

        follow.loaded(1, true, 0);
        follow
    }

    #[test]
    fn a_new_song_is_asked_for_once_and_then_waited_for() {
        let mut follow = Follow::default();

        assert_eq!(
            follow.step(&seen(0, Mine::Stopped, None)),
            Doing::Find { row: 1 }
        );
        assert_eq!(follow.step(&seen(1, Mine::Stopped, None)), Doing::Nothing);
        assert_eq!(follow.standing(), Standing::Finding);
    }

    #[test]
    fn a_song_nobody_here_holds_sits_out_and_the_next_one_is_asked_for() {
        let mut follow = Follow::default();

        follow.step(&seen(0, Mine::Stopped, None));
        follow.found(1, false);

        assert_eq!(follow.step(&seen(1, Mine::Stopped, None)), Doing::Nothing);
        assert_eq!(follow.standing(), Standing::SittingOut(SittingOut::NotHeld));

        let next = Seen {
            host_row: Some(2),
            host_next: &[],
            ..seen(2, Mine::Stopped, None)
        };

        assert_eq!(follow.step(&next), Doing::Find { row: 2 });
    }

    #[test]
    fn a_guest_a_moment_past_the_host_at_the_end_of_a_song_is_not_reloaded() {
        let mut follow = in_step();
        let ahead = seen(SECOND, Mine::Playing, Some(2));

        assert_eq!(follow.step(&ahead), Doing::Nothing);
        assert_eq!(follow.standing(), Standing::InStep);

        let long_past = seen(SECOND + 3 * SECOND, Mine::Playing, Some(2));

        assert_eq!(follow.step(&long_past), Doing::Find { row: 1 });
    }

    #[test]
    fn a_guest_a_moment_behind_the_host_at_the_end_of_a_song_is_not_reloaded() {
        let mut follow = in_step();

        assert_eq!(
            follow.step(&seen(0, Mine::Playing, Some(1))),
            Doing::Nothing
        );

        let moved_on = Seen {
            host_row: Some(2),
            host_next: &[3],
            ..seen(SECOND, Mine::Playing, Some(1))
        };

        assert_eq!(follow.step(&moved_on), Doing::Nothing);
    }

    #[test]
    fn a_pause_here_sits_the_party_out_and_a_play_comes_back_to_where_the_host_is() {
        let mut follow = in_step();

        for _ in 0..PAUSES_SEEN_BEFORE_SITTING_OUT {
            assert_eq!(follow.step(&seen(0, Mine::Paused, Some(1))), Doing::Nothing);
        }
        assert_eq!(follow.standing(), Standing::SittingOut(SittingOut::AskedTo));

        let back = Seen {
            mine_position: Some(4 * SECOND),
            ..seen(SECOND, Mine::Playing, Some(1))
        };

        assert_eq!(
            follow.step(&back),
            Doing::Seek {
                to_nanos: 10 * SECOND
            }
        );
    }

    #[test]
    fn the_host_pausing_pauses_a_guest_without_counting_as_the_guest_leaving() {
        let mut follow = in_step();
        let paused = Seen {
            host_playing: false,
            ..seen(0, Mine::Playing, Some(1))
        };

        assert_eq!(follow.step(&paused), Doing::Pause);

        let still = Seen {
            host_playing: false,
            ..seen(MS, Mine::Paused, Some(1))
        };

        assert_eq!(follow.step(&still), Doing::Nothing);
        assert_eq!(follow.standing(), Standing::HostPaused);

        assert_eq!(
            follow.step(&seen(2 * MS, Mine::Paused, Some(1))),
            Doing::Play
        );
    }

    #[test]
    fn a_passing_wobble_moves_nothing_and_a_lasting_one_nudges_once() {
        let mut follow = in_step();
        let off = |at| Seen {
            mine_position: Some(10 * SECOND + 80 * MS),
            ..seen(at, Mine::Playing, Some(1))
        };
        let mut at = 6 * SECOND;

        for _ in 0..SAMPLES_BEFORE_A_NUDGE - 1 {
            assert_eq!(follow.step(&off(at)), Doing::Nothing);
            at += 200 * MS;
        }
        assert_eq!(
            follow.step(&seen(at, Mine::Playing, Some(1))),
            Doing::Nothing
        );
        at += 200 * MS;
        for _ in 0..SAMPLES_BEFORE_A_NUDGE - 1 {
            assert_eq!(follow.step(&off(at)), Doing::Nothing);
            at += 200 * MS;
        }
        assert_eq!(follow.step(&off(at)), Doing::Nudge);
        at += 200 * MS;
        for _ in 0..SAMPLES_BEFORE_A_NUDGE * 2 {
            assert_eq!(follow.step(&off(at)), Doing::Nothing);
            at += 200 * MS;
        }
    }

    #[test]
    fn a_host_that_jumped_is_followed_by_a_seek_rather_than_a_nudge() {
        let mut follow = in_step();
        let mut doing = Doing::Nothing;
        let mut at = 6 * SECOND;

        for _ in 0..SAMPLES_BEFORE_A_NUDGE {
            doing = follow.step(&Seen {
                host_position: Some(60 * SECOND),
                ..seen(at, Mine::Playing, Some(1))
            });
            at += 200 * MS;
        }

        assert_eq!(
            doing,
            Doing::Seek {
                to_nanos: 60 * SECOND
            }
        );
    }

    #[test]
    fn a_host_on_a_network_renderer_is_lined_up_with_once_a_song_and_left_alone() {
        let mut follow = Follow::default();

        follow.step(&seen(0, Mine::Stopped, None));
        follow.found(1, true);
        follow.loaded(1, true, 0);

        let mut at = 6 * SECOND;
        for _ in 0..SAMPLES_BEFORE_A_NUDGE * 4 {
            let doing = follow.step(&Seen {
                coarse: true,
                mine_position: Some(10 * SECOND + 80 * MS),
                ..seen(at, Mine::Playing, Some(1))
            });
            assert_eq!(doing, Doing::Nothing);
            at += 200 * MS;
        }
    }

    #[test]
    fn a_song_that_will_not_start_mid_way_sits_out_until_the_next() {
        let mut follow = Follow::default();

        follow.step(&seen(0, Mine::Stopped, None));
        follow.found(1, true);
        follow.cannot_start_mid_song(1);
        follow.step(&seen(1, Mine::Stopped, None));

        assert_eq!(
            follow.standing(),
            Standing::SittingOut(SittingOut::CannotStartMidSong)
        );
    }
}
