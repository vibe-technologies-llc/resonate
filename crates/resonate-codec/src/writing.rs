use std::{
    fmt,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use lofty::{
    config::WriteOptions,
    file::{FileType, TaggedFileExt as _},
    picture::{MimeType, Picture, PictureType},
    probe::Probe,
    tag::{ItemKey, Tag, TagExt as _},
};
use resonate_core::MediaLocation;

use crate::{
    CoverArt, Error, Picturing, Result, TagSet, probe_pictured,
    source::Sources,
    tags::{TagSource, Tagged},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TagField {
    Title,
    Artist,
    Album,
    AlbumArtist,
    TrackNumber,
    TrackTotal,
    DiscNumber,
    DiscTotal,
    Date,
    Label,
    CatalogNumber,
    Barcode,
    Isrc,
    MusicBrainzTrackId,
    MusicBrainzReleaseTrackId,
    MusicBrainzAlbumId,
    MusicBrainzArtistId,
    MusicBrainzAlbumArtistId,
    MusicBrainzReleaseGroupId,
}

impl TagField {
    pub const ALL: [Self; 19] = [
        Self::Title,
        Self::Artist,
        Self::Album,
        Self::AlbumArtist,
        Self::TrackNumber,
        Self::TrackTotal,
        Self::DiscNumber,
        Self::DiscTotal,
        Self::Date,
        Self::Label,
        Self::CatalogNumber,
        Self::Barcode,
        Self::Isrc,
        Self::MusicBrainzTrackId,
        Self::MusicBrainzReleaseTrackId,
        Self::MusicBrainzAlbumId,
        Self::MusicBrainzArtistId,
        Self::MusicBrainzAlbumArtistId,
        Self::MusicBrainzReleaseGroupId,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::AlbumArtist => "album artist",
            Self::TrackNumber => "track number",
            Self::TrackTotal => "track total",
            Self::DiscNumber => "disc number",
            Self::DiscTotal => "disc total",
            Self::Date => "date",
            Self::Label => "label",
            Self::CatalogNumber => "catalogue number",
            Self::Barcode => "barcode",
            Self::Isrc => "ISRC",
            Self::MusicBrainzTrackId => "musicbrainz track id",
            Self::MusicBrainzReleaseTrackId => "musicbrainz release track id",
            Self::MusicBrainzAlbumId => "musicbrainz album id",
            Self::MusicBrainzArtistId => "musicbrainz artist id",
            Self::MusicBrainzAlbumArtistId => "musicbrainz album artist id",
            Self::MusicBrainzReleaseGroupId => "musicbrainz release group id",
        }
    }

    pub fn read(self, tags: &TagSet) -> Option<String> {
        match self {
            Self::Title => tags.title.clone(),
            Self::Artist => tags.artist.clone(),
            Self::Album => tags.album.clone(),
            Self::AlbumArtist => tags.album_artist.clone(),
            Self::TrackNumber => tags.track_number.map(|number| number.to_string()),
            Self::TrackTotal => tags.track_total.map(|total| total.to_string()),
            Self::DiscNumber => tags.disc_number.map(|number| number.to_string()),
            Self::DiscTotal => tags.disc_total.map(|total| total.to_string()),
            Self::Date => tags.date.clone(),
            Self::Label => tags.label.clone(),
            Self::CatalogNumber => tags.catalog_number.clone(),
            Self::Barcode => tags.barcode.clone(),
            Self::Isrc => tags.isrc.clone(),
            Self::MusicBrainzTrackId => tags.musicbrainz_track_id.clone(),
            Self::MusicBrainzReleaseTrackId => tags.musicbrainz_release_track_id.clone(),
            Self::MusicBrainzAlbumId => tags.musicbrainz_album_id.clone(),
            Self::MusicBrainzArtistId => tags.musicbrainz_artist_id.clone(),
            Self::MusicBrainzAlbumArtistId => tags.musicbrainz_album_artist_id.clone(),
            Self::MusicBrainzReleaseGroupId => tags.musicbrainz_release_group_id.clone(),
        }
    }

    const fn key(self) -> ItemKey {
        match self {
            Self::Title => ItemKey::TrackTitle,
            Self::Artist => ItemKey::TrackArtist,
            Self::Album => ItemKey::AlbumTitle,
            Self::AlbumArtist => ItemKey::AlbumArtist,
            Self::TrackNumber => ItemKey::TrackNumber,
            Self::TrackTotal => ItemKey::TrackTotal,
            Self::DiscNumber => ItemKey::DiscNumber,
            Self::DiscTotal => ItemKey::DiscTotal,
            Self::Date => ItemKey::RecordingDate,
            Self::Label => ItemKey::Label,
            Self::CatalogNumber => ItemKey::CatalogNumber,
            Self::Barcode => ItemKey::Barcode,
            Self::Isrc => ItemKey::Isrc,
            Self::MusicBrainzTrackId => ItemKey::MusicBrainzRecordingId,
            Self::MusicBrainzReleaseTrackId => ItemKey::MusicBrainzTrackId,
            Self::MusicBrainzAlbumId => ItemKey::MusicBrainzReleaseId,
            Self::MusicBrainzArtistId => ItemKey::MusicBrainzArtistId,
            Self::MusicBrainzAlbumArtistId => ItemKey::MusicBrainzReleaseArtistId,
            Self::MusicBrainzReleaseGroupId => ItemKey::MusicBrainzReleaseGroupId,
        }
    }
}

impl fmt::Display for TagField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagEdit {
    pub field: TagField,
    pub value: String,
}

#[derive(Clone, Copy, Debug)]
pub struct Writing<'a> {
    pub edits: &'a [TagEdit],
    pub picture: Option<&'a CoverArt>,
}

