#[cfg(feature = "ui")]
use std::ffi::OsString;
use std::{
    fs::File,
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
use resonate_sync::party::{
    PartyChat, PartyHanding, PartyReach, PartyReaction, SittingOut, Standing,
};
use resonate_sync::{
    HOSTED_SERVER, HeldShare, LocalPlaylistId, Refused,
    shared::{
        Invitation, Invited, LocalShared, MemberId, Shared, SharedCatalog, SharedSettings,
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
    let folder = config::private_data_dir()?;
    let alone = folder.join(SHARED_ALONE);
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&alone)
        .map_err(|source| Error::SyncLock {
            path: alone.clone(),
            source,
        })?;
    if file.try_lock().is_err() {
        return Ok(None);
    }

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
                .map(|asked| (asked.song.recording.title, asked.song.recording.artist))
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

    fn play_request_next(&self, at: usize) -> bool {
        crate::party_room::play_a_request_next(at)
    }

    fn dismiss_request(&self, at: usize) {
        crate::party_room::dismiss_a_request(at);
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
pub(crate) fn invitations_taken_out(arguments: &[OsString]) -> Vec<OsString> {
    arguments
        .iter()
        .filter(|argument| {
            let text = argument.to_string_lossy();
            if invitation_in(&text).is_none() {
                return true;
            }
            accepted(&text, Arrival::FromOutside);
            false
        })
        .cloned()
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arrival {
    #[cfg(feature = "ui")]
    Pasted,
    FromOutside,
}

pub(crate) fn accepted(text: &str, arrival: Arrival) -> bool {
    let Some(invitation) = invitation_in(text) else {
        return false;
    };
    if invitation.to == Invited::Party {
        #[cfg(feature = "ui")]
        return match arrival {
            Arrival::Pasted => crate::party_room::accept(text),
            Arrival::FromOutside => crate::party_room::offer(text),
        };
        #[cfg(not(feature = "ui"))]
        return {
            let _ = arrival;
            false
        };
    }

    if arrival == Arrival::FromOutside && another_server_named(&invitation).is_some() {
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

fn another_server_named(invitation: &Invitation) -> Option<&str> {
    let home = HOME_SERVER.lock().clone();

    invitation.naming_a_server_other_than(home.as_deref().unwrap_or(HOSTED_SERVER))
}

fn settled(shared: &Shared) {
    let deadline = Instant::now() + SETTLES_WITHIN;
    shared.force();
    while Instant::now() < deadline {
        let lists = shared.shown();
        if lists
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
        settled(shared);
        told_invites(&listed_as(&library, shared, name)?);
    } else if let Some(link) = &args.join {
        let invitation = invitation_in(link);
        let (code, server) = invitation
            .as_ref()
            .map_or((link.as_str(), None), |invitation| {
                (invitation.code.as_str(), invitation.server.clone())
            });
        let space = shared.join(code, server).map_err(Error::Sync)?;
        settled(shared);
        let list = shared.shown().into_iter().find(|list| list.space == space);
        match list {
            Some(list) if list.may_edit => said!("{}", words::together::may_edit(&list.name)),
            Some(list) => said!("{}", words::together::follows(&list.name)),
            None => said!("{}", words::together::joined()),
        }
    } else if let Some(name) = &args.leave {
        let list = listed_as(&library, shared, name)?;
        shared.leave(list.space).map_err(Error::Sync)?;
        settled(shared);
        said!("{}", words::together::left(&list.name));
    } else if let Some(person) = &args.remove_person {
        let name = args.from.as_deref().ok_or(Error::SharedNeedsAList)?;
        let list = listed_as(&library, shared, name)?;
        let member = person_named(&list, person)?;
        shared
            .reset(list.space, Some(member))
            .map_err(Error::Sync)?;
        settled(shared);
        said!("{}", words::together::removed(name, person));
        told_invites(&listed_as(&library, shared, name)?);
    } else if let Some(name) = &args.reset {
        let list = listed_as(&library, shared, name)?;
        shared.reset(list.space, None).map_err(Error::Sync)?;
        settled(shared);
        said!("{}", words::together::reset(name));
        told_invites(&listed_as(&library, shared, name)?);
    } else {
        settled(shared);
        print_every_list(&shared.shown());
    }

    Ok(())
}

fn person_named(list: &SharedShown, person: &str) -> Result<MemberId> {
    list.people
        .iter()
        .find(|held| !held.me && held.name.eq_ignore_ascii_case(person))
        .map(|held| held.member)
        .ok_or_else(|| Error::NoSuchPersonOnTheList(person.into()))
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

        let kept = invitations_taken_out(&arguments);

        assert_eq!(kept.len(), 2, "a party's link is no song either");
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

    fn naming(server: Option<&str>) -> Invitation {
        Invitation {
            to: Invited::Playlist,
            code: "E-ABCDE".to_owned(),
            server: server.map(str::to_owned),
        }
    }

    #[test]
    fn a_link_names_another_server_only_where_it_is_not_the_one_this_device_uses() {
        *HOME_SERVER.lock() = None;
        let stranger = naming(Some("wss://stranger.example/"));

        assert_eq!(another_server_named(&naming(None)), None);
        assert_eq!(
            another_server_named(&naming(Some(HOSTED_SERVER))),
            None,
            "a page of the hosted server names it"
        );
        assert_eq!(
            another_server_named(&stranger),
            Some("wss://stranger.example/")
        );

        *HOME_SERVER.lock() = Some("wss://stranger.example".to_owned());

        assert_eq!(another_server_named(&stranger), None);
        assert_eq!(
            another_server_named(&naming(Some(HOSTED_SERVER))),
            Some(HOSTED_SERVER)
        );

        *HOME_SERVER.lock() = None;
    }

    #[test]
    fn nothing_running_shares_nothing_so_a_personal_look_leaves_the_notes_alone() {
        assert!(shares_here().is_none());
    }
}
