use std::{sync::Arc, time::Duration};

use parking_lot::Mutex;
use resonate_core::{MediaLocation, Recording, Resumable, Span, ZoneId};
use resonate_engine::{Command, Moment, Placement, Player, QueueItem, Seat, Sounding};
use resonate_providers::{PlaysInPlace, found_exactly};
use resonate_sync::{
    DeviceId, HOSTED_SERVER, LocalPlaylistId,
    party::{
        Going, Mine, Party, PartyFinder, PartyReach, PartySeat, PartySettings, PartySong,
        SeatHeard, SittingOut, Standing,
    },
    shared::{Invited, MemberId, SharedShown, SharedStanding, invitation_in},
};
use rusqlite::OptionalExtension;

use crate::{
    Error, MobilePlayer, Result,
    catalog::Catalog,
    documents::location,
    error::SyncOp,
    served,
    sync::{RECORDING_COLUMNS, Resolving, recording_at},
};

const SONGS_TOLD_AHEAD: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum InvitationKind {
    Playlist,
    Party,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TogetherPerson {
    pub id: Vec<u8>,
    pub name: String,
    pub is_me: bool,
    pub is_host: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SharedPlaylistReach {
    Waiting,
    Reached,
    Unreached,
    ServerTooOld,
    TurnedAway,
    NoServer,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SharedPlaylist {
    pub playlist: Option<u64>,
    pub name: String,
    pub may_edit: bool,
    pub edit_link: Option<String>,
    pub follow_link: String,
    pub people: Vec<TogetherPerson>,
    pub not_held: Vec<String>,
    pub reach: SharedPlaylistReach,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PartyConnection {
    Connecting,
    Here,
    Reconnecting,
    Ended,
    NoSuchParty,
    TurnedAway,
    ServerTooOld,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PartyFollowing {
    WaitingForTheHost,
    Finding,
    NotHeld,
    CannotStartMidSong,
    SatOut,
    HostPaused,
    InStep,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PartyRequest {
    pub title: String,
    pub artist: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ListeningParty {
    pub hosting: bool,
    pub link: String,
    pub connection: PartyConnection,
    pub people: Vec<TogetherPerson>,
    pub following: PartyFollowing,
    pub song: Option<PartyRequest>,
    pub requests: Vec<PartyRequest>,
    pub round_trip_ms: Option<u64>,
}

#[derive(Default)]
pub(crate) struct Together {
    party: Mutex<Option<Room>>,
}

struct Room {
    going: Going,
    zone: ZoneId,
    player: Arc<Player>,
    finder: Arc<Finder>,
}

struct PhoneSeat {
    seat: Seat,
    catalog: Arc<Catalog>,
    served: Arc<served::Served>,
}

struct Finder {
    resolve: Resolving,
    services: Vec<Arc<dyn PlaysInPlace>>,
}

#[uniffi::export]
pub fn invitation_kind(text: String) -> Option<InvitationKind> {
    invitation_in(&text).map(|invitation| match invitation.to {
        Invited::Playlist => InvitationKind::Playlist,
        Invited::Party => InvitationKind::Party,
    })
}

fn song_of(
    catalog: &Catalog,
    served: &served::Served,
    location: &MediaLocation,
    span: Option<resonate_core::FrameSpan>,
) -> Option<PartySong> {
    let row = resonate_core::QueueStamp::of([(location, span)]).value();

    if served::Served::names(location) {
        let known = served.song_at(location);

        return Some(PartySong {
            row,
            recording: Recording {
                mbid: None,
                title: known.title,
                artist: known.artist.unwrap_or_default(),
                album: known.album.unwrap_or_default(),
                length: known.duration_ms.map(Duration::from_millis),
            },
            isrc: None,
        });
    }

    let recording = catalog
        .0
        .lock()
        .query_row(
            &format!("SELECT {RECORDING_COLUMNS} FROM documents d WHERE d.uri = ?1"),
            [location.to_uri()],
            |held| recording_at(held, 0),
        )
        .optional()
        .ok()
        .flatten()?;

    Some(PartySong {
        row,
        recording,
        isrc: None,
    })
}

impl PartySeat for PhoneSeat {
    type Row = Resumable;

    fn heard(&self) -> SeatHeard {
        let heard = self.seat.heard();

        SeatHeard {
            mine: match heard.sounding {
                Sounding::Playing => Mine::Playing,
                Sounding::Paused => Mine::Paused,
                Sounding::Stopped => Mine::Stopped,
            },
            row: heard.row.map(resonate_core::QueueStamp::value),
            position_nanos: heard.position.map(Seat::nanos_of),
            at_nanos: heard.at.nanos(),
            coarse: heard.holds_seconds_downstream,
        }
    }

    fn stamp_of(&self, row: &Resumable) -> u64 {
        Seat::stamp_of(row).value()
    }

    fn songs_up_next(&self) -> (Option<PartySong>, Vec<PartySong>) {
        let player = self.seat.player();
        let state = player.state();
        let queued = player.queued();
        let Some(at) = state.queue_position else {
            return (None, Vec::new());
        };
        let song = queued
            .rows
            .get(at)
            .and_then(|item| song_of(&self.catalog, &self.served, &item.location, item.span));
        let next = (at + 1..at + 1 + SONGS_TOLD_AHEAD)
            .filter_map(|row| queued.rows.get(row))
            .filter_map(|item| song_of(&self.catalog, &self.served, &item.location, item.span))
            .collect();

        (song, next)
    }

    fn added_here(&self, known: &[u64]) -> Vec<PartySong> {
        let player = self.seat.player();
        let queued = player.queued();
        let mut added = Vec::new();

        for (row, item) in queued.rows.iter().enumerate().rev() {
            let stamp = resonate_core::QueueStamp::of([(&item.location, item.span)]).value();
            if known.contains(&stamp) {
                continue;
            }
            if let Some(song) = song_of(&self.catalog, &self.served, &item.location, item.span) {
                added.push(song);
            }
            if let Err(error) = player.send(Command::Remove(Span::one(row))) {
                tracing::warn!(%error, "a song added during a party was left in the queue");
            }
        }

        added
    }

    fn take_up(&self, rows: &[Resumable], position: u64, at: u64, sounding: bool) -> bool {
        self.seat
            .take_up(
                rows,
                Duration::from_nanos(position),
                Moment::from_nanos(at),
                sounding,
            )
            .inspect_err(|error| tracing::debug!(%error, "a party's song did not start here"))
            .unwrap_or(false)
    }

    fn keep_ahead(&self, rows: &[Resumable]) -> bool {
        self.seat
            .keep_ahead(rows)
            .inspect_err(|error| tracing::debug!(%error, "a party's queue ahead was not kept"))
            .unwrap_or(false)
    }

    fn nudge(&self, position: u64, at: u64) {
        let nudged = self
            .seat
            .nudge(Duration::from_nanos(position), Moment::from_nanos(at));
        if let Err(error) = nudged {
            tracing::debug!(%error, "a party could not move back into step");
        }
    }

    fn seek(&self, to: u64) {
        if let Err(error) = self.seat.seek(Duration::from_nanos(to)) {
            tracing::debug!(%error, "a party could not seek to its host");
        }
    }

    fn pause(&self) {
        if let Err(error) = self.seat.pause() {
            tracing::debug!(%error, "a party could not pause");
        }
    }

    fn play(&self) {
        if let Err(error) = self.seat.play() {
            tracing::debug!(%error, "a party could not play");
        }
    }

    fn put_back(&self) {
        if let Err(error) = self.seat.put_back() {
            tracing::warn!(%error, "the queue from before a party was not put back");
        }
    }
}

impl PartyFinder for Finder {
    type Row = Resumable;

    fn found(&self, song: &PartySong) -> Option<Resumable> {
        let anywhere = Recording {
            album: String::new(),
            ..song.recording.clone()
        };
        let held = (self.resolve)(&song.recording).or_else(|| (self.resolve)(&anywhere));
        if let Some(uri) = held
            && let Ok(location) = location(&uri)
        {
            return Some(Resumable {
                location,
                span: None,
                held: None,
            });
        }

        found_exactly(&self.services, &song.recording).map(|location| Resumable {
            location,
            span: None,
            held: None,
        })
    }
}

fn people_of(list: &SharedShown) -> Vec<TogetherPerson> {
    list.people
        .iter()
        .map(|person| TogetherPerson {
            id: person.member.0.to_vec(),
            name: person.name.clone(),
            is_me: person.me,
            is_host: false,
        })
        .collect()
}

fn shared_of(list: SharedShown) -> SharedPlaylist {
    let people = people_of(&list);

    SharedPlaylist {
        playlist: list.at.map(|at| at.id.cast_unsigned()),
        name: list.name,
        may_edit: list.may_edit,
        edit_link: list.invites.edit_page.or(list.invites.edit),
        follow_link: list.invites.view_page.unwrap_or(list.invites.view),
        people,
        not_held: list
            .unheld
            .into_iter()
            .map(|recording| recording.title)
            .collect(),
        reach: match list.standing {
            SharedStanding::Waiting => SharedPlaylistReach::Waiting,
            SharedStanding::Reached => SharedPlaylistReach::Reached,
            SharedStanding::Unreached => SharedPlaylistReach::Unreached,
            SharedStanding::ServerTooOld => SharedPlaylistReach::ServerTooOld,
            SharedStanding::TurnedAway(_) => SharedPlaylistReach::TurnedAway,
            SharedStanding::NoServer => SharedPlaylistReach::NoServer,
        },
    }
}

fn member_from(bytes: &[u8]) -> Option<[u8; 16]> {
    bytes.try_into().ok()
}

#[uniffi::export]
impl MobilePlayer {
    pub fn shared_playlists(&self) -> Vec<SharedPlaylist> {
        self.syncing
            .shared()
            .map(|shared| shared.shown().into_iter().map(shared_of).collect())
            .unwrap_or_default()
    }

    pub fn share_playlist(&self, id: u64) -> Result<SharedPlaylist> {
        let shared = self.syncing.shared().ok_or(Error::SyncOff)?;
        let at = self.synced_playlist(id)?;

        shared
            .share(at)
            .map_err(|source| Error::sync(SyncOp::Start, source))?;

        self.a_shared_list_at(at).ok_or(Error::UnknownPlaylist)
    }

    pub fn invitation_naming_another_server(&self, text: String) -> Option<String> {
        let invitation = invitation_in(&text).filter(|one| one.to == Invited::Playlist)?;

        invitation
            .naming_a_server_other_than(&self.own_server())
            .map(str::to_owned)
    }

    pub fn accept_invitation(&self, text: String) -> Result<Option<InvitationKind>> {
        let Some(invitation) = invitation_in(&text) else {
            return Ok(None);
        };

        match invitation.to {
            Invited::Playlist => {
                let shared = self.syncing.shared().ok_or(Error::SyncOff)?;
                shared
                    .join(&invitation.code, invitation.server)
                    .map_err(|source| Error::sync(SyncOp::Start, source))?;

                Ok(Some(InvitationKind::Playlist))
            }
            Invited::Party => {
                self.party_join(&invitation.code, invitation.server)?;

                Ok(Some(InvitationKind::Party))
            }
        }
    }

    pub fn shared_leave(&self, id: u64) -> Result<()> {
        let (shared, list) = self.a_list_held(id)?;

        shared
            .leave(list.space)
            .map_err(|source| Error::sync(SyncOp::Forget, source))
    }

    pub fn shared_reset(&self, id: u64, removing: Option<Vec<u8>>) -> Result<SharedPlaylist> {
        let (shared, list) = self.a_list_held(id)?;
        let removing = removing.as_deref().and_then(member_from).map(MemberId);

        shared
            .reset(list.space, removing)
            .map_err(|source| Error::sync(SyncOp::Start, source))?;
        let at = self.synced_playlist(id)?;

        self.a_shared_list_at(at).ok_or(Error::UnknownPlaylist)
    }

    pub fn shared_watch(&self, id: Option<u64>) {
        let Some(shared) = self.syncing.shared() else {
            return;
        };
        let space = id
            .and_then(|id| self.a_list_held(id).ok())
            .map(|(_, list)| list.space);

        shared.watch(space);
    }

    pub fn together_server(&self, server: String) {
        let server = server.trim();
        *self.syncing.chosen_server.lock() = (!server.is_empty()).then(|| server.to_owned());
    }

    pub fn together_named(&self, name: String) {
        *self.syncing.shared_name.lock() = name.clone();
        if let Some(shared) = self.syncing.shared() {
            shared.named(name.clone());
        }
        if let Some(room) = self.together.party.lock().as_ref() {
            room.going.party().named(name);
        }
    }

    pub fn party_begin(&self) -> Result<ListeningParty> {
        let settings = self.party_settings(None)?;
        let party = Party::host(settings).map_err(|source| Error::sync(SyncOp::Start, source))?;
        self.party_seated(party)?;

        self.party_shown().ok_or(Error::SyncOff)
    }

    pub fn party_leave(&self) {
        let room = self.together.party.lock().take();
        drop(room);
    }

    pub fn party_shown(&self) -> Option<ListeningParty> {
        let guard = self.together.party.lock();
        let room = guard.as_ref()?;
        let shown = room.going.shown();
        let followed = room.going.followed();

        Some(ListeningParty {
            hosting: shown.hosting,
            link: shown.page.unwrap_or(shown.link),
            connection: match shown.reach {
                PartyReach::Connecting => PartyConnection::Connecting,
                PartyReach::Here => PartyConnection::Here,
                PartyReach::Reconnecting => PartyConnection::Reconnecting,
                PartyReach::Ended => PartyConnection::Ended,
                PartyReach::NoSuchParty => PartyConnection::NoSuchParty,
                PartyReach::TurnedAway => PartyConnection::TurnedAway,
                PartyReach::ServerTooOld => PartyConnection::ServerTooOld,
            },
            people: shown
                .people
                .into_iter()
                .map(|person| TogetherPerson {
                    id: person.member.0.to_vec(),
                    name: person.name,
                    is_me: person.me,
                    is_host: person.host,
                })
                .collect(),
            following: match followed.standing {
                Standing::WaitingForTheHost => PartyFollowing::WaitingForTheHost,
                Standing::Finding => PartyFollowing::Finding,
                Standing::SittingOut(SittingOut::NotHeld) => PartyFollowing::NotHeld,
                Standing::SittingOut(SittingOut::CannotStartMidSong) => {
                    PartyFollowing::CannotStartMidSong
                }
                Standing::SittingOut(SittingOut::AskedTo) => PartyFollowing::SatOut,
                Standing::HostPaused => PartyFollowing::HostPaused,
                Standing::InStep => PartyFollowing::InStep,
            },
            song: shown.now.and_then(|now| now.song).map(|song| PartyRequest {
                title: song.recording.title,
                artist: song.recording.artist,
            }),
            requests: shown
                .asked_for
                .into_iter()
                .map(|asked| PartyRequest {
                    title: asked.song.recording.title,
                    artist: asked.song.recording.artist,
                })
                .collect(),
            round_trip_ms: shown
                .round_trip
                .map(|trip| u64::try_from(trip.as_millis()).unwrap_or(u64::MAX)),
        })
    }

    pub fn party_turn_out(&self, person: Vec<u8>) {
        let Some(member) = member_from(&person) else {
            return;
        };
        if let Some(room) = self.together.party.lock().as_ref() {
            room.going.party().turn_out(DeviceId(member));
        }
    }

    pub fn party_play_next(&self, request: u32) -> bool {
        let at = request as usize;
        let Some((party, player, finder, asked)) =
            self.together.party.lock().as_ref().and_then(|room| {
                let asked = room.going.shown().asked_for.get(at).cloned()?;

                Some((
                    Arc::clone(room.going.party()),
                    Arc::clone(&room.player),
                    Arc::clone(&room.finder),
                    asked,
                ))
            })
        else {
            return false;
        };
        let Some(_claim) = party.claim_a_request(&asked) else {
            return true;
        };
        let Some(row) = finder.found(&asked.song) else {
            return false;
        };
        let queued = player.queued();
        let id = resonate_engine::unclaimed_id(queued.rows.iter().map(AsRef::as_ref));
        let placed = player.send(Command::Insert {
            items: vec![QueueItem {
                id,
                location: row.location,
                span: row.span,
            }],
            at: Placement::Next,
            play: false,
        });
        party.take_a_request(asked);

        placed.is_ok()
    }

    pub fn party_pass_over(&self, request: u32) {
        if let Some(room) = self.together.party.lock().as_ref()
            && let Some(asked) = room.going.shown().asked_for.get(request as usize).cloned()
        {
            room.going.party().take_a_request(asked);
        }
    }
}

impl MobilePlayer {
    fn a_shared_list_at(&self, at: LocalPlaylistId) -> Option<SharedPlaylist> {
        self.syncing
            .shared()?
            .shown()
            .into_iter()
            .find(|list| list.at == Some(at))
            .map(shared_of)
    }

    fn a_list_held(&self, id: u64) -> Result<(Arc<resonate_sync::shared::Shared>, SharedShown)> {
        let shared = self.syncing.shared().ok_or(Error::SyncOff)?;
        let at = self.synced_playlist(id)?;
        let list = shared
            .shown()
            .into_iter()
            .find(|list| list.at == Some(at))
            .ok_or(Error::UnknownPlaylist)?;

        Ok((shared, list))
    }

    fn own_server(&self) -> String {
        self.syncing
            .server()
            .or_else(|| self.syncing.chosen_server.lock().clone())
            .unwrap_or_else(|| HOSTED_SERVER.to_owned())
    }

    fn party_settings(&self, server: Option<String>) -> Result<PartySettings> {
        self.require_online()?;

        let server = server.unwrap_or_else(|| self.own_server());

        Ok(PartySettings {
            server,
            trusted: None,
            name: self.syncing.shared_name.lock().clone(),
            now_nanos: Arc::new(|| Moment::now().nanos()),
        })
    }

    fn party_join(&self, code: &str, server: Option<String>) -> Result<()> {
        let settings = self.party_settings(server)?;
        let party =
            Party::join(settings, code).map_err(|source| Error::sync(SyncOp::Start, source))?;

        self.party_seated(party)
    }

    fn party_seated(&self, party: Party) -> Result<()> {
        self.party_leave();
        let zone = self.playing.shown();
        let player = self.playing.player_of(zone)?;
        let seat = PhoneSeat {
            seat: Seat::on(Arc::clone(&player)),
            catalog: Arc::clone(&self.catalog),
            served: Arc::clone(&self.served),
        };
        let finder = Arc::new(Finder {
            resolve: self.syncing.resolver(),
            services: self.served.playing_services(),
        });
        let going = Going::begin(party, seat, Arc::clone(&finder))
            .map_err(|source| Error::sync(SyncOp::Start, source))?;

        *self.together.party.lock() = Some(Room {
            going,
            zone,
            player,
            finder,
        });

        Ok(())
    }

    pub(crate) fn party_following(&self, zone: ZoneId) -> bool {
        self.together
            .party
            .lock()
            .as_ref()
            .is_some_and(|room| room.zone == zone && !room.going.shown().hosting)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pasted_invitation_is_told_apart_from_anything_else() {
        assert_eq!(
            invitation_kind("resonate://shared/E-ABCDE".to_owned()),
            Some(InvitationKind::Playlist)
        );
        assert_eq!(
            invitation_kind("https://resonate.tecnio.dev/p/#ABCDE".to_owned()),
            Some(InvitationKind::Party)
        );
        assert_eq!(invitation_kind("love song".to_owned()), None);
    }

    #[test]
    fn a_shared_list_naming_a_server_other_than_the_phones_own_is_told_with_that_server() {
        let phone = crate::playlists::tests::player();
        let stranger = "resonate://shared/E-ABCDE?server=wss%3A%2F%2Fstranger.example%2F";
        let ask = |text: &str| phone.invitation_naming_another_server(text.to_owned());

        assert_eq!(ask("resonate://shared/E-ABCDE"), None);
        assert_eq!(ask("https://resonate.tecnio.dev/s/#E-ABCDE"), None);
        assert_eq!(ask(stranger), Some("wss://stranger.example/".to_owned()));
        assert_eq!(
            ask("resonate://party/ABCDE?server=wss%3A%2F%2Fstranger.example%2F"),
            None
        );

        phone.together_server("wss://stranger.example".to_owned());

        assert_eq!(ask(stranger), None);
    }

    #[test]
    fn a_party_is_refused_while_online_is_off() {
        let phone = crate::playlists::tests::player();

        phone.configure_online(false);

        assert!(matches!(phone.party_begin(), Err(Error::OnlineOff)));
        assert!(phone.party_shown().is_none());
    }

    #[test]
    fn a_party_reaches_the_server_chosen_even_while_sync_has_never_run() {
        let phone = crate::playlists::tests::player();

        phone.configure_online(true);

        let hosted = phone.party_settings(None).expect("settings").server;
        phone.together_server(" https://mine.example/ ".to_owned());

        assert_eq!(
            phone.party_settings(None).expect("settings").server,
            "https://mine.example/"
        );
        assert_eq!(
            phone
                .party_settings(Some("https://invited.example/".to_owned()))
                .expect("settings")
                .server,
            "https://invited.example/"
        );

        phone.together_server("  ".to_owned());

        assert_eq!(phone.party_settings(None).expect("settings").server, hosted);
    }

    #[test]
    fn a_member_is_read_only_from_the_length_a_member_is() {
        assert_eq!(member_from(&[1; 16]), Some([1; 16]));
        assert_eq!(member_from(&[1; 15]), None);
    }
}
