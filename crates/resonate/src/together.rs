#[cfg(feature = "ui")]
use std::ffi::OsString;
use std::{
    fs::{self, File},
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
#[cfg(feature = "ui")]
use resonate_core::ZoneId;
use resonate_core::{PlaylistId, Recording};
use resonate_library::{Library, PlaylistName, SyncedPlaylist};
#[cfg(feature = "ui")]
use resonate_sync::DeviceId;
#[cfg(feature = "ui")]
use resonate_sync::HOSTED_SERVER;
#[cfg(feature = "ui")]
use resonate_sync::party::{
    PartyChat, PartyHanding, PartyReach, PartyReaction, SittingOut, Standing,
};
#[cfg(any(feature = "ui", test))]
use resonate_sync::shared::MemberId;
use resonate_sync::{
    HeldShare, LocalPlaylistId, Refused, SpaceId,
    shared::{
        Invitation, Invited, LocalShared, PersonShown, Shared, SharedCatalog, SharedSettings,
        SharedShown, SharedStanding, invitation_in,
    },
};

use crate::{
    Error, Result,
    cli::SharedArgs,
    config::{self, Config},
    error::ConfigKey,
    sync::server_of,
    table::Table,
    words,
};

const SHARED_STORE: &str = "shared.sqlite";

const SHARED_ALONE: &str = "shared.lock";

const SETTLES_WITHIN: Duration = Duration::from_secs(20);

const SETTLED_LOOKED_AT_EVERY: Duration = Duration::from_millis(100);

static RUNNING: Mutex<Option<Arc<Shared>>> = Mutex::new(None);

static HOME_SERVER: Mutex<Option<String>> = Mutex::new(None);

pub(crate) struct Together {
    shared: Arc<Shared>,
    _alone: File,
}

impl Drop for Together {
    fn drop(&mut self) {
        *RUNNING.lock() = None;
        *HOME_SERVER.lock() = None;
    }
}

struct Listed {
    library: Arc<Library>,
}

fn answered<T>(read: resonate_library::Result<T>) -> std::result::Result<T, Refused> {
    read.map_err(|error| {
        tracing::warn!(%error, "the catalog could not answer for a shared playlist");
        Refused::Unavailable
    })
}

fn id_of(at: LocalPlaylistId) -> std::result::Result<PlaylistId, Refused> {
    PlaylistId::new(at.id.cast_unsigned()).map_err(|_| Refused::NotHere)
}

fn local_of(playlist: SyncedPlaylist) -> LocalShared {
    LocalShared {
        at: LocalPlaylistId {
            id: playlist.id.get().cast_signed(),
            created: playlist.created,
        },
        name: playlist.name,
        entries: playlist.entries,
    }
}

impl SharedCatalog for Listed {
    fn shared_read(
        &self,
        at: LocalPlaylistId,
    ) -> std::result::Result<Option<LocalShared>, Refused> {
        answered(self.library.shared_playlist(id_of(at)?, at.created))
            .map(|playlist| playlist.map(local_of))
    }

    fn shared_write(
        &self,
        at: Option<LocalPlaylistId>,
        name: &str,
        entries: &[Recording],
        editable: bool,
    ) -> std::result::Result<LocalShared, Refused> {
        let at = at
            .map(|at| id_of(at).map(|id| (id, at.created)))
            .transpose()?;

        answered(
            self.library
                .write_shared_playlist(at, name, entries, editable),
        )
        .map(local_of)
    }

    fn shared_flag(
        &self,
        at: LocalPlaylistId,
        editable: bool,
    ) -> std::result::Result<Option<LocalShared>, Refused> {
        answered(
            self.library
                .flag_playlist_shared(id_of(at)?, at.created, editable),
        )
        .map(|playlist| playlist.map(local_of))
    }

    fn shared_unflag(&self, at: LocalPlaylistId) -> std::result::Result<(), Refused> {
        answered(self.library.unflag_playlist_shared(id_of(at)?, at.created))
    }

    fn shared_held_as(
        &self,
        recording: &Recording,
    ) -> std::result::Result<Option<Recording>, Refused> {
        answered(self.library.held_as(recording))
    }

    fn shared_revision(&self) -> u64 {
        self.library.catalog_revision()
    }
}

pub(crate) fn running() -> Option<Arc<Shared>> {
    RUNNING.lock().clone()
}

pub(crate) fn hides_as_shared(
    library: &Library,
    at: LocalPlaylistId,
) -> std::result::Result<bool, Refused> {
    let id = id_of(at)?;

    Ok(answered(library.playlists_shared())?
        .iter()
        .any(|(shared, _)| *shared == id)
        && answered(library.shared_playlist(id, at.created))?.is_some())
}

pub(crate) fn shares_here() -> Option<Vec<HeldShare>> {
    running().map(|shared| shared.held())
}

pub(crate) fn start(config: &Config, library: &Arc<Library>) -> Result<Option<Together>> {
    if !config.syncs() {
        return Ok(None);
    }

    start_in(&config::private_data_dir()?, config, library)
}

fn alone_in(folder: &Path) -> Result<Option<File>> {
    let path = folder.join(SHARED_ALONE);
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|source| Error::SyncLock {
            path: path.clone(),
            source,
        })?;

    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(source)) => Err(Error::SyncLock { path, source }),
    }
}

