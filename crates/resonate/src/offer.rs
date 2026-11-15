use std::sync::Arc;

use crossbeam_channel::{Receiver, bounded};
use resonate_upnp::{Catalog, MediaServer, Offering, Page, Shelf, Udn};

use crate::{
    Error, Result,
    cli::Cli,
    config::{self, Config},
    offering::{Shelves, kept_udn},
    words,
};

struct Counts {
    songs: u32,
    albums: u32,
    artists: u32,
    playlists: u32,
}

pub(crate) fn serve(cli: &Cli, config: &Config) -> Result<()> {
    let library = Arc::new(crate::open_library(cli, config)?);
    let shelves = Arc::new(Shelves::over(library));

    let (stop, stopped) = bounded(1);
    let told = stop.clone();
    let _watching = crate::signals::cancel_when_told(move || {
        let _ = told.try_send(());
    });

    offer(
        &shelves,
        named_on_the_network(),
        Offering::on_the_network(),
        &stopped,
    )
}

fn named_on_the_network() -> Udn {
    match config::private_data_dir() {
        Ok(folder) => kept_udn(&folder),
        Err(error) => {
            tracing::warn!(%error, "no folder keeps the media server's name on the network");
            Udn::minted()
        }
    }
}

fn offer(
    shelves: &Arc<Shelves>,
    udn: Udn,
    offering: Offering,
    stopped: &Receiver<()>,
) -> Result<()> {
    let counts = counted(shelves)?;
    let server = MediaServer::start(Arc::clone(shelves), udn, offering).map_err(|source| {
        Error::CatalogNotOffered {
            source: Box::new(source),
        }
    })?;

    said!(
        "{}",
        words::offer::offering(server.port().to_string(), server.udn().as_str())
    );
    said!(
        "{}",
        words::offer::holds(
            counts.albums,
            counts.artists,
            counts.playlists,
            counts.songs
        )
    );

    let _ = stopped.recv();
    drop(server);

    said!("{}", words::offer::stopped());
    Ok(())
}

fn counted(shelves: &Shelves) -> Result<Counts> {
    let total = |shelf| -> Result<u32> {
        Ok(shelves
            .shelved(shelf, Page::counting())?
            .map_or(0, |shelved| shelved.total))
    };
    Ok(Counts {
        songs: total(Shelf::Songs)?,
        albums: total(Shelf::Albums)?,
        artists: total(Shelf::Artists)?,
        playlists: total(Shelf::Playlists)?,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Read as _, Write as _},
        net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket},
        thread,
        time::{Duration, Instant},
    };

    use super::*;
    use crate::{
        cli::Sub,
        fixtures::{ALBUM, ARTIST, TITLE, parsed, scanned, scratch, tagged_wave},
    };

    const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    const HEARD_WITHIN: Duration = Duration::from_secs(10);

    fn heard(socket: &UdpSocket, until: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + HEARD_WITHIN;
        let mut buffer = [0; 8192];
        while Instant::now() < deadline {
            let Ok(read) = socket.recv(&mut buffer) else {
                continue;
            };
            let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
            if until(&text) {
                return text;
            }
        }
        panic!("nothing was heard in time");
    }

    fn located_by(notice: &str) -> SocketAddr {
        notice
            .lines()
            .find_map(|line| line.strip_prefix("LOCATION: http://"))
            .and_then(|rest| rest.split('/').next())
            .and_then(|authority| authority.parse().ok())
            .expect("a location on loopback")
    }

    fn fetched(address: SocketAddr, path: &str) -> String {
        let mut stream = TcpStream::connect(address).expect("connects");
        stream
            .set_read_timeout(Some(HEARD_WITHIN))
            .expect("a read timeout");
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
        )
        .expect("asks");
        let mut answer = String::new();
        stream.read_to_string(&mut answer).expect("an answer");
        answer
    }

    #[test]
    fn the_subcommand_is_read_from_the_grammar() {
        assert!(matches!(
            parsed(&["resonate", "offer"]).command,
            Some(Sub::Offer)
        ));
    }

    #[test]
    fn the_catalog_is_offered_until_told_to_stop_and_byebye_is_sent() {
        let root = scratch("offer");
        for (file, title) in [("a.wav", "Alpha"), ("b.wav", "Bravo")] {
            fs::write(
                root.join(file),
                tagged_wave(&[(TITLE, title), (ARTIST, "Band"), (ALBUM, "Record")]),
            )
            .expect("a song");
        }
        let shelves = Arc::new(Shelves::over(Arc::new(scanned(&root))));
        let _ = fs::remove_dir_all(&root);

        let told = UdpSocket::bind(SocketAddr::new(LOOPBACK, 0)).expect("a socket told of it");
        told.set_read_timeout(Some(HEARD_WITHIN))
            .expect("a read timeout");
        let offering = Offering {
            heard_on: SocketAddr::new(LOOPBACK, 0),
            told: told.local_addr().expect("an address"),
            every: Duration::from_secs(60),
            serving_on: LOOPBACK,
        };
        let udn = Udn::minted();
        let (stop, stopped) = bounded(1);

        let offering_thread = {
            let udn = udn.clone();
            thread::spawn(move || offer(&shelves, udn, offering, &stopped))
        };

        let alive = heard(&told, |told| told.contains("NTS: ssdp:alive"));
        let at = located_by(&alive);
        let description = fetched(at, "/description.xml");
        assert!(description.starts_with("HTTP/1.1 200"));
        assert!(description.contains(udn.as_str()));

        assert!(!offering_thread.is_finished());
        stop.send(()).expect("the offer is listening");

        let gone = heard(&told, |told| told.contains("NTS: ssdp:byebye"));
        offering_thread
            .join()
            .expect("the offer did not panic")
            .expect("the offer ended well");

        assert!(gone.contains(udn.as_str()));
    }

    #[test]
    fn the_counts_the_offer_prints_are_the_shelves_totals() {
        let root = scratch("offer-counts");
        for (file, title) in [("a.wav", "Alpha"), ("b.wav", "Bravo"), ("c.wav", "Charlie")] {
            fs::write(
                root.join(file),
                tagged_wave(&[(TITLE, title), (ARTIST, "Band"), (ALBUM, "Record")]),
            )
            .expect("a song");
        }
        let shelves = Shelves::over(Arc::new(scanned(&root)));
        let _ = fs::remove_dir_all(&root);

        let counts = counted(&shelves).expect("the totals");

        assert_eq!(counts.songs, 3);
        assert_eq!(counts.albums, 1);
        assert_eq!(counts.artists, 1);
        assert_eq!(counts.playlists, 0);
    }
}
