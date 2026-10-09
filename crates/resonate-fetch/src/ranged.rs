use std::{
    io::{self, ErrorKind, Read},
    thread,
    time::Duration,
};

use ureq::{
    Agent, Body, BodyReader,
    http::{
        Response,
        header::{CONTENT_RANGE, RANGE},
    },
};

use crate::{failed::as_io, waited::retry_after_of};

pub const ASKED_LATER: [u16; 4] = [429, 502, 503, 504];
pub const RESUMES_AT_MOST: u32 = 5;
const LONGEST_WAIT: Duration = Duration::from_secs(8);
const WHOLE: u16 = 200;
const PARTIAL: u16 = 206;
const FORBIDDEN: u16 = 403;
const NOT_FOUND: u16 = 404;

#[derive(Debug)]
pub enum Unfetched {
    Io(io::Error),
    Refused(u16),
    AskedLater {
        status: u16,
        asked_for: Option<Duration>,
    },
}

impl Unfetched {
    pub fn into_io(self) -> io::Error {
        match self {
            Self::Io(error) => error,
            Self::Refused(FORBIDDEN) => io::Error::from(ErrorKind::PermissionDenied),
            Self::Refused(NOT_FOUND) => io::Error::from(ErrorKind::NotFound),
            Self::Refused(_) | Self::AskedLater { .. } => {
                io::Error::from(ErrorKind::ConnectionAborted)
            }
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Io(_) => None,
            Self::Refused(status) | Self::AskedLater { status, .. } => Some(*status),
        }
    }
}

pub fn waited_before(resume: u32, resumed_after: Duration) -> Duration {
    match resume {
        0 => Duration::ZERO,
        later => resumed_after
            .saturating_mul(1 << (later - 1).min(16))
            .min(LONGEST_WAIT),
    }
}

fn starts_at(range: &str) -> Option<u64> {
    range
        .trim()
        .strip_prefix("bytes")?
        .trim_start()
        .split('-')
        .next()?
        .trim()
        .parse()
        .ok()
}

fn asked(agent: &Agent, url: &str, from: u64) -> Result<BodyReader<'static>, Unfetched> {
    let mut request = agent.get(url);
    if from > 0 {
        request = request.header(RANGE, format!("bytes={from}-"));
    }
    let response = request
        .call()
        .map_err(|error| Unfetched::Io(as_io(error)))?;
    let status = response.status().as_u16();
    let resumed_at = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|range| range.to_str().ok())
        .and_then(starts_at);
    let skipped = match status {
        PARTIAL if from > 0 && resumed_at == Some(from) => 0,
        WHOLE => from,
        status if ASKED_LATER.contains(&status) => {
            return Err(Unfetched::AskedLater {
                status,
                asked_for: retry_after_of(&response),
            });
        }
        status => return Err(Unfetched::Refused(status)),
    };

    let mut reader = response.into_body().into_reader();
    if skipped > 0 {
        let passed =
            io::copy(&mut (&mut reader).take(skipped), &mut io::sink()).map_err(Unfetched::Io)?;
        if passed < skipped {
            return Err(Unfetched::Io(io::Error::from(ErrorKind::UnexpectedEof)));
        }
    }
    Ok(reader)
}

pub struct Ranged {
    agent: Agent,
    url: String,
    reader: BodyReader<'static>,
    read: u64,
    broken: u32,
    resumed_after: Duration,
}

impl Ranged {
    pub fn continuing(
        agent: Agent,
        url: String,
        response: Response<Body>,
        resumed_after: Duration,
    ) -> Self {
        Self {
            agent,
            url,
            reader: response.into_body().into_reader(),
            read: 0,
            broken: 0,
            resumed_after,
        }
    }

    pub fn opened(agent: Agent, url: String, resumed_after: Duration) -> Result<Self, Unfetched> {
        let mut tried = 0;
        let reader = loop {
            match asked(&agent, &url, 0) {
                Ok(reader) => break reader,
                Err(Unfetched::AskedLater { asked_for, .. }) if tried < RESUMES_AT_MOST => {
                    tried += 1;
                    let wait = asked_for
                        .unwrap_or_else(|| waited_before(tried, resumed_after))
                        .min(LONGEST_WAIT);
                    tracing::debug!(
                        ?wait,
                        "a download was asked for again later, as its host asked"
                    );
                    thread::sleep(wait);
                }
                Err(unfetched) => return Err(unfetched),
            }
        };
        Ok(Self {
            agent,
            url,
            reader,
            read: 0,
            broken: 0,
            resumed_after,
        })
    }

    pub fn read_so_far(&self) -> u64 {
        self.read
    }

    fn resumed_after(&mut self, broken_off: io::Error) -> io::Result<()> {
        let mut last = broken_off;
        let mut asked_for = None;
        while self.broken < RESUMES_AT_MOST {
            let wait = asked_for
                .take()
                .unwrap_or_else(|| waited_before(self.broken, self.resumed_after))
                .min(LONGEST_WAIT);
            self.broken += 1;
            tracing::debug!(error = %last, read = self.read, ?wait, "a download broke off and is asked for again from where it stopped");
            thread::sleep(wait);
            match asked(&self.agent, &self.url, self.read) {
                Ok(reader) => {
                    self.reader = reader;
                    return Ok(());
                }
                Err(refused @ Unfetched::Refused(_)) => return Err(refused.into_io()),
                Err(Unfetched::AskedLater {
                    status,
                    asked_for: named,
                }) => {
                    asked_for = named;
                    last = Unfetched::AskedLater {
                        status,
                        asked_for: named,
                    }
                    .into_io();
                }
                Err(unfetched @ Unfetched::Io(_)) => last = unfetched.into_io(),
            }
        }
        Err(last)
    }
}

impl Read for Ranged {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.reader.read(buf) {
                Ok(read) => {
                    if read > 0 {
                        self.read += read as u64;
                        self.broken = 0;
                    }
                    return Ok(read);
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => self.resumed_after(error)?,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resumed_range_says_where_it_starts() {
        assert_eq!(starts_at("bytes 100-999/1000"), Some(100));
        assert_eq!(starts_at("bytes 0-0/*"), Some(0));
        assert_eq!(starts_at("bytes */1000"), None);
        assert_eq!(starts_at("items 1-2/3"), None);
    }

    #[test]
    fn the_first_resume_is_at_once_and_each_later_one_waits_twice_as_long_up_to_a_ceiling() {
        let first = Duration::from_secs(1);
        let waits: Vec<_> = (0..RESUMES_AT_MOST + 2)
            .map(|resume| waited_before(resume, first).as_secs())
            .collect();

        assert_eq!(waits, [0, 1, 2, 4, 8, 8, 8]);
        assert_eq!(waited_before(u32::MAX, first), LONGEST_WAIT);
    }
}
