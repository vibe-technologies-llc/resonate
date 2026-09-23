use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Unchanged,
    Added { at: usize },
    Removed { at: usize },
    Replaced,
}

pub(crate) fn change(before: &[QueueItem], after: &[QueueItem]) -> Change {
    let shortest = before.len().min(after.len());
    let head = before
        .iter()
        .zip(after)
        .position(|(before, after)| before != after)
        .unwrap_or(shortest);
    let tail = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .position(|(before, after)| before != after)
        .unwrap_or(shortest);

    match (before.len(), after.len()) {
        (before, after) if before == after && head == before => Change::Unchanged,
        (before, after) if after == before + 1 && head + tail >= before => {
            Change::Added { at: head }
        }
        (before, after) if before == after + 1 && head + tail >= after => {
            Change::Removed { at: head }
        }
        _ => Change::Replaced,
    }
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
                Change::Added { at }
            );
        }
        assert_eq!(change(&[], &queue(&[9])), Change::Added { at: 0 });
    }

    #[test]
    fn one_row_leaving_is_announced_as_a_removal_of_the_row_it_held() {
        for (at, after) in [(0, vec![2, 3]), (1, vec![1, 3]), (2, vec![1, 2])] {
            assert_eq!(
                change(&queue(&[1, 2, 3]), &queue(&after)),
                Change::Removed { at }
            );
        }
        assert_eq!(change(&queue(&[9]), &[]), Change::Removed { at: 0 });
    }

    #[test]
    fn a_reshuffle_is_announced_as_a_whole_new_list() {
        assert_eq!(
            change(&queue(&[1, 2, 3]), &queue(&[3, 1, 2])),
            Change::Replaced
        );
        assert_eq!(
            change(&queue(&[1, 2]), &queue(&[3, 4, 5])),
            Change::Replaced
        );
        assert_eq!(change(&queue(&[1, 2, 3]), &queue(&[1])), Change::Replaced);
    }

    #[test]
    fn an_added_row_names_the_row_it_follows_or_no_track_at_all() {
        let queue = queue(&[1, 2, 3]);
        assert_eq!(after_row(&queue, 0).as_str(), NO_TRACK);
        assert_eq!(after_row(&queue, 1), track_path(queue[0].id));
        assert_eq!(after_row(&queue, 3), track_path(queue[2].id));
    }
}
