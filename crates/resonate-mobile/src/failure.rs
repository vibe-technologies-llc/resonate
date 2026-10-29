use resonate_engine::{Cause, CommandKind, Event, PlaybackState, PlayerState, QueueRows};
use resonate_eq::Error as Eq;
use resonate_library::{Error as Library, LookupOp};

use crate::{MobilePlayer, documents::document_uri, measurement::MeasuredFile, zoning::Zoned};

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

const fn waits_for_a_device(cause: Cause) -> &'static str {
    match cause {
        Cause::NoDevice => {
            "No speakers or headphones to play through — playing on once one is there"
        }
        _ => "The audio device stopped playing — playing on once one is there",
    }
}

pub(crate) fn would_not_play(cause: Cause, title: &str) -> String {
    match cause {
        Cause::Unreadable => format!("Couldn't find “{title}” — it may have been moved or deleted"),
        Cause::Unsupported => format!("“{title}” is in a format Resonate can't play"),
        Cause::Damaged => format!("Couldn't play “{title}” — the file looks damaged"),
        Cause::CannotSeek => format!("Couldn't jump around in “{title}”"),
        cause => plainly(cause).to_owned(),
    }
}

pub(crate) fn would_not_do(command: CommandKind, cause: Cause) -> String {
    match cause {
        Cause::Unreadable | Cause::Unsupported | Cause::Damaged => {
            "That song couldn't be opened".to_owned()
        }
        Cause::CannotSeek => "This song can't be skipped through".to_owned(),
        Cause::NothingPlaying => match command {
            CommandKind::Pause | CommandKind::Seek | CommandKind::SeekBy => {
                "Nothing is playing right now".to_owned()
            }
            _ => "There's nothing in the queue to play".to_owned(),
        },
        cause => plainly(cause).to_owned(),
    }
}

pub(crate) const fn of_a_command(cause: Cause) -> &'static str {
    match cause {
        Cause::Unreadable | Cause::Unsupported | Cause::Damaged => "That song couldn't be opened",
        cause => plainly(cause),
    }
}

pub(crate) const fn of_a_player(error: &resonate_engine::Error) -> &'static str {
    use resonate_engine::Error as Player;

    match error {
        Player::NothingToAudition => {
            "Play a song first: the test is heard on one playing, and a DSD stream sent as it is cannot be switched within"
        }
        Player::ArmsAlike => {
            "A and B would sound the same on this stream, so there is nothing to tell apart"
        }
        Player::LevelsUnmeasured => {
            "A and B could not be measured against each other, so the test cannot level them"
        }
        Player::LevelsTooFarApart { .. } => {
            "A and B play too far apart in loudness to be matched, so the test cannot level them"
        }
        other => of_a_command(other.cause()),
    }
}

pub(crate) const fn of_a_read(read: &resonate_codec::Error) -> &'static str {
    match Cause::of_a_read(read) {
        Cause::Unreadable => "The audio file couldn't be found or read",
        Cause::Unsupported => "The audio file is in a format Resonate can't play",
        Cause::Damaged => "The audio file looks damaged",
        Cause::CannotSeek => "The audio file can't be skipped through",
        cause => plainly(cause),
    }
}

pub(crate) fn of_a_lookup(op: LookupOp, source: &Library) -> String {
    let asked = op.asked_for();
    match source {
        Library::Unreachable { .. } => {
            format!("The online service could not be reached for {asked}")
        }
        Library::Refused { status, .. } => {
            format!("The online service refused the request for {asked} ({status})")
        }
        Library::Unreadable { .. } => {
            format!("The online service's answer about {asked} could not be read")
        }
        Library::TooLarge { .. } => {
            format!("The online service's answer about {asked} was too large to read")
        }
        Library::RefusedInARow { refusals, .. } => {
            format!("The online service refused {refusals} requests in a row, the last for {asked}")
        }
        _ => format!("The online lookup of {asked} failed"),
    }
}

pub(crate) fn of_a_measurement(file: MeasuredFile, source: &Eq) -> String {
    const MIB: usize = 1024 * 1024;
    match source {
        Eq::MeasurementTooLarge { limit } => {
            format!("The {file} file is larger than {} MiB", limit / MIB)
        }
        Eq::NoMeasuredPoints => {
            format!("No line of the {file} file reads as a frequency and a level")
        }
        Eq::TooFewMeasuredPoints { found, least } => {
            format!("The {file} file holds {found} points and a response needs at least {least}")
        }
        Eq::ResponseMissesTheMidBand {
            lowest_hertz,
            highest_hertz,
        } => format!(
            "The {file} spans {lowest_hertz:.0} Hz to {highest_hertz:.0} Hz and must reach 500 Hz to 2 kHz"
        ),
        Eq::NothingToCorrect { off_by_db } => {
            format!(
                "The measurement is within {off_by_db:.1} dB of the target, so there is nothing to correct"
            )
        }
        _ => format!("The {file} file could not be read"),
    }
}

const fn plainly(cause: Cause) -> &'static str {
    match cause {
        Cause::NoDevice => "No speakers or headphones to play through",
        Cause::DeviceGone => "The audio device was unplugged or switched off",
        Cause::SoundServer => "Lost touch with the sound system — trying again",
        Cause::DeviceRefused => "The audio device wouldn't play this song's format",
        Cause::QueueMoved => "The queue changed before that could happen",
        Cause::PlayerStopped => "The player stopped responding — restart Resonate",
        Cause::NothingPlaying => "Nothing is playing right now",
        Cause::CannotSeek => "This song can't be skipped through",
        Cause::Unreadable => "A song couldn't be found",
        Cause::Unsupported => "A song is in a format Resonate can't play",
        Cause::Damaged => "A song couldn't be played",
        Cause::HeldByAnotherZone => "Another zone plays that device bit-perfect",
        Cause::ZoneRefused => "That zone is no longer there",
        Cause::NotHandedOver => "The other zone didn't take the playback over",
        Cause::NothingToCompare => "The two sides of the listening test can't be told apart here",
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
