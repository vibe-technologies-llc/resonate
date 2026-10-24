use std::{
    collections::{BTreeMap, VecDeque},
    io::IoSliceMut,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    task::{Context, Waker},
};

use parking_lot::Mutex;
use quinn::udp::{RecvMeta, Transmit};
use tokio::sync::mpsc::Sender;

use crate::{
    key::DeviceId,
    wire::{LARGEST_DATAGRAM, ToServer},
};

pub const RELAYED_PORT: u16 = 443;
const RELAYED_NETWORK: [u8; 2] = [240, 82];
const ARRIVED_AT_MOST: usize = 256;

#[derive(Default)]
struct State {
    addresses: BTreeMap<DeviceId, SocketAddr>,
    devices: BTreeMap<SocketAddr, DeviceId>,
    out: Option<Sender<Vec<u8>>>,
    arrived: VecDeque<(DeviceId, Vec<u8>)>,
    waker: Option<Waker>,
}

#[derive(Default)]
pub struct Relay {
    state: Mutex<State>,
}

fn canonical(address: SocketAddr) -> SocketAddr {
    SocketAddr::new(address.ip().to_canonical(), address.port())
}

fn synthetic(number: u16) -> SocketAddr {
    let [high, low] = number.to_be_bytes();
    SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(
            RELAYED_NETWORK[0],
            RELAYED_NETWORK[1],
            high,
            low,
        )),
        RELAYED_PORT,
    )
}

impl State {
    fn address_of(&mut self, device: DeviceId) -> SocketAddr {
        if let Some(address) = self.addresses.get(&device) {
            return *address;
        }
        let number = u16::try_from(self.addresses.len() + 1).unwrap_or(u16::MAX);
        let address = synthetic(number);
        self.addresses.insert(device, address);
        self.devices.insert(address, device);
        address
    }
}

impl Relay {
    pub fn address_of(&self, device: DeviceId) -> SocketAddr {
        self.state.lock().address_of(device)
    }

    pub fn device_at(&self, address: SocketAddr) -> Option<DeviceId> {
        self.state.lock().devices.get(&canonical(address)).copied()
    }

    pub fn attach(&self, out: Sender<Vec<u8>>) {
        self.state.lock().out = Some(out);
    }

    pub fn detach(&self) {
        let mut state = self.state.lock();
        state.out = None;
        state.arrived.clear();
    }

    pub fn is_attached(&self) -> bool {
        self.state.lock().out.is_some()
    }

    pub fn carry(&self, device: DeviceId, transmit: &Transmit) {
        let size = transmit
            .segment_size
            .unwrap_or(transmit.contents.len())
            .max(1);
        let state = self.state.lock();
        let Some(out) = state.out.as_ref() else {
            return;
        };
        for datagram in transmit.contents.chunks(size) {
            if datagram.len() > LARGEST_DATAGRAM {
                continue;
            }
            let frame = ToServer::Relay {
                to: device,
                datagram: datagram.to_vec(),
            };
            let _ = out.try_send(frame.encoded());
        }
    }

