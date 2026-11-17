use crate::{
    Result,
    shared::op::SharedOp,
    wire::{BLOBS_A_FRAME_AT_MOST, LARGEST_FRAME},
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
