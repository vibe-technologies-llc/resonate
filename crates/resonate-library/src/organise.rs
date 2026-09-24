use std::{
    cmp::Ordering as Ranking,
    ffi::OsStr,
    fmt, fs,
    io::{self, Read as _},
    iter, mem,
    num::NonZeroU32,
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};

use ahash::{AHashMap, AHashSet};
use resonate_core::{AlbumId, MediaLocation, TrackId};
use rusqlite::{OptionalExtension as _, Statement, Transaction, params};

use crate::{
    Library, StoreOp,
    error::{Error, FieldName, LayoutFault, MoveOp, Result},
    pass::{Cancelling, OrganiseHandle, PassHandle, PassKind},
    scan, store,
};

pub const DEFAULT_LAYOUT: &str = "{albumartist}/{album}/{disc}{track} {title}";

const MOVES_PER_BATCH: usize = 256;

const COMPONENT_BYTES: usize = 255;
const SHEET_EXTENSION: &str = "cue";
const STAGED: &str = ".resonate-staging";
const COMPARED_AT_ONCE: usize = 1 << 20;
const SEGMENT_SEPARATOR: char = '/';
const SEPARATOR_STANDS_IN: char = '-';
const REFUSED_BY_A_PORTABLE_VOLUME: [char; 8] = ['\\', ':', '*', '?', '"', '<', '>', '|'];
const PORTABLE_VOLUMES: [&str; 7] = ["vfat", "msdos", "exfat", "ntfs", "ntfs3", "fuseblk", "fat"];
const MOUNT_TABLE: &str = "/proc/self/mounts";
const OCTAL_ESCAPE: char = '\\';
const EXTENSION_SEPARATOR: char = '.';
const FIRST_PRINTABLE: char = ' ';
const DELETED_CHARACTER: char = '\u{7f}';

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    AlbumArtist,
    Artist,
    Album,
    Title,
    Year,
    Disc,
    Track,
    Extension,
}

impl Field {
    pub const ALL: [Self; 8] = [
        Self::AlbumArtist,
        Self::Artist,
        Self::Album,
        Self::Title,
        Self::Year,
        Self::Disc,
        Self::Track,
        Self::Extension,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AlbumArtist => "albumartist",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::Title => "title",
            Self::Year => "year",
            Self::Disc => "disc",
            Self::Track => "track",
            Self::Extension => "ext",
        }
    }

    pub fn read(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.as_str() == text)
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Literal(Box<str>),
    Named(Field),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Segment {
    pieces: Vec<Piece>,
}

impl Segment {
    fn is_a_step_of_the_path_itself(&self) -> bool {
        match self.pieces.as_slice() {
            [Piece::Literal(only)] => &**only == "." || &**only == "..",
            _ => false,
        }
    }

    fn write(&self, named: &Named<'_>) -> String {
        let mut written = String::new();
        for piece in &self.pieces {
            match piece {
                Piece::Literal(text) => written.push_str(text),
                Piece::Named(field) => write_field(*field, named, &mut written),
            }
        }
        written
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    segments: Vec<Segment>,
    names_the_extension: bool,
}

impl Layout {
    pub fn read(template: &str) -> Result<Self> {
        if template.is_empty() {
            return Err(Error::LayoutSyntax {
                at: 0,
                fault: LayoutFault::Empty,
            });
        }

        let mut segments = Vec::new();
        let mut start = 0;
        for text in template.split(SEGMENT_SEPARATOR) {
            let at = start;
            start += text.len() + SEGMENT_SEPARATOR.len_utf8();
            if text.is_empty() {
                return Err(Error::LayoutSyntax {
                    at,
                    fault: LayoutFault::EmptySegment,
                });
            }
            segments.push(read_segment(text, at)?);
        }

        if let Some(segment) = segments
            .iter()
            .position(Segment::is_a_step_of_the_path_itself)
        {
            return Err(Error::LayoutEscapes { segment });
        }

        let names_the_extension = segments
            .iter()
            .flat_map(|segment| &segment.pieces)
            .any(|piece| matches!(piece, Piece::Named(Field::Extension)));

        Ok(Self {
            segments,
            names_the_extension,
        })
    }

    pub(crate) fn render(&self, named: &Named<'_>, naming: Naming) -> Option<PathBuf> {
        let last = self.segments.len().checked_sub(1)?;
        let appended = self.appended_extension(named);
        let tail = appended.map_or(0, |extension| {
            extension.len() + EXTENSION_SEPARATOR.len_utf8()
        });

        let mut path = PathBuf::new();
        for (index, segment) in self.segments.iter().enumerate() {
            let budget = if index == last {
                COMPONENT_BYTES.saturating_sub(tail)
            } else {
                COMPONENT_BYTES
            };

            let mut component = as_one_component(&segment.write(named), budget, naming);
            if component.is_empty() {
                if index == last {
                    return None;
                }
                continue;
            }

            if index == last
                && let Some(extension) = appended
            {
                component.push(EXTENSION_SEPARATOR);
                component.push_str(extension);
            }
            path.push(component);
        }

        Some(path)
    }

    fn appended_extension<'a>(&self, named: &Named<'a>) -> Option<&'a str> {
        if self.names_the_extension {
            return None;
        }
        named.extension.filter(|extension| !extension.is_empty())
    }
}

impl Default for Layout {
    fn default() -> Self {
        Self::read(DEFAULT_LAYOUT).expect("the layout this build ships is one it can read")
    }
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (at, segment) in self.segments.iter().enumerate() {
            if at > 0 {
                write!(f, "{SEGMENT_SEPARATOR}")?;
            }
            write!(f, "{segment}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for piece in &self.pieces {
            match piece {
                Piece::Literal(text) => f.write_str(&escaped(text))?,
                Piece::Named(field) => write!(f, "{{{field}}}")?,
            }
        }
        Ok(())
    }
}

fn escaped(literal: &str) -> String {
    let mut written = String::with_capacity(literal.len());
    for letter in literal.chars() {
        match letter {
            '{' => written.push_str("{{"),
            '}' => written.push_str("}}"),
            _ => written.push(letter),
        }
    }
    written
}

fn read_segment(text: &str, start: usize) -> Result<Segment> {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let bytes = text.as_bytes();
    let mut at = 0;

    while at < bytes.len() {
        match bytes[at] {
            b'{' if bytes.get(at + 1) == Some(&b'{') => {
                literal.push('{');
                at += 2;
            }
            b'}' if bytes.get(at + 1) == Some(&b'}') => {
                literal.push('}');
                at += 2;
            }
            b'{' => {
                let Some(width) = text[at + 1..].find('}') else {
                    return Err(Error::LayoutSyntax {
                        at: start + at,
                        fault: LayoutFault::Unclosed,
                    });
                };
                let name = &text[at + 1..at + 1 + width];
                if name.is_empty() {
                    return Err(Error::LayoutSyntax {
                        at: start + at,
                        fault: LayoutFault::EmptyField,
                    });
                }
                let Some(field) = Field::read(name) else {
                    return Err(Error::UnknownLayoutField {
                        field: FieldName::new(name),
                    });
                };
                flush(&mut literal, &mut pieces);
                pieces.push(Piece::Named(field));
                at += width + 2;
            }
            b'}' => {
                return Err(Error::LayoutSyntax {
                    at: start + at,
                    fault: LayoutFault::Unopened,
                });
            }
            _ => {
                let next = text[at..]
                    .find(['{', '}'])
                    .map_or(bytes.len(), |offset| at + offset);
                literal.push_str(&text[at..next]);
                at = next;
            }
        }
    }

    flush(&mut literal, &mut pieces);
    Ok(Segment { pieces })
}

fn flush(literal: &mut String, pieces: &mut Vec<Piece>) {
    if !literal.is_empty() {
        pieces.push(Piece::Literal(mem::take(literal).into_boxed_str()));
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Named<'a> {
    pub album_artist: Option<&'a str>,
    pub artist: Option<&'a str>,
    pub album: Option<&'a str>,
    pub title: &'a str,
    pub year: Option<i32>,
    pub disc: Option<NonZeroU32>,
    pub discs: u32,
    pub track: Option<u32>,
    pub extension: Option<&'a str>,
}

fn write_field(field: Field, named: &Named<'_>, into: &mut String) {
    match field {
        Field::AlbumArtist => into.push_str(named.album_artist.unwrap_or_default()),
        Field::Artist => into.push_str(named.artist.unwrap_or_default()),
        Field::Album => into.push_str(named.album.unwrap_or_default()),
        Field::Title => into.push_str(named.title),
        Field::Year => {
            if let Some(year) = named.year {
                into.push_str(&format!("{year:04}"));
            }
        }
        Field::Disc => {
            if let Some(disc) = named.disc.filter(|_| named.discs > 1) {
                into.push_str(&format!("{disc}-"));
            }
        }
        Field::Track => {
            if let Some(track) = named.track {
                into.push_str(&format!("{track:02}"));
            }
        }
        Field::Extension => into.push_str(named.extension.unwrap_or_default()),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Naming {
    #[default]
    Anything,
    Portable,
}

impl Naming {
    fn of(root: &Path) -> Self {
        match fs::read_to_string(MOUNT_TABLE) {
            Ok(table) => Self::in_table(&table, root),
            Err(error) => {
                tracing::debug!(%error, "no mount table to say what a root's volume will take");
                Self::Anything
            }
        }
    }

    fn in_table(table: &str, root: &Path) -> Self {
        let mounted = table
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let _device = fields.next()?;
                let point = PathBuf::from(unescaped_mount(fields.next()?));
                let kind = fields.next()?;
                root.starts_with(&point).then_some((point, kind))
            })
            .max_by_key(|(point, _)| point.components().count());

        match mounted {
            Some((_, kind)) if PORTABLE_VOLUMES.contains(&kind) => Self::Portable,
            _ => Self::Anything,
        }
    }

    fn refuses(self, character: char) -> bool {
        self == Self::Portable && REFUSED_BY_A_PORTABLE_VOLUME.contains(&character)
    }
}

fn unescaped_mount(field: &str) -> String {
    let mut written = String::with_capacity(field.len());
    let mut characters = field.chars().peekable();
    while let Some(character) = characters.next() {
        if character != OCTAL_ESCAPE {
            written.push(character);
            continue;
        }
        let digits: String = (0..3).filter_map(|_| characters.next()).collect();
        match u8::from_str_radix(&digits, 8) {
            Ok(byte) => written.push(char::from(byte)),
            Err(_) => {
                written.push(character);
                written.push_str(&digits);
            }
        }
    }
    written
}

fn as_one_component(text: &str, budget: usize, naming: Naming) -> String {
    let mut written = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            SEGMENT_SEPARATOR => written.push(SEPARATOR_STANDS_IN),
            refused if naming.refuses(refused) => written.push(SEPARATOR_STANDS_IN),
            control if control < FIRST_PRINTABLE || control == DELETED_CHARACTER => {}
            kept => written.push(kept),
        }
    }

    let trimmed = without_padding(&written);
    without_padding(largest_prefix_within(trimmed, budget)).to_owned()
}

