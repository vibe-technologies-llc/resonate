use std::io::{Read, Write};

use crate::{Error, IpcOp, Result};

pub(crate) const LARGEST_FRAME: u32 = 64 * 1024;
const HEADER: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Opcode {
    Handshake,
    Frame,
    Close,
    Ping,
    Pong,
}

impl Opcode {
    const fn code(self) -> u32 {
        match self {
            Self::Handshake => 0,
            Self::Frame => 1,
            Self::Close => 2,
            Self::Ping => 3,
            Self::Pong => 4,
        }
    }

    const fn of(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::Handshake),
            1 => Some(Self::Frame),
            2 => Some(Self::Close),
            3 => Some(Self::Ping),
            4 => Some(Self::Pong),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) opcode: Opcode,
    pub(crate) body: Vec<u8>,
}

pub(crate) fn write_frame(out: &mut impl Write, opcode: Opcode, body: &[u8]) -> Result<()> {
    let length = u32::try_from(body.len())
        .ok()
        .filter(|length| *length <= LARGEST_FRAME)
        .ok_or(Error::Oversized {
            length: u32::try_from(body.len()).unwrap_or(u32::MAX),
        })?;
    let mut framed = Vec::with_capacity(HEADER + body.len());
    framed.extend_from_slice(&opcode.code().to_le_bytes());
    framed.extend_from_slice(&length.to_le_bytes());
    framed.extend_from_slice(body);
    out.write_all(&framed)
        .and_then(|()| out.flush())
        .map_err(|source| Error::Socket {
            op: IpcOp::Write,
            source,
        })
}

pub(crate) fn read_frame(input: &mut impl Read) -> Result<Frame> {
    let mut header = [0; HEADER];
    read_exactly(input, &mut header)?;
    let [a, b, c, d, e, f, g, h] = header;
    let code = u32::from_le_bytes([a, b, c, d]);
    let length = u32::from_le_bytes([e, f, g, h]);

    let opcode = Opcode::of(code).ok_or(Error::Unexpected { opcode: code })?;
    if length > LARGEST_FRAME {
        return Err(Error::Oversized { length });
    }
    let mut body = vec![0; length as usize];
    read_exactly(input, &mut body)?;

    Ok(Frame { opcode, body })
}

fn read_exactly(input: &mut impl Read, into: &mut [u8]) -> Result<()> {
    input.read_exact(into).map_err(|source| Error::Socket {
        op: IpcOp::Read,
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn a_frame_reads_back_as_it_was_written() {
        let mut wire = Vec::new();
        write_frame(&mut wire, Opcode::Frame, br#"{"cmd":"SET_ACTIVITY"}"#).expect("written");

        assert_eq!(&wire[..4], &1_u32.to_le_bytes());
        assert_eq!(&wire[4..8], &22_u32.to_le_bytes());
        let frame = read_frame(&mut Cursor::new(wire)).expect("read");
        assert_eq!(frame.opcode, Opcode::Frame);
        assert_eq!(frame.body, br#"{"cmd":"SET_ACTIVITY"}"#);
    }

    #[test]
    fn a_frame_claiming_more_than_one_may_hold_is_refused_before_it_is_read() {
        let mut wire = Vec::new();
        wire.extend_from_slice(&1_u32.to_le_bytes());
        wire.extend_from_slice(&(LARGEST_FRAME + 1).to_le_bytes());

        let refused = read_frame(&mut Cursor::new(wire));
        assert!(matches!(refused, Err(Error::Oversized { length }) if length == LARGEST_FRAME + 1));
    }

    #[test]
    fn a_frame_under_an_opcode_nobody_speaks_is_refused() {
        let mut wire = Vec::new();
        wire.extend_from_slice(&9_u32.to_le_bytes());
        wire.extend_from_slice(&0_u32.to_le_bytes());

        let refused = read_frame(&mut Cursor::new(wire));
        assert!(matches!(refused, Err(Error::Unexpected { opcode: 9 })));
    }

    #[test]
    fn a_body_too_large_to_send_is_refused_before_anything_is_written() {
        let mut wire = Vec::new();
        let body = vec![b'x'; LARGEST_FRAME as usize + 1];

        let refused = write_frame(&mut wire, Opcode::Frame, &body);
        assert!(matches!(refused, Err(Error::Oversized { .. })));
        assert!(wire.is_empty());
    }

    #[test]
    fn a_frame_cut_short_is_a_failed_read() {
        let mut wire = Vec::new();
        write_frame(&mut wire, Opcode::Ping, b"{}").expect("written");
        wire.pop();

        let refused = read_frame(&mut Cursor::new(wire));
        assert!(matches!(
            refused,
            Err(Error::Socket {
                op: IpcOp::Read,
                ..
            })
        ));
    }
}
