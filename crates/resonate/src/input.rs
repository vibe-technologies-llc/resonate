use std::{
    io::{self, BufRead as _, Read as _},
    thread,
};

use crossbeam_channel::{Receiver, Sender, bounded};
use parking_lot::Mutex;
use rustix::termios::{self, LocalModes, OptionalActions, SpecialCodeIndex, Termios};

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

pub const KEYS: &str = "\
  space / p  play or pause    n  next track     b  previous track
  f / →      forward 10s      r / ←  back 10s      <secs> enter  seek to
  + / ↑      louder 5%        - / ↓  quieter       s  shuffle
  l          cycle repeat     x  stop         q  quit        ?  these keys
  z          sleep in 30m     :  type any line the piped form takes — :f 45, :z track";

const ESCAPE: u8 = 0x1b;
const INTRODUCER: u8 = b'[';
const UP: u8 = b'A';
const DOWN: u8 = b'B';
const RIGHT: u8 = b'C';
const LEFT: u8 = b'D';
const RUBOUT: u8 = 0x7f;
const BACKSPACE: u8 = 0x08;
const TYPED: u8 = b':';

static SAVED: Mutex<Option<Termios>> = Mutex::new(None);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pressed {
    Acted(Action),
    Typing(String),
    Unknown(String),
}

pub struct KeyAtATime;

impl KeyAtATime {
    pub fn where_a_terminal() -> Option<Self> {
        let stdin = io::stdin();
        if !termios::isatty(&stdin) {
            return None;
        }
        let saved = termios::tcgetattr(&stdin).ok()?;
        let mut keyed = saved.clone();
        keyed
            .local_modes
            .remove(LocalModes::ICANON | LocalModes::ECHO);
        keyed.special_codes[SpecialCodeIndex::VMIN] = 1;
        keyed.special_codes[SpecialCodeIndex::VTIME] = 0;
        termios::tcsetattr(&stdin, OptionalActions::Now, &keyed).ok()?;
        *SAVED.lock() = Some(saved);
        Some(Self)
    }
}

impl Drop for KeyAtATime {
    fn drop(&mut self) {
        restore_the_terminal();
    }
}

pub fn restore_the_terminal() {
    if let Some(saved) = SAVED.lock().take()
        && let Err(error) = termios::tcsetattr(io::stdin(), OptionalActions::Now, &saved)
    {
        tracing::warn!(%error, "the terminal was left reading a key at a time");
    }
}

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

fn read_as_typed(line: &str) -> Pressed {
    match parse(line) {
        Some(action) => Pressed::Acted(action),
        None => Pressed::Unknown(line.to_owned()),
    }
}

pub fn lines() -> Receiver<Pressed> {
    listening(|send| {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if send.send(read_as_typed(&line)).is_err() {
                break;
            }
        }
    })
}

pub fn keys() -> Receiver<Pressed> {
    listening(|send| {
        let mut bytes = io::stdin().lock().bytes().map_while(Result::ok);
        let mut keyed = Keyed::default();
        while let Some(byte) = bytes.next() {
            for pressed in keyed.pressed(byte, &mut bytes) {
                if send.send(pressed).is_err() {
                    return;
                }
            }
        }
    })
}

fn listening(read: impl FnOnce(Sender<Pressed>) + Send + 'static) -> Receiver<Pressed> {
    let (send, receive) = bounded(16);
    let spawned = thread::Builder::new()
        .name("resonate-input".to_owned())
        .spawn(move || read(send));

    if let Err(error) = spawned {
        tracing::warn!(%error, "no input thread; the transport can only be watched");
    }
    receive
}

#[derive(Default)]
struct Keyed {
    typing: Option<String>,
}