pub trait TagSink: TagSource {
    fn writes(&self, location: &MediaLocation) -> bool;
    fn write(&self, location: &MediaLocation, writing: Writing<'_>) -> Result<()>;
}

pub struct FileTags {
    sources: Sources,
}

impl FileTags {
    pub const fn over(sources: Sources) -> Self {
        Self { sources }
    }

    fn writable<'a>(&self, location: &'a MediaLocation) -> Result<&'a Path> {
        let path = location.as_path().ok_or_else(|| Error::LocatorNotUsable {
            location: location.clone(),
        })?;
        if !self.writes(location) {
            return Err(Error::Unwritable {
                location: location.clone(),
            });
        }
        Ok(path)
    }
}

impl Default for FileTags {
    fn default() -> Self {
        Self::over(Sources::local())
    }
}

impl TagSource for FileTags {
    fn read(&self, location: &MediaLocation, picturing: Picturing) -> Result<Tagged> {
        let (info, picture) = probe_pictured(&self.sources, location, picturing)?;
        Ok(Tagged {
            tags: info.tags,
            picture,
        })
    }
}

impl TagSink for FileTags {
    fn writes(&self, location: &MediaLocation) -> bool {
        location
            .as_path()
            .and_then(FileType::from_path)
            .is_some_and(read_back_here)
    }

    fn write(&self, location: &MediaLocation, writing: Writing<'_>) -> Result<()> {
        let path = self.writable(location)?;
        let mut tagged = opened(path, location)?;
        if tagged.primary_tag().is_none() {
            let kind = tagged.primary_tag_type();
            tagged.insert_tag(Tag::new(kind));
        }

        let tag = tagged
            .primary_tag_mut()
            .expect("a primary tag this call has just put there");
        for edit in writing.edits {
            tag.insert_text(edit.field.key(), edit.value.clone());
        }
        if let Some(picture) = writing.picture {
            tag.remove_picture_type(PictureType::CoverFront);
            tag.push_picture(front_cover(picture));
        }

        let staged = staged_beside(path);
        let written = fs::copy(path, &staged)
            .map_err(|source| Error::Io {
                location: location.clone(),
                source,
            })
            .and_then(|_| {
                tag.save_to_path(&staged, WriteOptions::default())
                    .map_err(|source| Error::TagsUnwritten {
                        location: location.clone(),
                        source,
                    })
            })
            .and_then(|()| {
                settled_over(&staged, path).map_err(|source| Error::Io {
                    location: location.clone(),
                    source,
                })
            });
        if written.is_err() {
            let _ = fs::remove_file(&staged);
        }
        written
    }
}

