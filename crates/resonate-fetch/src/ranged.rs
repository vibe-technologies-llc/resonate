use std::{
    io::{self, ErrorKind, Read, Take},
    mem, thread,
    time::Duration,
};

use resonate_providers::{EntityTag, Held, Modified, Resumed, Validators};
use ureq::{
    Agent, Body, BodyReader, RequestBuilder,
    http::{
        Response,
        header::{
            ACCEPT_ENCODING, AUTHORIZATION, CONTENT_RANGE, ETAG, HeaderName, IF_RANGE,
            LAST_MODIFIED, RANGE,
        },
    },
    typestate::WithoutBody,
};

use crate::{failed::as_io, waited::retry_after_of};

pub const ASKED_LATER: [u16; 4] = [429, 502, 503, 504];
pub const RESUMES_AT_MOST: u32 = 5;
const LONGEST_WAIT: Duration = Duration::from_secs(8);
const UNCODED: &str = "identity";
const WHOLE: u16 = 200;
const PARTIAL: u16 = 206;
const FORBIDDEN: u16 = 403;
const NOT_FOUND: u16 = 404;
const UNBOUNDED: u64 = u64::MAX;
const BYTES_UNIT: &str = "bytes";
const LENGTH_UNKNOWN: &str = "*";

type Bounded = Take<BodyReader<'static>>;

#[derive(Debug)]
pub enum Unfetched {
    Io(io::Error),
    Refused(u16),
    Changed,
    Misranged,
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
            Self::Changed | Self::Misranged => io::Error::from(ErrorKind::InvalidData),
            Self::Refused(_) | Self::AskedLater { .. } => {
                io::Error::from(ErrorKind::ConnectionAborted)
            }
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Io(_) | Self::Changed | Self::Misranged => None,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Interval {
    first: u64,
    end: u64,
    complete: Option<u64>,
}

impl Interval {
    fn read(range: &str) -> Option<Self> {
        let rest = range.trim().strip_prefix(BYTES_UNIT)?;
        let (span, complete) = rest.strip_prefix(' ')?.trim_start().split_once('/')?;
        let (first, last) = span.trim().split_once('-')?;
        let first: u64 = digits(first)?;
        let last: u64 = digits(last)?;
        let complete = match complete.trim() {
            LENGTH_UNKNOWN => None,
            complete => Some(digits(complete)?),
        };
        let end = last.checked_add(1)?;

        let within = complete.is_none_or(|complete| end <= complete);
        (first < end && within).then_some(Self {
            first,
            end,
            complete,
        })
    }

    fn of(response: &Response<Body>) -> Option<Self> {
        header_of(response, &CONTENT_RANGE).and_then(Self::read)
    }

    const fn bytes(&self) -> u64 {
        self.end - self.first
    }

    fn continues(&self, from: u64, body: Option<u64>) -> bool {
        self.first == from && body.is_none_or(|body| body == self.bytes())
    }
}

fn digits(text: &str) -> Option<u64> {
    let text = text.trim();
    (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn header_of<'a>(response: &'a Response<Body>, name: &HeaderName) -> Option<&'a str> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
}

pub(crate) fn validators_of(response: &Response<Body>) -> Validators {
    Validators {
        tag: header_of(response, &ETAG).and_then(EntityTag::new),
        modified: header_of(response, &LAST_MODIFIED).and_then(Modified::new),
    }
}

pub fn asking_to_resume(
    request: RequestBuilder<WithoutBody>,
    held: Option<&Held>,
) -> RequestBuilder<WithoutBody> {
    let request = request.header(ACCEPT_ENCODING, UNCODED);
    let Some(held) = held.filter(|held| held.can_be_checked()) else {
        return request;
    };

    let request = request.header(RANGE, format!("bytes={}-", held.bytes));
    match held.validators.strong_tag() {
        Some(tag) => request.header(IF_RANGE, tag.as_str()),
        None => request,
    }
}

#[derive(Clone, Debug, Default)]
pub struct Known {
    pub validators: Validators,
    pub total: Option<u64>,
}

#[derive(Clone, Copy)]
struct Expecting<'a> {
    validators: &'a Validators,
    total: Option<u64>,
}

