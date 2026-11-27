use resonate_core::{
    ChannelCount, SampleRate, Trim,
    abx::TRIALS_AT_MOST,
    eq::{BandGain, Frequency, HeadYaw, MAX_BANDS, Q, SpeakerAngle, SpeakerElevation},
    wrapped::{FIRST_YEAR_HELD, LAST_YEAR_HELD},
};
use resonate_dsp::Error as Dsp;
use resonate_engine::{Cause, CommandKind, Event, PlaybackState, PlayerState, QueueRows};
use resonate_eq::Error as Eq;
use resonate_library::{Error as Library, ListenFault, LookupOp};

use crate::{
    MobilePlayer,
    documents::document_uri,
    measurement::MeasuredFile,
    words::{self, domain, failure as said},
    zoning::Zoned,
};

impl MobilePlayer {
    pub(crate) fn still_waiting(zoned: &Zoned, state: &PlayerState) -> Option<String> {
        let mut waiting = zoned.waiting.lock();

        if waiting_is_over(state) {
            *waiting = None;
        }
        waiting.clone()
    }

    pub(crate) fn failure_told(&self, rows: &QueueRows, event: Event) -> Option<String> {
        match event {
            Event::Failed { track, error } => {
                tracing::error!(%error, %track, "playback failed");

                let title = rows
                    .iter()
                    .find(|item| item.id == track)
                    .and_then(|item| document_uri(&item.location).ok())
                    .and_then(|uri| self.catalog.track_uri(&uri).ok())
                    .map(|found| found.title)
                    .filter(|title| !title.is_empty());
                Some(match title {
                    Some(title) => would_not_play(error.cause(), &title),
                    None => would_not_do(CommandKind::Play, error.cause()),
                })
            }
            Event::CommandFailed { command, error } => {
                tracing::warn!(%error, ?command, "the engine refused a command");
                Some(would_not_do(command, error.cause()))
            }
            _ => None,
        }
    }
}

pub(crate) fn waiting_told(event: &Event) -> Option<String> {
    let Event::Waiting { track, error } = event else {
        return None;
    };

    tracing::warn!(%error, %track, "playback waits for a device");
    Some(waits_for_a_device(error.cause()).to_owned())
}

fn waiting_is_over(state: &PlayerState) -> bool {
    state.output.is_some() || matches!(state.playback, PlaybackState::Idle | PlaybackState::Stopped)
}

fn waits_for_a_device(cause: Cause) -> &'static str {
    match cause {
        Cause::NoDevice => said::no_device_waiting(),
        _ => said::device_stopped_waiting(),
    }
}

pub(crate) fn would_not_play(cause: Cause, title: &str) -> String {
    match cause {
        Cause::Unreadable => said::title_unreadable(title),
        Cause::Unsupported => said::title_unsupported(title),
        Cause::Damaged => said::title_damaged(title),
        Cause::CannotSeek => said::title_cannot_seek(title),
        cause => plainly(cause).to_owned(),
    }
}

pub(crate) fn would_not_do(command: CommandKind, cause: Cause) -> String {
    let told = match cause {
        Cause::Unreadable | Cause::Unsupported | Cause::Damaged => said::song_unopened(),
        Cause::CannotSeek => said::song_cannot_seek(),
        Cause::NothingPlaying => match command {
            CommandKind::Pause | CommandKind::Seek | CommandKind::SeekBy => said::nothing_playing(),
            _ => said::nothing_queued(),
        },
        cause => plainly(cause),
    };

    told.to_owned()
}

pub(crate) fn of_a_command(cause: Cause) -> &'static str {
    match cause {
        Cause::Unreadable | Cause::Unsupported | Cause::Damaged => said::song_unopened(),
        cause => plainly(cause),
    }
}

pub(crate) fn of_a_player(error: &resonate_engine::Error) -> &'static str {
    use resonate_engine::Error as Player;

    match error {
        Player::NothingToAudition => said::nothing_to_audition(),
        Player::ArmsAlike => said::arms_alike(),
        Player::LevelsUnmeasured => said::levels_unmeasured(),
        Player::LevelsTooFarApart { .. } => said::levels_too_far_apart(),
        other => of_a_command(other.cause()),
    }
}