    pub fn arrived(&self, from: DeviceId, datagram: Vec<u8>) {
        let waker = {
            let mut state = self.state.lock();
            if state.arrived.len() >= ARRIVED_AT_MOST {
                return;
            }
            state.address_of(from);
            state.arrived.push_back((from, datagram));
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    pub fn drain(
        &self,
        cx: &mut Context,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
        local_is_ipv6: bool,
    ) -> usize {
        let mut state = self.state.lock();
        let mut filled = 0;
        while filled < bufs.len().min(meta.len()) {
            let Some((from, datagram)) = state.arrived.pop_front() else {
                break;
            };
            let Some(address) = state.addresses.get(&from).copied() else {
                continue;
            };
            let length = datagram.len().min(bufs[filled].len());
            bufs[filled][..length].copy_from_slice(&datagram[..length]);
            let address = match address.ip() {
                IpAddr::V4(ip) if local_is_ipv6 => {
                    SocketAddr::new(IpAddr::V6(ip.to_ipv6_mapped()), address.port())
                }
                _ => address,
            };
            meta[filled] = RecvMeta {
                addr: address,
                len: length,
                stride: length,
                ..RecvMeta::default()
            };
            filled += 1;
        }
        if filled == 0 {
            state.waker = Some(cx.waker().clone());
        }
        filled
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use quinn::{AsyncUdpSocket, Endpoint, EndpointConfig, Runtime, TokioRuntime};
    use tokio::time::timeout;

    use super::*;
    use crate::{
        SyncKey,
        peer::dialled,
        quic::{PEER_NAME, PeerIdentity},
        service::peer_socket,
        stun::Reflector,
    };

    const ONE: DeviceId = DeviceId([1; 16]);
    const TWO: DeviceId = DeviceId([2; 16]);

    #[test]
    fn each_device_has_one_address_of_its_own_that_reads_back_as_the_device() {
        let relay = Relay::default();

        let one = relay.address_of(ONE);
        let two = relay.address_of(TWO);

        assert_ne!(one, two);
        assert_eq!(relay.address_of(ONE), one);
        assert_eq!(relay.device_at(one), Some(ONE));
        assert_eq!(relay.device_at(two), Some(TWO));
        assert!(one.ip().is_ipv4());
    }

    #[test]
    fn an_address_is_read_back_whether_it_is_plain_or_wrapped_in_six() {
        let relay = Relay::default();
        let one = relay.address_of(ONE);
        let IpAddr::V4(plain) = one.ip() else {
            panic!("a plain address");
        };
        let wrapped = SocketAddr::new(IpAddr::V6(plain.to_ipv6_mapped()), one.port());

        assert_eq!(relay.device_at(wrapped), Some(ONE));
        assert_eq!(
            relay.device_at("203.0.113.9:443".parse().expect("address")),
            None
        );
    }

    #[test]
    fn a_datagram_is_carried_as_a_frame_to_the_server_while_a_link_is_attached_and_dropped_after() {
        let relay = Relay::default();
        let (out, mut carried) = tokio::sync::mpsc::channel(4);
        let transmit = Transmit {
            destination: relay.address_of(TWO),
            ecn: None,
            contents: &[1, 2, 3, 4, 5, 6],
            segment_size: Some(4),
            src_ip: None,
        };

        relay.carry(TWO, &transmit);
        relay.attach(out);
        relay.carry(TWO, &transmit);
        relay.detach();
        relay.carry(TWO, &transmit);

        let first = carried.try_recv().expect("a frame");
        let second = carried.try_recv().expect("a frame");
        assert_eq!(
            ToServer::decoded(&first).expect("decoded"),
            ToServer::Relay {
                to: TWO,
                datagram: vec![1, 2, 3, 4]
            }
        );
        assert_eq!(
            ToServer::decoded(&second).expect("decoded"),
            ToServer::Relay {
                to: TWO,
                datagram: vec![5, 6]
            }
        );
        assert!(carried.try_recv().is_err());
    }

    #[test]
    fn a_full_queue_drops_what_does_not_fit_instead_of_waiting() {
        let relay = Relay::default();
        let (out, mut carried) = tokio::sync::mpsc::channel(1);
        relay.attach(out);
        let transmit = Transmit {
            destination: relay.address_of(ONE),
            ecn: None,
            contents: &[9; 10],
            segment_size: Some(5),
            src_ip: None,
        };

        relay.carry(ONE, &transmit);

        assert!(carried.try_recv().is_ok());
        assert!(carried.try_recv().is_err());
    }

    fn endpoint_over(identity: &PeerIdentity) -> (Endpoint, Arc<Reflector>) {
        let socket = peer_socket().expect("a socket");
        let reflector = Reflector::over(TokioRuntime.wrap_udp_socket(socket).expect("wrapped"));
        let endpoint = Endpoint::new_with_abstract_socket(
            EndpointConfig::default(),
            Some(identity.serving().expect("serving")),
            Arc::clone(&reflector) as Arc<dyn AsyncUdpSocket>,
            Arc::new(TokioRuntime),
        )
        .expect("an endpoint");
        (endpoint, reflector)
    }

    fn pumped(from: DeviceId, to: Arc<Reflector>) -> Sender<Vec<u8>> {
        let (out, mut carried) = tokio::sync::mpsc::channel::<Vec<u8>>(256);
        tokio::spawn(async move {
            while let Some(frame) = carried.recv().await {
                if let Ok(ToServer::Relay { datagram, .. }) = ToServer::decoded(&frame) {
                    to.relay().arrived(from, datagram);
                }
            }
        });
        out
    }

    #[tokio::test]
    async fn two_endpoints_reach_each_other_over_relayed_datagrams_alone_and_pass_a_song_between_them()
     {
        let derived = SyncKey::minted()
            .expect("a key")
            .derived()
            .expect("derived");
        let identity = PeerIdentity::of(&derived).expect("an identity");
        let (desk, desk_reflector) = endpoint_over(&identity);
        let (phone, phone_reflector) = endpoint_over(&identity);
        desk_reflector
            .relay()
            .attach(pumped(ONE, Arc::clone(&phone_reflector)));
        phone_reflector
            .relay()
            .attach(pumped(TWO, Arc::clone(&desk_reflector)));
        let song = (0..3_000_000_u32).map(|at| at as u8).collect::<Vec<_>>();
        let sent = song.clone();
        tokio::spawn(async move {
            let incoming = phone.accept().await.expect("an incoming");
            let connection = incoming.await.expect("accepted");
            let (mut send, mut recv) = connection.accept_bi().await.expect("a stream");
            let mut asked = [0_u8; 4];
            recv.read_exact(&mut asked).await.expect("asked");
            send.write_all(&sent).await.expect("sent");
            send.finish().expect("finished");
            let _ = connection.closed().await;
        });

        let connection = dialled(
            &desk,
            &identity.dialling().expect("dialling"),
            &[desk_reflector.relay().address_of(TWO)],
            TWO,
        )
        .await
        .expect("reached through the relay");
        let (mut send, mut recv) = connection.open_bi().await.expect("a stream");
        send.write_all(b"song").await.expect("asked");
        send.finish().expect("finished");
        let received = timeout(Duration::from_secs(30), recv.read_to_end(10_000_000))
            .await
            .expect("in time")
            .expect("read");

        assert_eq!(received, song);
        assert_eq!(PEER_NAME, "resonate-peer");
    }

    #[tokio::test]
    async fn a_dual_stack_endpoint_is_dialled_over_ipv6_and_over_ipv4_alike() {
        let derived = SyncKey::minted()
            .expect("a key")
            .derived()
            .expect("derived");
        let identity = PeerIdentity::of(&derived).expect("an identity");
        let (served, _) = endpoint_over(&identity);
        let (dialling, _) = endpoint_over(&identity);
        if !served.local_addr().expect("an address").is_ipv6() {
            return;
        }
        let port = served.local_addr().expect("an address").port();
        tokio::spawn(async move {
            while let Some(incoming) = served.accept().await {
                tokio::spawn(async move {
                    if let Ok(connection) = incoming.await {
                        let _ = connection.closed().await;
                    }
                });
            }
        });
        let config = identity.dialling().expect("dialling");

        let over_six = dialled(
            &dialling,
            &config,
            &[SocketAddr::new(
                IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
                port,
            )],
            ONE,
        )
        .await;
        let over_four = dialled(
            &dialling,
            &config,
            &[SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)],
            ONE,
        )
        .await;

        assert!(over_six.is_ok());
        assert!(over_four.is_ok());
    }
}
