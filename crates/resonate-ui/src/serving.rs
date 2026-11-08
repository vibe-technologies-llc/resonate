use std::{
    num::{NonZeroU32, NonZeroUsize},
    sync::Arc,
    time::{Duration, Instant},
};

use ahash::{AHashMap, AHashSet};
use gpui::{Context, Task};
use resonate_core::{MediaLocation, SourceId};
use resonate_library::{CoverArt, ImageFormat, SongToKeep};
use resonate_providers::{Error as ProviderError, Named};

use crate::{
    ResonateApp,
    drawing::Drawer,
    models::{AtSide, Drawn, Forget as _, Picture, Scale, drawn_within},
    recent::Recent,
    served::{ServedAlbum, ServedArtist, ServedCover, ServedFound, ServedSong, Serves},
};

const COVERS_HELD: NonZeroUsize = NonZeroUsize::new(256).expect("a cache holds an entry");

const COVERS_FETCHED_AT_ONCE: usize = 4;

const A_FAILED_COVER_IS_ASKED_AGAIN_AFTER: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    Artists,
    Artist {
        location: MediaLocation,
        name: String,
    },
    Album {
        location: MediaLocation,
        name: String,
    },
    Mix {
        location: MediaLocation,
        name: String,
    },
    Found {
        words: String,
    },
}

impl Place {
    pub(crate) fn title(&self) -> String {
        match self {
            Self::Artists => "Artists".to_owned(),
            Self::Artist { name, .. } | Self::Album { name, .. } | Self::Mix { name, .. } => {
                name.clone()
            }
            Self::Found { words } => format!("\u{201C}{words}\u{201D}"),
        }
    }