fn start_in(folder: &Path, config: &Config, library: &Arc<Library>) -> Result<Option<Together>> {
    let Some(file) = alone_in(folder)? else {
        return Ok(None);
    };

    let store = folder.join(SHARED_STORE);

    config::private_store(&store)?;

    let settings = SharedSettings {
        server: server_of(config),
        trusted: config.sync_server_certificate.clone(),
        store,
        name: config.shared_name.clone().unwrap_or_default(),
    };
    let catalog = Arc::new(Listed {
        library: Arc::clone(library),
    });

    let shared = Arc::new(Shared::start(settings, catalog).map_err(Error::Sync)?);

    *RUNNING.lock() = Some(Arc::clone(&shared));
    *HOME_SERVER.lock() = server_of(config).address();

    Ok(Some(Together {
        shared,
        _alone: file,
    }))
}

#[cfg(feature = "ui")]
pub(crate) fn started_in_the_window(
    config: &Config,
    library: &Arc<Library>,
    arrivals: Arrivals,
) -> Option<Together> {
    started(start(config, library), arrivals)
}

#[cfg(feature = "ui")]
fn started(outcome: Result<Option<Together>>, arrivals: Arrivals) -> Option<Together> {
    let together = outcome.unwrap_or_else(|error| {
        tracing::warn!(%error, "shared playlists could not start");
        None
    });

    arrivals.judged();

    together
}

#[cfg(feature = "ui")]
struct Seen {
    library: Arc<Library>,
}

#[cfg(feature = "ui")]
pub(crate) fn for_the_window(library: &Arc<Library>) -> Arc<dyn resonate_ui::Together> {
    Arc::new(Seen {
        library: Arc::clone(library),
    })
}

#[cfg(feature = "ui")]
impl Seen {
    fn local(&self, playlist: PlaylistId) -> Option<LocalPlaylistId> {
        Some(LocalPlaylistId {
            id: playlist.get().cast_signed(),
            created: self.library.playlist_stamp(playlist).ok()?,
        })
    }

    fn list_of(&self, playlist: PlaylistId) -> Option<SharedShown> {
        let at = self.local(playlist)?;

        running()?
            .shown()
            .into_iter()
            .find(|list| list.at == Some(at))
    }
}

#[cfg(feature = "ui")]
impl resonate_ui::Together for Seen {
    fn seen(&self, playlist: PlaylistId) -> Option<resonate_ui::SharedSeen> {
        let list = self.list_of(playlist)?;

        Some(resonate_ui::SharedSeen {
            may_edit: list.may_edit,
            edit_link: list.invites.edit_page.or(list.invites.edit),
            view_link: list.invites.view_page.unwrap_or(list.invites.view),
            people: list
                .people
                .into_iter()
                .map(|person| resonate_ui::SharedPerson {
                    id: person.member.0,
                    name: person.name,
                    me: person.me,
                })
                .collect(),
            unheld: list
                .unheld
                .into_iter()
                .map(|recording| recording.title)
                .collect(),
            reach: match list.standing {
                SharedStanding::Waiting => resonate_ui::SharedReach::Waiting,
                SharedStanding::Reached => resonate_ui::SharedReach::Reached,
                SharedStanding::Unreached => resonate_ui::SharedReach::Unreached,
                SharedStanding::ServerTooOld => resonate_ui::SharedReach::ServerTooOld,
                SharedStanding::TurnedAway(_) => resonate_ui::SharedReach::TurnedAway,
                SharedStanding::NoServer => resonate_ui::SharedReach::NoServer,
            },
            name: list.name,
        })
    }

    fn shared_playlists(&self) -> Vec<(PlaylistId, bool)> {
        let Some(shared) = running() else {
            return Vec::new();
        };

        shared
            .shown()
            .into_iter()
            .filter_map(|list| {
                let at = list.at?;

                Some((PlaylistId::new(at.id.cast_unsigned()).ok()?, list.may_edit))
            })
            .collect()
    }

    fn share(&self, playlist: PlaylistId) -> bool {
        let (Some(shared), Some(at)) = (running(), self.local(playlist)) else {
            return false;
        };

        shared
            .share(at)
            .inspect_err(|error| tracing::warn!(%error, "a playlist could not be shared"))
            .is_ok()
    }

    fn watch(&self, playlist: Option<PlaylistId>) {
        let Some(shared) = running() else {
            return;
        };

        shared.watch(
            playlist
                .and_then(|playlist| self.list_of(playlist))
                .map(|list| list.space),
        );
    }

    fn leave(&self, playlist: PlaylistId) -> bool {
        let (Some(shared), Some(list)) = (running(), self.list_of(playlist)) else {
            return false;
        };

        shared.leave(list.space).is_ok()
    }

