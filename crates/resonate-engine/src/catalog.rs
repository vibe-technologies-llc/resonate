use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use ahash::AHashMap;
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded, select};
use parking_lot::{Condvar, Mutex};
use resonate_codec::{
    CoverArt, MediaInfo, Pictured, Picturing, Sources, probe_cover_art, probe_pictured, probe_span,
};
use resonate_core::{FrameSpan, MediaLocation};

const ROWS_HELD: usize = 4_096;

const ART_BYTES_HELD: usize = 32 * 1024 * 1024;

const ROWS_ASKED: usize = 256;

const PICTURES_ASKED: usize = 32;

#[derive(Default)]
enum Look<T> {
    #[default]
    Unasked,
    Pending,
    Found(Arc<T>),
    Nothing,
}

impl<T> Clone for Look<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Unasked => Self::Unasked,
            Self::Pending => Self::Pending,
            Self::Found(found) => Self::Found(Arc::clone(found)),
            Self::Nothing => Self::Nothing,
        }
    }
}

impl<T> Look<T> {
    const fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }
}

impl Look<CoverArt> {
    fn bytes(&self) -> usize {
        match self {
            Self::Found(art) => art.bytes.len(),
            Self::Unasked | Self::Pending | Self::Nothing => 0,
        }
    }

