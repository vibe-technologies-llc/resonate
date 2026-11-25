use std::{
    io::{Read, Seek, SeekFrom, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, UdpSocket},
    thread,
    time::Duration,
};

use resonate_upnp::{MediaServers, ObjectId, ServerFound, Udn};

const FOUR: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

const SIX: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);

const PRIVATE: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 255, 255, 1));

const PUBLIC: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));

const PUBLIC_V6: IpAddr = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));

const SHELF: &str = "uuid:5e1f0000-0000-4000-8000-0000000000aa";

const ANOTHER: &str = "uuid:5e1f0000-0000-4000-8000-0000000000bb";

const ANSWERED_WITHIN: Duration = Duration::from_millis(800);

const SONG_BYTES: usize = 1_000;

const CHUNK_BYTES: usize = 333;

const SERVER_TYPE: &str = "urn:schemas-upnp-org:device:MediaServer:1";

#[derive(Clone, Copy)]
enum Framed {
    Declared,
    Chunked,
}

struct Shape {
    described: Vec<(IpAddr, &'static str)>,
    located_on: IpAddr,
    usn: Option<&'static str>,
    song_on: IpAddr,
    framed: Framed,
}

impl Shape {
    fn plain() -> Self {
        Self {
            described: vec![(FOUR, SHELF)],
            located_on: FOUR,
            usn: Some(SHELF),
            song_on: FOUR,
            framed: Framed::Declared,
        }
    }
}

struct Fake {
    heard_at: SocketAddr,
}

fn song() -> Vec<u8> {
    (0..SONG_BYTES).map(|at| (at * 7 % 251) as u8).collect()
}

fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn description(udn: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?><root xmlns=\"urn:schemas-upnp-org:device-1-0\"><specVersion><major>1</major><minor>0</minor></specVersion><device><deviceType>{SERVER_TYPE}</deviceType><friendlyName>Shelf</friendlyName><UDN>{udn}</UDN><serviceList><service><serviceType>urn:schemas-upnp-org:service:ContentDirectory:1</serviceType><serviceId>urn:upnp-org:serviceId:ContentDirectory</serviceId><controlURL>/ctl</controlURL><eventSubURL>/evt</eventSubURL><SCPDURL>/cd.xml</SCPDURL></service></serviceList></device></root>"
    )
}

fn browsed(song_at: SocketAddr) -> String {
    let didl = format!(
        "<DIDL-Lite xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\"><item id=\"song\" parentID=\"0\" restricted=\"1\"><dc:title>Elsewhere</dc:title><upnp:class>object.item.audioItem.musicTrack</upnp:class><res protocolInfo=\"http-get:*:audio/flac:*\">http://{song_at}/song.flac</res></item></DIDL-Lite>"
    );
    format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:BrowseResponse xmlns:u=\"urn:schemas-upnp-org:service:ContentDirectory:1\"><Result>{}</Result><NumberReturned>1</NumberReturned><TotalMatches>1</TotalMatches><UpdateID>1</UpdateID></u:BrowseResponse></s:Body></s:Envelope>",
        escaped(&didl)
    )
}

fn head_and_body(stream: &mut TcpStream) -> Option<String> {
    let mut held = Vec::new();
    let mut buffer = [0; 4096];

    let ends = loop {
        if let Some(at) = held.windows(4).position(|window| window == b"\r\n\r\n") {
            break at + 4;
        }

        let read = stream.read(&mut buffer).ok()?;

        if read == 0 {
            return None;
        }
        held.extend_from_slice(&buffer[..read]);
    };
    let head = String::from_utf8_lossy(&held[..ends]).into_owned();
    let declared = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|length| length.trim().parse::<usize>().ok())
        })
        .unwrap_or_default();

    while held.len() < ends + declared {
        let read = stream.read(&mut buffer).ok()?;

        if read == 0 {
            break;
        }
        held.extend_from_slice(&buffer[..read]);
    }
    Some(head)
}

