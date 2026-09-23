use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use resonate_core::{Frames, PlaylistId, TrackId};
use resonate_engine::{
    Asleep, MediaInfo, PlaybackState, PlayerState, QueueItem, RepeatMode, StreamDigest, TagSet,
    Until,
};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use crate::host::Heard;

pub(crate) const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
const NO_PLAYLIST: &str = "/org/resonate/playlist/none";
const TRACK_PREFIX: &str = "/org/resonate/track/";
const PLAYLIST_PREFIX: &str = "/org/resonate/playlist/";

const MICROS_PER_SECOND: u128 = 1_000_000;
const SECONDS_PER_DAY: i64 = 86_400;
const SECONDS_PER_HOUR: i64 = 3_600;
const SECONDS_PER_MINUTE: i64 = 60;
const DAYS_FROM_0000_03_01_TO_THE_EPOCH: i64 = 719_468;
const DAYS_PER_ERA: i64 = 146_097;
const LAST_DAY_OF_AN_ERA: i64 = DAYS_PER_ERA - 1;
const YEARS_PER_ERA: i64 = 400;
const DAYS_PER_COMMON_YEAR: i64 = 365;
const DAYS_PER_FOUR_YEARS: i64 = 1_460;
const DAYS_PER_CENTURY: i64 = 36_524;
const DAYS_PER_FIVE_MONTHS: i64 = 153;
const MARCH: i64 = 3;
const MONTHS_PER_YEAR: i64 = 12;

pub(crate) fn track_path(track: TrackId) -> OwnedObjectPath {
    let path = format!("{TRACK_PREFIX}{}", track.get());
    ObjectPath::try_from(path).map_or_else(
        |_| no_track(),
        |path| OwnedObjectPath::from(path.to_owned()),
    )
}

pub(crate) fn track_of(path: &str) -> Option<TrackId> {
    TrackId::new(path.strip_prefix(TRACK_PREFIX)?.parse().ok()?).ok()
}

pub(crate) fn no_track() -> OwnedObjectPath {
    ObjectPath::from_static_str_unchecked(NO_TRACK).into()
}

pub(crate) fn playlist_path(playlist: PlaylistId) -> OwnedObjectPath {
    let path = format!("{PLAYLIST_PREFIX}{}", playlist.get());
    ObjectPath::try_from(path).map_or_else(
        |_| no_playlist(),
        |path| OwnedObjectPath::from(path.to_owned()),
    )
}

pub(crate) fn playlist_of(path: &str) -> Option<PlaylistId> {
    PlaylistId::new(path.strip_prefix(PLAYLIST_PREFIX)?.parse().ok()?).ok()
}

pub(crate) fn no_playlist() -> OwnedObjectPath {
    ObjectPath::from_static_str_unchecked(NO_PLAYLIST).into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

impl PlaybackStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Playing => "Playing",
            Self::Paused => "Paused",
            Self::Stopped => "Stopped",
        }
    }

    pub fn read(status: &str) -> Option<Self> {
        [Self::Playing, Self::Paused, Self::Stopped]
            .into_iter()
            .find(|candidate| candidate.as_str() == status)
    }
}

impl From<PlaybackState> for PlaybackStatus {
    fn from(playback: PlaybackState) -> Self {
        match playback {
            PlaybackState::Playing | PlaybackState::Buffering => Self::Playing,
            PlaybackState::Paused => Self::Paused,
            PlaybackState::Idle | PlaybackState::Stopped => Self::Stopped,
        }
    }
}

pub(crate) const fn sounding(playback: PlaybackState) -> bool {
    matches!(playback, PlaybackState::Playing | PlaybackState::Buffering)
}

pub(crate) fn playing_digest<'a>(
    state: &PlayerState,
    digest: Option<&'a Arc<StreamDigest>>,
) -> Option<&'a StreamDigest> {
    let current = state.current?;
    digest
        .filter(|digest| digest.track == current.id)
        .map(Arc::as_ref)
}

pub(crate) fn loop_status(repeat: RepeatMode) -> &'static str {
    match repeat {
        RepeatMode::Off => "None",
        RepeatMode::Track => "Track",
        RepeatMode::Queue => "Playlist",
    }
}