pub(crate) fn of_a_domain(refusal: &resonate_core::Error) -> String {
    use resonate_core::Error as Core;

    match *refusal {
        Core::SampleRateOutOfRange(rate) => domain::SampleRateOutOfRange {
            highest: SampleRate::MAX_HZ,
            lowest: SampleRate::MIN_HZ,
            rate,
        }
        .said(),
        Core::ChannelCountOutOfRange(count) => domain::ChannelCountOutOfRange {
            count,
            most: ChannelCount::MAX.get(),
        }
        .said(),
        Core::BitDepthNotRepresentable(bits) => domain::bit_depth_not_representable(bits),
        Core::DecibelsNotFinite(decibels) => domain::decibels_not_finite(decibels.to_string()),
        Core::TrimOutOfRange(millibels) => domain::TrimOutOfRange {
            millibels,
            most: Trim::WIDEST_MILLIBELS,
        }
        .said(),
        Core::GainNotFinite(gain) => domain::gain_not_finite(gain.to_string()),
        Core::VolumeOutOfRange(volume) => domain::volume_out_of_range(volume.to_string()),
        Core::BandFrequencyOutOfRange(centihertz) => domain::BandFrequencyOutOfRange {
            hertz: hundredths(centihertz.into()),
            highest: hundredths(Frequency::HIGHEST_CENTIHERTZ.into()),
            lowest: hundredths(Frequency::LOWEST_CENTIHERTZ.into()),
        }
        .said(),
        Core::BandGainOutOfRange(milli) => domain::BandGainOutOfRange {
            decibels: thousandths(milli.into()),
            most: thousandths(BandGain::WIDEST_MILLI_DECIBELS.into()),
        }
        .said(),
        Core::BandQOutOfRange(milli) => domain::BandQOutOfRange {
            least: thousandths(Q::WIDEST_MILLI.into()),
            most: thousandths(Q::NARROWEST_MILLI.into()),
            q: thousandths(milli.into()),
        }
        .said(),
        Core::PreampOutOfRange(milli) => domain::PreampOutOfRange {
            decibels: thousandths(milli.into()),
            most: thousandths(BandGain::WIDEST_MILLI_DECIBELS.into()),
        }
        .said(),
        Core::SpeakerAngleOutOfRange(degrees) => domain::SpeakerAngleOutOfRange {
            degrees,
            least: SpeakerAngle::NARROWEST_DEGREES,
            most: SpeakerAngle::WIDEST_DEGREES,
        }
        .said(),
        Core::SpeakerElevationOutOfRange(degrees) => domain::SpeakerElevationOutOfRange {
            degrees,
            highest: SpeakerElevation::HIGHEST_DEGREES,
            lowest: SpeakerElevation::LOWEST_DEGREES,
        }
        .said(),
        Core::HeadYawOutOfRange(degrees) => domain::HeadYawOutOfRange {
            degrees,
            leftmost: HeadYaw::LEFTMOST_DEGREES,
            rightmost: HeadYaw::RIGHTMOST_DEGREES,
        }
        .said(),
        Core::TooManyBands(found) => words::errors::TooManyBands {
            found,
            most: MAX_BANDS,
        }
        .said(),
        Core::AbxTrialsOutOfRange(trials) => words::errors::ListeningTestTrials {
            most: TRIALS_AT_MOST,
            trials: u32::try_from(trials).unwrap_or(u32::MAX),
        }
        .said(),
        Core::AbxFinished => words::errors::listening_test_finished().to_owned(),
        Core::LevelsNotMatched { millibels } => words::errors::listening_test_levels(millibels),
        Core::ZeroId => domain::zero_id().to_owned(),
        Core::SourceNameNotUsable => domain::source_name_not_usable().to_owned(),
        Core::NotAnMbid => domain::not_an_mbid().to_owned(),
        Core::NotAnIsrc => domain::not_an_isrc().to_owned(),
        Core::NotAChromaprint => domain::not_a_chromaprint().to_owned(),
        Core::YearNotHeld(year) => domain::YearNotHeld {
            first: FIRST_YEAR_HELD,
            last: LAST_YEAR_HELD,
            year,
        }
        .said(),
    }
}

fn hundredths(whole: i64) -> String {
    (whole as f64 / 100.0).to_string()
}

fn thousandths(whole: i64) -> String {
    (whole as f64 / 1000.0).to_string()
}

