use crate::{
    Result,
    service::{InParts, Oversized, Pieces},
    shared::op::SharedOp,
    wire::{BLOBS_A_FRAME_AT_MOST, FromServer, LARGEST_FRAME, Refusal, ToServer},
};

pub const APPEND_BYTES_AT_MOST: usize = LARGEST_FRAME / 2;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct AppendFrame {
    pub blobs: Vec<Vec<u8>>,
    pub ops: usize,
    bytes: usize,
}

impl AppendFrame {
    fn takes(&self, sealed: &[Vec<u8>]) -> bool {
        let bytes = sealed.iter().map(Vec::len).sum::<usize>();
        self.blobs.is_empty()
            || (self.blobs.len() + sealed.len() <= BLOBS_A_FRAME_AT_MOST
                && self.bytes + bytes <= APPEND_BYTES_AT_MOST)
    }

    fn add(&mut self, sealed: Vec<Vec<u8>>, ops: usize) {
        self.bytes += sealed.iter().map(Vec::len).sum::<usize>();
        self.blobs.extend(sealed);
        self.ops += ops;
    }
}

pub fn append_frames<'a>(
    bundles: impl IntoIterator<Item = &'a [SharedOp]>,
    mut seal: impl FnMut(&[SharedOp], &mut Vec<Vec<u8>>) -> Result<()>,
) -> Result<Vec<AppendFrame>> {
    let mut frames = Vec::new();
    let mut frame = AppendFrame::default();

    for bundle in bundles {
        let mut sealed = Vec::new();
        seal(bundle, &mut sealed)?;
        if !frame.takes(&sealed) {
            frames.push(std::mem::take(&mut frame));
        }
        frame.add(sealed, bundle.len());
    }
    if !frame.blobs.is_empty() {
        frames.push(frame);
    }

    Ok(frames)
}

pub fn compaction_pieces<'a>(
    bundles: impl IntoIterator<Item = &'a [SharedOp]>,
    mut seal: impl FnMut(&[SharedOp], &mut Vec<Vec<u8>>) -> Result<()>,
) -> Result<Pieces> {
    let mut pieces = Pieces::default();
    let mut sealed = Vec::new();

    for bundle in bundles {
        seal(bundle, &mut sealed)?;
        for part in sealed.drain(..) {
            pieces.push(part);
        }
    }

    Ok(pieces)
}

#[derive(Debug, PartialEq, Eq)]
pub enum CompactionAnswered {
    Staging(Vec<ToServer>),
    Compacted,
    TakesNoParts(Oversized),
    SpaceFull(Oversized),
    Refused(Refusal),
    Unrelated,
}

pub fn compaction_answered(
    in_parts: &mut InParts,
    records_since: u64,
    heard: FromServer,
) -> CompactionAnswered {
    match heard {
        FromServer::Staging => CompactionAnswered::Staging(in_parts.taken()),
        FromServer::Compacted { .. } => CompactionAnswered::Compacted,
        FromServer::Refused(Refusal::Stale) => in_parts.refused_as_stale().map_or(
            CompactionAnswered::Compacted,
            CompactionAnswered::TakesNoParts,
        ),
        FromServer::Refused(Refusal::SpaceFull) => {
            CompactionAnswered::SpaceFull(Oversized::found_at(records_since))
        }
        FromServer::Refused(refusal) => CompactionAnswered::Refused(refusal),
        _ => CompactionAnswered::Unrelated,
    }
}