    fn reset(&self, playlist: PlaylistId, removing: Option<[u8; 16]>) -> bool {
        let (Some(shared), Some(list)) = (running(), self.list_of(playlist)) else {
            return false;
        };

        shared
            .reset(list.space, removing.map(MemberId))
            .inspect_err(|error| tracing::warn!(%error, "a shared playlist could not be reset"))
            .is_ok()
    }

    fn accept(&self, invitation: &str) -> bool {
        accepted(invitation, Arrival::Pasted)
    }

    fn join_party(&self, invitation: &str, zone: ZoneId) -> bool {
        crate::party_room::join_in(invitation, zone)
    }

    fn named(&self, name: &str) {
        if let Some(shared) = running() {
            shared.named(name.to_owned());
        }
        crate::party_room::named(name);
    }

    fn party(&self) -> Option<resonate_ui::PartySeen> {
        let (shown, followed, zone) = crate::party_room::shown()?;

        Some(resonate_ui::PartySeen {
            zone,
            hosting: shown.hosting,
            link: shown.page.unwrap_or(shown.link),
            reach: match shown.reach {
                PartyReach::Here => resonate_ui::PartyReached::Here,
                PartyReach::Connecting => resonate_ui::PartyReached::Connecting,
                PartyReach::Reconnecting => resonate_ui::PartyReached::Reconnecting,
                PartyReach::Ended => resonate_ui::PartyReached::Ended,
                PartyReach::NoSuchParty => resonate_ui::PartyReached::NoSuchParty,
                PartyReach::TurnedAway => resonate_ui::PartyReached::TurnedAway,
                PartyReach::ServerTooOld => resonate_ui::PartyReached::ServerTooOld,
                PartyReach::HandedOver => resonate_ui::PartyReached::HandedOver,
            },
            handing: shown.handing.map(|handing| {
                let name_of = |member: DeviceId| {
                    shown
                        .people
                        .iter()
                        .find(|person| person.member == member)
                        .map(|person| person.name.clone())
                        .unwrap_or_default()
                };
                match handing {
                    PartyHanding::To(member) => resonate_ui::PartyHandingSeen::To(name_of(member)),
                    PartyHanding::NotTaken(member) => {
                        resonate_ui::PartyHandingSeen::NotTaken(name_of(member))
                    }
                }
            }),
            people: shown
                .people
                .into_iter()
                .map(|person| resonate_ui::SharedPerson {
                    id: person.member.0,
                    name: person.name,
                    me: person.me,
                })
                .collect(),
            standing: match followed.standing {
                Standing::WaitingForTheHost => resonate_ui::PartyStanding::WaitingForTheHost,
                Standing::Finding => resonate_ui::PartyStanding::Finding,
                Standing::SittingOut(SittingOut::NotHeld) => resonate_ui::PartyStanding::NotHeld,
                Standing::SittingOut(SittingOut::CannotStartMidSong) => {
                    resonate_ui::PartyStanding::CannotStartMidSong
                }
                Standing::SittingOut(SittingOut::AskedTo) => resonate_ui::PartyStanding::SatOut,
                Standing::HostPaused => resonate_ui::PartyStanding::HostPaused,
                Standing::InStep => resonate_ui::PartyStanding::InStep,
            },
            song: shown
                .now
                .and_then(|now| now.song)
                .map(|song| (song.recording.title, song.recording.artist)),
            asked_for: shown
                .asked_for
                .into_iter()
                .map(|asked| resonate_ui::PartyRequestSeen {
                    id: resonate_ui::PartyRequestId(asked.id().to_bytes()),
                    title: asked.song.recording.title,
                    artist: asked.song.recording.artist,
                })
                .collect(),
            round_trip: shown.round_trip,
            apart: shown.apart.map(|apart| resonate_ui::PartyApartSeen {
                nanos: apart.nanos,
                corrected: apart.corrected,
            }),
            chat: shown
                .chat
                .into_iter()
                .map(|line| resonate_ui::PartyLineSeen {
                    id: line.id,
                    name: line.name,
                    me: line.from == shown.me,
                    saying: match line.chat {
                        PartyChat::Words(words) => resonate_ui::PartySaying::Words(words),
                        PartyChat::Reaction(reaction) => {
                            resonate_ui::PartySaying::Reaction(reaction_seen(reaction))
                        }
                    },
                })
                .collect(),
        })
    }

    fn begin_party(&self, zone: ZoneId) -> bool {
        crate::party_room::begin(zone)
            .inspect_err(|error| tracing::warn!(%error, "a party could not begin"))
            .is_ok()
    }

    fn leave_party(&self) {
        crate::party_room::leave();
    }

    fn turn_out(&self, person: [u8; 16]) {
        crate::party_room::turn_out(person);
    }

    fn play_request_next(&self, request: resonate_ui::PartyRequestId) -> bool {
        crate::party_room::play_a_request_next(request)
    }

