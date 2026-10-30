use crate::{Error, Result};

pub(crate) const LENGTH_BYTES: usize = 4;

pub(crate) const MESSAGE_BYTES_AT_MOST: usize = 64 * 1024;

const CASTV2_1_0: u64 = 0;

const PAYLOAD_TEXT: u64 = 0;

const PAYLOAD_BINARY: u64 = 1;

const VARINT: u8 = 0;

const FIXED_EIGHT: u8 = 1;

const DELIMITED: u8 = 2;

const FIXED_FOUR: u8 = 5;

const PROTOCOL_VERSION: u64 = 1;

const SOURCE_ID: u64 = 2;

const DESTINATION_ID: u64 = 3;

const NAMESPACE: u64 = 4;

const PAYLOAD_TYPE: u64 = 5;

const PAYLOAD_UTF8: u64 = 6;

const PAYLOAD_BINARY_FIELD: u64 = 7;

const SEVEN_BITS: u64 = 0x7F;

const MORE: u8 = 0x80;

const VARINT_BYTES_AT_MOST: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Payload {
    Text(String),
    Binary(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Message {
    pub(crate) source: String,
    pub(crate) destination: String,
    pub(crate) namespace: String,
    pub(crate) payload: Payload,
}

impl Message {
    pub(crate) fn framed(&self) -> Vec<u8> {
        let mut body = Vec::new();
        varint_field(&mut body, PROTOCOL_VERSION, CASTV2_1_0);
        delimited_field(&mut body, SOURCE_ID, self.source.as_bytes());
        delimited_field(&mut body, DESTINATION_ID, self.destination.as_bytes());
        delimited_field(&mut body, NAMESPACE, self.namespace.as_bytes());
        match &self.payload {
            Payload::Text(text) => {
                varint_field(&mut body, PAYLOAD_TYPE, PAYLOAD_TEXT);
                delimited_field(&mut body, PAYLOAD_UTF8, text.as_bytes());
            }
            Payload::Binary(bytes) => {
                varint_field(&mut body, PAYLOAD_TYPE, PAYLOAD_BINARY);
                delimited_field(&mut body, PAYLOAD_BINARY_FIELD, bytes);
            }
        }

        let mut framed = u32::try_from(body.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes()
            .to_vec();
        framed.extend_from_slice(&body);
        framed
    }

    pub(crate) fn read(body: &[u8]) -> Result<Self> {
        let mut source = None;
        let mut destination = None;
        let mut namespace = None;
        let mut text = None;
        let mut binary = None;
        let mut binary_named = false;

        let mut at = 0;
        while at < body.len() {
            let key = varint(body, &mut at)?;
            let field = key >> 3;
            let wire = u8::try_from(key & 0x7).map_err(|_| Error::NotACastMessage)?;
            match wire {
                VARINT => {
                    let value = varint(body, &mut at)?;
                    if field == PAYLOAD_TYPE {
                        binary_named = value == PAYLOAD_BINARY;
                    }
                }
                DELIMITED => {
                    let length = usize::try_from(varint(body, &mut at)?)
                        .map_err(|_| Error::NotACastMessage)?;
                    let end = at.checked_add(length).ok_or(Error::NotACastMessage)?;
                    let bytes = body.get(at..end).ok_or(Error::NotACastMessage)?;
                    at = end;
                    let text_of =
                        || String::from_utf8(bytes.to_vec()).map_err(|_| Error::NotACastMessage);
                    match field {
                        SOURCE_ID => source = Some(text_of()?),
                        DESTINATION_ID => destination = Some(text_of()?),
                        NAMESPACE => namespace = Some(text_of()?),
                        PAYLOAD_UTF8 => text = Some(text_of()?),
                        PAYLOAD_BINARY_FIELD => binary = Some(bytes.to_vec()),
                        _ => {}
                    }
                }
                FIXED_EIGHT | FIXED_FOUR => {
                    let width = if wire == FIXED_EIGHT { 8 } else { 4 };
                    at = at
                        .checked_add(width)
                        .filter(|end| *end <= body.len())
                        .ok_or(Error::NotACastMessage)?;
                }
                _ => return Err(Error::NotACastMessage),
            }
        }

        let payload = match (binary_named, text, binary) {
            (false, Some(text), _) => Payload::Text(text),
            (true, _, Some(binary)) => Payload::Binary(binary),
            _ => return Err(Error::NotACastMessage),
        };
        Ok(Self {
            source: source.ok_or(Error::NotACastMessage)?,
            destination: destination.ok_or(Error::NotACastMessage)?,
            namespace: namespace.ok_or(Error::NotACastMessage)?,
            payload,
        })
    }
}

pub(crate) fn whole(held: &[u8]) -> Result<Option<usize>> {
    let Some(length) = held.get(..LENGTH_BYTES) else {
        return Ok(None);
    };
    let length = u32::from_be_bytes([length[0], length[1], length[2], length[3]]) as usize;
    if length > MESSAGE_BYTES_AT_MOST {
        return Err(Error::TooLong { bytes: length });
    }
    Ok((held.len() >= LENGTH_BYTES + length).then_some(length))
}

fn varint_field(body: &mut Vec<u8>, field: u64, value: u64) {
    put_varint(body, field << 3 | u64::from(VARINT));
    put_varint(body, value);
}

fn delimited_field(body: &mut Vec<u8>, field: u64, bytes: &[u8]) {
    put_varint(body, field << 3 | u64::from(DELIMITED));
    put_varint(body, bytes.len() as u64);
    body.extend_from_slice(bytes);
}

fn put_varint(body: &mut Vec<u8>, mut value: u64) {
    while value > SEVEN_BITS {
        body.push((value & SEVEN_BITS) as u8 | MORE);
        value >>= 7;
    }
    body.push(value as u8);
}

fn varint(body: &[u8], at: &mut usize) -> Result<u64> {
    let mut value = 0_u64;
    for read in 0..VARINT_BYTES_AT_MOST {
        let byte = *body.get(*at).ok_or(Error::NotACastMessage)?;
        *at += 1;
        value |= (u64::from(byte) & SEVEN_BITS) << (7 * read);
        if byte & MORE == 0 {
            return Ok(value);
        }
    }
    Err(Error::NotACastMessage)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(payload: Payload) -> Message {
        Message {
            source: "sender-0".to_owned(),
            destination: "receiver-0".to_owned(),
            namespace: "urn:x-cast:com.google.cast.tp.heartbeat".to_owned(),
            payload,
        }
    }

    #[test]
    fn a_message_framed_reads_back_as_itself_whether_text_or_binary() {
        let text = message(Payload::Text("{\"type\":\"PING\"}".repeat(20)));
        let binary = message(Payload::Binary(vec![0, 1, 2, 255]));

        for sent in [text, binary] {
            let framed = sent.framed();
            let length = whole(&framed).expect("a length").expect("a whole message");

            assert_eq!(length, framed.len() - LENGTH_BYTES);
            assert_eq!(Message::read(&framed[LENGTH_BYTES..]).expect("reads"), sent);
            assert_eq!(whole(&framed[..framed.len() - 1]).expect("a length"), None);
        }
    }

    #[test]
    fn the_wire_is_the_protobuf_cast_message_byte_for_byte() {
        let framed = Message {
            source: "s".to_owned(),
            destination: "d".to_owned(),
            namespace: "n".to_owned(),
            payload: Payload::Text("p".to_owned()),
        }
        .framed();

        assert_eq!(
            framed,
            [
                0, 0, 0, 16, 0x08, 0, 0x12, 1, b's', 0x1A, 1, b'd', 0x22, 1, b'n', 0x28, 0, 0x32,
                1, b'p'
            ]
        );
    }

    #[test]
    fn fields_it_does_not_know_are_stepped_over_and_a_cut_message_is_refused() {
        let mut body = message(Payload::Text("{}".to_owned())).framed()[LENGTH_BYTES..].to_vec();
        body.extend_from_slice(&[0x4D, 1, 2, 3, 4, 0x51, 1, 2, 3, 4, 5, 6, 7, 8]);

        assert!(Message::read(&body).is_ok());
        assert!(matches!(
            Message::read(&body[..body.len() - 3]),
            Err(Error::NotACastMessage)
        ));
        assert!(matches!(
            whole(&u32::MAX.to_be_bytes()),
            Err(Error::TooLong { .. })
        ));
    }
}