fn without_padding(text: &str) -> &str {
    text.trim_start()
        .trim_end_matches(|character: char| character.is_whitespace() || character == '.')
}

fn largest_prefix_within(text: &str, budget: usize) -> &str {
    if text.len() <= budget {
        return text;
    }
    let end = (0..=budget)
        .rev()
        .find(|at| text.is_char_boundary(*at))
        .unwrap_or_default();
    &text[..end]
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sidecar {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Companion {
    pub from: PathBuf,
    pub to: PathBuf,
    pub rows: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Move {
    pub from: PathBuf,
    pub to: PathBuf,
    pub rows: u32,
    pub companions: Vec<Companion>,
    pub sidecars: Vec<Sidecar>,
}

impl Move {
    pub fn files(&self) -> impl Iterator<Item = (&Path, &Path)> {
        iter::once((self.from.as_path(), self.to.as_path())).chain(
            self.companions
                .iter()
                .map(|companion| (companion.from.as_path(), companion.to.as_path())),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Unidentified,
    Loose,
    Collided { with: PathBuf },
    SharesASheet { sheet: PathBuf },
    SourceGone,
    Unmoved { kind: io::ErrorKind },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub from: PathBuf,
    pub refusal: Refusal,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub moves: Vec<Move>,
    pub refused: Vec<Refused>,
    pub unchanged: u64,
    pub folders: Vec<PathBuf>,
}

impl Plan {
    pub fn files_moving(&self) -> usize {
        self.moves
            .iter()
            .map(|planned| planned.files().count())
            .sum()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrganiseOptions {
    pub layout: Layout,
    pub roots: Vec<PathBuf>,
    pub apply: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OrganiseStats {
    pub moved: u64,
    pub unchanged: u64,
    pub unidentified: u64,
    pub collided: u64,
    pub failed: u64,
    pub pruned: u64,
}

#[derive(Debug, Default)]
pub struct OrganiseProgress {
    moved: AtomicU64,
    unchanged: AtomicU64,
    unidentified: AtomicU64,
    collided: AtomicU64,
    failed: AtomicU64,
    pruned: AtomicU64,
    cancelled: AtomicBool,
}

impl OrganiseProgress {
    pub fn snapshot(&self) -> OrganiseStats {
        OrganiseStats {
            moved: self.moved.load(Ordering::Relaxed),
            unchanged: self.unchanged.load(Ordering::Relaxed),
            unidentified: self.unidentified.load(Ordering::Relaxed),
            collided: self.collided.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            pruned: self.pruned.load(Ordering::Relaxed),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Cancelling for OrganiseProgress {
    fn cancel(&self) {
        OrganiseProgress::cancel(self);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrganiseSummary {
    pub stats: OrganiseStats,
    pub plan: Plan,
    pub cancelled: bool,
}

pub(crate) fn start(library: Library, options: OrganiseOptions) -> Result<OrganiseHandle> {
    let walking = library.walk_the_tree()?;
    let progress = Arc::new(OrganiseProgress::default());
    let owned = Arc::clone(&progress);

    let thread = thread::Builder::new()
        .name("resonate-organise".to_owned())
        .spawn(move || {
            let outcome = run(&library, &options, &progress);
            drop(walking);
            outcome
        })
        .map_err(|source| Error::ThreadSpawn { source })?;

    Ok(PassHandle::of(PassKind::Organise, owned, thread))
}

fn run(
    library: &Library,
    options: &OrganiseOptions,
    progress: &OrganiseProgress,
) -> Result<OrganiseSummary> {
    let known = library.roots()?;
    for root in &options.roots {
        if !known.contains(root) {
            return Err(Error::NotARoot { path: root.clone() });
        }
    }

    let rows = library.tracks_to_file(&options.roots)?;
    let mut plan = planned(&rows, &options.layout, progress);
    if options.apply {
        let roots: AHashSet<PathBuf> = rows.iter().map(|row| row.root.clone()).collect();
        apply(library, &mut plan, &roots, progress);
    }

    Ok(OrganiseSummary {
        stats: progress.snapshot(),
        plan,
        cancelled: progress.is_cancelled(),
    })
}

#[derive(Clone, Debug)]
pub(crate) struct TrackToFile {
    pub id: TrackId,
    pub path: PathBuf,
    pub root: PathBuf,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_id: Option<AlbumId>,
    pub album_artist: Option<String>,
    pub year: Option<i32>,
    pub track: Option<u32>,
    pub disc: Option<NonZeroU32>,
    pub discs: u32,
}

fn planned(rows: &[TrackToFile], layout: &Layout, progress: &OrganiseProgress) -> Plan {
    let groups: Vec<&[TrackToFile]> = rows.chunk_by(|one, next| one.path == next.path).collect();
    let at: AHashMap<&Path, usize> = groups
        .iter()
        .enumerate()
        .filter_map(|(at, group)| Some((group.first()?.path.as_path(), at)))
        .collect();

    let mut planner = Planner::over(rows, layout, progress);
    let mut filed = vec![false; groups.len()];
    for (index, group) in groups.iter().enumerate() {
        if progress.is_cancelled() {
            break;
        }
        if filed[index] {
            continue;
        }
        let Some(first) = group.first() else {
            continue;
        };

        let tied = planner.tied_by_sheets(&first.path);
        if tied.sheets.is_empty() {
            filed[index] = true;
            planner.file(group);
            continue;
        }

        let members: Vec<Option<usize>> = tied
            .files
            .iter()
            .map(|file| at.get(file.as_path()).copied())
            .collect();
        for member in members.iter().flatten() {
            filed[*member] = true;
        }
        let held: Vec<&[TrackToFile]> = members.iter().flatten().map(|at| groups[*at]).collect();
        if members.iter().any(Option::is_none) {
            planner.refuse_the_sheets(&held, &tied, None);
            continue;
        }
        planner.file_together(&held, &tied);
    }

    planner.settle()
}

struct Beside {
    files: Vec<PathBuf>,
    nothing_but_files: bool,
}

impl Beside {
    fn of(folder: &Path) -> Self {
        let Ok(listed) = fs::read_dir(folder) else {
            tracing::debug!(
                folder = %folder.display(),
                "the folder a track sits in could not be listed"
            );
            return Self {
                files: Vec::new(),
                nothing_but_files: false,
            };
        };

        let mut files = Vec::new();
        let mut nothing_but_files = true;
        for entry in listed {
            match entry.as_ref().map(std::fs::DirEntry::file_type) {
                Ok(Ok(kind)) if !kind.is_dir() => {
                    files.push(entry.expect("an entry that answered its kind").path());
                }
                _ => nothing_but_files = false,
            }
        }

        files.sort();
        Self {
            files,
            nothing_but_files,
        }
    }
}

#[derive(Debug)]
enum InTheWay {
    Stands(Refusal),
    MayGo(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Unwalked,
    Walking,
    Ordered,
    Doomed,
}

struct Planned {
    planned: Move,
    waits_for: Option<PathBuf>,
}

struct Planner<'a> {
    layout: &'a Layout,
    progress: &'a OrganiseProgress,
    sources: AHashSet<&'a Path>,
    namings: AHashMap<&'a Path, Naming>,
    discs: AHashMap<AlbumId, u32>,
    claimed: AHashMap<PathBuf, PathBuf>,
    going: AHashSet<PathBuf>,
    beside: AHashMap<PathBuf, Beside>,
    sheets: AHashMap<PathBuf, Vec<Claiming>>,
    emptying: AHashMap<PathBuf, PathBuf>,
    moves: Vec<Planned>,
    plan: Plan,
}

struct Claiming {
    sheet: PathBuf,
    files: Vec<PathBuf>,
}

#[derive(Default)]
struct Tied {
    sheets: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

impl<'a> Planner<'a> {
    fn over(rows: &'a [TrackToFile], layout: &'a Layout, progress: &'a OrganiseProgress) -> Self {
        let mut sources = AHashSet::with_capacity(rows.len());
        let mut namings: AHashMap<&Path, Naming> = AHashMap::new();
        let mut discs: AHashMap<AlbumId, u32> = AHashMap::new();
        for row in rows {
            sources.insert(row.path.as_path());
            namings
                .entry(row.root.as_path())
                .or_insert_with(|| Naming::of(&row.root));
            if let Some(album) = row.album_id {
                let counted = discs.entry(album).or_default();
                *counted = (*counted)
                    .max(row.discs)
                    .max(disc_of(row).map_or(0, NonZeroU32::get));
            }
        }

        Self {
            layout,
            progress,
            sources,
            namings,
            discs,
            claimed: AHashMap::new(),
            going: AHashSet::new(),
            beside: AHashMap::new(),
            sheets: AHashMap::new(),
            emptying: AHashMap::new(),
            moves: Vec::new(),
            plan: Plan::default(),
        }
    }

    fn file(&mut self, group: &[TrackToFile]) {
        let Some(first) = group.first() else {
            return;
        };
        let Some(rendered) = self.rendered(group) else {
            self.refuse(first, Refusal::Unidentified);
            return;
        };

        let destination = first.root.join(&rendered);
        if destination == first.path {
            self.plan.unchanged += 1;
            self.progress.unchanged.fetch_add(1, Ordering::Relaxed);
            return;
        }

        if rendered.components().count() < 2 {
            self.refuse(first, Refusal::Loose);
            return;
        }

        let waits_for = match self.in_the_way(&first.path, &destination) {
            Some(InTheWay::Stands(refusal)) => {
                self.refuse(first, refusal);
                return;
            }
            Some(InTheWay::MayGo(vacating)) => Some(vacating),
            None => None,
        };

        let sheets = match self.sheets_travelling(&first.path, &destination) {
            Ok(sheets) => sheets,
            Err(refusal) => {
                self.refuse(first, refusal);
                return;
            }
        };

        self.claimed.insert(destination.clone(), first.path.clone());
        self.going.insert(first.path.clone());
        for sheet in &sheets {
            self.claimed.insert(sheet.to.clone(), sheet.from.clone());
            self.going.insert(sheet.from.clone());
        }
        let mut sidecars = sheets;
        sidecars.extend(self.sidecars(&first.path, &destination));
        self.note_the_folder(first);

        self.moves.push(Planned {
            planned: Move {
                from: first.path.clone(),
                to: destination,
                rows: group.len() as u32,
                companions: Vec::new(),
                sidecars,
            },
            waits_for,
        });
    }

    fn tied_by_sheets(&mut self, file: &Path) -> Tied {
        let mut tied = Tied {
            sheets: Vec::new(),
            files: vec![file.to_path_buf()],
        };
        let mut walked = 0;
        while let Some(next) = tied.files.get(walked).cloned() {
            walked += 1;
            let Some(folder) = next.parent() else {
                continue;
            };
            let claimed: Vec<(PathBuf, Vec<PathBuf>)> = self
                .sheets_in(folder)
                .iter()
                .filter(|claiming| claiming.files.len() > 1)
                .filter(|claiming| claiming.files.contains(&next))
                .map(|claiming| (claiming.sheet.clone(), claiming.files.clone()))
                .collect();
            for (sheet, files) in claimed {
                if !tied.sheets.contains(&sheet) {
                    tied.sheets.push(sheet);
                }
                for named in files {
                    if !tied.files.contains(&named) {
                        tied.files.push(named);
                    }
                }
            }
        }

        tied
    }

    fn refuse_the_sheets(
        &mut self,
        group: &[&[TrackToFile]],
        tied: &Tied,
        owned: Option<(&Path, Refusal)>,
    ) {
        let sheet = tied.sheets.first().cloned().unwrap_or_default();
        for member in group {
            let Some(first) = member.first() else {
                continue;
            };
            let refusal = match &owned {
                Some((failed, refusal)) if *failed == first.path.as_path() => refusal.clone(),
                Some(_) | None => Refusal::SharesASheet {
                    sheet: sheet.clone(),
                },
            };
            self.refuse(first, refusal);
        }
    }

    fn file_together(&mut self, group: &[&[TrackToFile]], tied: &Tied) {
        let mut landing: Option<PathBuf> = None;
        let mut destinations = Vec::with_capacity(group.len());
        for member in group {
            let Some(first) = member.first() else {
                continue;
            };
            let Some(rendered) = self.rendered(member) else {
                self.refuse_the_sheets(group, tied, Some((&first.path, Refusal::Unidentified)));
                return;
            };
            if rendered.components().count() < 2 {
                self.refuse_the_sheets(group, tied, Some((&first.path, Refusal::Loose)));
                return;
            }

            let destination = first.root.join(&rendered);
            let folder = destination.parent().map(Path::to_path_buf);
            if landing.is_some() && landing != folder {
                self.refuse_the_sheets(group, tied, None);
                return;
            }
            landing = folder;
            destinations.push((first, member.len() as u32, destination));
        }
        let Some(landing) = landing else {
            return;
        };

        let moving: Vec<(&TrackToFile, u32, PathBuf)> = destinations
            .into_iter()
            .filter(|(first, _, destination)| *destination != first.path)
            .collect();
        let unchanged = (group.len() - moving.len()) as u64;
        self.plan.unchanged += unchanged;
        self.progress
            .unchanged
            .fetch_add(unchanged, Ordering::Relaxed);
        if moving.is_empty() {
            return;
        }

        for (first, _, destination) in &moving {
            if let Some(standing) = self.in_the_way(&first.path, destination) {
                let refusal = match standing {
                    InTheWay::Stands(refusal) => refusal,
                    InTheWay::MayGo(vacating) => Refusal::Collided { with: vacating },
                };
                self.refuse_the_sheets(group, tied, Some((&first.path, refusal)));
                return;
            }
        }

        let sheets: Vec<Sidecar> = tied
            .sheets
            .iter()
            .filter_map(|sheet| {
                Some(Sidecar {
                    from: sheet.clone(),
                    to: landing.join(sheet.file_name()?),
                })
            })
            .collect();
        for sheet in &sheets {
            if self.in_the_way(&sheet.from, &sheet.to).is_some() {
                let with = sheet.to.clone();
                self.refuse_the_sheets(group, tied, None);
                tracing::debug!(
                    sheet = %sheet.from.display(),
                    standing = %with.display(),
                    "a sheet naming several files has another file where it would land"
                );
                return;
            }
        }

        for (first, _, destination) in &moving {
            self.claimed.insert(destination.clone(), first.path.clone());
            self.going.insert(first.path.clone());
        }
        for sheet in &sheets {
            self.claimed.insert(sheet.to.clone(), sheet.from.clone());
            self.going.insert(sheet.from.clone());
        }
        let mut sidecars = sheets;
        for (first, _, destination) in &moving {
            sidecars.extend(self.sidecars(&first.path, destination));
            self.note_the_folder(first);
        }

        let mut members = moving.into_iter();
        let Some((first, rows, to)) = members.next() else {
            return;
        };
        self.moves.push(Planned {
            planned: Move {
                from: first.path.clone(),
                to,
                rows,
                companions: members
                    .map(|(first, rows, to)| Companion {
                        from: first.path.clone(),
                        to,
                        rows,
                    })
                    .collect(),
                sidecars,
            },
            waits_for: None,
        });
    }

    fn naming(&self, row: &TrackToFile) -> Naming {
        self.namings
            .get(row.root.as_path())
            .copied()
            .unwrap_or_default()
    }

    fn rendered(&self, group: &[TrackToFile]) -> Option<PathBuf> {
        let first = group.first()?;
        if let [only] = group {
            return self
                .layout
                .render(&self.named(only, extension_of(only)), self.naming(only));
        }

        let folder = self.one_folder(group)?;
        Some(folder.join(first.path.file_name()?))
    }

    fn one_folder(&self, group: &[TrackToFile]) -> Option<PathBuf> {
        let mut agreed: Option<PathBuf> = None;
        for row in group {
            let written = self
                .layout
                .render(&self.named(row, extension_of(row)), self.naming(row))?;
            let under = written.parent()?.to_path_buf();
            match agreed {
                None => agreed = Some(under),
                Some(ref held) if *held == under => {}
                Some(ref held) => {
                    tracing::debug!(
                        track = %row.id,
                        path = %row.path.display(),
                        one = %held.display(),
                        another = %under.display(),
                        "the rows cut out of one file name two folders"
                    );
                    return None;
                }
            }
        }

        agreed
    }

    fn named<'n>(&self, row: &'n TrackToFile, extension: Option<&'n str>) -> Named<'n> {
        Named {
            album_artist: row.album_artist.as_deref(),
            artist: row.artist.as_deref(),
            album: row.album.as_deref(),
            title: &row.title,
            year: row.year,
            disc: disc_of(row),
            discs: self.discs_of(row),
            track: row.track,
            extension,
        }
    }

    fn discs_of(&self, row: &TrackToFile) -> u32 {
        row.album_id
            .and_then(|album| self.discs.get(&album).copied())
            .unwrap_or(row.discs)
    }

    fn in_the_way(&self, from: &Path, to: &Path) -> Option<InTheWay> {
        if let Some(winner) = self.claimed.get(to) {
            return Some(InTheWay::Stands(Refusal::Collided {
                with: winner.clone(),
            }));
        }

        let held = fs::symlink_metadata(to).ok()?;
        let Ok(standing) = fs::symlink_metadata(from) else {
            return Some(InTheWay::Stands(Refusal::SourceGone));
        };
        if standing.dev() == held.dev() && standing.ino() == held.ino() {
            return None;
        }
        if already_copied(from, to) {
            return None;
        }
        if self.sources.contains(to) {
            return Some(InTheWay::MayGo(to.to_path_buf()));
        }

        Some(InTheWay::Stands(Refusal::Collided {
            with: to.to_path_buf(),
        }))
    }

    fn sheets_travelling(
        &mut self,
        from: &Path,
        to: &Path,
    ) -> std::result::Result<Vec<Sidecar>, Refusal> {
        let (Some(folder), Some(landing)) = (from.parent(), to.parent()) else {
            return Ok(Vec::new());
        };
        let stem = from.file_stem().and_then(OsStr::to_str);
        let renamed = to.file_stem().and_then(OsStr::to_str);

        let mut travelling = Vec::new();
        for claiming in self.sheets_in(folder) {
            if !claiming.files.iter().any(|file| file == from) {
                continue;
            }

            let destination = match (
                stem.and_then(|stem| trailing(&claiming.sheet, stem)),
                renamed,
            ) {
                (Some(suffix), Some(renamed)) => landing.join(format!("{renamed}{suffix}")),
                _ => landing.join(claiming.sheet.file_name().unwrap_or_default()),
            };
            travelling.push(Sidecar {
                from: claiming.sheet.clone(),
                to: destination,
            });
        }

        for sheet in &travelling {
            if travelling
                .iter()
                .filter(|other| other.to == sheet.to)
                .count()
                > 1
            {
                return Err(Refusal::Collided {
                    with: sheet.to.clone(),
                });
            }
            if let Some(standing) = self.in_the_way(&sheet.from, &sheet.to) {
                let with = match standing {
                    InTheWay::Stands(Refusal::Collided { with }) => with,
                    InTheWay::Stands(_) | InTheWay::MayGo(_) => sheet.to.clone(),
                };
                return Err(Refusal::Collided { with });
            }
        }
        Ok(travelling)
    }

    fn sheets_in(&mut self, folder: &Path) -> &[Claiming] {
        if !self.sheets.contains_key(folder) {
            let read: Vec<Claiming> = listing(&mut self.beside, folder)
                .files
                .iter()
                .filter(|file| scan::is_a_sheet(file))
                .filter_map(|sheet| {
                    let read = scan::read_sheet(sheet)?;
                    let files = read
                        .files
                        .iter()
                        .filter_map(|cut| scan::beside(sheet, &cut.named))
                        .filter(|file| file.exists())
                        .collect();
                    Some(Claiming {
                        sheet: sheet.clone(),
                        files,
                    })
                })
                .collect();
            self.sheets.insert(folder.to_path_buf(), read);
        }
        self.sheets.get(folder).map_or(&[], Vec::as_slice)
    }

    fn sidecars(&mut self, from: &Path, to: &Path) -> Vec<Sidecar> {
        let (Some(folder), Some(stem), Some(landing), Some(renamed)) = (
            from.parent(),
            from.file_stem().and_then(OsStr::to_str),
            to.parent(),
            to.file_stem().and_then(OsStr::to_str),
        ) else {
            return Vec::new();
        };

        let claiming_elsewhere: AHashSet<PathBuf> = self
            .sheets_in(folder)
            .iter()
            .filter(|claiming| !claiming.files.is_empty())
            .map(|claiming| claiming.sheet.clone())
            .collect();
        let candidates: Vec<PathBuf> = listing(&mut self.beside, folder)
            .files
            .iter()
            .filter(|file| !self.sources.contains(file.as_path()))
            .filter(|file| !claiming_elsewhere.contains(file.as_path()))
            .cloned()
            .collect();

        let mut sidecars = Vec::new();
        for file in candidates {
            let Some(suffix) = trailing(&file, stem) else {
                continue;
            };
            if self.going.contains(&file) || names_audio(&file) {
                continue;
            }

            let destination = landing.join(format!("{renamed}{suffix}"));
            if let Some(standing) = self.in_the_way(&file, &destination) {
                tracing::debug!(
                    sidecar = %file.display(),
                    destination = %destination.display(),
                    ?standing,
                    "a sidecar was left where it stands"
                );
                continue;
            }

            self.claimed.insert(destination.clone(), file.clone());
            self.going.insert(file.clone());
            sidecars.push(Sidecar {
                from: file,
                to: destination,
            });
        }

        sidecars
    }

    fn note_the_folder(&mut self, row: &TrackToFile) {
        let Some(folder) = row.path.parent() else {
            return;
        };
        if folder == row.root {
            return;
        }

        self.emptying.insert(folder.to_path_buf(), row.root.clone());
    }

    fn refuse(&mut self, row: &TrackToFile, refusal: Refusal) {
        tracing::debug!(
            track = %row.id,
            path = %row.path.display(),
            ?refusal,
            "a track was left where it stands"
        );
        self.stands(row.path.clone(), refusal);
    }

    fn stands(&mut self, from: PathBuf, refusal: Refusal) {
        counted(self.progress, &refusal);
        self.plan.refused.push(Refused { from, refusal });
    }

    fn settle(mut self) -> Plan {
        self.order_the_chains();

        let mut folders: Vec<PathBuf> = self
            .emptying
            .iter()
            .filter(|(folder, root)| folder.as_path() != root.as_path())
            .filter(|(folder, _)| self.empties(folder))
            .map(|(folder, _)| folder.clone())
            .collect();

        folders.sort_by(|one, other| deepest_first(one, other));
        self.plan.folders = folders;
        self.plan
    }

    fn order_the_chains(&mut self) {
        let asked = mem::take(&mut self.moves);
        let mut waits = Vec::with_capacity(asked.len());
        let mut state = vec![Step::Unwalked; asked.len()];
        {
            let mut by_source: AHashMap<&Path, usize> = AHashMap::with_capacity(asked.len());
            for (at, held) in asked.iter().enumerate() {
                for (from, _) in held.planned.files() {
                    by_source.insert(from, at);
                }
            }

            for (at, held) in asked.iter().enumerate() {
                let vacating = held
                    .waits_for
                    .as_deref()
                    .map(|standing| by_source.get(standing).copied());
                match vacating {
                    None => waits.push(None),
                    Some(Some(mover)) => waits.push(Some(mover)),
                    Some(None) => {
                        waits.push(None);
                        state[at] = Step::Doomed;
                    }
                }
            }
        }

        let order = walked(&waits, &mut state);
        let mut held: Vec<Option<Planned>> = asked.into_iter().map(Some).collect();
        let mut moves = Vec::with_capacity(order.len());
        for at in order {
            let taken = held[at].take().expect("a move is ordered once");
            moves.push(taken.planned);
        }
        self.plan.moves = moves;

        for left in held.into_iter().flatten() {
            for (from, _) in left.planned.files() {
                self.going.remove(from);
            }
            for sidecar in &left.planned.sidecars {
                self.going.remove(&sidecar.from);
            }

            let with = left
                .waits_for
                .expect("a move left out of the order is one waiting on a file that stays");
            tracing::debug!(
                path = %left.planned.from.display(),
                standing = %with.display(),
                "a move waiting on a file that never goes was left where it stands"
            );
            self.stands(left.planned.from, Refusal::Collided { with });
        }
    }

    fn empties(&self, folder: &Path) -> bool {
        self.beside.get(folder).is_some_and(|beside| {
            beside.nothing_but_files && beside.files.iter().all(|file| self.going.contains(file))
        })
    }
}

fn walked(waits: &[Option<usize>], state: &mut [Step]) -> Vec<usize> {
    let mut order = Vec::with_capacity(waits.len());
    for start in 0..waits.len() {
        if state[start] != Step::Unwalked {
            continue;
        }

        let mut chain = Vec::new();
        let mut at = start;
        let outcome = loop {
            match state[at] {
                Step::Unwalked => {
                    state[at] = Step::Walking;
                    chain.push(at);
                    match waits[at] {
                        Some(vacating) => at = vacating,
                        None => break Step::Ordered,
                    }
                }
                Step::Ordered => break Step::Ordered,
                Step::Walking | Step::Doomed => break Step::Doomed,
            }
        };

        for step in chain.into_iter().rev() {
            state[step] = outcome;
            if outcome == Step::Ordered {
                order.push(step);
            }
        }
    }

    order
}

fn listing<'b>(beside: &'b mut AHashMap<PathBuf, Beside>, folder: &Path) -> &'b Beside {
    if !beside.contains_key(folder) {
        beside.insert(folder.to_path_buf(), Beside::of(folder));
    }

    beside
        .get(folder)
        .expect("the folder was listed a moment ago")
}

fn disc_of(row: &TrackToFile) -> Option<NonZeroU32> {
    row.disc.or_else(|| {
        row.path
            .parent()
            .and_then(Path::file_name)
            .and_then(OsStr::to_str)
            .and_then(scan::disc_in_folder)
    })
}

fn extension_of(row: &TrackToFile) -> Option<&str> {
    row.path.extension().and_then(OsStr::to_str)
}

fn trailing<'f>(file: &'f Path, stem: &str) -> Option<&'f str> {
    let name = file.file_name()?.to_str()?;
    let rest = name.strip_prefix(stem)?;
    rest.starts_with(EXTENSION_SEPARATOR).then_some(rest)
}

fn counted(progress: &OrganiseProgress, refusal: &Refusal) {
    match refusal {
        Refusal::Unidentified | Refusal::Loose => {
            progress.unidentified.fetch_add(1, Ordering::Relaxed)
        }
        Refusal::Collided { .. } | Refusal::SharesASheet { .. } => {
            progress.collided.fetch_add(1, Ordering::Relaxed)
        }
        Refusal::SourceGone | Refusal::Unmoved { .. } => {
            progress.failed.fetch_add(1, Ordering::Relaxed)
        }
    };
}

fn deepest_first(one: &Path, other: &Path) -> Ranking {
    other
        .components()
        .count()
        .cmp(&one.components().count())
        .then_with(|| one.cmp(other))
}

fn names_audio(file: &Path) -> bool {
    file.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            scan::AUDIO_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Landing {
    Renamed,
    Copied,
}

struct Renamed {
    from: PathBuf,
    to: PathBuf,
    how: Landing,
}

struct Applied {
    landed: Vec<Move>,
    refused: Vec<Refused>,
}

enum Pruning {
    Went,
    WasNotThere,
    Holds,
}

fn apply(
    library: &Library,
    plan: &mut Plan,
    roots: &AHashSet<PathBuf>,
    progress: &OrganiseProgress,
) {
    let asked = mem::take(&mut plan.moves);
    let mut landed = Vec::with_capacity(asked.len());

    for batch in asked.chunks(MOVES_PER_BATCH) {
        if progress.is_cancelled() {
            break;
        }

        let applied = batch_moved(library, batch, progress);
        landed.extend(applied.landed);
        plan.refused.extend(applied.refused);
    }

    if !landed.is_empty()
        && let Err(error) = library.albums_re_keyed(&landed)
    {
        tracing::warn!(
            %error,
            "an album grouped by its folder still names the folder it came out of"
        );
    }

    prune(&landed, roots, progress);
    plan.moves = landed;
}

fn batch_moved(library: &Library, batch: &[Move], progress: &OrganiseProgress) -> Applied {
    let mut made: Vec<PathBuf> = Vec::new();
    let applied = batch_landing(library, batch, progress, &mut made);
    take_back_the_empty(&made);
    applied
}

fn batch_landing(
    library: &Library,
    batch: &[Move],
    progress: &OrganiseProgress,
    made: &mut Vec<PathBuf>,
) -> Applied {
    let mut done: Vec<Renamed> = Vec::new();
    let mut landed: Vec<Move> = Vec::new();
    let mut refused: Vec<Refused> = Vec::new();

    for planned in batch {
        if let Some(refusal) = standing(planned) {
            refused.push(refused_now(progress, planned, refusal));
            continue;
        }

        let mark = done.len();
        match renamed_onto(planned, &mut done, made) {
            Ok(()) => landed.push(planned.clone()),
            Err(error) => {
                tracing::warn!(%error, "a track could not be moved and was put back where it stood");
                put_back(&done[mark..]);
                done.truncate(mark);
                let refusal = Refusal::Unmoved {
                    kind: moved_how(&error),
                };
                refused.push(refused_now(progress, planned, refusal));
            }
        }
    }

    if landed.is_empty() {
        return Applied { landed, refused };
    }

    settle(&landed);
    if let Err(error) = library.files_moved(&landed) {
        tracing::warn!(
            %error,
            "the catalog was not rewritten, so the files it names were put back"
        );
        put_back(&done);
        progress
            .failed
            .fetch_add(landed.len() as u64, Ordering::Relaxed);
        return Applied {
            landed: Vec::new(),
            refused,
        };
    }

    left_behind(&done);
    sheets_follow_their_audio(&landed);
    let files = landed
        .iter()
        .map(|planned| planned.files().count())
        .sum::<usize>();
    progress.moved.fetch_add(files as u64, Ordering::Relaxed);
    Applied { landed, refused }
}

fn moved_how(error: &Error) -> io::ErrorKind {
    match error {
        Error::Move { source, .. } => source.kind(),
        _ => io::ErrorKind::Other,
    }
}

fn standing(planned: &Move) -> Option<Refusal> {
    planned.files().find_map(|(from, to)| {
        if fs::symlink_metadata(from).is_err() {
            return Some(Refusal::SourceGone);
        }

        (fs::symlink_metadata(to).is_ok() && !already_copied(from, to)).then(|| Refusal::Collided {
            with: to.to_path_buf(),
        })
    })
}

fn refused_now(progress: &OrganiseProgress, planned: &Move, refusal: Refusal) -> Refused {
    counted(progress, &refusal);
    tracing::debug!(
        path = %planned.from.display(),
        ?refusal,
        "a track was left where it stands"
    );

    Refused {
        from: planned.from.clone(),
        refusal,
    }
}

fn renamed_onto(planned: &Move, done: &mut Vec<Renamed>, made: &mut Vec<PathBuf>) -> Result<()> {
    if let Some(folder) = planned.to.parent() {
        let fresh = not_there_yet(folder);
        fs::create_dir_all(folder).map_err(|source| Error::Move {
            op: MoveOp::MakeFolder,
            path: folder.to_path_buf(),
            source,
        })?;
        made.extend(fresh);
    }

    for (from, to) in planned.files() {
        done.push(landed_onto(from, to)?);
    }
    for sidecar in &planned.sidecars {
        done.push(landed_onto(&sidecar.from, &sidecar.to)?);
    }

    Ok(())
}

fn landed_onto(from: &Path, to: &Path) -> Result<Renamed> {
    let how = match fs::rename(from, to) {
        Ok(()) => Landing::Renamed,
        Err(source) if source.kind() == io::ErrorKind::CrossesDevices => {
            if !already_copied(from, to) {
                copying(from, to)?;
            }
            Landing::Copied
        }
        Err(source) => {
            return Err(Error::Move {
                op: MoveOp::Rename,
                path: from.to_path_buf(),
                source,
            });
        }
    };

    Ok(Renamed {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        how,
    })
}

fn copying(from: &Path, to: &Path) -> Result<()> {
    let mut staging = to.as_os_str().to_owned();
    staging.push(STAGED);
    let staged = PathBuf::from(staging);

    let landed = copied_whole(from, &staged).and_then(|()| {
        fs::rename(&staged, to).map_err(|source| Error::Move {
            op: MoveOp::Copy,
            path: to.to_path_buf(),
            source,
        })
    });
    if landed.is_err() {
        let _ = fs::remove_file(&staged);
    }
    landed
}

fn already_copied(from: &Path, to: &Path) -> bool {
    let (Ok(source), Ok(copy)) = (fs::symlink_metadata(from), fs::symlink_metadata(to)) else {
        return false;
    };
    let alike = source.is_file()
        && copy.is_file()
        && source.dev() != copy.dev()
        && source.len() == copy.len()
        && source
            .modified()
            .ok()
            .is_some_and(|stamp| copy.modified().ok() == Some(stamp));
    alike && same_bytes(from, to).unwrap_or(false)
}

fn same_bytes(one: &Path, other: &Path) -> io::Result<bool> {
    let mut one = fs::File::open(one)?;
    let mut other = fs::File::open(other)?;
    let mut ours = vec![0_u8; COMPARED_AT_ONCE];
    let mut theirs = vec![0_u8; COMPARED_AT_ONCE];
    loop {
        let read = one.read(&mut ours)?;
        if read == 0 {
            return Ok(other.read(&mut theirs)? == 0);
        }
        other.read_exact(&mut theirs[..read])?;
        if ours[..read] != theirs[..read] {
            return Ok(false);
        }
    }
}

fn copied_whole(from: &Path, to: &Path) -> Result<()> {
    let held = fs::metadata(from).map_err(|source| Error::Move {
        op: MoveOp::Copy,
        path: from.to_path_buf(),
        source,
    })?;
    fs::copy(from, to).map_err(|source| Error::Move {
        op: MoveOp::Copy,
        path: to.to_path_buf(),
        source,
    })?;

    let written = fs::File::options()
        .write(true)
        .open(to)
        .map_err(|source| Error::Move {
            op: MoveOp::Copy,
            path: to.to_path_buf(),
            source,
        })?;
    if let Ok(stamp) = held.modified()
        && let Err(source) = written.set_modified(stamp)
    {
        let error = Error::Move {
            op: MoveOp::Stamp,
            path: to.to_path_buf(),
            source,
        };
        tracing::debug!(%error, "a copied file carries the time it was copied at");
    }

    written.sync_all().map_err(|source| Error::Move {
        op: MoveOp::Copy,
        path: to.to_path_buf(),
        source,
    })
}

fn left_behind(done: &[Renamed]) {
    for step in done.iter().filter(|step| step.how == Landing::Copied) {
        if let Err(source) = fs::remove_file(&step.from) {
            let error = Error::Move {
                op: MoveOp::Discard,
                path: step.from.clone(),
                source,
            };
            tracing::warn!(
                %error,
                "a file copied onto another filesystem is still standing where it was"
            );
        }
    }
}

fn sheets_follow_their_audio(landed: &[Move]) {
    for planned in landed {
        for (from, to) in planned.files() {
            let (Some(named), Some(naming)) = (file_name_of(from), file_name_of(to)) else {
                continue;
            };
            if named == naming {
                continue;
            }

            for sidecar in planned
                .sidecars
                .iter()
                .filter(|sidecar| names_a_sheet(&sidecar.to))
            {
                sheet_renamed(&sidecar.to, named, naming);
            }
        }
    }
}

fn sheet_renamed(sheet: &Path, named: &str, naming: &str) {
    let held = match fs::read(sheet) {
        Ok(held) => held,
        Err(source) => {
            let error = Error::Move {
                op: MoveOp::Rewrite,
                path: sheet.to_path_buf(),
                source,
            };
            tracing::warn!(%error, "a sheet beside a renamed file still names the name it had");
            return;
        }
    };

    let Some(written) = resonate_codec::renamed_cue(&held, named, naming) else {
        tracing::debug!(
            sheet = %sheet.display(),
            named,
            "a sheet that does not name its audio exactly once is left as it was"
        );
        return;
    };

    if let Err(error) = staged_over(sheet, &written) {
        tracing::warn!(%error, "a sheet beside a renamed file still names the name it had");
    }
}

fn staged_over(sheet: &Path, written: &[u8]) -> Result<()> {
    let mut staging = sheet.as_os_str().to_owned();
    staging.push(STAGED);
    let staged = PathBuf::from(staging);

    let landing = |source| Error::Move {
        op: MoveOp::Rewrite,
        path: staged.clone(),
        source,
    };
    fs::write(&staged, written).map_err(landing)?;
    fs::File::open(&staged)
        .and_then(|opened| opened.sync_all())
        .map_err(landing)?;

    fs::rename(&staged, sheet).map_err(|source| {
        let _ = fs::remove_file(&staged);
        Error::Move {
            op: MoveOp::Rewrite,
            path: sheet.to_path_buf(),
            source,
        }
    })
}

fn file_name_of(path: &Path) -> Option<&str> {
    path.file_name().and_then(OsStr::to_str)
}

fn names_a_sheet(file: &Path) -> bool {
    file.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case(SHEET_EXTENSION))
}

fn not_there_yet(folder: &Path) -> Vec<PathBuf> {
    let mut fresh = Vec::new();
    let mut at = Some(folder);
    while let Some(step) = at.filter(|step| !step.as_os_str().is_empty() && !step.exists()) {
        fresh.push(step.to_path_buf());
        at = step.parent();
    }

    fresh
}

fn take_back_the_empty(made: &[PathBuf]) {
    let mut folders: Vec<&Path> = made.iter().map(PathBuf::as_path).collect();
    folders.sort_by(|one, other| deepest_first(one, other));
    folders.dedup();

    for folder in folders {
        let _ = taken_away(folder);
    }
}

fn put_back(done: &[Renamed]) {
    for step in done.iter().rev() {
        let (op, taken) = match step.how {
            Landing::Renamed => (MoveOp::Rename, fs::rename(&step.to, &step.from)),
            Landing::Copied => (MoveOp::Discard, fs::remove_file(&step.to)),
        };
        if let Err(source) = taken {
            let error = Error::Move {
                op,
                path: step.to.clone(),
                source,
            };
            tracing::error!(
                %error,
                stood = %step.from.display(),
                "a file this run moved is not back where it stood"
            );
        }
    }
}

fn settle(landed: &[Move]) {
    let mut folders: AHashSet<&Path> = AHashSet::new();
    for (from, to) in landed.iter().flat_map(Move::files) {
        folders.extend(from.parent());
        folders.extend(to.parent());
    }

    for folder in folders {
        if let Err(source) = fs::File::open(folder).and_then(|opened| opened.sync_all()) {
            let error = Error::Move {
                op: MoveOp::Settle,
                path: folder.to_path_buf(),
                source,
            };
            tracing::warn!(%error, "a folder this run wrote is not settled onto the disk");
        }
    }
}

pub(crate) fn files_moved(tx: &Transaction<'_>, landed: &[Move]) -> Result<()> {
    let mut stale_lyrics = prepared(tx, "DELETE FROM lyrics_kept WHERE path = ?1")?;
    let mut stale_tracks = prepared(tx, "DELETE FROM tracks WHERE path = ?1")?;
    let mut lyrics = prepared(tx, "UPDATE lyrics_kept SET path = ?2 WHERE path = ?1")?;
    let mut tracks = prepared(tx, "UPDATE tracks SET path = ?2 WHERE path = ?1")?;
    let mut entries = prepared(tx, "UPDATE playlist_entries SET path = ?2 WHERE path = ?1")?;
    let mut queued = prepared(tx, "UPDATE resume_rows SET uri = ?2 WHERE uri = ?1")?;

    for (from_path, to_path) in landed.iter().flat_map(Move::files) {
        let from = store::path_text(from_path)?;
        let to = store::path_text(to_path)?;

        for statement in [&mut stale_lyrics, &mut stale_tracks] {
            statement
                .execute(params![to])
                .map_err(|source| Error::store(StoreOp::Delete, source))?;
        }
        for statement in [&mut lyrics, &mut tracks, &mut entries] {
            statement
                .execute(params![from, to])
                .map_err(|source| Error::store(StoreOp::Update, source))?;
        }

        queued
            .execute(params![
                MediaLocation::local(from_path).to_uri(),
                MediaLocation::local(to_path).to_uri()
            ])
            .map_err(|source| Error::store(StoreOp::Update, source))?;
    }

    Ok(())
}

fn prepared<'t>(tx: &'t Transaction<'_>, sql: &str) -> Result<Statement<'t>> {
    tx.prepare(sql)
        .map_err(|source| Error::store(StoreOp::Prepare, source))
}

struct Sleeved {
    id: i64,
    title: String,
    key: String,
}

pub(crate) fn re_key_the_sleeves(tx: &Transaction<'_>, landed: &[Move]) -> Result<()> {
    for album in sleeved_albums(tx, landed)? {
        let Some(folder) = the_one_folder_holding(tx, album.id)? else {
            tracing::debug!(
                album = album.id,
                "an album whose tracks do not share one folder keeps the key it had"
            );
            continue;
        };

        let named = store::sleeve_key(&album.title, &folder);
        if named == album.key {
            continue;
        }
        if already_held(tx, &named, album.id)? {
            tracing::debug!(
                album = album.id,
                folder = %folder.display(),
                "another album is already keyed by the folder this one moved into"
            );
            continue;
        }

        store::re_key_album(tx, &album.key, &named)?;
    }

    Ok(())
}

fn sleeved_albums(tx: &Transaction<'_>, landed: &[Move]) -> Result<Vec<Sleeved>> {
    let mut naming = prepared(
        tx,
        "SELECT DISTINCT album_id FROM tracks WHERE path = ?1 AND album_id IS NOT NULL",
    )?;

    let mut moved: Vec<i64> = Vec::new();
    let mut counted: AHashSet<i64> = AHashSet::new();
    for (_, to) in landed.iter().flat_map(Move::files) {
        let to = store::path_text(to)?;
        let found = naming
            .query_map(params![to], |row| row.get::<_, i64>(0))
            .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
            .map_err(|source| Error::store(StoreOp::Query, source))?;

        for album in found {
            if counted.insert(album) {
                moved.push(album);
            }
        }
    }

    let mut held = prepared(tx, "SELECT title FROM albums WHERE id = ?1")?;
    let mut sleeved = Vec::new();
    for id in moved {
        let title = held
            .query_row(params![id], |row| row.get::<_, String>(0))
            .optional()
            .map_err(|source| Error::store(StoreOp::Query, source))?;

        let keys = store::keys_of(tx, id)?;
        let [key] = keys.as_slice() else {
            tracing::debug!(
                album = id,
                keys = keys.len(),
                "an album more than one grouping names keeps the keys it had"
            );
            continue;
        };

        if let Some(title) = title
            && store::is_keyed_by_its_folder(key)
        {
            sleeved.push(Sleeved {
                id,
                title,
                key: key.clone(),
            });
        }
    }

    Ok(sleeved)
}

fn the_one_folder_holding(tx: &Transaction<'_>, album: i64) -> Result<Option<PathBuf>> {
    let mut statement = prepared(
        tx,
        "SELECT tracks.path, roots.path FROM tracks
           JOIN roots ON roots.id = tracks.root_id
          WHERE tracks.album_id = ?1",
    )?;

    let rows = statement
        .query_map(params![album], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    let mut agreed: Option<PathBuf> = None;
    for (path, root) in rows {
        let Some(sleeve) = scan::sleeve(Path::new(&path), Path::new(&root)) else {
            return Ok(None);
        };
        match agreed {
            None => agreed = Some(sleeve),
            Some(ref held) if *held == sleeve => {}
            Some(_) => return Ok(None),
        }
    }

    Ok(agreed)
}

fn already_held(tx: &Transaction<'_>, key: &str, album: i64) -> Result<bool> {
    Ok(store::album_keyed(tx, key)?.is_some_and(|other| other != album))
}

fn prune(landed: &[Move], roots: &AHashSet<PathBuf>, progress: &OrganiseProgress) {
    let mut folders: Vec<&Path> = Vec::new();
    let mut named: AHashSet<&Path> = AHashSet::new();
    for (from, _) in landed.iter().flat_map(Move::files) {
        if let Some(folder) = from.parent()
            && named.insert(folder)
        {
            folders.push(folder);
        }
    }

    folders.sort_by(|one, other| deepest_first(one, other));
    for folder in folders {
        climb_out_of(folder, roots, progress);
    }
}

fn climb_out_of(folder: &Path, roots: &AHashSet<PathBuf>, progress: &OrganiseProgress) {
    let mut at = folder;
    while under_a_root(at, roots) {
        match taken_away(at) {
            Pruning::Went => {
                progress.pruned.fetch_add(1, Ordering::Relaxed);
            }
            Pruning::WasNotThere => {}
            Pruning::Holds => return,
        }

        let Some(up) = at.parent() else {
            return;
        };
        at = up;
    }
}

fn under_a_root(folder: &Path, roots: &AHashSet<PathBuf>) -> bool {
    !roots.contains(folder) && roots.iter().any(|root| folder.starts_with(root))
}

fn taken_away(folder: &Path) -> Pruning {
    match fs::remove_dir(folder) {
        Ok(()) => Pruning::Went,
        Err(source) if source.kind() == io::ErrorKind::NotFound => Pruning::WasNotThere,
        Err(source) if source.kind() == io::ErrorKind::DirectoryNotEmpty => Pruning::Holds,
        Err(source) => {
            let error = Error::Move {
                op: MoveOp::Prune,
                path: folder.to_path_buf(),
                source,
            };
            tracing::warn!(%error, "a folder this run emptied is still there");
            Pruning::Holds
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, slice,
        sync::atomic::{AtomicU64, Ordering as Counting},
    };

    use super::*;

    const NOWHERE: &str = "/nowhere/resonate";

    fn comfortably_numb() -> Named<'static> {
        Named {
            album_artist: Some("Pink Floyd"),
            artist: Some("Pink Floyd"),
            album: Some("The Wall"),
            title: "Comfortably Numb",
            year: Some(1979),
            disc: NonZeroU32::new(2),
            discs: 2,
            track: Some(6),
            extension: Some("flac"),
        }
    }

    fn read(template: &str) -> Layout {
        Layout::read(template).expect("a layout this build writes is a layout it reads")
    }

    fn fault(template: &str) -> (usize, LayoutFault) {
        match Layout::read(template) {
            Err(Error::LayoutSyntax { at, fault }) => (at, fault),
            otherwise => panic!("{template:?} was read as {otherwise:?}"),
        }
    }

    fn rendered(layout: &Layout, named: &Named<'_>) -> String {
        layout
            .render(named, Naming::Anything)
            .expect("this layout names a destination for this track")
            .to_str()
            .expect("a rendered path is the text it was rendered from")
            .to_owned()
    }

    #[test]
    fn a_root_on_a_windows_volume_is_named_in_what_that_volume_takes() {
        let table = "/dev/nvme0n1p2 / ext4 rw,relatime 0 0\n\
                     /dev/sdb1 /run/media/me/My\\040Stick vfat rw 0 0\n\
                     /dev/sdc1 /mnt/Shared ntfs3 rw 0 0\n\
                     /dev/sdc2 /mnt/Shared/inner btrfs rw 0 0\n";

        assert_eq!(
            Naming::in_table(table, Path::new("/run/media/me/My Stick/Music")),
            Naming::Portable
        );
        assert_eq!(
            Naming::in_table(table, Path::new("/mnt/Shared/Media")),
            Naming::Portable
        );
        assert_eq!(
            Naming::in_table(table, Path::new("/mnt/Shared/inner/Music")),
            Naming::Anything,
            "a volume mounted inside a Windows one was named by the outer one"
        );
        assert_eq!(
            Naming::in_table(table, Path::new("/home/me/Music")),
            Naming::Anything
        );
    }

    #[test]
    fn a_portable_name_writes_what_a_windows_volume_refuses_as_a_dash() {
        let title = "What's Going On? <Live: Take 2> *\\|\"";

        assert_eq!(
            as_one_component(title, COMPONENT_BYTES, Naming::Portable),
            "What's Going On- -Live- Take 2- ----"
        );
        assert_eq!(
            as_one_component(title, COMPONENT_BYTES, Naming::Anything),
            title
        );
    }

    #[test]
    fn a_layout_is_segments_of_literals_and_fields_and_a_field_it_does_not_know_names_what_was_written()
     {
        let layout = read("{albumartist}/{disc}{track} {title}");

        assert_eq!(layout.segments.len(), 2);
        assert_eq!(
            layout.segments[0].pieces,
            vec![Piece::Named(Field::AlbumArtist)]
        );
        assert_eq!(
            layout.segments[1].pieces,
            vec![
                Piece::Named(Field::Disc),
                Piece::Named(Field::Track),
                Piece::Literal(" ".into()),
                Piece::Named(Field::Title),
            ]
        );

        let Err(Error::UnknownLayoutField { field }) = Layout::read("{album}/{genre}") else {
            panic!("a field no track carries was read as one it does");
        };
        assert_eq!(field.as_str(), "genre");
    }

    #[test]
    fn a_layout_that_would_leave_the_root_is_refused_at_the_moment_it_is_read() {
        assert_eq!(fault(""), (0, LayoutFault::Empty));
        assert_eq!(fault("/{album}"), (0, LayoutFault::EmptySegment));
        assert_eq!(fault("{album}/"), (8, LayoutFault::EmptySegment));
        assert_eq!(fault("{album}//{title}"), (8, LayoutFault::EmptySegment));

        assert!(matches!(
            Layout::read("../{album}"),
            Err(Error::LayoutEscapes { segment: 0 })
        ));
        assert!(matches!(
            Layout::read("{album}/."),
            Err(Error::LayoutEscapes { segment: 1 })
        ));
        assert!(matches!(
            Layout::read("{album}/.."),
            Err(Error::LayoutEscapes { segment: 1 })
        ));

        let layout = read("{albumartist}/{title}");
        let mut named = comfortably_numb();
        named.album_artist = Some("/etc");
        named.title = "../../passwd";
        assert_eq!(rendered(&layout, &named), "-etc/..-..-passwd.flac");
    }

    #[test]
    fn a_brace_is_written_by_doubling_it_and_an_unclosed_one_is_a_fault_at_its_own_offset() {
        let layout = read("{{{album}}}");
        assert_eq!(
            layout.segments[0].pieces,
            vec![
                Piece::Literal("{".into()),
                Piece::Named(Field::Album),
                Piece::Literal("}".into()),
            ]
        );
        assert_eq!(rendered(&layout, &comfortably_numb()), "{The Wall}.flac");

        assert_eq!(fault("{album}/{title"), (8, LayoutFault::Unclosed));
        assert_eq!(fault("{album}/a}b"), (9, LayoutFault::Unopened));
        assert_eq!(fault("{album}/{}"), (8, LayoutFault::EmptyField));
    }

    #[test]
    fn the_default_layout_reads_back_as_the_one_this_build_ships() {
        assert_eq!(Layout::default(), read(DEFAULT_LAYOUT));
        assert_eq!(
            rendered(&Layout::default(), &comfortably_numb()),
            "Pink Floyd/The Wall/2-06 Comfortably Numb.flac"
        );
    }

    #[test]
    fn a_layout_writes_itself_back_as_the_template_it_was_read_from() {
        for template in [
            DEFAULT_LAYOUT,
            "{artist}/{year} {album}/{track} {title}",
            "{albumartist}/{album}/{disc}{track} {title}.{ext}",
            "{{braced}}/{album}/{title}",
            "one/two/{title}",
        ] {
            assert_eq!(read(template).to_string(), template);
            assert_eq!(read(&read(template).to_string()), read(template));
        }
    }

    #[test]
    fn every_field_the_enum_names_is_a_field_the_reader_knows() {
        for field in Field::ALL {
            assert_eq!(Field::read(field.as_str()), Some(field));

            let sharing = Field::ALL
                .into_iter()
                .filter(|other| other.as_str() == field.as_str())
                .count();
            assert_eq!(sharing, 1, "{field} is written under a name another claims");
        }
        assert_eq!(Field::read("genre"), None);
        assert_eq!(Field::read(""), None);
    }

    #[test]
    fn a_segment_that_resolves_to_nothing_is_dropped_and_a_file_name_that_does_leaves_no_destination()
     {
        let layout = read(DEFAULT_LAYOUT);
        let mut named = comfortably_numb();
        named.album_artist = None;
        assert_eq!(
            rendered(&layout, &named),
            "The Wall/2-06 Comfortably Numb.flac"
        );

        let ending_in_a_year = read("{album}/{year}");
        let mut undated = comfortably_numb();
        undated.year = None;
        assert_eq!(ending_in_a_year.render(&undated, Naming::Anything), None);
        assert_eq!(
            rendered(&ending_in_a_year, &comfortably_numb()),
            "The Wall/1979.flac"
        );
    }

    #[test]
    fn a_track_number_is_two_digits_and_an_unknown_one_is_nothing() {
        let layout = read("{track} {title}");
        let mut named = comfortably_numb();

        assert_eq!(rendered(&layout, &named), "06 Comfortably Numb.flac");

        named.track = Some(112);
        assert_eq!(rendered(&layout, &named), "112 Comfortably Numb.flac");

        named.track = None;
        assert_eq!(rendered(&layout, &named), "Comfortably Numb.flac");
    }

    #[test]
    fn a_disc_is_written_only_where_the_release_has_more_than_one_of_them() {
        let layout = read("{disc}{track} {title}");
        let mut named = comfortably_numb();

        assert_eq!(rendered(&layout, &named), "2-06 Comfortably Numb.flac");

        named.discs = 1;
        named.disc = NonZeroU32::new(1);
        assert_eq!(rendered(&layout, &named), "06 Comfortably Numb.flac");

        named.discs = 2;
        named.disc = None;
        assert_eq!(rendered(&layout, &named), "06 Comfortably Numb.flac");
    }

    #[test]
    fn the_extension_is_appended_only_where_the_layout_does_not_name_it() {
        let named = comfortably_numb();

        assert_eq!(rendered(&read("{title}"), &named), "Comfortably Numb.flac");
        assert_eq!(
            rendered(&read("{title}.{ext}"), &named),
            "Comfortably Numb.flac"
        );
        assert_eq!(
            rendered(&read("{track} {title} [{ext}]"), &named),
            "06 Comfortably Numb [flac]"
        );

        let mut unlabelled = named;
        unlabelled.extension = None;
        assert_eq!(rendered(&read("{title}"), &unlabelled), "Comfortably Numb");
    }

    #[test]
    fn a_compilation_has_no_owner_to_file_under_so_its_album_folder_sits_under_the_root() {
        let named = Named {
            album_artist: None,
            artist: Some("Kylie Minogue"),
            album: Some("Now That's What I Call Music! 50"),
            title: "Can't Get You Out of My Head",
            year: Some(2001),
            disc: NonZeroU32::new(1),
            discs: 2,
            track: Some(1),
            extension: Some("flac"),
        };

        let destination = read(DEFAULT_LAYOUT)
            .render(&named, Naming::Anything)
            .expect("a compilation is still filed somewhere");

        assert_eq!(
            destination,
            PathBuf::from(
                "Now That's What I Call Music! 50/1-01 Can't Get You Out of My Head.flac"
            )
        );
        assert!(!destination.starts_with("Kylie Minogue"));
    }

    #[test]
    fn a_separator_in_a_name_is_replaced_and_a_control_character_is_dropped() {
        let layout = read("{artist}/{title}");
        let mut named = comfortably_numb();
        named.artist = Some("AC/DC");
        named.title = "Ba\u{7}ck in Bla\u{7f}ck\nAgain";

        assert_eq!(rendered(&layout, &named), "AC-DC/Back in BlackAgain.flac");
    }

    #[test]
    fn a_component_is_cut_to_the_bytes_a_filesystem_holds_and_never_mid_character() {
        let chanted = "音".repeat(300);
        let layout = read("{album}/{title}");
        let mut named = comfortably_numb();
        named.album = Some(&chanted);
        named.title = &chanted;

        let destination = layout
            .render(&named, Naming::Anything)
            .expect("a long name is cut rather than refused");
        let mut components = destination.components();

        let folder = components
            .next()
            .expect("the album folder is the first component")
            .as_os_str()
            .to_str()
            .expect("a rendered component is the text it was rendered from");
        assert_eq!(folder.len(), 255);
        assert_eq!(folder.chars().count(), 85);

        let file = components
            .next()
            .expect("the file name is the second component")
            .as_os_str()
            .to_str()
            .expect("a rendered component is the text it was rendered from");
        assert_eq!(file.len(), 254);
        assert_eq!(file.chars().count(), 88);
        assert!(file.ends_with(".flac"));

        let cut_on_a_space = format!("{}{}", "a".repeat(249), " bbbbbbbbbb");
        named.album = Some("The Wall");
        named.title = &cut_on_a_space;
        assert_eq!(
            rendered(&layout, &named),
            format!("The Wall/{}.flac", "a".repeat(249))
        );
    }

    #[test]
    fn a_component_that_is_only_dots_or_only_spaces_resolves_to_nothing() {
        let layout = read("{album}/{title}");
        let mut named = comfortably_numb();

        for nothing in ["...", "   ", "..", ".", " . "] {
            named.album = Some(nothing);
            assert_eq!(
                rendered(&layout, &named),
                "Comfortably Numb.flac",
                "{nothing:?} was kept as a folder"
            );
        }

        named.album = Some("...And Justice for All");
        assert_eq!(
            rendered(&layout, &named),
            "...And Justice for All/Comfortably Numb.flac"
        );

        named.title = "Alive.";
        assert_eq!(
            rendered(&layout, &named),
            "...And Justice for All/Alive.flac"
        );

        named.title = "..";
        assert_eq!(layout.render(&named, Naming::Anything), None);
    }

    fn row(id: u64, path: &str) -> TrackToFile {
        TrackToFile {
            id: TrackId::new(id).expect("a non-zero id"),
            path: PathBuf::from(path),
            root: PathBuf::from(NOWHERE),
            title: "Echoes".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            album: Some("Meddle".to_owned()),
            album_id: AlbumId::new(1).ok(),
            album_artist: Some("Pink Floyd".to_owned()),
            year: Some(1971),
            track: Some(1),
            disc: None,
            discs: 0,
        }
    }

    fn preview(rows: &[TrackToFile], template: &str) -> (Plan, OrganiseStats) {
        let progress = OrganiseProgress::default();
        let plan = planned(rows, &read(template), &progress);
        (plan, progress.snapshot())
    }

    fn a_folder_of_its_own() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let folder = env::temp_dir().join(format!(
            "resonate-organise-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Counting::Relaxed)
        ));
        fs::create_dir_all(&folder).expect("a writable temporary directory");
        folder
    }

    #[test]
    fn a_track_already_where_the_layout_puts_it_is_counted_unchanged_and_never_planned() {
        let row = row(1, &format!("{NOWHERE}/Pink Floyd/Meddle/01 Echoes.wav"));
        let (plan, stats) = preview(slice::from_ref(&row), DEFAULT_LAYOUT);

        assert_eq!(plan.unchanged, 1);
        assert_eq!(stats.unchanged, 1);
        assert!(plan.moves.is_empty());
        assert!(plan.refused.is_empty());
    }

    #[test]
    fn a_destination_with_no_folder_between_it_and_the_root_is_refused_as_loose() {
        let row = row(1, &format!("{NOWHERE}/somewhere/whatever.wav"));
        let (plan, stats) = preview(slice::from_ref(&row), "{title}");

        assert!(plan.moves.is_empty());
        assert_eq!(
            plan.refused,
            vec![Refused {
                from: row.path.clone(),
                refusal: Refusal::Loose,
            }]
        );
        assert_eq!(stats.unidentified, 1);
    }

    #[test]
    fn a_layout_that_names_no_destination_for_a_track_leaves_it_unidentified() {
        let mut row = row(1, &format!("{NOWHERE}/somewhere/whatever.wav"));
        row.year = None;
        let (plan, stats) = preview(slice::from_ref(&row), "{album}/{year}");

        assert!(plan.moves.is_empty());
        assert_eq!(
            plan.refused,
            vec![Refused {
                from: row.path.clone(),
                refusal: Refusal::Unidentified,
            }]
        );
        assert_eq!(stats.unidentified, 1);
    }

    #[test]
    fn the_first_of_two_tracks_naming_one_destination_moves_and_the_rest_collide_with_it() {
        let rows = vec![
            row(1, &format!("{NOWHERE}/a.wav")),
            row(2, &format!("{NOWHERE}/b.wav")),
            row(3, &format!("{NOWHERE}/c.wav")),
        ];
        let (plan, stats) = preview(&rows, DEFAULT_LAYOUT);

        assert_eq!(plan.moves.len(), 1);
        assert_eq!(plan.moves[0].from, rows[0].path);
        assert_eq!(
            plan.moves[0].to,
            PathBuf::from(format!("{NOWHERE}/Pink Floyd/Meddle/01 Echoes.wav"))
        );
        assert_eq!(
            plan.refused,
            vec![
                Refused {
                    from: rows[1].path.clone(),
                    refusal: Refusal::Collided {
                        with: rows[0].path.clone()
                    },
                },
                Refused {
                    from: rows[2].path.clone(),
                    refusal: Refusal::Collided {
                        with: rows[0].path.clone()
                    },
                },
            ]
        );
        assert_eq!(stats.collided, 2);
    }

    #[test]
    fn the_rows_cut_out_of_one_file_file_it_by_their_folder_and_leave_it_the_name_it_has() {
        let mut first = row(1, &format!("{NOWHERE}/rips/Meddle.wav"));
        first.title = "One of These Days".to_owned();
        let mut second = row(2, &format!("{NOWHERE}/rips/Meddle.wav"));
        second.title = "Echoes".to_owned();
        second.track = Some(6);

        let (plan, stats) = preview(&[first, second], DEFAULT_LAYOUT);

        assert_eq!(stats.unidentified, 0);
        assert_eq!(plan.moves.len(), 1);
        assert_eq!(
            plan.moves[0].to,
            PathBuf::from(format!("{NOWHERE}/Pink Floyd/Meddle/Meddle.wav"))
        );
        assert_eq!(plan.moves[0].rows, 2);
    }

    #[test]
    fn the_rows_cut_out_of_one_file_that_name_two_folders_leave_it_unidentified() {
        let first = row(1, &format!("{NOWHERE}/rips/Meddle.wav"));
        let mut second = row(2, &format!("{NOWHERE}/rips/Meddle.wav"));
        second.album = Some("Obscured by Clouds".to_owned());

        let (plan, stats) = preview(&[first, second], DEFAULT_LAYOUT);

        assert!(plan.moves.is_empty());
        assert_eq!(plan.refused.len(), 1);
        assert_eq!(plan.refused[0].refusal, Refusal::Unidentified);
        assert_eq!(stats.unidentified, 1);
    }

    #[test]
    fn a_folder_spelling_a_disc_names_the_disc_where_no_tag_does() {
        let mut first = row(1, &format!("{NOWHERE}/The Wall/CD1/a.wav"));
        first.title = "In the Flesh?".to_owned();
        let mut second = row(2, &format!("{NOWHERE}/The Wall/CD2/b.wav"));
        second.title = "Hey You".to_owned();

        let (plan, _) = preview(&[first, second], DEFAULT_LAYOUT);

        assert_eq!(
            plan.moves
                .iter()
                .map(|planned| planned.to.clone())
                .collect::<Vec<_>>(),
            vec![
                PathBuf::from(format!(
                    "{NOWHERE}/Pink Floyd/Meddle/1-01 In the Flesh?.wav"
                )),
                PathBuf::from(format!("{NOWHERE}/Pink Floyd/Meddle/2-01 Hey You.wav")),
            ]
        );
    }

    #[test]
    fn a_destination_another_file_already_holds_collides_and_a_source_that_has_gone_fails() {
        let folder = a_folder_of_its_own();
        fs::create_dir_all(folder.join("Meddle")).expect("a writable temporary directory");
        fs::write(folder.join("Meddle/Echoes.wav"), b"held").expect("a writable temporary file");
        fs::write(folder.join("here.wav"), b"standing").expect("a writable temporary file");

        let mut standing = row(1, "");
        standing.root = folder.clone();
        standing.path = folder.join("here.wav");
        let (plan, stats) = preview(slice::from_ref(&standing), "{album}/{title}");

        assert!(plan.moves.is_empty());
        assert_eq!(
            plan.refused,
            vec![Refused {
                from: standing.path.clone(),
                refusal: Refusal::Collided {
                    with: folder.join("Meddle/Echoes.wav")
                },
            }]
        );
        assert_eq!(stats.collided, 1);

        let mut gone = standing.clone();
        gone.path = folder.join("gone.wav");
        let (plan, stats) = preview(slice::from_ref(&gone), "{album}/{title}");

        assert!(plan.moves.is_empty());
        assert_eq!(
            plan.refused,
            vec![Refused {
                from: gone.path.clone(),
                refusal: Refusal::SourceGone,
            }]
        );
        assert_eq!(stats.failed, 1);

        fs::remove_dir_all(&folder).expect("the temporary folder goes away");
    }

    #[test]
    fn a_file_beside_a_track_that_shares_its_name_travels_with_it_and_empties_the_folder() {
        let folder = a_folder_of_its_own();
        let rips = folder.join("rips");
        fs::create_dir_all(&rips).expect("a writable temporary directory");
        fs::write(rips.join("Meddle.wav"), b"audio").expect("a writable temporary file");
        fs::write(rips.join("Meddle.cue"), b"sheet").expect("a writable temporary file");
        fs::write(rips.join("Meddle.wav.log"), b"rip").expect("a writable temporary file");
        fs::write(rips.join("Meddlesome.txt"), b"another").expect("a writable temporary file");

        let mut track = row(1, "");
        track.root = folder.clone();
        track.path = rips.join("Meddle.wav");
        let (plan, _) = preview(slice::from_ref(&track), DEFAULT_LAYOUT);

        let landing = folder.join("Pink Floyd/Meddle");
        assert_eq!(plan.moves.len(), 1);
        assert_eq!(
            plan.moves[0].sidecars,
            vec![
                Sidecar {
                    from: rips.join("Meddle.cue"),
                    to: landing.join("01 Echoes.cue"),
                },
                Sidecar {
                    from: rips.join("Meddle.wav.log"),
                    to: landing.join("01 Echoes.wav.log"),
                },
            ]
        );
        assert!(
            plan.folders.is_empty(),
            "a folder still holding a file nothing moves is not one the moves empty"
        );

        fs::remove_file(rips.join("Meddlesome.txt")).expect("the odd file goes away");
        let (plan, _) = preview(slice::from_ref(&track), DEFAULT_LAYOUT);
        assert_eq!(plan.folders, vec![rips.clone()]);

        fs::remove_dir_all(&folder).expect("the temporary folder goes away");
    }

    #[test]
    fn a_track_moving_onto_a_file_that_is_itself_moving_is_ordered_behind_it() {
        let folder = a_folder_of_its_own();
        fs::create_dir_all(folder.join("Meddle")).expect("a writable temporary directory");
        fs::write(folder.join("Meddle/Echoes.wav"), b"held").expect("a writable temporary file");
        fs::write(folder.join("here.wav"), b"standing").expect("a writable temporary file");

        let mut arriving = row(1, "");
        arriving.root = folder.clone();
        arriving.path = folder.join("here.wav");
        let mut vacating = row(2, "");
        vacating.root = folder.clone();
        vacating.path = folder.join("Meddle/Echoes.wav");
        vacating.title = "Fearless".to_owned();

        let (plan, stats) = preview(&[arriving.clone(), vacating.clone()], "{album}/{title}");

        assert!(
            plan.refused.is_empty(),
            "a chain was refused rather than ordered"
        );
        assert_eq!(stats.collided, 0);
        assert_eq!(
            plan.moves
                .iter()
                .map(|planned| (planned.from.clone(), planned.to.clone()))
                .collect::<Vec<_>>(),
            vec![
                (vacating.path.clone(), folder.join("Meddle/Fearless.wav")),
                (arriving.path.clone(), folder.join("Meddle/Echoes.wav")),
            ]
        );

        fs::remove_dir_all(&folder).expect("the temporary folder goes away");
    }

    #[test]
    fn two_tracks_moving_onto_each_other_are_both_left_where_they_stand() {
        let folder = a_folder_of_its_own();
        fs::create_dir_all(folder.join("Meddle")).expect("a writable temporary directory");
        for named in ["Echoes", "Fearless"] {
            fs::write(folder.join(format!("Meddle/{named}.wav")), b"held")
                .expect("a writable temporary file");
        }

        let mut one = row(1, "");
        one.root = folder.clone();
        one.path = folder.join("Meddle/Echoes.wav");
        one.title = "Fearless".to_owned();
        let mut other = row(2, "");
        other.root = folder.clone();
        other.path = folder.join("Meddle/Fearless.wav");
        other.title = "Echoes".to_owned();

        let (plan, stats) = preview(&[one.clone(), other.clone()], "{album}/{title}");

        assert!(
            plan.moves.is_empty(),
            "a cycle was ordered rather than refused"
        );
        assert_eq!(
            plan.refused,
            vec![
                Refused {
                    from: one.path.clone(),
                    refusal: Refusal::Collided {
                        with: other.path.clone()
                    },
                },
                Refused {
                    from: other.path.clone(),
                    refusal: Refusal::Collided {
                        with: one.path.clone()
                    },
                },
            ]
        );
        assert_eq!(stats.collided, 2);

        fs::remove_dir_all(&folder).expect("the temporary folder goes away");
    }

    #[test]
    fn a_track_moving_onto_a_file_that_is_already_where_it_belongs_collides_with_it() {
        let folder = a_folder_of_its_own();
        fs::create_dir_all(folder.join("Meddle")).expect("a writable temporary directory");
        fs::write(folder.join("Meddle/Echoes.wav"), b"held").expect("a writable temporary file");
        fs::write(folder.join("here.wav"), b"standing").expect("a writable temporary file");

        let mut arriving = row(1, "");
        arriving.root = folder.clone();
        arriving.path = folder.join("here.wav");
        let mut standing = row(2, "");
        standing.root = folder.clone();
        standing.path = folder.join("Meddle/Echoes.wav");

        let (plan, stats) = preview(&[arriving.clone(), standing.clone()], "{album}/{title}");

        assert!(plan.moves.is_empty());
        assert_eq!(plan.unchanged, 1);
        assert_eq!(
            plan.refused,
            vec![Refused {
                from: arriving.path.clone(),
                refusal: Refusal::Collided {
                    with: standing.path.clone()
                },
            }]
        );
        assert_eq!(stats.collided, 1);

        fs::remove_dir_all(&folder).expect("the temporary folder goes away");
    }

    #[test]
    fn a_preview_names_the_folders_it_would_empty_deepest_first() {
        let folder = a_folder_of_its_own();
        let set = folder.join("The Wall");
        for disc in ["CD1", "CD2"] {
            fs::create_dir_all(set.join(disc)).expect("a writable temporary directory");
            fs::write(set.join(disc).join("a.wav"), b"audio").expect("a writable temporary file");
        }

        let mut rows = Vec::new();
        for (id, disc) in ["CD1", "CD2"].into_iter().enumerate() {
            let mut track = row(id as u64 + 1, "");
            track.root = folder.clone();
            track.path = set.join(disc).join("a.wav");
            track.title = format!("Track {disc}");
            rows.push(track);
        }
        let (plan, _) = preview(&rows, DEFAULT_LAYOUT);

        assert_eq!(plan.moves.len(), 2);
        assert_eq!(plan.folders, vec![set.join("CD1"), set.join("CD2")]);

        fs::remove_dir_all(&folder).expect("the temporary folder goes away");
    }
}
