use std::{sync::Arc, time::SystemTime};

use ahash::{AHashMap, AHashSet};
use resonate_core::{AlbumId, ArtistId, FrameSpan, MediaLocation, TrackId};
use resonate_engine::QueueItem;
use resonate_library::{
    AlbumFound, Codec, Direction, Found, HeldReleaseTrack, Imported, Library, Mbid, SortOrder,
    Track, folded_letters,
};

use super::{Beyond, ListedRow, MissingRow};
use crate::words;

pub(crate) struct ScannedPlaying {
    pub(crate) id: TrackId,
    pub(crate) title: String,
    pub(crate) artist: Option<String>,
    pub(crate) artist_id: Option<ArtistId>,
    pub(crate) album_id: Option<AlbumId>,
    pub(crate) album: Option<String>,
    pub(crate) codec: Codec,
    pub(crate) plays: u32,
    pub(crate) played: Option<SystemTime>,
    pub(crate) added: SystemTime,
    pub(crate) favourite: bool,
}

#[derive(Clone)]
pub(super) struct Named {
    pub(super) location: MediaLocation,
    pub(super) span: Option<FrameSpan>,
    pub(super) track: Option<Track>,
}

impl Named {
    pub(super) fn of(track: &Track) -> Self {
        Self {
            location: track.location.clone(),
            span: track.span,
            track: Some(track.clone()),
        }
    }

    pub(super) fn names(&self, item: &QueueItem) -> bool {
        self.location == item.location && self.span == item.span
    }
}

pub(crate) fn queued_rows<'a>(
    library: &Library,
    rows: impl IntoIterator<Item = &'a QueueItem>,
) -> Vec<Option<Track>> {
    let rows: Vec<&QueueItem> = rows.into_iter().collect();
    let ids: Vec<TrackId> = rows.iter().map(|item| item.id).collect();
    let by_id: AHashMap<TrackId, Track> = match library.tracks_with_ids(&ids) {
        Ok(read) => read.into_iter().map(|track| (track.id, track)).collect(),
        Err(error) => {
            tracing::warn!(%error, "the queued tracks could not be read by id");
            AHashMap::new()
        }
    };

    rows.into_iter()
        .map(|item| match by_id.get(&item.id) {
            Some(track) if track.location == item.location => Some(track.clone()),
            _ => queued_at(library, item),
        })
        .collect()
}

fn queued_at(library: &Library, item: &QueueItem) -> Option<Track> {
    library
        .track_at(item.location.as_path()?, item.span)
        .inspect_err(|error| tracing::warn!(%error, "a queued track could not be read by path"))
        .ok()
        .flatten()
}