pub(crate) fn of_a_read(read: &resonate_codec::Error) -> &'static str {
    match Cause::of_a_read(read) {
        Cause::Unreadable => said::file_unreadable(),
        Cause::Unsupported => said::file_unsupported(),
        Cause::Damaged => said::file_damaged(),
        Cause::CannotSeek => said::file_cannot_seek(),
        cause => plainly(cause),
    }
}

pub(crate) fn of_a_lookup(op: LookupOp, source: &Library) -> String {
    let asked = op.asked_for();
    match source {
        Library::Unreachable { .. } => said::lookup_unreachable(asked),
        Library::Offline { .. } => said::lookup_offline(asked),
        Library::Metered { .. } => said::lookup_metered(asked),
        Library::Refused { status, .. } => said::LookupRefused {
            asked,
            status: *status,
        }
        .said(),
        Library::Unreadable { .. } => said::lookup_unreadable(asked),
        Library::TooLarge { .. } => said::lookup_too_large(asked),
        Library::RefusedInARow { refusals, .. } => said::LookupRefusedInARow {
            asked,
            refusals: *refusals,
        }
        .said(),
        _ => said::lookup_failed(asked),
    }
}

pub(crate) fn of_the_history(source: &Library) -> String {
    match source {
        Library::ListenLine { line, fault } => said::HistoryLine {
            fault: fault_named(*fault),
            line: *line,
        }
        .said(),
        Library::Store { .. } => said::history_unkept().to_owned(),
        other => of_a_lookup(LookupOp::History, other),
    }
}

const fn fault_named(fault: ListenFault) -> &'static str {
    match fault {
        ListenFault::NotUtf8 => "not-utf8",
        ListenFault::NotJson => "not-json",
        ListenFault::NotAnObject => "not-an-object",
        ListenFault::NoMoment => "no-moment",
        ListenFault::NoTrackMetadata => "no-track-metadata",
    }
}

pub(crate) fn of_a_measurement(file: MeasuredFile, source: &Eq) -> String {
    const MIB: usize = 1024 * 1024;

    let file = file.to_string();
    match source {
        Eq::MeasurementTooLarge { limit } => said::MeasurementTooLarge {
            file: &file,
            mebibytes: limit / MIB,
        }
        .said(),
        Eq::NoMeasuredPoints => said::measurement_no_points(&file),
        Eq::TooFewMeasuredPoints { found, least } => said::MeasurementTooFewPoints {
            file: &file,
            found: *found,
            least: *least,
        }
        .said(),
        Eq::ResponseMissesTheMidBand {
            lowest_hertz,
            highest_hertz,
        } => said::MeasurementMissesTheMidBand {
            file: &file,
            highest: format!("{highest_hertz:.0}"),
            lowest: format!("{lowest_hertz:.0}"),
        }
        .said(),
        Eq::NothingToCorrect { off_by_db } => {
            said::measurement_nothing_to_correct(format!("{off_by_db:.1}"))
        }
        _ => said::measurement_unreadable(&file),
    }
}

pub(crate) fn of_a_sweep(source: &Dsp) -> String {
    match source {
        Dsp::SweepBand { rate, .. } => said::sweep_band(rate.to_string()),
        Dsp::HeardTooShort { heard, played } => said::SweepHeardTooShort {
            heard: *heard,
            played: *played,
        }
        .said(),
        Dsp::HeardTooLong { .. } => said::sweep_heard_too_long().to_owned(),
        Dsp::NothingHeard => said::sweep_nothing_heard().to_owned(),
        Dsp::SweepLostInNoise { .. } => said::sweep_lost_in_noise().to_owned(),
        _ => said::sweep_unmeasured().to_owned(),
    }
}

