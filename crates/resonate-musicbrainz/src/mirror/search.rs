use std::{collections::BTreeSet, time::Duration};

use resonate_core::{Mbid, folded_letters, titles, words_of_a_name};
use resonate_library::{
    AlbumMatch, ArtistMatch, Credit, GroupAsked, GroupMatch, RecordingAsked, RecordingMatch,
    Release, ReleaseAsked, ReleaseMatch, SongsAsked, Wording,
};

use super::Mirror;
use crate::Result;

pub const RECORDINGS_FOUND_AT_MOST: usize = 25;
pub const SONGS_FOUND_AT_MOST: usize = 100;
pub const RELEASES_FOUND_AT_MOST: usize = 10;
pub const GROUPS_FOUND_AT_MOST: usize = 5;
pub const ALBUMS_FOUND_AT_MOST: usize = 25;
pub const ARTISTS_FOUND_AT_MOST: usize = 5;
pub const CANDIDATES_WEIGHED: usize = 64;
pub const LENGTH_MAY_DIFFER_BY: Duration = Duration::from_secs(10);
const WHOLE: f64 = 1.0;
const BARE_ALIKE: f64 = 0.96;
const WORDS_SHARED_AT_MOST: f64 = 0.9;
const RECORDING_WORDS: &str = "recording_words";
const RELEASE_WORDS: &str = "release_words";
const GROUP_WORDS: &str = "group_words";
const ARTIST_WORDS: &str = "artist_words";

fn folded(text: &str) -> String {
    folded_letters(text.trim())
}

fn words_shared(asked: &[String], found: &[String]) -> f64 {
    let asked: BTreeSet<&String> = asked.iter().collect();
    let found: BTreeSet<&String> = found.iter().collect();
    let total = asked.len() + found.len();
    if total == 0 {
        return 0.0;
    }
    let shared = asked.intersection(&found).count();
    2.0 * shared as f64 / total as f64
}

pub fn agreement(asked: &str, found: &str) -> f64 {
    if folded(asked) == folded(found) {
        return WHOLE;
    }
    if folded(titles::bare(asked)) == folded(titles::bare(found)) {
        return BARE_ALIKE;
    }
    words_shared(&words_of_a_name(asked), &words_of_a_name(found)) * WORDS_SHARED_AT_MOST
}

fn credited_as(credit: &[Credit]) -> String {
    credit
        .iter()
        .map(|named| format!("{}{}", named.name, named.joined_by))
        .collect()
}

pub fn credit_agreement(name: Option<&str>, mbid: Option<&Mbid>, credit: &[Credit]) -> f64 {
    if let Some(mbid) = mbid
        && credit.iter().any(|named| named.mbid.as_ref() == Some(mbid))
    {
        return WHOLE;
    }
    let Some(name) = name else {
        return if mbid.is_some() { 0.0 } else { WHOLE };
    };
    let whole = agreement(name, &credited_as(credit));
    credit
        .iter()
        .map(|named| agreement(name, &named.name))
        .fold(whole, f64::max)
}

pub fn scored(agreement: f64) -> u8 {
    (agreement.clamp(0.0, 1.0) * 100.0).floor() as u8
}

