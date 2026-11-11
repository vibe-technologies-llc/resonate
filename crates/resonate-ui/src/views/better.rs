use std::{sync::Arc, time::Duration};

use gpui::{Context, Pixels, Point};
use resonate_core::{
    MediaLocation, SampleFormat, TrackId, folded_letters,
    titles::{self, Rating},
};
use resonate_engine::{QueueItem, Unclaimed};
use resonate_library::{Chosen, Codec, Played, Track, Version, codec_named};

use crate::{
    ResonateApp, format,
    icons::Icon,
    models::Notice,
    served::{ServedSong, Serves},
    views::{menu::Menu, root::RootView},
    words,
};

const LENGTHS_AGREE_WITHIN: Duration = Duration::from_secs(3);

const HELD_WEIGHED_AT_MOST: usize = 64;

const NAMES_APART: &str = ", ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sought {
    Better,
    Rated(Rating),
}

impl Sought {
    pub(crate) fn offered_for(track: &Track) -> Vec<Self> {
        let rated = match rating_of(track) {
            Rating::Explicit => vec![Self::Rated(Rating::Clean)],
            Rating::Clean => vec![Self::Rated(Rating::Explicit)],
            Rating::Unmarked => vec![Self::Rated(Rating::Clean), Self::Rated(Rating::Explicit)],
        };
        let mut offered = vec![Self::Better];
        offered.extend(rated);
        offered
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Better => words::better::find_a_better_version(),
            Self::Rated(Rating::Explicit) => words::better::look_for_explicit(),
            Self::Rated(Rating::Clean | Rating::Unmarked) => words::better::look_for_clean(),
        }
    }

    fn looking(self, title: &str) -> String {
        match self {
            Self::Better => words::better::looking(title),
            Self::Rated(Rating::Explicit) => words::better::looking_for_explicit(title),
            Self::Rated(Rating::Clean | Rating::Unmarked) => {
                words::better::looking_for_clean(title)
            }
        }
    }
}

