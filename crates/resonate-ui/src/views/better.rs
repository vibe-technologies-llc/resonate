use std::sync::Arc;

use gpui::{Context, Pixels, Point};
use resonate_core::{
    TrackId,
    titles::{self, Rating},
};
use resonate_engine::QueueItem;
use resonate_library::{
    Chosen, Codec, KeptAs, Played, SongHeard, SongToKeep, TheSong, Track, VersionSought,
    better_held, codec_named, rated_held, rating_of,
};

use crate::{
    ResonateApp, format,
    icons::Icon,
    models::Notice,
    serial::Serial,
    served::{ServedSong, Serves},
    serving::song_to_keep,
    views::{menu::Menu, root::RootView},
    words,
};

const HELD_WEIGHED_AT_MOST: usize = 64;

const NAMES_APART: &str = ", ";

pub(crate) type Sought = VersionSought;

pub(crate) fn sought_label(sought: Sought) -> &'static str {
    match sought {
        Sought::Better => words::better::find_a_better_version(),
        Sought::Rated(Rating::Explicit) => words::better::look_for_explicit(),
        Sought::Rated(Rating::Clean | Rating::Unmarked) => words::better::look_for_clean(),
    }
}

fn looking(sought: Sought, title: &str) -> String {
    match sought {
        Sought::Better => words::better::looking(title),
        Sought::Rated(Rating::Explicit) => words::better::looking_for_explicit(title),
        Sought::Rated(Rating::Clean | Rating::Unmarked) => words::better::looking_for_clean(title),
    }
}

pub(crate) fn offered_for(track: &Track) -> Vec<Sought> {
    Sought::offered_for(rating_of(track))
}

fn heard_of(song: &ServedSong) -> SongHeard<'_> {
    SongHeard {
        title: &song.title,
        artist: song.artist.as_deref(),
        length: song.length,
        lossless: served_codec(song).is_some_and(Codec::is_lossless),
        rating: song.rating,
    }
}

fn served_codec(song: &ServedSong) -> Option<Codec> {
    song.format.as_deref().and_then(codec_named)
}

#[derive(Clone, Debug)]
pub(crate) struct ServedVersion {
    service: String,
    song: ServedSong,
    kept: SongToKeep,
}

impl ServedVersion {
    fn label(&self, sought: Sought) -> String {
        let album = self.song.album.as_deref().filter(|album| !album.is_empty());
        match sought {
            Sought::Better => {
                let codec =
                    served_codec(&self.song).map_or(words::better::lossless(), Codec::as_str);
                match album {
                    Some(album) => {
                        words::better::download_from_served_on_album(album, codec, &self.service)
                    }
                    None => words::better::download_from_served(codec, &self.service),
                }
            }
            Sought::Rated(_) => match album {
                Some(album) => words::better::download_it_on_album(album, &self.service),
                None => words::better::download_it_from(&self.service),
            },
        }
    }

