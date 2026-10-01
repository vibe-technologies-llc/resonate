use std::{
    fmt,
    io::{self, Write as _},
    sync::atomic::{AtomicBool, Ordering},
};

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

pub fn written(said: fmt::Arguments<'_>, ending: &str) {
    if NOBODY_IS_READING.load(Ordering::Relaxed) {
        return;
    }
    let mut out = io::stdout().lock();
    let wrote = out
        .write_fmt(said)
        .and_then(|()| out.write_all(ending.as_bytes()));
    if let Err(error) = wrote {
        if error.kind() == io::ErrorKind::BrokenPipe {
            NOBODY_IS_READING.store(true, Ordering::Relaxed);
        } else {
            tracing::debug!(%error, "a line could not be written to the standard output");
        }
    }
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
    fn a_reader_gone_is_noted_once_and_nothing_more_is_written() {
        NOBODY_IS_READING.store(true, Ordering::Relaxed);

        said!("a line nobody reads {}", 1);
        said_on!("nor this");
        flushed();

        assert!(NOBODY_IS_READING.load(Ordering::Relaxed));
        NOBODY_IS_READING.store(false, Ordering::Relaxed);
    }
}
