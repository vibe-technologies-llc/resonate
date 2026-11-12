use std::{
    cell::Cell,
    time::{SystemTime, UNIX_EPOCH},
};

use ahash::AHashMap;
use resonate_core::{AlbumId, ArtistId, TrackId, WantId};
use rusqlite::{OptionalExtension as _, params, params_from_iter, types::Value};

use super::{
    Library,
    rows::{RawUnheldRelease, UnheldCoverDue, limit, rows},
    searched::{Infix, scoped_missing, unheld_holding},
    statements::{
        ALBUMS_NOT_HELD_BY_AN_ARTIST, ARTIST_MBID, ARTISTS_WITH_SONGS_DUE,
        MISSING_FROM_AN_ARTISTS_ALBUMS, MISSING_TRACKS_COUNTED, RELEASES_TRIED_AFTER_THE_FIRST,
        SHORT_OF_WHAT_IS_HELD_OR_WANTED, SONG_GROUP_STILL_DUE, SONG_HELD_NOWHERE,
        SONGS_DUE_FOR_AN_ARTIST, SONGS_IN_ORDER, SONGS_SOUGHT_AT_MOST, UNHELD_ALBUMS_KEPT,
        UNHELD_ALBUMS_KEPT_IN_ORDER, UNHELD_COUNTED, UNHELD_COVER_DUE, UNHELD_COVERS_DUE,
        UNHELD_IN_ORDER, UNHELD_RELEASES,
    },
};
use crate::{
    AlbumFound, AlbumNotHeld, ArtistFound, CoverArt, Covering, Cut, Error, Found, ImageFormat,
    Isrc, Learning, Mbid, Missing, REFRESH_AFTER, REFUSED_AGAIN_AFTER, Recording, RecordingMatch,
    Reference, Release, Result, StoreOp, Uncovered, UnheldRelease, elsewhere, enrich, enriched,
    learning,
    linked::{
        self, AlbumLink, ArtistHeldBy, ArtistLink, FollowedPlaylist, HeldBy, LinkNames, Linked,
        ListedSong, PlaylistLink, SongLink,
    },
    playlist::{self, Filled},
    songs, store,
};

impl Library {
    pub fn follow_link(&self, reference: &dyn Reference, link: &SongLink) -> Result<Linked> {
        match link {
            SongLink::MusicBrainz(id) => self.follow_recording(reference, id),
            SongLink::Deezer(_) | SongLink::Elsewhere(_) => {
                let Some(song) = reference.song_linked(link)? else {
                    return Ok(Linked::Unnamed);
                };
                self.follow_names(reference, &song, linked::Footage::of(link))
            }
        }
    }

    fn follow_recording(&self, reference: &dyn Reference, id: &Mbid) -> Result<Linked> {
        if let Some(held) = self
            .inner
            .read(|connection| linked::held_as(connection, HeldBy::Recording(id)))?
        {
            return Ok(held);
        }
        self.linked_to(reference.recording(id)?)
    }

    fn follow_names(
        &self,
        reference: &dyn Reference,
        song: &LinkNames,
        footage: linked::Footage,
    ) -> Result<Linked> {
        for isrc in &song.isrcs {
            if let Some(held) = self
                .inner
                .read(|connection| linked::held_as(connection, HeldBy::Isrc(isrc)))?
            {
                return Ok(held);
            }
        }
        let mut taken = None;
        for isrc in &song.isrcs {
            let takes = reference.recordings_of_isrc(isrc)?;
            taken = linked::the_take_linked(takes, song.length);
            if taken.is_some() {
                break;
            }
        }
        let recording = match taken {
            Some(take) => reference.recording(&take.id)?.or(Some(take)),
            None => linked::the_song_searched_for(reference, song, footage)?,
        };
        self.linked_to(recording)
    }

