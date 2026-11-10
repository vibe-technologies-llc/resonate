use resonate_dsp::Error as Dsp;
use resonate_engine::{Cause, CommandKind, Event, PlaybackState, PlayerState, QueueRows};
use resonate_eq::Error as Eq;
use resonate_library::{Error as Library, LookupOp};

use crate::{
    MobilePlayer, documents::document_uri, measurement::MeasuredFile, words::failure as said,
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
        Library::Refused { status, .. } => said::lookup_refused(asked, *status),
        Library::Unreadable { .. } => said::lookup_unreadable(asked),
        Library::TooLarge { .. } => said::lookup_too_large(asked),
        Library::RefusedInARow { refusals, .. } => said::lookup_refused_in_a_row(asked, *refusals),
        _ => said::lookup_failed(asked),
    }
}

pub(crate) fn of_a_measurement(file: MeasuredFile, source: &Eq) -> String {
    const MIB: usize = 1024 * 1024;

    let file = file.to_string();
    match source {
        Eq::MeasurementTooLarge { limit } => said::measurement_too_large(&file, limit / MIB),
        Eq::NoMeasuredPoints => said::measurement_no_points(&file),
        Eq::TooFewMeasuredPoints { found, least } => {
            said::measurement_too_few_points(&file, *found, *least)
        }
        Eq::ResponseMissesTheMidBand {
            lowest_hertz,
            highest_hertz,
        } => said::measurement_misses_the_mid_band(
            &file,
            format!("{highest_hertz:.0}"),
            format!("{lowest_hertz:.0}"),
        ),
        Eq::NothingToCorrect { off_by_db } => {
            said::measurement_nothing_to_correct(format!("{off_by_db:.1}"))
        }
        _ => said::measurement_unreadable(&file),
    }
}

pub(crate) fn of_a_sweep(source: &Dsp) -> String {
    match source {
        Dsp::SweepBand { rate, .. } => said::sweep_band(rate.to_string()),
        Dsp::HeardTooShort { heard, played } => said::sweep_heard_too_short(*heard, *played),
        Dsp::HeardTooLong { .. } => said::sweep_heard_too_long().to_owned(),
        Dsp::NothingHeard => said::sweep_nothing_heard().to_owned(),
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
            would_not_play(Cause::Unreadable, "Echoes"),
            "Couldn't find “Echoes” — it may have been moved or deleted"
        );
        assert_eq!(
            would_not_play(Cause::Damaged, "Echoes"),
            "Couldn't play “Echoes” — the file looks damaged"
        );
        assert_eq!(
            would_not_play(Cause::NoDevice, "Echoes"),
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
    fn a_read_failure_is_told_by_its_cause() {
        let gone = resonate_codec::Error::NoSuchSource {
            location: MediaLocation::new(SourceId::new("content").expect("a source name"), "x"),
        };

        assert_eq!(of_a_read(&gone), "The audio file couldn't be found or read");
    }
}
