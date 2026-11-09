use std::{sync::Arc, time::Duration};

use gpui::{Context, Pixels, Point};
use resonate_core::{MediaLocation, SampleFormat, folded_letters, titles};
use resonate_library::{Codec, Track, codec_named};

use crate::{
    ResonateApp, format,
    icons::Icon,
    models::Notice,
    served::{ServedSong, Serves},
    views::{menu::Menu, root::RootView},
    words,
};

const LENGTHS_AGREE_WITHIN: Duration = Duration::from_secs(3);

pub(crate) fn find_a_better_version() -> &'static str {
    words::better::find_a_better_version()
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
pub(crate) struct Sought {
    title: String,
    artist: Option<String>,
    length: Option<Duration>,
    lossless: bool,
}

impl Sought {
    pub(crate) fn of(track: &Track) -> Self {
        Self {
            title: folded_letters(titles::bare(&track.title)),
            artist: track
                .artist
                .as_deref()
                .map(folded_letters)
                .filter(|artist| !artist.is_empty()),
            length: track
                .duration
                .map(|frames| frames.to_duration(track.spec.rate)),
            lossless: track.codec.is_lossless(),
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
        !self.lossless && served_codec(song).is_some_and(Codec::is_lossless) && self.names(song)
    }

    fn names(&self, song: &ServedSong) -> bool {
        let same_title = folded_letters(titles::bare(&song.title)) == self.title;
        let same_artist = match (&self.artist, song.artist.as_deref().map(folded_letters)) {
            (Some(held), Some(served)) => {
                served.contains(held.as_str()) || held.contains(served.as_str())
            }
            (None, _) => true,
            (Some(_), None) => false,
        };
        let same_length = match (self.length, song.length) {
            (Some(held), Some(served)) => held.abs_diff(served) <= LENGTHS_AGREE_WITHIN,
            _ => true,
        };

        same_title && same_artist && same_length
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
    fn label(&self) -> String {
        let codec = served_codec(&self.song).map_or(words::better::lossless(), Codec::as_str);
        match self.song.album.as_deref().filter(|album| !album.is_empty()) {
            Some(album) => words::better::play_served_on_album(album, codec, &self.service),
            None => words::better::play_served(codec, &self.service),
        }
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

fn asked_of(services: &[Arc<dyn Serves>], words: &str, sought: &Sought) -> Vec<ServedVersion> {
    services
        .iter()
        .filter(|serves| serves.plays_songs())
        .flat_map(|serves| match serves.search(words) {
            Ok(found) => found
                .songs
                .into_iter()
                .filter(|song| sought.is_bettered_by(song))
                .map(|song| ServedVersion {
                    service: serves.title().to_owned(),
                    song,
                })
                .collect(),
            Err(error) => {
                tracing::warn!(%error, service = serves.title(), "a service could not be asked for a better version");
                Vec::new()
            }
        })
        .collect()
}

fn versions_menu(at: Point<Pixels>, held: Vec<Track>, served: Vec<ServedVersion>) -> Menu {
    let mut menu = Menu::at(at);
    for copy in held {
        let label = words::better::play_held(copy.codec.as_str(), format::quality(copy.spec));
        let playing: Arc<[Track]> = Arc::from([copy]);
        menu = menu.does(Icon::Play, label, move |this, _, cx| {
            this.play(&playing, 0, cx);
        });
    }
    for version in served {
        let label = version.label();
        let location: MediaLocation = version.song.location;
        menu = menu.does(Icon::Play, label, move |this, _, cx| {
            this.play_served(vec![location.clone()], 0, cx);
        });
    }
    menu
}

impl RootView {
    pub(crate) fn look_for_a_better_version(
        &mut self,
        track: Track,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let copies = match track.alternatives {
            0 => Vec::new(),
            _ => self.library.read(cx).alternatives_of(track.id),
        };
        let held = better_held(&track, copies);
        let app = cx.global::<ResonateApp>();
        let services = app.serving.opened(&app.online);
        let sought = Sought::of(&track);

        if sought.lossless || services.is_empty() {
            match held.is_empty() {
                true => self.report(Notice::Noted(words::better::none_found().to_owned()), cx),
                false => self.open_a_menu(versions_menu(at, held, Vec::new()), cx),
            }
            return;
        }

        self.report(Notice::Noted(words::better::looking().to_owned()), cx);
        let words = Sought::words(&track);
        let asking = cx
            .background_executor()
            .spawn(async move { asked_of(&services, &words, &sought) });
        cx.spawn(async move |this, cx| {
            let served = asking.await;
            let _ = this.update(cx, |this, cx| {
                if held.is_empty() && served.is_empty() {
                    this.report(Notice::Noted(words::better::none_found().to_owned()), cx);
                } else {
                    this.open_a_menu(versions_menu(at, held, served), cx);
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use resonate_core::{ChannelLayout, SampleRate, StreamSpec, TrackId};
    use resonate_library::Codec;

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
        let sought = Sought::of(&track(Codec::Mp3, SampleFormat::S16, SampleRate::HZ_44100));
        let lossless = Sought::of(&track(Codec::Flac, SampleFormat::S16, SampleRate::HZ_44100));

        assert!(sought.is_bettered_by(&served("Echoes", "Pink Floyd", 1_413, "flac")));
        assert!(sought.is_bettered_by(&served("ECHOES", "pink floyd", 1_410, "FLAC")));

        assert!(!sought.is_bettered_by(&served("Echoes", "Pink Floyd", 1_413, "mp3")));
        assert!(!sought.is_bettered_by(&served("Echoes", "Pink Floyd", 1_413, "m4a")));
        assert!(!sought.is_bettered_by(&served("Echoes", "Pink Floyd", 1_300, "flac")));
        assert!(!sought.is_bettered_by(&served("Time", "Pink Floyd", 1_413, "flac")));
        assert!(!sought.is_bettered_by(&served("Echoes", "Someone Else", 1_413, "flac")));
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
}