pub(crate) fn repeat_mode(status: &str) -> Option<RepeatMode> {
    match status {
        "None" => Some(RepeatMode::Off),
        "Track" => Some(RepeatMode::Track),
        "Playlist" => Some(RepeatMode::Queue),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SleepMode {
    Off,
    After,
    EndOfTrack,
    EndOfQueue,
}

impl SleepMode {
    pub(crate) const ALL: [Self; 4] = [Self::Off, Self::After, Self::EndOfTrack, Self::EndOfQueue];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::After => "after",
            Self::EndOfTrack => "end-of-track",
            Self::EndOfQueue => "end-of-queue",
        }
    }

    pub(crate) fn read(mode: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == mode)
    }

    pub(crate) const fn of(until: Until) -> Self {
        match until {
            Until::After(_) => Self::After,
            Until::EndOfTrack => Self::EndOfTrack,
            Until::EndOfQueue => Self::EndOfQueue,
        }
    }

    pub(crate) const fn until(self, seconds: u64) -> Option<Until> {
        match self {
            Self::Off => None,
            Self::After => Some(Until::After(Duration::from_secs(seconds))),
            Self::EndOfTrack => Some(Until::EndOfTrack),
            Self::EndOfQueue => Some(Until::EndOfQueue),
        }
    }

    pub(crate) fn every_one() -> String {
        Self::ALL
            .into_iter()
            .map(Self::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub(crate) type Sleep = (String, u64);

pub(crate) fn sleep_status(asleep: Option<Asleep>) -> Sleep {
    let Some(asleep) = asleep else {
        return (SleepMode::Off.as_str().to_owned(), 0);
    };

    (
        SleepMode::of(asleep.until).as_str().to_owned(),
        asleep.left.map_or(0, |left| left.as_secs()),
    )
}

pub(crate) fn sleep_asked(until: Option<Until>) -> Sleep {
    sleep_status(until.map(|until| Asleep {
        until,
        left: what_is_left(until),
    }))
}

pub(crate) fn sleep_timer(mode: &str, seconds: u64) -> Option<Asleep> {
    let until = SleepMode::read(mode)?.until(seconds)?;

    Some(Asleep {
        until,
        left: what_is_left(until),
    })
}

const fn what_is_left(until: Until) -> Option<Duration> {
    match until {
        Until::After(left) => Some(left),
        Until::EndOfTrack | Until::EndOfQueue => None,
    }
}

pub(crate) fn micros(frames: Frames, rate: u32) -> i64 {
    let rate = u128::from(rate.max(1));
    let micros = u128::from(frames.get()) * MICROS_PER_SECOND / rate;
    i64::try_from(micros).unwrap_or(i64::MAX)
}

pub(crate) fn frames(micros: u64, rate: u32) -> Frames {
    let micros = u128::from(micros);
    let rate = u128::from(rate.max(1));
    Frames(u64::try_from(micros * rate / MICROS_PER_SECOND).unwrap_or(u64::MAX))
}

pub(crate) fn metadata(
    state: &PlayerState,
    digest: Option<&Arc<StreamDigest>>,
    art: Option<String>,
    heard: Option<Heard>,
) -> HashMap<String, OwnedValue> {
    let mut fields = HashMap::new();
    let Some(current) = state.current else {
        insert(&mut fields, "mpris:trackid", no_track().into_inner());
        return fields;
    };

    insert(
        &mut fields,
        "mpris:trackid",
        track_path(current.id).into_inner(),
    );
    if let Some(duration) = current.duration {
        insert(
            &mut fields,
            "mpris:length",
            micros(duration, current.source.rate.hz()),
        );
    }
    absorb_plays(&mut fields, heard);

    let Some(digest) = playing_digest(state, digest) else {
        return fields;
    };
    insert(
        &mut fields,
        "xesam:url",
        digest.location.to_uri_within(digest.span),
    );
    if let Some(art) = art {
        insert(&mut fields, "mpris:artUrl", art);
    }
    absorb_tags(&mut fields, &digest.info.tags);
    fields
}

pub(crate) fn queued_metadata(
    item: &QueueItem,
    state: &PlayerState,
    digest: Option<&Arc<StreamDigest>>,
    media: Option<&MediaInfo>,
    playing_art: Option<String>,
    heard: Option<Heard>,
) -> HashMap<String, OwnedValue> {
    if state.current.is_some_and(|current| current.id == item.id) {
        return metadata(state, digest, playing_art, heard);
    }

    let mut fields = HashMap::new();
    insert(
        &mut fields,
        "mpris:trackid",
        track_path(item.id).into_inner(),
    );
    insert(
        &mut fields,
        "xesam:url",
        item.location.to_uri_within(item.span),
    );
    if let Some(stem) = item.location.stem() {
        insert(&mut fields, "xesam:title", stem.into_owned());
    }
    absorb_plays(&mut fields, heard);

    let Some(media) = media else {
        return fields;
    };
    if let Some(duration) = media.duration {
        insert(
            &mut fields,
            "mpris:length",
            micros(duration, media.spec.rate.hz()),
        );
    }
    absorb_tags(&mut fields, &media.tags);
    fields
}

pub(crate) fn unlisted_metadata(asked: &OwnedObjectPath) -> HashMap<String, OwnedValue> {
    let mut fields = HashMap::new();
    insert(&mut fields, "mpris:trackid", asked.clone().into_inner());
    fields
}

fn absorb_plays(fields: &mut HashMap<String, OwnedValue>, heard: Option<Heard>) {
    let Some(heard) = heard else {
        return;
    };
    insert(
        fields,
        "xesam:useCount",
        i32::try_from(heard.plays).unwrap_or(i32::MAX),
    );
    if let Some(played) = heard.played {
        insert(fields, "xesam:lastUsed", utc_stamp(played));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CivilDate {
    year: i64,
    month: i64,
    day: i64,
}

fn seconds_since_the_epoch(at: SystemTime) -> i64 {
    match at.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => {
            let before = before.duration();
            let whole = i64::try_from(before.as_secs()).unwrap_or(i64::MAX);
            let part_of_a_second = i64::from(before.subsec_nanos() > 0);
            whole.saturating_add(part_of_a_second).saturating_neg()
        }
    }
}

fn civil_from_days(days: i64) -> CivilDate {
    let shifted = days.saturating_add(DAYS_FROM_0000_03_01_TO_THE_EPOCH);
    let era = shifted.div_euclid(DAYS_PER_ERA);
    let day_of_era = shifted.rem_euclid(DAYS_PER_ERA);
    let year_of_era = (day_of_era - day_of_era / DAYS_PER_FOUR_YEARS
        + day_of_era / DAYS_PER_CENTURY
        - day_of_era / LAST_DAY_OF_AN_ERA)
        / DAYS_PER_COMMON_YEAR;
    let day_of_year =
        day_of_era - (DAYS_PER_COMMON_YEAR * year_of_era + year_of_era / 4 - year_of_era / 100);
    let months_from_march = (5 * day_of_year + 2) / DAYS_PER_FIVE_MONTHS;
    let day = day_of_year - (DAYS_PER_FIVE_MONTHS * months_from_march + 2) / 5 + 1;
    let month = months_from_march + MARCH;
    let month = if month > MONTHS_PER_YEAR {
        month - MONTHS_PER_YEAR
    } else {
        month
    };
    let year = year_of_era + era * YEARS_PER_ERA + i64::from(month <= 2);

    CivilDate { year, month, day }
}

pub fn utc_stamp(at: SystemTime) -> String {
    let seconds = seconds_since_the_epoch(at);
    let date = civil_from_days(seconds.div_euclid(SECONDS_PER_DAY));
    let time_of_day = seconds.rem_euclid(SECONDS_PER_DAY);
    let hour = time_of_day / SECONDS_PER_HOUR;
    let minute = time_of_day % SECONDS_PER_HOUR / SECONDS_PER_MINUTE;
    let second = time_of_day % SECONDS_PER_MINUTE;

    format!(
        "{:04}-{:02}-{:02}T{hour:02}:{minute:02}:{second:02}Z",
        date.year, date.month, date.day
    )
}

fn absorb_tags(fields: &mut HashMap<String, OwnedValue>, tags: &TagSet) {
    if let Some(title) = tags.title.as_ref() {
        insert(fields, "xesam:title", title.clone());
    }
    if let Some(artist) = tags.artist.as_ref() {
        insert(fields, "xesam:artist", vec![artist.clone()]);
    }
    if let Some(album) = tags.album.as_ref() {
        insert(fields, "xesam:album", album.clone());
    }
    if let Some(album_artist) = tags.album_artist.as_ref() {
        insert(fields, "xesam:albumArtist", vec![album_artist.clone()]);
    }
    if let Some(genre) = tags.genre.as_ref() {
        insert(fields, "xesam:genre", vec![genre.clone()]);
    }
    if let Some(date) = tags.date.as_ref() {
        insert(fields, "xesam:contentCreated", date.clone());
    }
    if let Some(track) = tags
        .track_number
        .and_then(|value| i32::try_from(value).ok())
    {
        insert(fields, "xesam:trackNumber", track);
    }
    if let Some(disc) = tags.disc_number.and_then(|value| i32::try_from(value).ok()) {
        insert(fields, "xesam:discNumber", disc);
    }
    if let Some(composer) = tags.credits.composer.as_ref() {
        insert(fields, "xesam:composer", vec![composer.clone()]);
    }
    if let Some(lyricist) = tags.credits.lyricist.as_ref() {
        insert(fields, "xesam:lyricist", vec![lyricist.clone()]);
    }
    if let Some(comment) = tags.comment.as_ref() {
        insert(fields, "xesam:comment", vec![comment.clone()]);
    }
    if let Some(beats) = tags
        .beats_per_minute
        .and_then(|value| i32::try_from(value).ok())
    {
        insert(fields, "xesam:audioBPM", beats);
    }
}

fn insert<'a, T: Into<Value<'a>>>(fields: &mut HashMap<String, OwnedValue>, key: &str, value: T) {
    let value: Value<'a> = value.into();
    match OwnedValue::try_from(value) {
        Ok(owned) => {
            fields.insert(key.to_owned(), owned);
        }
        Err(error) => tracing::debug!(%error, key, "a metadata field would not convert"),
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::MediaLocation;

    use super::*;

    fn at(seconds: i64) -> SystemTime {
        let magnitude = Duration::from_secs(seconds.unsigned_abs());
        if seconds < 0 {
            UNIX_EPOCH - magnitude
        } else {
            UNIX_EPOCH + magnitude
        }
    }

    fn count(fields: &HashMap<String, OwnedValue>, key: &str) -> Option<i32> {
        i32::try_from(fields.get(key)?.try_clone().ok()?).ok()
    }

    fn reads(fields: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
        String::try_from(fields.get(key)?.try_clone().ok()?).ok()
    }

    #[test]
    fn a_position_converts_to_microseconds_and_back_without_drifting() {
        for (frames, rate) in [(44_100_u64, 44_100), (0, 48_000), (96_000, 96_000)] {
            let micros = micros(Frames(frames), rate);
            assert_eq!(super::frames(micros.unsigned_abs(), rate), Frames(frames));
        }
        assert_eq!(micros(Frames(44_100), 44_100), 1_000_000);
    }

    #[test]
    fn every_loop_status_the_spec_names_maps_both_ways() {
        for repeat in [RepeatMode::Off, RepeatMode::Track, RepeatMode::Queue] {
            assert_eq!(repeat_mode(loop_status(repeat)), Some(repeat));
        }
        assert_eq!(repeat_mode("Sideways"), None);
    }

    #[test]
    fn every_sleep_mode_this_build_names_maps_both_ways() {
        for until in [
            Until::After(Duration::from_secs(900)),
            Until::EndOfTrack,
            Until::EndOfQueue,
        ] {
            let (mode, seconds) = sleep_asked(Some(until));

            assert_eq!(
                sleep_timer(&mode, seconds).map(|asleep| asleep.until),
                Some(until)
            );
        }
    }

    #[test]
    fn a_timer_nobody_set_reads_as_off_and_reads_back_as_nothing() {
        assert_eq!(sleep_status(None), ("off".to_owned(), 0));
        assert_eq!(sleep_asked(None), ("off".to_owned(), 0));
        assert_eq!(sleep_timer("off", 0), None);
        assert_eq!(sleep_timer("tomorrow", 0), None);
    }

    #[test]
    fn a_timer_with_no_clock_behind_it_has_no_seconds_left_to_say() {
        let asleep = Asleep {
            until: Until::EndOfQueue,
            left: None,
        };

        assert_eq!(sleep_status(Some(asleep)), ("end-of-queue".to_owned(), 0));
    }

    #[test]
    fn buffering_reads_as_playing_rather_than_as_a_state_of_its_own() {
        assert_eq!(
            PlaybackStatus::from(PlaybackState::Buffering),
            PlaybackStatus::Playing
        );
        assert_eq!(
            PlaybackStatus::from(PlaybackState::Playing),
            PlaybackStatus::Playing
        );
        assert_eq!(
            PlaybackStatus::from(PlaybackState::Paused),
            PlaybackStatus::Paused
        );
        assert_eq!(
            PlaybackStatus::from(PlaybackState::Idle),
            PlaybackStatus::Stopped
        );
        assert_eq!(
            PlaybackStatus::from(PlaybackState::Stopped),
            PlaybackStatus::Stopped
        );
    }

    #[test]
    fn a_status_read_back_off_the_bus_is_the_one_that_was_written() {
        for status in [
            PlaybackStatus::Playing,
            PlaybackStatus::Paused,
            PlaybackStatus::Stopped,
        ] {
            assert_eq!(PlaybackStatus::read(status.as_str()), Some(status));
        }
        assert_eq!(PlaybackStatus::read("Sideways"), None);
    }

    #[test]
    fn what_the_bus_calls_playing_is_what_the_desktop_is_told_there_is_sound_for() {
        for playback in [
            PlaybackState::Idle,
            PlaybackState::Buffering,
            PlaybackState::Playing,
            PlaybackState::Paused,
            PlaybackState::Stopped,
        ] {
            assert_eq!(
                sounding(playback),
                PlaybackStatus::from(playback) == PlaybackStatus::Playing
            );
        }
    }

    #[test]
    fn a_track_path_read_back_names_the_track_it_was_built_from() {
        let track = TrackId::new(7).expect("7 is non-zero");
        assert_eq!(track_of(track_path(track).as_str()), Some(track));
        assert_eq!(track_of(no_track().as_str()), None);
        assert_eq!(track_of("/org/resonate/track/0"), None);
        assert_eq!(track_of("/org/resonate/track/x"), None);
    }

    #[test]
    fn a_playlist_path_read_back_names_the_playlist_it_was_built_from() {
        let playlist = PlaylistId::new(4).expect("4 is non-zero");
        assert_eq!(
            playlist_of(playlist_path(playlist).as_str()),
            Some(playlist)
        );
        assert_eq!(playlist_of(no_playlist().as_str()), None);
        assert_eq!(playlist_of("/org/resonate/track/4"), None);
    }

    #[test]
    fn a_queued_track_nothing_has_decoded_still_carries_its_file() {
        let item = QueueItem {
            id: TrackId::new(3).expect("3 is non-zero"),
            location: MediaLocation::local("/music/Pink Floyd/Echoes.flac"),
            span: None,
        };
        let fields = queued_metadata(&item, &PlayerState::default(), None, None, None, None);

        assert_eq!(
            fields
                .get("xesam:title")
                .and_then(
                    |value| String::try_from(value.try_clone().expect("a cloneable value")).ok()
                ),
            Some("Echoes".to_owned())
        );
        assert!(fields.contains_key("xesam:url"));
        assert!(fields.contains_key("mpris:trackid"));
    }

    #[test]
    fn a_track_without_a_digest_still_carries_the_id_clients_key_on() {
        let state = PlayerState::default();
        let fields = metadata(&state, None, None, None);

        assert_eq!(fields.len(), 1);
        assert!(fields.contains_key("mpris:trackid"));
    }

    #[test]
    fn a_row_the_catalog_holds_carries_its_count_and_a_row_it_does_not_carries_neither_key() {
        let item = QueueItem {
            id: TrackId::new(3).expect("3 is non-zero"),
            location: MediaLocation::local("/music/Pink Floyd/Echoes.flac"),
            span: None,
        };
        let state = PlayerState::default();

        let unheard_of = queued_metadata(&item, &state, None, None, None, None);
        assert_eq!(count(&unheard_of, "xesam:useCount"), None);
        assert_eq!(reads(&unheard_of, "xesam:lastUsed"), None);

        let never_played = queued_metadata(
            &item,
            &state,
            None,
            None,
            None,
            Some(Heard {
                plays: 0,
                played: None,
            }),
        );
        assert_eq!(count(&never_played, "xesam:useCount"), Some(0));
        assert_eq!(reads(&never_played, "xesam:lastUsed"), None);

        let played = queued_metadata(
            &item,
            &state,
            None,
            None,
            None,
            Some(Heard {
                plays: 7,
                played: Some(at(1_000_000_000)),
            }),
        );
        assert_eq!(count(&played, "xesam:useCount"), Some(7));
        assert_eq!(
            reads(&played, "xesam:lastUsed").as_deref(),
            Some("2001-09-09T01:46:40Z")
        );
    }

    #[test]
    fn the_epoch_itself_is_the_first_second_of_nineteen_seventy() {
        assert_eq!(utc_stamp(at(0)), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn a_leap_day_and_the_seconds_either_side_of_it_are_each_their_own_date() {
        assert_eq!(utc_stamp(at(1_709_164_799)), "2024-02-28T23:59:59Z");
        assert_eq!(utc_stamp(at(1_709_164_800)), "2024-02-29T00:00:00Z");
        assert_eq!(utc_stamp(at(1_709_251_199)), "2024-02-29T23:59:59Z");
        assert_eq!(utc_stamp(at(1_709_251_200)), "2024-03-01T00:00:00Z");
    }

    #[test]
    fn a_year_and_a_month_roll_over_on_the_second_after_the_one_that_ends_them() {
        assert_eq!(utc_stamp(at(946_684_799)), "1999-12-31T23:59:59Z");
        assert_eq!(utc_stamp(at(946_684_800)), "2000-01-01T00:00:00Z");
        assert_eq!(utc_stamp(at(1_675_209_599)), "2023-01-31T23:59:59Z");
        assert_eq!(utc_stamp(at(1_675_209_600)), "2023-02-01T00:00:00Z");
    }

    #[test]
    fn a_century_divisible_by_four_hundred_leaps_and_one_that_is_not_does_not() {
        assert_eq!(utc_stamp(at(951_782_400)), "2000-02-29T00:00:00Z");
        assert_eq!(utc_stamp(at(4_107_542_399)), "2100-02-28T23:59:59Z");
        assert_eq!(utc_stamp(at(4_107_542_400)), "2100-03-01T00:00:00Z");
    }

    #[test]
    fn an_instant_before_the_epoch_counts_backwards_rather_than_stopping_at_it() {
        assert_eq!(utc_stamp(at(-1)), "1969-12-31T23:59:59Z");
        assert_eq!(utc_stamp(at(-14_182_940)), "1969-07-20T20:17:40Z");
        assert_eq!(utc_stamp(at(-2_208_988_800)), "1900-01-01T00:00:00Z");
        assert_eq!(utc_stamp(at(-2_203_891_201)), "1900-02-28T23:59:59Z");
        assert_eq!(utc_stamp(at(-2_203_891_200)), "1900-03-01T00:00:00Z");
    }

    #[test]
    fn a_part_of_a_second_before_the_epoch_lands_in_the_second_it_falls_inside() {
        assert_eq!(
            utc_stamp(UNIX_EPOCH - Duration::from_millis(500)),
            "1969-12-31T23:59:59Z"
        );
        assert_eq!(
            utc_stamp(UNIX_EPOCH + Duration::from_millis(500)),
            "1970-01-01T00:00:00Z"
        );
    }

    #[test]
    fn a_round_number_of_seconds_reads_as_the_date_it_is_known_to_be() {
        assert_eq!(utc_stamp(at(1_000_000_000)), "2001-09-09T01:46:40Z");
        assert_eq!(utc_stamp(at(2_000_000_000)), "2033-05-18T03:33:20Z");
        assert_eq!(utc_stamp(at(2_147_483_648)), "2038-01-19T03:14:08Z");
        assert_eq!(utc_stamp(at(4_000_000_000)), "2096-10-02T07:06:40Z");
    }

    #[test]
    fn a_time_further_ahead_than_a_year_has_digits_for_is_answered_rather_than_overflowing() {
        let furthest = UNIX_EPOCH
            .checked_add(Duration::from_secs(i64::MAX.unsigned_abs()))
            .expect("the platform clock reaches the end of a signed count of seconds");

        assert_eq!(utc_stamp(furthest), "292277026596-12-04T15:30:07Z");
    }

    #[test]
    fn every_day_of_a_leap_year_reads_back_as_the_day_it_was_counted_from() {
        let start_of_2024 = 1_704_067_200;
        let mut expected = (1_i64, 1_i64);
        let lengths = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

        for day in 0..366 {
            let stamp = utc_stamp(at(start_of_2024 + day * SECONDS_PER_DAY));
            assert_eq!(
                stamp,
                format!("2024-{:02}-{:02}T00:00:00Z", expected.0, expected.1)
            );
            let length = lengths[expected.0 as usize - 1];
            expected = if expected.1 == length {
                (expected.0 + 1, 1)
            } else {
                (expected.0, expected.1 + 1)
            };
        }
        assert_eq!(expected, (13, 1));
    }
}
