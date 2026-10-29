use std::{
    io::{self, ErrorKind, Read},
    thread,
    time::Duration,
};

use resonate_providers::{Held, Resumed, Validator};
use ureq::{
    Agent, Body, BodyReader, RequestBuilder,
    http::{
        Response,
        header::{AUTHORIZATION, CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE},
    },
    typestate::WithoutBody,
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

fn total_of(range: &str) -> Option<u64> {
    range.rsplit('/').next()?.trim().parse().ok()
}

pub fn validator_of(response: &Response<Body>) -> Option<Validator> {
    [ETAG, LAST_MODIFIED].iter().find_map(|name| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(Validator::new)
    })
}

pub fn asking_to_resume(
    request: RequestBuilder<WithoutBody>,
    held: Option<&Held>,
) -> RequestBuilder<WithoutBody> {
    let Some(held) = held.filter(|held| held.can_be_checked()) else {
        return request;
    };
    let request = request.header(RANGE, format!("bytes={}-", held.bytes));
    match held
        .validator
        .as_ref()
        .filter(|validator| !validator.is_weak())
    {
        Some(validator) => request.header(IF_RANGE, validator.as_str()),
        None => request,
    }
}

fn asked(agent: &Agent, url: &str, from: u64, bearer: Option<&str>) -> Result<Answered, Unfetched> {
    let mut request = agent.get(url);
    if let Some(bearer) = bearer {
        request = request.header(AUTHORIZATION, bearer);
    }
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

    let length = response.body().content_length();
    let mut reader = response.into_body().into_reader();
    if skipped > 0 {
        let passed =
            io::copy(&mut (&mut reader).take(skipped), &mut io::sink()).map_err(Unfetched::Io)?;
        if passed < skipped {
            return Err(Unfetched::Io(io::Error::from(ErrorKind::UnexpectedEof)));
        }
    }
    Ok(Answered { reader, length })
}

struct Answered {
    reader: BodyReader<'static>,
    length: Option<u64>,
}

pub struct Ranged {
    agent: Agent,
    url: String,
    bearer: Option<String>,
    reader: BodyReader<'static>,
    length: Option<u64>,
    read: u64,
    from: u64,
    validator: Option<Validator>,
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
            bearer: None,
            length: response.body().content_length(),
            validator: validator_of(&response),
            reader: response.into_body().into_reader(),
            read: 0,
            from: 0,
            broken: 0,
            resumed_after,
        }
    }

    pub fn continuing_held(
        agent: Agent,
        url: String,
        response: Response<Body>,
        held: &Held,
        resumed_after: Duration,
    ) -> Option<Self> {
        match response.status().as_u16() {
            WHOLE => Some(Self::continuing(agent, url, response, resumed_after)),
            PARTIAL => {
                let range = response
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|range| range.to_str().ok())?;
                let total = total_of(range);
                let unchanged = starts_at(range) == Some(held.bytes)
                    && (held.total.is_none() || total == held.total);
                unchanged.then(|| Self {
                    agent,
                    url,
                    bearer: None,
                    length: total.or(held.total),
                    validator: validator_of(&response).or_else(|| held.validator.clone()),
                    reader: response.into_body().into_reader(),
                    read: held.bytes,
                    from: held.bytes,
                    broken: 0,
                    resumed_after,
                })
            }
            _ => None,
        }
    }

    pub fn opened(agent: Agent, url: String, resumed_after: Duration) -> Result<Self, Unfetched> {
        Self::opened_from(agent, url, 0, resumed_after)
    }

    pub fn opened_from(
        agent: Agent,
        url: String,
        from: u64,
        resumed_after: Duration,
    ) -> Result<Self, Unfetched> {
        Self::opened_bearing(agent, url, from, None, resumed_after)
    }

    pub fn opened_bearing(
        agent: Agent,
        url: String,
        from: u64,
        bearer: Option<String>,
        resumed_after: Duration,
    ) -> Result<Self, Unfetched> {
        let mut tried = 0;
        let answered = loop {
            match asked(&agent, &url, from, bearer.as_deref()) {
                Ok(answered) => break answered,
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
            bearer,
            reader: answered.reader,
            length: answered.length,
            validator: None,
            read: from,
            from,
            broken: 0,
            resumed_after,
        })
    }

    pub fn bearing(self, bearer: String) -> Self {
        Self {
            bearer: Some(bearer),
            ..self
        }
    }

    pub fn from(&self) -> u64 {
        self.from
    }

    pub fn validator(&self) -> Option<&Validator> {
        self.validator.as_ref()
    }

    pub fn into_resumed(self) -> Resumed {
        Resumed {
            from: self.from,
            length: self.length,
            validator: self.validator.clone(),
            reader: Box::new(self),
        }
    }

    pub fn read_so_far(&self) -> u64 {
        self.read
    }

    pub fn length(&self) -> Option<u64> {
        self.length
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
            match asked(&self.agent, &self.url, self.read, self.bearer.as_deref()) {
                Ok(answered) => {
                    self.reader = answered.reader;
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
    fn a_range_says_how_long_the_whole_is() {
        assert_eq!(total_of("bytes 100-999/1000"), Some(1000));
        assert_eq!(total_of("bytes 0-0/*"), None);
        assert_eq!(total_of("bytes */1000"), Some(1000));
    }

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
