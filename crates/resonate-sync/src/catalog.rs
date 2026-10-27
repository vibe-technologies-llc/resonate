use std::{
    io::{Read, Seek},
    ops::Range,
    path::Path,
    time::Duration,
};

use resonate_core::{Answer, Answered, Mbid, Recording, SharedLyricsfile, SharedPrint, WholeFile};

use crate::{change::QueryBody, key::DeviceId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalPlaylistId {
    pub id: i64,
    pub created: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalPlaylist {
    pub at: LocalPlaylistId,
    pub name: String,
    pub entries: Vec<Option<Recording>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalQuery {
    pub at: LocalPlaylistId,
    pub body: QueryBody,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalListen {
    pub id: u64,
    pub recording: Recording,
    pub at: i64,
    pub heard: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteListen {
    pub from: DeviceId,
    pub id: u64,
    pub recording: Recording,
    pub at: i64,
    pub heard: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resumed {
    pub rows: Vec<Recording>,
    pub at: u32,
    pub position: Duration,
    pub playing: bool,
}

const HANDED_OFF_AT_MOST: usize = 500;

impl Resumed {
    pub fn window_around(playing: usize, rows: usize) -> Range<usize> {
        let playing = playing.min(rows.saturating_sub(1));
        let first = playing.saturating_sub(HANDED_OFF_AT_MOST / 2);

        first..rows.min(first + HANDED_OFF_AT_MOST)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub recording: Recording,
    pub size: u64,
    pub extension: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SongTags {
    pub album_artist: Option<String>,
    pub date: Option<String>,
    pub track_number: Option<u32>,
    pub track_total: Option<u32>,
    pub disc_number: Option<u32>,
    pub disc_total: Option<u32>,
    pub compilation: bool,
}

pub trait ReadSeek: Read + Seek + Send {}

impl<T: Read + Seek + Send> ReadSeek for T {}

pub struct Opened {
    pub reader: Box<dyn ReadSeek>,
    pub size: u64,
    pub extension: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refused {
    Unavailable,
    NotHere,
    Unwritable,
}

pub trait Catalog: Send + Sync + 'static {
    fn playlists(&self) -> Result<Vec<LocalPlaylist>, Refused>;

    fn write_playlist(
        &self,
        at: Option<LocalPlaylistId>,
        name: &str,
        entries: &[Recording],
    ) -> Result<LocalPlaylist, Refused>;

    fn remove_playlist(&self, at: LocalPlaylistId) -> Result<(), Refused>;

    fn keeps_queries(&self) -> bool {
        false
    }

    fn queries(&self) -> Result<Vec<LocalQuery>, Refused> {
        Ok(Vec::new())
    }

    fn write_query(
        &self,
        at: Option<LocalPlaylistId>,
        body: &QueryBody,
    ) -> Result<LocalQuery, Refused> {
        let _ = (at, body);
        Err(Refused::Unavailable)
    }

    fn favourites(&self) -> Result<Vec<Recording>, Refused>;

    fn favour(&self, recording: &Recording, favoured: bool) -> Result<bool, Refused>;

    fn listens_after(&self, mark: u64, most: usize) -> Result<Vec<LocalListen>, Refused>;

    fn credit(&self, listen: &RemoteListen) -> Result<bool, Refused>;

    fn resumption(&self) -> Result<Option<Resumed>, Refused>;

    fn holds(&self, recording: &Recording) -> Result<bool, Refused>;

    fn held_as(&self, recording: &Recording) -> Result<Option<Recording>, Refused> {
        let _ = recording;
        Ok(None)
    }

    fn holdings(&self) -> Result<Vec<Held>, Refused>;

    fn open(&self, recording: &Recording) -> Result<Option<Opened>, Refused>;

    fn tags(&self, recording: &Recording) -> Result<SongTags, Refused> {
        let _ = recording;
        Ok(SongTags::default())
    }

    fn land(
        &self,
        recording: &Recording,
        tags: &SongTags,
        extension: &str,
        staged: &Path,
    ) -> Result<bool, Refused>;

    fn takes_answers(&self) -> bool {
        false
    }

    fn answers_after(&self, mark: i64, most: usize) -> Result<Vec<Answered>, Refused> {
        let _ = (mark, most);
        Ok(Vec::new())
    }

    fn take_answer(&self, recording: &Recording, answer: &Answer) -> Result<bool, Refused> {
        let _ = (recording, answer);
        Ok(false)
    }

    fn covers_wanted(&self, most: usize) -> Result<Vec<Mbid>, Refused> {
        let _ = most;
        Ok(Vec::new())
    }

    fn cover(&self, release: &Mbid) -> Result<Option<Vec<u8>>, Refused> {
        let _ = release;
        Ok(None)
    }

    fn take_cover(&self, release: &Mbid, picture: &[u8]) -> Result<bool, Refused> {
        let _ = (release, picture);
        Ok(false)
    }

    fn lyricsfiles_wanted(&self, skip: usize, most: usize) -> Result<Vec<Recording>, Refused> {
        let _ = (skip, most);
        Ok(Vec::new())
    }

    fn lyricsfile(&self, recording: &Recording) -> Result<Option<SharedLyricsfile>, Refused> {
        let _ = recording;
        Ok(None)
    }

    fn take_lyricsfile(
        &self,
        recording: &Recording,
        lyricsfile: &SharedLyricsfile,
    ) -> Result<bool, Refused> {
        let _ = (recording, lyricsfile);
        Ok(false)
    }

    fn prints_wanted(&self, skip: usize, most: usize) -> Result<Vec<WholeFile>, Refused> {
        let _ = (skip, most);
        Ok(Vec::new())
    }

    fn print(&self, file: WholeFile) -> Result<Option<SharedPrint>, Refused> {
        let _ = file;
        Ok(None)
    }

    fn take_print(&self, file: WholeFile, print: &SharedPrint) -> Result<bool, Refused> {
        let _ = (file, print);
        Ok(false)
    }

    fn revision(&self) -> u64;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_queue_is_handed_off_whole() {
        assert_eq!(Resumed::window_around(3, 10), 0..10);
        assert_eq!(Resumed::window_around(0, 0), 0..0);
    }

    #[test]
    fn a_playhead_in_the_first_half_page_keeps_the_first_page() {
        assert_eq!(Resumed::window_around(200, 2_000), 0..HANDED_OFF_AT_MOST);
    }

    #[test]
    fn a_playhead_past_the_first_page_is_inside_the_window() {
        let window = Resumed::window_around(900, 2_000);

        assert!(window.contains(&900));
        assert_eq!(window.len(), HANDED_OFF_AT_MOST);
    }

    #[test]
    fn a_playhead_near_the_end_keeps_a_full_window_ending_with_the_queue() {
        let window = Resumed::window_around(1_999, 2_000);

        assert!(window.contains(&1_999));
        assert_eq!(window.end, 2_000);
        assert!(window.len() <= HANDED_OFF_AT_MOST);
    }

    #[test]
    fn a_playhead_past_the_end_is_clamped_to_the_last_row() {
        assert!(Resumed::window_around(5_000, 10).contains(&9));
    }
}
