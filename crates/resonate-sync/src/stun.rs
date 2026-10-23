use std::{
    fmt,
    io::{self, IoSliceMut},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, ready},
    time::Duration,
};

use parking_lot::Mutex;
use quinn::{
    AsyncUdpSocket, UdpPoller,
    udp::{RecvMeta, Transmit},
};
use tokio::{sync::oneshot, time::timeout};

use crate::{Error, Result, key::random_bytes};

pub const STUN_PORT: u16 = 3478;
const MAGIC_COOKIE: [u8; 4] = [0x21, 0x12, 0xA4, 0x42];
const BINDING_REQUEST: [u8; 2] = [0x00, 0x01];
const BINDING_SUCCESS: u16 = 0x0101;
const MAPPED_ADDRESS: u16 = 0x0001;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;
const HEADER_BYTES: usize = 20;
const TRANSACTION_BYTES: usize = 12;
const ATTRIBUTE_HEADER_BYTES: usize = 4;
const FAMILY_V4: u8 = 1;
const FAMILY_V6: u8 = 2;
const ATTEMPTS: usize = 3;
const RETRY_AFTER: Duration = Duration::from_millis(700);
const TWO_TOP_BITS: u8 = 0xC0;

type Transaction = [u8; TRANSACTION_BYTES];

pub struct Reflector {
    inner: Arc<dyn AsyncUdpSocket>,
    asked: Mutex<Vec<(Transaction, oneshot::Sender<SocketAddr>)>>,
}

impl fmt::Debug for Reflector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Reflector")
            .field("asked", &self.asked.lock().len())
            .finish_non_exhaustive()
    }
}

impl Reflector {
    pub fn over(inner: Arc<dyn AsyncUdpSocket>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            asked: Mutex::new(Vec::new()),
        })
    }

    pub async fn ask(&self, server: SocketAddr) -> Result<SocketAddr> {
        let transaction = random_bytes::<TRANSACTION_BYTES>()?;
        let (answer, mut answered) = oneshot::channel();
        self.asked.lock().push((transaction, answer));
        let request = binding_request(&transaction);
        for _ in 0..ATTEMPTS {
            let sent = self.inner.try_send(&Transmit {
                destination: server,
                ecn: None,
                contents: &request,
                segment_size: None,
                src_ip: None,
            });
            if let Err(error) = sent
                && error.kind() != io::ErrorKind::WouldBlock
            {
                tracing::debug!(%error, "a STUN request could not be sent");
            }
            if let Ok(Ok(public)) = timeout(RETRY_AFTER, &mut answered).await {
                return Ok(public);
            }
        }
        self.asked.lock().retain(|(asked, _)| *asked != transaction);
        Err(Error::StunSilent)
    }

    fn taken(&self, datagram: &[u8]) -> bool {
        let Some((transaction, public)) = binding_answer(datagram) else {
            return looks_like_stun(datagram);
        };
        let mut asked = self.asked.lock();
        if let Some(position) = asked.iter().position(|(asked, _)| *asked == transaction) {
            let (_, answer) = asked.swap_remove(position);
            let _ = answer.send(public);
        }
        true
    }
}

