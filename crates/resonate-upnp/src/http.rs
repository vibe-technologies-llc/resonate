use std::{
    io::{self, Read, Write},
    net::TcpStream,
    time::Duration,
};

use crate::{Asked, Error, Located, Result};

pub(crate) const ANSWERED_WITHIN: Duration = Duration::from_secs(3);

pub(crate) const ANSWER_BYTES_AT_MOST: usize = 1 << 20;

pub(crate) const HEAD_BYTES_AT_MOST: usize = 16 << 10;

pub(crate) const HEADERS_AT_MOST: usize = 64;

pub(crate) const USER_AGENT: &str = concat!("resonate/", env!("CARGO_PKG_VERSION"));

const READ_IN: usize = 8 << 10;

const LAST_CHUNK: &[u8] = b"0\r\n\r\n";

pub(crate) struct Asking<'a> {
    pub method: &'static str,
    pub to: &'a Located,
    pub headers: &'a [(&'a str, &'a str)],
    pub body: &'a [u8],
}

pub(crate) struct Answer {
    pub status: u16,
    pub body: Vec<u8>,
}

enum Length {
    Declared(usize),
    Chunked,
    UntilClosed,
}

pub(crate) fn exchange(asking: &Asking<'_>, asked: Asked) -> Result<Answer> {
    let reached = |source: io::Error| Error::Network { asked, source };

    let address = asking.to.reached_at().map_err(reached)?;
    let mut stream = TcpStream::connect_timeout(&address, ANSWERED_WITHIN).map_err(reached)?;
    stream
        .set_read_timeout(Some(ANSWERED_WITHIN))
        .map_err(reached)?;
    stream
        .set_write_timeout(Some(ANSWERED_WITHIN))
        .map_err(reached)?;

    stream.write_all(&request(asking)).map_err(reached)?;

    let mut held = Vec::new();
    let head_ends = loop {
        if let Some(at) = head_end(&held) {
            break at;
        }
        if held.len() > HEAD_BYTES_AT_MOST {
            return Err(Error::NotHttp { asked });
        }
        if read_more(&mut stream, &mut held).map_err(reached)? == 0 {
            return Err(Error::NotHttp { asked });
        }
    };

    let (status, length) = {
        let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];
        let mut response = httparse::Response::new(&mut headers);
        match response.parse(&held) {
            Ok(httparse::Status::Complete(_)) => {}
            Ok(httparse::Status::Partial) | Err(_) => return Err(Error::NotHttp { asked }),
        }
        let status = response.code.ok_or(Error::NotHttp { asked })?;
        (status, length_of(response.headers))
    };

    let mut body = held.split_off(head_ends);
    loop {
        if body.len() > ANSWER_BYTES_AT_MOST {
            return Err(Error::TooLong {
                asked,
                at_most: ANSWER_BYTES_AT_MOST,
            });
        }
        let whole = match length {
            Length::Declared(length) => body.len() >= length,
            Length::Chunked => body.ends_with(LAST_CHUNK),
            Length::UntilClosed => false,
        };
        if whole || read_more(&mut stream, &mut body).map_err(reached)? == 0 {
            break;
        }
    }

    let body = match length {
        Length::Declared(length) => {
            body.truncate(length);
            body
        }
        Length::Chunked => unchunked(&body).ok_or(Error::NotHttp { asked })?,
        Length::UntilClosed => body,
    };

    Ok(Answer { status, body })
}

fn request(asking: &Asking<'_>) -> Vec<u8> {
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {USER_AGENT}\r\nConnection: close\r\n",
        asking.method,
        asking.to.path(),
        asking.to.authority(),
    );
    for (name, value) in asking.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    if !asking.body.is_empty() {
        head.push_str(&format!("Content-Length: {}\r\n", asking.body.len()));
    }
    head.push_str("\r\n");

    let mut request = head.into_bytes();
    request.extend_from_slice(asking.body);
    request
}

pub(crate) fn head_end(held: &[u8]) -> Option<usize> {
    held.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|at| at + 4)
}

pub(crate) fn read_more(stream: &mut TcpStream, held: &mut Vec<u8>) -> io::Result<usize> {
    let mut buffer = [0; READ_IN];
    let read = stream.read(&mut buffer)?;
    held.extend_from_slice(buffer.get(..read).unwrap_or_default());
    Ok(read)
}

fn length_of(headers: &[httparse::Header<'_>]) -> Length {
    let named = |name: &str| {
        headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .and_then(|header| std::str::from_utf8(header.value).ok())
            .map(str::trim)
    };
    if named("Transfer-Encoding").is_some_and(|coding| coding.eq_ignore_ascii_case("chunked")) {
        return Length::Chunked;
    }
    named("Content-Length")
        .and_then(|length| length.parse().ok())
        .map_or(Length::UntilClosed, Length::Declared)
}

fn unchunked(mut chunked: &[u8]) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let line_ends = chunked.windows(2).position(|pair| pair == b"\r\n")?;
        let line = std::str::from_utf8(chunked.get(..line_ends)?).ok()?;
        let size = line.split(';').next()?.trim();
        let size = usize::from_str_radix(size, 16).ok()?;
        chunked = chunked.get(line_ends + 2..)?;
        if size == 0 {
            return Some(body);
        }
        body.extend_from_slice(chunked.get(..size)?);
        chunked = chunked.get(size..)?.strip_prefix(b"\r\n")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chunked_body_is_read_whole_and_a_cut_one_is_not() {
        let chunked = b"5\r\nhello\r\n7;ext=1\r\n, world\r\n0\r\n\r\n";

        assert_eq!(unchunked(chunked).as_deref(), Some(&b"hello, world"[..]));
        assert_eq!(unchunked(b"5\r\nhel"), None);
    }
}
