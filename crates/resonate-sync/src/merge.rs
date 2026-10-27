use std::collections::{BTreeMap, BTreeSet};

use resonate_core::{Recording, RecordingKey, folded_letters};
use ring::digest::{SHA256, digest};

use crate::{
    change::{PlaylistKey, QueryBody},
    wire::Writer,
};

pub const DIGEST_BYTES: usize = 32;

pub fn one_recording_a_song(recordings: impl IntoIterator<Item = Recording>) -> Vec<Recording> {
    let mut identified = BTreeSet::new();
    let mut named = BTreeSet::new();
    let mut kept: Vec<Recording> = Vec::new();
    for recording in recordings {
        if recording.mbid.is_some() {
            if identified.insert(recording.key()) {
                kept.push(recording);
            }
        } else if named.insert(recording.named_key()) {
            kept.push(recording);
        }
    }
    let named_by_an_mbid = kept
        .iter()
        .filter(|recording| recording.mbid.is_some())
        .map(Recording::named_key)
        .collect::<BTreeSet<RecordingKey>>();
    kept.retain(|recording| {
        recording.mbid.is_some() || !named_by_an_mbid.contains(&recording.named_key())
    });
    kept
}

pub struct Synced<'a> {
    pub recording: &'a Recording,
    pub here: bool,
}