    const fn kept_as(sought: Sought) -> KeptAs {
        match sought {
            Sought::Better => KeptAs::Lossless,
            Sought::Rated(_) => KeptAs::AsServed,
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
                    .filter(|found| song.served_as(sought, heard_of(found)))
                    .filter_map(|found| {
                        Some(ServedVersion {
                            service: service.clone(),
                            kept: song_to_keep(&found)?,
                            song: found,
                        })
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
    song: &Track,
    title: &str,
    held: Vec<(Track, Option<String>)>,
    served: Vec<ServedVersion>,
) -> Menu {
    let mut menu = Menu::at(at);
    let track = song.id;
    for (copy, album) in held {
        let label = held_label(sought, &copy, album.as_deref());
        let chosen = copy.id;
        let playing: Arc<[Track]> = Arc::from([copy]);
        let title = title.to_owned();
        menu = menu.does(Icon::Play, label, move |this, _, cx| {
            this.play(&playing, 0, cx);
            this.choose_the_version(track, chosen, &title, cx);
        });
    }
    for version in served {
        let label = version.label(sought);
        let kept = version.kept;
        let title = title.to_owned();
        menu = menu.does(Icon::Download, label, move |this, _, cx| {
            let (kept, title) = (kept.clone(), title.clone());
            this.library.update(cx, |library, cx| {
                library.keep_as_a_version(track, kept, ServedVersion::kept_as(sought), title, cx);
            });
        });
    }
    menu
}

fn words_of(track: &Track) -> String {
    TheSong::words(&track.title, track.artist.as_deref())
}

pub(crate) fn played_as_chosen(chosen: &Chosen, items: Vec<QueueItem>) -> Vec<QueueItem> {
    items
        .into_iter()
        .map(
            |item| match chosen.in_place_of(item.id, &item.location, item.span) {
                Some(Played { track, cut }) => QueueItem {
                    id: *track,
                    location: cut.location.clone(),
                    span: cut.span,
                },
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
        let library = self.library.read(cx).catalog();
        let asked = track.clone();
        let reading = cx.background_executor().spawn(async move {
            match sought {
                Sought::Better => {
                    let copies = match asked.alternatives {
                        0 => Vec::new(),
                        _ => library.alternatives_of(asked.id).unwrap_or_else(|error| {
                            tracing::warn!(%error, "the other copies of a track could not be read");
                            Vec::new()
                        }),
                    };
                    better_held(&asked, copies)
                }
                Sought::Rated(wanted) => {
                    let query = resonate_library::TrackQuery {
                        text: Some(words_of(&asked)),
                        limit: Some(HELD_WEIGHED_AT_MOST),
                        ..Default::default()
                    };
                    let found = library.tracks(&query).unwrap_or_else(|error| {
                        tracing::warn!(%error, "the songs named alike could not be read");
                        Vec::new()
                    });
                    rated_held(&asked, wanted, found)
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let held = reading.await;
            let _ = this.update(cx, |this, cx| {
                if this.window_has_moved_on_from(pressed, cx) {
                    return;
                }
                this.look_for_versions_beyond(sought, track, held, at, pressed, cx);
            });
        })
        .detach();
    }

    fn look_for_versions_beyond(
        &mut self,
        sought: Sought,
        track: Track,
        held: Vec<Track>,
        at: Point<Pixels>,
        pressed: Serial,
        cx: &mut Context<Self>,
    ) {
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

        let nothing_to_ask = match sought {
            Sought::Better if song.is_lossless() => Some(Nothing::AlreadyLossless),
            _ if services.iter().all(|serves| !serves.plays_songs()) => Some(Nothing::NowhereToAsk),
            _ => None,
        };
        if let Some(why) = nothing_to_ask {
            match held.is_empty() {
                true => self.report(Notice::Noted(nothing_said(sought, why, &title, &[])), cx),
                false => {
                    self.report(Notice::Done(words::better::found(held.len(), &title)), cx);
                    self.open_a_menu(
                        versions_menu(at, sought, &track, &title, held, Vec::new()),
                        cx,
                    );
                }
            }
            return;
        }

        self.report(Notice::Noted(looking(sought, &title)), cx);
        let words = words_of(&track);
        let asking = cx
            .background_executor()
            .spawn(async move { asked_of(&services, &words, &song, sought) });
        cx.spawn(async move |this, cx| {
            let asked = asking.await;
            let _ = this.update(cx, |this, cx| {
                if this.window_has_moved_on_from(pressed, cx) {
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
                    versions_menu(at, sought, &track, &title, held, asked.served),
                    cx,
                );
            });
        })
        .detach();
    }

    pub(crate) fn choose_the_version(
        &mut self,
        track: TrackId,
        copy: TrackId,
        title: &str,
        cx: &mut Context<Self>,
    ) {
        let title = title.to_owned();
        self.library.update(cx, |library, cx| {
            library.choose_version(track, copy, title, cx);
        });
    }

    pub(crate) fn with_the_versions_chosen(
        &self,
        items: Vec<QueueItem>,
        cx: &mut Context<Self>,
    ) -> Vec<QueueItem> {
        let chosen = self.library.read(cx).chosen_among(&items);
        match chosen.is_empty() {
            true => items,
            false => played_as_chosen(&chosen, items),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use resonate_core::{ChannelLayout, MediaLocation, SampleFormat, SampleRate, StreamSpec};
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

    #[test]
    fn a_song_with_a_version_chosen_is_queued_as_that_version_wherever_it_is_played() {
        let opus = rated(1, None, "Echoes");
        let flac = rated(2, None, "Echoes");
        let other = rated(3, None, "Time");
        let row = |track: &Track| QueueItem {
            id: track.id,
            location: track.location.clone(),
            span: track.span,
        };
        let chosen = Chosen::of(
            vec![ChosenVersion {
                track: opus.id,
                copy: flac.id,
            }],
            &[opus.clone(), flac.clone(), other.clone()],
        );
        let minted_apart = QueueItem {
            id: opus.id,
            location: MediaLocation::local("/elsewhere/unscanned.flac"),
            span: None,
        };
        let items = vec![row(&opus), row(&other), minted_apart.clone()];

        let queued = played_as_chosen(&chosen, items);

        assert_eq!(queued[0], row(&flac));
        assert_eq!(queued[1], row(&other));
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
