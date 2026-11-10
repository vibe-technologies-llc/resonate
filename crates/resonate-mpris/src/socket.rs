use std::{
    fs::{self, DirBuilder, Permissions},
    io,
    os::unix::{
        fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

use resonate_core::ZoneId;
use zbus::names::WellKnownName;

#[cfg(target_os = "macos")]
use crate::answering::Answering;
use crate::{
    BusOp, Error, PipeOp, Result,
    listening::{Serves, accepting},
    peer::{Peer, PipePeer},
    registry::{self, Claim},
    service::{BUS_NAME, INSTANCES, bus_name_of, instance_name},
};

const SOCKET_STEM: &str = "socket";
const OURS_ALONE: u32 = 0o700;
const READ_BY_US_ALONE: u32 = 0o600;
const OPEN_TO_OTHERS: u32 = 0o077;
const ACCEPT_THREAD: &str = "resonate-mpris-socket";

fn socket_name(player: &str) -> Option<String> {
    let instance = player.strip_prefix(BUS_NAME)?;

    Some(format!("{SOCKET_STEM}{instance}"))
}

fn socket_in(folder: &Path, player: &str) -> Option<PathBuf> {
    socket_name(player).map(|name| folder.join(name))
}

fn kept_to_this_user(folder: &Path) -> io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(OURS_ALONE)
        .create(folder)?;
    let standing = fs::metadata(folder)?;

    let ours = standing.uid() == rustix::process::getuid().as_raw();
    let closed = standing.mode() & OPEN_TO_OTHERS == 0;
    match ours && closed {
        true => Ok(()),
        false => Err(io::Error::from(io::ErrorKind::PermissionDenied)),
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn serving() -> Result<Vec<(WellKnownName<'static>, Arc<dyn Peer>)>> {
    serving_in(&registry::players_folder())
}

fn serving_in(folder: &Path) -> Result<Vec<(WellKnownName<'static>, Arc<dyn Peer>)>> {
    let names = registry::listed(folder).map_err(|source| Error::pipe(PipeOp::List, source))?;

    let mut found = Vec::new();
    for player in names {
        let Ok(name) = WellKnownName::try_from(player.clone()) else {
            continue;
        };
        let Some(socket) = socket_in(folder, &player) else {
            continue;
        };
        let reached = UnixStream::connect(&socket)
            .map_err(|source| Error::pipe(PipeOp::Connect, source))
            .and_then(PipePeer::over);
        match reached {
            Ok(peer) => {
                let peer: Arc<dyn Peer> = Arc::new(peer);
                found.push((name, peer));
            }
            Err(error) => {
                tracing::debug!(%error, %player, "a player named in the registry did not answer on its socket");
            }
        }
    }
    Ok(found)
}

pub(crate) struct Serving {
    name: WellKnownName<'static>,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    answering: Arc<dyn Serves<UnixStream>>,
    thread: Option<JoinHandle<()>>,
    claim: Option<Claim>,
}

impl Serving {
    #[cfg(target_os = "macos")]
    pub(crate) fn start(zone: ZoneId, answering: Arc<Answering>) -> Result<Self> {
        Self::start_in(&registry::players_folder(), zone, answering)
    }

    fn start_in(
        folder: &Path,
        zone: ZoneId,
        answering: Arc<dyn Serves<UnixStream>>,
    ) -> Result<Self> {
        kept_to_this_user(folder).map_err(|source| Error::pipe(PipeOp::Claim, source))?;
        let mut last = None;

        let names = std::iter::once(bus_name_of(zone))
            .chain((1..=INSTANCES).map(|attempt| instance_name(zone, attempt)));
        for player in names {
            let name = WellKnownName::try_from(player.clone())
                .map_err(|source| Error::bus(BusOp::ClaimName, source))?;
            match Self::taking(folder, &player)? {
                Some((claim, socket, listener)) => {
                    return Self::begun(name, socket, claim, listener, answering);
                }
                None => last = Some(name),
            }
        }

        match last {
            Some(name) => Err(Error::NameTaken { name: name.into() }),
            None => Err(Error::ThreadStopped),
        }
    }

    fn taking(folder: &Path, player: &str) -> Result<Option<(Claim, PathBuf, UnixListener)>> {
        let claim = match Claim::make(folder, player) {
            Ok(claim) => claim,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                tracing::debug!(%player, "another player already holds this name");
                return Ok(None);
            }
            Err(source) => return Err(Error::pipe(PipeOp::Claim, source)),
        };
        let Some(socket) = socket_in(folder, player) else {
            return Ok(None);
        };

        let listener =
            listening_at(&socket).map_err(|source| Error::pipe(PipeOp::Listen, source))?;
        Ok(Some((claim, socket, listener)))
    }

    fn begun(
        name: WellKnownName<'static>,
        socket: PathBuf,
        claim: Claim,
        listener: UnixListener,
        answering: Arc<dyn Serves<UnixStream>>,
    ) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let spawned = {
            let stop = Arc::clone(&stop);
            let answering = Arc::clone(&answering);
            thread::Builder::new()
                .name(ACCEPT_THREAD.to_owned())
                .spawn(move || {
                    accepting(
                        || listener.accept().map(|(stream, _)| stream),
                        &answering,
                        &stop,
                    );
                })
        };
        let thread = match spawned {
            Ok(thread) => thread,
            Err(source) => {
                let _ = fs::remove_file(&socket);
                return Err(Error::pipe(PipeOp::Spawn, source));
            }
        };

        tracing::info!(%name, "the player is served on a socket");
        Ok(Self {
            name,
            socket,
            stop,
            answering,
            thread: Some(thread),
            claim: Some(claim),
        })
    }

    pub(crate) fn name(&self) -> WellKnownName<'static> {
        self.name.clone()
    }

    pub(crate) fn rest(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.answering.close();
        if let Some(thread) = self.thread.take() {
            let _ = UnixStream::connect(&self.socket);
            let _ = thread.join();
        }
        if self.claim.is_some() {
            let _ = fs::remove_file(&self.socket);
        }
        drop(self.claim.take());
    }
}