pub fn spliced(local: &[Recording], synced: &[Synced<'_>]) -> Vec<Recording> {
    let mut leading = Vec::new();
    let mut following: Vec<Vec<&Recording>> = vec![Vec::new(); synced.len()];
    let mut anchor = None;
    for (at, entry) in synced.iter().enumerate() {
        if entry.here {
            anchor = Some(at);
        } else {
            match anchor {
                Some(anchor) => following[anchor].push(entry.recording),
                None => leading.push(entry.recording),
            }
        }
    }

    let mut taken = vec![false; synced.len()];
    let mut placed: Vec<(Recording, Option<usize>)> =
        Vec::with_capacity(local.len() + synced.len());
    for recording in local {
        let matched = synced.iter().enumerate().position(|(at, entry)| {
            entry.here && !taken[at] && entry.recording.names_the_same_song_as(recording)
        });
        match matched {
            Some(at) => {
                taken[at] = true;
                placed.push((synced[at].recording.clone(), Some(at)));
                placed.extend(following[at].iter().map(|held| ((*held).clone(), None)));
            }
            None => placed.push((recording.clone(), None)),
        }
    }

    for (anchor, held) in following.iter().enumerate() {
        if taken[anchor] || held.is_empty() {
            continue;
        }
        let after = placed
            .iter()
            .rposition(|(_, from)| from.is_some_and(|from| from < anchor))
            .map_or(0, |at| at + 1);
        let after = placed[after..]
            .iter()
            .position(|(_, from)| from.is_some())
            .map_or(placed.len(), |offset| after + offset);
        for (offset, recording) in held.iter().enumerate() {
            placed.insert(after + offset, ((*recording).clone(), None));
        }
    }

    leading
        .into_iter()
        .cloned()
        .chain(placed.into_iter().map(|(recording, _)| recording))
        .collect()
}

pub fn query_digest(body: &QueryBody) -> [u8; DIGEST_BYTES] {
    let mut writer = Writer::new();
    writer.text(&body.name);
    match &body.text {
        Some(text) => {
            writer.flag(true);
            writer.text(text);
        }
        None => writer.flag(false),
    }
    writer.byte(body.sort);
    writer.byte(body.reading);
    match body.rows {
        Some(rows) => {
            writer.flag(true);
            writer.number(u64::from(rows));
        }
        None => writer.flag(false),
    }
    let hashed = digest(&SHA256, &writer.finish());
    let mut out = [0; DIGEST_BYTES];
    out.copy_from_slice(hashed.as_ref());
    out
}

pub fn projection_digest(name: &str, entries: &[Option<Recording>]) -> [u8; DIGEST_BYTES] {
    let mut writer = Writer::new();
    writer.text(name);
    writer.length(entries.len());
    for entry in entries {
        match entry {
            Some(recording) => {
                writer.flag(true);
                writer.recording(recording);
            }
            None => writer.flag(false),
        }
    }
    let mut digested = [0; DIGEST_BYTES];
    digested.copy_from_slice(digest(&SHA256, &writer.finish()).as_ref());
    digested
}

pub fn names_alike(first: &str, second: &str) -> bool {
    folded_letters(first.trim()) == folded_letters(second.trim())
}

pub fn named_as_a_free_alias_of(name: &str, written: &str) -> bool {
    let Some(numbered) = written.strip_prefix(name.trim()) else {
        return written == name;
    };
    if numbered.is_empty() {
        return true;
    }
    numbered
        .strip_prefix(" (")
        .and_then(|numbered| numbered.strip_suffix(')'))
        .is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub fn absorbed(kept: &[Recording], folded: &[Recording]) -> Vec<Recording> {
    let mut taken = vec![false; kept.len()];
    let mut entries = kept.to_vec();
    for entry in folded {
        match (0..kept.len()).find(|at| !taken[*at] && kept[*at].names_the_same_song_as(entry)) {
            Some(at) => taken[at] = true,
            None => entries.push(entry.clone()),
        }
    }
    entries
}

#[derive(Default)]
pub struct Folds {
    folded: BTreeSet<PlaylistKey>,
    beside: BTreeMap<PlaylistKey, Vec<PlaylistKey>>,
}

impl Folds {
    pub fn of(pairs: impl IntoIterator<Item = (PlaylistKey, PlaylistKey)>) -> Self {
        let mut folds = Self::default();
        for (folded, into) in pairs {
            folds.folded.insert(folded);
            folds.beside.entry(folded).or_default().push(into);
            folds.beside.entry(into).or_default().push(folded);
        }
        folds
    }

    pub fn survivor_of(&self, key: PlaylistKey) -> Option<PlaylistKey> {
        if !self.beside.contains_key(&key) {
            return None;
        }
        let mut class = BTreeSet::from([key]);
        let mut reached = vec![key];
        while let Some(at) = reached.pop() {
            for next in self.beside.get(&at).into_iter().flatten() {
                if class.insert(*next) {
                    reached.push(*next);
                }
            }
        }
        class
            .iter()
            .find(|member| !self.folded.contains(member))
            .or_else(|| class.first())
            .copied()
    }

    pub fn folds_away(&self, key: PlaylistKey) -> Option<PlaylistKey> {
        self.survivor_of(key).filter(|survivor| *survivor != key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_class_of_folds_survives_as_its_one_unfolded_key_and_a_cycle_as_its_lowest() {
        let [a, b, c, d] = [1, 2, 3, 4].map(|byte| PlaylistKey([byte; 16]));
        let chained = Folds::of([(c, b), (b, a)]);
        let crossed = Folds::of([(c, a), (c, b)]);
        let cycled = Folds::of([(a, d), (d, a)]);

        assert_eq!(chained.survivor_of(c), Some(a));
        assert_eq!(chained.folds_away(a), None);
        assert_eq!(chained.folds_away(b), Some(a));
        assert_eq!(crossed.survivor_of(b), Some(a));
        assert_eq!(cycled.survivor_of(d), Some(a));
        assert_eq!(chained.survivor_of(d), None);
    }

    #[test]
    fn a_free_alias_is_the_name_or_the_name_numbered() {
        assert!(named_as_a_free_alias_of("Mix", "Mix"));
        assert!(named_as_a_free_alias_of("Mix", "Mix (2)"));
        assert!(named_as_a_free_alias_of("Mix", "Mix (12)"));
        assert!(!named_as_a_free_alias_of("Mix", "Mix (two)"));
        assert!(!named_as_a_free_alias_of("Mix", "Mixed"));
        assert!(!named_as_a_free_alias_of("Mix", "Mix ()"));
    }

    fn song(title: &str) -> Recording {
        Recording {
            mbid: None,
            title: title.into(),
            artist: "Artist".into(),
            album: "Album".into(),
            length: None,
        }
    }

    fn titles(recordings: &[Recording]) -> Vec<&str> {
        recordings
            .iter()
            .map(|recording| recording.title.as_str())
            .collect()
    }

    fn synced<'a>(entries: &'a [(Recording, bool)]) -> Vec<Synced<'a>> {
        entries
            .iter()
            .map(|(recording, here)| Synced {
                recording,
                here: *here,
            })
            .collect()
    }

    #[test]
    fn songs_this_device_lacks_stay_after_the_song_they_followed() {
        let entries = [
            (song("Away first"), false),
            (song("A"), true),
            (song("Away after A"), false),
            (song("B"), true),
            (song("C"), true),
            (song("Away last"), false),
        ];

        let reordered = spliced(&[song("C"), song("A"), song("B")], &synced(&entries));

        assert_eq!(
            titles(&reordered),
            ["Away first", "C", "Away last", "A", "Away after A", "B"]
        );
    }

    #[test]
    fn songs_after_a_removed_song_move_up_to_where_it_was() {
        let entries = [
            (song("A"), true),
            (song("B"), true),
            (song("Away after B"), false),
            (song("C"), true),
        ];

        let removed = spliced(&[song("A"), song("C"), song("D")], &synced(&entries));

        assert_eq!(titles(&removed), ["A", "Away after B", "C", "D"]);
    }

    #[test]
    fn a_song_listed_twice_keeps_each_place() {
        let entries = [(song("A"), true), (song("Away"), false), (song("A"), true)];

        let unchanged = spliced(&[song("A"), song("A")], &synced(&entries));

        assert_eq!(titles(&unchanged), ["A", "Away", "A"]);
    }

    #[test]
    fn the_projection_changes_with_name_order_and_absence() {
        let first = projection_digest("Summer", &[Some(song("A")), None]);

        assert_eq!(first, projection_digest("Summer", &[Some(song("A")), None]));
        assert_ne!(first, projection_digest("Winter", &[Some(song("A")), None]));
        assert_ne!(first, projection_digest("Summer", &[None, Some(song("A"))]));
    }
}
