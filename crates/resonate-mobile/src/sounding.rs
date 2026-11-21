use resonate_core::ZoneId;
use resonate_engine::{Command, PlayerState, QueueRows};

use crate::{MobilePlayer, Result, Track, Transport, row_being_opened, transport_of};

#[derive(Clone, Debug, uniffi::Record)]
pub struct HeardElsewhere {
    pub zone: u32,
    pub transport: Transport,
    pub current: Option<Track>,
    pub position_ms: u64,
    pub duration_ms: u64,
}

pub(crate) struct Heard {
    pub(crate) current: Option<Track>,
    pub(crate) position_ms: u64,
    pub(crate) duration_ms: u64,
}

impl MobilePlayer {
    pub(crate) fn heard(&self, state: &PlayerState, rows: &QueueRows) -> Heard {
        let current = state
            .current
            .map(|track| track.id)
            .or_else(|| row_being_opened(state, rows))
            .and_then(|id| self.queue_track(rows, id));
        let duration_ms = state
            .current
            .and_then(|track| {
                track
                    .duration
                    .map(|frames| frames.to_duration(track.source.rate).as_millis() as u64)
            })
            .unwrap_or_else(|| current.as_ref().map_or(0, |track| track.duration_ms));
        let position_ms = state.current.map_or(0, |track| {
            track.position.to_duration(track.source.rate).as_millis() as u64
        });

        Heard {
            current,
            position_ms,
            duration_ms,
        }
    }

    pub(crate) fn heard_elsewhere(&self, shown: ZoneId) -> Option<HeardElsewhere> {
        self.playing
            .listed()
            .into_iter()
            .filter(|zone| zone.id != shown)
            .find_map(|zone| {
                let state = zone.player.state();
                let transport = transport_of(state.playback);

                if !transport.sounds() {
                    return None;
                }

                let heard = self.heard(&state, &zone.player.queued().rows);
                Some(HeardElsewhere {
                    zone: zone.id.get(),
                    transport,
                    current: heard.current,
                    position_ms: heard.position_ms,
                    duration_ms: heard.duration_ms,
                })
            })
    }

    fn zones_where(&self, holds: impl Fn(Transport) -> bool) -> Vec<ZoneId> {
        self.playing
            .listed()
            .into_iter()
            .filter(|zone| holds(transport_of(zone.player.state().playback)))
            .map(|zone| zone.id)
            .collect()
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn pause_sounding_zones(&self) -> Result<Vec<u32>> {
        let sounding = self.zones_where(Transport::sounds);

        for &zone in &sounding {
            self.playing.send_to(zone, Command::Pause)?;
        }
        Ok(sounding.into_iter().map(ZoneId::get).collect())
    }

    pub fn play_zones(&self, zones: Vec<u32>) -> Result<()> {
        let listed = self.playing.listed();

        for zone in zones.into_iter().filter_map(ZoneId::new) {
            if listed.iter().any(|open| open.id == zone) {
                self.playing.send_to(zone, Command::Play)?;
            }
        }
        Ok(())
    }

    pub fn stop_every_zone(&self) -> Result<()> {
        for zone in self.zones_where(|transport| transport != Transport::Stopped) {
            self.playing.send_to(zone, Command::Stop)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Instant};

    use super::*;
    use crate::harness::{Harness, LOOKED_AGAIN_AFTER, SETTLED_WITHIN, name_of};

    fn transport_of_zone(harness: &Harness, zone: ZoneId) -> Transport {
        transport_of(
            harness
                .player
                .playing
                .player_of(zone)
                .expect("an open zone")
                .state()
                .playback,
        )
    }

    fn settled_at(harness: &Harness, zone: ZoneId, wanted: Transport) {
        let began = Instant::now();
        while transport_of_zone(harness, zone) != wanted {
            assert!(
                began.elapsed() < SETTLED_WITHIN,
                "zone {zone} never {wanted:?}: {:?}",
                transport_of_zone(harness, zone)
            );

            thread::sleep(LOOKED_AGAIN_AFTER);
        }
    }

    fn playing_in_the_kitchen_unseen() -> (Harness, ZoneId) {
        let harness = Harness::sounding();

        harness.resumed("ab", 0, 0);

        let kitchen = harness
            .player
            .zone_added("Kitchen", 0, String::new())
            .expect("a second zone")
            .id;

        harness
            .player
            .show_zone(kitchen.get())
            .expect("the kitchen shown");
        harness
            .player
            .load(harness.ids("c"), 0, true)
            .expect("the kitchen playing");
        settled_at(&harness, kitchen, Transport::Playing);
        harness
            .player
            .show_zone(ZoneId::FIRST.get())
            .expect("the first zone shown");
        (harness, kitchen)
    }

    #[test]
    fn a_zone_playing_unseen_is_heard_in_the_snapshot_of_the_zone_shown() {
        let (harness, kitchen) = playing_in_the_kitchen_unseen();

        let shown = harness.settled(|snapshot| snapshot.heard_elsewhere.is_some());
        let heard = shown.heard_elsewhere.expect("the kitchen heard");

        assert_eq!(shown.zone, ZoneId::FIRST.get());
        assert!(!shown.transport.sounds());
        assert_eq!(heard.zone, kitchen.get());
        assert!(heard.transport.sounds());
        assert_eq!(heard.current.as_ref().map(name_of), Some('c'));

        harness
            .player
            .show_zone(kitchen.get())
            .expect("the kitchen shown");

        let kitchen_shown = harness.settled(|snapshot| snapshot.zone == kitchen.get());

        assert!(kitchen_shown.transport.sounds());
        assert!(kitchen_shown.heard_elsewhere.is_none());
    }

    #[test]
    fn the_zones_that_sound_are_paused_whichever_is_shown_and_named_so_they_play_again() {
        let (harness, kitchen) = playing_in_the_kitchen_unseen();

        harness.player.play().expect("the first zone playing");
        settled_at(&harness, ZoneId::FIRST, Transport::Playing);

        let paused = harness
            .player
            .pause_sounding_zones()
            .expect("the zones paused");

        settled_at(&harness, ZoneId::FIRST, Transport::Paused);
        settled_at(&harness, kitchen, Transport::Paused);

        assert_eq!(paused, vec![ZoneId::FIRST.get(), kitchen.get()]);
        assert_eq!(
            harness.player.pause_sounding_zones().expect("nothing left"),
            Vec::<u32>::new()
        );

        harness
            .player
            .play_zones(vec![kitchen.get(), 99])
            .expect("the kitchen playing again");
        settled_at(&harness, kitchen, Transport::Playing);

        assert_eq!(
            transport_of_zone(&harness, ZoneId::FIRST),
            Transport::Paused
        );
    }

    #[test]
    fn every_zone_is_stopped_though_another_is_shown() {
        let (harness, kitchen) = playing_in_the_kitchen_unseen();

        harness
            .player
            .stop_every_zone()
            .expect("every zone stopped");

        settled_at(&harness, kitchen, Transport::Stopped);
        settled_at(&harness, ZoneId::FIRST, Transport::Stopped);
    }
}
