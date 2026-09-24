use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use ahash::AHashMap;
use resonate_core::TrackId;
use resonate_engine::{Command, Placement, QueueItem, Span, unclaimed_id};
use zbus::{
    fdo, interface,
    object_server::SignalEmitter,
    zvariant::{ObjectPath, OwnedObjectPath, OwnedValue},
};

use crate::{
    interfaces::Shared,
    track::{NO_TRACK, no_track, queued_metadata, track_of, track_path, unlisted_metadata},
};

const READ_BUDGET: Duration = Duration::from_millis(500);

const EDITS_ANNOUNCED_AT_MOST: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edit {
    Removed { at: usize },
    Added { at: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Unchanged,
    Edited(Vec<Edit>),
    Replaced,
}

pub(crate) fn change(before: &[QueueItem], after: &[QueueItem]) -> Change {
    if before == after {
        return Change::Unchanged;
    }

    let was_at: AHashMap<TrackId, usize> = before
        .iter()
        .enumerate()
        .map(|(at, item)| (item.id, at))
        .collect();
    let is_at: AHashMap<TrackId, usize> = after
        .iter()
        .enumerate()
        .map(|(at, item)| (item.id, at))
        .collect();
    if was_at.len() != before.len() || is_at.len() != after.len() {
        return Change::Replaced;
    }

    let mut kept = 0;
    let mut last_kept = None;
    for item in after {
        let Some(&was) = was_at.get(&item.id) else {
            continue;
        };
        let moved = last_kept.is_some_and(|last| was < last);
        if moved || before.get(was) != Some(item) {
            return Change::Replaced;
        }
        last_kept = Some(was);
        kept += 1;
    }
    if kept == 0 && !before.is_empty() && !after.is_empty() {
        return Change::Replaced;
    }

    let removed = before
        .iter()
        .enumerate()
        .filter(|(_, item)| !is_at.contains_key(&item.id))
        .map(|(at, _)| Edit::Removed { at });
    let added = after
        .iter()
        .enumerate()
        .filter(|(_, item)| !was_at.contains_key(&item.id))
        .map(|(at, _)| Edit::Added { at });
    let edits: Vec<Edit> = removed.chain(added).collect();
    if edits.len() > EDITS_ANNOUNCED_AT_MOST {
        return Change::Replaced;
    }
    Change::Edited(edits)
}

pub(crate) struct TrackList {
    pub(crate) shared: Arc<Shared>,
}

impl TrackList {
    fn row_of(queue: &[QueueItem], path: &str) -> Option<usize> {
        let track = track_of(path)?;
        queue.iter().position(|item| item.id == track)
    }

    fn landing(queue: &[QueueItem], after: &ObjectPath<'_>) -> fdo::Result<usize> {
        if after.as_str() == NO_TRACK {
            return Ok(0);
        }
        Self::row_of(queue, after.as_str())
            .map(|row| row.saturating_add(1))
            .ok_or_else(|| {
                fdo::Error::InvalidArgs("that track is not in the track list".to_owned())
            })
    }
}

#[interface(name = "org.mpris.MediaPlayer2.TrackList")]
impl TrackList {
    fn get_tracks_metadata(
        &self,
        track_ids: Vec<OwnedObjectPath>,
    ) -> Vec<HashMap<String, OwnedValue>> {
        let queue = self.shared.player.queue();
        let state = self.shared.player.state();
        let digest = self.shared.player.digest();
        let art = self.shared.art(&state, digest.as_ref());
        let until = Instant::now() + READ_BUDGET;

        track_ids
            .iter()
            .map(|path| {
                let Some(item) = Self::row_of(&queue, path.as_str()).and_then(|row| queue.get(row))
                else {
                    return unlisted_metadata(path);
                };
                let media = self.shared.player.media_within(
                    &item.location,
                    item.span,
                    until.saturating_duration_since(Instant::now()),
                );
                if media.is_none() {
                    self.shared.owed.note(item.id);
                }
                queued_metadata(
                    item,
                    &state,
                    digest.as_ref(),
                    media.as_deref(),
                    art.clone(),
                    self.shared.host.heard(&item.location, item.span),
                )
            })
            .collect()
    }

    fn add_track(
        &self,
        uri: &str,
        after_track: ObjectPath<'_>,
        set_as_current: bool,
    ) -> fdo::Result<()> {
        let (location, span) = self.shared.located(uri)?;
        let queue = self.shared.player.queue();
        let at = Self::landing(&queue, &after_track)?;

        self.shared.settle(Command::Insert {
            items: vec![QueueItem {
                id: unclaimed_id(&queue),
                location,
                span,
            }],
            at: Placement::At(at),
            play: set_as_current,
        })
    }

    fn remove_track(&self, track_id: ObjectPath<'_>) -> fdo::Result<()> {
        let queue = self.shared.player.queue();
        let Some(row) = Self::row_of(&queue, track_id.as_str()) else {
            return Ok(());
        };
        self.shared.settle(Command::Remove(Span::one(row)))
    }

    fn go_to(&self, track_id: ObjectPath<'_>) -> fdo::Result<()> {
        let queue = self.shared.player.queue();
        let Some(row) = Self::row_of(&queue, track_id.as_str()) else {
            return Ok(());
        };
        self.shared.settle(Command::JumpTo(row))
    }

    #[zbus(property(emits_changed_signal = "invalidates"))]
    fn tracks(&self) -> Vec<OwnedObjectPath> {
        self.shared
            .player
            .queue()
            .iter()
            .map(|item| track_path(item.id))
            .collect()
    }

    #[zbus(property)]
    const fn can_edit_tracks(&self) -> bool {
        true
    }

    #[zbus(signal)]
    pub(crate) async fn track_list_replaced(
        emitter: &SignalEmitter<'_>,
        tracks: Vec<OwnedObjectPath>,
        current_track: OwnedObjectPath,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub(crate) async fn track_added(
        emitter: &SignalEmitter<'_>,
        metadata: HashMap<String, OwnedValue>,
        after_track: OwnedObjectPath,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub(crate) async fn track_removed(
        emitter: &SignalEmitter<'_>,
        track_id: OwnedObjectPath,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub(crate) async fn track_metadata_changed(
        emitter: &SignalEmitter<'_>,
        track_id: OwnedObjectPath,
        metadata: HashMap<String, OwnedValue>,
    ) -> zbus::Result<()>;
}

pub(crate) fn after_row(queue: &[QueueItem], at: usize) -> OwnedObjectPath {
    at.checked_sub(1)
        .and_then(|before| queue.get(before))
        .map_or_else(no_track, |item| track_path(item.id))
}

#[cfg(test)]
mod tests {
    use resonate_core::{MediaLocation, TrackId};

    use super::*;

    fn queue(ids: &[u64]) -> Vec<QueueItem> {
        ids.iter()
            .map(|id| QueueItem {
                id: TrackId::new(*id).expect("track ids start at one"),
                location: MediaLocation::local(format!("/music/{id}.flac")),
                span: None,
            })
            .collect()
    }

    fn edited(edits: &[Edit]) -> Change {
        Change::Edited(edits.to_vec())
    }

    #[test]
    fn a_queue_that_did_not_move_is_announced_as_nothing_at_all() {
        assert_eq!(
            change(&queue(&[1, 2, 3]), &queue(&[1, 2, 3])),
            Change::Unchanged
        );
        assert_eq!(change(&[], &[]), Change::Unchanged);
    }

    #[test]
    fn one_row_arriving_is_announced_as_an_addition_wherever_it_lands() {
        for (at, after) in [
            (0, vec![9, 1, 2, 3]),
            (1, vec![1, 9, 2, 3]),
            (3, vec![1, 2, 3, 9]),
        ] {
            assert_eq!(
                change(&queue(&[1, 2, 3]), &queue(&after)),
                edited(&[Edit::Added { at }])
            );
        }
        assert_eq!(change(&[], &queue(&[9])), edited(&[Edit::Added { at: 0 }]));
    }

    #[test]
    fn one_row_leaving_is_announced_as_a_removal_of_the_row_it_held() {
        for (at, after) in [(0, vec![2, 3]), (1, vec![1, 3]), (2, vec![1, 2])] {
            assert_eq!(
                change(&queue(&[1, 2, 3]), &queue(&after)),
                edited(&[Edit::Removed { at }])
            );
        }
        assert_eq!(
            change(&queue(&[9]), &[]),
            edited(&[Edit::Removed { at: 0 }])
        );
    }

    #[test]
    fn two_edits_inside_one_sample_are_announced_as_two() {
        assert_eq!(
            change(&queue(&[1, 2, 3]), &queue(&[1, 8, 2, 3, 9])),
            edited(&[Edit::Added { at: 1 }, Edit::Added { at: 4 }])
        );
        assert_eq!(
            change(&queue(&[1, 2, 3]), &queue(&[1, 3, 9])),
            edited(&[Edit::Removed { at: 1 }, Edit::Added { at: 2 }])
        );
        assert_eq!(
            change(&queue(&[1, 2, 3]), &queue(&[1])),
            edited(&[Edit::Removed { at: 1 }, Edit::Removed { at: 2 }])
        );
    }

    #[test]
    fn a_reshuffle_or_a_list_with_nothing_kept_is_announced_as_a_whole_new_list() {
        assert_eq!(
            change(&queue(&[1, 2, 3]), &queue(&[3, 1, 2])),
            Change::Replaced
        );
        assert_eq!(
            change(&queue(&[1, 2, 3]), &queue(&[1, 3, 2, 9])),
            Change::Replaced
        );
        assert_eq!(
            change(&queue(&[1, 2]), &queue(&[3, 4, 5])),
            Change::Replaced
        );
    }

    #[test]
    fn more_edits_than_a_listener_should_replay_are_a_whole_new_list() {
        let before: Vec<u64> = (1..=10).collect();
        let after: Vec<u64> = (1..=10)
            .chain(100..100 + EDITS_ANNOUNCED_AT_MOST as u64 + 1)
            .collect();

        assert_eq!(change(&queue(&before), &queue(&after)), Change::Replaced);
    }

    #[test]
    fn an_added_row_names_the_row_it_follows_or_no_track_at_all() {
        let queue = queue(&[1, 2, 3]);
        assert_eq!(after_row(&queue, 0).as_str(), NO_TRACK);
        assert_eq!(after_row(&queue, 1), track_path(queue[0].id));
        assert_eq!(after_row(&queue, 3), track_path(queue[2].id));
    }
}