    fn linked_to(&self, recording: Option<Recording>) -> Result<Linked> {
        let Some(recording) = recording else {
            return Ok(Linked::Unnamed);
        };
        if let Some(held) = self
            .inner
            .read(|connection| linked::held_as(connection, HeldBy::Recording(&recording.id)))?
        {
            return Ok(held);
        }

        Ok(
            elsewhere::found_among(vec![RecordingMatch::from(recording)], None, |_| false)
                .into_iter()
                .next()
                .map_or(Linked::Unnamed, |found| Linked::Found(Box::new(found))),
        )
    }

    pub fn follow_playlist_link(
        &self,
        reference: &dyn Reference,
        link: &PlaylistLink,
    ) -> Result<Option<FollowedPlaylist>> {
        let Some(linked) = reference.playlist_linked(link)? else {
            return Ok(None);
        };
        let mut followed = FollowedPlaylist {
            name: linked.name,
            held: Vec::new(),
            found: Vec::new(),
            unnamed: 0,
        };
        for song in &linked.songs {
            let song = match song {
                ListedSong::Recording(id) => self.follow_recording(reference, id)?,
                ListedSong::Named(names) => {
                    self.follow_names(reference, names, linked::Footage::Heard)?
                }
                ListedSong::Uploaded(names) => {
                    self.follow_names(reference, names, linked::Footage::Filmed)?
                }
            };
            match song {
                Linked::Held { track, .. } => followed.held.push(track),
                Linked::Found(found) => followed.found.push(*found),
                Linked::HeldAlbum { .. }
                | Linked::Album { .. }
                | Linked::HeldArtist { .. }
                | Linked::Artist(_)
                | Linked::Unnamed => followed.unnamed += 1,
            }
        }

        Ok(Some(followed))
    }

    pub fn fill_playlist_named(&self, name: &str, tracks: &[TrackId]) -> Result<Filled> {
        let mut cuts = Vec::with_capacity(tracks.len());
        for track in tracks {
            if let Some(held) = self.track(*track)? {
                cuts.push(Cut::of(&held));
            }
        }
        playlist::fill(&self.inner, name, &cuts)
    }

    pub fn follow_album_link(&self, reference: &dyn Reference, link: &AlbumLink) -> Result<Linked> {
        let Some(named) = linked::album_named(reference, link)? else {
            return Ok(Linked::Unnamed);
        };
        if let Some(held) = self
            .inner
            .read(|connection| linked::album_held(connection, &named))?
        {
            return Ok(held);
        }

        Ok(named.linked())
    }

    pub fn follow_artist_link(
        &self,
        reference: &dyn Reference,
        link: &ArtistLink,
    ) -> Result<Linked> {
        if let ArtistLink::MusicBrainz(mbid) = link {
            return self.follow_artist_named(reference, mbid);
        }
        for page in link.pages() {
            if let Some(mbid) = reference.artist_at(&page)? {
                return self.follow_artist_named(reference, &mbid);
            }
        }
        if !matches!(link, ArtistLink::Deezer(_)) {
            return Ok(Linked::Unnamed);
        }
        let Some(name) = reference.artist_linked(link)? else {
            return Ok(Linked::Unnamed);
        };
        if let Some(held) = self
            .inner
            .read(|connection| linked::artist_held(connection, ArtistHeldBy::Name(&name)))?
        {
            return Ok(held);
        }
        Ok(enrich::top_artist(reference.find_artist(&name)?, &name)
            .map_or(Linked::Unnamed, |mbid| {
                Linked::Artist(ArtistFound { mbid, name })
            }))
    }

    fn follow_artist_named(&self, reference: &dyn Reference, mbid: &Mbid) -> Result<Linked> {
        if let Some(held) = self
            .inner
            .read(|connection| linked::artist_held(connection, ArtistHeldBy::Id(mbid)))?
        {
            return Ok(held);
        }
        Ok(reference.artist(mbid)?.map_or(Linked::Unnamed, |profile| {
            Linked::Artist(ArtistFound {
                mbid: profile.mbid,
                name: profile.name,
            })
        }))
    }

