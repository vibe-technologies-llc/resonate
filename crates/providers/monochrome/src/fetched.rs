use std::io::{self, ErrorKind, Read};

use ureq::{
    Agent, Body, BodyReader,
    http::{
        self,
        header::{CONTENT_RANGE, RANGE},
    },
};

const RESUMES_AT_MOST: u32 = 3;
const WHOLE: u16 = 200;
const PARTIAL: u16 = 206;

pub(crate) fn as_io(error: ureq::Error) -> io::Error {
    match error {
        ureq::Error::Io(source) => source,
        ureq::Error::Timeout(_) => io::Error::from(ErrorKind::TimedOut),
        ureq::Error::HostNotFound => io::Error::from(ErrorKind::NotFound),
        _ => io::Error::from(ErrorKind::ConnectionRefused),
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

pub(crate) struct Fetched {
    agent: Agent,
    url: String,
    reader: BodyReader<'static>,
    read: u64,
    broken: u32,
}

impl Fetched {
    pub(crate) fn continuing(agent: Agent, url: String, response: http::Response<Body>) -> Self {
        Self {
            agent,
            url,
            reader: response.into_body().into_reader(),
            read: 0,
            broken: 0,
        }
    }

    fn reopened(&self) -> io::Result<BodyReader<'static>> {
        let response = self
            .agent
            .get(&self.url)
            .header(RANGE, format!("bytes={}-", self.read))
            .call()
            .map_err(as_io)?;
        let status = response.status().as_u16();
        let resumed_at = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|range| range.to_str().ok())
            .and_then(starts_at);
        let skipped = match status {
            PARTIAL if resumed_at == Some(self.read) => 0,
            WHOLE => self.read,
            _ => return Err(io::Error::from(ErrorKind::ConnectionAborted)),
        };

        let mut reader = response.into_body().into_reader();
        if skipped > 0 {
            let passed = io::copy(&mut (&mut reader).take(skipped), &mut io::sink())?;
            if passed < skipped {
                return Err(io::Error::from(ErrorKind::UnexpectedEof));
            }
        }
        Ok(reader)
    }

    fn resumed_after(&mut self, broken_off: io::Error) -> io::Result<()> {
        let mut last = broken_off;
        while self.broken < RESUMES_AT_MOST {
            self.broken += 1;
            tracing::debug!(error = %last, read = self.read, "a Monochrome download broke off and is asked for again from where it stopped");
            match self.reopened() {
                Ok(reader) => {
                    self.reader = reader;
                    return Ok(());
                }
                Err(error) => last = error,
            }
        }
        Err(last)
    }
}

impl Read for Fetched {
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
}
