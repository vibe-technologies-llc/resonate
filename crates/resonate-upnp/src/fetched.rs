use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    net::TcpStream,
    time::Duration,
};

use crate::{
    Asked, Error, Located, Result,
    http::{HEAD_BYTES_AT_MOST, HEADERS_AT_MOST, USER_AGENT, head_end, read_more},
};

pub const CONNECTED_WITHIN: Duration = Duration::from_secs(3);

pub const SILENT_AT_MOST: Duration = Duration::from_secs(15);

const PASSED_OVER_AT_MOST: u64 = 64 << 20;

const DISCARDED_IN: usize = 64 << 10;

pub struct Fetched {
    at: Located,
    length: Option<u64>,
    media_type: Option<String>,
    position: u64,
    answer: Option<Answer>,
}

struct Answer {
    stream: TcpStream,
    held: Vec<u8>,
    left: Option<u64>,
}

struct Head {
    status: u16,
    length: Option<u64>,
    range: Option<(u64, Option<u64>)>,
    media_type: Option<String>,
    chunked: bool,
}

impl Fetched {
    pub(crate) fn open(at: Located) -> Result<Self> {
        let mut fetched = Self {
            at,
            length: None,
            media_type: None,
            position: 0,
            answer: None,
        };
        fetched.answer = Some(fetched.asked_from(0)?);
        Ok(fetched)
    }

    pub const fn length(&self) -> Option<u64> {
        self.length
    }

    pub fn media_type(&self) -> Option<&str> {
        self.media_type.as_deref()
    }

    fn asked_from(&mut self, from: u64) -> Result<Answer> {
        let failed = |source: io::Error| Error::Network {
            asked: Asked::Media,
            source,
        };
        let address = self.at.reached_at().map_err(failed)?;
        let mut stream = TcpStream::connect_timeout(&address, CONNECTED_WITHIN).map_err(failed)?;
        stream
            .set_write_timeout(Some(CONNECTED_WITHIN))
            .map_err(failed)?;
        stream
            .set_read_timeout(Some(SILENT_AT_MOST))
            .map_err(failed)?;
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {USER_AGENT}\r\nConnection: \
             close\r\nRange: bytes={from}-\r\nAccept-Encoding: identity\r\n\r\n",
            self.at.path(),
            self.at.authority(),
        );
        stream.write_all(request.as_bytes()).map_err(failed)?;

        let mut held = Vec::new();
        let ends = loop {
            if let Some(at) = head_end(&held) {
                break at;
            }
            if held.len() > HEAD_BYTES_AT_MOST
                || read_more(&mut stream, &mut held).map_err(failed)? == 0
            {
                return Err(Error::NotHttp {
                    asked: Asked::Media,
                });
            }
        };
        let head = head_of(held.get(..ends).unwrap_or_default()).ok_or(Error::NotHttp {
            asked: Asked::Media,
        })?;
        let held = held.split_off(ends);
        if head.chunked {
            return Err(Error::NotHttp {
                asked: Asked::Media,
            });
        }
        if self.media_type.is_none() {
            self.media_type = head.media_type;
        }

        let mut answer = Answer {
            stream,
            held,
            left: head.length,
        };
        match (head.status, head.range) {
            (206, Some((start, total))) if start == from => {
                if let Some(total) = total {
                    self.length = Some(total);
                }
            }
            (206, _) => return Err(Error::RangeMisanswered),
            (200, _) => {
                if let Some(length) = head.length {
                    self.length = Some(length);
                }
                if from > PASSED_OVER_AT_MOST {
                    return Err(Error::RangeMisanswered);
                }
                answer.pass_over(from).map_err(failed)?;
            }
            (status, _) => {
                return Err(Error::Status {
                    asked: Asked::Media,
                    status,
                });
            }
        }
        Ok(answer)
    }
}

impl Answer {
    fn pass_over(&mut self, mut bytes: u64) -> io::Result<()> {
        let mut discarded = vec![0; DISCARDED_IN];
        while bytes > 0 {
            let wanted = usize::try_from(bytes)
                .unwrap_or(usize::MAX)
                .min(DISCARDED_IN);
            let read = self.read(&mut discarded[..wanted])?;
            if read == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            bytes -= read as u64;
        }
        Ok(())
    }
}

impl Read for Answer {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let room = match self.left {
            Some(0) => return Ok(0),
            Some(left) => usize::try_from(left).unwrap_or(usize::MAX).min(buf.len()),
            None => buf.len(),
        };
        let Some(into) = buf.get_mut(..room) else {
            return Ok(0);
        };
        let read = if self.held.is_empty() {
            self.stream.read(into)?
        } else {
            let taken = self.held.len().min(into.len());
            into[..taken].copy_from_slice(&self.held[..taken]);
            self.held.drain(..taken);
            taken
        };
        if let Some(left) = &mut self.left {
            *left -= read as u64;
        }
        Ok(read)
    }
}

impl Read for Fetched {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.length.is_some_and(|length| self.position >= length) {
            return Ok(0);
        }
        if self.answer.is_none() {
            let answer = self.asked_from(self.position).map_err(io::Error::other)?;
            self.answer = Some(answer);
        }
        let Some(answer) = &mut self.answer else {
            return Ok(0);
        };
        let read = answer.read(buf)?;
        if read == 0 && self.length.is_some_and(|length| self.position < length) {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for Fetched {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::Current(by) => self.position.checked_add_signed(by),
            SeekFrom::End(by) => self
                .length
                .ok_or_else(|| io::Error::from(io::ErrorKind::Unsupported))?
                .checked_add_signed(by),
        }
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        if target != self.position {
            self.answer = None;
            self.position = target;
        }
        Ok(target)
    }
}

fn head_of(bytes: &[u8]) -> Option<Head> {
    let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];
    let mut response = httparse::Response::new(&mut headers);
    match response.parse(bytes) {
        Ok(httparse::Status::Complete(_)) => {}
        Ok(httparse::Status::Partial) | Err(_) => return None,
    }
    let named = |name: &str| {
        response
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .and_then(|header| std::str::from_utf8(header.value).ok())
            .map(str::trim)
    };
    Some(Head {
        status: response.code?,
        length: named("Content-Length").and_then(|length| length.parse().ok()),
        range: named("Content-Range").and_then(range_of),
        media_type: named("Content-Type")
            .map(|kind| kind.split(';').next().unwrap_or_default().trim().to_owned())
            .filter(|kind| !kind.is_empty()),
        chunked: named("Transfer-Encoding")
            .is_some_and(|coding| coding.eq_ignore_ascii_case("chunked")),
    })
}

pub(crate) fn range_of(header: &str) -> Option<(u64, Option<u64>)> {
    let span = header.strip_prefix("bytes")?.trim_start();
    let (span, total) = span.split_once('/')?;
    let (start, end) = span.trim().split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let end: u64 = end.trim().parse().ok()?;
    if end < start {
        return None;
    }
    let total = match total.trim() {
        "*" => None,
        total => Some(total.parse::<u64>().ok().filter(|total| *total > end)?),
    };
    Some((start, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_content_range_is_read_for_its_start_and_whole_and_a_wrong_one_is_not() {
        assert_eq!(range_of("bytes 100-199/1000"), Some((100, Some(1_000))));
        assert_eq!(range_of("bytes 0-0/*"), Some((0, None)));
        assert_eq!(range_of("bytes 5-4/10"), None);
        assert_eq!(range_of("bytes 0-99/50"), None);
        assert_eq!(range_of("items 0-1/2"), None);
    }
}
