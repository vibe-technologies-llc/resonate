use resonate_engine::{Cause, CommandKind, Event, QueueItem};
use resonate_library::{Error as Library, LookupOp};

use crate::{MobilePlayer, documents::document_uri};

impl MobilePlayer {
    pub(crate) fn failure_told(&self, rows: &[QueueItem], event: Event) -> Option<String> {
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
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::{MediaLocation, SourceId};

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