impl Keyed {
    fn pressed(&mut self, byte: u8, rest: &mut impl Iterator<Item = u8>) -> Vec<Pressed> {
        if byte == ESCAPE {
            let cancelled = self.typing.take().map(|_| Pressed::Typing(String::new()));
            let after = rest.next();
            if after == Some(INTRODUCER) {
                let arrow = rest.next().and_then(arrowed).map(Pressed::Acted);
                return cancelled.into_iter().chain(arrow).collect();
            }
            let mut pressed: Vec<Pressed> = cancelled.into_iter().collect();
            if let Some(after) = after {
                pressed.extend(self.pressed(after, rest));
            }
            return pressed;
        }

        if let Some(typed) = self.typing.as_mut() {
            return match byte {
                b'\r' | b'\n' => {
                    let line = self.typing.take().unwrap_or_default();
                    vec![
                        Pressed::Typing(String::new()),
                        read_as_typed(line.trim_start_matches(char::from(TYPED))),
                    ]
                }
                RUBOUT | BACKSPACE => {
                    typed.pop();
                    if typed.is_empty() {
                        self.typing = None;
                        vec![Pressed::Typing(String::new())]
                    } else {
                        vec![Pressed::Typing(typed.clone())]
                    }
                }
                printable if printable.is_ascii_graphic() || printable == b' ' => {
                    typed.push(char::from(printable));
                    vec![Pressed::Typing(typed.clone())]
                }
                _ => Vec::new(),
            };
        }

        if byte == TYPED || byte.is_ascii_digit() {
            let typed = char::from(byte).to_string();
            self.typing = Some(typed.clone());
            return vec![Pressed::Typing(typed)];
        }

        match byte {
            b' ' | b'\r' | b'\n' => vec![Pressed::Acted(Action::Toggle)],
            b'=' => vec![Pressed::Acted(Action::VolumeBy(VOLUME_STEP))],
            printable if printable.is_ascii_graphic() => {
                vec![read_as_typed(&char::from(printable).to_string())]
            }
            _ => Vec::new(),
        }
    }
}

const fn arrowed(key: u8) -> Option<Action> {
    match key {
        UP => Some(Action::VolumeBy(VOLUME_STEP)),
        DOWN => Some(Action::VolumeBy(-VOLUME_STEP)),
        RIGHT => Some(Action::SeekBy(SEEK_STEP)),
        LEFT => Some(Action::SeekBy(-SEEK_STEP)),
        _ => None,
    }
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

    fn keyed(bytes: &[u8]) -> Vec<Pressed> {
        let mut keyed = Keyed::default();
        let mut rest = bytes.iter().copied();
        let mut pressed = Vec::new();
        while let Some(byte) = rest.next() {
            pressed.extend(keyed.pressed(byte, &mut rest));
        }
        pressed
    }

    #[test]
    fn a_key_acts_the_moment_it_is_pressed() {
        assert_eq!(keyed(b" "), vec![Pressed::Acted(Action::Toggle)]);
        assert_eq!(keyed(b"n"), vec![Pressed::Acted(Action::Next)]);
        assert_eq!(
            keyed(b"f+-"),
            vec![
                Pressed::Acted(Action::SeekBy(SEEK_STEP)),
                Pressed::Acted(Action::VolumeBy(VOLUME_STEP)),
                Pressed::Acted(Action::VolumeBy(-VOLUME_STEP)),
            ]
        );
        assert_eq!(keyed(b"w"), vec![Pressed::Unknown("w".to_owned())]);
    }

    #[test]
    fn the_arrows_seek_and_turn_the_volume() {
        assert_eq!(
            keyed(b"\x1b[C\x1b[D\x1b[A\x1b[B"),
            vec![
                Pressed::Acted(Action::SeekBy(SEEK_STEP)),
                Pressed::Acted(Action::SeekBy(-SEEK_STEP)),
                Pressed::Acted(Action::VolumeBy(VOLUME_STEP)),
                Pressed::Acted(Action::VolumeBy(-VOLUME_STEP)),
            ]
        );
    }

    #[test]
    fn a_number_or_a_colon_is_typed_out_and_read_as_the_piped_form_is() {
        assert_eq!(
            keyed(b"90\r"),
            vec![
                Pressed::Typing("9".to_owned()),
                Pressed::Typing("90".to_owned()),
                Pressed::Typing(String::new()),
                Pressed::Acted(Action::SeekTo(90)),
            ]
        );
        assert_eq!(
            keyed(b":z tracj\x7fk\r").last(),
            Some(&Pressed::Acted(Action::Sleep(Sleep::EndOfTrack)))
        );
        assert_eq!(
            keyed(b":f 4\x1bn"),
            vec![
                Pressed::Typing(":".to_owned()),
                Pressed::Typing(":f".to_owned()),
                Pressed::Typing(":f ".to_owned()),
                Pressed::Typing(":f 4".to_owned()),
                Pressed::Typing(String::new()),
                Pressed::Acted(Action::Next),
            ]
        );
    }

    #[test]
    fn a_key_that_means_nothing_is_not_an_action() {
        assert_eq!(parse("w"), None);
        assert_eq!(parse("9w"), None);
        assert_eq!(parse("/"), None);
    }
}
