use std::{
    io::{self, ErrorKind, Read},
    vec,
};

use ureq::{
    Agent, BodyReader,
    http::header::{CONTENT_RANGE, RANGE},
};

const RESUMES_AT_MOST: u32 = 3;
const WHOLE: u16 = 200;
const PARTIAL: u16 = 206;
const FORBIDDEN: u16 = 403;
const NOT_FOUND: u16 = 404;

#[derive(Debug)]
pub(crate) enum Unfetched {
    Io(io::Error),
    Refused(u16),
}

impl Unfetched {
    fn into_io(self) -> io::Error {
        match self {
            Self::Io(error) => error,
            Self::Refused(FORBIDDEN) => io::Error::from(ErrorKind::PermissionDenied),
            Self::Refused(NOT_FOUND) => io::Error::from(ErrorKind::NotFound),
            Self::Refused(_) => io::Error::from(ErrorKind::ConnectionAborted),
        }
    }
}

pub(crate) fn as_io(error: ureq::Error) -> io::Error {
    match error {
        ureq::Error::Io(source) => source,
        ureq::Error::Timeout(_) => io::Error::from(ErrorKind::TimedOut),
        ureq::Error::HostNotFound => io::Error::from(ErrorKind::NotFound),
        _ => io::Error::from(ErrorKind::ConnectionRefused),
    }
}

struct Piece {
    url: String,
    reader: BodyReader<'static>,
    read: u64,
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

impl Piece {
    fn opened(agent: &Agent, url: String, from: u64) -> Result<Self, Unfetched> {
        let mut request = agent.get(&url);
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
            _ => return Err(Unfetched::Refused(status)),
        };

        let mut reader = response.into_body().into_reader();
        if skipped > 0 {
            let passed = io::copy(&mut (&mut reader).take(skipped), &mut io::sink())
                .map_err(Unfetched::Io)?;
            if passed < skipped {
                return Err(Unfetched::Io(io::Error::from(ErrorKind::UnexpectedEof)));
            }
        }
        Ok(Self {
            url,
            reader,
            read: from,
        })
    }

    fn resumed(agent: &Agent, url: String, from: u64) -> io::Result<Self> {
        let mut last = io::Error::from(ErrorKind::ConnectionAborted);
        for _ in 0..RESUMES_AT_MOST {
            match Self::opened(agent, url.clone(), from) {
                Ok(piece) => return Ok(piece),
                Err(Unfetched::Refused(status)) => return Err(Unfetched::Refused(status).into_io()),
                Err(Unfetched::Io(error)) => last = error,
            }
        }
        Err(last)
    }
}

pub(crate) struct Fetched {
    agent: Agent,
    left: vec::IntoIter<String>,
    piece: Option<Piece>,
    broken: u32,
}

impl Fetched {
    pub(crate) fn opened(agent: Agent, urls: Vec<String>) -> Result<Self, Unfetched> {
        let mut left = urls.into_iter();
        let piece = left
            .next()
            .map(|url| Piece::opened(&agent, url, 0))
            .transpose()?;
        Ok(Self {
            agent,
            left,
            piece,
            broken: 0,
        })
    }
}

impl Read for Fetched {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let Some(piece) = self.piece.as_mut() else {
                let Some(url) = self.left.next() else {
                    return Ok(0);
                };
                self.piece = Some(Piece::opened(&self.agent, url, 0).map_err(Unfetched::into_io)?);
                continue;
            };
            match piece.reader.read(buf) {
                Ok(0) => {
                    self.piece = None;
                    self.broken = 0;
                }
                Ok(read) => {
                    piece.read += read as u64;
                    self.broken = 0;
                    return Ok(read);
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) if self.broken < RESUMES_AT_MOST => {
                    self.broken += 1;
                    tracing::debug!(%error, read = piece.read, "a Tidal segment broke off and is asked for again from where it stopped");
                    let url = std::mem::take(&mut piece.url);
                    let from = piece.read;
                    self.piece = Some(Piece::resumed(&self.agent, url, from)?);
                }
                Err(error) => return Err(error),
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
}