    pub fn want_found(&self, reference: &dyn Reference, found: &Found) -> Result<WantId> {
        let Uncovered { wanted, covering } = self.want_found_uncovered(reference, found)?;
        if let Some(covering) = covering {
            self.cover_what_was_wanted(reference, &covering);
        }
        Ok(wanted)
    }

    pub fn want_found_uncovered(
        &self,
        reference: &dyn Reference,
        found: &Found,
    ) -> Result<Uncovered<WantId>> {
        let releases: Vec<Mbid> = match &found.release {
            Some(release) => std::iter::once(release.id.clone())
                .chain(
                    found
                        .in_the_order_worth_offering()
                        .into_iter()
                        .map(|offered| offered.id.clone())
                        .filter(|offered| *offered != release.id),
                )
                .collect(),
            None => reference
                .recording(&found.recording)?
                .map(|recording| {
                    elsewhere::in_the_order_worth_offering(&recording.releases)
                        .into_iter()
                        .map(|release| release.id.clone())
                        .collect()
                })
                .unwrap_or_default(),
        };
        let Some((first, rest)) = releases.split_first() else {
            return Err(Error::Unreleased {
                recording: found.recording.clone(),
            });
        };

        let mut failed = match self.want_found_on(reference, found, first) {
            Ok(wanted) => return Ok(wanted),
            Err(error) => error,
        };
        for release in rest.iter().take(RELEASES_TRIED_AFTER_THE_FIRST) {
            if !matches!(
                failed,
                Error::UnknownRelease { .. } | Error::NotOnTheRelease { .. }
            ) {
                break;
            }
            tracing::debug!(%release, error = %failed, "a found song is wanted from its next release");
            failed = match self.want_found_on(reference, found, release) {
                Ok(wanted) => return Ok(wanted),
                Err(error) => error,
            };
        }
        Err(failed)
    }

    fn want_found_on(
        &self,
        reference: &dyn Reference,
        found: &Found,
        release: &Mbid,
    ) -> Result<Uncovered<WantId>> {
        let Uncovered { wanted, covering } = self.want_from_release(
            reference,
            release,
            std::slice::from_ref(&found.recording),
            &found.isrcs,
        )?;
        wanted
            .into_iter()
            .next()
            .map(|(_, want)| Uncovered {
                wanted: want,
                covering,
            })
            .ok_or_else(|| Error::NotOnTheRelease {
                recording: found.recording.clone(),
                release: release.clone(),
            })
    }

    pub fn want_album(
        &self,
        reference: &dyn Reference,
        group: &Mbid,
        pressing: Option<&Mbid>,
    ) -> Result<Vec<(Found, WantId)>> {
        let Uncovered { wanted, covering } =
            self.want_album_uncovered(reference, group, pressing)?;
        if let Some(covering) = covering {
            self.cover_what_was_wanted(reference, &covering);
        }
        Ok(wanted)
    }

    pub fn want_album_uncovered(
        &self,
        reference: &dyn Reference,
        group: &Mbid,
        pressing: Option<&Mbid>,
    ) -> Result<Uncovered<Vec<(Found, WantId)>>> {
        let named = pressing
            .map(|release| Self::release_named(reference, release))
            .transpose()?;
        if let Some(named) = &named {
            self.land_songs_of(group, Some(named), SystemTime::now())?;
        }
        let songs = self.songs_read_for(reference, group)?;
        let release = Self::pressing_of_songs(&songs, group)?;
        let recordings: Vec<Mbid> = songs.iter().map(|song| song.recording.clone()).collect();
        let landed = match named {
            Some(named) if named.id == release => named,
            _ => Self::release_named(reference, &release)?,
        };

        let Uncovered { wanted, covering } = self.want_from_landed(&landed, &recordings, &[])?;
        let wanted: AHashMap<Mbid, WantId> = wanted.into_iter().collect();
        Ok(Uncovered {
            wanted: songs
                .into_iter()
                .filter_map(|song| {
                    let want = *wanted.get(&song.recording)?;
                    Some((song, want))
                })
                .collect(),
            covering,
        })
    }

