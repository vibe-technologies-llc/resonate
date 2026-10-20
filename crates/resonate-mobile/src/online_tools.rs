use std::{
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use resonate_core::{ListenId, Mbid};
use resonate_eq::Corrections;
use resonate_library::{Billed, LookupOp, Reference, Scrobble, Scrobbler, TokenHeld};
use rusqlite::{OptionalExtension, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track,
    catalog::read,
    equaliser::{EqualiserImport, imported},
    features::{Listing, SearchSong},
};

#[derive(Clone, Debug, uniffi::Record)]
pub struct AutoEqDevice {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SearchAlbum {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub date: String,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct SearchDiscography {
    pub albums: Vec<SearchAlbum>,
    pub next: Option<u32>,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct SearchArtist {
    pub id: String,
    pub name: String,
    pub kind: String,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct SearchRelease {
    pub id: String,
    pub title: String,
    pub date: String,
    pub country: String,
    pub songs: u32,
}

#[derive(Clone, uniffi::Record)]
pub struct ScrobbleAccount {
    pub listenbrainz_token: String,
    pub lastfm_key: String,
    pub lastfm_secret: String,
    pub lastfm_session: String,
}

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum ScrobbleService {
    ListenBrainz,
    LastFm,
}

impl ScrobbleService {
    fn name(self) -> &'static str {
        match self {
            Self::ListenBrainz => "listenbrainz",
            Self::LastFm => "lastfm",
        }
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn search_albums_online(&self, text: String) -> Result<Vec<SearchAlbum>> {
        self.require_online()?;
        self.services
            .online
            .find_albums(&text)
            .map_err(|source| Error::reference(LookupOp::FindReleaseGroup, source))
            .map(|albums| {
                albums
                    .into_iter()
                    .take(50)
                    .map(|album| SearchAlbum {
                        id: album.group.to_string(),
                        artist: album.credited_as(),
                        title: album.title,
                        date: album.first_released.unwrap_or_default(),
                    })
                    .collect()
            })
    }

    pub fn search_artists_online(&self, text: String) -> Result<Vec<SearchArtist>> {
        self.require_online()?;
        self.services
            .online
            .find_artist(&text)
            .map_err(|source| Error::reference(LookupOp::FindArtist, source))
            .map(|artists| {
                artists
                    .into_iter()
                    .take(50)
                    .map(|artist| SearchArtist {
                        id: artist.mbid.to_string(),
                        name: artist.name,
                        kind: artist.kind.unwrap_or_default(),
                    })
                    .collect()
            })
    }

    pub fn artist_albums(&self, id: String, from: u32) -> Result<SearchDiscography> {
        self.require_online()?;
        let discography = self
            .services
            .online
            .release_groups_of(&Mbid::new(&id)?, from)
            .map_err(|source| Error::reference(LookupOp::ReleaseGroupsOfArtist, source))?;
        Ok(SearchDiscography {
            next: (discography.unread > 0 && discography.read_to > from)
                .then_some(discography.read_to),
            albums: discography
                .releases
                .into_iter()
                .map(|album| SearchAlbum {
                    id: album.mbid.to_string(),
                    title: album.title,
                    artist: String::new(),
                    date: album.first_released.unwrap_or_default(),
                })
                .collect(),
        })
    }

    pub fn online_releases(&self, group: String) -> Result<Vec<SearchRelease>> {
        self.require_online()?;
        self.services
            .online
            .releases_of_group(&Mbid::new(&group)?)
            .map_err(|source| Error::reference(LookupOp::ReleasesOfGroup, source))
            .map(|releases| {
                releases
                    .into_iter()
                    .map(|release| SearchRelease {
                        songs: release.track_count(),
                        id: release.id.to_string(),
                        title: release.title,
                        date: release.date.unwrap_or_default(),
                        country: release.country.unwrap_or_default(),
                    })
                    .collect()
            })
    }

    pub fn release_songs(&self, id: String) -> Result<Vec<SearchSong>> {
        self.require_online()?;
        let Some(release) = self
            .services
            .online
            .release(&Mbid::new(&id)?)
            .map_err(|source| Error::reference(LookupOp::Release, source))?
        else {
            return Ok(Vec::new());
        };
        let artist = release.credited_as();
        let artist_id = release
            .credit
            .first()
            .and_then(|credit| credit.mbid.as_ref())
            .map_or_else(String::new, ToString::to_string);
        let songs = release
            .media
            .iter()
            .flat_map(|medium| {
                medium
                    .tracks
                    .iter()
                    .map(move |track| (medium.position, track))
            })
            .map(|(disc, track)| SearchSong {
                recording: track
                    .recording
                    .as_ref()
                    .map_or_else(String::new, ToString::to_string),
                title: track.title.clone(),
                artist: track.artist.clone().unwrap_or_else(|| artist.clone()),
                artist_id: if track.artist.is_none() {
                    artist_id.clone()
                } else {
                    String::new()
                },
                album: release.title.clone(),
                release: release.id.to_string(),
                release_group: release
                    .group
                    .as_ref()
                    .map_or_else(String::new, ToString::to_string),
                disc: Some(disc),
                position: Some(track.position),
                duration_ms: track
                    .length
                    .map(|length| length.as_millis().min(u128::from(u64::MAX)) as u64),
                isrcs: track.isrcs.clone(),
                held: false,
            })
            .collect();
        self.offered(songs, Listing::Listed)
    }

    pub fn search_auto_eq(&self, text: String) -> Result<Vec<AutoEqDevice>> {
        self.require_online()?;
        let catalogue = self.services.auto_eq.catalogue()?;
        Ok(resonate_eq::search(&catalogue, &text)
            .into_iter()
            .take(50)
            .filter_map(|found| catalogue.device(found))
            .map(|device| AutoEqDevice {
                id: device.id.as_str().to_owned(),
                name: device.shown(),
            })
            .collect())
    }

    pub fn auto_eq_profile(&self, id: String) -> Result<Option<EqualiserImport>> {
        self.require_online()?;
        let id = resonate_eq::DeviceId::new(&id)?;
        let catalogue = self.services.auto_eq.catalogue()?;
        let name = catalogue
            .get(&id)
            .map_or_else(|| id.as_str().to_owned(), |device| device.label.clone());
        Ok(self
            .services
            .auto_eq
            .profile(&id)?
            .map(|profile| imported(&profile, name)))
    }

    pub fn start_scrobbling(&self, service: ScrobbleService, include_history: bool) -> Result<()> {
        self.catalog.0.lock().execute("INSERT INTO submissions(service, through) VALUES (?1, CASE WHEN ?2 THEN 0 ELSE (SELECT coalesce(max(id), 0) FROM listens) END) ON CONFLICT(service) DO UPDATE SET through = excluded.through", params![service.name(), include_history]).map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub fn validate_scrobbling(&self, account: ScrobbleAccount) -> Result<Vec<String>> {
        self.require_online()?;
        self.scrobblers(account)
            .into_iter()
            .map(|client| {
                match client
                    .token_held()
                    .map_err(|source| Error::reference(LookupOp::Token, source))?
                {
                    TokenHeld::By(name) => Ok(format!("{}: {name}", client.service().name())),
                    TokenHeld::Unknown => Err(Error::UnknownAccount),
                }
            })
            .collect()
    }

    pub fn lastfm_sign_in(
        &self,
        key: String,
        secret: String,
        user: String,
        password: String,
    ) -> Result<String> {
        self.require_online()?;
        resonate_online::signed_in(
            self.services.online.client(),
            &resonate_online::Application { key, secret },
            &user,
            &password,
        )
        .map(|session| session.key)
        .map_err(|source| Error::reference(LookupOp::Token, source))
    }

    pub fn submit_scrobbles(&self, account: ScrobbleAccount, playing: Option<u64>) -> Result<u64> {
        self.require_online()?;
        let mut submitted = 0;
        let mut failure = None;
        for client in self.scrobblers(account) {
            let told = self.submit_to(client.as_ref(), playing);
            match told {
                Ok(count) => submitted += count,
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        if let Some(failure) = failure {
            return Err(failure);
        }
        Ok(submitted)
    }
}

impl MobilePlayer {
    pub(crate) fn scrobblers(&self, account: ScrobbleAccount) -> Vec<Box<dyn Scrobbler>> {
        let mut clients: Vec<Box<dyn Scrobbler>> = Vec::new();
        if !account.listenbrainz_token.is_empty() {
            clients.push(Box::new(resonate_online::ListenBrainz::new(
                Arc::clone(self.services.online.client()),
                account.listenbrainz_token,
            )));
        }
        if !account.lastfm_key.is_empty()
            && !account.lastfm_secret.is_empty()
            && !account.lastfm_session.is_empty()
        {
            clients.push(Box::new(resonate_online::LastFm::new(
                Arc::clone(self.services.online.client()),
                resonate_online::Application {
                    key: account.lastfm_key,
                    secret: account.lastfm_secret,
                },
                account.lastfm_session,
            )));
        }
        clients
    }

    fn submit_to(&self, client: &dyn Scrobbler, playing: Option<u64>) -> Result<u64> {
        let service = client.service().name();
        let (through, listens) = {
            let connection = self.catalog.0.lock();
            let through: Option<i64> = connection
                .query_row(
                    "SELECT through FROM submissions WHERE service = ?1",
                    [service],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Read, source))?;
            let through = through.unwrap_or(i64::MAX);
            let mut query = connection.prepare("SELECT d.*, l.id AS listen_id, l.began AS listen_began FROM listens l JOIN documents d ON d.id = l.track WHERE l.counted = 1 AND l.elsewhere = 0 AND l.id > ?1 ORDER BY l.id LIMIT 100").map_err(|source| Error::store(StoreOp::Read, source))?;
            let found = query
                .query_map([through], |row| {
                    Ok((
                        read(row)?,
                        row.get::<_, i64>("listen_id")?,
                        row.get::<_, i64>("listen_began")?,
                    ))
                })
                .map_err(|source| Error::store(StoreOp::Read, source))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|source| Error::store(StoreOp::Read, source))?;
            (through, found)
        };
        let scrobbles = listens
            .iter()
            .map(|(track, id, began)| {
                Ok(Scrobble {
                    listen: ListenId::new(*id as u64)?,
                    at: UNIX_EPOCH + Duration::from_secs((*began).max(0) as u64),
                    billed: billed(track),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if !scrobbles.is_empty() {
            client
                .submit(&scrobbles)
                .map_err(|source| Error::reference(LookupOp::Submit, source))?;
            let last = listens.last().map_or(through, |(_, id, _)| *id);
            self.catalog
                .0
                .lock()
                .execute(
                    "UPDATE submissions SET through = max(through, ?2) WHERE service = ?1",
                    params![service, last],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        if let Some(id) = playing {
            client
                .playing_now(&billed(&self.catalog.track(id)?))
                .map_err(|source| Error::reference(LookupOp::Submit, source))?;
        }
        Ok(scrobbles.len() as u64)
    }
}

fn billed(track: &Track) -> Billed {
    Billed {
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: (!track.album.is_empty()).then(|| track.album.clone()),
        recording: track.recording.as_deref().and_then(|id| Mbid::new(id).ok()),
        release: track.release.as_deref().and_then(|id| Mbid::new(id).ok()),
        release_group: None,
        artist_mbid: track
            .artist_mbid
            .as_deref()
            .and_then(|id| Mbid::new(id).ok()),
        isrc: None,
        number: track.track_number,
        length: (track.duration_ms > 0).then_some(Duration::from_millis(track.duration_ms)),
    }
}

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;
    use resonate_library::{Error as LibraryError, ListeningService, Love, Loved};

    use super::*;
    use crate::harness::Harness;

    struct Told {
        service: ListeningService,
        submitted: Mutex<Vec<Vec<u64>>>,
        playing: Mutex<Vec<String>>,
        fails: Mutex<bool>,
        refuses_playing: Mutex<bool>,
    }

    impl Told {
        fn new(service: ListeningService) -> Self {
            Self {
                service,
                submitted: Mutex::new(Vec::new()),
                playing: Mutex::new(Vec::new()),
                fails: Mutex::new(false),
                refuses_playing: Mutex::new(false),
            }
        }

        fn failing(&self, fails: bool) {
            *self.fails.lock() = fails;
        }

        fn refusal(&self) -> resonate_library::Result<()> {
            if *self.fails.lock() {
                return Err(LibraryError::Refused {
                    op: LookupOp::Submit,
                    status: 503,
                });
            }
            Ok(())
        }
    }

    impl Scrobbler for Told {
        fn service(&self) -> ListeningService {
            self.service
        }

        fn submit(&self, listens: &[Scrobble]) -> resonate_library::Result<()> {
            self.refusal()?;
            self.submitted
                .lock()
                .push(listens.iter().map(|listen| listen.listen.get()).collect());
            Ok(())
        }

        fn playing_now(&self, playing: &Billed) -> resonate_library::Result<()> {
            if *self.refuses_playing.lock() {
                return Err(LibraryError::Refused {
                    op: LookupOp::Submit,
                    status: 429,
                });
            }
            self.playing.lock().push(playing.title.clone());
            Ok(())
        }

        fn love(&self, _: &Loved, _: Love) -> resonate_library::Result<()> {
            Ok(())
        }

        fn token_held(&self) -> resonate_library::Result<TokenHeld> {
            Ok(TokenHeld::Unknown)
        }
    }

    fn listened(harness: &Harness, song: u64, counted: bool, elsewhere: bool, times: usize) {
        let connection = harness.player.catalog.0.lock();
        for began in 0..times {
            connection
                .execute(
                    "INSERT INTO listens(track, began, heard_ms, counted, elsewhere) VALUES (?1, ?2, 120000, ?3, ?4)",
                    params![song as i64, 1_000 + began as i64, counted, elsewhere],
                )
                .expect("a listen");
        }
    }

    fn through(harness: &Harness, service: ScrobbleService) -> Option<i64> {
        harness
            .player
            .catalog
            .0
            .lock()
            .query_row(
                "SELECT through FROM submissions WHERE service = ?1",
                [service.name()],
                |row| row.get(0),
            )
            .optional()
            .expect("a mark")
    }

    #[test]
    fn listens_go_a_hundred_at_a_time_in_order_and_the_mark_moves_with_each_batch() {
        let harness = Harness::new();
        let song = harness.song('a').id;
        listened(&harness, song, true, false, 150);
        harness
            .player
            .start_scrobbling(ScrobbleService::ListenBrainz, true)
            .expect("a mark begun");
        let client = Told::new(ListeningService::ListenBrainz);

        let first = harness.player.submit_to(&client, None).expect("a batch");
        assert_eq!(first, 100);
        assert_eq!(through(&harness, ScrobbleService::ListenBrainz), Some(100));

        let second = harness.player.submit_to(&client, None).expect("a batch");
        assert_eq!(second, 50);
        assert_eq!(through(&harness, ScrobbleService::ListenBrainz), Some(150));

        assert_eq!(harness.player.submit_to(&client, None).expect("none"), 0);
        let sent = client.submitted.lock().clone();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0], (1..=100).collect::<Vec<u64>>());
        assert_eq!(sent[1], (101..=150).collect::<Vec<u64>>());
    }

    #[test]
    fn a_refused_batch_leaves_the_mark_where_it_was_and_is_sent_again() {
        let harness = Harness::new();
        let song = harness.song('a').id;
        listened(&harness, song, true, false, 3);
        harness
            .player
            .start_scrobbling(ScrobbleService::LastFm, true)
            .expect("a mark begun");
        let client = Told::new(ListeningService::LastFm);

        client.failing(true);
        assert!(harness.player.submit_to(&client, None).is_err());
        assert_eq!(through(&harness, ScrobbleService::LastFm), Some(0));

        client.failing(false);
        assert_eq!(harness.player.submit_to(&client, None).expect("sent"), 3);
        assert_eq!(through(&harness, ScrobbleService::LastFm), Some(3));
    }

    #[test]
    fn only_counted_listens_of_this_device_are_sent_and_history_only_where_asked_for() {
        let harness = Harness::new();
        let song = harness.song('b').id;
        listened(&harness, song, true, false, 2);
        listened(&harness, song, false, false, 2);
        listened(&harness, song, true, true, 2);
        let client = Told::new(ListeningService::ListenBrainz);

        assert_eq!(
            harness.player.submit_to(&client, None).expect("no mark"),
            0,
            "a service never begun sent the history"
        );

        harness
            .player
            .start_scrobbling(ScrobbleService::ListenBrainz, false)
            .expect("a mark begun");
        assert_eq!(harness.player.submit_to(&client, None).expect("sent"), 0);

        listened(&harness, song, true, false, 1);
        listened(&harness, song, true, true, 1);
        assert_eq!(harness.player.submit_to(&client, None).expect("sent"), 1);

        harness
            .player
            .start_scrobbling(ScrobbleService::ListenBrainz, true)
            .expect("a mark begun");
        assert_eq!(harness.player.submit_to(&client, None).expect("sent"), 3);
    }

    #[test]
    fn each_service_keeps_its_own_mark() {
        let harness = Harness::new();
        let song = harness.song('c').id;
        listened(&harness, song, true, false, 4);
        for service in [ScrobbleService::ListenBrainz, ScrobbleService::LastFm] {
            harness
                .player
                .start_scrobbling(service, true)
                .expect("a mark begun");
        }
        let brainz = Told::new(ListeningService::ListenBrainz);
        let last = Told::new(ListeningService::LastFm);

        brainz.failing(false);
        last.failing(true);
        assert_eq!(harness.player.submit_to(&brainz, None).expect("sent"), 4);
        assert!(harness.player.submit_to(&last, None).is_err());

        assert_eq!(through(&harness, ScrobbleService::ListenBrainz), Some(4));
        assert_eq!(through(&harness, ScrobbleService::LastFm), Some(0));
    }

    #[test]
    fn a_song_playing_now_is_told_after_the_listens_and_its_refusal_keeps_their_mark() {
        let harness = Harness::new();
        let song = harness.song('d').id;
        listened(&harness, song, true, false, 2);
        harness
            .player
            .start_scrobbling(ScrobbleService::ListenBrainz, true)
            .expect("a mark begun");
        let client = Told::new(ListeningService::ListenBrainz);

        assert_eq!(
            harness
                .player
                .submit_to(&client, Some(song))
                .expect("sent and told"),
            2
        );
        assert_eq!(client.playing.lock().clone(), ["d.wav"]);

        listened(&harness, song, true, false, 1);
        *client.refuses_playing.lock() = true;
        assert!(harness.player.submit_to(&client, Some(song)).is_err());
        assert_eq!(through(&harness, ScrobbleService::ListenBrainz), Some(3));
    }
}