impl<'a> Expecting<'a> {
    fn of(known: &'a Known) -> Self {
        Self {
            validators: &known.validators,
            total: known.total,
        }
    }

    fn total_differs(&self, whole: Option<u64>) -> bool {
        self.total
            .zip(whole)
            .is_some_and(|(known, whole)| known != whole)
    }

    fn changed(&self, answered: &Validators, partial: bool) -> bool {
        let unvouched_whole = !partial
            && self.validators.strong_tag().is_some()
            && !self.validators.vouch_for(answered);
        self.validators.conflicts_with(answered) || unvouched_whole
    }
}

fn asked(
    agent: &Agent,
    url: &str,
    from: u64,
    bearer: Option<&str>,
    expecting: Expecting<'_>,
) -> Result<Answered, Unfetched> {
    let mut request = agent.get(url).header(ACCEPT_ENCODING, UNCODED);

    if let Some(bearer) = bearer {
        request = request.header(AUTHORIZATION, bearer);
    }
    if from > 0 {
        request = request.header(RANGE, format!("bytes={from}-"));
        if let Some(tag) = expecting.validators.strong_tag() {
            request = request.header(IF_RANGE, tag.as_str());
        }
    }

    let response = request
        .call()
        .map_err(|error| Unfetched::Io(as_io(error)))?;
    let status = response.status().as_u16();
    let length = response.body().content_length();
    let validators = validators_of(&response);

    let (skipped, total, bound) = match status {
        PARTIAL if from > 0 => {
            let interval = Interval::of(&response)
                .filter(|interval| interval.continues(from, length))
                .ok_or(Unfetched::Misranged)?;
            (0, interval.complete, interval.bytes())
        }
        WHOLE => (from, length, UNBOUNDED),
        status if ASKED_LATER.contains(&status) => {
            return Err(Unfetched::AskedLater {
                status,
                asked_for: retry_after_of(&response),
            });
        }
        status => return Err(Unfetched::Refused(status)),
    };

    if expecting.total_differs(total) || expecting.changed(&validators, status == PARTIAL) {
        return Err(Unfetched::Changed);
    }

    let mut reader = response.into_body().into_reader();

    if skipped > 0 {
        let passed =
            io::copy(&mut (&mut reader).take(skipped), &mut io::sink()).map_err(Unfetched::Io)?;
        if passed < skipped {
            return Err(Unfetched::Io(io::Error::from(ErrorKind::UnexpectedEof)));
        }
    }
    Ok(Answered {
        reader: reader.take(bound),
        length,
        total,
        validators,
    })
}

struct Answered {
    reader: Bounded,
    length: Option<u64>,
    total: Option<u64>,
    validators: Validators,
}

pub struct Ranged {
    agent: Agent,
    url: String,
    bearer: Option<String>,
    reader: Bounded,
    length: Option<u64>,
    total: Option<u64>,
    read: u64,
    from: u64,
    validators: Validators,
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
            total: response.body().content_length(),
            validators: validators_of(&response),
            reader: response.into_body().into_reader().take(UNBOUNDED),
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
                let interval = Interval::of(&response)?;
                let answered = validators_of(&response);
                let expecting = Expecting {
                    validators: &held.validators,
                    total: held.total,
                };