fn plainly(cause: Cause) -> &'static str {
    match cause {
        Cause::NoDevice => said::no_device(),
        Cause::DeviceGone => said::device_gone(),
        Cause::SoundServer => said::sound_server(),
        Cause::DeviceRefused => said::device_refused(),
        Cause::QueueMoved => said::queue_moved(),
        Cause::PlayerStopped => said::player_stopped(),
        Cause::NothingPlaying => said::nothing_playing(),
        Cause::CannotSeek => said::song_cannot_seek(),
        Cause::Unreadable => said::unreadable(),
        Cause::Unsupported => said::unsupported(),
        Cause::Damaged => said::damaged(),
        Cause::HeldByAnotherZone => said::held_by_another_zone(),
        Cause::ZoneRefused => said::zone_refused(),
        Cause::NotHandedOver => said::not_handed_over(),
        Cause::NothingToCompare => said::nothing_to_compare(),
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::{MediaLocation, SourceId, TrackId};

    use super::*;

    #[test]
    fn a_missing_file_says_it_may_have_moved_and_a_damaged_one_says_so() {
        assert_eq!(
            would_not_play(Cause::Unreadable, "Slow Water"),
            "Couldn't find “Slow Water” — it may have been moved or deleted"
        );
        assert_eq!(
            would_not_play(Cause::Damaged, "Slow Water"),
            "Couldn't play “Slow Water” — the file looks damaged"
        );
        assert_eq!(
            would_not_play(Cause::NoDevice, "Slow Water"),
            "No speakers or headphones to play through"
        );
    }

    #[test]
    fn a_device_waited_for_is_told_as_the_desktop_tells_it_and_nothing_else_is() {
        let track = TrackId::new(1).expect("an id");
        let none_there = Event::Waiting {
            track,
            error: resonate_engine::Error::Sink(resonate_audio::Error::NoSink),
        };
        let gone = Event::Waiting {
            track,
            error: resonate_engine::Error::Sink(resonate_audio::Error::SinkGone {
                node: resonate_audio::SinkId::new(4),
            }),
        };
        let failed = Event::Failed {
            track,
            error: resonate_engine::Error::Sink(resonate_audio::Error::NoSink),
        };

        assert_eq!(
            waiting_told(&none_there).as_deref(),
            Some("No speakers or headphones to play through — playing on once one is there")
        );
        assert_eq!(
            waiting_told(&gone).as_deref(),
            Some("The audio device stopped playing — playing on once one is there")
        );
        assert_eq!(waiting_told(&failed), None);
    }

    #[test]
    fn a_wait_lasts_through_a_pause_and_ends_with_the_transport() {
        let waiting_at = |playback| PlayerState {
            playback,
            ..PlayerState::default()
        };

        assert!(!waiting_is_over(&waiting_at(PlaybackState::Buffering)));
        assert!(!waiting_is_over(&waiting_at(PlaybackState::Paused)));
        assert!(waiting_is_over(&waiting_at(PlaybackState::Stopped)));
        assert!(waiting_is_over(&waiting_at(PlaybackState::Idle)));
    }

    #[test]
    fn a_refused_command_names_why() {
        assert_eq!(
            would_not_do(CommandKind::Pause, Cause::NothingPlaying),
            "Nothing is playing right now"
        );
        assert_eq!(
            would_not_do(CommandKind::Next, Cause::NothingPlaying),
            "There's nothing in the queue to play"
        );
        assert_eq!(
            would_not_do(CommandKind::Play, Cause::Unsupported),
            "That song couldn't be opened"
        );
    }

    #[test]
    fn a_lookup_failure_names_what_was_asked_and_how_it_failed() {
        let refused = Library::Refused {
            op: LookupOp::Cover,
            status: 503,
        };

        assert_eq!(
            of_a_lookup(LookupOp::Cover, &refused),
            "The online service refused the request for a cover (503)"
        );
        assert_eq!(
            of_a_lookup(
                LookupOp::Lyrics,
                &Library::Unreadable {
                    op: LookupOp::Lyrics
                }
            ),
            "The online service's answer about lyrics could not be read"
        );
    }

    #[test]
    fn a_lookup_a_switched_off_network_or_a_metered_connection_held_back_is_not_told_unreachable() {
        let offline = Library::Offline {
            op: LookupOp::Cover,
        };
        let metered = Library::Metered {
            op: LookupOp::Lyrics,
        };

        assert_eq!(
            of_a_lookup(LookupOp::Cover, &offline),
            "Online features are switched off, so nothing was asked about a cover"
        );
        assert_eq!(
            of_a_lookup(LookupOp::Lyrics, &metered),
            "The connection is metered, so nothing was asked about lyrics"
        );
    }

    #[test]
    fn a_read_failure_is_told_by_its_cause() {
        let gone = resonate_codec::Error::NoSuchSource {
            location: MediaLocation::new(SourceId::new("content").expect("a source name"), "x"),
        };

        assert_eq!(of_a_read(&gone), "The audio file couldn't be found or read");
    }
}
