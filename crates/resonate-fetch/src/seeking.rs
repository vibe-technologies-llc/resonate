use std::{
    io::{self, ErrorKind, Read, Seek, SeekFrom},
    time::Duration,
};

use ureq::{Agent, Body, http::Response};

use crate::ranged::{Known, Ranged, Unfetched};

pub const SKIPPED_RATHER_THAN_ASKED: u64 = 256 * 1024;

pub struct Seeking {
    agent: Agent,
    url: String,
    length: Option<u64>,
    at: u64,
    reading: Option<Ranged>,
    known: Known,
    bearer: Option<String>,
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
            known: Known {
                validator: reading.validator().cloned(),
                total: reading.total(),
            },
            reading: Some(reading),
            bearer: None,
            resumed_after,
        }
    }

    pub fn bearing(self, bearer: String) -> Self {
        Self {
            reading: self.reading.map(|reading| reading.bearing(bearer.clone())),
            bearer: Some(bearer),
            ..self
        }
    }

    pub const fn length(&self) -> Option<u64> {
        self.length
    }

    fn reading(&mut self) -> io::Result<&mut Ranged> {
        let reading = match self.reading.take() {
            Some(reading) => reading,
            None => Ranged::opened_bearing(
                self.agent.clone(),
                self.url.clone(),
                self.at,
                self.bearer.clone(),
                &self.known,
                self.resumed_after,
            )
            .map_err(Unfetched::into_io)?,
        };
        self.known.validator = self
            .known
            .validator
            .take()
            .or_else(|| reading.validator().cloned());
        self.known.total = self.known.total.or(reading.total());
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

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use super::*;
    use crate::{
        Trusted, configured,
        testing::{WHOLE_BODY, answering, lower, partial_from, serving},
    };

    const READ_FIRST: usize = 30;
    const SOUGHT_BACK_TO: u64 = 10;

    fn seeking_over(url: &str) -> Seeking {
        let agent = configured(Duration::from_secs(5), Trusted::BuiltInRoots)
            .build()
            .new_agent();
        let response = agent.get(url).call().expect("the first answer");
        Seeking::continuing(agent, url.to_owned(), response, Duration::ZERO)
    }

    fn read_then_seek_back(seeking: &mut Seeking) {
        seeking
            .read_exact(&mut [0; READ_FIRST])
            .expect("the first part");
        seeking
            .seek(SeekFrom::Start(SOUGHT_BACK_TO))
            .expect("the seek moves the place");
    }

    fn whole_with(headers: &[&str]) -> Vec<u8> {
        answering("200 OK", headers, WHOLE_BODY, WHOLE_BODY.len())
    }

    #[test]
    fn a_seek_asks_the_range_with_the_validator_of_the_first_answer_and_reads_on() {
        let (url, heads) = serving(vec![
            Box::new(|_| whole_with(&["ETag: \"v1\""])),
            Box::new(|_| {
                partial_from(SOUGHT_BACK_TO as usize, WHOLE_BODY.len(), &["ETag: \"v1\""])
            }),
        ]);
        let mut seeking = seeking_over(&url);
        read_then_seek_back(&mut seeking);
        let mut read = Vec::new();

        seeking.read_to_end(&mut read).expect("the rest is read");

        assert_eq!(read, &WHOLE_BODY[SOUGHT_BACK_TO as usize..]);
        let asked = lower(&heads.lock()[1]);
        assert!(
            asked.contains(&format!("range: bytes={SOUGHT_BACK_TO}-")),
            "{asked}"
        );
        assert!(asked.contains("if-range: \"v1\""), "{asked}");
    }

    #[test]
    fn a_file_replaced_between_two_seeks_is_not_joined_old_to_new() {
        let (url, _) = serving(vec![
            Box::new(|_| whole_with(&["ETag: \"v1\""])),
            Box::new(|_| whole_with(&["ETag: \"v2\""])),
        ]);
        let mut seeking = seeking_over(&url);
        read_then_seek_back(&mut seeking);

        let error = seeking
            .read_to_end(&mut Vec::new())
            .expect_err("the change is refused");

        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn a_range_naming_another_total_than_the_first_answer_is_refused() {
        let (url, _) = serving(vec![
            Box::new(|_| whole_with(&[])),
            Box::new(|_| partial_from(SOUGHT_BACK_TO as usize, WHOLE_BODY.len() + 50, &[])),
        ]);
        let mut seeking = seeking_over(&url);
        read_then_seek_back(&mut seeking);

        let error = seeking
            .read_to_end(&mut Vec::new())
            .expect_err("the change is refused");

        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }
}