                let unchanged = interval.continues(held.bytes, response.body().content_length())
                    && held
                        .total
                        .is_none_or(|total| interval.complete == Some(total))
                    && !expecting.changed(&answered, true);
                unchanged.then(|| Self {
                    agent,
                    url,
                    bearer: None,
                    length: interval.complete.or(held.total),
                    total: interval.complete.or(held.total),
                    validators: held.validators.clone().joined(answered),
                    reader: response.into_body().into_reader().take(interval.bytes()),
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
        Self::opened_bearing(agent, url, from, None, &Known::default(), resumed_after)
    }

    pub fn opened_bearing(
        agent: Agent,
        url: String,
        from: u64,
        bearer: Option<String>,
        known: &Known,
        resumed_after: Duration,
    ) -> Result<Self, Unfetched> {
        let expecting = Expecting::of(known);
        let mut tried = 0;

        let answered = loop {
            match asked(&agent, &url, from, bearer.as_deref(), expecting) {
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
            total: answered.total.or(known.total),
            validators: known.validators.clone().joined(answered.validators),
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

    pub fn validators(&self) -> &Validators {
        &self.validators
    }

    pub fn total(&self) -> Option<u64> {
        self.total
    }

    pub fn into_resumed(self) -> Resumed {
        Resumed {
            from: self.from,
            length: self.length,
            validators: self.validators.clone(),
            reader: Box::new(self),
        }
    }

    pub fn read_so_far(&self) -> u64 {
        self.read
    }

    pub fn length(&self) -> Option<u64> {
        self.length
    }

    fn ended_short(&self, buf: &[u8]) -> bool {
        !buf.is_empty() && self.total.is_some_and(|total| self.read < total)
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

            let expecting = Expecting {
                validators: &self.validators,
                total: self.total,
            };
            match asked(
                &self.agent,
                &self.url,
                self.read,
                self.bearer.as_deref(),
                expecting,
            ) {
                Ok(answered) => {
                    self.reader = answered.reader;
                    self.total = self.total.or(answered.total);
                    self.validators = mem::take(&mut self.validators).joined(answered.validators);
                    return Ok(());
                }
                Err(
                    stopped @ (Unfetched::Refused(_) | Unfetched::Changed | Unfetched::Misranged),
                ) => {
                    return Err(stopped.into_io());
                }
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
                Ok(0) if self.ended_short(buf) => {
                    self.resumed_after(io::Error::from(ErrorKind::UnexpectedEof))?;
                }
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
    use crate::{
        Trusted, configured,
        testing::{
            Answer, CAPPED_AT, CUT_AT, WHOLE_BODY, answering, capped_partial, coded_as_asked,
            cut_first_answer, emptied_partial, lower, opened, partial_from, serving,
        },
    };

    const OPENED_PART_WAY: u64 = 10;

    #[test]
    fn a_server_capping_every_partial_answer_still_yields_the_whole_body() {
        let (url, heads) = serving(vec![
            Box::new(|head| capped_partial(head, CAPPED_AT)),
            Box::new(|head| capped_partial(head, CAPPED_AT)),
            Box::new(|head| capped_partial(head, CAPPED_AT)),
            Box::new(|head| capped_partial(head, CAPPED_AT)),
        ]);
        let agent = configured(Duration::from_secs(5), Trusted::BuiltInRoots)
            .build()
            .new_agent();

        let mut ranged = Ranged::opened_from(agent, url, OPENED_PART_WAY, Duration::ZERO)
            .expect("the first partial answer");
        let mut read = Vec::new();

        ranged.read_to_end(&mut read).expect("the whole is read");

        assert_eq!(read, &WHOLE_BODY[OPENED_PART_WAY as usize..]);
        assert_eq!(ranged.read_so_far(), WHOLE_BODY.len() as u64);

        let asked_from: Vec<String> = heads
            .lock()
            .iter()
            .filter_map(|head| {
                lower(head)
                    .lines()
                    .find_map(|line| line.strip_prefix("range: ").map(str::to_owned))
            })
            .collect();

        assert_eq!(
            asked_from,
            ["bytes=10-", "bytes=30-", "bytes=50-", "bytes=70-"]
        );
    }

    #[test]
    fn a_server_answering_nothing_more_under_the_total_ends_in_an_error_after_a_bounded_number_of_asks()
     {
        let mut answers: Vec<Answer> = vec![Box::new(|head| capped_partial(head, CAPPED_AT))];

        answers.extend((0..RESUMES_AT_MOST + 2).map(|_| Box::new(emptied_partial) as Answer));

        let (url, heads) = serving(answers);
        let agent = configured(Duration::from_secs(5), Trusted::BuiltInRoots)
            .build()
            .new_agent();

        let mut ranged = Ranged::opened_from(agent, url, OPENED_PART_WAY, Duration::ZERO)
            .expect("the first partial answer");
        let mut read = Vec::new();

        let error = ranged
            .read_to_end(&mut read)
            .expect_err("the song ends under its total");

        assert_eq!(error.kind(), ErrorKind::UnexpectedEof);
        assert_eq!(read, &WHOLE_BODY[OPENED_PART_WAY as usize..30]);
        assert_eq!(heads.lock().len(), 1 + RESUMES_AT_MOST as usize);
    }

    #[test]
    fn a_download_cut_off_is_asked_for_again_with_its_validator_and_read_on() {
        let (url, heads) = serving(vec![
            Box::new(|_| cut_first_answer(&["ETag: \"v1\""])),
            Box::new(|_| partial_from(CUT_AT, WHOLE_BODY.len(), &["ETag: \"v1\""])),
        ]);
        let mut ranged = opened(&url);
        let mut read = Vec::new();

        ranged.read_to_end(&mut read).expect("the whole is read");

        assert_eq!(read, WHOLE_BODY);

        let asked = lower(&heads.lock()[1]);

        assert!(
            asked.contains(&format!("range: bytes={CUT_AT}-")),
            "{asked}"
        );
        assert!(asked.contains("if-range: \"v1\""), "{asked}");
    }

    #[test]
    fn a_file_replaced_between_the_break_and_the_resume_is_not_joined_old_head_to_new_tail() {
        let replaced = || answering("200 OK", &["ETag: \"v2\""], WHOLE_BODY, WHOLE_BODY.len());
        let (url, _) = serving(vec![
            Box::new(|_| cut_first_answer(&["ETag: \"v1\""])),
            Box::new(move |_| replaced()),
        ]);
        let mut ranged = opened(&url);
        let mut read = Vec::new();

        let error = ranged
            .read_to_end(&mut read)
            .expect_err("the change is refused");

        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert_eq!(read, &WHOLE_BODY[..CUT_AT]);
    }

    #[test]
    fn a_partial_answer_naming_another_total_than_the_one_held_is_refused() {
        let (url, _) = serving(vec![
            Box::new(|_| cut_first_answer(&[])),
            Box::new(|_| partial_from(CUT_AT, WHOLE_BODY.len() + 50, &[])),
        ]);
        let mut ranged = opened(&url);

        let error = ranged
            .read_to_end(&mut Vec::new())
            .expect_err("the change is refused");

        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn a_whole_answer_of_another_length_than_the_one_held_is_refused_and_one_of_the_same_is_read_past()
     {
        let longer = [WHOLE_BODY, b"extra"].concat();
        let (url, _) = serving(vec![
            Box::new(|_| cut_first_answer(&[])),
            Box::new(move |_| answering("200 OK", &[], &longer, longer.len())),
        ]);
        let error = opened(&url)
            .read_to_end(&mut Vec::new())
            .expect_err("the change is refused");

        assert_eq!(error.kind(), ErrorKind::InvalidData);

        let (url, heads) = serving(vec![
            Box::new(|_| cut_first_answer(&[])),
            Box::new(|_| answering("200 OK", &[], WHOLE_BODY, WHOLE_BODY.len())),
        ]);
        let mut read = Vec::new();

        opened(&url)
            .read_to_end(&mut read)
            .expect("the whole is read");

        assert_eq!(read, WHOLE_BODY);
        assert!(!lower(&heads.lock()[1]).contains("if-range"));
    }

    const MONDAY: &str = "Last-Modified: Mon, 02 Nov 2026 10:00:00 GMT";
    const TUESDAY: &str = "Last-Modified: Tue, 03 Nov 2026 10:00:00 GMT";

    fn read_after_a_cut(first: &[&str], resumed: Answer) -> (io::Result<Vec<u8>>, String) {
        let first: Vec<String> = first.iter().map(|header| (*header).to_owned()).collect();
        let (url, heads) = serving(vec![
            Box::new(move |_| {
                let first: Vec<&str> = first.iter().map(String::as_str).collect();
                cut_first_answer(&first)
            }),
            resumed,
        ]);
        let mut read = Vec::new();

        let ended = opened(&url).read_to_end(&mut read).map(|_| read);
        let asked = lower(&heads.lock()[1]);
        (ended, asked)
    }

    #[test]
    fn a_date_validated_resume_may_bring_a_tag_beside_its_unchanged_date() {
        let (read, _) = read_after_a_cut(
            &[MONDAY],
            Box::new(|_| partial_from(CUT_AT, WHOLE_BODY.len(), &[MONDAY, "ETag: \"new\""])),
        );

        assert_eq!(read.expect("the whole is read"), WHOLE_BODY);
    }

    #[test]
    fn a_resume_bearing_only_the_date_of_a_tagged_first_answer_is_read_on() {
        let (read, _) = read_after_a_cut(
            &["ETag: \"v1\"", MONDAY],
            Box::new(|_| partial_from(CUT_AT, WHOLE_BODY.len(), &[MONDAY])),
        );

        assert_eq!(read.expect("the whole is read"), WHOLE_BODY);
    }

    #[test]
    fn a_whole_answer_bearing_the_date_of_a_tagged_first_answer_is_read_past() {
        let (read, _) = read_after_a_cut(
            &["ETag: \"v1\"", MONDAY],
            Box::new(|_| answering("200 OK", &[MONDAY], WHOLE_BODY, WHOLE_BODY.len())),
        );

        assert_eq!(read.expect("the whole is read"), WHOLE_BODY);
    }

    #[test]
    fn a_resume_naming_another_validator_of_the_same_kind_is_refused() {
        for (first, resumed) in [
            ([MONDAY], TUESDAY),
            (["ETag: \"v1\""], "ETag: \"v2\""),
            (["ETag: W/\"v1\""], "ETag: W/\"v2\""),
        ] {
            let (read, _) = read_after_a_cut(
                &first,
                Box::new(move |_| partial_from(CUT_AT, WHOLE_BODY.len(), &[resumed])),
            );

            assert_eq!(
                read.map_err(|error| error.kind()),
                Err(ErrorKind::InvalidData),
                "{resumed}"
            );
        }
    }

    #[test]
    fn if_range_carries_only_a_strong_tag() {
        for (first, if_range) in [
            (vec![MONDAY], None),
            (vec!["ETag: W/\"v1\"", MONDAY], None),
            (vec!["ETag: \"v1\"", MONDAY], Some("if-range: \"v1\"")),
        ] {
            let (_, asked) = read_after_a_cut(
                &first,
                Box::new(|_| partial_from(CUT_AT, WHOLE_BODY.len(), &[])),
            );

            assert!(
                asked.contains(&format!("range: bytes={CUT_AT}-")),
                "{asked}"
            );
            match if_range {
                Some(if_range) => assert!(asked.contains(if_range), "{asked}"),
                None => assert!(!asked.contains("if-range"), "{asked}"),
            }
        }
    }

    #[test]
    fn a_resume_whose_interval_disagrees_with_its_body_or_its_whole_is_refused() {
        let whole = WHOLE_BODY.len();
        for (range, declared) in [
            (
                format!("Content-Range: bytes {CUT_AT}-{}/{whole}", whole - 1),
                whole - CUT_AT - 5,
            ),
            (
                format!("Content-Range: bytes {CUT_AT}-{whole}/{whole}"),
                whole - CUT_AT + 1,
            ),
            (
                format!("Content-Range: bytes {CUT_AT}-{}/{whole}", CUT_AT - 1),
                0,
            ),
            (
                format!("Content-Range: bytes {}-{}/{whole}", CUT_AT + 1, whole - 1),
                whole - CUT_AT - 1,
            ),
        ] {
            let header = range.clone();
            let (read, _) = read_after_a_cut(
                &[],
                Box::new(move |_| {
                    answering(
                        "206 Partial Content",
                        &[header.as_str()],
                        &WHOLE_BODY[CUT_AT..],
                        declared,
                    )
                }),
            );

            assert_eq!(
                read.map_err(|error| error.kind()),
                Err(ErrorKind::InvalidData),
                "{range}"
            );
        }
    }

    fn held_at_the_cut(validators: &[&str]) -> Held {
        let mut known = Validators::default();

        for header in validators {
            match header.split_once(": ") {
                Some(("ETag", tag)) => known.tag = EntityTag::new(tag),
                Some(("Last-Modified", date)) => known.modified = Modified::new(date),
                _ => panic!("{header} is no validator"),
            }
        }
        Held {
            bytes: CUT_AT as u64,
            total: Some(WHOLE_BODY.len() as u64),
            validators: known,
        }
    }

    fn continued(held: &Held, answer: Answer) -> (Option<Vec<u8>>, String) {
        let (url, heads) = serving(vec![
            answer,
            Box::new(|head| capped_partial(head, WHOLE_BODY.len())),
        ]);
        let agent = configured(Duration::from_secs(5), Trusted::BuiltInRoots)
            .build()
            .new_agent();

        let response = asking_to_resume(agent.get(&url), Some(held))
            .call()
            .expect("an answer");

        let read = Ranged::continuing_held(agent, url, response, held, Duration::ZERO).map(
            |mut ranged| {
                let mut read = Vec::new();

                ranged.read_to_end(&mut read).expect("the rest is read");
                read
            },
        );
        let asked = lower(&heads.lock()[0]);
        (read, asked)
    }

    #[test]
    fn a_held_partial_answered_with_a_conflicting_strong_tag_is_not_continued() {
        let held = held_at_the_cut(&["ETag: \"v1\""]);

        let (conflicting, asked) = continued(
            &held,
            Box::new(|_| partial_from(CUT_AT, WHOLE_BODY.len(), &["ETag: \"v2\""])),
        );
        let (matching, _) = continued(
            &held,
            Box::new(|_| partial_from(CUT_AT, WHOLE_BODY.len(), &["ETag: \"v1\""])),
        );

        assert!(asked.contains("if-range: \"v1\""), "{asked}");
        assert_eq!(conflicting, None);
        assert_eq!(matching.as_deref(), Some(&WHOLE_BODY[CUT_AT..]));
    }

    #[test]
    fn a_held_partial_validated_by_date_takes_a_tag_beside_the_same_date_and_refuses_another_date()
    {
        let held = held_at_the_cut(&[MONDAY]);

        let (tagged, asked) = continued(
            &held,
            Box::new(|_| partial_from(CUT_AT, WHOLE_BODY.len(), &[MONDAY, "ETag: \"new\""])),
        );
        let (redated, _) = continued(
            &held,
            Box::new(|_| partial_from(CUT_AT, WHOLE_BODY.len(), &[TUESDAY])),
        );

        assert!(!asked.contains("if-range"), "{asked}");
        assert_eq!(tagged.as_deref(), Some(&WHOLE_BODY[CUT_AT..]));
        assert_eq!(redated, None);
    }

    #[test]
    fn a_held_partial_answered_with_an_inconsistent_interval_is_not_continued() {
        let held = held_at_the_cut(&[]);
        let whole = WHOLE_BODY.len();

        let (misframed, _) = continued(
            &held,
            Box::new(move |_| {
                let range = format!("Content-Range: bytes {CUT_AT}-{}/{whole}", whole - 1);
                answering(
                    "206 Partial Content",
                    &[range.as_str()],
                    &WHOLE_BODY[CUT_AT..],
                    7,
                )
            }),
        );
        let (past_the_whole, _) = continued(
            &held,
            Box::new(move |_| {
                let range = format!("Content-Range: bytes {CUT_AT}-{}/{whole}", whole + 3);
                answering(
                    "206 Partial Content",
                    &[range.as_str()],
                    &WHOLE_BODY[CUT_AT..],
                    whole + 4 - CUT_AT,
                )
            }),
        );

        assert_eq!(misframed, None);
        assert_eq!(past_the_whole, None);
    }

    #[test]
    fn a_held_partial_answered_with_a_shorter_valid_interval_is_continued_and_read_to_the_end() {
        let held = held_at_the_cut(&[]);

        let (read, _) = continued(&held, Box::new(|head| capped_partial(head, CAPPED_AT)));

        assert_eq!(read.as_deref(), Some(&WHOLE_BODY[CUT_AT..]));
    }

    const LEFT_UNSENT: usize = 12;

    #[test]
    fn a_download_is_asked_for_uncoded_so_a_resume_counts_the_bytes_the_server_ranges_over() {
        let (url, heads) = serving(vec![
            Box::new(move |head| coded_as_asked(head, Some(LEFT_UNSENT))),
            Box::new(|head| coded_as_asked(head, None)),
        ]);
        let agent = configured(Duration::from_secs(5), Trusted::BuiltInRoots)
            .build()
            .new_agent();

        let mut ranged =
            Ranged::opened_from(agent, url, 0, Duration::ZERO).expect("the first answer");
        let mut read = Vec::new();

        ranged.read_to_end(&mut read).expect("the whole is read");

        assert_eq!(read, WHOLE_BODY);
        for head in heads.lock().iter() {
            assert!(lower(head).contains("accept-encoding: identity"), "{head}");
        }
    }

    #[test]
    fn a_request_asking_to_resume_asks_for_uncoded_bytes_held_or_not() {
        let (url, heads) = serving(vec![Box::new(|_| {
            answering("200 OK", &[], WHOLE_BODY, WHOLE_BODY.len())
        })]);
        let agent = configured(Duration::from_secs(5), Trusted::BuiltInRoots)
            .build()
            .new_agent();

        asking_to_resume(agent.get(&url), None)
            .call()
            .expect("an answer");

        assert!(lower(&heads.lock()[0]).contains("accept-encoding: identity"));
    }

    #[test]
    fn a_range_is_read_as_one_interval_within_its_whole() {
        assert_eq!(
            Interval::read("bytes 100-999/1000"),
            Some(Interval {
                first: 100,
                end: 1000,
                complete: Some(1000)
            })
        );
        assert_eq!(
            Interval::read("bytes 0-0/*"),
            Some(Interval {
                first: 0,
                end: 1,
                complete: None
            })
        );
        assert_eq!(
            Interval::read("bytes 100-999/1000").map(|i| i.bytes()),
            Some(900)
        );
    }

    #[test]
    fn a_range_whose_bounds_disagree_is_no_interval() {
        for refused in [
            "bytes */1000",
            "items 1-2/3",
            "bytes 100-99/1000",
            "bytes 100-1000/1000",
            "bytes 100-/1000",
            "bytes -5/1000",
            "bytes +1-5/1000",
            "bytes 0-18446744073709551615/*",
            "bytes 1-5/x",
            "bytes 1-5",
            "bytes1-5/9",
        ] {
            assert_eq!(Interval::read(refused), None, "{refused}");
        }
    }

    #[test]
    fn an_interval_continues_only_from_where_it_was_asked_and_as_long_as_its_body() {
        let interval = Interval::read("bytes 20-71/72").expect("an interval");

        assert!(interval.continues(20, Some(52)));
        assert!(interval.continues(20, None));
        assert!(!interval.continues(21, Some(52)));
        assert!(!interval.continues(20, Some(51)));
        assert!(!interval.continues(20, Some(53)));
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