    pub(crate) const fn eyebrow(&self) -> &'static str {
        match self {
            Self::Artists => "Services",
            Self::Artist { .. } => "Artist on the server",
            Self::Album { .. } => "Album on the server",
            Self::Mix { .. } => "Mix on the server",
            Self::Found { .. } => "Search on the server",
        }
    }

    pub(crate) fn location(&self) -> Option<&MediaLocation> {
        match self {
            Self::Artist { location, .. }
            | Self::Album { location, .. }
            | Self::Mix { location, .. } => Some(location),
            Self::Artists | Self::Found { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Row {
    Heading(&'static str),
    Artist(ServedArtist),
    Album(ServedAlbum),
    Mix(ServedAlbum),
    Song(ServedSong),
}

impl Row {
    pub(crate) fn location(&self) -> Option<&MediaLocation> {
        match self {
            Self::Heading(_) => None,
            Self::Artist(artist) => Some(&artist.location),
            Self::Album(album) | Self::Mix(album) => Some(&album.location),
            Self::Song(song) => Some(&song.location),
        }
    }

    pub(crate) fn cover(&self) -> Option<&ServedCover> {
        match self {
            Self::Heading(_) => None,
            Self::Artist(artist) => artist.cover.as_ref(),
            Self::Album(album) | Self::Mix(album) => album.cover.as_ref(),
            Self::Song(song) => song.cover.as_ref(),
        }
    }

    pub(crate) fn song(&self) -> Option<&ServedSong> {
        match self {
            Self::Song(song) => Some(song),
            Self::Heading(_) | Self::Artist(_) | Self::Album(_) | Self::Mix(_) => None,
        }
    }
}

pub(crate) fn song_to_keep(song: &ServedSong) -> Option<SongToKeep> {
    let source = song.location.source().clone();
    let Some(Named::Song(video)) = Named::read(&song.location, &source) else {
        return None;
    };
    Some(SongToKeep {
        source,
        video: video.to_string(),
        title: song.title.clone(),
        artist: song.artist.clone(),
        album: song.album.clone(),
        length: song.length,
        cover: song.cover.as_ref().map(|cover| cover.0.clone()),
    })
}

pub(crate) fn first_rows(first: ServedFound) -> Vec<Row> {
    if first.albums.is_empty() && first.songs.is_empty() && first.mixes.is_empty() {
        return first.artists.into_iter().map(Row::Artist).collect();
    }
    rows_of_found(first)
}

pub(crate) fn favourites_in(rows: &[Row]) -> bool {
    rows.iter().any(|row| matches!(row, Row::Heading(_)))
}

pub(crate) fn rows_of_found(found: ServedFound) -> Vec<Row> {
    let ServedFound {
        artists,
        albums,
        songs,
        mixes,
    } = found;
    let mut rows = Vec::new();
    if !artists.is_empty() {
        rows.push(Row::Heading("Artists"));
        rows.extend(artists.into_iter().map(Row::Artist));
    }
    if !albums.is_empty() {
        rows.push(Row::Heading("Albums"));
        rows.extend(albums.into_iter().map(Row::Album));
    }
    if !mixes.is_empty() {
        rows.push(Row::Heading("Mixes"));
        rows.extend(mixes.into_iter().map(Row::Mix));
    }
    if !songs.is_empty() {
        rows.push(Row::Heading("Songs"));
        rows.extend(songs.into_iter().map(Row::Song));
    }
    rows
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Unwelcome,
    Unreached,
    Unreadable,
}

impl Failure {
    pub(crate) fn of(error: &ProviderError) -> Self {
        match error {
            ProviderError::Unwelcome { .. } => Self::Unwelcome,
            error if error.is_the_provider_away() => Self::Unreached,
            _ => Self::Unreadable,
        }
    }

    pub(crate) const fn says(self) -> &'static str {
        match self {
            Self::Unwelcome => "The server turned the account away",
            Self::Unreached => "The server could not be reached",
            Self::Unreadable => "The server did not answer in a way that could be read",
        }
    }

    pub(crate) const fn advises(self) -> &'static str {
        match self {
            Self::Unwelcome => "Check the account in Settings, under Online.",
            Self::Unreached => {
                "Check the address in Settings, under Online, and that the server is running."
            }
            Self::Unreadable => "The address may not be the service's.",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Reading,
    Read(Arc<[Row]>),
    Failed(Failure),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CoverKey {
    Named(SourceId, ServedCover),
    Located(MediaLocation),
}

impl CoverKey {
    fn of(location: &MediaLocation, known: Option<&ServedCover>) -> Self {
        known.map_or_else(
            || Self::Located(location.clone()),
            |known| Self::Named(location.source().clone(), known.clone()),
        )
    }
}

fn the_same(held: &[Arc<dyn Serves>], opened: &[Arc<dyn Serves>]) -> bool {
    held.len() == opened.len()
        && held
            .iter()
            .zip(opened)
            .all(|(held, opened)| Arc::ptr_eq(held, opened))
}

pub(crate) struct ServerModel {
    services: Vec<Arc<dyn Serves>>,
    chosen: usize,
    trail: Vec<Place>,
    page: Page,
    reading: u64,
    _read: Option<Task<()>>,
    covers: Recent<AtSide<CoverKey>, Option<Picture>>,
    fetching: AHashSet<CoverKey>,
    unfetched: AHashMap<CoverKey, Instant>,
    favoured: AHashMap<MediaLocation, bool>,
    scale: Scale,
    shown: bool,
    owes_a_first_page: bool,
}

impl ServerModel {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let mut model = Self {
            services: Vec::new(),
            chosen: 0,
            trail: vec![Place::Artists],
            page: Page::Reading,
            reading: 0,
            _read: None,
            covers: Recent::new(COVERS_HELD),
            fetching: AHashSet::new(),
            unfetched: AHashMap::new(),
            favoured: AHashMap::new(),
            scale: Scale::ONE,
            shown: false,
            owes_a_first_page: false,
        };
        model.refresh(cx);
        model
    }

    pub(crate) fn is_open(&self) -> bool {
        !self.services.is_empty()
    }

    pub(crate) fn shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.shown = shown;
        if shown && self.owes_a_first_page {
            self.read(cx);
        }
    }

    fn waits_to_be_shown(&self, serves: &dyn Serves) -> bool {
        !self.shown && serves.keeps_songs() && *self.place() == Place::Artists
    }

    pub(crate) fn serves(&self) -> Option<Arc<dyn Serves>> {
        self.services.get(self.chosen).cloned()
    }

    pub(crate) fn titles(&self) -> Vec<String> {
        self.services
            .iter()
            .map(|serves| serves.title().to_owned())
            .collect()
    }

    pub(crate) const fn chosen(&self) -> usize {
        self.chosen
    }

    pub(crate) fn choose(&mut self, chosen: usize, cx: &mut Context<Self>) {
        if chosen == self.chosen || chosen >= self.services.len() {
            return;
        }
        self.chosen = chosen;
        self.trail = vec![Place::Artists];
        self.read(cx);
    }

    fn serving(&self, location: &MediaLocation) -> Option<Arc<dyn Serves>> {
        self.services
            .iter()
            .find(|serves| serves.names(location))
            .cloned()
    }

    pub(crate) fn keeps_favourites(&self, location: &MediaLocation) -> bool {
        self.serving(location)
            .is_some_and(|serves| serves.keeps_favourites())
    }

    pub(crate) fn plays_songs(&self, location: &MediaLocation) -> bool {
        self.serving(location)
            .is_none_or(|serves| serves.plays_songs())
    }

    pub(crate) fn keeps_songs(&self, location: &MediaLocation) -> bool {
        self.serving(location)
            .is_some_and(|serves| serves.keeps_songs())
    }

    pub(crate) fn is_favoured(&self, song: &ServedSong) -> bool {
        self.favoured
            .get(&song.location)
            .copied()
            .or(song.favourite)
            .unwrap_or(false)
    }

    pub(crate) fn note_favoured(&mut self, location: MediaLocation, favoured: bool) {
        self.favoured.insert(location, favoured);
    }

    pub(crate) fn names(&self, location: &MediaLocation) -> bool {
        self.services.iter().any(|serves| serves.names(location))
    }

    pub(crate) fn place(&self) -> &Place {
        self.trail.last().unwrap_or(&Place::Artists)
    }

    pub(crate) fn way_back_name(&self) -> Option<String> {
        let at = self.trail.len().checked_sub(2)?;
        self.trail.get(at).map(Place::title)
    }

    pub(crate) fn page(&self) -> &Page {
        &self.page
    }

    pub(crate) fn scaled_by(&mut self, scale: Scale) {
        self.scale = scale;
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let app = cx.global::<ResonateApp>();
        let online = app.online.clone();
        let serving = app.serving.clone();
        let opened = serving.opened(&online);
        if the_same(&self.services, &opened) {
            return;
        }
        let was = self.serves().map(|serves| serves.scheme().clone());
        self.chosen = was
            .and_then(|was| opened.iter().position(|serves| *serves.scheme() == was))
            .unwrap_or_default();
        self.services = opened;
        self.favoured.clear();
        self.trail = vec![Place::Artists];
        self.forget_the_covers(cx);
        self.read(cx);
    }

    pub(crate) fn open(&mut self, place: Place, cx: &mut Context<Self>) {
        if let Place::Found { .. } = place
            && let Some(Place::Found { .. }) = self.trail.last()
        {
            self.trail.pop();
        }
        self.trail.push(place);
        self.read(cx);
    }

    pub(crate) fn back(&mut self, cx: &mut Context<Self>) -> bool {
        if self.trail.len() < 2 {
            return false;
        }
        self.trail.pop();
        self.read(cx);
        true
    }

    pub(crate) fn leave_the_search(&mut self, cx: &mut Context<Self>) {
        if let Some(Place::Found { .. }) = self.trail.last() {
            self.back(cx);
        }
    }

    pub(crate) fn read_again(&mut self, cx: &mut Context<Self>) {
        self.read(cx);
    }

    fn read(&mut self, cx: &mut Context<Self>) {
        self.reading += 1;
        let serial = self.reading;
        let Some(serves) = self.serves() else {
            self.page = Page::Reading;
            self._read = None;
            cx.notify();
            return;
        };
        self.page = Page::Reading;
        self.owes_a_first_page = self.waits_to_be_shown(serves.as_ref());
        cx.notify();
        if self.owes_a_first_page {
            self._read = None;
            return;
        }

        let place = self.place().clone();
        let read = cx
            .background_executor()
            .spawn(async move { rows_at(serves.as_ref(), &place) });
        self._read = Some(cx.spawn(async move |this, cx| {
            let read = read.await;
            let _ = this.update(cx, |this, cx| {
                if this.reading != serial {
                    return;
                }
                this.page = match read {
                    Ok(rows) => Page::Read(rows.into()),
                    Err(error) => {
                        tracing::warn!(%error, "the service could not be browsed");
                        Page::Failed(Failure::of(&error))
                    }
                };
                cx.notify();
            });
        }));
    }

    fn forget_the_covers(&mut self, cx: &mut Context<Self>) {
        let held: Vec<_> = self.covers.entries().map(|(key, _)| key.clone()).collect();
        for key in held {
            if let Some(Some(picture)) = self.covers.peek(&key).cloned() {
                picture.forget(cx);
            }
        }
        self.covers = Recent::new(COVERS_HELD);
        self.fetching.clear();
        self.unfetched.clear();
    }

    pub(crate) fn cover(
        &mut self,
        of: &MediaLocation,
        known: Option<&ServedCover>,
        drawn: Drawn,
        cx: &mut Context<Self>,
    ) -> Option<Picture> {
        let serves = self.serving(of).or_else(|| self.serves())?;
        let key = CoverKey::of(of, known);
        let wanted = AtSide {
            key: key.clone(),
            side: drawn.side(self.scale),
        };
        if let Some(held) = self.covers.get(&wanted) {
            return held.clone();
        }
        if self.fetching.contains(&key)
            || self.fetching.len() >= COVERS_FETCHED_AT_ONCE
            || self
                .unfetched
                .get(&key)
                .is_some_and(|failed| failed.elapsed() < A_FAILED_COVER_IS_ASKED_AGAIN_AFTER)
        {
            return None;
        }
        self.fetching.insert(key.clone());

        let side = wanted.side;
        let location = of.clone();
        let known = known.cloned();
        let fetched = cx
            .background_executor()
            .spawn(async move { serves.cover(&location, known.as_ref(), side.get()) });
        cx.spawn(async move |this, cx| {
            let bytes = fetched.await;
            let bytes = match bytes {
                Ok(bytes) => Some(bytes),
                Err(error) => {
                    tracing::debug!(%error, "a cover the server holds could not be fetched");
                    None
                }
            };
            let answered = bytes.is_some();
            let drawing = this.update(cx, |_, cx| {
                cx.global::<Drawer>()
                    .draw(move || bytes.and_then(|bytes| drawn_from(bytes, side)))
            });
            let Ok(drawing) = drawing else {
                return;
            };
            let decoded = drawing.await.flatten();
            let _ = this.update(cx, |this, cx| {
                this.fetching.remove(&wanted.key);
                if answered {
                    this.unfetched.remove(&wanted.key);
                    this.covers.insert(wanted, decoded).forget(cx);
                    cx.notify();
                } else {
                    this.unfetched.insert(wanted.key, Instant::now());
                }
            });
        })
        .detach();
        None
    }

    pub(crate) fn cover_is_absent(
        &self,
        of: &MediaLocation,
        known: Option<&ServedCover>,
        drawn: Drawn,
    ) -> bool {
        let wanted = AtSide {
            key: CoverKey::of(of, known),
            side: drawn.side(self.scale),
        };
        matches!(self.covers.peek(&wanted), Some(None))
    }
}

fn drawn_from(bytes: Vec<u8>, side: NonZeroU32) -> Option<Picture> {
    let format = ImageFormat::sniff(&bytes)?;
    drawn_within(&CoverArt { format, bytes }, side)
}

fn rows_at(serves: &dyn Serves, place: &Place) -> Result<Vec<Row>, ProviderError> {
    Ok(match place {
        Place::Artists => first_rows(serves.first_page()?),
        Place::Artist { location, .. } => serves
            .albums_of(location)?
            .into_iter()
            .map(Row::Album)
            .collect(),
        Place::Album { location, .. } | Place::Mix { location, .. } => serves
            .songs_of(location)?
            .into_iter()
            .map(Row::Song)
            .collect(),
        Place::Found { words } => rows_of_found(serves.search(words)?),
    })
}
