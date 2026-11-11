use std::{
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener},
    sync::Arc,
    thread,
    time::Duration,
};

use parking_lot::Mutex;

use crate::{Ranged, Trusted, configured};

pub(crate) const WHOLE_BODY: &[u8] =
    b"0123456789abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

pub(crate) const CUT_AT: usize = 20;

pub(crate) type Answer = Box<dyn Fn(&str) -> Vec<u8> + Send>;

pub(crate) fn answering(status: &str, headers: &[&str], body: &[u8], declared: usize) -> Vec<u8> {
    let mut answer =
        format!("HTTP/1.1 {status}\r\nContent-Length: {declared}\r\nConnection: close\r\n");
    for header in headers {
        answer.push_str(header);
        answer.push_str("\r\n");
    }
    answer.push_str("\r\n");
    let mut answer = answer.into_bytes();
    answer.extend_from_slice(body);
    answer
}

pub(crate) fn cut_first_answer(headers: &[&str]) -> Vec<u8> {
    answering("200 OK", headers, &WHOLE_BODY[..CUT_AT], WHOLE_BODY.len())
}

pub(crate) fn serving(answers: Vec<Answer>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("binds");
    let url = format!("http://{}/song", listener.local_addr().expect("an address"));
    let heads = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&heads);
    thread::spawn(move || {
        for answer in answers {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut head = Vec::new();
            let mut buffer = [0; 1024];
            while !head.ends_with(b"\r\n\r\n") {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => head.extend_from_slice(&buffer[..read]),
                }
            }
            let head = String::from_utf8_lossy(&head).into_owned();
            let reply = answer(&head);
            seen.lock().push(head);
            let _ = stream.write_all(&reply);
        }
    });
    (url, heads)
}

pub(crate) fn opened(url: &str) -> Ranged {
    let agent = configured(Duration::from_secs(5), Trusted::BuiltInRoots)
        .build()
        .new_agent();
    let response = agent.get(url).call().expect("the first answer");
    Ranged::continuing(agent, url.to_owned(), response, Duration::ZERO)
}

pub(crate) fn partial_from(at: usize, total: usize, headers: &[&str]) -> Vec<u8> {
    let range = format!("Content-Range: bytes {at}-{}/{total}", WHOLE_BODY.len() - 1);
    let mut all = vec![range.as_str()];
    all.extend_from_slice(headers);
    answering(
        "206 Partial Content",
        &all,
        &WHOLE_BODY[at..],
        WHOLE_BODY.len() - at,
    )
}

pub(crate) fn lower(head: &str) -> String {
    head.to_ascii_lowercase()
}
