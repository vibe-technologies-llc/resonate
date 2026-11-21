use std::{
    io::{self, BufRead, BufReader, Chain, Cursor, Read, Seek, SeekFrom, Write},
    net::TcpStream,
    time::Duration,
};

use crate::{
    Asked, Error, Located, Result,
    http::{HEAD_BYTES_AT_MOST, HEADERS_AT_MOST, USER_AGENT, head_end, read_more},
};

pub const CONNECTED_WITHIN: Duration = Duration::from_secs(3);

pub const SILENT_AT_MOST: Duration = Duration::from_secs(15);

const ASKED_AGAIN_AT_MOST: u32 = 5;

const PASSED_OVER_AT_MOST: u64 = 64 << 20;

const DISCARDED_IN: usize = 64 << 10;

const CHUNK_LINE_BYTES_AT_MOST: u64 = 1 << 10;

const LINE_END: &[u8] = b"\r\n";

pub struct Fetched {
    at: Located,
    length: Option<u64>,
    media_type: Option<String>,
    position: u64,
    answer: Option<Answer>,
}

struct Answer {
    body: BufReader<Chain<Cursor<Vec<u8>>, TcpStream>>,
    framing: Framing,
}

enum Framing {
    Declared { left: u64 },
    UntilClosed,
    Chunked(Chunk),
}

#[derive(Clone, Copy)]
enum Chunk {
    SizeNext,
    Within { left: u64 },
    Ended,
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

        if self.media_type.is_none() {
            self.media_type = head.media_type;
        }

        let framing = match (head.chunked, head.length) {
            (true, _) => Framing::Chunked(Chunk::SizeNext),
            (false, Some(left)) => Framing::Declared { left },
            (false, None) => Framing::UntilClosed,
        };
        let mut answer = Answer {
            body: BufReader::new(Cursor::new(held).chain(stream)),
            framing,
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

impl Answer {
    fn read_chunked(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let Framing::Chunked(chunk) = &mut self.framing else {
                return Ok(0);
            };
            match *chunk {
                Chunk::Ended => return Ok(0),
                Chunk::SizeNext => {
                    *chunk = match chunk_size(&mut self.body)? {
                        0 => Chunk::Ended,
                        left => Chunk::Within { left },
                    };
                }
                Chunk::Within { left: 0 } => {
                    let mut end = [0; LINE_END.len()];

                    self.body.read_exact(&mut end)?;
                    if end != LINE_END {
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                    *chunk = Chunk::SizeNext;
                }
                Chunk::Within { left } => {
                    let room = usize::try_from(left).unwrap_or(usize::MAX).min(buf.len());
                    let read = self.body.read(&mut buf[..room])?;

                    if read == 0 {
                        return Err(io::ErrorKind::UnexpectedEof.into());
                    }
                    *chunk = Chunk::Within {
                        left: left - read as u64,
                    };
                    return Ok(read);
                }
            }
        }
    }
}

fn chunk_size(body: &mut impl BufRead) -> io::Result<u64> {
    let mut line = Vec::new();

    body.take(CHUNK_LINE_BYTES_AT_MOST)
        .read_until(b'\n', &mut line)?;

    let Some(line) = line.strip_suffix(LINE_END) else {
        return Err(if line.is_empty() {
            io::ErrorKind::UnexpectedEof
        } else {
            io::ErrorKind::InvalidData
        }
        .into());
    };
    let size = line
        .split(|byte| *byte == b';')
        .next()
        .and_then(|size| std::str::from_utf8(size).ok())
        .map(str::trim)
        .filter(|size| !size.is_empty())
        .and_then(|size| u64::from_str_radix(size, 16).ok());
    size.ok_or_else(|| io::ErrorKind::InvalidData.into())
}

impl Read for Answer {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        match self.framing {
            Framing::Chunked(_) => self.read_chunked(buf),
            Framing::UntilClosed => self.body.read(buf),
            Framing::Declared { left: 0 } => Ok(0),
            Framing::Declared { left } => {
                let room = usize::try_from(left).unwrap_or(usize::MAX).min(buf.len());
                let read = self.body.read(&mut buf[..room])?;

                self.framing = Framing::Declared {
                    left: left - read as u64,
                };
                Ok(read)
            }
        }
    }
}

impl Read for Fetched {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.length.is_some_and(|length| self.position >= length) {
            return Ok(0);
        }

