use std::{io, sync::Arc};

use tracing::{Level, Metadata, subscriber};
use tracing_subscriber::fmt::MakeWriter;

const SHOWN_FROM: Level = Level::WARN;

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

impl LogLevel {
    fn of(level: &Level) -> Self {
        match *level {
            Level::ERROR => Self::Error,
            Level::WARN => Self::Warn,
            Level::INFO => Self::Info,
            _ => Self::Debug,
        }
    }
}

#[uniffi::export(callback_interface)]
pub trait Logbook: Send + Sync {
    fn write(&self, level: LogLevel, line: String);
}

#[derive(Clone)]
struct Lines {
    logbook: Arc<dyn Logbook>,
}

struct Line {
    logbook: Arc<dyn Logbook>,
    level: LogLevel,
    bytes: Vec<u8>,
}

impl Lines {
    fn line_at(&self, level: LogLevel) -> Line {
        Line {
            logbook: self.logbook.clone(),
            level,
            bytes: Vec::new(),
        }
    }
}

impl<'a> MakeWriter<'a> for Lines {
    type Writer = Line;

    fn make_writer(&'a self) -> Line {
        self.line_at(LogLevel::Info)
    }

    fn make_writer_for(&'a self, metadata: &Metadata<'_>) -> Line {
        self.line_at(LogLevel::of(metadata.level()))
    }
}

impl io::Write for Line {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Line {
    fn drop(&mut self) {
        let text = String::from_utf8_lossy(&self.bytes);
        let line = text.trim_end();
        if !line.is_empty() {
            self.logbook.write(self.level, line.to_owned());
        }
    }
}

fn subscriber_over(logbook: Arc<dyn Logbook>) -> impl tracing::Subscriber + Send + Sync {
    tracing_subscriber::fmt()
        .with_writer(Lines { logbook })
        .with_ansi(false)
        .without_time()
        .with_max_level(SHOWN_FROM)
        .finish()
}

#[uniffi::export]
pub fn keep_log(logbook: Box<dyn Logbook>) -> bool {
    subscriber::set_global_default(subscriber_over(Arc::from(logbook))).is_ok()
}

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;

    use super::*;

    #[derive(Default)]
    struct Kept(Mutex<Vec<(LogLevel, String)>>);

    struct Shared(Arc<Kept>);

    impl Logbook for Shared {
        fn write(&self, level: LogLevel, line: String) {
            self.0.0.lock().push((level, line));
        }
    }

    #[test]
    fn a_warning_reaches_the_logbook_with_its_level_and_a_debug_line_does_not() {
        let kept = Arc::new(Kept::default());
        let subscriber = subscriber_over(Arc::new(Shared(kept.clone())));

        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!("nobody asked");
            tracing::warn!(song = 7, "the file would not open");
        });

        let lines = kept.0.lock();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, LogLevel::Warn);
        assert!(lines[0].1.contains("the file would not open"));
        assert!(lines[0].1.contains("song=7"));
    }
}