    const fn is_a_picture(&self) -> bool {
        matches!(self, Self::Found(_))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Row {
    location: MediaLocation,
    span: Option<FrameSpan>,
}

impl Row {
    fn new(location: &MediaLocation, span: Option<FrameSpan>) -> Self {
        Self {
            location: location.clone(),
            span,
        }
    }
}

enum Wanted {
    Tags(Row),
    Art(MediaLocation),
}

enum Sent {
    Waiting,
    Backlogged,
    NoReader,
}

enum Claim<T> {
    Answered(Option<Arc<T>>),
    Ours,
}

#[derive(Clone, Debug)]
pub enum TagsRead {
    NotYet,
    Answered(Arc<MediaInfo>),
    Nothing,
}

#[derive(Clone, Debug)]
pub enum ArtRead {
    NotYet,
    Answered(Arc<CoverArt>),
    Nothing,
}

struct Asking {
    tags: Sender<Row>,
    art: Sender<MediaLocation>,
}

impl Asking {
    fn send(&self, wanted: Wanted) -> Sent {
        match wanted {
            Wanted::Tags(row) => sent(self.tags.try_send(row)),
            Wanted::Art(location) => sent(self.art.try_send(location)),
        }
    }
}

fn sent<T>(asked: Result<(), TrySendError<T>>) -> Sent {
    match asked {
        Ok(()) => Sent::Waiting,
        Err(TrySendError::Full(_)) => Sent::Backlogged,
        Err(TrySendError::Disconnected(_)) => Sent::NoReader,
    }
}

struct Asked {
    tags: Receiver<Row>,
    art: Receiver<MediaLocation>,
}

impl Asked {
    fn next(&self) -> Option<Wanted> {
        if let Ok(row) = self.tags.try_recv() {
            return Some(Wanted::Tags(row));
        }

        select! {
            recv(self.tags) -> row => row.ok().map(Wanted::Tags),
            recv(self.art) -> location => location.ok().map(Wanted::Art),
        }
    }
}

struct Cut {
    span: FrameSpan,
    tags: Look<MediaInfo>,
}

#[derive(Default)]
struct Entry {
    tags: Look<MediaInfo>,
    cuts: Vec<Cut>,
    art: Look<CoverArt>,
    used: u64,
}

impl Entry {
    fn tags(&self, span: Option<FrameSpan>) -> Look<MediaInfo> {
        let Some(span) = span else {
            return self.tags.clone();
        };
        self.cuts
            .iter()
            .find(|cut| cut.span == span)
            .map_or_else(Look::default, |cut| cut.tags.clone())
    }

    fn keep(&mut self, span: Option<FrameSpan>, look: Look<MediaInfo>) -> usize {
        let Some(span) = span else {
            self.tags = look;
            return 0;
        };
        match self.cuts.iter_mut().find(|cut| cut.span == span) {
            Some(cut) => {
                cut.tags = look;
                0
            }
            None => {
                self.cuts.push(Cut { span, tags: look });
                1
            }
        }
    }

    fn is_pending(&self) -> bool {
        self.tags.is_pending()
            || self.art.is_pending()
            || self.cuts.iter().any(|cut| cut.tags.is_pending())
    }

    fn rows(&self) -> usize {
        1 + self.cuts.len()
    }
}

#[derive(Default)]
struct Held {
    entries: AHashMap<Arc<MediaLocation>, Entry>,
    order: BTreeMap<u64, Arc<MediaLocation>>,
    pictures: BTreeMap<u64, Arc<MediaLocation>>,
    pictured: usize,
    rows: usize,
    clock: u64,
}

impl Held {
    fn tags(&mut self, row: &Row) -> Look<MediaInfo> {
        self.looked_at(&row.location)
            .map_or_else(Look::default, |entry| entry.tags(row.span))
    }

    fn art(&mut self, location: &MediaLocation) -> Look<CoverArt> {
        self.looked_at(location)
            .map_or_else(Look::default, |entry| entry.art.clone())
    }

    fn claim_tags(&mut self, row: &Row) -> Claim<MediaInfo> {
        match self.tags(row) {
            Look::Found(info) => Claim::Answered(Some(info)),
            Look::Pending | Look::Nothing => Claim::Answered(None),
            Look::Unasked => {
                self.keep_tags(row, Look::Pending);
                Claim::Ours
            }
        }
    }

    fn claim_art(&mut self, location: &MediaLocation) -> Claim<CoverArt> {
        match self.art(location) {
            Look::Found(art) => Claim::Answered(Some(art)),
            Look::Pending | Look::Nothing => Claim::Answered(None),
            Look::Unasked => {
                self.keep_art(location, Look::Pending);
                Claim::Ours
            }
        }
    }

    fn keep_tags(&mut self, row: &Row, look: Look<MediaInfo>) {
        self.make_room_for(&row.location);
        let added = self
            .entries
            .get_mut(&row.location)
            .map_or(0, |entry| entry.keep(row.span, look));
        self.rows += added;
    }

    fn keep_art(&mut self, location: &MediaLocation, look: Look<CoverArt>) {
        self.make_room_for(location);
        let gained = look.bytes();
        let draws_a_picture = look.is_a_picture();

        let Some((named, entry)) = self.entries.get_key_value(location) else {
            return;
        };
        let named = Arc::clone(named);
        let at = entry.used;
        let freed = entry.art.bytes();

        if let Some(entry) = self.entries.get_mut(location) {
            entry.art = look;
        }
        if draws_a_picture {
            self.pictures.insert(at, named);
        } else {
            self.pictures.remove(&at);
        }

        self.pictured = self.pictured - freed + gained;
        self.trim_pictures();
    }

    fn looked_at(&mut self, location: &MediaLocation) -> Option<&mut Entry> {
        if !self.made_current(location) {
            return None;
        }
        self.entries.get_mut(location)
    }

    fn made_current(&mut self, location: &MediaLocation) -> bool {
        let Some((named, entry)) = self.entries.get_key_value(location) else {
            return false;
        };
        let was = entry.used;
        let named = Arc::clone(named);

        let now = self.tick();
        self.order.remove(&was);
        self.order.insert(now, Arc::clone(&named));
        if self.pictures.remove(&was).is_some() {
            self.pictures.insert(now, named);
        }
        if let Some(entry) = self.entries.get_mut(location) {
            entry.used = now;
        }
        true
    }

    fn make_room_for(&mut self, location: &MediaLocation) {
        let held = self.made_current(location);
        self.evict_until_one_fits();
        if held {
            return;
        }

        let now = self.tick();
        let named = Arc::new(location.clone());
        self.order.insert(now, Arc::clone(&named));
        self.entries.insert(
            named,
            Entry {
                used: now,
                ..Entry::default()
            },
        );
        self.rows += 1;
    }

    fn evict_until_one_fits(&mut self) {
        while self.rows >= ROWS_HELD {
            let Some(stale) = self.stalest_row() else {
                return;
            };
            self.forget(&stale);
        }
    }

    fn stalest_row(&self) -> Option<Arc<MediaLocation>> {
        self.order
            .values()
            .find(|named| {
                self.entries
                    .get(named.as_ref())
                    .is_some_and(|entry| !entry.is_pending())
            })
            .map(Arc::clone)
    }

    fn trim_pictures(&mut self) {
        while self.pictured > ART_BYTES_HELD {
            let Some((_, stale)) = self.pictures.pop_first() else {
                return;
            };
            let Some(entry) = self.entries.get_mut(stale.as_ref()) else {
                continue;
            };

            let freed = entry.art.bytes();
            entry.art = Look::Unasked;
            self.pictured -= freed;
        }
    }

    fn forget(&mut self, location: &MediaLocation) {
        let Some(entry) = self.entries.remove(location) else {
            return;
        };
        self.order.remove(&entry.used);
        self.pictures.remove(&entry.used);
        self.pictured -= entry.art.bytes();
        self.rows -= entry.rows();
    }

    fn tick(&mut self) -> u64 {
        self.clock = self.clock.saturating_add(1);
        self.clock
    }
}

#[derive(Default)]
struct Shelf {
    held: Mutex<Held>,
    landed: Condvar,
    revision: AtomicU64,
}

impl Shelf {
    fn claim_tags(&self, row: &Row) -> Claim<MediaInfo> {
        self.held.lock().claim_tags(row)
    }

    fn tags_read(&self, row: &Row) -> TagsRead {
        match self.held.lock().tags(row) {
            Look::Found(info) => TagsRead::Answered(info),
            Look::Nothing => TagsRead::Nothing,
            Look::Unasked | Look::Pending => TagsRead::NotYet,
        }
    }

    fn claim_art(&self, location: &MediaLocation) -> Claim<CoverArt> {
        self.held.lock().claim_art(location)
    }

    fn art_read(&self, location: &MediaLocation) -> ArtRead {
        match self.held.lock().art(location) {
            Look::Found(art) => ArtRead::Answered(art),
            Look::Nothing => ArtRead::Nothing,
            Look::Unasked | Look::Pending => ArtRead::NotYet,
        }
    }

    fn keep_tags(&self, row: &Row, look: Look<MediaInfo>) {
        self.held.lock().keep_tags(row, look);
        self.landing();
    }

    fn keep_art(&self, location: &MediaLocation, look: Look<CoverArt>) {
        self.held.lock().keep_art(location, look);
        self.landing();
    }

    fn picturing(&self, location: &MediaLocation) -> Picturing {
        if self.held.lock().art(location).is_pending() {
            Picturing::Copied
        } else {
            Picturing::Whether
        }
    }

    fn art_is_pending(&self, location: &MediaLocation) -> bool {
        self.held.lock().art(location).is_pending()
    }

    fn keep_what_the_tags_saw(&self, location: &MediaLocation, pictured: Pictured) {
        let look = match pictured {
            Pictured::Copied(art) => Look::Found(Arc::new(art)),
            Pictured::Bare => Look::Nothing,
            Pictured::StoodIn | Pictured::Carried => return,
        };
        let mut held = self.held.lock();
        if matches!(held.art(location), Look::Found(_)) {
            return;
        }
        held.keep_art(location, look);
        drop(held);
        self.landing();
    }

    fn release_tags(&self, row: &Row) {
        self.held.lock().keep_tags(row, Look::Unasked);
        self.landed.notify_all();
    }

    fn release_art(&self, location: &MediaLocation) {
        self.held.lock().keep_art(location, Look::Unasked);
        self.landed.notify_all();
    }

    fn landing(&self) {
        self.revision.fetch_add(1, Ordering::Release);
        self.landed.notify_all();
    }
}

pub(crate) struct Catalog {
    shelf: Arc<Shelf>,
    wanted: Option<Asking>,
    reader: Option<JoinHandle<()>>,
}

impl Catalog {
    pub(crate) fn new(sources: Arc<Sources>) -> Self {
        let shelf = Arc::new(Shelf::default());
        let (tags, tags_asked) = bounded(ROWS_ASKED);
        let (art, art_asked) = bounded(PICTURES_ASKED);
        let asked = Asked {
            tags: tags_asked,
            art: art_asked,
        };
        let reader = thread::Builder::new()
            .name("resonate-tags".to_owned())
            .spawn({
                let shelf = Arc::clone(&shelf);
                move || read_each(&shelf, &sources, &asked)
            });

        match reader {
            Ok(reader) => Self {
                shelf,
                wanted: Some(Asking { tags, art }),
                reader: Some(reader),
            },
            Err(error) => {
                tracing::warn!(%error, "no tag reader started; a queued row keeps its file name");
                Self {
                    shelf,
                    wanted: None,
                    reader: None,
                }
            }
        }
    }

    pub(crate) fn media(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Option<Arc<MediaInfo>> {
        let row = Row::new(location, span);
        match self.shelf.claim_tags(&row) {
            Claim::Answered(answer) => answer,
            Claim::Ours => {
                self.ask_for_tags(row);
                None
            }
        }
    }

    pub(crate) fn tags_read(&self, location: &MediaLocation, span: Option<FrameSpan>) -> TagsRead {
        self.shelf.tags_read(&Row::new(location, span))
    }

    pub(crate) fn art(&self, location: &MediaLocation) -> Option<Arc<CoverArt>> {
        match self.shelf.claim_art(location) {
            Claim::Answered(answer) => answer,
            Claim::Ours => {
                self.ask_for_art(location);
                None
            }
        }
    }

    pub(crate) fn art_read(&self, location: &MediaLocation) -> ArtRead {
        match self.art(location) {
            Some(art) => ArtRead::Answered(art),
            None => self.shelf.art_read(location),
        }
    }

    pub(crate) fn media_within(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
        patience: Duration,
    ) -> Option<Arc<MediaInfo>> {
        let until = Instant::now() + patience;
        if let Some(info) = self.media(location, span) {
            return Some(info);
        }

        let row = Row::new(location, span);
        let mut held = self.shelf.held.lock();
        loop {
            match held.tags(&row) {
                Look::Found(info) => return Some(info),
                Look::Pending => {}
                Look::Nothing | Look::Unasked => return None,
            }
            if self.shelf.landed.wait_until(&mut held, until).timed_out() {
                return None;
            }
        }
    }

    pub(crate) fn revision(&self) -> u64 {
        self.shelf.revision.load(Ordering::Acquire)
    }

    fn ask_for_tags(&self, row: Row) {
        match self.send(Wanted::Tags(row.clone())) {
            Sent::Waiting => {}
            Sent::Backlogged => self.shelf.release_tags(&row),
            Sent::NoReader => self.shelf.keep_tags(&row, Look::Nothing),
        }
    }

    fn ask_for_art(&self, location: &MediaLocation) {
        match self.send(Wanted::Art(location.clone())) {
            Sent::Waiting => {}
            Sent::Backlogged => self.shelf.release_art(location),
            Sent::NoReader => self.shelf.keep_art(location, Look::Nothing),
        }
    }

    fn send(&self, wanted: Wanted) -> Sent {
        self.wanted
            .as_ref()
            .map_or(Sent::NoReader, |reader| reader.send(wanted))
    }
}

impl Drop for Catalog {
    fn drop(&mut self) {
        drop(self.wanted.take());
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn read_each(shelf: &Shelf, sources: &Sources, asked: &Asked) {
    while let Some(wanted) = asked.next() {
        match wanted {
            Wanted::Tags(row) => {
                let read = match row.span {
                    Some(span) => probe_span(sources, &row.location, span),
                    None => whole(shelf, sources, &row.location),
                };
                let look = match read {
                    Ok(info) => Look::Found(Arc::new(info)),
                    Err(error) => {
                        let location = &row.location;
                        tracing::debug!(%error, %location, "a queued item would not be read");
                        Look::Nothing
                    }
                };
                shelf.keep_tags(&row, look);
            }
            Wanted::Art(location) => {
                if !shelf.art_is_pending(&location) {
                    continue;
                }
                let look = match probe_cover_art(sources, &location) {
                    Ok(Some(art)) => Look::Found(Arc::new(art)),
                    Ok(None) => Look::Nothing,
                    Err(error) => {
                        tracing::debug!(%error, %location, "a queued item would not be opened");
                        Look::Nothing
                    }
                };
                shelf.keep_art(&location, look);
            }
        }
    }
}

fn whole(
    shelf: &Shelf,
    sources: &Sources,
    location: &MediaLocation,
) -> resonate_codec::Result<MediaInfo> {
    let (info, pictured) = probe_pictured(sources, location, shelf.picturing(location))?;
    shelf.keep_what_the_tags_saw(location, pictured);
    Ok(info)
}

#[cfg(test)]
mod tests {
    use std::{env, fs, io::Cursor, path::PathBuf, process, sync::atomic::AtomicU32};

    use resonate_codec::{Error as CodecError, ImageFormat, Media, MediaProvider, Reading};
    use resonate_core::{Frames, SourceId};

    use super::*;

    const PATIENCE: Duration = Duration::from_secs(20);
    const RATE: u32 = 44_100;
    const CHANNELS: u16 = 2;
    const BITS: u16 = 16;
    const UTF8: u8 = 3;
    const SHEET_EXTENSION: &str = "cue";
    const SECOND_ROW: Frames = Frames(RATE as u64 * 30 / 75);
    const FRONT_COVER: u8 = 3;

    struct Fixture {
        path: PathBuf,
    }

    impl Fixture {
        fn holding(bytes: &[u8]) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = env::temp_dir().join(format!(
                "resonate-catalog-{}-{}.wav",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::write(&path, bytes).expect("a writable temporary file");
            Self { path }
        }

        fn location(&self) -> MediaLocation {
            MediaLocation::local(&self.path)
        }

        fn cut_in_two(&self) -> Self {
            let named = self
                .path
                .file_name()
                .and_then(|named| named.to_str())
                .expect("a temporary file with a name");
            fs::write(
                self.path.with_extension(SHEET_EXTENSION),
                format!(
                    "TITLE \"Meddle\"\nFILE \"{named}\" WAVE\n  TRACK 01 AUDIO\n    TITLE \
                     \"One of These Days\"\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    \
                     TITLE \"Echoes\"\n    INDEX 01 00:00:30\n"
                ),
            )
            .expect("a writable temporary file");

            Self {
                path: self.path.clone(),
            }
        }
    }

    fn catalog() -> Catalog {
        Catalog::new(Arc::new(Sources::local()))
    }

    struct Counted {
        source: SourceId,
        bytes: Vec<u8>,
        opens: Arc<AtomicU32>,
    }

    impl MediaProvider for Counted {
        fn source(&self) -> &SourceId {
            &self.source
        }

        fn open(&self, _location: &MediaLocation) -> resonate_codec::Result<Media> {
            self.opens.fetch_add(1, Ordering::Relaxed);
            Ok(Media {
                stream: Box::new(Reading::new(Cursor::new(self.bytes.clone()))),
                hint: None,
            })
        }
    }

    struct Stalls {
        source: SourceId,
        gate: Arc<Mutex<()>>,
    }

    impl MediaProvider for Stalls {
        fn source(&self) -> &SourceId {
            &self.source
        }

        fn open(&self, location: &MediaLocation) -> resonate_codec::Result<Media> {
            drop(self.gate.lock());
            Err(CodecError::LocatorNotUsable {
                location: location.clone(),
            })
        }
    }

    fn rows(source: &SourceId, count: usize) -> Vec<MediaLocation> {
        (0..count)
            .map(|row| MediaLocation::new(source.clone(), format!("tracks/{row}.wav")))
            .collect()
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
            let _ = fs::remove_file(self.path.with_extension(SHEET_EXTENSION));
        }
    }

    fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
        into.extend_from_slice(id);
        into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        into.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            into.push(0);
        }
    }

    fn frame(into: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
        into.extend_from_slice(id);
        into.extend_from_slice(&synchsafe(body.len() as u32));
        into.extend_from_slice(&[0, 0]);
        into.extend_from_slice(body);
    }

    fn synchsafe(value: u32) -> [u8; 4] {
        [
            ((value >> 21) & 0x7f) as u8,
            ((value >> 14) & 0x7f) as u8,
            ((value >> 7) & 0x7f) as u8,
            (value & 0x7f) as u8,
        ]
    }

    fn png(size: usize) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend((0..size).map(|n| n as u8));
        bytes
    }

    fn id3_with_a_picture(bytes: &[u8]) -> Vec<u8> {
        let mut body = vec![UTF8];
        body.extend_from_slice(b"image/png\0");
        body.push(FRONT_COVER);
        body.push(0);
        body.extend_from_slice(bytes);

        let mut frames = Vec::new();
        frame(&mut frames, b"APIC", &body);

        let mut tag = b"ID3\x04\x00\x00".to_vec();
        tag.extend_from_slice(&synchsafe(frames.len() as u32));
        tag.extend_from_slice(&frames);
        tag
    }

    fn wav(titled: &str, by: &str) -> Vec<u8> {
        let block_align = CHANNELS * BITS / 8;
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1_u16.to_le_bytes());
        fmt.extend_from_slice(&CHANNELS.to_le_bytes());
        fmt.extend_from_slice(&RATE.to_le_bytes());
        fmt.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
        fmt.extend_from_slice(&block_align.to_le_bytes());
        fmt.extend_from_slice(&BITS.to_le_bytes());

        let mut info = b"INFO".to_vec();
        for (id, value) in [(b"INAM", titled), (b"IART", by)] {
            let mut terminated = value.as_bytes().to_vec();
            terminated.push(0);
            chunk(&mut info, id, &terminated);
        }

        let mut body = b"WAVE".to_vec();
        chunk(&mut body, b"fmt ", &fmt);
        chunk(&mut body, b"LIST", &info);
        chunk(
            &mut body,
            b"data",
            &vec![0; RATE as usize * usize::from(block_align)],
        );

        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }

    fn wav_with_a_cover(titled: &str, by: &str, picture: &[u8]) -> Vec<u8> {
        let mut file = id3_with_a_picture(picture);
        file.extend_from_slice(&wav(titled, by));
        file
    }

    fn art_within(catalog: &Catalog, location: &MediaLocation) -> Option<Arc<CoverArt>> {
        let until = Instant::now() + PATIENCE;
        loop {
            if let Some(art) = catalog.art(location) {
                return Some(art);
            }
            let mut held = catalog.shelf.held.lock();
            match held.art(location) {
                Look::Pending => {}
                Look::Found(art) => return Some(art),
                Look::Nothing | Look::Unasked => return None,
            }
            if catalog
                .shelf
                .landed
                .wait_until(&mut held, until)
                .timed_out()
            {
                return None;
            }
        }
    }

    fn pictured(bytes: usize) -> Look<CoverArt> {
        Look::Found(Arc::new(CoverArt {
            format: ImageFormat::Png,
            bytes: vec![0; bytes],
        }))
    }

    #[test]
    fn a_row_nothing_has_read_answers_with_its_tags_once_the_read_lands() {
        let file = Fixture::holding(&wav("Echoes", "Pink Floyd"));
        let catalog = catalog();

        assert!(
            catalog.media(&file.location(), None).is_none(),
            "a first look answered before the file was read"
        );

        let info = catalog
            .media_within(&file.location(), None, PATIENCE)
            .expect("a well-formed wav is read");
        assert_eq!(info.tags.title.as_deref(), Some("Echoes"));
        assert_eq!(info.tags.artist.as_deref(), Some("Pink Floyd"));
        assert_eq!(info.duration, Some(resonate_core::Frames(u64::from(RATE))));

        let landed = catalog.revision();
        assert!(landed > 0, "a read that landed moved nothing");
        assert!(catalog.media(&file.location(), None).is_some());
        assert_eq!(catalog.revision(), landed, "a held row was read again");
    }

    #[test]
    fn each_row_a_sheet_cuts_out_of_one_file_is_read_as_the_track_it_names() {
        let file = Fixture::holding(&wav("Meddle", "Pink Floyd"));
        let cut = file.cut_in_two();
        let catalog = catalog();

        let echoes = FrameSpan::between(SECOND_ROW, Frames(u64::from(RATE)));
        let row = catalog
            .media_within(&cut.location(), Some(echoes), PATIENCE)
            .expect("the row the sheet names");
        assert_eq!(row.tags.title.as_deref(), Some("Echoes"));
        assert_eq!(row.tags.album.as_deref(), Some("Meddle"));
        assert_eq!(row.duration, echoes.frames());

        let whole = catalog
            .media_within(&cut.location(), None, PATIENCE)
            .expect("the file the rows are cut from");
        assert_eq!(whole.tags.title.as_deref(), Some("Meddle"));
        assert_eq!(whole.duration, Some(Frames(u64::from(RATE))));

        assert_eq!(
            catalog.shelf.held.lock().entries.len(),
            1,
            "a row cut out of a file was held apart from the file"
        );
        assert_eq!(catalog.shelf.held.lock().rows, 2);
    }

    #[test]
    fn a_file_that_is_not_audio_is_answered_for_without_being_read_again() {
        let file = Fixture::holding(b"not a container");
        let catalog = catalog();

        assert!(
            catalog
                .media_within(&file.location(), None, PATIENCE)
                .is_none()
        );
        let refused = catalog.revision();

        assert!(catalog.media(&file.location(), None).is_none());
        assert!(
            catalog
                .media_within(&file.location(), None, PATIENCE)
                .is_none()
        );
        assert_eq!(catalog.revision(), refused, "the file was opened again");
    }

    #[test]
    fn a_row_nothing_has_read_answers_with_the_cover_the_file_carries() {
        let picture = png(512);
        let file = Fixture::holding(&wav_with_a_cover("Echoes", "Pink Floyd", &picture));
        let catalog = catalog();

        assert!(
            catalog.art(&file.location()).is_none(),
            "a first look answered before the file was opened"
        );

        let art = art_within(&catalog, &file.location()).expect("a tagged wav carries its picture");
        assert_eq!(art.format, ImageFormat::Png);
        assert_eq!(art.bytes, picture);
    }

    #[test]
    fn a_cover_still_being_read_is_told_apart_from_a_file_that_carries_none() {
        let pictured = Fixture::holding(&wav_with_a_cover("Echoes", "Pink Floyd", &png(512)));
        let bare = Fixture::holding(&wav("Echoes", "Pink Floyd"));
        let catalog = catalog();

        assert!(matches!(
            catalog.art_read(&pictured.location()),
            ArtRead::NotYet | ArtRead::Answered(_)
        ));
        assert!(art_within(&catalog, &pictured.location()).is_some());
        assert!(matches!(
            catalog.art_read(&pictured.location()),
            ArtRead::Answered(_)
        ));

        assert!(art_within(&catalog, &bare.location()).is_none());
        assert!(matches!(
            catalog.art_read(&bare.location()),
            ArtRead::Nothing
        ));
    }

    #[test]
    fn a_file_carrying_no_cover_is_answered_for_without_being_opened_again() {
        let file = Fixture::holding(&wav("Echoes", "Pink Floyd"));
        let catalog = catalog();

        assert!(art_within(&catalog, &file.location()).is_none());
        let refused = catalog.revision();

        assert!(catalog.art(&file.location()).is_none());
        assert_eq!(catalog.revision(), refused, "the file was opened again");
    }

    #[test]
    fn the_tags_of_a_row_survive_the_cover_it_was_asked_for() {
        let picture = png(512);
        let file = Fixture::holding(&wav_with_a_cover("Echoes", "Pink Floyd", &picture));
        let catalog = catalog();

        let info = catalog
            .media_within(&file.location(), None, PATIENCE)
            .expect("a well-formed wav is read");
        assert!(art_within(&catalog, &file.location()).is_some());

        let held = catalog
            .media(&file.location(), None)
            .expect("the tags were dropped by the look for a cover");
        assert!(Arc::ptr_eq(&info, &held), "the file was read twice");
    }

    fn counted(bytes: Vec<u8>) -> (Catalog, MediaLocation, Arc<AtomicU32>) {
        let opens = Arc::new(AtomicU32::new(0));
        let named = SourceId::new("counted").expect("a lowercase name");
        let sources = Sources::local().and(Arc::new(Counted {
            source: named.clone(),
            bytes,
            opens: Arc::clone(&opens),
        }));
        let row = rows(&named, 1).remove(0);
        (Catalog::new(Arc::new(sources)), row, opens)
    }

    #[test]
    fn a_row_whose_tags_found_no_picture_is_never_opened_for_one() {
        let (catalog, row, opens) = counted(wav("Echoes", "Pink Floyd"));

        assert!(catalog.media_within(&row, None, PATIENCE).is_some());
        assert!(catalog.art(&row).is_none());
        assert!(art_within(&catalog, &row).is_none());
        assert_eq!(
            opens.load(Ordering::Relaxed),
            1,
            "a file its tags read showed no picture was opened again for one"
        );
    }

    #[test]
    fn a_row_whose_picture_is_waited_on_is_read_for_both_in_one_open() {
        let picture = png(512);
        let (catalog, row, opens) = counted(wav_with_a_cover("Echoes", "Pink Floyd", &picture));

        assert!(matches!(catalog.shelf.claim_art(&row), Claim::Ours));
        let info = catalog
            .media_within(&row, None, PATIENCE)
            .expect("a well-formed wav is read");
        assert_eq!(info.tags.title.as_deref(), Some("Echoes"));

        let art = catalog.art(&row).expect("the picture landed with the tags");
        assert_eq!(art.bytes, picture);
        assert_eq!(
            opens.load(Ordering::Relaxed),
            1,
            "the file was opened twice"
        );
    }

    #[test]
    fn a_row_whose_picture_nobody_waits_on_does_not_copy_it_with_the_tags() {
        let picture = png(512);
        let (catalog, row, opens) = counted(wav_with_a_cover("Echoes", "Pink Floyd", &picture));

        assert!(catalog.media_within(&row, None, PATIENCE).is_some());
        assert_eq!(catalog.shelf.held.lock().pictured, 0);

        let art = art_within(&catalog, &row).expect("the picture is read when it is asked for");
        assert_eq!(art.bytes, picture);
        assert_eq!(opens.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn one_claim_on_a_row_is_handed_out_however_many_ask() {
        let mut held = Held::default();
        let row = MediaLocation::local("/music/one.flac");

        assert!(matches!(
            held.claim_tags(&Row::new(&row, None)),
            Claim::Ours
        ));
        assert!(matches!(
            held.claim_tags(&Row::new(&row, None)),
            Claim::Answered(None)
        ));
        assert!(matches!(held.claim_art(&row), Claim::Ours));
        assert!(matches!(held.claim_art(&row), Claim::Answered(None)));
    }

    #[test]
    fn a_row_every_thread_asks_for_at_once_is_opened_once() {
        const ROWS: usize = 64;
        const ASKING: usize = 8;

        let opens = Arc::new(AtomicU32::new(0));
        let named = SourceId::new("counted").expect("a lowercase name");
        let sources = Sources::local().and(Arc::new(Counted {
            source: named.clone(),
            bytes: wav("Echoes", "Pink Floyd"),
            opens: Arc::clone(&opens),
        }));
        let catalog = Catalog::new(Arc::new(sources));
        let asked = rows(&named, ROWS);

        thread::scope(|scope| {
            for _ in 0..ASKING {
                scope.spawn(|| {
                    for row in &asked {
                        let _ = catalog.media(row, None);
                    }
                });
            }
        });

        for row in &asked {
            assert!(
                catalog.media_within(row, None, PATIENCE).is_some(),
                "a row every thread asked for was never read"
            );
        }
        assert_eq!(
            opens.load(Ordering::Relaxed) as usize,
            ROWS,
            "a row two threads asked for at once was opened twice"
        );
    }

    #[test]
    fn a_reader_that_is_behind_leaves_a_row_to_be_asked_for_again() {
        let gate = Arc::new(Mutex::new(()));
        let named = SourceId::new("stalls").expect("a lowercase name");
        let sources = Sources::local().and(Arc::new(Stalls {
            source: named.clone(),
            gate: Arc::clone(&gate),
        }));
        let catalog = Catalog::new(Arc::new(sources));
        let stalled = gate.lock();

        let asked = rows(&named, ROWS_ASKED + 2);
        for row in &asked {
            assert!(catalog.media(row, None).is_none());
        }
        let turned_away = asked
            .iter()
            .filter(|row| {
                matches!(
                    catalog.shelf.held.lock().tags(&Row::new(row, None)),
                    Look::Unasked
                )
            })
            .count();
        assert!(
            turned_away > 0,
            "a queue bounded at {ROWS_ASKED} took all {} rows",
            asked.len()
        );
        assert!(
            catalog.shelf.held.lock().entries.len() <= asked.len(),
            "a row turned away was held anyway"
        );

        drop(stalled);
        let again = asked.last().expect("a row to ask for again");
        let settled = Instant::now() + PATIENCE;
        while !matches!(
            catalog.shelf.held.lock().tags(&Row::new(again, None)),
            Look::Nothing
        ) {
            assert!(
                Instant::now() < settled,
                "a row the queue turned away was never read"
            );
            let _ = catalog.media(again, None);
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_row_looked_at_again_is_not_the_one_evicted() {
        let mut held = Held::default();
        let first = MediaLocation::local("/music/first.flac");
        held.keep_tags(&Row::new(&first, None), Look::Nothing);
        for row in 0..ROWS_HELD - 1 {
            held.keep_tags(
                &Row::new(&MediaLocation::local(format!("/music/{row}.flac")), None),
                Look::Nothing,
            );
        }

        let _ = held.tags(&Row::new(&first, None));
        held.keep_tags(
            &Row::new(&MediaLocation::local("/music/last.flac"), None),
            Look::Nothing,
        );

        assert!(
            matches!(held.tags(&Row::new(&first, None)), Look::Nothing),
            "the row looked at most recently was the one evicted"
        );
        assert!(
            matches!(
                held.tags(&Row::new(&MediaLocation::local("/music/0.flac"), None)),
                Look::Unasked
            ),
            "the row looked at longest ago was kept"
        );
    }

    #[test]
    fn the_rows_held_stay_within_the_bound_however_many_are_asked_for() {
        let mut held = Held::default();
        for row in 0..ROWS_HELD + 64 {
            held.keep_tags(
                &Row::new(&MediaLocation::local(format!("/music/{row}.flac")), None),
                Look::Nothing,
            );
        }

        assert!(held.entries.len() <= ROWS_HELD, "{}", held.entries.len());
        assert!(
            matches!(
                held.tags(&Row::new(
                    &MediaLocation::local(format!("/music/{}.flac", ROWS_HELD + 63)),
                    None
                )),
                Look::Nothing
            ),
            "the row just stored was the one evicted"
        );
    }

    #[test]
    fn a_row_still_waiting_on_its_read_is_not_the_one_evicted() {
        let mut held = Held::default();
        let wanted = MediaLocation::local("/music/wanted.flac");
        held.keep_tags(&Row::new(&wanted, None), Look::Pending);
        for row in 0..ROWS_HELD + 64 {
            held.keep_tags(
                &Row::new(&MediaLocation::local(format!("/music/{row}.flac")), None),
                Look::Nothing,
            );
        }

        assert!(matches!(held.tags(&Row::new(&wanted, None)), Look::Pending));
    }

    #[test]
    fn the_pictures_held_stay_within_their_budget_and_the_tags_beside_them_stay() {
        let mut held = Held::default();
        let picture = ART_BYTES_HELD / 4;
        let rows: Vec<MediaLocation> = (0..8)
            .map(|row| MediaLocation::local(format!("/music/{row}.flac")))
            .collect();

        for row in &rows {
            held.keep_tags(&Row::new(row, None), Look::Nothing);
            held.keep_art(row, pictured(picture));
        }

        assert!(held.pictured <= ART_BYTES_HELD, "{}", held.pictured);
        assert!(
            matches!(held.art(&rows[0]), Look::Unasked),
            "the picture looked at longest ago was kept"
        );
        assert!(
            matches!(held.art(&rows[7]), Look::Found(_)),
            "the picture just stored was the one dropped"
        );
        assert!(
            matches!(held.tags(&Row::new(&rows[0], None)), Look::Nothing),
            "the tags went with the picture"
        );
    }

    #[test]
    fn a_picture_dropped_for_a_newer_one_gives_its_bytes_back() {
        let mut held = Held::default();
        let row = MediaLocation::local("/music/one.flac");

        held.keep_art(&row, pictured(1_024));
        held.keep_art(&row, pictured(64));

        assert_eq!(held.pictured, 64);
        assert_eq!(held.entries.len(), 1);
    }

    #[test]
    fn a_row_evicted_gives_the_bytes_of_its_picture_back() {
        let mut held = Held::default();
        for row in 0..ROWS_HELD + 64 {
            let location = MediaLocation::local(format!("/music/{row}.flac"));
            held.keep_art(&location, pictured(16));
        }

        assert_eq!(held.pictured, held.entries.len() * 16);
    }
}
