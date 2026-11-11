use std::time::Duration;

use resonate_core::{Isrc, Link, Mbid};
use resonate_library::{
    AlbumMatch, ArtistMatch, ArtistPressings, ArtistProfile, ArtistRelease, Barcode, BarcodeMatch,
    ByArtist, Credit, Discography, Genre, GroupAsked, GroupMatch, GroupRelease, Issued, LifeSpan,
    Medium, Recording, RecordingAsked, RecordingMatch, RecordingRelease, Release, ReleaseAsked,
    ReleaseGroup, ReleaseMatch, ReleaseTrack, SongsAsked, Wording,
};

use crate::{Error, Malformed, Result};

pub const MBID_BYTES: usize = 16;
pub const LONGEST_TEXT: usize = 64 * 1024;
const ITEM_AT_LEAST: usize = 1;
const MBID_GROUPS: [usize; 5] = [8, 4, 4, 4, 12];

#[derive(Default)]
pub struct Writer(Vec<u8>);

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn finish(self) -> Vec<u8> {
        self.0
    }

    pub fn byte(&mut self, byte: u8) {
        self.0.push(byte);
    }

    pub fn raw(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    pub fn number(&mut self, mut number: u64) {
        loop {
            let low = (number & 0x7f) as u8;
            number >>= 7;
            if number == 0 {
                self.0.push(low);
                return;
            }
            self.0.push(low | 0x80);
        }
    }

    pub fn signed(&mut self, number: i64) {
        self.number(((number << 1) ^ (number >> 63)) as u64);
    }

    pub fn length(&mut self, length: usize) {
        self.number(length as u64);
    }

    pub fn flag(&mut self, flag: bool) {
        self.byte(u8::from(flag));
    }

    pub fn bytes(&mut self, bytes: &[u8]) {
        self.length(bytes.len());
        self.0.extend_from_slice(bytes);
    }

    pub fn text(&mut self, text: &str) {
        let mut end = text.len().min(LONGEST_TEXT);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.bytes(&text.as_bytes()[..end]);
    }

    pub fn maybe_text(&mut self, text: Option<&str>) {
        match text {
            Some(text) => {
                self.byte(1);
                self.text(text);
            }
            None => self.byte(0),
        }
    }

    pub fn maybe_number(&mut self, number: Option<u64>) {
        match number {
            Some(number) => {
                self.byte(1);
                self.number(number);
            }
            None => self.byte(0),
        }
    }

    pub fn mbid(&mut self, mbid: &Mbid) {
        self.0.extend_from_slice(&mbid_bytes(mbid));
    }

    pub fn maybe_mbid(&mut self, mbid: Option<&Mbid>) {
        match mbid {
            Some(mbid) => {
                self.byte(1);
                self.mbid(mbid);
            }
            None => self.byte(0),
        }
    }

    pub fn length_of(&mut self, length: Option<Duration>) {
        self.maybe_number(
            length.map(|length| u64::try_from(length.as_millis()).unwrap_or(u64::MAX)),
        );
    }

    pub fn texts(&mut self, texts: &[String]) {
        self.length(texts.len());
        for text in texts {
            self.text(text);
        }
    }

    pub fn isrcs(&mut self, isrcs: &[Isrc]) {
        self.length(isrcs.len());
        for isrc in isrcs {
            self.text(isrc.as_str());
        }
    }

    pub fn credit(&mut self, credit: &[Credit]) {
        self.length(credit.len());
        for named in credit {
            self.text(&named.name);
            self.text(&named.joined_by);
            self.maybe_mbid(named.mbid.as_ref());
        }
    }

    pub fn links(&mut self, links: &[Link]) {
        self.length(links.len());
        for link in links {
            self.text(link.relation.spelled());
            self.text(&link.url);
        }
    }

    pub fn wording(&mut self, wording: Wording) {
        self.byte(match wording {
            Wording::Phrase => 0,
            Wording::Words => 1,
        });
    }

    pub fn release(&mut self, release: &Release) {
        self.mbid(&release.id);
        self.maybe_mbid(release.group.as_ref());
        self.text(&release.title);
        self.credit(&release.credit);
        self.maybe_text(release.date.as_deref());
        self.maybe_text(release.country.as_deref());
        self.maybe_text(release.label.as_deref());
        self.maybe_text(release.catalog_number.as_deref());
        self.maybe_text(release.barcode.as_deref());
        self.maybe_text(release.kind.as_deref());
        self.maybe_text(release.disambiguation.as_deref());
        self.flag(release.has_front_cover);
        self.links(&release.links);
        self.length(release.media.len());
        for medium in &release.media {
            self.medium(medium);
        }
    }

    fn medium(&mut self, medium: &Medium) {
        self.number(u64::from(medium.position));
        self.maybe_text(medium.format.as_deref());
        self.maybe_text(medium.title.as_deref());
        self.length(medium.tracks.len());
        for track in &medium.tracks {
            self.number(u64::from(track.position));
            self.text(&track.number);
            self.text(&track.title);
            self.maybe_text(track.artist.as_deref());
            self.maybe_mbid(track.recording.as_ref());
            self.maybe_mbid(track.track.as_ref());
            self.length_of(track.length);
            self.texts(&track.isrcs);
            self.links(&track.links);
        }
    }

    pub fn releases(&mut self, releases: &[Release]) {
        self.length(releases.len());
        for release in releases {
            self.release(release);
        }
    }

    pub fn recording(&mut self, recording: &Recording) {
        self.mbid(&recording.id);
        self.text(&recording.title);
        self.credit(&recording.credit);
        self.length_of(recording.length);
        self.isrcs(&recording.isrcs);
        self.recording_releases(&recording.releases);
    }

    fn recording_releases(&mut self, releases: &[RecordingRelease]) {
        self.length(releases.len());
        for release in releases {
            self.mbid(&release.id);
            self.text(&release.title);
            self.maybe_text(release.date.as_deref());
            self.maybe_number(release.disc.map(u64::from));
            self.maybe_number(release.position.map(u64::from));
            self.maybe_text(release.issued.kind.as_deref());
            self.texts(&release.issued.secondary);
            self.maybe_text(release.issued.status.as_deref());
        }
    }

    pub fn recordings(&mut self, recordings: &[Recording]) {
        self.length(recordings.len());
        for recording in recordings {
            self.recording(recording);
        }
    }

    pub fn recording_matches(&mut self, matches: &[RecordingMatch]) {
        self.length(matches.len());
        for found in matches {
            self.mbid(&found.recording);
            self.byte(found.score);
            self.text(&found.title);
            self.credit(&found.credit);
            self.length_of(found.length);
            self.isrcs(&found.isrcs);
            self.recording_releases(&found.releases);
        }
    }

    pub fn release_matches(&mut self, matches: &[ReleaseMatch]) {
        self.length(matches.len());
        for found in matches {
            self.mbid(&found.release);
            self.maybe_mbid(found.group.as_ref());
            self.byte(found.score);
            self.text(&found.title);
            self.credit(&found.credit);
            self.maybe_number(found.track_count.map(u64::from));
            self.maybe_text(found.date.as_deref());
        }
    }

    pub fn barcode_matches(&mut self, matches: &[BarcodeMatch]) {
        self.length(matches.len());
        for found in matches {
            self.mbid(&found.release);
            self.maybe_mbid(found.group.as_ref());
            self.maybe_text(found.barcode.as_deref());
            self.text(&found.title);
            self.credit(&found.credit);
        }
    }

    pub fn group(&mut self, group: &ReleaseGroup) {
        self.mbid(&group.id);
        self.text(&group.title);
        self.credit(&group.credit);
        self.maybe_text(group.kind.as_deref());
        self.maybe_text(group.first_released.as_deref());
        self.maybe_text(group.disambiguation.as_deref());
        self.links(&group.links);
        self.length(group.releases.len());
        for release in &group.releases {
            self.mbid(&release.id);
            self.text(&release.title);
            self.maybe_text(release.date.as_deref());
            self.maybe_text(release.country.as_deref());
            self.maybe_number(release.track_count.map(u64::from));
            self.length(release.discs.len());
            for disc in &release.discs {
                self.number(u64::from(*disc));
            }
        }
    }

    pub fn group_matches(&mut self, matches: &[GroupMatch]) {
        self.length(matches.len());
        for found in matches {
            self.mbid(&found.group);
            self.byte(found.score);
            self.text(&found.title);
            self.credit(&found.credit);
        }
    }

    pub fn album_matches(&mut self, matches: &[AlbumMatch]) {
        self.length(matches.len());
        for found in matches {
            self.mbid(&found.group);
            self.byte(found.score);
            self.text(&found.title);
            self.credit(&found.credit);
            self.maybe_text(found.kind.as_deref());
            self.texts(&found.secondary);
            self.maybe_text(found.first_released.as_deref());
        }
    }

    pub fn artist(&mut self, artist: &ArtistProfile) {
        self.mbid(&artist.mbid);
        self.text(&artist.name);
        self.maybe_text(artist.sort_name.as_deref());
        self.maybe_text(artist.kind.as_deref());
        self.maybe_text(artist.gender.as_deref());
        self.maybe_text(artist.country.as_deref());
        self.maybe_text(artist.area.as_deref());
        self.maybe_text(artist.began_in.as_deref());
        self.maybe_text(artist.span.begin.as_deref());
        self.maybe_text(artist.span.end.as_deref());
        self.flag(artist.span.ended);
        self.maybe_text(artist.disambiguation.as_deref());
        self.texts(&artist.aliases);
        self.length(artist.genres.len());
        for genre in &artist.genres {
            self.text(&genre.name);
            self.number(u64::from(genre.weight));
        }
        self.links(&artist.links);
    }

    pub fn artist_matches(&mut self, matches: &[ArtistMatch]) {
        self.length(matches.len());
        for found in matches {
            self.mbid(&found.mbid);
            self.text(&found.name);
            self.byte(found.score);
            self.maybe_text(found.kind.as_deref());
            self.maybe_text(found.disambiguation.as_deref());
            self.texts(&found.aliases);
        }
    }

    pub fn artist_releases(&mut self, releases: &[ArtistRelease]) {
        self.length(releases.len());
        for release in releases {
            self.mbid(&release.mbid);
            self.text(&release.title);
            self.maybe_text(release.kind.as_deref());
            self.texts(&release.secondary);
            self.maybe_text(release.first_released.as_deref());
        }
    }

    pub fn discography(&mut self, discography: &Discography) {
        self.artist_releases(&discography.releases);
        self.number(u64::from(discography.unread));
        self.number(u64::from(discography.read_to));
    }

    pub fn pressings(&mut self, pressings: &ArtistPressings) {
        self.releases(&pressings.pressings);
        self.number(u64::from(pressings.credited));
        self.number(u64::from(pressings.read_to));
    }

    pub fn release_asked(&mut self, asked: &ReleaseAsked) {
        self.text(&asked.title);
        self.maybe_text(asked.artist.as_deref());
        self.maybe_mbid(asked.artist_mbid.as_ref());
        self.maybe_text(asked.barcode.as_deref());
        self.maybe_text(asked.catalog_number.as_deref());
        self.wording(asked.wording);
    }

    pub fn recording_asked(&mut self, asked: &RecordingAsked) {
        self.text(&asked.title);
        self.maybe_text(asked.artist.as_deref());
        self.maybe_mbid(asked.artist_mbid.as_ref());
        self.maybe_text(asked.release.as_deref());
        self.length_of(asked.length);
        self.wording(asked.wording);
    }

    pub fn group_asked(&mut self, asked: &GroupAsked) {
        self.text(&asked.title);
        self.maybe_text(asked.artist.as_deref());
        self.maybe_mbid(asked.artist_mbid.as_ref());
        match asked.year {
            Some(year) => {
                self.byte(1);
                self.signed(i64::from(year));
            }
            None => self.byte(0),
        }
        self.wording(asked.wording);
    }

    pub fn songs_asked(&mut self, asked: &SongsAsked) {
        self.text(&asked.words);
        self.text(&asked.typed);
        match &asked.by {
            Some(by) => {
                self.byte(1);
                self.text(&by.title);
                self.text(&by.artist);
            }
            None => self.byte(0),
        }
    }

    pub fn barcode(&mut self, barcode: &Barcode) {
        self.text(barcode.as_str());
    }
}

pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

fn malformed(kind: Malformed) -> Error {
    Error::Malformed(kind)
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub fn done(&self) -> Result<()> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(malformed(Malformed::Trailing))
        }
    }

    pub fn is_done(&self) -> bool {
        self.at == self.bytes.len()
    }

    fn left(&self) -> usize {
        self.bytes.len() - self.at
    }

    pub fn byte(&mut self) -> Result<u8> {
        let byte = *self.bytes.get(self.at).ok_or(malformed(Malformed::Short))?;
        self.at += 1;
        Ok(byte)
    }

    pub fn number(&mut self) -> Result<u64> {
        let mut number = 0_u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            let low = u64::from(byte & 0x7f);
            if shift == 63 && low > 1 {
                return Err(malformed(Malformed::Number));
            }
            number |= low << shift;
            if byte & 0x80 == 0 {
                return Ok(number);
            }
        }
        Err(malformed(Malformed::Number))
    }

    pub fn signed(&mut self) -> Result<i64> {
        let zigzag = self.number()?;
        Ok(((zigzag >> 1) as i64) ^ -((zigzag & 1) as i64))
    }

    pub fn small(&mut self) -> Result<u32> {
        u32::try_from(self.number()?).map_err(|_| malformed(Malformed::Number))
    }

    pub fn flag(&mut self) -> Result<bool> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            tag => Err(malformed(Malformed::Tag { tag })),
        }
    }

    pub fn count_of(&mut self, at_least: usize) -> Result<usize> {
        let count = usize::try_from(self.number()?).map_err(|_| malformed(Malformed::Number))?;
        if count.saturating_mul(at_least) > self.left() {
            return Err(malformed(Malformed::Overlong { length: count }));
        }
        Ok(count)
    }

    pub fn bytes(&mut self, at_most: usize) -> Result<&'a [u8]> {
        let length = self.count_of(1)?;
        if length > at_most {
            return Err(malformed(Malformed::Overlong { length }));
        }
        let bytes = &self.bytes[self.at..self.at + length];
        self.at += length;
        Ok(bytes)
    }

    pub fn text(&mut self) -> Result<String> {
        let bytes = self.bytes(LONGEST_TEXT)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| malformed(Malformed::Text))
    }

    pub fn maybe<T>(&mut self, read: impl FnOnce(&mut Self) -> Result<T>) -> Result<Option<T>> {
        if self.flag()? {
            read(self).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn maybe_text(&mut self) -> Result<Option<String>> {
        self.maybe(Self::text)
    }

    pub fn maybe_small(&mut self) -> Result<Option<u32>> {
        self.maybe(Self::small)
    }

    pub fn mbid(&mut self) -> Result<Mbid> {
        if self.left() < MBID_BYTES {
            return Err(malformed(Malformed::Short));
        }
        let mut raw = [0_u8; MBID_BYTES];
        raw.copy_from_slice(&self.bytes[self.at..self.at + MBID_BYTES]);
        self.at += MBID_BYTES;
        Ok(mbid_of(raw))
    }

    pub fn maybe_mbid(&mut self) -> Result<Option<Mbid>> {
        self.maybe(Self::mbid)
    }

    pub fn length_of(&mut self) -> Result<Option<Duration>> {
        Ok(self.maybe(Self::number)?.map(Duration::from_millis))
    }

    fn list<T>(&mut self, mut read: impl FnMut(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        let count = self.count_of(ITEM_AT_LEAST)?;
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(read(self)?);
        }
        Ok(items)
    }

    pub fn texts(&mut self) -> Result<Vec<String>> {
        self.list(Self::text)
    }

    pub fn isrcs(&mut self) -> Result<Vec<Isrc>> {
        self.list(|reader| Isrc::new(&reader.text()?).map_err(|_| malformed(Malformed::Isrc)))
    }

    pub fn credit(&mut self) -> Result<Vec<Credit>> {
        self.list(|reader| {
            Ok(Credit {
                name: reader.text()?,
                joined_by: reader.text()?,
                mbid: reader.maybe_mbid()?,
            })
        })
    }

    pub fn links(&mut self) -> Result<Vec<Link>> {
        self.list(|reader| {
            let relation = reader.text()?;
            Ok(Link::new(&relation, reader.text()?))
        })
    }

    pub fn wording(&mut self) -> Result<Wording> {
        match self.byte()? {
            0 => Ok(Wording::Phrase),
            1 => Ok(Wording::Words),
            tag => Err(malformed(Malformed::Tag { tag })),
        }
    }

    pub fn release(&mut self) -> Result<Release> {
        Ok(Release {
            id: self.mbid()?,
            group: self.maybe_mbid()?,
            title: self.text()?,
            credit: self.credit()?,
            date: self.maybe_text()?,
            country: self.maybe_text()?,
            label: self.maybe_text()?,
            catalog_number: self.maybe_text()?,
            barcode: self.maybe_text()?,
            kind: self.maybe_text()?,
            disambiguation: self.maybe_text()?,
            has_front_cover: self.flag()?,
            links: self.links()?,
            media: self.list(Self::medium)?,
        })
    }

    fn medium(&mut self) -> Result<Medium> {
        Ok(Medium {
            position: self.small()?,
            format: self.maybe_text()?,
            title: self.maybe_text()?,
            tracks: self.list(|reader| {
                Ok(ReleaseTrack {
                    position: reader.small()?,
                    number: reader.text()?,
                    title: reader.text()?,
                    artist: reader.maybe_text()?,
                    recording: reader.maybe_mbid()?,
                    track: reader.maybe_mbid()?,
                    length: reader.length_of()?,
                    isrcs: reader.texts()?,
                    links: reader.links()?,
                })
            })?,
        })
    }

    pub fn releases(&mut self) -> Result<Vec<Release>> {
        self.list(Self::release)
    }

    pub fn recording(&mut self) -> Result<Recording> {
        Ok(Recording {
            id: self.mbid()?,
            title: self.text()?,
            credit: self.credit()?,
            length: self.length_of()?,
            isrcs: self.isrcs()?,
            releases: self.recording_releases()?,
        })
    }

    fn recording_releases(&mut self) -> Result<Vec<RecordingRelease>> {
        self.list(|reader| {
            Ok(RecordingRelease {
                id: reader.mbid()?,
                title: reader.text()?,
                date: reader.maybe_text()?,
                disc: reader.maybe_small()?,
                position: reader.maybe_small()?,
                issued: Issued {
                    kind: reader.maybe_text()?,
                    secondary: reader.texts()?,
                    status: reader.maybe_text()?,
                },
            })
        })
    }

    pub fn recordings(&mut self) -> Result<Vec<Recording>> {
        self.list(Self::recording)
    }

    pub fn recording_matches(&mut self) -> Result<Vec<RecordingMatch>> {
        self.list(|reader| {
            Ok(RecordingMatch {
                recording: reader.mbid()?,
                score: reader.byte()?,
                title: reader.text()?,
                credit: reader.credit()?,
                length: reader.length_of()?,
                isrcs: reader.isrcs()?,
                releases: reader.recording_releases()?,
            })
        })
    }

    pub fn release_matches(&mut self) -> Result<Vec<ReleaseMatch>> {
        self.list(|reader| {
            Ok(ReleaseMatch {
                release: reader.mbid()?,
                group: reader.maybe_mbid()?,
                score: reader.byte()?,
                title: reader.text()?,
                credit: reader.credit()?,
                track_count: reader.maybe_small()?,
                date: reader.maybe_text()?,
            })
        })
    }

    pub fn barcode_matches(&mut self) -> Result<Vec<BarcodeMatch>> {
        self.list(|reader| {
            Ok(BarcodeMatch {
                release: reader.mbid()?,
                group: reader.maybe_mbid()?,
                barcode: reader.maybe_text()?,
                title: reader.text()?,
                credit: reader.credit()?,
            })
        })
    }

    pub fn group(&mut self) -> Result<ReleaseGroup> {
        Ok(ReleaseGroup {
            id: self.mbid()?,
            title: self.text()?,
            credit: self.credit()?,
            kind: self.maybe_text()?,
            first_released: self.maybe_text()?,
            disambiguation: self.maybe_text()?,
            links: self.links()?,
            releases: self.list(|reader| {
                Ok(GroupRelease {
                    id: reader.mbid()?,
                    title: reader.text()?,
                    date: reader.maybe_text()?,
                    country: reader.maybe_text()?,
                    track_count: reader.maybe_small()?,
                    discs: reader.list(Self::small)?,
                })
            })?,
        })
    }

    pub fn group_matches(&mut self) -> Result<Vec<GroupMatch>> {
        self.list(|reader| {
            Ok(GroupMatch {
                group: reader.mbid()?,
                score: reader.byte()?,
                title: reader.text()?,
                credit: reader.credit()?,
            })
        })
    }

    pub fn album_matches(&mut self) -> Result<Vec<AlbumMatch>> {
        self.list(|reader| {
            Ok(AlbumMatch {
                group: reader.mbid()?,
                score: reader.byte()?,
                title: reader.text()?,
                credit: reader.credit()?,
                kind: reader.maybe_text()?,
                secondary: reader.texts()?,
                first_released: reader.maybe_text()?,
            })
        })
    }

    pub fn artist(&mut self) -> Result<ArtistProfile> {
        Ok(ArtistProfile {
            mbid: self.mbid()?,
            name: self.text()?,
            sort_name: self.maybe_text()?,
            kind: self.maybe_text()?,
            gender: self.maybe_text()?,
            country: self.maybe_text()?,
            area: self.maybe_text()?,
            began_in: self.maybe_text()?,
            span: LifeSpan {
                begin: self.maybe_text()?,
                end: self.maybe_text()?,
                ended: self.flag()?,
            },
            disambiguation: self.maybe_text()?,
            aliases: self.texts()?,
            genres: self.list(|reader| {
                Ok(Genre {
                    name: reader.text()?,
                    weight: reader.small()?,
                })
            })?,
            links: self.links()?,
        })
    }

    pub fn artist_matches(&mut self) -> Result<Vec<ArtistMatch>> {
        self.list(|reader| {
            Ok(ArtistMatch {
                mbid: reader.mbid()?,
                name: reader.text()?,
                score: reader.byte()?,
                kind: reader.maybe_text()?,
                disambiguation: reader.maybe_text()?,
                aliases: reader.texts()?,
            })
        })
    }

    pub fn artist_releases(&mut self) -> Result<Vec<ArtistRelease>> {
        self.list(|reader| {
            Ok(ArtistRelease {
                mbid: reader.mbid()?,
                title: reader.text()?,
                kind: reader.maybe_text()?,
                secondary: reader.texts()?,
                first_released: reader.maybe_text()?,
            })
        })
    }

    pub fn discography(&mut self) -> Result<Discography> {
        Ok(Discography {
            releases: self.artist_releases()?,
            unread: self.small()?,
            read_to: self.small()?,
        })
    }

    pub fn pressings(&mut self) -> Result<ArtistPressings> {
        Ok(ArtistPressings {
            pressings: self.releases()?,
            credited: self.small()?,
            read_to: self.small()?,
        })
    }

    pub fn release_asked(&mut self) -> Result<ReleaseAsked> {
        Ok(ReleaseAsked {
            title: self.text()?,
            artist: self.maybe_text()?,
            artist_mbid: self.maybe_mbid()?,
            barcode: self.maybe_text()?,
            catalog_number: self.maybe_text()?,
            wording: self.wording()?,
        })
    }

    pub fn recording_asked(&mut self) -> Result<RecordingAsked> {
        Ok(RecordingAsked {
            title: self.text()?,
            artist: self.maybe_text()?,
            artist_mbid: self.maybe_mbid()?,
            release: self.maybe_text()?,
            length: self.length_of()?,
            wording: self.wording()?,
        })
    }

    pub fn group_asked(&mut self) -> Result<GroupAsked> {
        Ok(GroupAsked {
            title: self.text()?,
            artist: self.maybe_text()?,
            artist_mbid: self.maybe_mbid()?,
            year: self
                .maybe(Self::signed)?
                .map(|year| i32::try_from(year).map_err(|_| malformed(Malformed::Number)))
                .transpose()?,
            wording: self.wording()?,
        })
    }

    pub fn songs_asked(&mut self) -> Result<SongsAsked> {
        Ok(SongsAsked {
            words: self.text()?,
            typed: self.text()?,
            by: self.maybe(|reader| {
                Ok(ByArtist {
                    title: reader.text()?,
                    artist: reader.text()?,
                })
            })?,
        })
    }

    pub fn barcode(&mut self) -> Result<Barcode> {
        Barcode::new(&self.text()?).ok_or(malformed(Malformed::Barcode))
    }
}

pub fn mbid_bytes(mbid: &Mbid) -> [u8; MBID_BYTES] {
    let mut raw = [0_u8; MBID_BYTES];
    let digits = mbid.as_str().bytes().filter(u8::is_ascii_hexdigit);
    for (at, digit) in digits.enumerate().take(MBID_BYTES * 2) {
        let nibble = (digit as char).to_digit(16).unwrap_or(0) as u8;
        raw[at / 2] |= if at % 2 == 0 { nibble << 4 } else { nibble };
    }
    raw
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

pub fn mbid_of(raw: [u8; MBID_BYTES]) -> Mbid {
    let mut dashed = String::with_capacity(36);
    let mut digits = raw.iter().flat_map(|byte| {
        [
            HEX_DIGITS[usize::from(byte >> 4)],
            HEX_DIGITS[usize::from(byte & 0x0f)],
        ]
    });
    for (group, width) in MBID_GROUPS.iter().enumerate() {
        if group > 0 {
            dashed.push('-');
        }
        dashed.extend(digits.by_ref().take(*width).map(char::from));
    }
    Mbid::new(&dashed).unwrap_or_else(|_| unreachable!("thirty-two hex digits dashed as an mbid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOME_MBID: &str = "a74b1b7f-71a5-4011-9441-d0b5e4122711";

    fn mbid() -> Mbid {
        Mbid::new(SOME_MBID).expect("an mbid")
    }

    fn written(with: impl FnOnce(&mut Writer)) -> Vec<u8> {
        let mut writer = Writer::new();
        with(&mut writer);
        writer.finish()
    }

    #[test]
    fn numbers_read_back_as_written_at_every_width() {
        for number in [
            0,
            1,
            127,
            128,
            16_383,
            16_384,
            u64::from(u32::MAX),
            u64::MAX,
        ] {
            let bytes = written(|writer| writer.number(number));

            let mut reader = Reader::new(&bytes);

            assert_eq!(reader.number().expect("a number"), number);
            reader.done().expect("nothing left");
        }
    }

    #[test]
    fn signed_numbers_read_back_as_written_either_side_of_zero() {
        for number in [0, 1, -1, 63, -64, i64::from(i32::MIN), i64::MAX, i64::MIN] {
            let bytes = written(|writer| writer.signed(number));

            assert_eq!(Reader::new(&bytes).signed().expect("a number"), number);
        }
    }

    #[test]
    fn a_number_wider_than_sixty_four_bits_is_malformed() {
        let eleven_bytes = [0xff_u8; 11];
        let tenth_byte_too_big = [0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x02];

        assert!(matches!(
            Reader::new(&eleven_bytes).number(),
            Err(Error::Malformed(Malformed::Number))
        ));
        assert!(matches!(
            Reader::new(&tenth_byte_too_big).number(),
            Err(Error::Malformed(Malformed::Number))
        ));
    }

    #[test]
    fn a_number_cut_off_mid_way_is_short() {
        assert!(matches!(
            Reader::new(&[0x80]).number(),
            Err(Error::Malformed(Malformed::Short))
        ));
    }

    #[test]
    fn text_with_multibyte_letters_and_an_empty_text_read_back() {
        let bytes = written(|writer| {
            writer.text("Caf\u{e9} \u{1F3B5}");
            writer.text("");
            writer.maybe_text(None);
            writer.maybe_text(Some("x"));
        });

        let mut reader = Reader::new(&bytes);

        assert_eq!(reader.text().expect("text"), "Caf\u{e9} \u{1F3B5}");
        assert_eq!(reader.text().expect("text"), "");
        assert_eq!(reader.maybe_text().expect("nothing"), None);
        assert_eq!(reader.maybe_text().expect("text").as_deref(), Some("x"));
        reader.done().expect("nothing left");
    }

    #[test]
    fn a_text_longer_than_the_most_is_cut_on_a_letter_and_still_reads_back() {
        let long = "\u{e9}".repeat(LONGEST_TEXT);
        let bytes = written(|writer| writer.text(&long));

        let read = Reader::new(&bytes).text().expect("a cut text");

        assert!(read.len() <= LONGEST_TEXT);
        assert!(long.starts_with(&read));
    }

    #[test]
    fn text_that_is_not_utf8_a_length_past_the_input_and_a_length_past_the_most_are_malformed() {
        let not_text = [1, 0xff];
        let past_the_input = [5, b'a'];
        let past_the_most = written(|writer| writer.number(LONGEST_TEXT as u64 + 1));

        assert!(matches!(
            Reader::new(&not_text).text(),
            Err(Error::Malformed(Malformed::Text))
        ));
        assert!(matches!(
            Reader::new(&past_the_input).text(),
            Err(Error::Malformed(Malformed::Overlong { length: 5 }))
        ));
        assert!(matches!(
            Reader::new(&past_the_most).text(),
            Err(Error::Malformed(Malformed::Overlong { .. }))
        ));
    }

    #[test]
    fn a_flag_that_is_neither_zero_nor_one_is_a_bad_tag() {
        assert!(matches!(
            Reader::new(&[2]).flag(),
            Err(Error::Malformed(Malformed::Tag { tag: 2 }))
        ));
        assert!(matches!(
            Reader::new(&[7]).maybe_text(),
            Err(Error::Malformed(Malformed::Tag { tag: 7 }))
        ));
    }

    #[test]
    fn a_count_larger_than_the_bytes_left_is_refused_before_anything_is_allocated() {
        let bytes = written(|writer| writer.number(u64::MAX / 2));

        assert!(matches!(
            Reader::new(&bytes).texts(),
            Err(Error::Malformed(Malformed::Overlong { .. }))
        ));
    }

    #[test]
    fn an_mbid_reads_back_and_one_cut_short_is_short() {
        let bytes = written(|writer| writer.mbid(&mbid()));

        assert_eq!(bytes.len(), MBID_BYTES);
        assert_eq!(Reader::new(&bytes).mbid().expect("an mbid"), mbid());
        assert!(matches!(
            Reader::new(&bytes[..MBID_BYTES - 1]).mbid(),
            Err(Error::Malformed(Malformed::Short))
        ));
    }

    #[test]
    fn a_credit_with_and_without_an_mbid_reads_back() {
        let credit = vec![
            Credit {
                name: "Pink Floyd".to_owned(),
                joined_by: " & ".to_owned(),
                mbid: Some(mbid()),
            },
            Credit {
                name: "Syd".to_owned(),
                joined_by: String::new(),
                mbid: None,
            },
        ];
        let bytes = written(|writer| writer.credit(&credit));

        let mut reader = Reader::new(&bytes);

        assert_eq!(reader.credit().expect("a credit"), credit);
        reader.done().expect("nothing left");
    }

    #[test]
    fn bytes_left_after_a_value_are_trailing() {
        let mut bytes = written(|writer| writer.text("a"));
        bytes.push(0);

        let mut reader = Reader::new(&bytes);
        reader.text().expect("text");

        assert!(!reader.is_done());
        assert!(matches!(
            reader.done(),
            Err(Error::Malformed(Malformed::Trailing))
        ));
    }

    #[test]
    fn every_prefix_of_a_value_is_refused_and_never_panics() {
        let credit = vec![Credit {
            name: "Pink Floyd".to_owned(),
            joined_by: " & ".to_owned(),
            mbid: Some(mbid()),
        }];
        let bytes = written(|writer| writer.credit(&credit));

        for cut in 0..bytes.len() {
            assert!(Reader::new(&bytes[..cut]).credit().is_err(), "{cut}");
        }
    }
}
