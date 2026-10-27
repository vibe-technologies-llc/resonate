use std::{
    collections::BTreeMap,
    fs,
    io::Cursor,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use parking_lot::Mutex;
use resonate_core::{Answer, Answered, Mbid, Recording, SharedLyricsfile, SharedPrint, WholeFile};
use resonate_sync::{
    Catalog, Held, LocalListen, LocalPlaylist, LocalPlaylistId, LocalQuery, Opened, QueryBody,
    Refused, RemoteListen, Resumed, SongTags,
};

#[derive(Default)]
pub struct Shelf {
    pub inside: Mutex<Inside>,
    revision: AtomicU64,
}

#[derive(Default)]
pub struct Inside {
    pub songs: Vec<(Recording, Vec<u8>)>,
    pub declared: Vec<(Recording, SongTags)>,
    pub landed_with: Vec<(Recording, SongTags)>,
    pub playlists: BTreeMap<i64, (i64, String, Vec<Recording>)>,
    pub queries: BTreeMap<i64, (i64, QueryBody)>,
    pub without_queries: bool,
    pub favourites: Vec<Recording>,
    pub listens: Vec<LocalListen>,
    pub credited: Vec<RemoteListen>,
    pub dies_after_crediting: bool,
    pub resumed: Option<Resumed>,
    pub deleted_by_hand: Vec<String>,
    pub answers: Vec<Answered>,
    pub takes_no_answers: bool,
    pub covers: BTreeMap<String, Vec<u8>>,
    pub uncovered: Vec<Mbid>,
    pub lyricsfiles: Vec<(Recording, SharedLyricsfile)>,
    pub lacking_lyricsfiles: Vec<Recording>,
    pub prints: Vec<(WholeFile, SharedPrint)>,
    pub lacking_prints: Vec<WholeFile>,
    next_id: i64,
    next_answered: i64,
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
}

#[allow(dead_code)]
impl Shelf {
    pub fn answer_here(&self, title: &str, answer: Answer) {
        let mut inside = self.inside.lock();
        Self::answered(&mut inside, song(title), answer);
        drop(inside);
        self.moved();
    }

    pub fn answers_of(&self, title: &str) -> Vec<Answer> {
        self.inside
            .lock()
            .answers
            .iter()
            .filter(|answered| answered.recording.title == title)
            .map(|answered| answered.answer.clone())
            .collect()
    }

    fn answered(inside: &mut Inside, recording: Recording, answer: Answer) {
        inside.next_answered += 1;
        let at = inside.next_answered;
        inside.answers.retain(|held| {
            !(held.recording.names_the_same_song_as(&recording)
                && std::mem::discriminant(&held.answer) == std::mem::discriminant(&answer)
                && held.answer.file() == answer.file())
        });
        inside.answers.push(Answered {
            recording,
            at,
            answer,
        });
    }

    pub fn declare(&self, title: &str, tags: SongTags) {
        self.inside.lock().declared.push((song(title), tags));
    }

    pub fn landed_with(&self, title: &str) -> Option<SongTags> {
        self.inside
            .lock()
            .landed_with
            .iter()
            .find(|(recording, _)| recording.title == title)
            .map(|(_, tags)| tags.clone())
    }

    pub fn make_query(&self, name: &str, text: &str) -> i64 {
        let mut inside = self.inside.lock();
        inside.next_id += 1;
        let id = inside.next_id;
        inside.queries.insert(
            id,
            (
                id * 1000,
                QueryBody {
                    name: name.into(),
                    text: Some(text.into()),
                    sort: 0,
                    reading: 0,
                    rows: None,
                },
            ),
        );
        drop(inside);
        self.moved();
        id
    }

    pub fn revise_query(&self, id: i64, name: &str, text: &str) {
        let mut inside = self.inside.lock();
        let body = &mut inside.queries.get_mut(&id).expect("a query").1;
        body.name = name.into();
        body.text = Some(text.into());
        drop(inside);
        self.moved();
    }

    pub fn drop_query(&self, id: i64) {
        self.inside.lock().queries.remove(&id);
        self.moved();
    }

    pub fn query_named(&self, name: &str) -> Option<(i64, Option<String>)> {
        self.inside
            .lock()
            .queries
            .iter()
            .find(|(_, (_, body))| body.name == name)
            .map(|(id, (_, body))| (*id, body.text.clone()))
    }

    pub fn query_names(&self) -> Vec<String> {
        self.inside
            .lock()
            .queries
            .values()
            .map(|(_, body)| body.name.clone())
            .collect()
    }
}

impl Shelf {
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
        let mut inside = self.inside.lock();
        inside.playlists.remove(&at.id);
        inside.queries.remove(&at.id);
        drop(inside);
        self.moved();
        Ok(())
    }

    fn keeps_queries(&self) -> bool {
        !self.inside.lock().without_queries
    }

    fn queries(&self) -> Result<Vec<LocalQuery>, Refused> {
        Ok(self
            .inside
            .lock()
            .queries
            .iter()
            .map(|(id, (created, body))| LocalQuery {
                at: LocalPlaylistId {
                    id: *id,
                    created: *created,
                },
                body: body.clone(),
            })
            .collect())
    }

    fn write_query(
        &self,
        at: Option<LocalPlaylistId>,
        body: &QueryBody,
    ) -> Result<LocalQuery, Refused> {
        let mut inside = self.inside.lock();
        let taken = |inside: &Inside, candidate: &str| {
            inside
                .queries
                .iter()
                .any(|(id, (_, held))| held.name == candidate && at.is_none_or(|at| at.id != *id))
        };
        let mut used = body.name.clone();
        let mut copy = 2;
        while taken(&inside, &used) {
            used = format!("{} ({copy})", body.name);
            copy += 1;
        }
        let at = match at.filter(|at| inside.queries.contains_key(&at.id)) {
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
        let written = QueryBody {
            name: used,
            ..body.clone()
        };
        inside.queries.insert(at.id, (at.created, written.clone()));
        drop(inside);
        self.moved();
        Ok(LocalQuery { at, body: written })
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
        if !inside
            .credited
            .iter()
            .any(|credited| (credited.from, credited.id) == (listen.from, listen.id))
        {
            inside.credited.push(listen.clone());
        }
        if inside.dies_after_crediting {
            return Err(Refused::Unavailable);
        }
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

    fn tags(&self, recording: &Recording) -> Result<SongTags, Refused> {
        Ok(self
            .inside
            .lock()
            .declared
            .iter()
            .find(|(declared, _)| declared.names_the_same_song_as(recording))
            .map(|(_, tags)| tags.clone())
            .unwrap_or_default())
    }

    fn land(
        &self,
        recording: &Recording,
        tags: &SongTags,
        _extension: &str,
        staged: &Path,
    ) -> Result<bool, Refused> {
        let bytes = fs::read(staged).map_err(|_| Refused::Unwritable)?;
        self.inside
            .lock()
            .landed_with
            .push((recording.clone(), tags.clone()));
        self.add_song(recording.clone(), bytes);
        Ok(true)
    }

    fn takes_answers(&self) -> bool {
        !self.inside.lock().takes_no_answers
    }

    fn answers_after(&self, mark: i64, most: usize) -> Result<Vec<Answered>, Refused> {
        let mut answers = self
            .inside
            .lock()
            .answers
            .iter()
            .filter(|answered| answered.at >= mark)
            .cloned()
            .collect::<Vec<_>>();
        answers.sort_by_key(|answered| answered.at);
        answers.truncate(most);
        Ok(answers)
    }

    fn take_answer(&self, recording: &Recording, answer: &Answer) -> Result<bool, Refused> {
        let mut inside = self.inside.lock();
        let Some(own) = inside
            .songs
            .iter()
            .find(|(held, _)| held.names_the_same_song_as(recording))
            .map(|(held, _)| held.clone())
        else {
            return Ok(false);
        };
        let richer_here = inside.answers.iter().any(|held| {
            held.recording.names_the_same_song_as(recording)
                && match (&held.answer, answer) {
                    (Answer::Lyrics(held), Answer::Lyrics(told)) => held.synced && !told.synced,
                    (held, told) => held == told,
                }
        });
        if !richer_here {
            Self::answered(&mut inside, own, answer.clone());
            drop(inside);
            self.moved();
        }
        Ok(true)
    }

    fn covers_wanted(&self, most: usize) -> Result<Vec<Mbid>, Refused> {
        let inside = self.inside.lock();
        Ok(inside
            .uncovered
            .iter()
            .filter(|release| !inside.covers.contains_key(release.as_str()))
            .take(most)
            .cloned()
            .collect())
    }

    fn cover(&self, release: &Mbid) -> Result<Option<Vec<u8>>, Refused> {
        Ok(self.inside.lock().covers.get(release.as_str()).cloned())
    }

    fn take_cover(&self, release: &Mbid, picture: &[u8]) -> Result<bool, Refused> {
        let mut inside = self.inside.lock();
        if !inside.uncovered.contains(release) || inside.covers.contains_key(release.as_str()) {
            return Ok(false);
        }
        inside
            .covers
            .insert(release.as_str().to_owned(), picture.to_vec());
        Ok(true)
    }

    fn lyricsfiles_wanted(&self, skip: usize, most: usize) -> Result<Vec<Recording>, Refused> {
        let inside = self.inside.lock();
        Ok(inside
            .lacking_lyricsfiles
            .iter()
            .filter(|recording| {
                !inside
                    .lyricsfiles
                    .iter()
                    .any(|(held, _)| held.names_the_same_song_as(recording))
            })
            .skip(skip)
            .take(most)
            .cloned()
            .collect())
    }

    fn lyricsfile(&self, recording: &Recording) -> Result<Option<SharedLyricsfile>, Refused> {
        Ok(self
            .inside
            .lock()
            .lyricsfiles
            .iter()
            .find(|(held, _)| held.names_the_same_song_as(recording))
            .map(|(_, lyricsfile)| lyricsfile.clone()))
    }

    fn take_lyricsfile(
        &self,
        recording: &Recording,
        lyricsfile: &SharedLyricsfile,
    ) -> Result<bool, Refused> {
        let mut inside = self.inside.lock();
        let lacking = inside
            .lacking_lyricsfiles
            .iter()
            .any(|held| held.names_the_same_song_as(recording));
        let held = inside
            .lyricsfiles
            .iter()
            .any(|(held, _)| held.names_the_same_song_as(recording));
        if !lacking || held {
            return Ok(false);
        }
        inside
            .lyricsfiles
            .push((recording.clone(), lyricsfile.clone()));
        Ok(true)
    }

    fn prints_wanted(&self, skip: usize, most: usize) -> Result<Vec<WholeFile>, Refused> {
        let inside = self.inside.lock();
        Ok(inside
            .lacking_prints
            .iter()
            .filter(|file| !inside.prints.iter().any(|(held, _)| held == *file))
            .skip(skip)
            .take(most)
            .copied()
            .collect())
    }

    fn print(&self, file: WholeFile) -> Result<Option<SharedPrint>, Refused> {
        Ok(self
            .inside
            .lock()
            .prints
            .iter()
            .find(|(held, _)| *held == file)
            .map(|(_, print)| print.clone()))
    }

    fn take_print(&self, file: WholeFile, print: &SharedPrint) -> Result<bool, Refused> {
        let mut inside = self.inside.lock();
        let lacking = inside.lacking_prints.contains(&file);
        let held = inside.prints.iter().any(|(held, _)| *held == file);
        if !lacking || held {
            return Ok(false);
        }
        inside.prints.push((file, print.clone()));
        Ok(true)
    }

    fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }
}
