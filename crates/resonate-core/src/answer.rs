use crate::{Chromaprint, Link, Mbid, Recording};

pub const LONGEST_SHARED_LYRICS: usize = 32 * 1024;
pub const LARGEST_SHARED_LYRICSFILE: usize = 1024 * 1024;
pub const LONGEST_SHARED_PRINT: usize = 64 * 1024;
pub const HEAVIEST_SHARED_RELEASE: usize = 64 * 1024;
pub const LINKS_SHARED_AT_MOST: usize = 32;
pub const CODES_SHARED_AT_MOST: usize = 8;
const WEIGHT_OF_AN_ITEM: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Figure(u32);

impl Figure {
    pub const fn of(value: f32) -> Self {
        Self(value.to_bits())
    }

    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn value(self) -> f32 {
        f32::from_bits(self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedLyrics {
    pub text: String,
    pub synced: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedLyricsfile {
    pub text: String,
    pub synced: bool,
    pub document: String,
}

impl SharedLyricsfile {
    pub fn travels(&self) -> bool {
        !self.text.trim().is_empty()
            && self.text.len() <= LONGEST_SHARED_LYRICS
            && !self.document.trim().is_empty()
            && self.document.len() <= LARGEST_SHARED_LYRICSFILE
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedHeard {
    pub recording: Mbid,
    pub score: u8,
    pub title: String,
    pub artist: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SharedRecognition {
    Unheard,
    Heard(SharedHeard),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedPrint {
    pub print: Chromaprint,
    pub recognition: Option<SharedRecognition>,
}

impl SharedPrint {
    pub fn travels(&self) -> bool {
        self.print.encoded().len() <= LONGEST_SHARED_PRINT
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WholeFile {
    pub packets: u64,
    pub frames: u64,
    pub rate: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedStudy {
    pub file: WholeFile,
    pub judged_under: u32,
    pub verdict: String,
    pub cutoff_hz: Option<u32>,
    pub cutoff_drop: Option<Figure>,
    pub lossy_guess: Option<String>,
    pub upsampled_from: Option<u32>,
    pub bits_in_use: Option<u8>,
    pub declared_bits: Option<u8>,
    pub peak: Figure,
    pub rms: Figure,
    pub true_peak: Figure,
    pub loudness: Option<Figure>,
    pub loudness_range: Option<Figure>,
    pub dynamic_range: Option<u8>,
    pub clipped: u64,
    pub mono_as_stereo: bool,
    pub band_levels: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedCredit {
    pub name: String,
    pub joined_by: String,
    pub mbid: Option<Mbid>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedTrack {
    pub position: u32,
    pub number: String,
    pub title: String,
    pub artist: Option<String>,
    pub recording: Option<Mbid>,
    pub track: Option<Mbid>,
    pub length_ms: Option<u32>,
    pub isrcs: Vec<String>,
    pub links: Vec<Link>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedMedium {
    pub position: u32,
    pub format: Option<String>,
    pub title: Option<String>,
    pub tracks: Vec<SharedTrack>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedRelease {
    pub id: Mbid,
    pub group: Option<Mbid>,
    pub title: String,
    pub credit: Vec<SharedCredit>,
    pub date: Option<String>,
    pub country: Option<String>,
    pub label: Option<String>,
    pub catalog_number: Option<String>,
    pub barcode: Option<String>,
    pub kind: Option<String>,
    pub disambiguation: Option<String>,
    pub has_front_cover: bool,
    pub links: Vec<Link>,
    pub media: Vec<SharedMedium>,
}

impl SharedRelease {
    pub fn tracks(&self) -> impl Iterator<Item = &SharedTrack> {
        self.media.iter().flat_map(|medium| &medium.tracks)
    }

    pub fn weight(&self) -> usize {
        let text = |text: &str| text.len() + WEIGHT_OF_AN_ITEM;
        let some_text = |held: &Option<String>| held.as_deref().map_or(1, text);
        let id = |mbid: &Option<Mbid>| mbid.as_ref().map_or(1, |mbid| text(mbid.as_str()));
        let links = |links: &[Link]| links.iter().map(|link| text(&link.url)).sum::<usize>();

        let head = text(self.id.as_str())
            + id(&self.group)
            + text(&self.title)
            + self
                .credit
                .iter()
                .map(|credit| text(&credit.name) + text(&credit.joined_by) + id(&credit.mbid))
                .sum::<usize>()
            + [
                &self.date,
                &self.country,
                &self.label,
                &self.catalog_number,
                &self.barcode,
                &self.kind,
                &self.disambiguation,
            ]
            .into_iter()
            .map(some_text)
            .sum::<usize>()
            + links(&self.links);
        let media = self
            .media
            .iter()
            .map(|medium| {
                some_text(&medium.format)
                    + some_text(&medium.title)
                    + medium
                        .tracks
                        .iter()
                        .map(|track| {
                            text(&track.number)
                                + text(&track.title)
                                + some_text(&track.artist)
                                + id(&track.recording)
                                + id(&track.track)
                                + track.isrcs.iter().map(|code| text(code)).sum::<usize>()
                                + links(&track.links)
                        })
                        .sum::<usize>()
            })
            .sum::<usize>();
        head + media
    }

    fn travels(&self) -> bool {
        let lists_fit = self.links.len() <= LINKS_SHARED_AT_MOST
            && self.tracks().all(|track| {
                track.links.len() <= LINKS_SHARED_AT_MOST
                    && track.isrcs.len() <= CODES_SHARED_AT_MOST
            });
        self.tracks().next().is_some() && lists_fit && self.weight() <= HEAVIEST_SHARED_RELEASE
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Lyrics(SharedLyrics),
    Study(SharedStudy),
    Release(Box<SharedRelease>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answered {
    pub recording: Recording,
    pub at: i64,
    pub answer: Answer,
}

impl Answer {
    pub const fn file(&self) -> Option<WholeFile> {
        match self {
            Self::Lyrics(_) | Self::Release(_) => None,
            Self::Study(study) => Some(study.file),
        }
    }

    pub fn travels(&self) -> bool {
        match self {
            Self::Lyrics(lyrics) => {
                !lyrics.text.trim().is_empty() && lyrics.text.len() <= LONGEST_SHARED_LYRICS
            }
            Self::Study(_) => true,
            Self::Release(release) => release.travels(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_figure_keeps_every_bit_of_its_value() {
        let quiet = Figure::of(-23.5);

        assert_eq!(quiet.value(), -23.5);
        assert_eq!(Figure::from_bits(quiet.bits()), quiet);
        assert_eq!(Figure::of(f32::NAN), Figure::of(f32::NAN));
    }

    #[test]
    fn lyrics_too_long_or_empty_stay_home() {
        let lyrics = |text: String| {
            Answer::Lyrics(SharedLyrics {
                text,
                synced: false,
            })
        };

        assert!(lyrics("A line".into()).travels());
        assert!(!lyrics("  ".into()).travels());
        assert!(!lyrics("a".repeat(LONGEST_SHARED_LYRICS + 1)).travels());
    }

    #[test]
    fn a_lyricsfile_travels_with_its_words_and_stays_home_empty_or_too_heavy() {
        let lyricsfile = |text: &str, document: String| SharedLyricsfile {
            text: text.into(),
            synced: true,
            document,
        };

        assert!(lyricsfile("A line", "version: '1.0'".into()).travels());
        assert!(!lyricsfile("  ", "version: '1.0'".into()).travels());
        assert!(!lyricsfile("A line", " ".into()).travels());
        assert!(
            !lyricsfile(
                &"a".repeat(LONGEST_SHARED_LYRICS + 1),
                "version: '1.0'".into()
            )
            .travels()
        );
        assert!(!lyricsfile("A line", "a".repeat(LARGEST_SHARED_LYRICSFILE + 1)).travels());
    }

    #[test]
    fn a_print_travels_unless_it_is_heavier_than_a_print_can_be() {
        let printed = |encoded: &str| SharedPrint {
            print: Chromaprint::new(encoded, std::time::Duration::from_secs(200)).expect("a print"),
            recognition: Some(SharedRecognition::Unheard),
        };

        assert!(printed("AQADtEmSJEmSJEmS").travels());
        assert!(!printed(&"A".repeat(LONGEST_SHARED_PRINT + 1)).travels());
    }

    fn track(title: &str) -> SharedTrack {
        SharedTrack {
            position: 1,
            number: "1".into(),
            title: title.into(),
            artist: None,
            recording: None,
            track: None,
            length_ms: Some(1_000),
            isrcs: Vec::new(),
            links: Vec::new(),
        }
    }

    fn release(tracks: Vec<SharedTrack>) -> Answer {
        Answer::Release(Box::new(SharedRelease {
            id: Mbid::new("b84ee12a-09ef-421b-82de-0441a926375b").expect("an mbid"),
            group: None,
            title: "Meddle".into(),
            credit: Vec::new(),
            date: None,
            country: None,
            label: None,
            catalog_number: None,
            barcode: None,
            kind: None,
            disambiguation: None,
            has_front_cover: false,
            links: Vec::new(),
            media: vec![SharedMedium {
                position: 1,
                format: None,
                title: None,
                tracks,
            }],
        }))
    }

    #[test]
    fn a_release_with_no_tracks_or_heavier_than_the_server_keeps_stays_home() {
        let heavy = (0..HEAVIEST_SHARED_RELEASE / 1024)
            .map(|_| track(&"a".repeat(1024)))
            .collect();
        let coded = SharedTrack {
            isrcs: vec!["GBN9Y1100001".into(); CODES_SHARED_AT_MOST + 1],
            ..track("Echoes")
        };

        assert!(release(vec![track("Echoes")]).travels());
        assert!(!release(Vec::new()).travels());
        assert!(!release(heavy).travels());
        assert!(!release(vec![coded]).travels());
    }
}