pub(crate) fn rating_of(track: &Track) -> Rating {
    Rating::of(Rating::flagged(track.explicit), &track.title, None)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Quality {
    lossless: bool,
    bits: u8,
    rate: u32,
}

impl Quality {
    pub(crate) fn of(track: &Track) -> Self {
        Self {
            lossless: track.codec.is_lossless(),
            bits: significant_bits(track.spec.format),
            rate: track.spec.rate.hz(),
        }
    }

    pub(crate) fn is_above(self, held: Self) -> bool {
        let at_least =
            self.lossless >= held.lossless && self.bits >= held.bits && self.rate >= held.rate;
        at_least && self != held
    }
}

const fn significant_bits(format: SampleFormat) -> u8 {
    match format {
        SampleFormat::S16 => 16,
        SampleFormat::S24 | SampleFormat::F32 => 24,
        SampleFormat::S32 => 32,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TheSong {
    title: String,
    artist: Option<String>,
    length: Option<Duration>,
    lossless: bool,
    rating: Rating,
}

impl TheSong {
    pub(crate) fn of(track: &Track) -> Self {
        Self {
            title: folded_letters(titles::bare(&track.title)),
            artist: track
                .artist
                .as_deref()
                .map(folded_letters)
                .filter(|artist| !artist.is_empty()),
            length: length_of(track),
            lossless: track.codec.is_lossless(),
            rating: rating_of(track),
        }
    }

    pub(crate) fn words(track: &Track) -> String {
        let title = titles::bare(&track.title);
        match track
            .artist
            .as_deref()
            .filter(|artist| !artist.trim().is_empty())
        {
            Some(artist) => format!("{title} {artist}"),
            None => title.to_owned(),
        }
    }

    pub(crate) fn is_bettered_by(&self, song: &ServedSong) -> bool {
        !self.lossless
            && served_codec(song).is_some_and(Codec::is_lossless)
            && self.names(&song.title, song.artist.as_deref(), song.length)
    }

    pub(crate) fn is_rated_as(&self, wanted: Rating, rating: Rating) -> bool {
        match wanted {
            Rating::Explicit => rating == Rating::Explicit,
            Rating::Clean | Rating::Unmarked => {
                rating == Rating::Clean
                    || (rating == Rating::Unmarked && self.rating == Rating::Explicit)
            }
        }
    }

    pub(crate) fn served_as(&self, sought: Sought, song: &ServedSong) -> bool {
        match sought {
            Sought::Better => self.is_bettered_by(song),
            Sought::Rated(wanted) => {
                self.is_rated_as(wanted, song.rating)
                    && self.names(&song.title, song.artist.as_deref(), song.length)
            }
        }
    }

    pub(crate) fn held_as(&self, wanted: Rating, track: &Track) -> bool {
        self.is_rated_as(wanted, rating_of(track))
            && self.names(&track.title, track.artist.as_deref(), length_of(track))
    }

    fn names(&self, title: &str, artist: Option<&str>, length: Option<Duration>) -> bool {
        let same_title = folded_letters(titles::bare(title)) == self.title;
        let same_artist = match (&self.artist, artist.map(folded_letters)) {
            (Some(held), Some(served)) => {
                served.contains(held.as_str()) || held.contains(served.as_str())
            }
            (None, _) => true,
            (Some(_), None) => false,
        };
        let same_length = match (self.length, length) {
            (Some(held), Some(served)) => held.abs_diff(served) <= LENGTHS_AGREE_WITHIN,
            _ => true,
        };

        same_title && same_artist && same_length
    }
}

fn length_of(track: &Track) -> Option<Duration> {
    track
        .duration
        .map(|frames| frames.to_duration(track.spec.rate))
}

fn served_codec(song: &ServedSong) -> Option<Codec> {
    song.format.as_deref().and_then(codec_named)
}

#[derive(Clone, Debug)]
pub(crate) struct ServedVersion {
    service: String,
    song: ServedSong,
}

impl ServedVersion {
    fn label(&self, sought: Sought) -> String {
        let album = self.song.album.as_deref().filter(|album| !album.is_empty());
        match sought {
            Sought::Better => {
                let codec =
                    served_codec(&self.song).map_or(words::better::lossless(), Codec::as_str);
                match album {
                    Some(album) => words::better::play_served_on_album(album, codec, &self.service),
                    None => words::better::play_served(codec, &self.service),
                }
            }
            Sought::Rated(_) => match album {
                Some(album) => words::better::play_it_on_album(album, &self.service),
                None => words::better::play_it_on(&self.service),
            },
        }
    }
}

fn held_label(sought: Sought, copy: &Track, album: Option<&str>) -> String {
    match (sought, album) {
        (Sought::Better, _) => {
            words::better::play_held(copy.codec.as_str(), format::quality(copy.spec))
        }
        (Sought::Rated(_), Some(album)) => words::better::play_your_copy_from(album),
        (Sought::Rated(_), None) => words::better::play_your_copy(copy.codec.as_str()),
    }
}

pub(crate) fn better_held(track: &Track, copies: Vec<Track>) -> Vec<Track> {
    let held = Quality::of(track);
    let mut better: Vec<Track> = copies
        .into_iter()
        .filter(|copy| Quality::of(copy).is_above(held))
        .collect();
    better.sort_by_key(|copy| {
        let quality = Quality::of(copy);
        std::cmp::Reverse((quality.lossless, quality.bits, quality.rate))
    });
    better
}

pub(crate) fn rated_held(track: &Track, wanted: Rating, found: Vec<Track>) -> Vec<Track> {
    let song = TheSong::of(track);
    found
        .into_iter()
        .filter(|copy| copy.id != track.id && song.held_as(wanted, copy))
        .collect()
}

#[derive(Debug, Default)]
pub(crate) struct Asked {
    served: Vec<ServedVersion>,
    services: Vec<String>,
    refused: Vec<String>,
}

fn asked_of(services: &[Arc<dyn Serves>], words: &str, song: &TheSong, sought: Sought) -> Asked {
    let mut asked = Asked::default();
    for serves in services.iter().filter(|serves| serves.plays_songs()) {
        let service = serves.title().to_owned();
        match serves.search(words) {
            Ok(found) => asked.served.extend(
                found
                    .songs
                    .into_iter()
                    .filter(|found| song.served_as(sought, found))
                    .map(|found| ServedVersion {
                        service: service.clone(),
                        song: found,
                    }),
            ),
            Err(error) => {
                tracing::warn!(%error, service = %service, "a service could not be asked for another version");
                asked.refused.push(service.clone());
            }
        }
        asked.services.push(service);
    }
    asked
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Nothing {
    AlreadyLossless,
    NowhereToAsk,
    NoneOn,
}

pub(crate) fn nothing_said(
    sought: Sought,
    why: Nothing,
    title: &str,
    services: &[String],
) -> String {
    let services = services.join(NAMES_APART);
    match (sought, why) {
        (Sought::Better, Nothing::AlreadyLossless) => words::better::already_lossless(title),
        (Sought::Better, Nothing::NowhereToAsk) => {
            words::better::none_held_and_nowhere_to_ask(title)
        }
        (Sought::Better, Nothing::NoneOn) => words::better::none_found(&services, title),
        (Sought::Rated(Rating::Explicit), Nothing::NoneOn) => {
            words::better::no_explicit_found(&services, title)
        }
        (Sought::Rated(Rating::Explicit), _) => {
            words::better::no_explicit_and_nowhere_to_ask(title)
        }
        (Sought::Rated(_), Nothing::NoneOn) => words::better::no_clean_found(&services, title),
        (Sought::Rated(_), _) => words::better::no_clean_and_nowhere_to_ask(title),
    }
}

fn versions_menu(
    at: Point<Pixels>,
    sought: Sought,
    track: TrackId,
    title: &str,
    held: Vec<(Track, Option<String>)>,
    served: Vec<ServedVersion>,
) -> Menu {
    let mut menu = Menu::at(at);
    for (copy, album) in held {
        let label = held_label(sought, &copy, album.as_deref());
        let chosen = copy.id;
        let playing: Arc<[Track]> = Arc::from([copy]);
        let title = title.to_owned();
        menu = menu.does(Icon::Play, label, move |this, _, cx| {
            this.play(&playing, 0, cx);
            this.choose_the_version(track, Version::Held(chosen), &title, cx);
        });
    }
    for version in served {
        let label = version.label(sought);
        let location: MediaLocation = version.song.location;
        let title = title.to_owned();
        menu = menu.does(Icon::Play, label, move |this, _, cx| {
            this.play_served(vec![location.clone()], 0, cx);
            this.choose_the_version(track, Version::Served(location.clone()), &title, cx);
        });
    }
    menu
}

pub(crate) fn played_as_chosen(
    chosen: &Chosen,
    items: Vec<QueueItem>,
    minting: &mut Unclaimed,
) -> Vec<QueueItem> {
    items
        .into_iter()
        .map(
            |item| match chosen.in_place_of(item.id, &item.location, item.span) {
                Some(Played::Held { track, cut }) => QueueItem {
                    id: *track,
                    location: cut.location.clone(),
                    span: cut.span,
                },
                Some(Played::Served(location)) => {
                    QueueItem::whole(minting.mint(), location.clone())
                }
                None => item,
            },
        )
        .collect()
}

impl RootView {
    pub(crate) fn look_for_a_version(
        &mut self,
        sought: Sought,
        track: Track,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let pressed = self.press_a_lookup();
        let held = match sought {
            Sought::Better => {
                let copies = match track.alternatives {
                    0 => Vec::new(),
                    _ => self.library.read(cx).alternatives_of(track.id),
                };
                better_held(&track, copies)
            }
            Sought::Rated(wanted) => {
                let found = self
                    .library
                    .read(cx)
                    .tracks_named(&TheSong::words(&track), HELD_WEIGHED_AT_MOST);
                rated_held(&track, wanted, found)
            }
        };
        let held: Vec<(Track, Option<String>)> = held
            .into_iter()
            .map(|copy| {
                let album = self
                    .album_named(copy.album_id, &copy.location, copy.span, cx)
                    .map(|album| album.to_string());
                (copy, album)
            })
            .collect();
        let app = cx.global::<ResonateApp>();
        let services = app.serving.opened(&app.online);
        let song = TheSong::of(&track);
        let title = titles::bare(&track.title).to_owned();
        let id = track.id;

        let nothing_to_ask = match sought {
            Sought::Better if song.lossless => Some(Nothing::AlreadyLossless),
            _ if services.iter().all(|serves| !serves.plays_songs()) => Some(Nothing::NowhereToAsk),
            _ => None,
        };
        if let Some(why) = nothing_to_ask {
            match held.is_empty() {
                true => self.report(Notice::Noted(nothing_said(sought, why, &title, &[])), cx),
                false => {
                    self.report(Notice::Done(words::better::found(held.len(), &title)), cx);
                    self.open_a_menu(versions_menu(at, sought, id, &title, held, Vec::new()), cx);
                }
            }
            return;
        }

        self.report(Notice::Noted(sought.looking(&title)), cx);
        let words = TheSong::words(&track);
        let asking = cx
            .background_executor()
            .spawn(async move { asked_of(&services, &words, &song, sought) });
        cx.spawn(async move |this, cx| {
            let asked = asking.await;
            let _ = this.update(cx, |this, cx| {
                if this.window_has_moved_on_from(pressed) {
                    return;
                }
                let found = held.len() + asked.served.len();
                if !asked.refused.is_empty() {
                    let refused = asked.refused.join(NAMES_APART);
                    this.report(Notice::Trouble(words::better::could_not_ask(&refused)), cx);
                }
                if found == 0 {
                    let answered: Vec<String> = asked
                        .services
                        .iter()
                        .filter(|service| !asked.refused.contains(service))
                        .cloned()
                        .collect();
                    if !answered.is_empty() {
                        let said = nothing_said(sought, Nothing::NoneOn, &title, &answered);
                        this.report(Notice::Noted(said), cx);
                    }
                    return;
                }
                this.report(Notice::Done(words::better::found(found, &title)), cx);
                this.open_a_menu(
                    versions_menu(at, sought, id, &title, held, asked.served),
                    cx,
                );
            });
        })
        .detach();
    }

    pub(crate) fn choose_the_version(
        &mut self,
        track: TrackId,
        version: Version,
        title: &str,
        cx: &mut Context<Self>,
    ) {
        let title = title.to_owned();
        self.library.update(cx, |library, cx| {
            library.choose_version(track, version, title, cx);
        });
    }

    pub(crate) fn play_your_own_copy(&mut self, track: Track, cx: &mut Context<Self>) {
        let title = titles::bare(&track.title).to_owned();
        let id = track.id;
        let root = cx.weak_entity();
        self.library.update(cx, |library, cx| {
            library.forget_the_version_chosen(
                id,
                title,
                move |cx| {
                    let _ = root.update(cx, |root, cx| root.play(&[track], 0, cx));
                },
                cx,
            );
        });
    }

    pub(crate) fn with_the_versions_chosen(
        &self,
        items: Vec<QueueItem>,
        inserting: bool,
        cx: &mut Context<Self>,
    ) -> Vec<QueueItem> {
        let chosen = self.library.read(cx).chosen_among(&items);
        if chosen.is_empty() {
            return items;
        }
        let queue = self.player.read(cx).queue();
        let beside = queue.iter().filter(|_| inserting).map(AsRef::as_ref);
        let mut minting = Unclaimed::beside(beside.chain(&items));
        played_as_chosen(&chosen, items, &mut minting)
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use resonate_core::{ChannelLayout, SampleRate, StreamSpec};
    use resonate_library::{ChosenVersion, Codec};

    use super::*;

    fn track(codec: Codec, format: SampleFormat, rate: SampleRate) -> Track {
        Track {
            explicit: None,
            id: TrackId::new(1).expect("a valid id"),
            location: MediaLocation::local("/music/Echoes.flac"),
            title: "Echoes (2011 Remaster)".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            artist_id: None,
            album_id: None,
            track_number: None,
            disc_number: None,
            duration: Some(resonate_core::Frames::from_duration(
                Duration::from_secs(1_412),
                rate,
            )),
            spec: StreamSpec::new(rate, ChannelLayout::Stereo, format),
            codec,
            replay_gain: Default::default(),
            file_size: 0,
            modified: SystemTime::UNIX_EPOCH,
            added: SystemTime::UNIX_EPOCH,
            plays: 0,
            played: None,
            span: None,
            favourite: None,
            genre: None,
            hidden: false,
            alternatives: 0,
            delivered: false,
        }
    }

    fn rated(id: u64, explicit: Option<bool>, title: &str) -> Track {
        Track {
            id: TrackId::new(id).expect("a valid id"),
            explicit,
            title: title.to_owned(),
            location: MediaLocation::local(format!("/music/{id}.flac")),
            ..track(Codec::Flac, SampleFormat::S16, SampleRate::HZ_44100)
        }
    }

    fn served(title: &str, artist: &str, seconds: u64, suffix: &str) -> ServedSong {
        ServedSong {
            rating: Default::default(),
            location: MediaLocation::from_uri("tidal:track/1").expect("a location"),
            title: title.to_owned(),
            artist: Some(artist.to_owned()),
            album: Some("Meddle".to_owned()),
            track: None,
            length: Some(Duration::from_secs(seconds)),
            format: Some(suffix.to_owned()),
            cover: None,
            favourite: None,
        }
    }

    #[test]
    fn a_copy_is_higher_quality_only_where_it_is_no_lower_on_any_count_and_higher_on_one() {
        let cd = Quality::of(&track(Codec::Flac, SampleFormat::S16, SampleRate::HZ_44100));
        let mp3 = Quality::of(&track(Codec::Mp3, SampleFormat::S16, SampleRate::HZ_44100));
        let hi_res = Quality::of(&track(Codec::Flac, SampleFormat::S24, SampleRate::HZ_96000));
        let lossy_hi_rate =
            Quality::of(&track(Codec::Mp3, SampleFormat::S16, SampleRate::HZ_48000));

        assert!(cd.is_above(mp3));
        assert!(hi_res.is_above(cd));
        assert!(hi_res.is_above(mp3));

        assert!(!cd.is_above(cd));
        assert!(!mp3.is_above(cd));
        assert!(!lossy_hi_rate.is_above(cd));
        assert!(!cd.is_above(lossy_hi_rate));
    }

    #[test]
    fn a_served_song_betters_a_lossy_track_only_where_it_is_lossless_and_the_same_song() {
        let song = TheSong::of(&track(Codec::Mp3, SampleFormat::S16, SampleRate::HZ_44100));
        let lossless = TheSong::of(&track(Codec::Flac, SampleFormat::S16, SampleRate::HZ_44100));

        assert!(song.is_bettered_by(&served("Echoes", "Pink Floyd", 1_413, "flac")));
        assert!(song.is_bettered_by(&served("ECHOES", "pink floyd", 1_410, "FLAC")));

        assert!(!song.is_bettered_by(&served("Echoes", "Pink Floyd", 1_413, "mp3")));
        assert!(!song.is_bettered_by(&served("Echoes", "Pink Floyd", 1_413, "m4a")));
        assert!(!song.is_bettered_by(&served("Echoes", "Pink Floyd", 1_300, "flac")));
        assert!(!song.is_bettered_by(&served("Time", "Pink Floyd", 1_413, "flac")));
        assert!(!song.is_bettered_by(&served("Echoes", "Someone Else", 1_413, "flac")));
        assert!(!lossless.is_bettered_by(&served("Echoes", "Pink Floyd", 1_413, "flac")));
    }

    #[test]
    fn the_better_copies_held_are_offered_best_first_and_none_worse_than_the_track() {
        let held = track(Codec::Flac, SampleFormat::S16, SampleRate::HZ_44100);
        let copies = vec![
            track(Codec::Mp3, SampleFormat::S16, SampleRate::HZ_44100),
            track(Codec::Flac, SampleFormat::S24, SampleRate::HZ_48000),
            track(Codec::Flac, SampleFormat::S24, SampleRate::HZ_96000),
        ];

        let better = better_held(&held, copies);

        assert_eq!(
            better.iter().map(|copy| copy.spec.rate).collect::<Vec<_>>(),
            [SampleRate::HZ_96000, SampleRate::HZ_48000]
        );
    }

    #[test]
    fn a_rating_is_offered_only_where_the_track_is_not_already_it() {
        let explicit = rated(1, Some(true), "Echoes");
        let clean = rated(2, None, "Echoes (Clean)");
        let unmarked = rated(3, None, "Echoes");

        assert_eq!(
            Sought::offered_for(&explicit),
            [Sought::Better, Sought::Rated(Rating::Clean)]
        );
        assert_eq!(
            Sought::offered_for(&clean),
            [Sought::Better, Sought::Rated(Rating::Explicit)]
        );
        assert_eq!(
            Sought::offered_for(&unmarked),
            [
                Sought::Better,
                Sought::Rated(Rating::Clean),
                Sought::Rated(Rating::Explicit)
            ]
        );
    }

    #[test]
    fn a_clean_version_of_an_explicit_song_may_be_unmarked_and_an_explicit_one_must_be_marked() {
        let explicit = rated(1, Some(true), "Echoes");
        let found = vec![
            rated(1, Some(true), "Echoes"),
            rated(2, None, "Echoes"),
            rated(3, None, "Echoes (Clean)"),
            rated(4, Some(true), "Echoes"),
            rated(5, None, "Time"),
        ];

        let clean: Vec<u64> = rated_held(&explicit, Rating::Clean, found.clone())
            .iter()
            .map(|copy| copy.id.get())
            .collect();
        let unmarked = rated(9, None, "Echoes");
        let marked: Vec<u64> = rated_held(&unmarked, Rating::Explicit, found.clone())
            .iter()
            .map(|copy| copy.id.get())
            .collect();
        let strictly_clean: Vec<u64> = rated_held(&unmarked, Rating::Clean, found)
            .iter()
            .map(|copy| copy.id.get())
            .collect();

        assert_eq!(clean, [2, 3]);
        assert_eq!(marked, [1, 4]);
        assert_eq!(strictly_clean, [3]);
    }

    #[test]
    fn a_song_with_a_version_chosen_is_queued_as_that_version_wherever_it_is_played() {
        let opus = rated(1, None, "Echoes");
        let flac = rated(2, None, "Echoes");
        let other = rated(3, None, "Time");
        let streamed = MediaLocation::from_uri("tidal:track/7").expect("a location");
        let row = |track: &Track| QueueItem {
            id: track.id,
            location: track.location.clone(),
            span: track.span,
        };
        let chosen = Chosen::of(
            vec![
                ChosenVersion {
                    track: opus.id,
                    version: Version::Held(flac.id),
                },
                ChosenVersion {
                    track: other.id,
                    version: Version::Served(streamed.clone()),
                },
            ],
            &[opus.clone(), flac.clone(), other.clone()],
        );
        let minted_apart = QueueItem {
            id: opus.id,
            location: MediaLocation::local("/elsewhere/unscanned.flac"),
            span: None,
        };
        let items = vec![row(&opus), row(&other), minted_apart.clone()];
        let mut minting = Unclaimed::beside(&items);

        let queued = played_as_chosen(&chosen, items, &mut minting);

        assert_eq!(queued[0], row(&flac));
        assert_eq!(queued[1].location, streamed);
        assert_ne!(queued[1].id, other.id);
        assert_eq!(queued[2], minted_apart);
    }

    #[test]
    fn nothing_found_says_why_and_where_it_was_looked_for() {
        let services = ["TIDAL".to_owned(), "Subsonic".to_owned()];

        assert_eq!(
            nothing_said(Sought::Better, Nothing::NoneOn, "Echoes", &services),
            "No higher quality version of Echoes in your library or on TIDAL, Subsonic"
        );
        assert_eq!(
            nothing_said(Sought::Better, Nothing::AlreadyLossless, "Echoes", &[]),
            "Echoes is already lossless, and no copy you hold is better"
        );
        assert_eq!(
            nothing_said(
                Sought::Rated(Rating::Clean),
                Nothing::NowhereToAsk,
                "Echoes",
                &[]
            ),
            "No Clean version of Echoes in your library, and no service is set up to ask"
        );
        assert_eq!(
            nothing_said(
                Sought::Rated(Rating::Explicit),
                Nothing::NoneOn,
                "Echoes",
                &services[..1]
            ),
            "No Explicit version of Echoes in your library or on TIDAL"
        );
    }
}