fn all_of(words: &[String]) -> Option<String> {
    (!words.is_empty()).then(|| {
        words
            .iter()
            .map(|word| format!("\"{}\"", word.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ")
    })
}

fn words_of_credit(credit: &[Credit]) -> Vec<String> {
    credit
        .iter()
        .flat_map(|named| words_of_a_name(&named.name))
        .collect()
}

pub(crate) fn words_of_a_credited(title: &str, credit: &[Credit]) -> String {
    let mut words = words_of_a_name(title);
    words.extend(words_of_credit(credit));
    words.join(" ")
}

fn any_of(words: &[String]) -> Option<String> {
    (!words.is_empty()).then(|| {
        let alternatives = words
            .iter()
            .map(|word| format!("\"{}\"", word.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" OR ");
        format!("({alternatives})")
    })
}

fn asked_queries(title: &str, artist: Option<&str>, wording: Wording) -> Vec<String> {
    let titled = words_of_a_name(title);
    let credited = artist.map(words_of_a_name).unwrap_or_default();
    let Some(title_alone) = all_of(&titled) else {
        return Vec::new();
    };
    match (wording, any_of(&credited)) {
        (Wording::Phrase, _) => {
            let mut every = titled;
            every.extend(credited);
            all_of(&every).into_iter().collect()
        }
        (Wording::Words, Some(any_artist)) => {
            vec![format!("{title_alone} AND {any_artist}"), title_alone]
        }
        (Wording::Words, None) => vec![title_alone],
    }
}

fn lengths_agree(asked: Option<Duration>, found: Option<Duration>) -> bool {
    match (asked, found) {
        (Some(asked), Some(found)) => asked.abs_diff(found) <= LENGTH_MAY_DIFFER_BY,
        _ => true,
    }
}

fn best_first<T>(found: &mut Vec<(u8, T)>, most: usize) -> Vec<T> {
    found.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    found.drain(..).take(most).map(|(_, found)| found).collect()
}

impl Mirror {
    pub fn find_recording(&self, asked: &RecordingAsked) -> Result<Vec<RecordingMatch>> {
        let queries = asked_queries(&asked.title, asked.artist.as_deref(), asked.wording);
        let phrase = asked.wording == Wording::Phrase;
        let mut found = Vec::new();
        for id in self.matched_first(RECORDING_WORDS, &queries)? {
            let Some(recording) = self.recording_at(id)? else {
                continue;
            };
            if phrase && !lengths_agree(asked.length, recording.length) {
                continue;
            }
            if phrase
                && let Some(release) = &asked.release
                && !recording
                    .releases
                    .iter()
                    .any(|on| folded(&on.title) == folded(release))
            {
                continue;
            }
            let score = scored(
                agreement(&asked.title, &recording.title)
                    * credit_agreement(
                        asked.artist.as_deref(),
                        asked.artist_mbid.as_ref(),
                        &recording.credit,
                    ),
            );
            let mut matched = RecordingMatch::from(recording);
            matched.score = score;
            found.push((score, matched));
        }
        Ok(best_first(&mut found, RECORDINGS_FOUND_AT_MOST))
    }

    pub fn find_songs(&self, asked: &SongsAsked) -> Result<Vec<RecordingMatch>> {
        if let Some(by) = &asked.by {
            let read = self.find_recording(&RecordingAsked {
                title: by.title.clone(),
                artist: Some(by.artist.clone()),
                artist_mbid: None,
                release: None,
                length: None,
                wording: Wording::Phrase,
            })?;
            if !read.is_empty() {
                return Ok(read);
            }
        }
        let words = words_of_a_name(&asked.words);
        let Some(query) = all_of(&words) else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        for id in self.matched(RECORDING_WORDS, &query, SONGS_FOUND_AT_MOST)? {
            let Some(recording) = self.recording_at(id)? else {
                continue;
            };
            let mut held = words_of_a_name(&recording.title);
            held.extend(words_of_credit(&recording.credit));
            let score = scored(words_shared(&words, &held));
            let mut matched = RecordingMatch::from(recording);
            matched.score = score;
            found.push((score, matched));
        }
        Ok(best_first(&mut found, SONGS_FOUND_AT_MOST))
    }

    pub fn find_release(&self, asked: &ReleaseAsked) -> Result<Vec<ReleaseMatch>> {
        let queries = asked_queries(&asked.title, asked.artist.as_deref(), asked.wording);
        let phrase = asked.wording == Wording::Phrase;
        let mut found = Vec::new();
        for id in self.matched_first(RELEASE_WORDS, &queries)? {
            let Some(doc) = self.release_at(id)? else {
                continue;
            };
            let release = doc.release;
            if phrase && !pressed_as_asked(asked, &release) {
                continue;
            }
            let score = scored(
                agreement(&asked.title, &release.title)
                    * credit_agreement(
                        asked.artist.as_deref(),
                        asked.artist_mbid.as_ref(),
                        &release.credit,
                    ),
            );
            found.push((
                score,
                ReleaseMatch {
                    track_count: Some(release.track_count()),
                    release: release.id,
                    group: release.group,
                    score,
                    title: release.title,
                    credit: release.credit,
                    date: release.date,
                },
            ));
        }
        Ok(best_first(&mut found, RELEASES_FOUND_AT_MOST))
    }

    pub fn find_release_group(&self, asked: &GroupAsked) -> Result<Vec<GroupMatch>> {
        let queries = asked_queries(&asked.title, asked.artist.as_deref(), asked.wording);
        let phrase = asked.wording == Wording::Phrase;
        let mut found = Vec::new();
        for id in self.matched_first(GROUP_WORDS, &queries)? {
            let Some(doc) = self.group_at(id)? else {
                continue;
            };
            let group = doc.group;
            if phrase
                && let Some(year) = asked.year
                && !group
                    .first_released
                    .as_deref()
                    .is_some_and(|date| date.starts_with(&format!("{year:04}")))
            {
                continue;
            }
            let score = scored(
                agreement(&asked.title, &group.title)
                    * credit_agreement(
                        asked.artist.as_deref(),
                        asked.artist_mbid.as_ref(),
                        &group.credit,
                    ),
            );
            found.push((
                score,
                GroupMatch {
                    group: group.id,
                    score,
                    title: group.title,
                    credit: group.credit,
                },
            ));
        }
        Ok(best_first(&mut found, GROUPS_FOUND_AT_MOST))
    }

    pub fn find_albums(&self, words: &str) -> Result<Vec<AlbumMatch>> {
        let asked = words_of_a_name(words);
        let Some(query) = all_of(&asked) else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        for id in self.matched(GROUP_WORDS, &query, CANDIDATES_WEIGHED)? {
            let Some(doc) = self.group_at(id)? else {
                continue;
            };
            let group = doc.group;
            let mut held = words_of_a_name(&group.title);
            held.extend(words_of_credit(&group.credit));
            let score = scored(words_shared(&asked, &held));
            found.push((
                score,
                AlbumMatch {
                    group: group.id,
                    score,
                    title: group.title,
                    credit: group.credit,
                    kind: group.kind,
                    secondary: doc.secondary,
                    first_released: group.first_released,
                },
            ));
        }
        Ok(best_first(&mut found, ALBUMS_FOUND_AT_MOST))
    }

    pub fn find_artist(&self, name: &str) -> Result<Vec<ArtistMatch>> {
        let Some(query) = all_of(&words_of_a_name(name)) else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        for id in self.matched(ARTIST_WORDS, &query, CANDIDATES_WEIGHED)? {
            let Some(doc) = self.artist_at(id)? else {
                continue;
            };
            let artist = doc.artist;
            let named = artist
                .aliases
                .iter()
                .chain(artist.sort_name.iter())
                .map(|alias| agreement(name, alias))
                .fold(agreement(name, &artist.name), f64::max);
            let score = scored(named);
            found.push((
                score,
                ArtistMatch {
                    mbid: artist.mbid,
                    name: artist.name,
                    score,
                    kind: artist.kind,
                    disambiguation: artist.disambiguation,
                    aliases: artist.aliases,
                },
            ));
        }
        Ok(best_first(&mut found, ARTISTS_FOUND_AT_MOST))
    }
}

impl Mirror {
    fn matched_first(&self, table: &str, queries: &[String]) -> Result<Vec<i64>> {
        for query in queries {
            let matched = self.matched(table, query, CANDIDATES_WEIGHED)?;
            if !matched.is_empty() {
                return Ok(matched);
            }
        }
        Ok(Vec::new())
    }
}

fn pressed_as_asked(asked: &ReleaseAsked, release: &Release) -> bool {
    let barcode_agrees = match (&asked.barcode, &release.barcode) {
        (Some(asked), Some(pressed)) => {
            resonate_library::Barcode::new(asked).is_none_or(|asked| asked.names(pressed))
        }
        (Some(_), None) => false,
        (None, _) => true,
    };
    let number_agrees = match (&asked.catalog_number, &release.catalog_number) {
        (Some(asked), Some(pressed)) => folded(asked) == folded(pressed),
        (Some(_), None) => false,
        (None, _) => true,
    };
    barcode_agrees && number_agrees
}
