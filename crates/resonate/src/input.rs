use std::{
    io::{self, BufRead as _},
    thread,
};

use crossbeam_channel::{Receiver, bounded};

use crate::sleep::{Sleep, WITHOUT_A_SPEC};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Toggle,
    Next,
    Previous,
    Stop,
    SeekTo(u64),
    SeekBy(i64),
    VolumeBy(i32),
    ToggleShuffle,
    CycleRepeat,
    Sleep(Sleep),
    Help,
    Quit,
}

pub const HELP: &str = "\
  enter / p  play or pause    n  next track     b  previous track
  f [secs]   forward 10s      r [secs]  back 10s   <secs>  seek to
  + [step]   louder 5%        - [step]  quieter    s  shuffle
  l          cycle repeat     x  stop         q  quit        ?  this help
  z [spec]   sleep in 30m, or z <mins> / z track / z queue / z off";

const SEEK_STEP: i64 = 10;
const VOLUME_STEP: i32 = 5;

fn amount<T: std::str::FromStr>(rest: &str, default: T) -> T {
    if rest.is_empty() {
        return default;
    }
    rest.parse().unwrap_or(default)
}

pub fn parse(line: &str) -> Option<Action> {
    let line = line.trim();
    if line.is_empty() {
        return Some(Action::Toggle);
    }

    let mut characters = line.chars();
    let head = characters.next()?;
    let rest = characters.as_str().trim();

    match head {
        'p' => Some(Action::Toggle),
        'n' => Some(Action::Next),
        'b' => Some(Action::Previous),
        'x' => Some(Action::Stop),
        's' => Some(Action::ToggleShuffle),
        'l' => Some(Action::CycleRepeat),
        'q' => Some(Action::Quit),
        'h' | '?' => Some(Action::Help),
        'z' => asked_of_the_timer(rest).map(Action::Sleep),
        'f' => Some(Action::SeekBy(amount(rest, SEEK_STEP).abs())),
        'r' => Some(Action::SeekBy(-amount(rest, SEEK_STEP).abs())),
        '+' => Some(Action::VolumeBy(amount(rest, VOLUME_STEP).abs())),
        '-' => Some(Action::VolumeBy(-amount(rest, VOLUME_STEP).abs())),
        digit if digit.is_ascii_digit() => line.parse().ok().map(Action::SeekTo),
        _ => None,
    }
}

fn asked_of_the_timer(rest: &str) -> Option<Sleep> {
    if rest.is_empty() {
        return Some(WITHOUT_A_SPEC);
    }
    Sleep::read(rest).ok()
}

pub fn lines() -> Receiver<String> {
    let (send, receive) = bounded(16);
    let spawned = thread::Builder::new()
        .name("resonate-input".to_owned())
        .spawn(move || {
            for line in io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });

    if let Err(error) = spawned {
        tracing::warn!(%error, "no input thread; the transport can only be watched");
    }
    receive
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_newline_toggles_playback() {
        assert_eq!(parse(""), Some(Action::Toggle));
        assert_eq!(parse("   "), Some(Action::Toggle));
        assert_eq!(parse("p"), Some(Action::Toggle));
    }

    #[test]
    fn every_transport_key_maps_to_its_action() {
        assert_eq!(parse("n"), Some(Action::Next));
        assert_eq!(parse("b"), Some(Action::Previous));
        assert_eq!(parse("x"), Some(Action::Stop));
        assert_eq!(parse("s"), Some(Action::ToggleShuffle));
        assert_eq!(parse("l"), Some(Action::CycleRepeat));
        assert_eq!(parse("q"), Some(Action::Quit));
        assert_eq!(parse("?"), Some(Action::Help));
        assert_eq!(parse("h"), Some(Action::Help));
    }

    #[test]
    fn seeking_defaults_to_its_step_and_accepts_an_override() {
        assert_eq!(parse("f"), Some(Action::SeekBy(SEEK_STEP)));
        assert_eq!(parse("r"), Some(Action::SeekBy(-SEEK_STEP)));
        assert_eq!(parse("f 45"), Some(Action::SeekBy(45)));
        assert_eq!(parse("r 45"), Some(Action::SeekBy(-45)));
    }

    #[test]
    fn a_signed_override_never_flips_the_direction_its_key_chose() {
        assert_eq!(parse("f -45"), Some(Action::SeekBy(45)));
        assert_eq!(parse("r -45"), Some(Action::SeekBy(-45)));
        assert_eq!(parse("- -5"), Some(Action::VolumeBy(-5)));
    }

    #[test]
    fn volume_defaults_to_its_step_and_accepts_an_override() {
        assert_eq!(parse("+"), Some(Action::VolumeBy(VOLUME_STEP)));
        assert_eq!(parse("-"), Some(Action::VolumeBy(-VOLUME_STEP)));
        assert_eq!(parse("+20"), Some(Action::VolumeBy(20)));
        assert_eq!(parse("-20"), Some(Action::VolumeBy(-20)));
    }

    #[test]
    fn a_bare_number_seeks_to_that_second() {
        assert_eq!(parse("90"), Some(Action::SeekTo(90)));
        assert_eq!(parse(" 0 "), Some(Action::SeekTo(0)));
    }

    #[test]
    fn an_unparseable_override_falls_back_to_the_step() {
        assert_eq!(parse("f abc"), Some(Action::SeekBy(SEEK_STEP)));
        assert_eq!(parse("+ abc"), Some(Action::VolumeBy(VOLUME_STEP)));
    }

    #[test]
    fn the_sleep_key_takes_every_spec_and_a_bare_press_is_half_an_hour() {
        assert_eq!(parse("z"), Some(Action::Sleep(WITHOUT_A_SPEC)));
        assert_eq!(
            parse("z 45"),
            Some(Action::Sleep(Sleep::read("45").unwrap()))
        );
        assert_eq!(parse("z track"), Some(Action::Sleep(Sleep::EndOfTrack)));
        assert_eq!(parse("z queue"), Some(Action::Sleep(Sleep::EndOfQueue)));
        assert_eq!(parse("z off"), Some(Action::Sleep(Sleep::Off)));
    }

    #[test]
    fn a_sleep_spec_that_cannot_be_read_is_refused_rather_than_taking_the_default() {
        assert_eq!(parse("z soon"), None);
        assert_eq!(parse("z 0"), None);
    }

    #[test]
    fn a_key_that_means_nothing_is_not_an_action() {
        assert_eq!(parse("w"), None);
        assert_eq!(parse("9w"), None);
        assert_eq!(parse("/"), None);
    }
}
