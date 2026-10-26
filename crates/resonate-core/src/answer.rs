use crate::Recording;

pub const LONGEST_SHARED_LYRICS: usize = 32 * 1024;

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
pub enum Answer {
    Lyrics(SharedLyrics),
    Study(SharedStudy),
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
            Self::Lyrics(_) => None,
            Self::Study(study) => Some(study.file),
        }
    }

    pub fn travels(&self) -> bool {
        match self {
            Self::Lyrics(lyrics) => {
                !lyrics.text.trim().is_empty() && lyrics.text.len() <= LONGEST_SHARED_LYRICS
            }
            Self::Study(_) => true,
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
}
