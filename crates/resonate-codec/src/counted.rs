use std::{fs::File, path::Path};

use lofty::{
    aac::AacFile,
    ape::{ApeFile, ApeItem, ApeTag},
    config::{ParseOptions, WriteOptions},
    error::{FileEncodingError, FileParseError},
    file::{AudioFile, FileType},
    flac::FlacFile,
    id3::v2::{Id3v2Tag, Id3v2Version},
    iff::{aiff::AiffFile, wav::WavFile},
    io::FileLike,
    mp4::{Atom, AtomData, AtomIdent, Ilst, Mp4File},
    mpeg::MpegFile,
    ogg::{OpusFile, VorbisFile, tag::VorbisComments},
    tag::{ItemValue, Tag, TagExt as _, TagType},
    wavpack::WavPackFile,
};

const COMMENTED_PLAYS: &str = "FMPS_PLAYCOUNT";
const FRAMED_PLAYS: &str = "FMPS_PlayCount";
const ITUNES_MEAN: &str = "com.apple.iTunes";
const ATOM_PLAYS: &str = "FMPS_Playcount";
const APE_RATING: &str = "FMPS_RATING";
const APE_FAVOURITE: &str = "1.0";

pub(crate) const fn counts(kind: TagType) -> bool {
    matches!(
        kind,
        TagType::VorbisComments | TagType::Ape | TagType::Mp4Ilst | TagType::Id3v2
    )
}

pub(crate) enum Counted {
    Commented(VorbisComments),
    Ape(ApeTag),
    Atoms(Ilst),
    Framed(Id3v2Tag),
}

impl Counted {
    pub(crate) fn of(tag: Tag) -> Option<Self> {
        match tag.tag_type() {
            TagType::VorbisComments => Some(Self::Commented(tag.into())),
            TagType::Ape => Some(Self::Ape(tag.into())),
            TagType::Mp4Ilst => Some(Self::Atoms(tag.into())),
            TagType::Id3v2 => Some(Self::Framed(tag.into())),
            _ => None,
        }
    }

    pub(crate) fn read(path: &Path, kind: FileType) -> Result<Option<Self>, FileParseError> {
        let mut file = File::open(path)?;
        let options = ParseOptions::new()
            .read_properties(false)
            .read_cover_art(false);
        Ok(match kind {
            FileType::Flac => FlacFile::read_from(&mut file, options)?
                .vorbis_comments()
                .cloned()
                .map(Self::Commented),
            FileType::Vorbis => Some(Self::Commented(
                VorbisFile::read_from(&mut file, options)?
                    .vorbis_comments()
                    .clone(),
            )),
            FileType::Opus => Some(Self::Commented(
                OpusFile::read_from(&mut file, options)?
                    .vorbis_comments()
                    .clone(),
            )),
            FileType::Ape => ApeFile::read_from(&mut file, options)?
                .ape()
                .cloned()
                .map(Self::Ape),
            FileType::WavPack => WavPackFile::read_from(&mut file, options)?
                .ape()
                .cloned()
                .map(Self::Ape),
            FileType::Mp4 => Mp4File::read_from(&mut file, options)?
                .ilst()
                .cloned()
                .map(Self::Atoms),
            FileType::Mpeg => MpegFile::read_from(&mut file, options)?
                .id3v2()
                .cloned()
                .map(Self::Framed),
            FileType::Aac => AacFile::read_from(&mut file, options)?
                .id3v2()
                .cloned()
                .map(Self::Framed),
            FileType::Aiff => AiffFile::read_from(&mut file, options)?
                .id3v2()
                .cloned()
                .map(Self::Framed),
            FileType::Wav => WavFile::read_from(&mut file, options)?
                .id3v2()
                .cloned()
                .map(Self::Framed),
            _ => None,
        })
    }

    pub(crate) fn holds_id3v2_3(path: &Path, kind: FileType) -> bool {
        let framed = matches!(
            kind,
            FileType::Mpeg | FileType::Aac | FileType::Aiff | FileType::Wav
        );
        framed
            && matches!(
                Self::read(path, kind),
                Ok(Some(Self::Framed(frames))) if frames.original_version() == Id3v2Version::V3
            )
    }

