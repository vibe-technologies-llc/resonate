use std::{
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use resonate_core::{ListenId, Mbid};
use resonate_eq::Corrections;
use resonate_library::{
    Billed, LookupOp, Love, Loved, LovedNames, Reference, Scrobble, Scrobbler, TokenHeld,
};
use rusqlite::{OptionalExtension, params};

use crate::{Error, MobilePlayer, Result, StoreOp, Track, catalog::read};

#[derive(Clone, Debug, uniffi::Record)]
pub struct AutoEqDevice {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct EqPreset {
    pub name: String,
    pub text: String,
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

    pub fn release_songs(&self, id: String) -> Result<Vec<crate::features::SearchSong>> {
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
        Ok(release
            .media
            .iter()
            .flat_map(|medium| medium.tracks.iter())
            .map(|track| crate::features::SearchSong {
                recording: track
                    .recording
                    .as_ref()
                    .map_or_else(String::new, ToString::to_string),
                title: track.title.clone(),
                artist: track.artist.clone().unwrap_or_else(|| artist.clone()),
                album: release.title.clone(),
                duration_ms: track
                    .length
                    .map(|length| length.as_millis().min(u128::from(u64::MAX)) as u64),
                isrcs: track.isrcs.clone(),
            })
            .collect())
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

    pub fn auto_eq_profile(&self, id: String) -> Result<Option<EqPreset>> {
        self.require_online()?;
        let id = resonate_eq::DeviceId::new(&id)?;
        let catalogue = self.services.auto_eq.catalogue()?;
        let name = catalogue
            .get(&id)
            .map_or_else(|| id.as_str().to_owned(), |device| device.label.clone());
        Ok(self.services.auto_eq.profile(&id)?.map(|profile| EqPreset {
            name,
            text: resonate_eq::write_profile(&profile),
        }))
    }

    pub fn parse_eq_profile(&self, bytes: Vec<u8>, name: String) -> Result<EqPreset> {
        let profile = resonate_eq::read_profile(&resonate_core::text::decoded(&bytes).0)?;
        Ok(EqPreset {
            name,
            text: resonate_eq::write_profile(&profile),
        })
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

    pub fn send_favourites(&self, account: ScrobbleAccount) -> Result<u64> {
        self.require_online()?;
        let favourites = {
            let connection = self.catalog.0.lock();
            let mut query = connection.prepare("SELECT * FROM documents WHERE favourite = 1 AND recording IS NOT NULL ORDER BY id")
                .map_err(|source| Error::store(StoreOp::Read, source))?;
            query
                .query_map([], read)
                .map_err(|source| Error::store(StoreOp::Read, source))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|source| Error::store(StoreOp::Read, source))?
        };
        let mut sent = 0;
        for client in self.scrobblers(account) {
            for track in &favourites {
                let Some(recording) = track.recording.as_deref().and_then(|id| Mbid::new(id).ok())
                else {
                    continue;
                };
                client
                    .love(
                        &Loved {
                            recording,
                            named: Some(LovedNames {
                                title: track.title.clone(),
                                artist: track.artist.clone(),
                            }),
                        },
                        Love::Loved,
                    )
                    .map_err(|source| Error::reference(LookupOp::Love, source))?;
                sent += 1;
            }
        }
        Ok(sent)
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
    fn scrobblers(&self, account: ScrobbleAccount) -> Vec<Box<dyn Scrobbler>> {
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