impl Drop for Serving {
    fn drop(&mut self) {
        self.rest();
    }
}

fn listening_at(socket: &Path) -> io::Result<UnixListener> {
    match fs::remove_file(socket) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(socket)?;
    fs::set_permissions(socket, Permissions::from_mode(READ_BY_US_ALONE))?;

    Ok(listener)
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        io::{Read as _, Write as _},
        process,
    };

    use super::*;

    const LONGEST_PATH_A_MAC_BINDS: usize = 103;
    const A_LONG_HOME: &str = "/Users/a-listener-with-a-long-name";
    const HIGHEST_PROCESS_A_MAC_NUMBERS: u32 = 99_999;
    const MORE_ZONES_THAN_A_WINDOW_HOLDS: u32 = 99;

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("resonate-socket-{}-{name}", process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct Echoing;

    impl Serves<UnixStream> for Echoing {
        fn serve(&self, mut stream: UnixStream) {
            let mut heard = [0; 1];
            while stream.read_exact(&mut heard).is_ok() {
                if stream.write_all(&heard).is_err() {
                    return;
                }
            }
        }

        fn close(&self) {}
    }

    fn echoing() -> Arc<dyn Serves<UnixStream>> {
        Arc::new(Echoing)
    }

    fn second_zone() -> ZoneId {
        ZoneId::new(2).expect("a second zone")
    }

    fn echoed(socket: &Path, byte: u8) -> u8 {
        let mut stream = UnixStream::connect(socket).expect("the socket answers");
        stream.write_all(&[byte]).expect("a byte sent");
        let mut heard = [0; 1];
        stream.read_exact(&mut heard).expect("a byte back");
        heard[0]
    }

    #[test]
    fn a_socket_is_named_for_what_follows_the_players_name_and_a_stranger_has_none() {
        assert_eq!(socket_name(BUS_NAME).as_deref(), Some("socket"));
        assert_eq!(
            socket_name(&bus_name_of(second_zone())).as_deref(),
            Some("socket.zone2")
        );
        assert_eq!(
            socket_name(&format!("{BUS_NAME}.instance4242.zone2")).as_deref(),
            Some("socket.instance4242.zone2")
        );
        assert_eq!(socket_name("org.mpris.MediaPlayer2.vlc"), None);
    }

    #[test]
    fn the_longest_name_a_player_is_likely_to_take_still_binds_under_a_long_home_on_a_mac() {
        let folder = registry::players_folder_of_a_mac(
            Some(Path::new(A_LONG_HOME)),
            Path::new("/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000/T"),
        );
        let longest = format!(
            "{BUS_NAME}.instance{HIGHEST_PROCESS_A_MAC_NUMBERS}-{INSTANCES}.zone{MORE_ZONES_THAN_A_WINDOW_HOLDS}"
        );

        let socket = socket_in(&folder, &longest).expect("a name of this build");

        assert!(
            socket.as_os_str().len() <= LONGEST_PATH_A_MAC_BINDS,
            "{} bytes",
            socket.as_os_str().len()
        );
    }

    #[test]
    fn a_player_is_reached_on_the_socket_beside_its_claim_and_leaves_neither_behind() {
        let folder = Folder::new("served");

        let mut serving =
            Serving::start_in(&folder.0, ZoneId::FIRST, echoing()).expect("a player served");
        let socket = folder.0.join("socket");

        assert_eq!(serving.name().as_str(), BUS_NAME);
        assert_eq!(echoed(&socket, 7), 7);
        assert_eq!(
            registry::listed(&folder.0).expect("the folder lists"),
            vec![BUS_NAME.to_owned()],
            "the socket was listed as a player"
        );

        serving.rest();

        assert!(!socket.exists(), "the socket outlived its player");
        assert!(!folder.0.join(BUS_NAME).exists());
    }

    #[test]
    fn the_folder_and_the_socket_are_this_users_alone() {
        let folder = Folder::new("private");

        let _serving =
            Serving::start_in(&folder.0, ZoneId::FIRST, echoing()).expect("a player served");

        let mode = |path: &Path| fs::metadata(path).expect("it stands").mode() & 0o777;
        assert_eq!(mode(&folder.0), OURS_ALONE);
        assert_eq!(mode(&folder.0.join("socket")), READ_BY_US_ALONE);
    }

    #[test]
    fn a_folder_others_can_reach_is_refused_rather_than_served_in() {
        let folder = Folder::new("open");
        fs::create_dir_all(&folder.0).expect("a folder");
        fs::set_permissions(&folder.0, Permissions::from_mode(0o755)).expect("opened to others");

        let refused = Serving::start_in(&folder.0, ZoneId::FIRST, echoing()).map(drop);

        assert!(matches!(
            refused,
            Err(Error::Pipe {
                op: PipeOp::Claim,
                ref source
            }) if source.kind() == io::ErrorKind::PermissionDenied
        ));
    }

    #[test]
    fn a_socket_a_dead_player_left_is_taken_over_by_the_next_to_claim_its_name() {
        let folder = Folder::new("stale");
        kept_to_this_user(&folder.0).expect("a folder");
        let socket = folder.0.join("socket");
        drop(UnixListener::bind(&socket).expect("what a dead player left"));

        assert!(UnixStream::connect(&socket).is_err());

        let _serving =
            Serving::start_in(&folder.0, ZoneId::FIRST, echoing()).expect("a player served");

        assert_eq!(echoed(&socket, 9), 9);
    }

    #[test]
    fn a_second_player_takes_an_instance_name_and_a_socket_of_its_own() {
        let folder = Folder::new("second");

        let first =
            Serving::start_in(&folder.0, ZoneId::FIRST, echoing()).expect("a player served");
        let second =
            Serving::start_in(&folder.0, ZoneId::FIRST, echoing()).expect("a second player");

        let instance = format!("{BUS_NAME}.instance{}", process::id());
        assert_eq!(second.name().as_str(), instance);
        assert_eq!(
            echoed(
                &folder.0.join(format!("socket.instance{}", process::id())),
                3
            ),
            3
        );
        assert_eq!(echoed(&folder.0.join("socket"), 4), 4);
        drop((first, second));
    }

    #[test]
    fn a_player_of_another_zone_is_found_on_its_own_socket_and_a_silent_one_is_passed_over() {
        let folder = Folder::new("found");
        let zone = Serving::start_in(&folder.0, second_zone(), echoing()).expect("a zone served");
        let silent = Claim::make(&folder.0, BUS_NAME).expect("a player with no socket");

        let found = serving_in(&folder.0).expect("the folder is read");

        let names: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec![bus_name_of(second_zone())]);
        drop((zone, silent));
    }
}
