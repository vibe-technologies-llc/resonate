#![recursion_limit = "512"]

pub mod abx;
mod answer;
mod appearance;
mod buffer;
mod calendar;
mod channel;
mod config_key;
mod date;
pub mod elapsed;
pub mod eq;
mod error;
pub mod file_facts;
mod fold;
mod format;
pub mod gaming;
mod hints;
mod hosted;
mod id;
mod identity;
mod link;
mod media;
mod metered;
pub mod naming;
pub mod os_bytes;
pub mod path_text;
pub mod positioned;
mod presence;
mod print;
mod quantise;
mod recorded;
mod resume;
mod rt;
mod span;
mod stamp;
mod synced;
pub mod text;
mod time;
pub mod titles;
mod volume;
#[cfg(any(windows, test))]
mod volume_mounts;
#[cfg(any(windows, test))]
mod windows_uri;
pub mod wrapped;
pub mod writer;
mod zone;

mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

pub use crate::{
    answer::{
        Answer, Answered, CODES_SHARED_AT_MOST, Figure, HEAVIEST_SHARED_RELEASE,
        LARGEST_SHARED_LYRICSFILE, LINKS_SHARED_AT_MOST, LONGEST_SHARED_LYRICS,
        LONGEST_SHARED_PRINT, SharedBandPeaks, SharedCredit, SharedHeard, SharedLyrics,
        SharedLyricsfile, SharedMedium, SharedPrint, SharedRecognition, SharedRelease, SharedStudy,
        SharedTrack, WholeFile,
    },
    appearance::{
        Accent, Appearance, ArtistsDrawn, DesktopLook, Following, Ground, ScrollbarMode, Spectral,
        SpectrumBands, SpectrumFalls, SpectrumFloor, SpectrumTilt, TextSize, Theme,
    },
    buffer::{AudioBuffer, SampleData},
    calendar::Calendar,
    channel::{ChannelCount, ChannelLayout, ChannelPosition, Places},
    config_key::{ConfigKey, Sorted, Switch, Switches, Valued},
    date::{CivilDate, SECONDS_PER_DAY, seconds_since_the_epoch},
    eq::{HeadYaw, SpeakerAngle, SpeakerElevation, Speakers},
    error::{Error, Result},
    fold::{folded_letters, is_lettered, words_of_a_name},
    format::{BitDepth, RateFamily, Ratio, SampleFormat, SampleRate, StreamSpec},
    hints::{MeasuredGain, TrackHints},
    hosted::{RESONATE_HOST, RESONATE_SOCKET_ADDRESS, RESONATE_WEB_ADDRESS},
    id::{AlbumId, ArtistId, ListenId, PlaylistId, ReleaseTrackId, TrackId, WantId},
    identity::{Isrc, Mbid},
    link::{Link, Relation, Service},
    media::{
        AUDIO_EXTENSIONS, HRIR_EXTENSIONS, Locator, MediaLocation, PICTURE_EXTENSIONS, SourceId,
        names_a_picture, names_audio, uri_escaped, uri_unescaped,
    },
    metered::{Gate, Meter, Metering},
    presence::{AppId, Icon, Pictured, Presence, Shown},
    print::Chromaprint,
    recorded::{Recording, RecordingKey, title_words},
    resume::{Reordered, Resumable, Resumption, plays_in},
    rt::{RtFault, Silence},
    span::Span,
    stamp::{QueueStamp, RowStamp},
    synced::SyncedSongs,
    text::{LegacyEncoding, TextEncoding},
    time::{FrameSpan, Frames},
    volume::{AppliedGain, Decibels, Gain, Trim, Volume},
    zone::{
        KeptZone, LONGEST_ZONE_NAME, RemoteTrust, ZoneId, ZoneName, remote_trust_asked,
        remote_trust_title, remote_trust_told,
    },
};

#[cfg(test)]
mod said_through_the_catalog {
    use std::path::Path;

    const PROPER: &[&str] = &[
        "Resonate",
        "Midnight",
        "Graphite",
        "Plum",
        "Rosé Pine",
        "Rosé Pine Moon",
        "Catppuccin Mocha",
        "Catppuccin Macchiato",
        "Catppuccin Frappé",
        "Nord",
        "Gruvbox Dark",
        "Tokyo Night",
        "Dracula",
        "One Dark",
        "Solarized Dark",
        "High Contrast",
        "Resonate Light",
        "High Contrast Light",
        "Resonate Wrapped",
        "free streaming",
        "purchase for download",
        "purchase for mail-order",
        "official homepage",
        "social network",
        "youtube music",
        "apple music",
        "amazon music",
        "wikimedia commons",
        "Spotify",
        "Tidal",
        "Apple Music",
        "Deezer",
        "Qobuz",
        "Amazon Music",
        "YouTube",
        "YouTube Music",
        "Bandcamp",
        "SoundCloud",
        "Beatport",
        "Discogs",
        "Wikipedia",
        "Wikidata",
        "AllMusic",
        "Last.fm",
        "Wikimedia Commons",
        "Facebook",
        "Instagram",
    ];

    #[test]
    fn every_word_core_hands_a_listener_is_said_through_its_catalog() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let found = resonate_words_build::prose_in(&root, PROPER).expect("core's sources read");

        let written: Vec<String> = found
            .iter()
            .flat_map(|(path, prose)| {
                prose
                    .iter()
                    .map(move |said| format!("{}:{} {:?}", path.display(), said.line, said.text))
            })
            .collect();

        assert!(
            written.is_empty(),
            "words written in the code:\n{}",
            written.join("\n")
        );
    }
}
