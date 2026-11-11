use std::str;

use crate::{
    frame::{self, LENGTH_BYTES, Message, Payload},
    json::Json,
};

pub fn read_what_a_receiver_says(bytes: &[u8]) {
    if let Ok(Some(length)) = frame::whole(bytes)
        && let Some(body) = bytes.get(LENGTH_BYTES..LENGTH_BYTES + length)
    {
        let _ = Message::read(body);
    }

    if let Ok(message) = Message::read(bytes) {
        let framed = message.framed();
        assert_eq!(
            frame::whole(&framed).ok().flatten(),
            Some(framed.len() - LENGTH_BYTES)
        );
        assert_eq!(
            Message::read(&framed[LENGTH_BYTES..]).ok(),
            Some(message.clone())
        );
        if let Payload::Text(text) = &message.payload {
            let _ = Json::read(text);
        }
    }

    if let Ok(text) = str::from_utf8(bytes)
        && let Some(json) = Json::read(text)
    {
        let _ = json.kind();
        let _ = json.get("status").and_then(Json::as_list);
        let _ = json.written();
    }
}
