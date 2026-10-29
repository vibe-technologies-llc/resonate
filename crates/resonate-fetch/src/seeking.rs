use std::{
    io::{self, ErrorKind, Read, Seek, SeekFrom},
    time::Duration,
};

use ureq::{Agent, Body, http::Response};

use crate::ranged::{Ranged, Unfetched};

pub const SKIPPED_RATHER_THAN_ASKED: u64 = 256 * 1024;

pub struct Seeking {
    agent: Agent,
    url: String,
    length: Option<u64>,
    at: u64,
    reading: Option<Ranged>,
    resumed_after: Duration,
}

impl Seeking {
    pub fn continuing(
        agent: Agent,
        url: String,
        response: Response<Body>,
        resumed_after: Duration,
    ) -> Self {
        let reading = Ranged::continuing(agent.clone(), url.clone(), response, resumed_after);
        Self {
            agent,
            url,
            length: reading.length(),
            at: 0,
            reading: Some(reading),
            resumed_after,
        }
    }

    pub const fn length(&self) -> Option<u64> {
        self.length
    }

    fn reading(&mut self) -> io::Result<&mut Ranged> {
        let reading = match self.reading.take() {
            Some(reading) => reading,
            None => Ranged::opened_from(
                self.agent.clone(),
                self.url.clone(),
                self.at,
                self.resumed_after,
            )
            .map_err(Unfetched::into_io)?,
        };
        Ok(self.reading.insert(reading))
    }

    fn passed_over(&mut self, bytes: u64) -> io::Result<bool> {
        let Some(reading) = self.reading.as_mut() else {
            return Ok(false);
        };
        let passed = io::copy(&mut reading.take(bytes), &mut io::sink())?;
        self.at += passed;
        Ok(passed == bytes)
    }
}

impl Read for Seeking {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.length.is_some_and(|length| self.at >= length) {
            return Ok(0);
        }
        let read = self.reading()?.read(buf)?;
        self.at += read as u64;
        Ok(read)
    }
}

impl Seek for Seeking {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(target) => Some(target),
            SeekFrom::Current(by) => self.at.checked_add_signed(by),
            SeekFrom::End(by) => self
                .length
                .ok_or_else(|| io::Error::from(ErrorKind::Unsupported))?
                .checked_add_signed(by),
        }
        .ok_or_else(|| io::Error::from(ErrorKind::InvalidInput))?;
        if target == self.at {
            return Ok(target);
        }
        let ahead = target.checked_sub(self.at);
        let within_reach = ahead.is_some_and(|ahead| ahead <= SKIPPED_RATHER_THAN_ASKED);
        let still_reading = self.length.is_none_or(|length| target <= length);
        if within_reach && still_reading && self.passed_over(ahead.unwrap_or_default())? {
            return Ok(target);
        }
        self.reading = None;
        self.at = target;
        Ok(target)
    }
}