pub fn owed_sent(owed: &mut Vec<(i64, Vec<SharedOp>)>, sent: usize) {
    let mut left = sent;
    let mut whole = 0;
    for (_, ops) in owed.iter_mut() {
        if ops.len() > left {
            ops.drain(..left);
            break;
        }
        left -= ops.len();
        whole += 1;
    }
    owed.drain(..whole);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Stamp, key::DeviceId};

    fn renamed(nanos: i64) -> SharedOp {
        SharedOp::Renamed {
            name: "Mix".into(),
            stamp: Stamp {
                nanos,
                device: DeviceId([1; 16]),
            },
        }
    }

    fn sealed_as(bytes: usize) -> impl FnMut(&[SharedOp], &mut Vec<Vec<u8>>) -> Result<()> {
        move |_, sealed| {
            sealed.push(vec![0_u8; bytes]);
            Ok(())
        }
    }

    #[test]
    fn an_append_of_more_bundles_than_a_frame_carries_is_split_across_frames() {
        let ops = (0..=BLOBS_A_FRAME_AT_MOST)
            .map(|nanos| renamed(i64::try_from(nanos).expect("a small stamp")))
            .collect::<Vec<_>>();

        let frames = append_frames(ops.chunks(1), sealed_as(64)).expect("sealed");
        let blobs = frames
            .iter()
            .map(|frame| frame.blobs.len())
            .collect::<Vec<_>>();
        let carried = frames.iter().map(|frame| frame.ops).sum::<usize>();

        assert_eq!(blobs, [BLOBS_A_FRAME_AT_MOST, 1]);
        assert_eq!(carried, ops.len());
    }

    #[test]
    fn a_frame_carries_at_most_half_a_frame_of_bytes_and_a_heavier_bundle_goes_alone() {
        let ops = (0..3).map(renamed).collect::<Vec<_>>();

        let frames = append_frames(ops.chunks(1), sealed_as(APPEND_BYTES_AT_MOST)).expect("sealed");
        let blobs = frames
            .iter()
            .map(|frame| frame.blobs.len())
            .collect::<Vec<_>>();

        assert_eq!(blobs, [1, 1, 1]);
    }

    #[test]
    fn a_compaction_of_more_parts_than_a_frame_carries_is_staged_and_one_that_fits_is_one_compact()
    {
        let ops = (0..=BLOBS_A_FRAME_AT_MOST)
            .map(|nanos| renamed(i64::try_from(nanos).expect("a small stamp")))
            .collect::<Vec<_>>();
        let mut past_in_parts = InParts::default();
        let mut fitting_in_parts = InParts::default();

        let past = compaction_pieces(ops.chunks(1), sealed_as(64)).expect("sealed");
        let fitting = compaction_pieces(ops[1..].chunks(1), sealed_as(64)).expect("sealed");

        assert_eq!(past.frame_lengths(), [BLOBS_A_FRAME_AT_MOST, 1]);
        assert_eq!(past_in_parts.asked(600, 600, past), ToServer::Stage);
        assert!(matches!(
            fitting_in_parts.asked(600, 600, fitting),
            ToServer::Compact { through: 600, parts } if parts.len() == BLOBS_A_FRAME_AT_MOST
        ));
    }

    #[test]
    fn a_compaction_heavier_than_half_a_frame_is_cut_where_half_a_frame_ends() {
        let ops = (0..3).map(renamed).collect::<Vec<_>>();

        let past = compaction_pieces(ops.chunks(1), sealed_as(APPEND_BYTES_AT_MOST / 2 + 1))
            .expect("sealed");
        let fitting = compaction_pieces(ops[1..].chunks(1), sealed_as(APPEND_BYTES_AT_MOST / 2))
            .expect("sealed");

        assert_eq!(past.frame_lengths(), [1, 1, 1]);
        assert_eq!(fitting.frame_lengths(), [2]);
    }

    fn staged_of_two_frames() -> InParts {
        let ops = (0..=BLOBS_A_FRAME_AT_MOST)
            .map(|nanos| renamed(i64::try_from(nanos).expect("a small stamp")))
            .collect::<Vec<_>>();
        let mut in_parts = InParts::default();
        in_parts.asked(
            700,
            600,
            compaction_pieces(ops.chunks(1), sealed_as(64)).expect("sealed"),
        );
        in_parts
    }

    #[test]
    fn a_server_staging_is_sent_every_frame_and_named_through_its_last() {
        let mut in_parts = staged_of_two_frames();

        let answered = compaction_answered(&mut in_parts, 600, FromServer::Staging);
        let compacted =
            compaction_answered(&mut in_parts, 600, FromServer::Compacted { through: 700 });

        assert_eq!(
            answered,
            CompactionAnswered::Staging(vec![
                ToServer::Staged {
                    parts: vec![vec![0; 64]; BLOBS_A_FRAME_AT_MOST]
                },
                ToServer::CompactStaged {
                    through: 700,
                    parts: vec![vec![0; 64]]
                },
            ])
        );
        assert_eq!(compacted, CompactionAnswered::Compacted);
    }

    #[test]
    fn a_server_refusing_to_stage_takes_no_parts_and_a_stale_compaction_was_overtaken() {
        let mut staged = staged_of_two_frames();
        let mut sent_whole = InParts::default();

        let refused_staging =
            compaction_answered(&mut staged, 600, FromServer::Refused(Refusal::Stale));
        let overtaken =
            compaction_answered(&mut sent_whole, 600, FromServer::Refused(Refusal::Stale));

        assert_eq!(
            refused_staging,
            CompactionAnswered::TakesNoParts(Oversized::found_at(600))
        );
        assert_eq!(overtaken, CompactionAnswered::Compacted);
    }

    #[test]
    fn a_full_space_backs_the_compaction_off_until_the_records_double_and_other_refusals_stand() {
        let mut in_parts = staged_of_two_frames();

        let full = compaction_answered(&mut in_parts, 600, FromServer::Refused(Refusal::SpaceFull));
        let too_fast =
            compaction_answered(&mut in_parts, 600, FromServer::Refused(Refusal::TooFast));
        let unrelated = compaction_answered(&mut in_parts, 600, FromServer::Appended { count: 1 });

        assert_eq!(
            full,
            CompactionAnswered::SpaceFull(Oversized::found_at(600))
        );
        assert!(Oversized::found_at(600).waits(1199));
        assert!(!Oversized::found_at(600).waits(1200));
        assert_eq!(too_fast, CompactionAnswered::Refused(Refusal::TooFast));
        assert_eq!(unrelated, CompactionAnswered::Unrelated);
    }

    #[test]
    fn nothing_owed_sends_no_frame() {
        let frames = append_frames(Vec::<&[SharedOp]>::new(), sealed_as(64)).expect("sealed");

        assert!(frames.is_empty());
    }

    #[test]
    fn what_a_frame_carried_leaves_the_owed_ops_and_the_rest_stay_in_order() {
        let mut owed = vec![
            (1, vec![renamed(1), renamed(2)]),
            (2, vec![renamed(3), renamed(4), renamed(5)]),
            (3, vec![renamed(6)]),
        ];

        owed_sent(&mut owed, 3);
        let after_three = owed.clone();
        owed_sent(&mut owed, 3);

        assert_eq!(
            after_three,
            [(2, vec![renamed(4), renamed(5)]), (3, vec![renamed(6)])]
        );
        assert!(owed.is_empty());
    }
}
