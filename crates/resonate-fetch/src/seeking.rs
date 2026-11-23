use std::{
    io::{self, ErrorKind, Read, Seek, SeekFrom},
    mem,
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
                validators: reading.validators().clone(),
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

        self.known.validators =
            mem::take(&mut self.known.validators).joined(reading.validators().clone());
        self.known.total = self.known.total.or(reading.total());
        Ok(self.reading.insert(reading))
    }

    fn passed_over(&mut self, bytes: u64) -> io::Result<bool> {
        let Some(reading) = self.reading.as_mut() else {
            return Ok(false);
        };
        match io::copy(&mut reading.take(bytes), &mut io::sink()) {
            Ok(passed) => {
                self.at += passed;
                Ok(passed == bytes)
            }
            Err(error) => {
                self.reading = None;
                Err(error)
            }
        }
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
        Trusted, configured, downloading_agent,
        testing::{
            Answer, WHOLE_BODY, answering, coded_as_asked, cut_first_answer, lower, partial_from,
            serving,
        },
    };

    const READ_FIRST: usize = 30;
    const SOUGHT_BACK_TO: u64 = 10;
    const SOUGHT_PAST_THE_BREAK: u64 = 40;

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
    fn a_stream_a_downloading_agent_opens_is_asked_uncoded_from_its_first_request_and_seeks_within_its_length()
     {
        let (url, heads) = serving(vec![
            Box::new(|head| coded_as_asked(head, None)),
            Box::new(|head| coded_as_asked(head, None)),
        ]);
        let agent = downloading_agent(
            configured(Duration::from_secs(5), Trusted::BuiltInRoots),
            Duration::from_secs(5),
        );

        let response = agent.get(&url).call().expect("the first answer");
        let mut seeking = Seeking::continuing(agent, url, response, Duration::ZERO);

        read_then_seek_back(&mut seeking);

        let mut read = Vec::new();

        seeking.read_to_end(&mut read).expect("the rest is read");

        assert_eq!(seeking.length(), Some(WHOLE_BODY.len() as u64));
        assert_eq!(read, &WHOLE_BODY[SOUGHT_BACK_TO as usize..]);

        let heads = heads.lock();

        assert_eq!(heads.len(), 2);
        for head in heads.iter() {
            assert!(lower(head).contains("accept-encoding: identity"), "{head}");
        }
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

    fn rewound_after(first: &'static [&'static str], again: Answer) -> io::Result<Vec<u8>> {
        let (url, heads) = serving(vec![Box::new(move |_| whole_with(first)), again]);
        let mut seeking = seeking_over(&url);

        seeking
            .read_exact(&mut [0; READ_FIRST])
            .expect("the first part");
        seeking
            .seek(SeekFrom::Start(0))
            .expect("the rewind moves the place");

        let mut read = Vec::new();

        let ended = seeking.read_to_end(&mut read).map(|_| read);

        assert!(!lower(&heads.lock()[1]).contains("range:"));
        ended
    }

    #[test]
    fn a_rewind_after_the_file_was_replaced_reports_a_changed_stream() {
        let replaced = rewound_after(
            &["ETag: \"v1\""],
            Box::new(|_| whole_with(&["ETag: \"v2\""])),
        );
        let regrown = rewound_after(
            &[],
            Box::new(|_| answering("200 OK", &[], WHOLE_BODY, WHOLE_BODY.len() - 1)),
        );
        let untagged = rewound_after(&["ETag: \"v1\""], Box::new(|_| whole_with(&[])));

        assert_eq!(
            replaced.map_err(|error| error.kind()),
            Err(ErrorKind::InvalidData)
        );
        assert_eq!(
            regrown.map_err(|error| error.kind()),
            Err(ErrorKind::InvalidData)
        );
        assert_eq!(
            untagged.map_err(|error| error.kind()),
            Err(ErrorKind::InvalidData)
        );
    }

    #[test]
    fn an_unchanged_file_rewound_plays_from_the_start() {
        let tagged = rewound_after(
            &["ETag: \"v1\""],
            Box::new(|_| whole_with(&["ETag: \"v1\""])),
        );
        let untagged = rewound_after(&[], Box::new(|_| whole_with(&[])));

        assert_eq!(tagged.expect("the whole is read again"), WHOLE_BODY);
        assert_eq!(untagged.expect("the whole is read again"), WHOLE_BODY);
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

    #[test]
    fn a_seek_whose_skipping_copy_fails_leaves_the_place_where_it_was_and_reads_on_from_there() {
        let (url, heads) = serving(vec![
            Box::new(|_| cut_first_answer(&["ETag: \"v1\""])),
            Box::new(|_| answering("403 Forbidden", &[], b"", 0)),
            Box::new(|_| {
                partial_from(SOUGHT_BACK_TO as usize, WHOLE_BODY.len(), &["ETag: \"v1\""])
            }),
        ]);
        let mut seeking = seeking_over(&url);

        seeking
            .read_exact(&mut [0; SOUGHT_BACK_TO as usize])
            .expect("the first part");

        let failed = seeking
            .seek(SeekFrom::Start(SOUGHT_PAST_THE_BREAK))
            .expect_err("the skip is refused");
        let stayed_at = seeking.stream_position().expect("the place");
        let mut read = Vec::new();

        seeking.read_to_end(&mut read).expect("the rest is read");

        assert_eq!(failed.kind(), ErrorKind::PermissionDenied);
        assert_eq!(stayed_at, SOUGHT_BACK_TO);
        assert_eq!(read, &WHOLE_BODY[SOUGHT_BACK_TO as usize..]);

        let asked = lower(&heads.lock()[2]);

        assert!(
            asked.contains(&format!("range: bytes={SOUGHT_BACK_TO}-")),
            "{asked}"
        );
    }
}