    fn dismiss_request(&self, request: resonate_ui::PartyRequestId) {
        crate::party_room::dismiss_a_request(request);
    }

    fn following(&self, zone: ZoneId) -> bool {
        crate::party_room::following(zone)
    }

    fn say_in_party(&self, words: &str) {
        if let Some(chat) = PartyChat::words(words) {
            crate::party_room::say(chat);
        }
    }

    fn react_in_party(&self, reaction: resonate_ui::PartyReaction) {
        crate::party_room::say(PartyChat::Reaction(reaction_said(reaction)));
    }

    fn hand_over(&self, person: [u8; 16]) {
        crate::party_room::hand_over(person);
    }
}

#[cfg(feature = "ui")]
const fn reaction_seen(reaction: PartyReaction) -> resonate_ui::PartyReaction {
    match reaction {
        PartyReaction::Love => resonate_ui::PartyReaction::Love,
        PartyReaction::Fire => resonate_ui::PartyReaction::Fire,
        PartyReaction::Laugh => resonate_ui::PartyReaction::Laugh,
        PartyReaction::Clap => resonate_ui::PartyReaction::Clap,
        PartyReaction::Dance => resonate_ui::PartyReaction::Dance,
        PartyReaction::Wow => resonate_ui::PartyReaction::Wow,
    }
}

#[cfg(feature = "ui")]
const fn reaction_said(reaction: resonate_ui::PartyReaction) -> PartyReaction {
    match reaction {
        resonate_ui::PartyReaction::Love => PartyReaction::Love,
        resonate_ui::PartyReaction::Fire => PartyReaction::Fire,
        resonate_ui::PartyReaction::Laugh => PartyReaction::Laugh,
        resonate_ui::PartyReaction::Clap => PartyReaction::Clap,
        resonate_ui::PartyReaction::Dance => PartyReaction::Dance,
        resonate_ui::PartyReaction::Wow => PartyReaction::Wow,
    }
}

#[cfg(feature = "ui")]
pub(crate) struct Arrivals(Vec<String>);

#[cfg(feature = "ui")]
impl Arrivals {
    fn judged(self) {
        for text in self.0 {
            accepted(&text, Arrival::FromOutside);
        }
    }
}

