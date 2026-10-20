use resonate_core::{Recording, folded_letters};
use ring::digest::{SHA256, digest};

use crate::{change::QueryBody, wire::Writer};

pub const DIGEST_BYTES: usize = 32;

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

#[cfg(test)]
mod tests {
    use super::*;

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
