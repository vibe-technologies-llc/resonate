use std::{
    fmt::{self, Write as _},
    io::{self, Write as _},
    sync::atomic::{AtomicBool, Ordering},
};

use crate::table::TURNS_THE_READING;

const LINES_AND_COLUMNS: [char; 2] = ['\n', '\t'];

static NOBODY_IS_READING: AtomicBool = AtomicBool::new(false);

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

pub fn raw(text: &str) {
    if NOBODY_IS_READING.load(Ordering::Relaxed) {
        return;
    }
    let wrote = io::stdout().lock().write_all(text.as_bytes());
    if let Err(error) = wrote {
        if error.kind() == io::ErrorKind::BrokenPipe {
            NOBODY_IS_READING.store(true, Ordering::Relaxed);
        } else {
            tracing::debug!(%error, "a line could not be written to the standard output");
        }
    }
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
    if let Err(error) = io::stdout().flush()
        && error.kind() == io::ErrorKind::BrokenPipe
    {
        NOBODY_IS_READING.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_cannot_steer_the_terminal_it_is_printed_on() {
        let printed = plain("Echoes\x1b]0;owned\x07 by Pink\u{202e}Floyd\r\nnext\tcolumn\u{9b}");

        assert_eq!(printed, "Echoes ]0;owned  by Pink Floyd \nnext\tcolumn ");
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
