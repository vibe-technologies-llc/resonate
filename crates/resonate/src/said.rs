use std::{
    fmt::{self, Write as _},
    io::{self, Write as _},
    sync::atomic::{AtomicBool, Ordering},
};

use parking_lot::Mutex;

use crate::table::TURNS_THE_READING;

const LINES_AND_COLUMNS: [char; 2] = ['\n', '\t'];

static NOBODY_IS_READING: AtomicBool = AtomicBool::new(false);

static FAILED_TO_WRITE: Mutex<Option<io::Error>> = Mutex::new(None);

macro_rules! said {
    () => {
        $crate::said::written(format_args!(""), "\n")
    };
    ($($said:tt)*) => {
        $crate::said::written(format_args!($($said)*), "\n")
    };
}

macro_rules! said_on {
    ($($said:tt)*) => {
        $crate::said::written(format_args!($($said)*), "")
    };
}

macro_rules! told {
    ($($told:tt)*) => {
        $crate::said::complained(format_args!($($told)*))
    };
}

pub fn complained(told: fmt::Arguments<'_>) {
    let mut text = String::new();

    if text.write_fmt(told).is_err() {
        return;
    }
    text.push('\n');

    let _ = io::stderr().lock().write_all(plain(&text).as_bytes());
}

pub fn written(said: fmt::Arguments<'_>, ending: &str) {
    if NOBODY_IS_READING.load(Ordering::Relaxed) {
        return;
    }

    let mut text = String::new();

    if text.write_fmt(said).is_err() {
        return;
    }
    text.push_str(ending);
    raw(&plain(&text));
}

#[cfg(unix)]
pub fn the_terminal_hung_up() {
    use std::io::IsTerminal as _;

    if io::stdout().is_terminal() {
        NOBODY_IS_READING.store(true, Ordering::Relaxed);
    }
}

pub fn raw(text: &str) {
    if NOBODY_IS_READING.load(Ordering::Relaxed) || FAILED_TO_WRITE.lock().is_some() {
        return;
    }

    let wrote = io::stdout().lock().write_all(text.as_bytes());
    if let Err(error) = wrote {
        refused(error);
    }
}

fn refused(error: io::Error) {
    if error.kind() == io::ErrorKind::BrokenPipe {
        NOBODY_IS_READING.store(true, Ordering::Relaxed);
        return;
    }

    tracing::debug!(%error, "a line could not be written to the standard output");

    let mut failed = FAILED_TO_WRITE.lock();
    if failed.is_none() {
        *failed = Some(error);
    }
}

pub fn the_write_that_failed() -> Option<io::Error> {
    flushed();
    FAILED_TO_WRITE.lock().take()
}

fn plain(text: &str) -> String {
    text.chars()
        .map(|character| {
            let steers = character.is_control() && !LINES_AND_COLUMNS.contains(&character);
            if steers || TURNS_THE_READING.contains(&character) {
                ' '
            } else {
                character
            }
        })
        .collect()
}

pub fn flushed() {
    if NOBODY_IS_READING.load(Ordering::Relaxed) {
        return;
    }
    if let Err(error) = io::stdout().flush() {
        refused(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_cannot_steer_the_terminal_it_is_printed_on() {
        let printed =
            plain("Slow Water\x1b]0;owned\x07 by Example\u{202e}Band\r\nnext\tcolumn\u{9b}");

        assert_eq!(
            printed,
            "Slow Water ]0;owned  by Example Band \nnext\tcolumn "
        );
    }

    #[test]
    fn a_reader_gone_is_noted_once_and_nothing_more_is_written() {
        NOBODY_IS_READING.store(true, Ordering::Relaxed);

        said!("a line nobody reads {}", 1);
        said_on!("nor this");
        flushed();

        assert!(NOBODY_IS_READING.load(Ordering::Relaxed));

        NOBODY_IS_READING.store(false, Ordering::Relaxed);
    }
}
