use std::time::{Duration, SystemTime};

use resonate_core::{Mbid, PlaylistId, TrackId};
use rusqlite::{OptionalExtension as _, Transaction, params};

use crate::{
    Cut, Error, FollowedPlaylist, Found, Library, PlaylistLink, ProfileLink, Reference, Result,
    StoreOp, playlist, store,
};

pub const KEPT_IN_STEP_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Upkeep {
    Once,
    InStep,
}

impl Upkeep {
    const fn code(self) -> i64 {
        match self {
            Self::Once => 0,
            Self::InStep => 1,
        }
    }

    const fn coded(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Once),
            1 => Some(Self::InStep),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaylistSource {
    pub link: PlaylistLink,
    pub upkeep: Upkeep,
    pub refreshed: SystemTime,
}

impl PlaylistSource {
    pub fn is_due(&self, now: SystemTime) -> bool {
        self.upkeep == Upkeep::InStep
            && now
                .duration_since(self.refreshed)
                .is_ok_and(|since| since >= KEPT_IN_STEP_EVERY)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilledFromTheLink {
    pub playlist: PlaylistId,
    pub started: bool,
    pub added: usize,
    pub removed: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FromTheLink {
    pub playlist: PlaylistId,
    pub name: String,
    pub started: bool,
    pub added: usize,
    pub removed: usize,
    pub found: Vec<Found>,
    pub unnamed: usize,
}

pub(crate) fn read(
    playlist: PlaylistId,
    link: Option<String>,
    upkeep: Option<i64>,
    refreshed: Option<i64>,
) -> Result<Option<PlaylistSource>> {
    let (Some(link), Some(upkeep), Some(refreshed)) = (link, upkeep, refreshed) else {
        return Ok(None);
    };
    let unreadable = || Error::UnreadablePlaylistSource { playlist };

    Ok(Some(PlaylistSource {
        link: PlaylistLink::read(&link).ok_or_else(unreadable)?,
        upkeep: Upkeep::coded(upkeep).ok_or_else(unreadable)?,
        refreshed: store::from_nanos(refreshed),
    }))
}

pub(crate) fn forget_the_source_of_a_discarded_holder(
    transaction: &Transaction<'_>,
    id: PlaylistId,
) -> Result<()> {
    transaction
        .execute(
            "DELETE FROM playlist_sources WHERE playlist_id = ?1",
            params![id.get() as i64],
        )
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Delete, source))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Remembering {
    OverWhatWasThere,
    WhereNoneWas,
}

impl Library {
    pub fn import_playlist_link(
        &self,
        reference: &dyn Reference,
        link: &PlaylistLink,
        name: Option<&str>,
        upkeep: Upkeep,
    ) -> Result<Option<FromTheLink>> {
        let Some(followed) = self.follow_playlist_link(reference, link)? else {
            return Ok(None);
        };
        let wanted_name = name.unwrap_or(&followed.name).to_owned();
        let filled = self.fill_from_the_link(link, &wanted_name, &followed.held, upkeep)?;

        Ok(Some(self.told(
            filled.playlist,
            filled.started,
            filled.added,
            filled.removed,
            followed,
        )?))
    }

    pub fn fill_from_the_link(
        &self,
        link: &PlaylistLink,
        name: &str,
        held: &[TrackId],
        upkeep: Upkeep,
    ) -> Result<FilledFromTheLink> {
        let cuts = self.cuts_of(held)?;

        let filled = match upkeep {
            Upkeep::Once => {
                let filled = playlist::fill(self.inner(), name, &cuts)?;
                FilledFromTheLink {
                    playlist: filled.playlist,
                    started: filled.started,
                    added: filled.added,
                    removed: 0,
                }
            }
            Upkeep::InStep => match self.playlist_kept_from(link)? {
                Some(standing) => {
                    let mirrored = playlist::mirror(self.inner(), standing, &cuts)?;
                    FilledFromTheLink {
                        playlist: standing,
                        started: false,
                        added: mirrored.added,
                        removed: mirrored.removed,
                    }
                }
                None => {
                    let named = self.a_free_playlist_name(name)?;
                    FilledFromTheLink {
                        playlist: playlist::start(self.inner(), &named, &cuts)?,
                        started: true,
                        added: cuts.len(),
                        removed: 0,
                    }
                }
            },
        };
        let remembering = match upkeep {
            Upkeep::InStep => Remembering::OverWhatWasThere,
            Upkeep::Once if filled.started => Remembering::OverWhatWasThere,
            Upkeep::Once => Remembering::WhereNoneWas,
        };
        self.remember_the_source(filled.playlist, link, upkeep, remembering)?;

        Ok(filled)
    }

    pub fn refresh_playlist(
        &self,
        reference: &dyn Reference,
        playlist: PlaylistId,
    ) -> Result<Option<FromTheLink>> {
        let source = self
            .playlist_source(playlist)?
            .ok_or(Error::NoPlaylistSource { playlist })?;
        let Some(mut followed) = self.follow_playlist_link(reference, &source.link)? else {
            return Ok(None);
        };
        let cuts = self.cuts_of(&followed.held)?;

        let (added, removed) = match source.upkeep {
            Upkeep::Once => (playlist::fill_into(self.inner(), playlist, &cuts)?, 0),
            Upkeep::InStep => {
                let mirrored = playlist::mirror(self.inner(), playlist, &cuts)?;
                (mirrored.added, mirrored.removed)
            }
        };
        self.stamp_refreshed(playlist)?;

        followed.found = self.not_yet_wanted(followed.found)?;

        Ok(Some(self.told(playlist, false, added, removed, followed)?))
    }

    pub fn playlist_source(&self, playlist: PlaylistId) -> Result<Option<PlaylistSource>> {
        self.inner().read(|connection| {
            let row = connection
                .query_row(
                    "SELECT link, kept, refreshed FROM playlist_sources WHERE playlist_id = ?1",
                    params![playlist.get() as i64],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?;
            match row {
                Some((link, upkeep, refreshed)) => {
                    read(playlist, Some(link), Some(upkeep), Some(refreshed))
                }
                None => Ok(None),
            }
        })
    }

    pub fn keep_playlist_from_its_link(
        &self,
        playlist: PlaylistId,
        upkeep: Upkeep,
    ) -> Result<bool> {
        let changed = self.inner().write(|transaction| {
            let changed = transaction
                .execute(
                    "UPDATE playlist_sources SET kept = ?2 WHERE playlist_id = ?1 AND kept != ?2",
                    params![playlist.get() as i64, upkeep.code()],
                )
                .map_err(|source| Error::store(StoreOp::Update, source))?;
            if changed == 0 && !has_a_source(transaction, playlist)? {
                return Err(Error::NoPlaylistSource { playlist });
            }
            Ok(changed > 0)
        })?;

        if changed {
            self.inner().playlists_changed();
        }
        Ok(changed)
    }

    pub fn forget_playlist_source(&self, playlist: PlaylistId) -> Result<bool> {
        let forgotten = self.inner().write(|transaction| {
            transaction
                .execute(
                    "DELETE FROM playlist_sources WHERE playlist_id = ?1",
                    params![playlist.get() as i64],
                )
                .map_err(|source| Error::store(StoreOp::Delete, source))
        })?;

        if forgotten > 0 {
            self.inner().playlists_changed();
        }
        Ok(forgotten > 0)
    }

    pub fn playlists_kept_in_step(&self) -> Result<Vec<(PlaylistId, PlaylistSource)>> {
        self.inner().read(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT s.playlist_id, s.link, s.kept, s.refreshed FROM playlist_sources s
                     JOIN playlists p ON p.id = s.playlist_id
                     WHERE s.kept = ?1 ORDER BY s.refreshed, s.playlist_id",
                )
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let rows = statement
                .query_map(params![Upkeep::InStep.code()], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|source| Error::store(StoreOp::Query, source))?;

            let mut kept = Vec::with_capacity(rows.len());
            for (id, link, upkeep, refreshed) in rows {
                let id = PlaylistId::new(id as u64)?;
                if let Some(source) = read(id, Some(link), Some(upkeep), Some(refreshed))? {
                    kept.push((id, source));
                }
            }
            Ok(kept)
        })
    }

    pub fn playlists_due_a_refresh(&self, now: SystemTime) -> Result<Vec<PlaylistId>> {
        Ok(self
            .playlists_kept_in_step()?
            .into_iter()
            .filter(|(_, source)| source.is_due(now))
            .map(|(id, _)| id)
            .collect())
    }

    pub fn is_recording_wanted(&self, recording: &Mbid) -> Result<bool> {
        self.inner().read(|connection| {
            connection
                .query_row(
                    "SELECT 1 FROM wants w JOIN release_tracks r ON r.id = w.release_track_id
                     WHERE r.recording_mbid = ?1 LIMIT 1",
                    params![recording.as_str()],
                    |_| Ok(()),
                )
                .optional()
                .map(|held| held.is_some())
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    fn not_yet_wanted(&self, found: Vec<Found>) -> Result<Vec<Found>> {
        let mut unwanted = Vec::with_capacity(found.len());
        for song in found {
            if !self.is_recording_wanted(&song.recording)? {
                unwanted.push(song);
            }
        }
        Ok(unwanted)
    }

    fn cuts_of(&self, tracks: &[TrackId]) -> Result<Vec<Cut>> {
        let mut cuts = Vec::with_capacity(tracks.len());
        for track in tracks {
            if let Some(held) = self.track(*track)? {
                cuts.push(Cut::of(&held));
            }
        }
        Ok(cuts)
    }

    fn playlist_kept_from(&self, link: &PlaylistLink) -> Result<Option<PlaylistId>> {
        Ok(self
            .playlists_kept_in_step()?
            .into_iter()
            .find(|(_, source)| source.link == *link)
            .map(|(id, _)| id))
    }

    fn a_free_playlist_name(&self, wanted: &str) -> Result<String> {
        playlist::a_free_name_for(self.inner(), wanted)
    }

    fn remember_the_source(
        &self,
        playlist: PlaylistId,
        link: &PlaylistLink,
        upkeep: Upkeep,
        remembering: Remembering,
    ) -> Result<()> {
        let sql = match remembering {
            Remembering::OverWhatWasThere => {
                "INSERT INTO playlist_sources (playlist_id, link, kept, refreshed)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (playlist_id) DO UPDATE
                 SET link = excluded.link, kept = excluded.kept, refreshed = excluded.refreshed"
            }
            Remembering::WhereNoneWas => {
                "INSERT INTO playlist_sources (playlist_id, link, kept, refreshed)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (playlist_id) DO NOTHING"
            }
        };

        self.inner().write(|transaction| {
            transaction
                .execute(
                    sql,
                    params![
                        playlist.get() as i64,
                        link.page(),
                        upkeep.code(),
                        store::to_nanos(SystemTime::now())
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))
        })?;

        self.inner().playlists_changed();
        Ok(())
    }

    fn stamp_refreshed(&self, playlist: PlaylistId) -> Result<()> {
        self.inner().write(|transaction| {
            transaction
                .execute(
                    "UPDATE playlist_sources SET refreshed = ?2 WHERE playlist_id = ?1",
                    params![playlist.get() as i64, store::to_nanos(SystemTime::now())],
                )
                .map_err(|source| Error::store(StoreOp::Update, source))
        })?;
        Ok(())
    }

    fn told(
        &self,
        playlist: PlaylistId,
        started: bool,
        added: usize,
        removed: usize,
        followed: FollowedPlaylist,
    ) -> Result<FromTheLink> {
        let name = self
            .playlist(playlist)?
            .map_or(followed.name, |standing| standing.name);

        Ok(FromTheLink {
            playlist,
            name,
            started,
            added,
            removed,
            found: followed.found,
            unnamed: followed.unnamed,
        })
    }
}

fn has_a_source(transaction: &Transaction<'_>, playlist: PlaylistId) -> Result<bool> {
    transaction
        .query_row(
            "SELECT 1 FROM playlist_sources WHERE playlist_id = ?1",
            params![playlist.get() as i64],
            |_| Ok(()),
        )
        .optional()
        .map(|held| held.is_some())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

#[derive(Clone, Debug, PartialEq)]
pub struct FollowedProfile {
    pub name: String,
    pub playlists: Vec<FromTheLink>,
    pub let_go: usize,
    pub gone: usize,
    pub unread: usize,
}

impl Library {
    pub fn follow_profile(
        &self,
        reference: &dyn Reference,
        link: &ProfileLink,
        upkeep: Upkeep,
    ) -> Result<Option<FollowedProfile>> {
        let Some(profile) = reference.profile_linked(link)? else {
            return Ok(None);
        };
        let known = self.pages_of(link)?;
        let mut followed = FollowedProfile {
            name: profile.name,
            playlists: Vec::new(),
            let_go: 0,
            gone: 0,
            unread: 0,
        };

        for listed in &profile.playlists {
            let page = listed.link.page();
            if upkeep == Upkeep::InStep
                && known.contains(&page)
                && self.playlist_kept_from(&listed.link)?.is_none()
            {
                followed.let_go += 1;
                continue;
            }
            let Some(mut read) =
                self.import_playlist_link(reference, &listed.link, Some(&listed.title), upkeep)?
            else {
                followed.unread += 1;
                continue;
            };
            read.found = self.not_yet_wanted(read.found)?;
            if upkeep == Upkeep::Once
                && self
                    .playlist_source(read.playlist)?
                    .is_some_and(|source| source.link == listed.link)
            {
                self.forget_playlist_source(read.playlist)?;
            }
            followed.playlists.push(read);
        }

        let listed: Vec<String> = profile
            .playlists
            .iter()
            .map(|listed| listed.link.page())
            .collect();
        for page in known.iter().filter(|page| !listed.contains(page)) {
            let Some(was) = PlaylistLink::read(page) else {
                continue;
            };
            if let Some(playlist) = self.playlist_kept_from(&was)? {
                self.forget_playlist_source(playlist)?;
                followed.gone += 1;
            }
        }

        match upkeep {
            Upkeep::InStep => self.keep_following(link, &listed)?,
            Upkeep::Once => self.stop_following(link)?,
        }
        Ok(Some(followed))
    }

    pub fn profiles_followed(&self) -> Result<Vec<(ProfileLink, SystemTime)>> {
        self.inner().read(|connection| {
            let mut statement = connection
                .prepare("SELECT link, refreshed FROM profile_sources ORDER BY refreshed, link")
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|source| Error::store(StoreOp::Query, source))?;

            Ok(rows
                .into_iter()
                .filter_map(|(link, refreshed)| {
                    ProfileLink::read(&link).map(|link| (link, store::from_nanos(refreshed)))
                })
                .collect())
        })
    }

    pub fn profiles_due_a_refresh(&self, now: SystemTime) -> Result<Vec<ProfileLink>> {
        Ok(self
            .profiles_followed()?
            .into_iter()
            .filter(|(_, refreshed)| {
                now.duration_since(*refreshed)
                    .is_ok_and(|since| since >= KEPT_IN_STEP_EVERY)
            })
            .map(|(link, _)| link)
            .collect())
    }

    pub fn stop_following(&self, link: &ProfileLink) -> Result<()> {
        self.inner().write(|transaction| {
            transaction
                .execute(
                    "DELETE FROM profile_sources WHERE link = ?1",
                    params![link.page()],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Delete, source))
        })
    }

    pub fn let_go_of_the_profile(&self, link: &ProfileLink) -> Result<usize> {
        let mut let_go = 0;
        for page in self.pages_of(link)? {
            let Some(was) = PlaylistLink::read(&page) else {
                continue;
            };
            if let Some(playlist) = self.playlist_kept_from(&was)? {
                self.forget_playlist_source(playlist)?;
                let_go += 1;
            }
        }
        self.stop_following(link)?;

        Ok(let_go)
    }

    fn pages_of(&self, link: &ProfileLink) -> Result<Vec<String>> {
        self.inner().read(|connection| {
            let mut statement = connection
                .prepare("SELECT page FROM profile_playlists WHERE profile = ?1 ORDER BY page")
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            statement
                .query_map(params![link.page()], |row| row.get::<_, String>(0))
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    fn keep_following(&self, link: &ProfileLink, pages: &[String]) -> Result<()> {
        let profile = link.page();
        self.inner().write(|transaction| {
            transaction
                .execute(
                    "INSERT INTO profile_sources (link, refreshed) VALUES (?1, ?2)
                     ON CONFLICT (link) DO UPDATE SET refreshed = excluded.refreshed",
                    params![profile, store::to_nanos(SystemTime::now())],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            transaction
                .execute(
                    "DELETE FROM profile_playlists WHERE profile = ?1",
                    params![profile],
                )
                .map_err(|source| Error::store(StoreOp::Delete, source))?;
            for page in pages {
                transaction
                    .execute(
                        "INSERT OR IGNORE INTO profile_playlists (profile, page) VALUES (?1, ?2)",
                        params![profile, page],
                    )
                    .map_err(|source| Error::store(StoreOp::Insert, source))?;
            }
            Ok(())
        })
    }
}
