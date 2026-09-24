use std::time::Duration;

use resonate_core::{Frames, QueueStamp, Reordered, Resumable, Resumption, plays_in};

use crate::{PlayerState, QueueItem, Queued};

pub const KEPT_EVERY: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Keep {
    Queue(Resumption),
    Order(Reordered),
    Place { row: usize, at: Frames },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Held {
    stamp: QueueStamp,
    revision: u64,
    shuffle: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keeping {
    held: Option<Held>,
    kept: Option<(usize, Frames)>,
}

impl Keeping {
    pub fn kept(&mut self, state: &PlayerState, queued: &Queued) -> Option<Keep> {
        let row = state.queue_position.unwrap_or(0);
        let at = state
            .current
            .as_ref()
            .map_or(Frames::ZERO, |track| track.position);
        let held = Held {
            stamp: state.queue_stamp,
            revision: queued.revision,
            shuffle: state.shuffle,
        };
        let standing = self.held.replace(held);

        if standing.is_none_or(|standing| standing.stamp != held.stamp) {
            self.kept = Some((row, at));
            let (rows, order) = in_load_order(queued);
            return Some(Keep::Queue(Resumption {
                rows,
                order,
                row,
                at,
                shuffle: state.shuffle,
            }));
        }
        if standing != Some(held) {
            self.kept = Some((row, at));
            return Some(Keep::Order(Reordered {
                order: in_load_order(queued).1,
                row,
                at,
                shuffle: state.shuffle,
            }));
        }

        if state.queue_position.is_none() || !self.moved_on(state, row, at) {
            return None;
        }
        self.kept = Some((row, at));
        Some(Keep::Place { row, at })
    }

    fn moved_on(&self, state: &PlayerState, row: usize, at: Frames) -> bool {
        let Some((held_row, held_at)) = self.kept else {
            return true;
        };
        if held_row != row {
            return true;
        }

        let rate = match state.current.as_ref() {
            Some(track) => track.source.rate,
            None => return held_at != at,
        };
        Frames(held_at.get().abs_diff(at.get())) >= Frames::from_duration(KEPT_EVERY, rate)
    }
}

fn in_load_order(queued: &Queued) -> (Vec<Resumable>, Vec<usize>) {
    let order = plays_in(&queued.loaded_at, queued.rows.len());
    let mut loaded: Vec<Option<Resumable>> = (0..queued.rows.len()).map(|_| None).collect();
    for (plays_at, item) in queued.rows.iter().enumerate() {
        loaded[order[plays_at]] = Some(resumable(item));
    }

    (loaded.into_iter().flatten().collect(), order)
}

fn resumable(item: &QueueItem) -> Resumable {
    Resumable {
        location: item.location.clone(),
        span: item.span,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use resonate_core::{
        ChannelLayout, MediaLocation, SampleFormat, SampleRate, StreamSpec, TrackId, Volume,
    };

    use super::*;
    use crate::{PlaybackState, RepeatMode, Seeks, TrackState, queue::stamp_of};

    const RATE: SampleRate = SampleRate::HZ_44100;

    fn seconds(count: u64) -> Frames {
        Frames(RATE.hz() as u64 * count)
    }

    fn track(number: u64) -> TrackId {
        TrackId::new(number).expect("track ids start at one")
    }

    fn queued(count: u64) -> Vec<QueueItem> {
        (1..=count)
            .map(|number| QueueItem {
                id: track(number),
                location: MediaLocation::local(format!("/music/{number}.flac")),
                span: None,
            })
            .collect()
    }

    fn drawn(queue: &[QueueItem], revision: u64) -> Queued {
        Queued {
            revision,
            rows: Arc::new(queue.to_vec()),
            loaded_at: Arc::from((0..queue.len()).collect::<Vec<_>>()),
        }
    }

    fn reordered(queue: &[QueueItem], revision: u64, loaded_at: Vec<usize>) -> Queued {
        Queued {
            revision,
            rows: Arc::new(queue.to_vec()),
            loaded_at: Arc::from(loaded_at),
        }
    }

    fn playing_at(queue: &[QueueItem], row: usize, position: Frames) -> PlayerState {
        PlayerState {
            playback: PlaybackState::Playing,
            current: Some(TrackState {
                id: queue[row].id,
                source: StreamSpec::new(RATE, ChannelLayout::Stereo, SampleFormat::S16),
                position,
                duration: None,
            }),
            volume: Volume::MAX,
            repeat: RepeatMode::Off,
            skip_under_repeat: crate::SkipUnderRepeat::default(),
            shuffle: false,
            queue_position: Some(row),
            loaded_position: Some(row),
            queue_len: queue.len(),
            queue_stamp: stamp_of(queue),
            seeks: Seeks::default(),
            sleeping: None,
            output: None,
        }
    }

    fn shuffled_at(queue: &[QueueItem], row: usize, position: Frames) -> PlayerState {
        PlayerState {
            shuffle: true,
            ..playing_at(queue, row, position)
        }
    }

    #[test]
    fn a_queue_first_seen_is_kept_whole() {
        let mut keeping = Keeping::default();
        let queue = queued(3);

        let Some(Keep::Queue(kept)) =
            keeping.kept(&playing_at(&queue, 0, Frames::ZERO), &drawn(&queue, 1))
        else {
            panic!("a queue nothing has kept is kept whole");
        };
        assert_eq!(kept.rows.len(), 3);
        assert_eq!(kept.row, 0);
        assert_eq!(kept.at, Frames::ZERO);
        assert!(!kept.shuffle);
    }

    #[test]
    fn a_position_that_has_barely_moved_is_not_written_again() {
        let mut keeping = Keeping::default();
        let queue = queued(3);

        keeping.kept(&playing_at(&queue, 0, Frames::ZERO), &drawn(&queue, 1));
        for second in 1..5 {
            assert_eq!(
                keeping.kept(&playing_at(&queue, 0, seconds(second)), &drawn(&queue, 1)),
                None
            );
        }
    }

    #[test]
    fn a_position_that_has_moved_far_enough_keeps_the_place_alone() {
        let mut keeping = Keeping::default();
        let queue = queued(3);

        keeping.kept(&playing_at(&queue, 0, Frames::ZERO), &drawn(&queue, 1));
        assert_eq!(
            keeping.kept(&playing_at(&queue, 0, seconds(5)), &drawn(&queue, 1)),
            Some(Keep::Place {
                row: 0,
                at: seconds(5),
            })
        );
    }

    #[test]
    fn a_seek_backwards_is_as_much_of_a_move_as_a_seek_forwards() {
        let mut keeping = Keeping::default();
        let queue = queued(3);

        keeping.kept(&playing_at(&queue, 0, Frames::ZERO), &drawn(&queue, 1));
        keeping.kept(&playing_at(&queue, 0, seconds(60)), &drawn(&queue, 1));
        assert_eq!(
            keeping.kept(&playing_at(&queue, 0, seconds(20)), &drawn(&queue, 1)),
            Some(Keep::Place {
                row: 0,
                at: seconds(20),
            })
        );
    }

    #[test]
    fn a_row_that_has_changed_keeps_the_place_at_once() {
        let mut keeping = Keeping::default();
        let queue = queued(3);

        keeping.kept(&playing_at(&queue, 0, Frames::ZERO), &drawn(&queue, 1));
        assert_eq!(
            keeping.kept(&playing_at(&queue, 1, Frames::ZERO), &drawn(&queue, 1)),
            Some(Keep::Place {
                row: 1,
                at: Frames::ZERO,
            })
        );
    }

    #[test]
    fn a_queue_that_played_through_keeps_the_last_place_it_reached() {
        let mut keeping = Keeping::default();
        let queue = queued(3);

        keeping.kept(&playing_at(&queue, 0, Frames::ZERO), &drawn(&queue, 1));
        keeping.kept(&playing_at(&queue, 2, seconds(40)), &drawn(&queue, 1));
        let finished = PlayerState {
            playback: PlaybackState::Stopped,
            current: None,
            queue_position: None,
            loaded_position: None,
            ..playing_at(&queue, 2, seconds(40))
        };
        assert_eq!(
            keeping.kept(&finished, &drawn(&queue, 1)),
            None,
            "a queue that ran out was kept as its first row"
        );
        assert_eq!(
            keeping.kept(&playing_at(&queue, 0, Frames::ZERO), &drawn(&queue, 1)),
            Some(Keep::Place {
                row: 0,
                at: Frames::ZERO,
            })
        );
    }

    #[test]
    fn rows_that_have_changed_are_kept_whole_again() {
        let mut keeping = Keeping::default();
        let queue = queued(3);
        let longer = queued(4);

        keeping.kept(&playing_at(&queue, 0, Frames::ZERO), &drawn(&queue, 1));
        let Some(Keep::Queue(kept)) =
            keeping.kept(&playing_at(&longer, 0, Frames::ZERO), &drawn(&longer, 2))
        else {
            panic!("a queue that has gained a row is kept whole");
        };
        assert_eq!(kept.rows.len(), 4);
    }

    #[test]
    fn an_order_that_has_changed_keeps_the_order_alone_where_the_rows_have_not() {
        let mut keeping = Keeping::default();
        let queue = queued(4);
        let moved = vec![
            queue[2].clone(),
            queue[0].clone(),
            queue[1].clone(),
            queue[3].clone(),
        ];

        keeping.kept(&playing_at(&queue, 0, Frames::ZERO), &drawn(&queue, 1));
        let Some(Keep::Order(kept)) = keeping.kept(
            &playing_at(&queue, 0, Frames::ZERO),
            &reordered(&moved, 2, vec![2, 0, 1, 3]),
        ) else {
            panic!("a queue whose rows were dragged into another order rewrote every row");
        };
        assert_eq!(
            kept.order,
            vec![2, 0, 1, 3],
            "the order the queue was playing in was not kept"
        );
    }

    #[test]
    fn shuffling_is_kept_beside_the_order_it_produced() {
        let mut keeping = Keeping::default();
        let queue = queued(4);

        keeping.kept(&playing_at(&queue, 0, Frames::ZERO), &drawn(&queue, 1));
        let Some(Keep::Order(kept)) = keeping.kept(
            &shuffled_at(&queue, 0, Frames::ZERO),
            &reordered(&queue, 2, vec![3, 1, 0, 2]),
        ) else {
            panic!("a queue that has been shuffled keeps the order it produced");
        };
        assert!(
            kept.shuffle,
            "the shuffle the queue was playing under was lost"
        );
        assert_eq!(kept.order, vec![3, 1, 0, 2]);

        assert_eq!(
            keeping.kept(
                &shuffled_at(&queue, 0, Frames::ZERO),
                &reordered(&queue, 2, vec![3, 1, 0, 2])
            ),
            None,
            "a queue that had not moved was written again"
        );
    }

    #[test]
    fn a_queue_that_has_emptied_keeps_nothing_to_resume() {
        let mut keeping = Keeping::default();
        let queue = queued(3);

        keeping.kept(&playing_at(&queue, 0, Frames::ZERO), &drawn(&queue, 1));
        let Some(Keep::Queue(kept)) = keeping.kept(&PlayerState::default(), &Queued::default())
        else {
            panic!("an emptied queue is kept as the nothing it is");
        };
        assert!(kept.is_empty());
    }

    #[test]
    fn a_span_is_kept_beside_the_location_it_cuts() {
        let mut keeping = Keeping::default();
        let cut = vec![QueueItem {
            id: track(1),
            location: MediaLocation::local("/music/whole.flac"),
            span: Some(resonate_core::FrameSpan::between(seconds(10), seconds(20))),
        }];

        let Some(Keep::Queue(kept)) =
            keeping.kept(&playing_at(&cut, 0, Frames::ZERO), &drawn(&cut, 1))
        else {
            panic!("a queue nothing has kept is kept whole");
        };
        assert_eq!(kept.rows[0].span, cut[0].span);
    }
}