impl AsyncUdpSocket for Reflector {
    fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
        Arc::clone(&self.inner).create_io_poller()
    }

    fn try_send(&self, transmit: &Transmit) -> io::Result<()> {
        self.inner.try_send(transmit)
    }

    fn poll_recv(
        &self,
        cx: &mut Context,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Poll<io::Result<usize>> {
        loop {
            let received = ready!(self.inner.poll_recv(cx, bufs, meta))?;
            let mut kept = 0;
            for index in 0..received {
                let length = meta[index].len.min(bufs[index].len());
                if self.taken(&bufs[index][..length]) {
                    continue;
                }
                if kept != index {
                    let (before, after) = bufs.split_at_mut(index);
                    before[kept][..length].copy_from_slice(&after[0][..length]);
                    meta[kept] = meta[index];
                }
                kept += 1;
            }
            if kept > 0 {
                return Poll::Ready(Ok(kept));
            }
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    fn max_transmit_segments(&self) -> usize {
        self.inner.max_transmit_segments()
    }

    fn max_receive_segments(&self) -> usize {
        self.inner.max_receive_segments()
    }

    fn may_fragment(&self) -> bool {
        self.inner.may_fragment()
    }
}

fn binding_request(transaction: &Transaction) -> [u8; HEADER_BYTES] {
    let mut request = [0; HEADER_BYTES];
    request[..2].copy_from_slice(&BINDING_REQUEST);
    request[4..8].copy_from_slice(&MAGIC_COOKIE);
    request[8..].copy_from_slice(transaction);
    request
}

fn looks_like_stun(datagram: &[u8]) -> bool {
    datagram.len() >= HEADER_BYTES
        && datagram[0] & TWO_TOP_BITS == 0
        && datagram[4..8] == MAGIC_COOKIE
}

fn binding_answer(datagram: &[u8]) -> Option<(Transaction, SocketAddr)> {
    if !looks_like_stun(datagram) {
        return None;
    }
    let kind = u16::from_be_bytes([datagram[0], datagram[1]]);
    let declared = usize::from(u16::from_be_bytes([datagram[2], datagram[3]]));
    if kind != BINDING_SUCCESS || HEADER_BYTES + declared > datagram.len() {
        return None;
    }
    let mut transaction = [0; TRANSACTION_BYTES];
    transaction.copy_from_slice(&datagram[8..HEADER_BYTES]);
    let mut attributes = &datagram[HEADER_BYTES..HEADER_BYTES + declared];
    let mut plain = None;
    while attributes.len() >= ATTRIBUTE_HEADER_BYTES {
        let kind = u16::from_be_bytes([attributes[0], attributes[1]]);
        let length = usize::from(u16::from_be_bytes([attributes[2], attributes[3]]));
        let padded = length.next_multiple_of(4);
        let value = attributes.get(ATTRIBUTE_HEADER_BYTES..ATTRIBUTE_HEADER_BYTES + length)?;
        match kind {
            XOR_MAPPED_ADDRESS => {
                return address_of(value, Some(&transaction)).map(|a| (transaction, a));
            }
            MAPPED_ADDRESS => plain = address_of(value, None),
            _ => {}
        }
        attributes = attributes.get(ATTRIBUTE_HEADER_BYTES + padded..)?;
    }
    plain.map(|address| (transaction, address))
}

fn address_of(value: &[u8], xored_with: Option<&Transaction>) -> Option<SocketAddr> {
    let family = *value.get(1)?;
    let mut port = u16::from_be_bytes([*value.get(2)?, *value.get(3)?]);
    if xored_with.is_some() {
        port ^= u16::from_be_bytes([MAGIC_COOKIE[0], MAGIC_COOKIE[1]]);
    }
    let mask = |index: usize| -> u8 {
        match xored_with {
            None => 0,
            Some(_) if index < MAGIC_COOKIE.len() => MAGIC_COOKIE[index],
            Some(transaction) => transaction[index - MAGIC_COOKIE.len()],
        }
    };
    let ip = match family {
        FAMILY_V4 => {
            let mut octets: [u8; 4] = value.get(4..8)?.try_into().ok()?;
            for (index, octet) in octets.iter_mut().enumerate() {
                *octet ^= mask(index);
            }
            IpAddr::V4(Ipv4Addr::from(octets))
        }
        FAMILY_V6 => {
            let mut octets: [u8; 16] = value.get(4..20)?.try_into().ok()?;
            for (index, octet) in octets.iter_mut().enumerate() {
                *octet ^= mask(index);
            }
            IpAddr::V6(Ipv6Addr::from(octets))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

#[cfg(test)]
mod tests {
    use quinn::{Endpoint, EndpointConfig, Runtime, TokioRuntime};
    use tokio::net::UdpSocket;

    use super::*;

    fn answer_to(request: &[u8], public: SocketAddrV4Parts) -> Vec<u8> {
        let mut attribute = vec![0, FAMILY_V4];
        attribute.extend((public.port ^ 0x2112).to_be_bytes());
        for (octet, mask) in public.octets.iter().zip(MAGIC_COOKIE) {
            attribute.push(octet ^ mask);
        }
        let mut answer = Vec::new();
        answer.extend(BINDING_SUCCESS.to_be_bytes());
        answer.extend(
            u16::try_from(ATTRIBUTE_HEADER_BYTES + attribute.len())
                .unwrap()
                .to_be_bytes(),
        );
        answer.extend(&request[4..HEADER_BYTES]);
        answer.extend(XOR_MAPPED_ADDRESS.to_be_bytes());
        answer.extend(u16::try_from(attribute.len()).unwrap().to_be_bytes());
        answer.extend(attribute);
        answer
    }

    struct SocketAddrV4Parts {
        octets: [u8; 4],
        port: u16,
    }

    #[test]
    fn an_xor_mapped_address_reads_back_as_the_address_it_hides() {
        let transaction = [7; TRANSACTION_BYTES];
        let request = binding_request(&transaction);
        let answer = answer_to(
            &request,
            SocketAddrV4Parts {
                octets: [203, 0, 113, 9],
                port: 40000,
            },
        );

        let (read, public) = binding_answer(&answer).expect("an answer");

        assert_eq!(read, transaction);
        assert_eq!(public, "203.0.113.9:40000".parse().expect("address"));
    }

    #[test]
    fn a_datagram_that_is_not_an_answer_is_not_read_as_one() {
        let transaction = [7; TRANSACTION_BYTES];

        assert!(binding_answer(&binding_request(&transaction)).is_none());
        assert!(binding_answer(&[0x40; 40]).is_none());
        assert!(binding_answer(&[]).is_none());
    }

    #[tokio::test]
    async fn a_device_learns_the_address_a_stun_server_saw_it_at_and_quic_never_sees_the_exchange()
    {
        let stun = UdpSocket::bind("127.0.0.1:0").await.expect("a stun socket");
        let stun_at = stun.local_addr().expect("address");
        tokio::spawn(async move {
            let mut buffer = [0; 64];
            loop {
                let Ok((length, from)) = stun.recv_from(&mut buffer).await else {
                    return;
                };
                let std::net::IpAddr::V4(ip) = from.ip() else {
                    return;
                };
                let answer = answer_to(
                    &buffer[..length],
                    SocketAddrV4Parts {
                        octets: ip.octets(),
                        port: from.port(),
                    },
                );
                let _ = stun.send_to(&answer, from).await;
            }
        });
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("a socket");
        let own = socket.local_addr().expect("address");
        let wrapped = TokioRuntime.wrap_udp_socket(socket).expect("wrapped");
        let reflector = Reflector::over(wrapped);
        let _endpoint = Endpoint::new_with_abstract_socket(
            EndpointConfig::default(),
            None,
            reflector.clone(),
            Arc::new(TokioRuntime),
        )
        .expect("an endpoint");

        let public = reflector.ask(stun_at).await.expect("an address");

        assert_eq!(public, own);
    }

    #[tokio::test]
    async fn a_stun_server_that_never_answers_is_a_typed_silence() {
        let silent = UdpSocket::bind("127.0.0.1:0").await.expect("a socket");
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("a socket");
        let reflector = Reflector::over(TokioRuntime.wrap_udp_socket(socket).expect("wrapped"));
        let _endpoint = Endpoint::new_with_abstract_socket(
            EndpointConfig::default(),
            None,
            reflector.clone(),
            Arc::new(TokioRuntime),
        )
        .expect("an endpoint");

        let asked = reflector.ask(silent.local_addr().expect("address")).await;

        assert!(matches!(asked, Err(Error::StunSilent)));
    }
}
