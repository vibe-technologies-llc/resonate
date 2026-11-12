use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fs,
    path::{self, Path, PathBuf},
};

use resonate_codec::{Sources, probe, read_cue_media, the_file_a_cue_names};
use resonate_core::{FrameSpan, MediaLocation, names_audio};
use resonate_engine::{QueueItem, Unclaimed};
use resonate_library::{PlaylistFormat, cuts_listed_in};

use crate::{
    error::{Error, Result},
    served::Served,
};

pub(crate) const SHEET_EXTENSION: &str = "cue";

const FOLDERS_DEEP: usize = 32;

pub(crate) fn local_path(argument: &Path) -> PathBuf {
    argument
        .to_str()
        .and_then(MediaLocation::from_uri)
        .and_then(|named| named.as_path().map(Path::to_path_buf))
        .unwrap_or_else(|| argument.to_path_buf())
}

pub(crate) fn location_of_argument(argument: &OsStr, sources: &Sources) -> MediaLocation {
    argument
        .to_str()
        .and_then(MediaLocation::from_uri)
        .filter(|named| held_by(named, sources))
        .map(settled_here)
        .unwrap_or_else(|| MediaLocation::local(from_here(Path::new(argument))))
}

pub(crate) fn cut_of_argument(
    argument: &OsStr,
    sources: &Sources,
) -> Result<(MediaLocation, Option<FrameSpan>)> {
    if let Some(uri) = argument
        .to_str()
        .filter(|uri| MediaLocation::claims_a_span(uri))
    {
        if let Some((location, span)) =
            MediaLocation::from_uri_within(uri).filter(|(named, _)| held_by(named, sources))
        {
            return Ok((settled_here(location), span));
        }
        if let Some(location) = MediaLocation::from_uri(uri).filter(|named| held_by(named, sources))
        {
            return Err(Error::UnreadableSpan {
                location: settled_here(location),
            });
        }
    }
    Ok((location_of_argument(argument, sources), None))
}

fn held_by(named: &MediaLocation, sources: &Sources) -> bool {
    named.as_path().is_some() || sources.provider(named.source()).is_some()
}

fn settled_here(location: MediaLocation) -> MediaLocation {
    match location.as_path() {
        Some(path) => MediaLocation::local(from_here(path)),
        None => location,
    }
}

pub(crate) fn from_here(path: &Path) -> PathBuf {
    path.canonicalize()
        .or_else(|_| {
            path::absolute(path).map(|absolute| resonate_core::path_text::verbatim(&absolute))
        })
        .unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn queue_items(arguments: &[OsString]) -> Vec<QueueItem> {
    queue_items_served(arguments, &Served::default())
}

pub(crate) fn queue_items_served(arguments: &[OsString], served: &Served) -> Vec<QueueItem> {
    let sources = served.sources(Sources::local());
    let mut minting = Unclaimed::beside(&[]);
    let mut items = Vec::with_capacity(arguments.len());

    for argument in arguments {
        let (location, span) = match cut_of_argument(argument, &sources) {
            Ok(cut) => cut,
            Err(error) => {
                tracing::warn!(%error, "an argument naming a cut was not queued");
                continue;
            }
        };
        if span.is_some() {
            items.push(QueueItem {
                id: minting.mint(),
                location,
                span,
            });
            continue;
        }
        if let Some(songs) = served.songs_at(&location) {
            items.extend(
                songs
                    .into_iter()
                    .map(|song| QueueItem::whole(minting.mint(), song)),
            );
            continue;
        }
        items.extend(items_of(&sources, location, &mut minting));
    }
    items
}

fn items_of(sources: &Sources, location: MediaLocation, minting: &mut Unclaimed) -> Vec<QueueItem> {
    let Some(path) = location.as_path().map(Path::to_path_buf) else {
        return vec![QueueItem::whole(minting.mint(), location)];
    };
    if path.is_dir() {
        return folder_items(sources, &path, minting, FOLDERS_DEEP, &mut BTreeSet::new());
    }
    if names_a_sheet(&location) {
        return sheet_items(sources, &path, minting);
    }
    if PlaylistFormat::named_by(&path).is_some() {
        return listed_items(&path, minting);
    }
    vec![QueueItem::whole(minting.mint(), location)]
}

fn listed_items(path: &Path, minting: &mut Unclaimed) -> Vec<QueueItem> {
    match cuts_listed_in(path) {
        Ok(cuts) => cuts
            .into_iter()
            .map(|cut| QueueItem {
                id: minting.mint(),
                location: cut.location,
                span: cut.span,
            })
            .collect(),
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "a playlist could not be read");
            Vec::new()
        }
    }
}

