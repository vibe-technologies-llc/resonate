use std::sync::Arc;

use gpui::{Context, Pixels, Point};
use resonate_core::{
    MediaLocation, TrackId,
    titles::{self, Rating},
};
use resonate_engine::{QueueItem, Unclaimed};
use resonate_library::{
    Chosen, Codec, Played, SongHeard, TheSong, Track, Version, VersionSought, better_held,
    codec_named, rated_held, rating_of,
};

use crate::{
    ResonateApp, format,
    icons::Icon,
    models::Notice,
    served::{ServedSong, Serves},
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

fn words_of(track: &Track) -> String {
    TheSong::words(&track.title, track.artist.as_deref())
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
                    .tracks_named(&words_of(&track), HELD_WEIGHED_AT_MOST);
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
            Sought::Better if song.is_lossless() => Some(Nothing::AlreadyLossless),
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

        self.report(Notice::Noted(looking(sought, &title)), cx);
        let words = words_of(&track);
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
    use std::time::{Duration, SystemTime};

    use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
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