    pub fn open_album_uncovered(
        &self,
        reference: &dyn Reference,
        group: &Mbid,
    ) -> Result<Uncovered<AlbumId>> {
        let songs = self.songs_read_for(reference, group)?;
        let release = Self::pressing_of_songs(&songs, group)?;
        self.land_release_of(reference, &release)
    }

    fn songs_read_for(&self, reference: &dyn Reference, group: &Mbid) -> Result<Vec<Found>> {
        let songs = self.songs_of_group(group)?;
        if !songs.is_empty() {
            return Ok(songs);
        }
        self.learn_the_songs_of(reference, group)?;
        self.songs_of_group(group)
    }

    fn pressing_of_songs(songs: &[Found], group: &Mbid) -> Result<Mbid> {
        songs
            .first()
            .and_then(|song| song.release.as_ref())
            .map(|release| release.id.clone())
            .ok_or_else(|| Error::UnknownRelease {
                release: group.clone(),
            })
    }

    fn land_release_of(
        &self,
        reference: &dyn Reference,
        release: &Mbid,
    ) -> Result<Uncovered<AlbumId>> {
        let landed = reference
            .release(release)?
            .ok_or_else(|| Error::UnknownRelease {
                release: release.clone(),
            })?;
        let now = SystemTime::now();

        let album = self
            .inner
            .write(|transaction| elsewhere::album_of_release(transaction, &landed, now))?;
        Ok(Uncovered {
            wanted: album,
            covering: self.covering_of(album, &landed)?,
        })
    }

    fn covering_of(&self, album: AlbumId, landed: &Release) -> Result<Option<Covering>> {
        let wants_a_cover =
            (landed.has_front_cover || landed.group.is_some()) && self.cover_art(album)?.is_none();
        Ok(wants_a_cover.then(|| Covering {
            album,
            release: landed.id.clone(),
            group: landed.group.clone(),
        }))
    }

