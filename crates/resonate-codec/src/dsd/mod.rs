mod dff;
mod dop;
mod dsf;
mod pcm;
mod rate;
mod window;

use std::io::{Read, Seek, SeekFrom};

use resonate_core::{ChannelCount, ChannelLayout, Frames};

pub use crate::dsd::rate::{DsdRate, Packing};
pub(crate) use crate::dsd::{dop::Dop, pcm::Decimator, rate::DOP_DECIMATION};
use crate::{Error, MediaInfo, Result, source::MediaStream};

pub(crate) const MAX_DSD_CHANNELS: u16 = 8;
pub(crate) const DSD_SILENCE: u8 = 0x69;
pub(crate) const BYTES_PER_DOP_FRAME: u64 = 2;
pub(crate) const DSD_BLOCK_FRAMES: u64 = 2_048;

const SNIFF_BYTES: usize = 4;

pub(crate) const DSF_FORMAT_ID: symphonia::core::formats::FormatId =
    symphonia::core::formats::FormatId::new(symphonia::core::common::FourCc::new(*b"DSF "));
pub(crate) const DFF_FORMAT_ID: symphonia::core::formats::FormatId =
    symphonia::core::formats::FormatId::new(symphonia::core::common::FourCc::new(*b"DFF "));
pub(crate) const DSD_CODEC_ID: symphonia::core::codecs::audio::AudioCodecId =
    symphonia::core::codecs::audio::AudioCodecId::new(symphonia::core::common::FourCc::new(
        *b"DSD ",
    ));

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum BitOrder {
    LeastSignificantFirst,
    MostSignificantFirst,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Interleave {
    Blocked(u64),
    PerByte,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Layout {
    pub(crate) container: Container,
    pub(crate) rate: DsdRate,
    pub(crate) channels: ChannelLayout,
    pub(crate) samples: u64,
    pub(crate) data_at: u64,
    pub(crate) data_bytes: u64,
    pub(crate) order: Interleave,
    pub(crate) bits: BitOrder,
    pub(crate) metadata_at: Option<u64>,
    pub(crate) edited: Edited,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Edited {
    pub(crate) artist: Option<String>,
    pub(crate) title: Option<String>,
}

impl Layout {
    pub(crate) fn frames(&self) -> Frames {
        Frames(self.samples / DOP_DECIMATION as u64)
    }

    fn lanes(&self) -> usize {
        usize::from(self.channels.count().get())
    }

    fn bytes_per_channel(&self) -> u64 {
        self.data_bytes / self.lanes() as u64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Container {
    Dsf,
    Dff,
}

pub(crate) fn sniff(bytes: &mut dyn MediaStream) -> Option<Container> {
    let Ok(origin) = bytes.stream_position() else {
        return None;
    };
    let mut magic = [0_u8; SNIFF_BYTES];
    let read = bytes.read_exact(&mut magic).is_ok();
    let _ = bytes.seek(SeekFrom::Start(origin));

    if !read {
        return None;
    }
    match &magic {
        b"DSD " => Some(Container::Dsf),
        b"FRM8" => Some(Container::Dff),
        _ => None,
    }
}

pub(crate) fn layout(
    bytes: &mut dyn MediaStream,
    container: Container,
    location: &resonate_core::MediaLocation,
) -> Result<Layout> {
    match container {
        Container::Dsf => dsf::read(bytes, location),
        Container::Dff => dff::read(bytes, location),
    }
}

pub(crate) fn channels(
    count: u32,
    location: &resonate_core::MediaLocation,
) -> Result<ChannelLayout> {
    let count = u16::try_from(count).unwrap_or(u16::MAX);
    if count == 0 || count > MAX_DSD_CHANNELS {
        return Err(Error::DsdFieldNotUsable {
            location: location.clone(),
            field: DsdField::ChannelCount,
            value: u64::from(count),
        });
    }
    Ok(ChannelLayout::from_count(ChannelCount::new(count)?))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DsdField {
    FormatVersion,
    FormatId,
    ChannelCount,
    BitOrder,
    BlockSize,
    SampleRate,
}

impl std::fmt::Display for DsdField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::FormatVersion => "format version",
            Self::FormatId => "format id",
            Self::ChannelCount => "channel count",
            Self::BitOrder => "bit order",
            Self::BlockSize => "block size",
            Self::SampleRate => "sample rate",
        };
        f.write_str(name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DsdChunk {
    Format,
    Data,
    SampleRate,
    Channels,
    Compression,
}

impl std::fmt::Display for DsdChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Format => "fmt",
            Self::Data => "data",
            Self::SampleRate => "FS",
            Self::Channels => "CHNL",
            Self::Compression => "CMPR",
        };
        f.write_str(name)
    }
}

pub(crate) struct Planes {
    bytes: Box<dyn MediaStream>,
    layout: Layout,
    at: u64,
    planes: Vec<Vec<u8>>,
    interleaved: Vec<u8>,
}

impl Planes {
    pub(crate) fn over(bytes: Box<dyn MediaStream>, layout: Layout) -> Self {
        let lanes = layout.lanes();
        Self {
            bytes,
            layout,
            at: 0,
            planes: vec![Vec::new(); lanes],
            interleaved: Vec::new(),
        }
    }

    pub(crate) const fn at(&self) -> u64 {
        self.at
    }

    pub(crate) fn held(&self) -> &[Vec<u8>] {
        &self.planes
    }

    pub(crate) fn shortest(&self) -> u64 {
        self.planes.iter().map(Vec::len).min().unwrap_or(0) as u64
    }

    pub(crate) fn seek_to_byte(&mut self, byte: u64) -> Result<()> {
        self.at = byte.min(self.layout.bytes_per_channel());
        Ok(())
    }

    pub(crate) fn read(&mut self, wanted: u64) -> Result<()> {
        let left = self.layout.bytes_per_channel().saturating_sub(self.at);
        let taking = wanted.min(left);

        match self.layout.order {
            Interleave::Blocked(block) => self.read_blocked(block, taking)?,
            Interleave::PerByte => self.read_per_byte(taking)?,
        }
        self.at = self.at.saturating_add(taking);
        Ok(())
    }

    fn read_blocked(&mut self, block: u64, taking: u64) -> Result<()> {
        let lanes = self.layout.lanes() as u64;
        for plane in &mut self.planes {
            plane.clear();
        }

        let mut done = 0;
        while done < taking {
            let at = self.at.saturating_add(done);
            let index = at / block;
            let within = at % block;
            let step = (block - within).min(taking - done);

            for lane in 0..lanes {
                let offset = self
                    .layout
                    .data_at
                    .saturating_add(index.saturating_mul(block).saturating_mul(lanes))
                    .saturating_add(lane.saturating_mul(block))
                    .saturating_add(within);
                let into = self
                    .planes
                    .get_mut(lane as usize)
                    .expect("a lane per plane");
                read_at(self.bytes.as_mut(), offset, step as usize, into)?;
            }
            done = done.saturating_add(step);
        }
        Ok(())
    }

    fn read_per_byte(&mut self, taking: u64) -> Result<()> {
        let lanes = self.layout.lanes();
        let wanted = taking as usize;
        let held = &mut self.interleaved;
        held.clear();
        held.resize(wanted.saturating_mul(lanes), 0);

        let offset = self
            .layout
            .data_at
            .saturating_add(self.at.saturating_mul(lanes as u64));
        if self.bytes.seek(SeekFrom::Start(offset)).is_err() {
            return Ok(());
        }
        let read = fill(self.bytes.as_mut(), held);
        held.truncate(read);

        for (lane, plane) in self.planes.iter_mut().enumerate() {
            plane.clear();
            plane.extend(held.iter().skip(lane).step_by(lanes).copied());
        }
        Ok(())
    }
}

fn read_at(
    bytes: &mut dyn MediaStream,
    offset: u64,
    wanted: usize,
    into: &mut Vec<u8>,
) -> Result<()> {
    if bytes.seek(SeekFrom::Start(offset)).is_err() {
        return Ok(());
    }
    let from = into.len();
    into.resize(from.saturating_add(wanted), 0);
    let read = into.get_mut(from..).map_or(0, |held| fill(bytes, held));
    into.truncate(from.saturating_add(read));
    Ok(())
}

fn fill(bytes: &mut dyn MediaStream, into: &mut [u8]) -> usize {
    let mut read = 0;
    while read < into.len() {
        let Some(rest) = into.get_mut(read..) else {
            break;
        };
        match bytes.read(rest) {
            Ok(0) | Err(_) => break,
            Ok(taken) => read = read.saturating_add(taken),
        }
    }
    read
}

pub(crate) fn info(layout: &Layout, is_seekable: bool, tags: crate::TagSet) -> MediaInfo {
    let container = match layout.container {
        Container::Dsf => DSF_FORMAT_ID,
        Container::Dff => DFF_FORMAT_ID,
    };

    MediaInfo {
        container,
        codec: DSD_CODEC_ID,
        spec: resonate_core::StreamSpec::new(
            layout.rate.carrier(),
            layout.channels,
            resonate_core::SampleFormat::S24,
        ),
        speakers: crate::Speakers::UNNAMED,
        duration: Some(layout.frames()),
        encoder_delay: 0,
        encoder_padding: 0,
        playable: None,
        bits_per_coded_sample: Some(1),
        is_seekable,
        packing: Packing::DopMarked(layout.rate),
        tags,
        cue: None,
    }
}

const MAX_METADATA_BYTES: u64 = 1 << 20;

pub(crate) fn tags(bytes: &mut dyn MediaStream, layout: &Layout) -> crate::TagSet {
    let mut tags = embedded_tags(bytes, layout);
    let Edited { artist, title } = layout.edited.clone();
    tags.artist = tags.artist.or(artist);
    tags.title = tags.title.or(title);
    tags
}

fn embedded_tags(bytes: &mut dyn MediaStream, layout: &Layout) -> crate::TagSet {
    use symphonia::core::{
        io::{MediaSourceStream, MediaSourceStreamOptions},
        meta::{MetadataOptions, MetadataReader},
    };

    let Some(at) = layout.metadata_at else {
        return crate::TagSet::default();
    };
    if bytes.seek(SeekFrom::Start(at)).is_err() {
        return crate::TagSet::default();
    }

    let mut held = Vec::new();
    if bytes
        .take(MAX_METADATA_BYTES)
        .read_to_end(&mut held)
        .is_err()
    {
        return crate::TagSet::default();
    }

    let stream = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(held)),
        MediaSourceStreamOptions::default(),
    );
    let Ok(mut reader) =
        symphonia::default::meta::Id3v2Reader::try_new(stream, MetadataOptions::default())
    else {
        return crate::TagSet::default();
    };
    let Ok(buffered) = reader.read_all() else {
        return crate::TagSet::default();
    };

    crate::tags::from_revision(&buffered.revision)
}

pub(crate) struct Stream {
    planes: Planes,
    dop: Dop,
    decimator: Option<Decimator>,
    decimated: Vec<f32>,
    packing: Packing,
    lanes: usize,
    frame: u64,
    rate: DsdRate,
    bits: BitOrder,
}

impl Stream {
    pub(crate) fn over(bytes: Box<dyn MediaStream>, layout: Layout, packing: Packing) -> Self {
        let lanes = layout.lanes();
        let bits = layout.bits;
        let rate = layout.rate;

        Self {
            dop: Dop::reading(bits),
            decimator: matches!(packing, Packing::Samples)
                .then(|| Decimator::new(rate.hz(), rate.carrier().hz(), lanes, bits)),
            decimated: Vec::with_capacity(DSD_BLOCK_FRAMES as usize * lanes),
            packing,
            lanes,
            frame: 0,
            rate,
            bits,
            planes: Planes::over(bytes, layout),
        }
    }

    pub(crate) fn deliver(&mut self, packing: Packing) {
        let rate = self.rate;
        let bits = self.bits;
        self.packing = packing;
        self.decimator = matches!(packing, Packing::Samples)
            .then(|| Decimator::new(rate.hz(), rate.carrier().hz(), self.lanes, bits));
    }

    pub(crate) fn position(&self) -> u64 {
        self.planes.at() / BYTES_PER_DOP_FRAME
    }

    pub(crate) fn seek(&mut self, to: Frames) -> Result<Frames> {
        self.planes
            .seek_to_byte(to.get().saturating_mul(BYTES_PER_DOP_FRAME))?;
        self.frame = to.get();
        if let Some(decimator) = self.decimator.as_mut() {
            decimator.prime();
        }
        Ok(Frames(self.position()))
    }

    pub(crate) fn reset(&mut self) {
        if let Some(decimator) = self.decimator.as_mut() {
            decimator.prime();
        }
    }

    pub(crate) fn skip(&mut self, frames: u64) -> u64 {
        let before = self.planes.at();
        let _ = self
            .planes
            .seek_to_byte(before.saturating_add(frames.saturating_mul(BYTES_PER_DOP_FRAME)));
        let moved = self.planes.at().saturating_sub(before) / BYTES_PER_DOP_FRAME;
        self.frame = self.frame.saturating_add(moved);
        moved
    }

    pub(crate) fn next_block(
        &mut self,
        out: &mut resonate_core::AudioBuffer,
        wanted: Option<u64>,
        block_frames: u64,
    ) -> Result<usize> {
        let frames = wanted.map_or(block_frames, |left| left.min(block_frames));
        self.planes
            .read(frames.saturating_mul(BYTES_PER_DOP_FRAME))?;

        let taking = self.planes.shortest() / BYTES_PER_DOP_FRAME;
        if taking == 0 {
            return Ok(0);
        }
        out.set_frames(taking as usize);

        let lanes = self.lanes;
        let first = self.frame;
        let held = self.planes.held();

        match self.packing {
            Packing::DopMarked(_) => pack(held, self.dop, lanes, first, out),
            Packing::Samples => {
                if let Some(decimator) = self.decimator.as_mut() {
                    self.decimated.clear();
                    decimator.frames(held, &mut self.decimated, lanes);
                    out.data_mut().write_f32(&self.decimated);
                }
            }
        }

        self.frame = self.frame.saturating_add(taking);
        Ok(taking as usize)
    }
}

fn pack(
    planes: &[Vec<u8>],
    dop: Dop,
    lanes: usize,
    first: u64,
    out: &mut resonate_core::AudioBuffer,
) {
    let frames = out.frames();
    let resonate_core::SampleData::S24(into) = out.data_mut() else {
        return;
    };

    for frame in 0..frames {
        for lane in 0..lanes {
            let Some(plane) = planes.get(lane) else {
                continue;
            };
            let at = frame * BYTES_PER_DOP_FRAME as usize;
            let earlier = plane.get(at).copied().unwrap_or(DSD_SILENCE);
            let later = plane.get(at + 1).copied().unwrap_or(DSD_SILENCE);

            if let Some(slot) = into.get_mut(frame * lanes + lane) {
                *slot = dop.frame(first.saturating_add(frame as u64), earlier, later);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use resonate_core::{AudioBuffer, SampleData, SampleFormat, SampleRate, StreamSpec};

    use super::*;
    use crate::source::Reading;

    const DSD64: u32 = 2_822_400;
    const S24_GREATEST: i32 = 8_388_607;
    const S24_LEAST: i32 = -8_388_608;

    fn decimating(plane: u8, blocks: usize) -> Stream {
        let bytes = vec![plane; blocks * DSD_BLOCK_FRAMES as usize * BYTES_PER_DOP_FRAME as usize];
        let layout = Layout {
            container: Container::Dsf,
            rate: DsdRate::new(DSD64).expect("dsd64"),
            channels: ChannelLayout::Mono,
            samples: bytes.len() as u64 * 8,
            data_at: 0,
            data_bytes: bytes.len() as u64,
            order: Interleave::PerByte,
            bits: BitOrder::MostSignificantFirst,
            metadata_at: None,
            edited: Edited::default(),
        };
        Stream::over(
            Box::new(Reading::new(Cursor::new(bytes))),
            layout,
            Packing::Samples,
        )
    }

    fn twenty_four_bit() -> AudioBuffer {
        AudioBuffer::empty(StreamSpec::new(
            SampleRate::HZ_176400,
            ChannelLayout::Mono,
            SampleFormat::S24,
        ))
    }

    fn first_block_in_s24(plane: u8) -> Vec<i32> {
        let mut stream = decimating(plane, 1);
        let mut out = twenty_four_bit();
        stream
            .next_block(&mut out, None, DSD_BLOCK_FRAMES)
            .expect("a block decimates");
        match out.data() {
            SampleData::S24(samples) => samples.clone(),
            other => panic!("expected S24, got {other:?}"),
        }
    }

    #[test]
    fn a_decimated_plane_past_full_scale_lands_on_the_greatest_24_bit_value_rather_than_wrapping() {
        let high = first_block_in_s24(0xFF);
        let low = first_block_in_s24(0x00);

        assert_eq!(
            high.iter().max(),
            Some(&S24_GREATEST),
            "positive full scale reached a 24-bit word as something other than its greatest value"
        );
        assert_eq!(low.iter().min(), Some(&S24_LEAST));
    }

    #[test]
    fn decimating_a_block_writes_into_the_samples_the_last_block_left_behind() {
        let mut stream = decimating(DSD_SILENCE, 2);
        let store = stream.decimated.as_ptr();
        let mut out = twenty_four_bit();

        for _ in 0..2 {
            let taken = stream
                .next_block(&mut out, None, DSD_BLOCK_FRAMES)
                .expect("a block decimates");
            assert_eq!(taken, DSD_BLOCK_FRAMES as usize);
            assert_eq!(
                stream.decimated.as_ptr(),
                store,
                "the decimated samples were reallocated"
            );
        }
    }
}