#[cfg(feature = "ui")]
pub(crate) fn invitations_among(arguments: &[OsString]) -> (Vec<OsString>, Arrivals) {
    let (invitations, files): (Vec<&OsString>, Vec<&OsString>) = arguments
        .iter()
        .partition(|argument| invitation_in(&argument.to_string_lossy()).is_some());

    (
        files.into_iter().cloned().collect(),
        Arrivals(
            invitations
                .into_iter()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect(),
        ),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arrival {
    #[cfg(feature = "ui")]
    Pasted,
    #[cfg(feature = "ui")]
    FromOutside,
    FromTheBus,
}

impl Arrival {
    #[cfg(feature = "ui")]
    fn asks_first_of_a_list(self, invitation: &Invitation) -> bool {
        match self {
            Self::Pasted => false,
            Self::FromOutside => !names_the_server_in_use(invitation),
            Self::FromTheBus => true,
        }
    }

    #[cfg(not(feature = "ui"))]
    fn asks_first_of_a_list(self, _invitation: &Invitation) -> bool {
        true
    }
}

pub(crate) fn accepted(text: &str, arrival: Arrival) -> bool {
    let Some(invitation) = invitation_in(text) else {
        return false;
    };

    if invitation.to == Invited::Party {
        #[cfg(feature = "ui")]
        return match arrival {
            Arrival::Pasted => crate::party_room::accept(text),
            Arrival::FromOutside | Arrival::FromTheBus => crate::party_room::offer(text),
        };
        #[cfg(not(feature = "ui"))]
        return {
            let _ = arrival;
            false
        };
    }

    if arrival.asks_first_of_a_list(&invitation) {
        #[cfg(feature = "ui")]
        return crate::party_room::offer(text);
        #[cfg(not(feature = "ui"))]
        return false;
    }

    thread::spawn(move || {
        let deadline = Instant::now() + SETTLES_WITHIN;
        loop {
            if let Some(shared) = running() {
                if let Err(error) = shared.join(&invitation.code, invitation.server.clone()) {
                    tracing::warn!(%error, "a shared playlist's link could not be joined");
                }
                return;
            }
            if Instant::now() >= deadline {
                tracing::warn!("a shared playlist's link came while nothing was sharing");
                return;
            }
            thread::sleep(SETTLED_LOOKED_AT_EVERY);
        }
    });

    true
}

#[cfg(feature = "ui")]
pub(crate) fn server_in_use() -> String {
    HOME_SERVER
        .lock()
        .clone()
        .unwrap_or_else(|| HOSTED_SERVER.to_owned())
}

#[cfg(feature = "ui")]
pub(crate) fn names_the_server_in_use(invitation: &Invitation) -> bool {
    invitation
        .naming_a_server_other_than(&server_in_use())
        .is_none()
}

enum Looked<T> {
    Settled(T),
    Standing(SharedStanding),
}

const fn not_to_be_waited_out(standing: SharedStanding) -> bool {
    matches!(
        standing,
        SharedStanding::ServerTooOld | SharedStanding::TurnedAway(_) | SharedStanding::NoServer
    )
}

fn waited_for<T>(
    shared: &Shared,
    within: Duration,
    look: impl Fn(&Shared) -> Looked<T>,
) -> Result<T> {
    let deadline = Instant::now() + within;

    shared.force();
    loop {
        let standing = match look(shared) {
            Looked::Settled(settled) => return Ok(settled),
            Looked::Standing(standing) => standing,
        };

        if Instant::now() >= deadline || not_to_be_waited_out(standing) {
            return Err(Error::SharedNotSettled { standing });
        }
        thread::sleep(SETTLED_LOOKED_AT_EVERY);
    }
}

fn looked_at(list: Option<SharedShown>) -> Looked<SharedShown> {
    match list {
        Some(list) if list.standing != SharedStanding::Reached => Looked::Standing(list.standing),
        Some(list) if list.owed > 0 => Looked::Standing(SharedStanding::Waiting),
        Some(list) => Looked::Settled(list),
        None => Looked::Standing(SharedStanding::Waiting),
    }
}

fn settled_at(shared: &Shared, within: Duration, at: LocalPlaylistId) -> Result<SharedShown> {
    waited_for(shared, within, |shared| {
        looked_at(shared.shown().into_iter().find(|list| list.at == Some(at)))
    })
}

fn settled_in(shared: &Shared, within: Duration, space: SpaceId) -> Result<SharedShown> {
    waited_for(shared, within, |shared| {
        looked_at(shared.shown().into_iter().find(|list| list.space == space))
    })
}

fn settled_leaving(shared: &Shared, within: Duration, space: SpaceId) -> Result<()> {
    waited_for(shared, within, |shared| {
        shared
            .leaving()
            .into_iter()
            .find(|(leaving, _)| *leaving == space)
            .map_or(Looked::Settled(()), |(_, standing)| {
                Looked::Standing(standing)
            })
    })
}

fn given_time_to_settle(shared: &Shared) {
    let deadline = Instant::now() + SETTLES_WITHIN;

    shared.force();
    while Instant::now() < deadline {
        if shared
            .shown()
            .iter()
            .all(|list| list.owed == 0 && !matches!(list.standing, SharedStanding::Waiting))
        {
            return;
        }
        thread::sleep(SETTLED_LOOKED_AT_EVERY);
    }
}

fn listed_as(library: &Library, shared: &Shared, name: &str) -> Result<SharedShown> {
    let playlist = library
        .playlist_named(name)?
        .ok_or_else(|| Error::NoSuchPlaylist(PlaylistName::new(name)))?;
    let at = playlist.id.get().cast_signed();

    shared
        .shown()
        .into_iter()
        .find(|list| list.at.is_some_and(|local| local.id == at))
        .ok_or_else(|| Error::PlaylistNotShared(name.into()))
}

fn said_of(standing: SharedStanding) -> &'static str {
    match standing {
        SharedStanding::Waiting => words::together::waiting(),
        SharedStanding::Reached => words::together::up_to_date(),
        SharedStanding::Unreached => words::together::server_not_reached(),
        SharedStanding::ServerTooOld => words::together::server_too_old(),
        SharedStanding::TurnedAway(_) => words::together::turned_away(),
        SharedStanding::NoServer => words::together::no_server(),
    }
}

fn told_invites(list: &SharedShown) {
    if let Some(edit) = list
        .invites
        .edit_page
        .as_ref()
        .or(list.invites.edit.as_ref())
    {
        said!("{}\t{edit}", words::together::to_edit());
    }
    said!(
        "{}\t{}",
        words::together::to_follow(),
        list.invites
            .view_page
            .as_ref()
            .unwrap_or(&list.invites.view)
    );
}

fn named_for_the_run(config: &Config, name: Option<&str>) -> Config {
    Config {
        shared_name: name
            .map(str::to_owned)
            .or_else(|| config.shared_name.clone()),
        ..config.clone()
    }
}

pub(crate) fn run(
    config: &Config,
    settings: &std::path::Path,
    library: Arc<Library>,
    args: &SharedArgs,
) -> Result<()> {
    if let Some(name) = &args.name {
        config::store(settings, ConfigKey::SharedName, name.clone())?;
        said!("{}", words::together::seen_as(name));
    }

    let named_here = named_for_the_run(config, args.name.as_deref());
    let Some(together) = start(&named_here, &library)? else {
        return Err(if config.syncs() {
            Error::SharedBusy
        } else {
            Error::SyncOff
        });
    };
    let shared = &together.shared;

    if let Some(name) = &args.name {
        shared.named(name.clone());
    }

    if let Some(name) = &args.share {
        let playlist = library
            .playlist_named(name)?
            .ok_or_else(|| Error::NoSuchPlaylist(PlaylistName::new(name.as_str())))?;
        let at = LocalPlaylistId {
            id: playlist.id.get().cast_signed(),
            created: library.playlist_stamp(playlist.id)?,
        };

        shared.share(at).map_err(Error::Sync)?;

        told_invites(&settled_at(shared, SETTLES_WITHIN, at)?);
    } else if let Some(link) = &args.join {
        let invitation = invitation_in(link);
        let (code, server) = invitation
            .as_ref()
            .map_or((link.as_str(), None), |invitation| {
                (invitation.code.as_str(), invitation.server.clone())
            });

        let space = shared.join(code, server).map_err(Error::Sync)?;
        let list = settled_in(shared, SETTLES_WITHIN, space)?;

        if list.may_edit {
            said!("{}", words::together::may_edit(&list.name));
        } else {
            said!("{}", words::together::follows(&list.name));
        }
    } else if let Some(name) = &args.leave {
        let list = listed_as(&library, shared, name)?;

        shared.leave(list.space).map_err(Error::Sync)?;
        settled_leaving(shared, SETTLES_WITHIN, list.space)?;
        said!("{}", words::together::left(&list.name));
    } else if let Some(person) = &args.remove_person {
        let name = args.from.as_deref().ok_or(Error::SharedNeedsAList)?;
        let list = listed_as(&library, shared, name)?;
        let removing = person_named(&list, person)?;

        shared
            .reset(list.space, Some(removing.member))
            .map_err(Error::Sync)?;
        given_time_to_settle(shared);

        let called = if removing.name.is_empty() {
            person.as_str()
        } else {
            removing.name.as_str()
        };

        said!("{}", words::together::removed(name, called));
        told_invites(&listed_as(&library, shared, name)?);
    } else if let Some(name) = &args.reset {
        let list = listed_as(&library, shared, name)?;

        shared.reset(list.space, None).map_err(Error::Sync)?;
        given_time_to_settle(shared);
        said!("{}", words::together::reset(name));
        told_invites(&listed_as(&library, shared, name)?);
    } else {
        given_time_to_settle(shared);
        print_every_list(&shared.shown());
    }

    Ok(())
}

fn person_named<'list>(list: &'list SharedShown, person: &str) -> Result<&'list PersonShown> {
    let others = || list.people.iter().filter(|held| !held.me);

    if let Some(by_id) = others().find(|held| held.member.to_string().eq_ignore_ascii_case(person))
    {
        return Ok(by_id);
    }

    let namesakes: Vec<&PersonShown> = others()
        .filter(|held| held.name.eq_ignore_ascii_case(person))
        .collect();
    match namesakes.as_slice() {
        [] => Err(Error::NoSuchPersonOnTheList(person.into())),
        [only] => Ok(only),
        _ => Err(Error::NamesakesOnTheList {
            name: person.into(),
            members: namesakes.iter().map(|held| held.member).collect(),
        }),
    }
}

