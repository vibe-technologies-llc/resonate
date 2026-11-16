use resonate_library::{Library, Playlist, Replaceable, Replacing};

use crate::{error::Result, table::Table, words, words::columns};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fate {
    Waiting,
    Held,
    Wanted,
    Unmatched,
}

impl Fate {
    const fn of(song: &Replaceable) -> Self {
        match (
            song.pending,
            song.held.is_some(),
            song.release_track.is_some(),
        ) {
            (true, _, _) => Self::Waiting,
            (false, true, _) => Self::Held,
            (false, false, true) => Self::Wanted,
            (false, false, false) => Self::Unmatched,
        }
    }

    fn said(self, song: &Replaceable) -> String {
        match (self, song.held.as_ref()) {
            (Self::Held, Some(held)) => words::lossy::held_copy(held.codec.as_str()),
            (Self::Held | Self::Wanted, _) => words::lossy::wanted().to_owned(),
            (Self::Waiting, _) => words::lossy::already_waiting().to_owned(),
            (Self::Unmatched, _) => words::lossy::no_release().to_owned(),
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Settled {
    pub(crate) replacing: Replacing,
    pub(crate) waiting: usize,
    pub(crate) unreplaceable: usize,
}

pub(crate) fn list(library: &Library, playlist: &Playlist) -> Result<()> {
    let songs = library.lossy_songs_of(playlist.id)?;

    said_on!("{}", listed(&playlist.name, &songs));
    Ok(())
}

fn listed(name: &str, songs: &[Replaceable]) -> String {
    if songs.is_empty() {
        return format!("{}\n", words::lossy::none_lossy(name));
    }

    let replaceable = songs.iter().filter(|song| song.can_be_ticked()).count();
    format!(
        "{}{}\n",
        table_of(songs).render(),
        words::lossy::can_be_replaced(replaceable, songs.len())
    )
}

pub(crate) fn replace(library: &Library, playlist: &Playlist) -> Result<()> {
    let songs = library.lossy_songs_of(playlist.id)?;
    if songs.is_empty() {
        said!("{}", words::lossy::none_lossy(&playlist.name));
        return Ok(());
    }

    let settled = settle(library, &songs)?;
    said!("{}", told(&playlist.name, &settled));
    Ok(())
}

pub(crate) fn settle(library: &Library, songs: &[Replaceable]) -> Result<Settled> {
    let chosen: Vec<Replaceable> = songs
        .iter()
        .filter(|song| song.can_be_ticked())
        .cloned()
        .collect();
    let waiting = songs.iter().filter(|song| song.pending).count();
    let replacing = match chosen.is_empty() {
        true => Replacing::default(),
        false => library.replace(&chosen)?,
    };

    Ok(Settled {
        replacing,
        waiting,
        unreplaceable: songs.len() - chosen.len() - waiting,
    })
}

fn table_of(songs: &[Replaceable]) -> Table {
    let mut table = Table::new(vec![
        columns::title(),
        columns::artist(),
        columns::format(),
        columns::status(),
    ]);
    for song in songs {
        table.push(vec![
            song.track.title.clone(),
            song.track.artist.clone().unwrap_or_default(),
            song.track.codec.as_str().to_owned(),
            Fate::of(song).said(song),
        ]);
    }
    table
}

fn told(name: &str, settled: &Settled) -> String {
    let Settled {
        replacing,
        waiting,
        unreplaceable,
    } = settled;
    let left = unreplaceable + usize::try_from(replacing.unmatched).unwrap_or(usize::MAX);
    let parts: Vec<String> = [
        (replacing.folded > 0).then(|| words::lossy::replaced(replacing.folded)),
        (replacing.trashed.files > 0).then(|| words::lossy::trashed(replacing.trashed.files)),
        (replacing.trashed.kept > 0).then(|| words::lossy::not_trashed(replacing.trashed.kept)),
        (replacing.wanted > 0).then(|| words::lossy::now_wanted(replacing.wanted)),
        (*waiting > 0).then(|| words::lossy::waiting(*waiting)),
        (left > 0).then(|| words::lossy::left(left)),
    ]
    .into_iter()
    .flatten()
    .collect();

    match parts.is_empty() {
        true => words::lossy::nothing_changed(name),
        false => words::lossy::in_playlist(name, parts.join(" · ")),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use resonate_core::MediaLocation;
    use resonate_library::{Cut, Mbid, Medium, Release, ReleaseTrack};

    use super::*;
    use crate::fixtures::{ALBUM, ARTIST, TITLE, scanned, scratch, tagged_mp3, tagged_wave};

    const TAGS: [(&[u8; 4], &str); 3] =
        [(TITLE, "Echoes"), (ARTIST, "Pink Floyd"), (ALBUM, "Meddle")];

    struct Collection {
        library: Library,
        folder: PathBuf,
    }

    fn collection(files: &[(&str, Vec<u8>)]) -> Collection {
        let folder = scratch("lossy");
        for (name, bytes) in files {
            let path = folder.join(name);
            fs::create_dir_all(path.parent().expect("a folder to hold it")).expect("a folder");
            fs::write(path, bytes).expect("a file");
        }
        let folder = folder.canonicalize().expect("a canonical folder");
        let library = scanned(&folder);
        library.trash_into(Some(scratch("lossy-trash")));
        Collection { library, folder }
    }

    fn whole(collection: &Collection, name: &str) -> Cut {
        Cut::whole(MediaLocation::local(collection.folder.join(name)))
    }

    fn playlist_of(collection: &Collection, name: &str, files: &[&str]) -> Playlist {
        let cuts: Vec<Cut> = files.iter().map(|file| whole(collection, file)).collect();
        let id = collection
            .library
            .start_playlist(name, &cuts)
            .expect("a playlist");
        collection
            .library
            .playlist(id)
            .expect("a read")
            .expect("the playlist stands")
    }

    fn the_release_names(collection: &Collection, file: &str) {
        let held = collection
            .library
            .track_at(&collection.folder.join(file), None)
            .expect("a read")
            .expect("the file is held");
        let album = held.album_id.expect("filed under an album");
        collection
            .library
            .land_release(
                album,
                &Release {
                    id: Mbid::new("83d91898-7763-47d7-b03b-b92132375c47").expect("an mbid"),
                    group: None,
                    title: "Meddle".to_owned(),
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
                    media: vec![Medium {
                        position: 1,
                        format: None,
                        title: None,
                        tracks: vec![ReleaseTrack {
                            position: 1,
                            number: "1".to_owned(),
                            title: "Echoes".to_owned(),
                            artist: None,
                            recording: None,
                            track: None,
                            length: None,
                            isrcs: Vec::new(),
                            links: Vec::new(),
                        }],
                    }],
                },
            )
            .expect("the release lands");
        collection.library.rematch(album).expect("the rows match");
    }

    #[test]
    fn a_lossy_song_with_a_lossless_copy_held_is_folded_into_it_and_the_lossy_file_trashed() {
        let held = collection(&[
            ("lossy/echoes.mp3", tagged_mp3(&TAGS)),
            ("lossless/echoes.wav", tagged_wave(&TAGS)),
        ]);
        let playlist = playlist_of(&held, "Dark", &["lossy/echoes.mp3"]);
        let songs = held.library.lossy_songs_of(playlist.id).expect("a read");

        assert_eq!(Fate::of(&songs[0]), Fate::Held);
        assert!(listed("Dark", &songs).contains("replaced by the PCM copy held"));

        let settled = settle(&held.library, &songs).expect("replaced");

        assert_eq!(settled.replacing.folded, 1, "{settled:?}");
        assert_eq!(settled.replacing.wanted, 0);
        assert_eq!(settled.replacing.trashed.files, 1);
        assert!(!held.folder.join("lossy/echoes.mp3").exists());
        assert!(held.folder.join("lossless/echoes.wav").exists());
        let cuts = held.library.playlist_cuts(playlist.id).expect("a read");
        assert_eq!(cuts, [whole(&held, "lossless/echoes.wav")]);
        assert_eq!(
            told("Dark", &settled),
            "Dark: replaced 1 lossy song with the lossless copy held \u{b7} moved 1 lossy file to the trash"
        );
        assert!(
            held.library
                .lossy_songs_of(playlist.id)
                .expect("a read")
                .is_empty()
        );
    }

    #[test]
    fn a_lossy_song_with_a_release_and_no_copy_is_wanted_and_stays_where_it_is() {
        let held = collection(&[("lossy/echoes.mp3", tagged_mp3(&TAGS))]);
        the_release_names(&held, "lossy/echoes.mp3");
        let playlist = playlist_of(&held, "Dark", &["lossy/echoes.mp3"]);
        let songs = held.library.lossy_songs_of(playlist.id).expect("a read");

        assert_eq!(Fate::of(&songs[0]), Fate::Wanted);

        let settled = settle(&held.library, &songs).expect("replaced");

        assert_eq!(settled.replacing.wanted, 1, "{settled:?}");
        assert_eq!(settled.replacing.folded, 0);
        assert_eq!(settled.replacing.trashed.files, 0);
        assert!(held.folder.join("lossy/echoes.mp3").exists());
        assert_eq!(held.library.wants().expect("a read").len(), 1);
        assert!(told("Dark", &settled).contains("`resonate poll`"));
        let again = held.library.lossy_songs_of(playlist.id).expect("a read");
        assert_eq!(Fate::of(&again[0]), Fate::Waiting);
        assert_eq!(settle(&held.library, &again).expect("replaced").waiting, 1);
    }

    #[test]
    fn a_lossy_song_with_neither_is_left_and_nothing_is_trashed() {
        let held = collection(&[("lossy/echoes.mp3", tagged_mp3(&TAGS))]);
        let playlist = playlist_of(&held, "Dark", &["lossy/echoes.mp3"]);
        let songs = held.library.lossy_songs_of(playlist.id).expect("a read");

        assert_eq!(Fate::of(&songs[0]), Fate::Unmatched);

        let settled = settle(&held.library, &songs).expect("settled");

        assert_eq!(settled.replacing, Replacing::default());
        assert_eq!(settled.unreplaceable, 1);
        assert!(held.folder.join("lossy/echoes.mp3").exists());
        assert!(held.library.wants().expect("a read").is_empty());
        assert_eq!(
            told("Dark", &settled),
            "Dark: left 1 with no lossless copy held and no release to want"
        );
    }

    #[test]
    fn a_playlist_with_no_lossy_song_says_so_plainly() {
        let held = collection(&[("lossless/echoes.wav", tagged_wave(&TAGS))]);
        let playlist = playlist_of(&held, "Bright", &["lossless/echoes.wav"]);
        let songs = held.library.lossy_songs_of(playlist.id).expect("a read");

        assert!(songs.is_empty());
        assert_eq!(listed("Bright", &songs), "Bright holds no lossy songs\n");
        assert_eq!(
            told("Bright", &Settled::default()),
            "nothing in Bright could be replaced"
        );
    }

    #[test]
    fn the_listing_tells_each_song_what_replacing_would_do() {
        let held = collection(&[("lossy/echoes.mp3", tagged_mp3(&TAGS))]);
        let playlist = playlist_of(&held, "Dark", &["lossy/echoes.mp3"]);
        let songs = held.library.lossy_songs_of(playlist.id).expect("a read");

        let said = listed("Dark", &songs);

        assert!(said.starts_with("TITLE"), "{said}");
        assert!(said.contains("Echoes"));
        assert!(said.contains("Pink Floyd"));
        assert!(said.contains("no lossless copy held and no release to want"));
        assert!(said.ends_with("0 of 1 lossy song can be replaced now\n"));
    }
}