static STAGED: AtomicU64 = AtomicU64::new(0);

fn staged_beside(path: &Path) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = path.extension().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(
        ".{stem}.{}-{}.{extension}",
        process::id(),
        STAGED.fetch_add(1, Ordering::Relaxed)
    ))
}

fn settled_over(staged: &Path, path: &Path) -> io::Result<()> {
    File::open(staged)?.sync_all()?;
    fs::rename(staged, path)?;
    if let Some(folder) = path.parent() {
        File::open(folder)?.sync_all()?;
    }
    Ok(())
}

fn front_cover(picture: &CoverArt) -> Picture {
    Picture::unchecked(picture.bytes.clone())
        .pic_type(PictureType::CoverFront)
        .mime_type(MimeType::from_str(picture.format.media_type()))
        .build()
}

const fn read_back_here(kind: FileType) -> bool {
    matches!(
        kind,
        FileType::Aac
            | FileType::Aiff
            | FileType::Flac
            | FileType::Mp4
            | FileType::Mpeg
            | FileType::Opus
            | FileType::Vorbis
            | FileType::Wav
    )
}

fn opened(path: &Path, location: &MediaLocation) -> Result<lofty::file::TaggedFile> {
    let opened = Probe::open(path).map_err(|source| Error::TagsUnread {
        location: location.clone(),
        source,
    })?;
    let probed = opened.guess_file_type().map_err(|source| Error::Io {
        location: location.clone(),
        source,
    })?;

    if !probed.file_type().is_some_and(read_back_here) {
        return Err(Error::Unwritable {
            location: location.clone(),
        });
    }

    probed.read().map_err(|source| Error::TagsUnread {
        location: location.clone(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        io::Cursor,
        path::PathBuf,
        process,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;
    use crate::{ImageFormat, Pictured};

    const RATE: u32 = 44_100;
    const CHANNELS: u16 = 2;
    const BITS: u16 = 16;
    const FRAMES: u32 = 4_410;
    const FLAC_BLOCK: u32 = 192;
    const STREAMINFO: u8 = 0x80;
    const CRC8_POLYNOMIAL: u8 = 0x07;
    const CRC16_POLYNOMIAL: u16 = 0x8005;
    const FLAC_44100: u8 = 0b1001;
    const FLAC_192_SAMPLES: u8 = 0b0001;
    const FLAC_16_BIT: u8 = 0b100;
    const FLAC_CONSTANT_SUBFRAME: u8 = 0x00;

    struct Folder {
        root: PathBuf,
    }

    impl Folder {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = env::temp_dir().join(format!(
                "resonate-writing-{}-{}",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).expect("a writable temporary directory");
            Self { root }
        }

        fn holding(&self, name: &str, bytes: &[u8]) -> MediaLocation {
            let path = self.root.join(name);
            fs::write(&path, bytes).expect("a writable temporary file");
            MediaLocation::local(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn aiff() -> Vec<u8> {
        let mut common = Vec::new();
        common.extend_from_slice(&CHANNELS.to_be_bytes());
        common.extend_from_slice(&FRAMES.to_be_bytes());
        common.extend_from_slice(&BITS.to_be_bytes());
        let leading = RATE.leading_zeros();
        let exponent = u16::try_from(16_383 + 31 - leading).expect("a rate inside the exponent");
        common.extend_from_slice(&exponent.to_be_bytes());
        common.extend_from_slice(&(u64::from(RATE) << (leading + 32)).to_be_bytes());

        let stride = usize::from(CHANNELS) * usize::from(BITS / 8);
        let mut sound = vec![0_u8; 8];
        sound.resize(sound.len() + FRAMES as usize * stride, 0);

        let mut body = b"AIFF".to_vec();
        chunk(&mut body, b"COMM", &common);
        chunk(&mut body, b"SSND", &sound);

        let mut file = b"FORM".to_vec();
        file.extend_from_slice(
            &u32::try_from(body.len())
                .expect("a small file")
                .to_be_bytes(),
        );
        file.extend_from_slice(&body);
        file
    }

    fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
        into.extend_from_slice(id);
        into.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("a small chunk")
                .to_be_bytes(),
        );
        into.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            into.push(0);
        }
    }

    fn flac() -> Vec<u8> {
        let mut info = Vec::new();
        info.extend_from_slice(
            &u16::try_from(FLAC_BLOCK)
                .expect("a small block")
                .to_be_bytes(),
        );
        info.extend_from_slice(
            &u16::try_from(FLAC_BLOCK)
                .expect("a small block")
                .to_be_bytes(),
        );
        info.extend_from_slice(&[0; 6]);
        let packed = (u64::from(RATE) << 44)
            | (u64::from(CHANNELS - 1) << 41)
            | (u64::from(BITS - 1) << 36)
            | u64::from(FLAC_BLOCK);
        info.extend_from_slice(&packed.to_be_bytes());
        info.extend_from_slice(&[0; 16]);

        let mut file = b"fLaC".to_vec();
        file.push(STREAMINFO);
        file.extend_from_slice(
            &u32::try_from(info.len())
                .expect("a small block")
                .to_be_bytes()[1..],
        );
        file.extend_from_slice(&info);
        file.extend_from_slice(&flac_frame());
        file
    }

    fn flac_frame() -> Vec<u8> {
        let channels = u8::try_from(CHANNELS).expect("a small channel count");
        let mut frame = vec![
            0xFF,
            0xF8,
            FLAC_192_SAMPLES << 4 | FLAC_44100,
            (channels - 1) << 4 | FLAC_16_BIT << 1,
            0x00,
        ];
        frame.push(crc8(&frame));
        for _ in 0..channels {
            frame.push(FLAC_CONSTANT_SUBFRAME);
            frame.extend_from_slice(&[0; 2]);
        }
        frame.extend_from_slice(&crc16(&frame).to_be_bytes());
        frame
    }

    fn crc8(bytes: &[u8]) -> u8 {
        bytes.iter().fold(0, |crc, byte| {
            (0..8).fold(crc ^ byte, |crc, _| {
                (crc << 1) ^ (if crc & 0x80 == 0 { 0 } else { CRC8_POLYNOMIAL })
            })
        })
    }

    fn crc16(bytes: &[u8]) -> u16 {
        bytes.iter().fold(0, |crc, byte| {
            (0..8).fold(crc ^ (u16::from(*byte) << 8), |crc, _| {
                (crc << 1)
                    ^ (if crc & 0x8000 == 0 {
                        0
                    } else {
                        CRC16_POLYNOMIAL
                    })
            })
        })
    }

    fn riff_chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
        into.extend_from_slice(id);
        into.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("a small chunk")
                .to_le_bytes(),
        );
        into.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            into.push(0);
        }
    }

    fn wave() -> Vec<u8> {
        wave_with(&[])
    }

    fn wave_with(info: &[(&[u8; 4], &str)]) -> Vec<u8> {
        let mut format = Vec::new();
        format.extend_from_slice(&1_u16.to_le_bytes());
        format.extend_from_slice(&CHANNELS.to_le_bytes());
        format.extend_from_slice(&RATE.to_le_bytes());
        let align = CHANNELS * BITS / 8;
        format.extend_from_slice(&(RATE * u32::from(align)).to_le_bytes());
        format.extend_from_slice(&align.to_le_bytes());
        format.extend_from_slice(&BITS.to_le_bytes());

        let mut body = b"WAVE".to_vec();
        riff_chunk(&mut body, b"fmt ", &format);
        if !info.is_empty() {
            let mut list = b"INFO".to_vec();
            for (id, value) in info {
                let mut terminated = value.as_bytes().to_vec();
                terminated.push(0);
                riff_chunk(&mut list, id, &terminated);
            }
            riff_chunk(&mut body, b"LIST", &list);
        }
        riff_chunk(
            &mut body,
            b"data",
            &vec![0; FRAMES as usize * usize::from(align)],
        );

        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(
            &u32::try_from(body.len())
                .expect("a small file")
                .to_le_bytes(),
        );
        file.extend_from_slice(&body);
        file
    }

    fn edited(field: TagField, value: &str) -> TagEdit {
        TagEdit {
            field,
            value: value.to_owned(),
        }
    }

    fn named_and_identified() -> Vec<TagEdit> {
        vec![
            edited(TagField::Title, "Echoes"),
            edited(TagField::Artist, "Pink Floyd"),
            edited(TagField::Album, "Meddle"),
            edited(TagField::AlbumArtist, "Pink Floyd"),
            edited(TagField::TrackNumber, "6"),
            edited(TagField::TrackTotal, "6"),
            edited(TagField::DiscNumber, "1"),
            edited(TagField::DiscTotal, "2"),
            edited(TagField::Date, "1971-10-30"),
            edited(TagField::Label, "Harvest"),
            edited(TagField::CatalogNumber, "SHVL 795"),
            edited(TagField::Barcode, "5099902893525"),
            edited(TagField::Isrc, "GBAYE7100195"),
            edited(
                TagField::MusicBrainzTrackId,
                "b1a9c0de-1111-4222-8333-444455556666",
            ),
            edited(
                TagField::MusicBrainzReleaseTrackId,
                "3e7f0f5c-4d8a-4a7e-9b2a-0d3f6b6f9c11",
            ),
            edited(
                TagField::MusicBrainzAlbumId,
                "1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f",
            ),
            edited(
                TagField::MusicBrainzArtistId,
                "83d91898-7763-47d7-b03b-b92132375c47",
            ),
            edited(
                TagField::MusicBrainzAlbumArtistId,
                "83d91898-7763-47d7-b03b-b92132375c47",
            ),
            edited(
                TagField::MusicBrainzReleaseGroupId,
                "f5093c06-23e3-404f-aeaa-40f72885ee3a",
            ),
        ]
    }

    fn assert_read_back(tags: &TagSet, edits: &[TagEdit]) {
        assert_eq!(
            edits.len(),
            TagField::ALL.len(),
            "a field was added to the vocabulary without a round trip to hold it"
        );
        for edit in edits {
            assert_eq!(
                edit.field.read(tags).as_deref(),
                Some(edit.value.as_str()),
                "{} did not read back as it was written",
                edit.field
            );
        }
    }

    #[test]
    fn a_write_lands_through_a_staged_copy_and_a_failed_one_leaves_the_file_as_it_was() {
        use std::os::unix::fs::MetadataExt as _;

        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let path = location.as_path().expect("a local file").to_path_buf();
        let before = fs::metadata(&path).expect("a file").ino();
        let tags = FileTags::default();

        tags.write(&location, just(&named_and_identified()))
            .expect("a written FLAC");
        assert_ne!(
            fs::metadata(&path).expect("a file").ino(),
            before,
            "the tags were spliced into the file where it stood"
        );

        let broken = folder.holding("broken.flac", b"fLaC but nothing after it");
        assert!(tags.write(&broken, just(&named_and_identified())).is_err());
        assert_eq!(
            fs::read(broken.as_path().expect("a local file")).expect("the file"),
            b"fLaC but nothing after it"
        );

        let left: Vec<_> = fs::read_dir(&folder.root)
            .expect("the folder")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name())
            .filter(|name| name.to_string_lossy().starts_with('.'))
            .collect();
        assert!(left.is_empty(), "a staged copy was left behind: {left:?}");
    }

    #[test]
    fn every_field_written_into_a_vorbis_comment_reads_back() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let tags = FileTags::default();
        let edits = named_and_identified();

        tags.write(&location, just(&edits)).expect("a written FLAC");
        assert_read_back(
            &tags
                .read(&location, Picturing::Whether)
                .expect("a readable FLAC")
                .tags,
            &edits,
        );
    }

    #[test]
    fn every_field_written_into_an_id3_tag_reads_back() {
        let folder = Folder::new();
        let location = folder.holding("echoes.aiff", &aiff());
        let tags = FileTags::default();
        let edits = named_and_identified();

        tags.write(&location, just(&edits)).expect("a written AIFF");
        assert_read_back(
            &tags
                .read(&location, Picturing::Whether)
                .expect("a readable AIFF")
                .tags,
            &edits,
        );
    }

    #[test]
    fn writing_leaves_the_tags_it_was_not_asked_about_where_they_stood() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let tags = FileTags::default();

        tags.write(&location, just(&named_and_identified()))
            .expect("a written FLAC");
        tags.write(
            &location,
            just(&[edited(TagField::Title, "Echoes (2011 Remaster)")]),
        )
        .expect("a rewritten FLAC");

        let read = tags
            .read(&location, Picturing::Whether)
            .expect("a readable FLAC")
            .tags;
        assert_eq!(read.title.as_deref(), Some("Echoes (2011 Remaster)"));
        assert_eq!(read.album.as_deref(), Some("Meddle"));
        assert_eq!(
            read.musicbrainz_album_id.as_deref(),
            Some("1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f")
        );
    }

    fn painted(shade: u8) -> CoverArt {
        const SIDE: u32 = 8;
        let drawn =
            image::RgbaImage::from_pixel(SIDE, SIDE, image::Rgba([shade, shade, shade, 255]));
        let mut bytes = Vec::new();
        drawn
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("a flat image is written as PNG");

        CoverArt {
            format: ImageFormat::Png,
            bytes,
        }
    }

    fn just(edits: &[TagEdit]) -> Writing<'_> {
        Writing {
            edits,
            picture: None,
        }
    }

    fn only(picture: &CoverArt) -> Writing<'_> {
        Writing {
            edits: &[],
            picture: Some(picture),
        }
    }

    #[test]
    fn a_picture_written_into_a_vorbis_comment_reads_back() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let tags = FileTags::default();
        let cover = painted(200);

        tags.write(&location, only(&cover)).expect("a written FLAC");

        assert_eq!(
            tags.read(&location, Picturing::Copied)
                .expect("a readable FLAC")
                .picture
                .into_copied(),
            Some(cover)
        );
    }

    #[test]
    fn a_picture_written_into_an_id3_tag_reads_back() {
        let folder = Folder::new();
        let location = folder.holding("echoes.aiff", &aiff());
        let tags = FileTags::default();
        let cover = painted(137);

        tags.write(&location, only(&cover)).expect("a written AIFF");

        assert_eq!(
            tags.read(&location, Picturing::Copied)
                .expect("a readable AIFF")
                .picture
                .into_copied(),
            Some(cover)
        );
    }

    #[test]
    fn a_picture_takes_the_place_of_the_front_cover_rather_than_standing_beside_it() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let tags = FileTags::default();
        let first = painted(40);
        let second = painted(210);

        tags.write(&location, only(&first)).expect("a written FLAC");
        tags.write(&location, only(&second))
            .expect("a rewritten FLAC");

        assert_eq!(
            tags.read(&location, Picturing::Copied)
                .expect("a readable FLAC")
                .picture
                .into_copied(),
            Some(second)
        );
    }

    #[test]
    fn a_file_with_no_picture_in_it_answers_with_none() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let tags = FileTags::default();

        tags.write(&location, just(&named_and_identified()))
            .expect("a written FLAC");

        assert_eq!(
            tags.read(&location, Picturing::Copied)
                .expect("a readable FLAC")
                .picture
                .into_copied(),
            None
        );
        assert_eq!(
            tags.read(&location, Picturing::Whether)
                .expect("a readable FLAC")
                .picture,
            Pictured::Bare
        );
    }

    #[test]
    fn one_read_answers_the_fields_and_whether_a_picture_is_there_without_copying_it() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let tags = FileTags::default();
        let picture = painted(0x40);
        let edits = named_and_identified();

        tags.write(
            &location,
            Writing {
                edits: &edits,
                picture: Some(&picture),
            },
        )
        .expect("a written FLAC");

        let weighed = tags
            .read(&location, Picturing::Whether)
            .expect("a readable FLAC");
        assert_read_back(&weighed.tags, &edits);
        assert_eq!(weighed.picture, Pictured::Carried);

        let copied = tags
            .read(&location, Picturing::Copied)
            .expect("a readable FLAC");
        assert_read_back(&copied.tags, &edits);
        assert_eq!(copied.picture, Pictured::Copied(picture));
    }

    #[test]
    fn a_picture_rides_beside_the_fields_in_one_write() {
        let folder = Folder::new();
        let location = folder.holding("echoes.flac", &flac());
        let tags = FileTags::default();
        let edits = named_and_identified();
        let cover = painted(90);

        tags.write(
            &location,
            Writing {
                edits: &edits,
                picture: Some(&cover),
            },
        )
        .expect("a written FLAC");

        assert_read_back(
            &tags
                .read(&location, Picturing::Whether)
                .expect("a readable FLAC")
                .tags,
            &edits,
        );
        assert_eq!(
            tags.read(&location, Picturing::Copied)
                .expect("a readable FLAC")
                .picture
                .into_copied(),
            Some(cover)
        );
    }

    #[test]
    fn every_field_and_the_picture_written_into_a_wave_file_read_back() {
        let folder = Folder::new();
        let location = folder.holding("echoes.wav", &wave());
        let tags = FileTags::default();
        let edits = named_and_identified();
        let cover = painted(77);

        assert!(tags.writes(&location), "a WAV was refused for writing");
        tags.write(
            &location,
            Writing {
                edits: &edits,
                picture: Some(&cover),
            },
        )
        .expect("a written WAV");

        let read = tags
            .read(&location, Picturing::Copied)
            .expect("a readable WAV");
        assert_read_back(&read.tags, &edits);
        assert_eq!(read.picture, Pictured::Copied(cover));
    }

    #[test]
    fn a_title_written_into_a_wave_file_outranks_the_info_list_beside_it() {
        let folder = Folder::new();
        let listed = wave_with(&[(b"INAM", "Untitled"), (b"IART", "Pink Floyd")]);
        let location = folder.holding("echoes.wav", &listed);
        let tags = FileTags::default();

        tags.write(&location, just(&[edited(TagField::Title, "Echoes")]))
            .expect("a written WAV");

        let read = tags
            .read(&location, Picturing::Whether)
            .expect("a readable WAV")
            .tags;
        assert_eq!(read.title.as_deref(), Some("Echoes"));
        assert_eq!(read.artist.as_deref(), Some("Pink Floyd"));
    }

    #[test]
    fn a_format_whose_tags_this_build_would_not_read_back_is_refused() {
        let folder = Folder::new();
        let location = folder.holding("echoes.wv", b"wvpk");
        let tags = FileTags::default();

        assert!(!tags.writes(&location), "a WavPack was offered for writing");
        assert!(
            matches!(
                tags.write(&location, just(&[edited(TagField::Title, "Echoes")])),
                Err(Error::Unwritable { .. })
            ),
            "a WavPack was written rather than refused"
        );
    }

    #[test]
    fn a_location_with_no_path_behind_it_is_refused() {
        let location = MediaLocation::new(
            resonate_core::SourceId::new("elsewhere").expect("a nameable source"),
            "echoes.flac",
        );
        let tags = FileTags::default();

        assert!(!tags.writes(&location));
        assert!(matches!(
            tags.write(&location, just(&[edited(TagField::Title, "Echoes")])),
            Err(Error::LocatorNotUsable { .. })
        ));
    }

    #[test]
    fn a_file_whose_bytes_disagree_with_its_extension_is_refused() {
        let folder = Folder::new();
        let mut wavpack = b"wvpk".to_vec();
        wavpack.resize(64, 0);
        let location = folder.holding("echoes.flac", &wavpack);
        let tags = FileTags::default();

        assert!(
            tags.writes(&location),
            "the extension alone reads as a FLAC"
        );
        assert!(
            matches!(
                tags.write(&location, just(&[edited(TagField::Title, "Echoes")])),
                Err(Error::Unwritable { .. })
            ),
            "a WavPack named .flac was written rather than refused"
        );
    }

    #[test]
    fn a_field_names_itself_once() {
        let mut named: Vec<&str> = TagField::ALL.iter().map(|field| field.as_str()).collect();
        named.sort_unstable();
        let held = named.len();
        named.dedup();
        assert_eq!(named.len(), held, "two fields answer to one name");
    }
}