    pub fn cover_what_was_wanted(&self, reference: &dyn Reference, covering: &Covering) {
        let Covering {
            album,
            release,
            group,
        } = covering;
        match reference.cover(release, group.as_ref()) {
            Ok(Some(art)) => {
                if let Err(error) = self.land_archive_cover(*album, &art) {
                    tracing::warn!(%error, %album, "a cover for a found song was dropped");
                }
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, %album, "a cover for a found song did not arrive");
            }
        }
    }

    fn release_named(reference: &dyn Reference, release: &Mbid) -> Result<Release> {
        reference
            .release(release)?
            .ok_or_else(|| Error::UnknownRelease {
                release: release.clone(),
            })
    }

    fn want_from_release(
        &self,
        reference: &dyn Reference,
        release: &Mbid,
        recordings: &[Mbid],
        codes: &[Isrc],
    ) -> Result<Uncovered<Vec<(Mbid, WantId)>>> {
        let landed = Self::release_named(reference, release)?;
        self.want_from_landed(&landed, recordings, codes)
    }

    fn want_from_landed(
        &self,
        landed: &Release,
        recordings: &[Mbid],
        codes: &[Isrc],
    ) -> Result<Uncovered<Vec<(Mbid, WantId)>>> {
        let release = &landed.id;
        let now = SystemTime::now();

        let (wanted, album) = self.inner.write(|transaction| {
            let album = elsewhere::album_of_release(transaction, landed, now)?;
            let mut wanted = Vec::new();
            for recording in recordings {
                let Some(row) = elsewhere::release_track_of(transaction, album, recording)? else {
                    tracing::debug!(%recording, %release, "a song is not on the release it was wanted from");
                    continue;
                };
                let coded: Vec<String> = codes.iter().map(|code| code.as_str().to_owned()).collect();
                enriched::code(transaction, row.get() as i64, &coded)?;
                wanted.push((recording.clone(), elsewhere::want_in(transaction, row, now)?));
            }
            if wanted.is_empty()
                && let Some(recording) = recordings.first()
            {
                return Err(Error::NotOnTheRelease {
                    recording: recording.clone(),
                    release: release.clone(),
                });
            }
            Ok((wanted, album))
        })?;

        let covering = self.covering_of(album, landed)?;

        Ok(Uncovered { wanted, covering })
    }

    pub(crate) fn learn_the_songs_of(
        &self,
        reference: &dyn Reference,
        group: &Mbid,
    ) -> Result<usize> {
        let pressings = reference.releases_of_group(group)?;
        self.land_songs_of(
            group,
            songs::pressing_of(pressings).as_ref(),
            SystemTime::now(),
        )
    }

    pub fn learn_the_songs_of_artist(
        &self,
        reference: &dyn Reference,
        artist: ArtistId,
        told: &dyn Learning,
    ) -> Result<usize> {
        let due = self.songs_due_for(artist, SystemTime::now())?;
        let page = learning::PageLearning {
            told,
            landed: Cell::new(0),
        };
        learning::learn_the_songs_of(self, reference, &due, &page)?;
        Ok(page.landed.get())
    }

    fn songs_due_binds(now: SystemTime) -> Vec<Value> {
        let refused_before = now.checked_sub(REFUSED_AGAIN_AFTER).unwrap_or(UNIX_EPOCH);
        let read_before = now.checked_sub(REFRESH_AFTER).unwrap_or(UNIX_EPOCH);
        vec![
            Value::Integer(store::to_nanos(refused_before)),
            Value::Integer(store::to_nanos(read_before)),
        ]
    }

    pub(crate) fn songs_due_for(
        &self,
        artist: ArtistId,
        now: SystemTime,
    ) -> Result<songs::SongsDue> {
        let mut binds = Self::songs_due_binds(now);
        binds.push(Value::Integer(artist.get() as i64));

        self.inner.read(|connection| {
            let groups = rows(connection, SONGS_DUE_FOR_AN_ARTIST, binds, |row| {
                row.get::<_, String>(0)
                    .map(|mbid| Mbid::new(&mbid).map_err(Error::from))
            })?;
            let artist = connection
                .query_row(ARTIST_MBID, [artist.get() as i64], |row| {
                    row.get::<_, Option<String>>(0)
                })
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?
                .flatten()
                .and_then(|mbid| Mbid::new(&mbid).ok());
            Ok(songs::SongsDue { artist, groups })
        })
    }

    pub(crate) fn songs_still_due(&self, group: &Mbid, now: SystemTime) -> Result<bool> {
        let mut binds = Self::songs_due_binds(now);
        binds.push(Value::Text(group.as_str().to_owned()));

        self.inner.read(|connection| {
            connection
                .query_row(SONG_GROUP_STILL_DUE, params_from_iter(binds), |row| {
                    row.get::<_, bool>(0)
                })
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    pub(crate) fn artists_whose_songs_are_due(&self, now: SystemTime) -> Result<Vec<ArtistId>> {
        let binds = Self::songs_due_binds(now);

        self.inner.read(|connection| {
            rows(connection, ARTISTS_WITH_SONGS_DUE, binds, |row| {
                row.get::<_, i64>(0)
                    .map(|id| ArtistId::new(id as u64).map_err(Error::from))
            })
        })
    }

    pub(crate) fn unheld_cover_is_due(&self, group: &Mbid, now: SystemTime) -> Result<bool> {
        let asked_before = now
            .checked_sub(enriched::COVERS_ASKED_AGAIN_AFTER)
            .unwrap_or(UNIX_EPOCH);

        self.inner.read(|connection| {
            connection
                .query_row(
                    UNHELD_COVER_DUE,
                    params![store::to_nanos(asked_before), group.as_str()],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    pub(crate) fn unheld_covers_due(
        &self,
        now: SystemTime,
        at_most: Option<usize>,
    ) -> Result<Vec<UnheldCoverDue>> {
        let asked_before = now
            .checked_sub(enriched::COVERS_ASKED_AGAIN_AFTER)
            .unwrap_or(UNIX_EPOCH);
        let binds = vec![
            Value::Integer(store::to_nanos(asked_before)),
            Value::Integer(limit(at_most)),
        ];

        self.inner.read(|connection| {
            rows(connection, UNHELD_COVERS_DUE, binds, |row| {
                let group: String = row.get(0)?;
                let pressing: Option<String> = row.get(1)?;
                Ok(Mbid::new(&group).map_err(Error::from).and_then(|group| {
                    Ok(UnheldCoverDue {
                        group,
                        pressing: pressing.as_deref().map(Mbid::new).transpose()?,
                    })
                }))
            })
        })
    }

    pub(crate) fn land_unheld_cover(&self, group: &Mbid, art: Option<&CoverArt>) -> Result<()> {
        self.inner.write(|transaction| {
            enriched::land_unheld_cover(transaction, group, art, SystemTime::now())
        })
    }

    pub fn unheld_cover(&self, group: &Mbid) -> Result<Option<CoverArt>> {
        let stored = self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT cover FROM unheld_covers WHERE release_group = ?1",
                    params![group.as_str()],
                    |row| row.get::<_, Option<Vec<u8>>>(0),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;

        Ok(stored
            .flatten()
            .and_then(|bytes| ImageFormat::sniff(&bytes).map(|format| CoverArt { format, bytes })))
    }

    pub(crate) fn land_songs_of(
        &self,
        group: &Mbid,
        pressing: Option<&Release>,
        now: SystemTime,
    ) -> Result<usize> {
        let read = store::to_nanos(now);
        self.inner
            .write(|transaction| songs::land(transaction, group, pressing, read))
    }

    pub(crate) fn songs_of_refused(&self, group: &Mbid, now: SystemTime) -> Result<()> {
        let read = store::to_nanos(now);
        self.inner
            .write(|transaction| songs::refused(transaction, group, read))
    }

    pub fn albums_kept_for(&self, text: &str) -> Result<Vec<AlbumFound>> {
        let words = elsewhere::folded_words_asked(text);
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let mut binds: Vec<Value> = elsewhere::ALBUM_KINDS
            .into_iter()
            .map(|kind| Value::Text(kind.to_owned()))
            .collect();
        let sought = words
            .iter()
            .map(|word| Infix::held_by_a_release_or_its_artist(word, &mut binds))
            .collect::<Vec<String>>()
            .join(" AND ");
        let sql = format!("{UNHELD_ALBUMS_KEPT}{sought}{UNHELD_ALBUMS_KEPT_IN_ORDER}");

        let kept = self.inner.read(|connection| {
            rows(connection, &sql, binds, |row| {
                let group: String = row.get(0)?;
                let (title, artist, kind, first_released) =
                    (row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?);
                Ok(Mbid::new(&group)
                    .map_err(Error::from)
                    .map(|group| AlbumFound {
                        group,
                        title,
                        artist,
                        kind,
                        first_released,
                    }))
            })
        })?;
        Ok(elsewhere::albums_kept_named_by(kept, text))
    }

    pub fn songs_kept_for(&self, text: &str) -> Result<Vec<Found>> {
        let words = songs::sought_words(text);
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let sought = vec!["instr(s.folded, ?) > 0"; words.len()].join(" AND ");
        let sql = format!(
            "SELECT {} FROM discography_songs s WHERE {sought} AND {SONG_HELD_NOWHERE})
               ORDER BY s.kind IS NOT 'Album', s.released IS NULL, s.released, s.disc, s.position
               LIMIT ?",
            songs::SONG_COLUMNS
        );
        let mut binds: Vec<Value> = words.into_iter().map(Value::Text).collect();
        binds.push(Value::Integer(SONGS_SOUGHT_AT_MOST));

        let found = self.inner.read(|connection| {
            rows(connection, &sql, binds, |row| {
                songs::RawSong::read(row).map(songs::RawSong::into_found)
            })
        })?;
        Ok(songs::once_each(
            found,
            Some(elsewhere::FOUND_ELSEWHERE_AT_MOST),
        ))
    }

    pub fn songs_not_held_by(&self, artist: ArtistId) -> Result<Vec<Found>> {
        let id = Value::Integer(artist.get() as i64);
        let from_unheld = format!(
            "SELECT {} FROM discography_songs s
              WHERE {SONG_HELD_NOWHERE} AND r.artist_id = ?1){SONGS_IN_ORDER}",
            songs::SONG_COLUMNS
        );

        let found = self.inner.read(|connection| {
            let mut found = rows(
                connection,
                MISSING_FROM_AN_ARTISTS_ALBUMS,
                vec![id.clone()],
                |row| songs::RawSong::read(row).map(songs::RawSong::into_found),
            )?;
            found.extend(rows(connection, &from_unheld, vec![id], |row| {
                songs::RawSong::read(row).map(songs::RawSong::into_found)
            })?);
            Ok(found)
        })?;
        Ok(songs::once_each(found, None))
    }

    pub fn albums_not_held_by(&self, artist: ArtistId) -> Result<Vec<AlbumNotHeld>> {
        self.inner.read(|connection| {
            rows(
                connection,
                ALBUMS_NOT_HELD_BY_AN_ARTIST,
                vec![Value::Integer(artist.get() as i64)],
                |row| {
                    let release = RawUnheldRelease::read(row)?;
                    let pressing: Option<String> = row.get(6)?;
                    Ok(release.into_unheld().and_then(|release| {
                        Ok(AlbumNotHeld {
                            release,
                            pressing: pressing.as_deref().map(Mbid::new).transpose()?,
                        })
                    }))
                },
            )
        })
    }

    fn songs_of_group(&self, group: &Mbid) -> Result<Vec<Found>> {
        let sql = format!(
            "SELECT {} FROM discography_songs s
              WHERE s.release_group = ?1
                AND NOT EXISTS (SELECT 1 FROM tracks t WHERE t.mbid = s.recording_mbid)
                AND NOT EXISTS (SELECT 1 FROM artist_releases r
                                  JOIN tracks t ON t.artist_id = r.artist_id
                                 WHERE r.mbid = s.release_group
                                   AND t.title_words = s.words)
              ORDER BY s.disc, s.position",
            songs::SONG_COLUMNS
        );

        self.inner.read(|connection| {
            rows(
                connection,
                &sql,
                vec![Value::Text(group.as_str().to_owned())],
                |row| songs::RawSong::read(row).map(songs::RawSong::into_found),
            )
        })
    }

    pub fn unheld_releases(
        &self,
        narrowing: Option<&str>,
        at_most: Option<usize>,
    ) -> Result<Vec<UnheldRelease>> {
        let (held, mut binds) = unheld_holding(narrowing);
        let sql = format!("{UNHELD_RELEASES}{held}{UNHELD_IN_ORDER}");
        binds.push(Value::Integer(limit(at_most)));

        self.inner.read(|connection| {
            rows(connection, &sql, binds, |row| {
                RawUnheldRelease::read(row).map(RawUnheldRelease::into_unheld)
            })
        })
    }

    pub fn missing_counted(&self, narrowing: Option<&str>) -> Result<Missing> {
        let tracks = match scoped_missing(narrowing) {
            Some(scoped) => self.counted(
                &format!(
                    "{MISSING_TRACKS_COUNTED}{}{SHORT_OF_WHAT_IS_HELD_OR_WANTED}",
                    scoped.from
                ),
                scoped.binds,
            )?,
            None => 0,
        };
        let (held, binds) = unheld_holding(narrowing);
        let releases = self.counted(&format!("{UNHELD_COUNTED}{held}"), binds)?;

        Ok(Missing {
            tracks: u64::from(tracks),
            releases: u64::from(releases),
        })
    }
}