fn print_every_list(lists: &[SharedShown]) {
    if lists.is_empty() {
        said!("{}", words::together::none_shared());
        return;
    }

    let mut table = Table::new(vec![
        words::together::playlist(),
        words::together::you_heading(),
        words::together::people(),
        words::together::not_held_here(),
        words::together::standing(),
    ]);

    for list in lists {
        table.push(vec![
            list.name.clone(),
            if list.may_edit {
                words::together::edit()
            } else {
                words::together::follow()
            }
            .to_owned(),
            list.people
                .iter()
                .map(|person| {
                    if person.me {
                        words::together::you().to_owned()
                    } else if person.name.is_empty() {
                        words::together::someone().to_owned()
                    } else {
                        person.name.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
            list.unheld.len().to_string(),
            said_of(list.standing).to_owned(),
        ]);
    }
    said_on!("{}", table.render());
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "ui")]
    use std::ffi::OsString;

    use super::*;

    #[cfg(feature = "ui")]
    #[test]
    fn an_argument_that_invites_is_not_queued_and_a_file_is() {
        let arguments = [
            OsString::from("a song.flac"),
            OsString::from("resonate://party/ABCDE"),
            OsString::from("subsonic:track/12"),
        ];

        let (kept, arrivals) = invitations_among(&arguments);

        assert_eq!(kept.len(), 2, "a party's link is no song either");
        assert_eq!(arrivals.0, ["resonate://party/ABCDE"]);
    }

    #[test]
    fn the_name_typed_for_a_run_is_the_one_it_starts_with_and_none_typed_keeps_the_settings() {
        let settings = Config {
            shared_name: Some("Old".to_owned()),
            ..Config::default()
        };

        let typed = named_for_the_run(&settings, Some("New"));
        let untyped = named_for_the_run(&settings, None);
        let unset = named_for_the_run(&Config::default(), None);

        assert_eq!(typed.shared_name.as_deref(), Some("New"));
        assert_eq!(untyped.shared_name.as_deref(), Some("Old"));
        assert_eq!(unset.shared_name, None);
    }

    #[cfg(feature = "ui")]
    fn naming(server: Option<&str>) -> Invitation {
        Invitation {
            to: Invited::Playlist,
            code: "E-ABCDE".to_owned(),
            server: server.map(str::to_owned),
        }
    }

    #[cfg(feature = "ui")]
    #[test]
    fn a_link_names_another_server_only_where_it_is_not_the_one_this_device_uses() {
        *HOME_SERVER.lock() = None;

        let stranger = naming(Some("wss://stranger.example/"));

        assert!(names_the_server_in_use(&naming(None)));
        assert!(
            names_the_server_in_use(&naming(Some(HOSTED_SERVER))),
            "a page of the hosted server names it"
        );
        assert!(!names_the_server_in_use(&stranger));

        *HOME_SERVER.lock() = Some("wss://stranger.example".to_owned());

        assert!(names_the_server_in_use(&stranger));
        assert!(!names_the_server_in_use(&naming(Some(HOSTED_SERVER))));

        *HOME_SERVER.lock() = None;
    }

    fn a_list_with(people: &[(u8, &str, bool)]) -> SharedShown {
        SharedShown {
            space: resonate_sync::SpaceId([1; 16]),
            at: None,
            name: "Mix".to_owned(),
            may_edit: true,
            invites: resonate_sync::shared::Invites {
                edit: None,
                view: String::new(),
                edit_page: None,
                view_page: None,
            },
            people: people
                .iter()
                .map(|(id, name, me)| PersonShown {
                    member: MemberId([*id; 16]),
                    name: (*name).to_owned(),
                    me: *me,
                })
                .collect(),
            unheld: Vec::new(),
            standing: SharedStanding::Reached,
            owed: 0,
        }
    }

    #[test]
    fn removing_a_name_two_people_share_is_refused_and_names_both_so_either_may_be_picked_by_id() {
        let list = a_list_with(&[(1, "Ada", true), (2, "Ada", false), (3, "ada", false)]);

        let refused = person_named(&list, "Ada");
        let picked = person_named(&list, &MemberId([3; 16]).to_string());

        assert!(matches!(
            refused,
            Err(Error::NamesakesOnTheList { ref members, .. })
                if *members == [MemberId([2; 16]), MemberId([3; 16])]
        ));
        assert_eq!(picked.map(|held| held.member).ok(), Some(MemberId([3; 16])));
    }

    #[test]
    fn removing_a_name_one_other_person_has_takes_them_and_never_the_one_removing() {
        let list = a_list_with(&[(1, "Ada", true), (2, "Ada", false), (3, "Grace", false)]);

        let taken = person_named(&list, "ada");
        let nobody = person_named(&list, "Linus");
        let not_me = person_named(&list, &MemberId([1; 16]).to_string());

        assert_eq!(taken.map(|held| held.member).ok(), Some(MemberId([2; 16])));
        assert!(matches!(nobody, Err(Error::NoSuchPersonOnTheList(_))));
        assert!(matches!(not_me, Err(Error::NoSuchPersonOnTheList(_))));
    }

    fn a_shared_catalog_holding(name: &str) -> (Arc<Library>, LocalPlaylistId) {
        let library = Arc::new(Library::open_in_memory().expect("a catalog"));
        let playlist = library.create_playlist(name).expect("a playlist");

        let at = LocalPlaylistId {
            id: playlist.get().cast_signed(),
            created: library.playlist_stamp(playlist).expect("a stamp"),
        };

        (library, at)
    }

    fn running_toward(server: &str, library: &Arc<Library>) -> (Together, std::path::PathBuf) {
        let folder = crate::fixtures::scratch("together");
        let config = Config {
            sync_server: Some(server.to_owned()),
            ..Config::default()
        };

        let together = start_in(&folder, &config, library)
            .expect("the shared playlists start")
            .expect("nothing else holds them");

        (together, folder)
    }

    const NO_SERVER_HERE: &str = "ws://127.0.0.1:9/";

    const A_LOOK_AT_A_STOPPED_SERVER: Duration = Duration::from_millis(800);

    #[test]
    fn a_playlist_shared_toward_a_stopped_server_is_never_settled_so_no_link_is_told() {
        let (library, at) = a_shared_catalog_holding("Mix");
        let (together, folder) = running_toward(NO_SERVER_HERE, &library);

        together.shared.share(at).expect("shared offline");

        let settled = settled_at(&together.shared, A_LOOK_AT_A_STOPPED_SERVER, at);

        drop(together);
        fs::remove_dir_all(folder).ok();

        assert!(matches!(
            settled,
            Err(Error::SharedNotSettled {
                standing: SharedStanding::Unreached
            })
        ));
    }

    #[test]
    fn a_list_left_toward_a_stopped_server_is_not_settled_while_the_leaving_is_owed() {
        let (library, at) = a_shared_catalog_holding("Mix");
        let (together, folder) = running_toward(NO_SERVER_HERE, &library);

        together.shared.share(at).expect("shared offline");

        let space = together.shared.shown()[0].space;

        together.shared.leave(space).expect("left offline");

        let settled = settled_leaving(&together.shared, A_LOOK_AT_A_STOPPED_SERVER, space);

        drop(together);
        fs::remove_dir_all(folder).ok();

        assert!(matches!(
            settled,
            Err(Error::SharedNotSettled {
                standing: SharedStanding::Unreached
            })
        ));
    }

    #[test]
    fn a_list_is_settled_only_once_the_server_has_been_reached_and_nothing_is_owed() {
        let mut list = a_list_with(&[(1, "Ada", true)]);

        assert!(matches!(looked_at(Some(list.clone())), Looked::Settled(_)));

        list.owed = 2;

        assert!(matches!(
            looked_at(Some(list.clone())),
            Looked::Standing(SharedStanding::Waiting)
        ));

        list.standing = SharedStanding::Unreached;

        assert!(matches!(
            looked_at(Some(list)),
            Looked::Standing(SharedStanding::Unreached)
        ));
        assert!(matches!(
            looked_at(None),
            Looked::Standing(SharedStanding::Waiting)
        ));
    }

    #[test]
    fn a_standing_the_server_gave_for_good_is_not_waited_out() {
        assert!(not_to_be_waited_out(SharedStanding::ServerTooOld));
        assert!(not_to_be_waited_out(SharedStanding::NoServer));
        assert!(not_to_be_waited_out(SharedStanding::TurnedAway(
            resonate_sync::wire::Refusal::Token
        )));
        assert!(!not_to_be_waited_out(SharedStanding::Unreached));
        assert!(!not_to_be_waited_out(SharedStanding::Waiting));
    }

    #[test]
    fn a_lock_another_resonate_holds_is_told_from_a_folder_that_cannot_be_read() {
        let folder = crate::fixtures::scratch("together-lock");
        let missing = folder.join("not-here");

        let first = alone_in(&folder).expect("the lock is free");
        let second = alone_in(&folder).expect("a held lock is no failure");
        let unreadable = alone_in(&missing);

        fs::remove_dir_all(&folder).ok();

        assert!(first.is_some());
        assert!(second.is_none());
        assert!(matches!(unreadable, Err(Error::SyncLock { .. })));
    }

    #[cfg(feature = "ui")]
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    #[cfg(feature = "ui")]
    impl std::io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().extend_from_slice(bytes);

            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[cfg(feature = "ui")]
    #[test]
    fn a_state_folder_that_cannot_be_read_says_why_together_did_not_start() {
        let folder = crate::fixtures::scratch("together-unreadable");
        let missing = folder.join("not-here");
        let library = Arc::new(Library::open_in_memory().expect("a catalog"));

        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let _listening = tracing::subscriber::set_default(subscriber);

        let together = started(
            start_in(&missing, &Config::default(), &library),
            invitations_among(&[]).1,
        );
        let said = String::from_utf8_lossy(&captured.0.lock()).into_owned();

        fs::remove_dir_all(&folder).ok();

        assert!(together.is_none());
        assert!(said.contains("shared playlists could not start"), "{said}");
        assert!(said.contains("could not be locked for syncing"), "{said}");
        assert!(said.contains("not-here"), "{said}");
        assert!(said.contains("No such file or directory"), "{said}");
    }

    #[cfg(feature = "ui")]
    #[test]
    fn a_launchs_links_are_judged_against_the_server_the_launch_runs_on() {
        let (library, _) = a_shared_catalog_holding("Mix");
        let folder = crate::fixtures::scratch("together-launch");
        let home = "wss://home.example";
        let config = Config {
            sync_server: Some(home.to_owned()),
            ..Config::default()
        };
        let own = format!(
            "resonate://shared/E-ABCDE?server={}",
            "wss%3A%2F%2Fhome.example"
        );
        let hosted = "https://resonate.tecnio.dev/s/#E-FGHIJ";

        let (_, arrivals) = invitations_among(&[own.clone().into(), hosted.into()]);

        let together = started(start_in(&folder, &config, &library), arrivals);

        drop(together);
        fs::remove_dir_all(&folder).ok();

        assert_eq!(crate::party_room::offers_waiting(), [hosted]);
    }

    #[cfg(feature = "ui")]
    #[test]
    fn a_launchs_links_are_still_judged_when_together_could_not_start() {
        let (library, _) = a_shared_catalog_holding("Mix");
        let folder = crate::fixtures::scratch("together-launch-failed");
        let party = "resonate://party/ABCDE";
        let (_, arrivals) = invitations_among(&[party.into()]);

        let together = started(
            start_in(&folder.join("not-here"), &Config::default(), &library),
            arrivals,
        );

        fs::remove_dir_all(&folder).ok();

        assert!(together.is_none());
        assert_eq!(crate::party_room::offers_waiting(), [party]);
    }

    #[test]
    fn nothing_running_shares_nothing_so_a_personal_look_leaves_the_notes_alone() {
        assert!(shares_here().is_none());
    }
}