        let mut asked_without_a_byte = 0;
        loop {
            if self.answer.is_none() {
                let answer = self.asked_from(self.position).map_err(io::Error::other)?;

                self.answer = Some(answer);
            }

            let Some(answer) = &mut self.answer else {
                return Ok(0);
            };
            let read = answer.read(buf)?;
            let ended_short = read == 0 && self.length.is_some_and(|length| self.position < length);

            if !ended_short {
                self.position += read as u64;
                return Ok(read);
            }
            if asked_without_a_byte == ASKED_AGAIN_AT_MOST {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            asked_without_a_byte += 1;
            self.answer = None;
        }
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
    let chunked =
        named("Transfer-Encoding").is_some_and(|coding| coding.eq_ignore_ascii_case("chunked"));
    Some(Head {
        status: response.code?,
        length: named("Content-Length")
            .filter(|_| !chunked)
            .and_then(|length| length.parse().ok()),
        range: named("Content-Range").and_then(range_of),
        media_type: named("Content-Type")
            .map(|kind| kind.split(';').next().unwrap_or_default().trim().to_owned())
            .filter(|kind| !kind.is_empty()),
        chunked,
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

    fn answer_of(served: &[u8]) -> Answer {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bound");
        let stream =
            TcpStream::connect(listener.local_addr().expect("an address")).expect("connected");
        let (mut serving, _) = listener.accept().expect("accepted");

        serving.write_all(served).expect("written");
        drop(serving);
        Answer {
            body: BufReader::new(Cursor::new(Vec::new()).chain(stream)),
            framing: Framing::Chunked(Chunk::SizeNext),
        }
    }

    const SONG_BYTES: usize = 100;

    const CAPPED_AT: usize = 30;

    const SOUGHT_TO: u64 = 40;

    fn song() -> Vec<u8> {
        (0..SONG_BYTES).map(|at| at as u8).collect()
    }

    fn asked_from_of(head: &str) -> usize {
        head.lines()
            .find_map(|line| line.strip_prefix("Range: bytes="))
            .and_then(|range| range.strip_suffix('-'))
            .and_then(|from| from.parse().ok())
            .unwrap_or_default()
    }

    fn serving_capped_answers(answers: usize, cap: usize) -> Located {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bound");
        let at = listener.local_addr().expect("an address");

        std::thread::spawn(move || {
            for _ in 0..answers {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut head = Vec::new();
                let mut buffer = [0; 512];

                while head_end(&head).is_none() {
                    match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => head.extend_from_slice(&buffer[..read]),
                    }
                }

                let from = asked_from_of(&String::from_utf8_lossy(&head));
                let end = (from + cap).min(SONG_BYTES);

                let reply = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {from}-{}/{SONG_BYTES}\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n",
                    end.saturating_sub(1),
                    end - from,
                );
                let _ = stream.write_all(reply.as_bytes());
                let _ = stream.write_all(&song()[from..end]);
            }
        });
        Located::parse(&format!("http://{at}/song")).expect("an address")
    }

    #[test]
    fn a_server_capping_every_partial_answer_is_asked_again_from_where_it_ended() {
        let mut fetched = Fetched::open(serving_capped_answers(8, CAPPED_AT)).expect("opened");
        let mut whole = Vec::new();

        fetched.read_to_end(&mut whole).expect("read whole");

        let landed = fetched.seek(SeekFrom::Start(SOUGHT_TO)).expect("a seek");
        let mut after = Vec::new();

        fetched.read_to_end(&mut after).expect("read from there");

        assert_eq!(whole, song());
        assert_eq!(landed, SOUGHT_TO);
        assert_eq!(after, &song()[SOUGHT_TO as usize..]);
    }

    #[test]
    fn a_server_answering_nothing_under_its_length_is_asked_a_bounded_number_of_times() {
        let mut fetched =
            Fetched::open(serving_capped_answers(ASKED_AGAIN_AT_MOST as usize + 3, 0))
                .expect("opened");
        let mut read = Vec::new();

        let error = fetched.read_to_end(&mut read).expect_err("ends short");

        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert!(read.is_empty());
    }

    #[test]
    fn a_chunked_body_is_read_as_its_chunks_and_a_cut_or_malformed_one_is_refused() {
        let mut whole = Vec::new();

        let read = answer_of(b"5\r\nhello\r\n7;ext=1\r\n, world\r\n0\r\nTrailer: x\r\n\r\n")
            .read_to_end(&mut whole);
        let mut cut = Vec::new();

        let cut_read = answer_of(b"5\r\nhel").read_to_end(&mut cut);
        let mut unended = Vec::new();

        let unended_read = answer_of(b"5\r\nhello\r\n").read_to_end(&mut unended);
        let mut malformed = Vec::new();

        let malformed_read = answer_of(b"zz\r\nhello\r\n0\r\n\r\n").read_to_end(&mut malformed);
        let mut overlong = Vec::new();

        let overlong_read = answer_of(b"5\r\nhelloXX0\r\n\r\n").read_to_end(&mut overlong);

        assert_eq!(read.expect("read whole"), 12);
        assert_eq!(whole, b"hello, world");
        assert_eq!(
            cut_read.expect_err("cut").kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert_eq!(
            unended_read.expect_err("unended").kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert_eq!(
            malformed_read.expect_err("malformed").kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            overlong_read.expect_err("overlong").kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn a_content_range_is_read_for_its_start_and_whole_and_a_wrong_one_is_not() {
        assert_eq!(range_of("bytes 100-199/1000"), Some((100, Some(1_000))));
        assert_eq!(range_of("bytes 0-0/*"), Some((0, None)));
        assert_eq!(range_of("bytes 5-4/10"), None);
        assert_eq!(range_of("bytes 0-99/50"), None);
        assert_eq!(range_of("items 0-1/2"), None);
    }
}
