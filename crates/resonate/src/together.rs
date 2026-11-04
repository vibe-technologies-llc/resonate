use std::{
    ffi::OsString,
    fs::{self, File},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::{PlaylistId, Recording};
use resonate_library::{Library, PlaylistName, SyncedPlaylist};
use resonate_sync::{
    HeldShare, LocalPlaylistId, Refused,
    shared::{
        Invited, LocalShared, MemberId, Shared, SharedCatalog, SharedSettings, SharedShown,
        SharedStanding, invitation_in,
    },
};

use crate::{
    Error, Result,
    cli::SharedArgs,
    config::{self, Config},
    error::ConfigKey,
    sync::server_of,
    table::Table,
};

const SHARED_STORE: &str = "shared.sqlite";

const SHARED_ALONE: &str = "shared.lock";

const SETTLES_WITHIN: Duration = Duration::from_secs(20);

const SETTLED_LOOKED_AT_EVERY: Duration = Duration::from_millis(100);

static RUNNING: Mutex<Option<Arc<Shared>>> = Mutex::new(None);

pub(crate) struct Together {
    shared: Arc<Shared>,
    _alone: File,
}

impl Drop for Together {
    fn drop(&mut self) {
        *RUNNING.lock() = None;
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
    let folder = config::data_dir()?;
    fs::create_dir_all(&folder).map_err(|source| Error::CreateDir {
        path: folder.clone(),
        source,
    })?;
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

    let settings = SharedSettings {
        server: server_of(config),
        trusted: config.sync_server_certificate.clone(),
        store: folder.join(SHARED_STORE),
        name: config.shared_name.clone().unwrap_or_default(),
    };
    let catalog = Arc::new(Listed {
        library: Arc::clone(library),
    });
    let shared = Arc::new(Shared::start(settings, catalog).map_err(Error::Sync)?);
    *RUNNING.lock() = Some(Arc::clone(&shared));

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
        self.library.playlists_shared().unwrap_or_default()
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
        accepted(invitation)
    }

    fn named(&self, name: &str) {
        if let Some(shared) = running() {
            shared.named(name.to_owned());
        }
    }
}

pub(crate) fn invitations_taken_out(arguments: &[OsString]) -> Vec<OsString> {
    arguments
        .iter()
        .filter(|argument| !accepted(&argument.to_string_lossy()))
        .cloned()
        .collect()
}

pub(crate) fn accepted(text: &str) -> bool {
    let Some(invitation) = invitation_in(text) else {
        return false;
    };
    if invitation.to != Invited::Playlist {
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
        SharedStanding::Waiting => "waiting",
        SharedStanding::Reached => "up to date",
        SharedStanding::Unreached => "server not reached",
        SharedStanding::ServerTooOld => "server too old",
        SharedStanding::TurnedAway(_) => "turned away by the server",
        SharedStanding::NoServer => "no server",
    }
}

fn told_invites(list: &SharedShown) {
    if let Some(edit) = list
        .invites
        .edit_page
        .as_ref()
        .or(list.invites.edit.as_ref())
    {
        said!("To edit\t{edit}");
    }
    said!(
        "To follow\t{}",
        list.invites
            .view_page
            .as_ref()
            .unwrap_or(&list.invites.view)
    );
}

pub(crate) fn run(
    config: &Config,
    settings: &std::path::Path,
    library: Arc<Library>,
    args: &SharedArgs,
) -> Result<()> {
    if let Some(name) = &args.name {
        config::store(settings, ConfigKey::SharedName, name.clone())?;
        said!("Others will see you as {name}");
    }

    let Some(together) = start(config, &library)? else {
        return Err(if config.syncs() {
            Error::SharedBusy
        } else {
            Error::SyncOff
        });
    };
    let shared = &together.shared;

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
            Some(list) if list.may_edit => said!("You can now edit {}", list.name),
            Some(list) => said!("You now follow {}", list.name),
            None => said!("Joined; the list arrives once the server is reached"),
        }
    } else if let Some(name) = &args.leave {
        let list = listed_as(&library, shared, name)?;
        shared.leave(list.space).map_err(Error::Sync)?;
        settled(shared);
        said!(
            "You left {}; the songs stay here as your own playlist",
            list.name
        );
    } else if let Some(person) = &args.remove_person {
        let name = args.from.as_deref().ok_or(Error::SharedNeedsAList)?;
        let list = listed_as(&library, shared, name)?;
        let member = person_named(&list, person)?;
        shared
            .reset(list.space, Some(member))
            .map_err(Error::Sync)?;
        settled(shared);
        said!("{person} no longer reaches {name}; everyone else carries on");
        told_invites(&listed_as(&library, shared, name)?);
    } else if let Some(name) = &args.reset {
        let list = listed_as(&library, shared, name)?;
        shared.reset(list.space, None).map_err(Error::Sync)?;
        settled(shared);
        said!("The old links to {name} reach nothing now");
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
        said!("No playlist is shared with anyone");
        return;
    }

    let mut table = Table::new(vec![
        "PLAYLIST",
        "YOU",
        "PEOPLE",
        "NOT HELD HERE",
        "STANDING",
    ]);
    for list in lists {
        table.push(vec![
            list.name.clone(),
            if list.may_edit { "edit" } else { "follow" }.to_owned(),
            list.people
                .iter()
                .map(|person| {
                    if person.me {
                        "you".to_owned()
                    } else if person.name.is_empty() {
                        "someone".to_owned()
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
    use std::ffi::OsString;

    use super::*;

    #[test]
    fn an_argument_that_invites_is_not_queued_and_a_file_is() {
        let arguments = [
            OsString::from("a song.flac"),
            OsString::from("resonate://party/ABCDE"),
            OsString::from("subsonic:track/12"),
        ];

        let kept = invitations_taken_out(&arguments);

        assert_eq!(kept.len(), 3, "a party is left for the party to take up");
    }

    #[test]
    fn nothing_running_shares_nothing_so_a_personal_look_leaves_the_notes_alone() {
        assert!(shares_here().is_none());
    }
}