pub(super) fn held_at(rows: &[ListedRow], row: usize) -> Option<usize> {
    match rows.get(row)? {
        ListedRow::Held(held) => Some(*held),
        ListedRow::Disc(_)
        | ListedRow::Missing(_)
        | ListedRow::NotHeld(_)
        | ListedRow::Found(_) => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Reaching {
    pub(super) held: usize,
    pub(super) whole: bool,
    pub(super) found: usize,
}

pub(super) fn kept_before_the_rest(kept: &[Found], rest: &[Found]) -> Vec<Found> {
    let named = |found: &Found| (folded_letters(&found.title), folded_letters(&found.artist));
    let coded = |found: &Found| !found.isrcs.is_empty();
    let in_place: Vec<Found> = kept
        .iter()
        .map(|song| {
            rest.iter()
                .filter(|_| !coded(song))
                .find(|other| coded(other) && named(other) == named(song))
                .unwrap_or(song)
                .clone()
        })
        .collect();
    let recordings: AHashSet<&Mbid> = kept
        .iter()
        .chain(&in_place)
        .map(|found| &found.recording)
        .collect();
    let names: AHashSet<(String, String)> = kept.iter().map(named).collect();

    in_place
        .iter()
        .cloned()
        .chain(
            rest.iter()
                .filter(|found| !recordings.contains(&found.recording))
                .filter(|found| !names.contains(&named(found)))
                .cloned(),
        )
        .collect()
}

pub(super) fn albums_kept_before_the_rest(
    kept: &[AlbumFound],
    rest: &[AlbumFound],
) -> Vec<AlbumFound> {
    let named = |album: &AlbumFound| (folded_letters(&album.title), folded_letters(&album.artist));
    let groups: AHashSet<&Mbid> = kept.iter().map(|album| &album.group).collect();
    let names: AHashSet<(String, String)> = kept.iter().map(named).collect();

    kept.iter()
        .cloned()
        .chain(
            rest.iter()
                .filter(|album| !groups.contains(&album.group))
                .filter(|album| !names.contains(&named(album)))
                .cloned(),
        )
        .collect()
}

pub(super) fn beyond_the_listing(reaching: Reaching) -> Vec<ListedRow> {
    let Reaching { held, whole, found } = reaching;
    if !whole || found == 0 {
        return Vec::new();
    }

    (0..held)
        .map(ListedRow::Held)
        .chain([ListedRow::NotHeld(found)])
        .chain((0..found).map(ListedRow::Found))
        .collect()
}

pub(super) const fn elsewhere_standing(
    found: usize,
    asking: bool,
    unreached: bool,
) -> Option<Beyond> {
    match (found, asking) {
        (0, true) => Some(Beyond::Asking),
        (found, false) if unreached => Some(Beyond::Unreached(found)),
        (0, false) => None,
        (found, true) => Some(Beyond::Refining(found)),
        (found, false) => Some(Beyond::Elsewhere(found)),
    }
}

pub(crate) enum AsDrawn {
    Listed,
    Album {
        release_tracks: Arc<[HeldReleaseTrack]>,
        arranging: Arranging,
    },
}

impl AsDrawn {
    pub(crate) fn ordered(&self, listing: Arc<[Track]>) -> Arc<[Track]> {
        let Self::Album {
            release_tracks,
            arranging,
        } = self
        else {
            return listing;
        };

        held_in(&album_rows(&listing, release_tracks, *arranging))
            .into_iter()
            .filter_map(|at| listing.get(at).cloned())
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Arranging {
    pub(super) sort: SortOrder,
    pub(super) reading: Direction,
}

impl Arranging {
    const fn by_seat(self) -> bool {
        matches!(self.sort, SortOrder::Relevance | SortOrder::AlbumThenTrack)
    }
}

pub(super) fn held_in(rows: &[ListedRow]) -> Vec<usize> {
    rows.iter()
        .filter_map(|row| match row {
            ListedRow::Held(held) => Some(*held),
            ListedRow::Disc(_)
            | ListedRow::Missing(_)
            | ListedRow::NotHeld(_)
            | ListedRow::Found(_) => None,
        })
        .collect()
}

pub(super) type Seat = (u32, u32);

pub(super) fn album_rows(
    tracks: &[Track],
    release_tracks: &[HeldReleaseTrack],
    arranging: Arranging,
) -> Vec<ListedRow> {
    let placed: AHashMap<TrackId, Seat> = release_tracks
        .iter()
        .filter_map(|row| row.track.map(|track| (track, (row.disc, row.position))))
        .collect();

    let held = tracks
        .iter()
        .enumerate()
        .map(|(index, track)| {
            let place = placed.get(&track.id).copied().unwrap_or((
                track.disc_number.unwrap_or(1),
                track.track_number.unwrap_or(u32::MAX),
            ));
            (place, ListedRow::Held(index))
        })
        .collect();
    let missing = release_tracks
        .iter()
        .enumerate()
        .filter(|(_, row)| row.track.is_none())
        .map(|(index, row)| ((row.disc, row.position), ListedRow::Missing(index)))
        .collect();

    arranged(held, missing, arranging)
}

pub(super) fn arranged(
    held: Vec<(Seat, ListedRow)>,
    mut missing: Vec<(Seat, ListedRow)>,
    arranging: Arranging,
) -> Vec<ListedRow> {
    if !arranging.by_seat() {
        missing.sort_by_key(|(place, _)| *place);
        return held
            .into_iter()
            .chain(missing)
            .map(|(_, row)| row)
            .collect();
    }

    let mut rows = held;
    rows.append(&mut missing);
    rows.sort_by_key(|(place, _)| *place);
    if arranging.reading == Direction::Descending {
        rows.reverse();
    }
    headed_by_disc(rows)
}

pub(super) fn headed_by_disc(rows: Vec<((u32, u32), ListedRow)>) -> Vec<ListedRow> {
    let discs: AHashSet<u32> = rows.iter().map(|((disc, _), _)| *disc).collect();
    if discs.len() < 2 {
        return rows.into_iter().map(|(_, row)| row).collect();
    }

    let mut listed = Vec::with_capacity(rows.len() + discs.len());
    let mut heading = None;
    for ((disc, _), row) in rows {
        if heading != Some(disc) {
            listed.push(ListedRow::Disc(disc));
            heading = Some(disc);
        }
        listed.push(row);
    }

    listed
}

pub(super) fn missing_track_rows(albums: impl Iterator<Item = (AlbumId, u32)>) -> Vec<MissingRow> {
    let mut listed = Vec::new();
    headed_by_album_and_disc(albums, &mut listed);
    listed
}

pub(super) fn unheld_release_rows(artists: impl Iterator<Item = ArtistId>) -> Vec<MissingRow> {
    let mut listed = Vec::new();
    headed_by_run(
        artists,
        MissingRow::Artist,
        MissingRow::Release,
        &mut listed,
    );
    listed
}

fn over_more_than_one_disc(run: &[(AlbumId, u32)]) -> bool {
    let Some((album, first)) = run.first().copied() else {
        return false;
    };

    run.iter()
        .take_while(|(held, _)| *held == album)
        .any(|(_, disc)| *disc != first)
}

fn headed_by_album_and_disc(
    albums: impl Iterator<Item = (AlbumId, u32)>,
    listed: &mut Vec<MissingRow>,
) {
    let placed: Vec<(AlbumId, u32)> = albums.collect();
    let mut under = None;
    let mut over = None;
    let mut discs_are_headed = false;
    for (index, (album, disc)) in placed.iter().copied().enumerate() {
        if under != Some(album) {
            listed.push(MissingRow::Album(index));
            under = Some(album);
            over = None;
            discs_are_headed = over_more_than_one_disc(&placed[index..]);
        }
        if discs_are_headed && over != Some(disc) {
            listed.push(MissingRow::Disc(index));
            over = Some(disc);
        }
        listed.push(MissingRow::Track(index));
    }
}

fn headed_by_run<K: PartialEq>(
    keys: impl Iterator<Item = K>,
    heading: fn(usize) -> MissingRow,
    row: fn(usize) -> MissingRow,
    listed: &mut Vec<MissingRow>,
) {
    let mut under = None;
    for (index, key) in keys.enumerate() {
        if under.as_ref() != Some(&key) {
            listed.push(heading(index));
            under = Some(key);
        }
        listed.push(row(index));
    }
}

pub(super) fn read_in(imported: Imported) -> String {
    let mut said = vec![words::models::read_in(
        imported.added,
        imported.encoding.name(),
        imported.format.name(),
        &imported.name,
    )];
    if imported.already > 0 {
        said.push(words::models::already_in_it(imported.already));
    }
    if imported.elsewhere > 0 {
        said.push(words::models::passed_over_naming_no_local_file(
            imported.elsewhere,
        ));
    }
    if imported.missing > 0 {
        said.push(words::models::naming_a_file_not_there(imported.missing));
    }
    if imported.short > 0 {
        said.push(words::models::declared_and_not_held(imported.short));
    }
    said.join(", ")
}