fn listed_in_order(folder: &Path) -> Vec<PathBuf> {
    let mut listed: Vec<PathBuf> = match fs::read_dir(folder) {
        Ok(entries) => entries
            .flatten()
            .filter(|entry| !entry.file_name().as_encoded_bytes().starts_with(b"."))
            .map(|entry| entry.path())
            .collect(),
        Err(error) => {
            tracing::warn!(%error, folder = %folder.display(), "a folder could not be read");
            Vec::new()
        }
    };
    listed.sort();
    listed
}

fn folder_items(
    sources: &Sources,
    folder: &Path,
    minting: &mut Unclaimed,
    deeper: usize,
    walked: &mut BTreeSet<PathBuf>,
) -> Vec<QueueItem> {
    let Some(deeper) = deeper.checked_sub(1) else {
        tracing::warn!(folder = %folder.display(), "a folder nested too deep was not queued");
        return Vec::new();
    };
    let first_time_through = folder
        .canonicalize()
        .map_or(true, |canonical| walked.insert(canonical));
    if !first_time_through {
        tracing::debug!(folder = %folder.display(), "a folder reached again through a link was queued once");
        return Vec::new();
    }
    let (folders, files): (Vec<PathBuf>, Vec<PathBuf>) = listed_in_order(folder)
        .into_iter()
        .partition(|path| path.is_dir());
    let sheets: BTreeMap<&Path, Vec<(MediaLocation, FrameSpan)>> = files
        .iter()
        .filter(|file| names_a_sheet(&MediaLocation::local(file)))
        .map(|sheet| (sheet.as_path(), sheet_cuts(sources, sheet)))
        .collect();
    let cut_from: BTreeSet<PathBuf> = sheets
        .values()
        .flatten()
        .filter_map(|(location, _)| location.as_path().map(from_here))
        .collect();

    let mut items = Vec::new();
    for file in &files {
        if let Some(cuts) = sheets.get(file.as_path()) {
            items.extend(cuts.iter().map(|(location, span)| QueueItem {
                id: minting.mint(),
                location: location.clone(),
                span: Some(*span),
            }));
        } else if names_audio(file) && !cut_from.contains(&from_here(file)) {
            items.push(QueueItem::whole(minting.mint(), MediaLocation::local(file)));
        }
    }
    for inside in &folders {
        items.extend(folder_items(sources, inside, minting, deeper, walked));
    }
    items
}

pub(crate) fn names_a_sheet(location: &MediaLocation) -> bool {
    location
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(SHEET_EXTENSION))
}

pub(crate) fn sheet_items(
    sources: &Sources,
    path: &Path,
    minting: &mut Unclaimed,
) -> Vec<QueueItem> {
    sheet_cuts(sources, path)
        .into_iter()
        .map(|(location, span)| QueueItem {
            id: minting.mint(),
            location,
            span: Some(span),
        })
        .collect()
}