fn answered(stream: &mut TcpStream, kind: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

fn song_answered(stream: &mut TcpStream, framed: Framed) {
    let song = song();
    match framed {
        Framed::Declared => answered(stream, "audio/flac", &song),
        Framed::Chunked => {
            let mut written = b"HTTP/1.1 200 OK\r\nContent-Type: audio/flac\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();

            for chunk in song.chunks(CHUNK_BYTES) {
                written.extend_from_slice(format!("{:x};name=part\r\n", chunk.len()).as_bytes());
                written.extend_from_slice(chunk);
                written.extend_from_slice(b"\r\n");
            }
            written.extend_from_slice(b"0\r\n\r\n");

            let _ = stream.write_all(&written);
        }
    }
}

fn serve(listener: TcpListener, udn: &'static str, song_at: SocketAddr, framed: Framed) {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            continue;
        };
        let Some(head) = head_and_body(&mut stream) else {
            continue;
        };
        let path = head
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_owned();
        match path.as_str() {
            "/desc.xml" => answered(&mut stream, "text/xml", description(udn).as_bytes()),
            "/ctl" => answered(&mut stream, "text/xml", browsed(song_at).as_bytes()),
            "/song.flac" => song_answered(&mut stream, framed),
            _ => {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            }
        }
    }
}

fn answer_searches(socket: UdpSocket, location: SocketAddr, usn: Option<&'static str>) {
    let mut asked = [0; 2048];
    while let Ok((_, searcher)) = socket.recv_from(&mut asked) {
        let usn = usn
            .map(|udn| format!("USN: {udn}::{SERVER_TYPE}\r\n"))
            .unwrap_or_default();
        let answer = format!(
            "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: http://{location}/desc.xml\r\nST: {SERVER_TYPE}\r\n{usn}\r\n"
        );
        let _ = socket.send_to(answer.as_bytes(), searcher);
    }
}

impl Fake {
    fn started(shape: Shape) -> Self {
        let (first, _) = shape.described[0];
        let listener = TcpListener::bind(SocketAddr::new(first, 0)).expect("bound");
        let port = listener.local_addr().expect("an address").port();
        let song_at = SocketAddr::new(shape.song_on, port);

        let mut listeners = vec![(listener, shape.described[0].1)];

        for (ip, udn) in shape.described.iter().skip(1) {
            let also = TcpListener::bind(SocketAddr::new(*ip, port)).expect("bound beside");

            listeners.push((also, udn));
        }
        for (listener, udn) in listeners {
            let framed = shape.framed;

            thread::spawn(move || serve(listener, udn, song_at, framed));
        }

        let searched = UdpSocket::bind(SocketAddr::new(FOUR, 0)).expect("bound");
        let heard_at = searched.local_addr().expect("an address");
        let location = SocketAddr::new(shape.located_on, port);
        let usn = shape.usn;

        thread::spawn(move || answer_searches(searched, location, usn));

        Self { heard_at }
    }

    fn found(&self) -> Vec<ServerFound> {
        MediaServers::searching(self.heard_at, ANSWERED_WITHIN)
            .found()
            .expect("the search is answered")
    }
}

fn the_song_of(server: &ServerFound) -> Option<Vec<u8>> {
    let listing = server
        .children(&ObjectId::root())
        .expect("the root is browsed");
    let item = listing.items.first()?;
    let played = item.played_from()?;
    let mut fetched = server.fetched(&played.at).expect("the song is fetched");
    let mut whole = Vec::new();

    fetched.read_to_end(&mut whole).expect("read whole");
    Some(whole)
}

