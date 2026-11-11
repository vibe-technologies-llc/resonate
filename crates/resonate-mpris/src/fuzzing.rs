use std::io::Cursor;

use crate::wire::{Reply, Request, read_frame, write_frame};

pub fn read_what_a_peer_says(bytes: &[u8]) {
    let mut stream = Cursor::new(bytes);
    while let Ok(Some(body)) = read_frame(&mut stream) {
        let _ = Request::decode(&body);
        let _ = Reply::decode(&body);
    }

    if let Ok(request) = Request::decode(bytes) {
        let encoded = request.encode();
        let again = Request::decode(&encoded).expect("a request reads back");
        assert_eq!(again.encode(), encoded);
        assert_eq!(again.call(), request.call());
    }

    if let Ok(reply) = Reply::decode(bytes) {
        let encoded = reply.encode();
        let again = Reply::decode(&encoded).expect("a reply reads back");
        assert_eq!(again.encode(), encoded);

        let mut framed = Vec::new();
        write_frame(&mut framed, &encoded).expect("a frame within the limit is written");
        let read = read_frame(&mut Cursor::new(framed)).expect("a written frame is read");
        assert_eq!(read.as_deref(), Some(encoded.as_slice()));
    }
}
