use std::{
    collections::BTreeMap,
    fs,
    io::Cursor,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use parking_lot::Mutex;
use resonate_core::Recording;
use resonate_sync::{
    Catalog, Held, LocalListen, LocalPlaylist, LocalPlaylistId, Opened, Refused, RemoteListen,
    Resumed,
};

#[derive(Default)]
pub struct Shelf {
    pub inside: Mutex<Inside>,
    revision: AtomicU64,
}

#[derive(Default)]
pub struct Inside {
    pub songs: Vec<(Recording, Vec<u8>)>,
    pub playlists: BTreeMap<i64, (i64, String, Vec<Recording>)>,
    pub favourites: Vec<Recording>,
    pub listens: Vec<LocalListen>,
    pub credited: Vec<RemoteListen>,
    pub resumed: Option<Resumed>,
    pub deleted_by_hand: Vec<String>,
    next_id: i64,
}

pub fn song(title: &str) -> Recording {
    Recording {
        mbid: None,
        title: title.into(),
        artist: "Artist".into(),
        album: "Album".into(),
        length: Some(Duration::from_secs(180)),
    }
}

impl Shelf {
    pub fn holding(titles: &[&str]) -> Self {
        let shelf = Self::default();
        for title in titles {
            shelf.add_song(song(title), title.as_bytes().repeat(1000));
        }
        shelf
    }

    pub fn add_song(&self, recording: Recording, bytes: Vec<u8>) {
        self.inside.lock().songs.push((recording, bytes));
        self.moved();
    }

    pub fn moved(&self) {
        self.revision.fetch_add(1, Ordering::SeqCst);
    }

    pub fn make_playlist(&self, name: &str, titles: &[&str]) -> i64 {
        let mut inside = self.inside.lock();
        inside.next_id += 1;
        let id = inside.next_id;
        let entries = titles.iter().map(|title| song(title)).collect();
        inside
            .playlists
            .insert(id, (id * 1000, name.into(), entries));
        drop(inside);
        self.moved();
        id
    }

    pub fn rename_playlist(&self, id: i64, name: &str) {
        self.inside
            .lock()
            .playlists
            .get_mut(&id)
            .expect("a playlist")
            .1 = name.into();
        self.moved();
    }

    pub fn playlist_named(&self, name: &str) -> Option<(i64, Vec<String>)> {
        let inside = self.inside.lock();
        inside
            .playlists
            .iter()
            .find(|(_, (_, named, _))| named == name)
            .map(|(id, (_, _, entries))| {
                (
                    *id,
                    entries.iter().map(|entry| entry.title.clone()).collect(),
                )
            })
    }

    pub fn favour_here(&self, title: &str, on: bool) {
        let mut inside = self.inside.lock();
        inside
            .favourites
            .retain(|recording| recording.title != title);
        if on {
            inside.favourites.push(song(title));
        }
        drop(inside);
        self.moved();
    }

    pub fn favourite_titles(&self) -> Vec<String> {
        let mut titles = self
            .inside
            .lock()
            .favourites
            .iter()
            .map(|recording| recording.title.clone())
            .collect::<Vec<_>>();
        titles.sort();
        titles
    }

    fn holds_song(inside: &Inside, recording: &Recording) -> bool {
        inside
            .songs
            .iter()
            .any(|(held, _)| held.names_the_same_song_as(recording))
    }
}

impl Catalog for Shelf {
    fn playlists(&self) -> Result<Vec<LocalPlaylist>, Refused> {
        let inside = self.inside.lock();
        Ok(inside
            .playlists
            .iter()
            .map(|(id, (created, name, entries))| LocalPlaylist {
                at: LocalPlaylistId {
                    id: *id,
                    created: *created,
                },
                name: name.clone(),
                entries: entries.iter().cloned().map(Some).collect(),
            })
            .collect())
    }

    fn write_playlist(
        &self,
        at: Option<LocalPlaylistId>,
        name: &str,
        entries: &[Recording],
    ) -> Result<LocalPlaylist, Refused> {
        let mut inside = self.inside.lock();
        let held = entries
            .iter()
            .filter(|entry| Self::holds_song(&inside, entry))
            .cloned()
            .collect::<Vec<_>>();
        let taken = |inside: &Inside, candidate: &str| {
            inside
                .playlists
                .iter()
                .any(|(id, (_, named, _))| named == candidate && at.is_none_or(|at| at.id != *id))
        };
        let mut used = name.to_owned();
        let mut copy = 2;
        while taken(&inside, &used) {
            used = format!("{name} ({copy})");
            copy += 1;
        }
        let at = match at.filter(|at| inside.playlists.contains_key(&at.id)) {
            Some(at) => at,
            None => {
                inside.next_id += 1;
                let id = inside.next_id;
                LocalPlaylistId {
                    id,
                    created: id * 1000,
                }
            }
        };
        inside
            .playlists
            .insert(at.id, (at.created, used.clone(), held.clone()));
        drop(inside);
        self.moved();
        Ok(LocalPlaylist {
            at,
            name: used,
            entries: held.into_iter().map(Some).collect(),
        })
    }

    fn remove_playlist(&self, at: LocalPlaylistId) -> Result<(), Refused> {
        self.inside.lock().playlists.remove(&at.id);
        self.moved();
        Ok(())
    }

    fn favourites(&self) -> Result<Vec<Recording>, Refused> {
        Ok(self.inside.lock().favourites.clone())
    }

    fn favour(&self, recording: &Recording, favoured: bool) -> Result<bool, Refused> {
        let mut inside = self.inside.lock();
        if !Self::holds_song(&inside, recording) {
            return Ok(false);
        }
        inside
            .favourites
            .retain(|held| !held.names_the_same_song_as(recording));
        if favoured {
            inside.favourites.push(recording.clone());
        }
        drop(inside);
        self.moved();
        Ok(true)
    }

    fn listens_after(&self, mark: u64, most: usize) -> Result<Vec<LocalListen>, Refused> {
        Ok(self
            .inside
            .lock()
            .listens
            .iter()
            .filter(|listen| listen.id > mark)
            .take(most)
            .cloned()
            .collect())
    }

    fn credit(&self, listen: &RemoteListen) -> Result<bool, Refused> {
        let mut inside = self.inside.lock();
        if !Self::holds_song(&inside, &listen.recording) {
            return Ok(false);
        }
        inside.credited.push(listen.clone());
        Ok(true)
    }

    fn resumption(&self) -> Result<Option<Resumed>, Refused> {
        Ok(self.inside.lock().resumed.clone())
    }

    fn holds(&self, recording: &Recording) -> Result<bool, Refused> {
        let inside = self.inside.lock();
        Ok(Self::holds_song(&inside, recording)
            && !inside.deleted_by_hand.contains(&recording.title))
    }

    fn holdings(&self) -> Result<Vec<Held>, Refused> {
        Ok(self
            .inside
            .lock()
            .songs
            .iter()
            .map(|(recording, bytes)| Held {
                recording: recording.clone(),
                size: u64::try_from(bytes.len()).expect("a size"),
                extension: "flac".into(),
            })
            .collect())
    }

    fn open(&self, recording: &Recording) -> Result<Option<Opened>, Refused> {
        let inside = self.inside.lock();
        Ok(inside
            .songs
            .iter()
            .find(|(held, _)| held.names_the_same_song_as(recording))
            .map(|(_, bytes)| Opened {
                size: u64::try_from(bytes.len()).expect("a size"),
                reader: Box::new(Cursor::new(bytes.clone())),
                extension: "flac".into(),
            }))
    }

    fn land(
        &self,
        recording: &Recording,
        _extension: &str,
        staged: &Path,
    ) -> Result<bool, Refused> {
        let bytes = fs::read(staged).map_err(|_| Refused::Unwritable)?;
        self.add_song(recording.clone(), bytes);
        Ok(true)
    }

    fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }
}