#[test]
fn a_server_whose_location_names_another_neighbour_address_is_described_there_once_its_udn_agrees()
{
    let elsewhere = Fake::started(Shape {
        described: vec![(SIX, SHELF)],
        located_on: SIX,
        song_on: SIX,
        ..Shape::plain()
    });
    let unvouched = Fake::started(Shape {
        described: vec![(SIX, SHELF)],
        located_on: SIX,
        usn: Some(ANOTHER),
        song_on: SIX,
        ..Shape::plain()
    });
    let unnamed = Fake::started(Shape {
        described: vec![(SIX, SHELF)],
        located_on: SIX,
        usn: None,
        song_on: SIX,
        ..Shape::plain()
    });

    let found = elsewhere.found();

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].udn(), &Udn::new(SHELF));
    assert_eq!(found[0].located().host(), "::1");
    assert_eq!(the_song_of(&found[0]), Some(song()));
    assert!(unvouched.found().is_empty());
    assert!(unnamed.found().is_empty());
}

#[test]
fn a_location_that_cannot_be_reached_is_asked_for_on_the_address_that_answered_the_search() {
    let docker = Fake::started(Shape {
        located_on: SIX,
        ..Shape::plain()
    });

    let found = docker.found();

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].udn(), &Udn::new(SHELF));
    assert_eq!(found[0].located().host(), "127.0.0.1");
    assert_eq!(the_song_of(&found[0]), Some(song()));
}

#[test]
fn a_location_naming_an_address_that_is_not_a_neighbour_is_dropped_and_a_private_one_is_kept() {
    let neighbour = Fake::started(Shape {
        located_on: PRIVATE,
        ..Shape::plain()
    });
    let public = Fake::started(Shape {
        located_on: PUBLIC,
        ..Shape::plain()
    });
    let public_v6 = Fake::started(Shape {
        located_on: PUBLIC_V6,
        ..Shape::plain()
    });

    let kept = neighbour.found();

    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].located().host(), "127.0.0.1");
    assert!(public.found().is_empty());
    assert!(public_v6.found().is_empty());
}

#[test]
fn a_song_on_an_address_that_is_not_a_neighbour_is_passed_over_without_asking_it_to_describe_itself()
 {
    let public = Fake::started(Shape {
        song_on: PUBLIC,
        ..Shape::plain()
    });

    let passed_over = the_song_of(&public.found()[0]);

    assert_eq!(passed_over, None);
}

#[test]
fn a_song_on_another_address_describing_the_same_server_is_played_and_one_describing_another_is_not()
 {
    let two_addresses = Fake::started(Shape {
        described: vec![(FOUR, SHELF), (SIX, SHELF)],
        song_on: SIX,
        ..Shape::plain()
    });
    let hostile = Fake::started(Shape {
        described: vec![(FOUR, SHELF), (SIX, ANOTHER)],
        song_on: SIX,
        ..Shape::plain()
    });
    let undescribed = Fake::started(Shape {
        song_on: SIX,
        ..Shape::plain()
    });

    let played = the_song_of(&two_addresses.found()[0]);
    let refused = the_song_of(&hostile.found()[0]);
    let passed_over = the_song_of(&undescribed.found()[0]);

    assert_eq!(played, Some(song()));
    assert_eq!(refused, None);
    assert_eq!(passed_over, None);
}

#[test]
fn a_chunked_song_is_read_whole_and_a_seek_reads_through_to_where_it_lands() {
    let chunked = Fake::started(Shape {
        framed: Framed::Chunked,
        ..Shape::plain()
    });
    let server = chunked.found().remove(0);

    let listing = server
        .children(&ObjectId::root())
        .expect("the root is browsed");
    let played = listing.items[0].played_from().expect("a resource");
    let mut fetched = server.fetched(&played.at).expect("the song is fetched");
    let mut whole = Vec::new();

    fetched.read_to_end(&mut whole).expect("read whole");

    let landed = fetched.seek(SeekFrom::Start(600)).expect("a seek");
    let mut after = Vec::new();

    fetched.read_to_end(&mut after).expect("read from there");

    assert_eq!(fetched.length(), None);
    assert_eq!(whole, song());
    assert_eq!(landed, 600);
    assert_eq!(after, song()[600..]);
}
