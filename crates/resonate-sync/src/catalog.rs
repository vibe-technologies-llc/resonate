use std::{
    io::{Read, Seek},
    ops::Range,
    path::Path,
    time::Duration,
};

use resonate_core::{
    Answer, Answered, AnswersMark, Mbid, Recording, SharedLyricsfile, SharedPrint, WholeFile,
};

use crate::{change::QueryBody, key::DeviceId};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
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

    pub(crate) fn fitted(
        self,
        mut fits: impl FnMut(&Self) -> crate::Result<bool>,
    ) -> crate::Result<Self> {
        if self.rows.len() <= 1 || fits(&self)? {
            return Ok(self);
        }

        let mut best = self.around_the_playhead(1);
        let mut fewest_refused = self.rows.len();
        let mut most_fitting = 1;

        while fewest_refused - most_fitting > 1 {
            let tried = most_fitting + (fewest_refused - most_fitting) / 2;
            let candidate = self.around_the_playhead(tried);

            if fits(&candidate)? {
                best = candidate;
                most_fitting = tried;
            } else {
                fewest_refused = tried;
            }
        }
        Ok(best)
    }

    fn around_the_playhead(&self, kept: usize) -> Self {
        let last = self.rows.len().saturating_sub(1);
        let playing = usize::try_from(self.at).unwrap_or(usize::MAX).min(last);
        let neighbours = kept.saturating_sub(1);

        let following = (last - playing).min(neighbours.div_ceil(2));
        let preceding = playing.min(neighbours - following);
        let following = (last - playing).min(neighbours - preceding);
        let first = playing - preceding;

        Self {
            rows: self.rows[first..=playing + following].to_vec(),
            at: self.at - u32::try_from(first).unwrap_or(u32::MAX),
            position: self.position,
            playing: self.playing,
        }
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldShare {
    pub code: String,
    pub server: Option<String>,
    pub member: [u8; 16],
    pub personal: [u8; 32],
    pub at: Option<LocalPlaylistId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeletedEverywhere {
    pub id: i64,
    pub recording: Recording,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldSince {
    pub at: i64,
    pub id: i64,
    pub recording: Recording,
    pub received_under: Vec<String>,
    pub file: Option<WholeFile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeldMark {
    pub at: i64,
    pub after: Option<i64>,
}

impl HeldMark {
    pub const fn past(song: &HeldSince) -> Self {
        Self {
            at: song.at,
            after: Some(song.id),
        }
    }

    pub const fn from_the_second_of(song: &HeldSince) -> Self {
        Self {
            at: song.at,
            after: None,
        }
    }

    pub fn lists(self, song: &HeldSince) -> bool {
        (song.at, Some(song.id)) > (self.at, self.after)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SongsHeldAfter {
    Untracked,
    Listed(Vec<HeldSince>),
}

pub trait ReadSeek: Read + Seek + Send {}

impl<T: Read + Seek + Send> ReadSeek for T {}

pub struct Opened {
    pub reader: Box<dyn ReadSeek>,
    pub size: u64,
    pub extension: String,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
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

    fn deleted_here(&self, recording: &Recording) -> Result<bool, Refused> {
        let _ = recording;
        Ok(false)
    }

    fn forgive_deletion(&self, recording: &Recording) -> Result<(), Refused> {
        let _ = recording;
        Ok(())
    }

    fn forgive_deletions(&self) -> Result<(), Refused> {
        Ok(())
    }

    fn deleted_everywhere(&self) -> Result<Vec<DeletedEverywhere>, Refused> {
        Ok(Vec::new())
    }

    fn deletions_told(&self, told: &[i64]) -> Result<(), Refused> {
        let _ = told;
        Ok(())
    }

    fn takes_deletions(&self) -> bool {
        false
    }

    fn take_deletion(&self, recording: &Recording, before: i64) -> Result<u64, Refused> {
        let _ = (recording, before);
        Ok(0)
    }

    fn deletion_waits(&self, recording: &Recording) -> Result<bool, Refused> {
        let _ = recording;
        Ok(false)
    }

    fn withdraw_deletion(&self, recording: &Recording) -> Result<(), Refused> {
        let _ = recording;
        Ok(())
    }

    fn added_by_hand_since(&self, recording: &Recording, since: i64) -> Result<bool, Refused> {
        let _ = (recording, since);
        Ok(false)
    }

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

    fn answers_after(&self, mark: AnswersMark, most: usize) -> Result<Vec<Answered>, Refused> {
        let _ = (mark, most);
        Ok(Vec::new())
    }

    fn take_answer(&self, recording: &Recording, answer: &Answer) -> Result<bool, Refused> {
        let _ = (recording, answer);
        Ok(false)
    }

    fn songs_held_after(&self, mark: HeldMark, most: usize) -> Result<SongsHeldAfter, Refused> {
        let _ = (mark, most);
        Ok(SongsHeldAfter::Untracked)
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

    fn hides_as_shared(&self, at: LocalPlaylistId) -> Result<bool, Refused> {
        let _ = at;
        Ok(false)
    }

    fn shares_here(&self) -> Result<Option<Vec<HeldShare>>, Refused> {
        Ok(None)
    }

    fn take_share(
        &self,
        share: &HeldShare,
        taking_over: Option<LocalPlaylistId>,
    ) -> Result<(), Refused> {
        let _ = (share, taking_over);
        Ok(())
    }

    fn drop_share(&self, code: &str) -> Result<(), Refused> {
        let _ = code;
        Ok(())
    }

    fn organise_layout(&self) -> Option<String> {
        None
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

    fn queue(rows: u32, at: u32) -> Resumed {
        Resumed {
            rows: (0..rows)
                .map(|number| Recording {
                    mbid: None,
                    title: number.to_string(),
                    artist: String::new(),
                    album: String::new(),
                    length: None,
                })
                .collect(),
            at,
            position: Duration::from_secs(90),
            playing: true,
        }
    }

    fn titles(resumed: &Resumed) -> Vec<u32> {
        resumed
            .rows
            .iter()
            .map(|row| row.title.parse().expect("a number"))
            .collect()
    }

    #[test]
    fn a_handoff_that_fits_is_kept_whole() {
        let fitted = queue(10, 4).fitted(|_| Ok(true)).expect("fitted");

        assert_eq!(fitted, queue(10, 4));
    }

    #[test]
    fn a_handoff_too_large_keeps_the_most_rows_that_fit_around_the_playhead_the_extra_one_following()
     {
        let fitted = queue(100, 50)
            .fitted(|candidate| Ok(candidate.rows.len() <= 6))
            .expect("fitted");

        assert_eq!(titles(&fitted), [48, 49, 50, 51, 52, 53]);
        assert_eq!(fitted.at, 2);
        assert_eq!(fitted.position, Duration::from_secs(90));
        assert!(fitted.playing);
    }

    #[test]
    fn rows_one_side_lacks_are_taken_from_the_other() {
        let at_the_start = queue(100, 1)
            .fitted(|candidate| Ok(candidate.rows.len() <= 5))
            .expect("fitted");
        let at_the_end = queue(100, 98)
            .fitted(|candidate| Ok(candidate.rows.len() <= 5))
            .expect("fitted");

        assert_eq!(titles(&at_the_start), [0, 1, 2, 3, 4]);
        assert_eq!(at_the_start.at, 1);
        assert_eq!(titles(&at_the_end), [95, 96, 97, 98, 99]);
        assert_eq!(at_the_end.at, 3);
    }

    #[test]
    fn the_playing_row_is_kept_even_where_nothing_fits() {
        let fitted = queue(100, 70).fitted(|_| Ok(false)).expect("fitted");

        assert_eq!(titles(&fitted), [70]);
        assert_eq!(fitted.at, 0);
    }

    #[test]
    fn a_playhead_past_the_end_stays_past_the_rows_kept() {
        let fitted = queue(100, 100)
            .fitted(|candidate| Ok(candidate.rows.len() <= 3))
            .expect("fitted");

        assert_eq!(titles(&fitted), [97, 98, 99]);
        assert_eq!(fitted.at, 3);
    }
}