    pub(crate) fn plays(&self) -> Option<u64> {
        let text = match self {
            Self::Commented(comments) => comments.get(COMMENTED_PLAYS),
            Self::Ape(tag) => tag.get(COMMENTED_PLAYS).and_then(ape_text),
            Self::Atoms(atoms) => atoms.get(&plays_atom()).and_then(atom_text),
            Self::Framed(frames) => frames.get_user_text(FRAMED_PLAYS),
        }?;
        whole_plays(text)
    }

    pub(crate) fn count(&mut self, plays: u64) {
        match self {
            Self::Commented(comments) => {
                comments.remove(COMMENTED_PLAYS).for_each(drop);
                if plays > 0 {
                    comments.insert(COMMENTED_PLAYS.to_owned(), plays.to_string());
                }
            }
            Self::Ape(tag) => {
                tag.remove(COMMENTED_PLAYS);
                if plays > 0
                    && let Ok(item) = ApeItem::new(
                        COMMENTED_PLAYS.to_owned(),
                        ItemValue::Text(plays.to_string()),
                    )
                {
                    tag.insert(item);
                }
            }
            Self::Atoms(atoms) => {
                atoms.remove(&plays_atom()).for_each(drop);
                if plays > 0 {
                    atoms.insert(Atom::new(plays_atom(), AtomData::UTF8(plays.to_string())));
                }
            }
            Self::Framed(frames) => {
                frames.remove_user_text(FRAMED_PLAYS);
                if plays > 0 {
                    frames.insert_user_text(FRAMED_PLAYS.to_owned(), plays.to_string());
                }
            }
        }
    }

    pub(crate) fn favoured_in_ape(&self) -> Option<bool> {
        match self {
            Self::Ape(tag) => Some(
                tag.get(APE_RATING)
                    .and_then(ape_text)
                    .and_then(|text| text.trim().parse::<f64>().ok())
                    .is_some_and(|rating| rating >= 1.0),
            ),
            _ => None,
        }
    }

    pub(crate) fn favour_in_ape(&mut self, favourite: bool) {
        if let Self::Ape(tag) = self {
            tag.remove(APE_RATING);
            if favourite
                && let Ok(item) = ApeItem::new(
                    APE_RATING.to_owned(),
                    ItemValue::Text(APE_FAVOURITE.to_owned()),
                )
            {
                tag.insert(item);
            }
        }
    }

    pub(crate) fn save<F: FileLike>(
        &self,
        file: &mut F,
        options: WriteOptions,
    ) -> Result<(), FileEncodingError> {
        match self {
            Self::Commented(comments) => comments.save_to(file, options),
            Self::Ape(tag) => tag.save_to(file, options),
            Self::Atoms(atoms) => atoms.save_to(file, options),
            Self::Framed(frames) => frames.save_to(file, options),
        }
    }
}

fn plays_atom() -> AtomIdent<'static> {
    AtomIdent::Freeform {
        mean: ITUNES_MEAN.into(),
        name: ATOM_PLAYS.into(),
    }
}

fn ape_text(item: &ApeItem) -> Option<&str> {
    match item.value() {
        ItemValue::Text(text) => Some(text),
        _ => None,
    }
}

fn atom_text<'a>(atom: &'a Atom<'_>) -> Option<&'a str> {
    atom.data().find_map(|data| match data {
        AtomData::UTF8(text) | AtomData::UTF16(text) => Some(text.as_str()),
        _ => None,
    })
}

fn whole_plays(text: &str) -> Option<u64> {
    let text = text.trim();
    text.parse::<u64>().ok().or_else(|| {
        text.parse::<f64>()
            .ok()
            .filter(|plays| plays.is_finite() && *plays >= 0.0)
            .map(|plays| plays as u64)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_count_is_read_as_a_whole_number_however_a_player_spelt_it() {
        assert_eq!(whole_plays("7"), Some(7));
        assert_eq!(whole_plays(" 7.0 "), Some(7));
        assert_eq!(whole_plays("12.9"), Some(12));
        assert_eq!(whole_plays("-1"), None);
        assert_eq!(whole_plays("NaN"), None);
        assert_eq!(whole_plays("many"), None);
    }
}
