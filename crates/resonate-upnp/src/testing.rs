use std::{
    io,
    net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket},
    sync::Arc,
};

use resonate_audio::{AudioSource, StreamClock};
use resonate_core::{ChannelLayout, Frames, SampleFormat, SampleRate, StreamSpec};
use socket2::{Domain, Protocol, Socket, Type};

use crate::{Carriage, Feed, Wrapping};

pub(crate) struct Silence;

impl AudioSource for Silence {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        dst.len()
    }
}

pub(crate) fn silent_feed() -> Arc<Feed> {
    let spec = StreamSpec::new(
        SampleRate::HZ_44100,
        ChannelLayout::Stereo,
        SampleFormat::S16,
    );
    let carriage = Carriage::new(
        Wrapping::Wave,
        vec![SampleRate::HZ_44100],
        vec![ChannelLayout::Stereo],
    );
    Arc::new(Feed::new(
        spec,
        carriage,
        Box::new(Silence),
        Arc::new(StreamClock::behind_by(Frames::ZERO)),
    ))
}

pub(crate) const ANOTHER_LOOPBACK: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);

fn bound_elsewhere(kind: Type, protocol: Protocol) -> Option<Socket> {
    let socket = Socket::new(Domain::IPV4, kind, Some(protocol)).expect("a socket");

    match socket.bind(&SocketAddr::new(ANOTHER_LOOPBACK.into(), 0).into()) {
        Ok(()) => Some(socket),
        Err(error) if error.kind() == io::ErrorKind::AddrNotAvailable => {
            eprintln!("skipped: {ANOTHER_LOOPBACK} is not an address of this machine");
            None
        }
        Err(error) => panic!("{ANOTHER_LOOPBACK} could not be bound: {error}"),
    }
}

pub(crate) fn connected_from_another_address(to: SocketAddr) -> Option<TcpStream> {
    let socket = bound_elsewhere(Type::STREAM, Protocol::TCP)?;

    socket.connect(&to.into()).expect("connects");
    Some(socket.into())
}

pub(crate) fn heard_on_another_address() -> Option<UdpSocket> {
    bound_elsewhere(Type::DGRAM, Protocol::UDP).map(UdpSocket::from)
}