pub(crate) fn sheet_cuts(sources: &Sources, path: &Path) -> Vec<(MediaLocation, FrameSpan)> {
    let sheet = match read_cue_media(sources, &MediaLocation::local(path)) {
        Ok(sheet) => sheet,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "a cue sheet could not be read");
            return Vec::new();
        }
    };

    let mut cuts = Vec::new();
    for cut in &sheet.files {
        let Some(file) = path
            .parent()
            .and_then(|folder| the_file_a_cue_names(folder, &cut.named))
        else {
            tracing::warn!(sheet = %path.display(), file = %cut.named, "a cue sheet names a file that is not where it says");
            continue;
        };
        let location = MediaLocation::local(from_here(&file));
        let Ok(info) = probe(sources, &location) else {
            tracing::warn!(path = %file.display(), "a cue sheet names a file that will not probe");
            continue;
        };

        for (index, _) in cut.audio_tracks() {
            let Some(span) = cut.span_of(index, info.spec.rate, info.duration) else {
                continue;
            };
            cuts.push((location.clone(), span));
        }
    }
    cuts
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::{env, fs, process, sync::Arc};

    use resonate_codec::{Media, MediaProvider};
    use resonate_core::{Frames, SourceId};

    use super::*;
    use crate::analyse;

    fn located(argument: &str) -> Option<MediaLocation> {
        queue_items(&[OsString::from(argument)])
            .first()
            .map(|item| item.location.clone())
    }

    #[test]
    fn a_file_uri_names_the_file_a_command_reads_and_a_path_is_taken_as_written() {
        assert_eq!(
            local_path(Path::new("file:///music/Pink%20Floyd/Echoes.flac")),
            Path::new("/music/Pink Floyd/Echoes.flac")
        );
        assert_eq!(
            local_path(Path::new("FILE://localhost/music/a.flac")),
            Path::new("/music/a.flac")
        );
        assert_eq!(
            local_path(Path::new("music/a.flac")),
            Path::new("music/a.flac")
        );
        assert_eq!(
            local_path(Path::new("subsonic:track/1")),
            Path::new("subsonic:track/1")
        );
    }

    #[test]
    fn a_uri_naming_frames_queues_that_cut_and_one_naming_no_span_queues_nothing() {
        let (uri, path, unreadable) = if cfg!(windows) {
            (
                "file:///C:/music/Meddle.flac#frames=588-44100",
                r"\\?\C:\music\Meddle.flac",
                "file:///C:/music/Meddle.flac#frames=100-50",
            )
        } else {
            (
                "file:///music/Meddle.flac#frames=588-44100",
                "/music/Meddle.flac",
                "file:///music/Meddle.flac#frames=100-50",
            )
        };
        let queued = queue_items(&[OsString::from(uri)]);
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].location, MediaLocation::local(path));
        assert_eq!(
            queued[0].span,
            Some(FrameSpan::between(Frames(588), Frames(44_100))),
            "the cut a URI names was queued as the whole file"
        );

        assert!(
            queue_items(&[OsString::from(unreadable)]).is_empty(),
            "a cut that is no span was queued as the whole file"
        );
    }

    #[test]
    fn a_folder_is_queued_as_the_audio_inside_it_and_its_sheet_as_the_rows_it_cuts() {
        let folder = env::temp_dir().join(format!("resonate-queued-folder-{}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(folder.join("Album/CD2")).expect("a scratch folder");
        for name in [
            "Album/02.wav",
            "Album/01.wav",
            "Album/CD2/01.wav",
            "Album/.hidden.wav",
        ] {
            fs::write(folder.join(name), analyse::tests::silent_wave()).expect("a wave file");
        }
        fs::write(folder.join("Album/notes.txt"), b"words").expect("a note");
        fs::create_dir_all(folder.join("Rip")).expect("a scratch folder");
        fs::write(folder.join("Rip/whole.wav"), analyse::tests::silent_wave()).expect("a wave");
        fs::write(
            folder.join("Rip/whole.cue"),
            "FILE \"whole.wav\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n  TRACK 02 \
             AUDIO\n    INDEX 01 00:00:30\n",
        )
        .expect("a sheet");
        let named = |items: Vec<QueueItem>| {
            items
                .into_iter()
                .map(|item| {
                    (
                        item.location.as_path().map(Path::to_path_buf),
                        item.span.is_some(),
                    )
                })
                .collect::<Vec<_>>()
        };

        let album = named(queue_items(&[folder.join("Album").into_os_string()]));
        let rip = named(queue_items(&[folder.join("Rip").into_os_string()]));

        let at = |name: &str| Some(from_here(&folder.join(name)));
        assert_eq!(
            album,
            [
                (at("Album/01.wav"), false),
                (at("Album/02.wav"), false),
                (at("Album/CD2/01.wav"), false)
            ]
        );
        assert_eq!(
            rip,
            [(at("Rip/whole.wav"), true), (at("Rip/whole.wav"), true)]
        );

        fs::remove_dir_all(&folder).expect("the scratch folder goes");
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_linked_back_to_itself_or_to_a_sibling_is_queued_once() {
        let folder = env::temp_dir().join(format!("resonate-queued-loop-{}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(folder.join("Music/Album")).expect("a scratch folder");
        fs::write(
            folder.join("Music/Album/01.wav"),
            analyse::tests::silent_wave(),
        )
        .expect("a wave");
        std::os::unix::fs::symlink("..", folder.join("Music/Album/Backup")).expect("a link up");
        std::os::unix::fs::symlink("Album", folder.join("Music/Again")).expect("a sibling link");
        std::os::unix::fs::symlink(".", folder.join("Music/Here")).expect("a link to itself");

        let queued = queue_items(&[folder.join("Music").into_os_string()]);

        fs::remove_dir_all(&folder).expect("the scratch folder goes");
        assert_eq!(queued.len(), 1);
    }

    #[test]
    fn a_playlist_is_queued_as_the_rows_it_lists() {
        let folder = env::temp_dir().join(format!("resonate-queued-list-{}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("a scratch folder");
        let folder = folder
            .canonicalize()
            .expect("the scratch folder is canonical");
        for name in ["one.wav", "two.wav"] {
            fs::write(folder.join(name), analyse::tests::silent_wave()).expect("a wave file");
        }
        fs::write(folder.join("mix.m3u8"), "#EXTM3U\ntwo.wav\none.wav\n").expect("a list");

        let queued: Vec<_> = queue_items(&[folder.join("mix.m3u8").into_os_string()])
            .into_iter()
            .map(|item| item.location.as_path().map(Path::to_path_buf))
            .collect();

        assert_eq!(
            queued,
            [Some(folder.join("two.wav")), Some(folder.join("one.wav"))]
        );

        fs::remove_dir_all(&folder).expect("the scratch folder goes");
    }

    #[test]
    fn a_sheet_finds_its_audio_as_the_scan_does_whatever_case_or_extension_it_wrote() {
        let folder = env::temp_dir().join(format!("resonate-sheet-names-{}", process::id()));
        fs::create_dir_all(folder.join("CD1")).expect("a scratch folder");
        fs::write(folder.join("album.wav"), analyse::tests::silent_wave()).expect("a wave file");
        fs::write(folder.join("CD1/01.wav"), analyse::tests::silent_wave()).expect("a wave file");
        let one_track = |named: &str| {
            format!("FILE \"{named}\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n")
        };

        for (written, found) in [
            ("ALBUM.WAV", "album.wav"),
            ("album.flac", "album.wav"),
            ("CD1\\01.wav", "CD1/01.wav"),
        ] {
            let sheet = folder.join("sheet.cue");
            fs::write(&sheet, one_track(written)).expect("a sheet");

            let cuts = sheet_cuts(&Sources::local(), &sheet);

            assert_eq!(cuts.len(), 1, "{written} found nothing");
            assert_eq!(
                cuts[0].0.as_path(),
                Some(from_here(&folder.join(found)).as_path()),
                "{written}"
            );
        }

        fs::remove_dir_all(&folder).expect("the scratch folder goes");
    }

    #[test]
    fn a_uri_naming_a_source_this_build_holds_keeps_the_source_that_named_it() {
        let subsonic = SourceId::new("subsonic").expect("a source name");
        let sources = Sources::local().and(Arc::new(Elsewhere(subsonic)));

        assert_eq!(
            Some(location_of_argument(
                OsStr::new("subsonic:track/1"),
                &sources
            )),
            MediaLocation::from_uri("subsonic:track/1")
        );
    }

    #[test]
    fn a_uri_a_file_manager_passes_names_the_file_it_points_at() {
        let (uri, path) = if cfg!(windows) {
            (
                "file:///C:/music/Pink%20Floyd/Echoes.flac",
                r"\\?\C:\music\Pink Floyd\Echoes.flac",
            )
        } else {
            (
                "file:///music/Pink%20Floyd/Echoes.flac",
                "/music/Pink Floyd/Echoes.flac",
            )
        };
        assert_eq!(located(uri), Some(MediaLocation::local(path)));
    }

    #[test]
    fn an_argument_that_is_a_path_rather_than_a_uri_is_read_as_the_path_it_is() {
        let paths = if cfg!(windows) {
            [
                (
                    r"C:\music\Pink Floyd\Echoes.flac",
                    r"\\?\C:\music\Pink Floyd\Echoes.flac",
                ),
                (r"C:\music\a.flac", r"\\?\C:\music\a.flac"),
            ]
        } else {
            [
                (
                    "/music/Pink Floyd/Echoes.flac",
                    "/music/Pink Floyd/Echoes.flac",
                ),
                ("/music/a:b.flac", "/music/a:b.flac"),
            ]
        };
        for (argument, path) in paths {
            assert_eq!(located(argument), Some(MediaLocation::local(path)));
        }
    }

    #[test]
    fn an_argument_naming_a_file_from_here_is_read_as_the_file_it_names() {
        let here = env::current_dir().expect("a working directory");
        let here = resonate_core::path_text::verbatim(&here);

        assert_eq!(
            located("a.flac"),
            Some(MediaLocation::local(here.join("a.flac")))
        );
        assert_eq!(
            located("./sheets/../a.flac"),
            Some(MediaLocation::local(here.join("sheets/../a.flac"))),
            "a path was resolved past a folder that is not there"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_uri_names_the_same_file_a_path_through_the_same_link_does() {
        let folder = env::temp_dir().join(format!("resonate-uri-{}", process::id()));
        let real = folder.join("real");
        fs::create_dir_all(&real).expect("a scratch folder");
        fs::write(real.join("a.flac"), b"").expect("a file");
        let linked = folder.join("linked");
        symlink(&real, &linked).expect("a link to the folder");

        let through_the_link = linked.join("../linked/a.flac");
        let uri = MediaLocation::local(&through_the_link).to_uri();
        assert_eq!(
            located(&uri),
            located(through_the_link.to_str().expect("a UTF-8 path"))
        );
        assert_eq!(
            located(&uri),
            Some(MediaLocation::local(
                real.canonicalize().expect("the folder").join("a.flac")
            )),
            "a file:// argument kept the link it was named through"
        );

        fs::remove_dir_all(&folder).expect("the scratch folder goes");
    }

    #[test]
    fn a_queued_row_is_named_by_a_uri_that_reads_back_as_the_same_row() {
        let queued = located("a.flac").expect("one argument makes one row");

        assert_eq!(MediaLocation::from_uri(&queued.to_uri()), Some(queued));
    }

    #[test]
    fn an_argument_naming_a_source_this_build_does_not_hold_is_a_file_from_here() {
        let here = env::current_dir().expect("a working directory");
        let here = resonate_core::path_text::verbatim(&here);

        for named in ["01:intro.flac", "subsonic:track/1"] {
            assert_eq!(located(named), Some(MediaLocation::local(here.join(named))));
        }
    }

    struct Elsewhere(SourceId);

    impl MediaProvider for Elsewhere {
        fn source(&self) -> &SourceId {
            &self.0
        }

        fn open(&self, location: &MediaLocation) -> resonate_codec::Result<Media> {
            Err(resonate_codec::Error::LocatorNotUsable {
                location: location.clone(),
            })
        }
    }
}
