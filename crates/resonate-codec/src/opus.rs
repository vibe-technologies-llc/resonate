use std::sync::LazyLock;

use opus_rs::{
    OpusDecoder,
    multistream::{ChannelMappingTable, MultistreamDecoder},
};
use resonate_core::Frames;
use symphonia::core::{
    audio::{AsGenericAudioBufferRef, AudioBuffer, AudioMut, AudioSpec, Channels, Position},
    codecs::{
        CodecInfo,
        audio::{
            AudioCodecId, AudioCodecParameters, AudioDecoder, AudioDecoderOptions, FinalizeResult,
            well_known::CODEC_ID_OPUS,
        },
        registry::{CodecRegistry, RegisterableAudioDecoder, SupportedAudioCodec},
    },
    errors::{Result, decode_error, unsupported_error},
    packet::PacketRef,
};

pub(crate) const OPUS_RATE: u32 = 48_000;
const PRE_ROLL: Frames = Frames(19_200);

const LONGEST_PACKET_FRAMES: usize = 5_760;
const MOST_CHANNELS: usize = 8;
const HEAD_MAGIC: &[u8; 8] = b"OpusHead";
const HEAD_BYTES: usize = 19;
const VERSION_AT: usize = 8;
const CHANNELS_AT: usize = 9;
const PRE_SKIP_AT: usize = 10;
const GAIN_AT: usize = 16;
const FAMILY_AT: usize = 18;
const ISO_BMFF_VERSION: u8 = 0;
const SILENT_CHANNEL: u8 = 255;
const DB_PER_DECADE: f32 = 20.0;
const STEREO_FLAG: u8 = 0b100;
const GAIN_STEPS_PER_DB: f32 = 256.0;

const VORBIS_ORDER: [&[Position]; MOST_CHANNELS] = [
    &[Position::FRONT_LEFT],
    &[Position::FRONT_LEFT, Position::FRONT_RIGHT],
    &[
        Position::FRONT_LEFT,
        Position::FRONT_CENTER,
        Position::FRONT_RIGHT,
    ],
    &[
        Position::FRONT_LEFT,
        Position::FRONT_RIGHT,
        Position::REAR_LEFT,
        Position::REAR_RIGHT,
    ],
    &[
        Position::FRONT_LEFT,
        Position::FRONT_CENTER,
        Position::FRONT_RIGHT,
        Position::REAR_LEFT,
        Position::REAR_RIGHT,
    ],
    &[
        Position::FRONT_LEFT,
        Position::FRONT_CENTER,
        Position::FRONT_RIGHT,
        Position::REAR_LEFT,
        Position::REAR_RIGHT,
        Position::LFE1,
    ],
    &[
        Position::FRONT_LEFT,
        Position::FRONT_CENTER,
        Position::FRONT_RIGHT,
        Position::SIDE_LEFT,
        Position::SIDE_RIGHT,
        Position::REAR_CENTER,
        Position::LFE1,
    ],
    &[
        Position::FRONT_LEFT,
        Position::FRONT_CENTER,
        Position::FRONT_RIGHT,
        Position::SIDE_LEFT,
        Position::SIDE_RIGHT,
        Position::REAR_LEFT,
        Position::REAR_RIGHT,
        Position::LFE1,
    ],
];

static CODECS: LazyLock<CodecRegistry> = LazyLock::new(|| {
    let mut registry = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    registry.register_audio_decoder::<Opus>();
    registry
});

pub(crate) fn codecs() -> &'static CodecRegistry {
    &CODECS
}

pub(crate) fn pre_roll(codec: AudioCodecId) -> Frames {
    if codec == CODEC_ID_OPUS {
        PRE_ROLL
    } else {
        Frames::ZERO
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Head {
    channels: u8,
    pub(crate) pre_skip: u16,
    gain: i16,
    table: Option<ChannelMappingTable>,
}

impl Head {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        let fixed = bytes.get(..HEAD_BYTES)?;
        if fixed[..HEAD_MAGIC.len()] != *HEAD_MAGIC {
            return None;
        }
        let big_endian = fixed[VERSION_AT] == ISO_BMFF_VERSION;
        let pair = |at: usize| [fixed[at], fixed[at + 1]];
        let (pre_skip, gain) = if big_endian {
            (
                u16::from_be_bytes(pair(PRE_SKIP_AT)),
                i16::from_be_bytes(pair(GAIN_AT)),
            )
        } else {
            (
                u16::from_le_bytes(pair(PRE_SKIP_AT)),
                i16::from_le_bytes(pair(GAIN_AT)),
            )
        };
        let channels = fixed[CHANNELS_AT];
        let table = match fixed[FAMILY_AT] {
            0 => None,
            family => Some(
                ChannelMappingTable::parse(family, channels, &bytes[HEAD_BYTES..])
                    .filter(routes_only_what_it_decodes)?,
            ),
        };
        if channels == 0 || (table.is_none() && channels > 2) {
            return None;
        }

        Some(Self {
            channels,
            pre_skip,
            gain,
            table,
        })
    }

    fn amplitude(&self) -> Option<f32> {
        (self.gain != 0)
            .then(|| 10_f32.powf(f32::from(self.gain) / GAIN_STEPS_PER_DB / DB_PER_DECADE))
    }
}

fn routes_only_what_it_decodes(table: &ChannelMappingTable) -> bool {
    let Some(mono) = table.stream_count.checked_sub(table.coupled_count) else {
        return false;
    };
    let decoded = usize::from(table.coupled_count) * 2 + usize::from(mono);
    table.stream_count > 0
        && table
            .mapping
            .iter()
            .all(|source| *source == SILENT_CHANNEL || usize::from(*source) < decoded)
}

enum Streams {
    Plain {
        decoder: Box<OpusDecoder>,
        other: Option<Box<OpusDecoder>>,
    },
    Multi(MultistreamDecoder),
}

impl Streams {
    fn of(head: &Head) -> Result<Self> {
        let rate = OPUS_RATE as i32;
        match &head.table {
            Some(table) => MultistreamDecoder::new(rate, table.clone())
                .map(Self::Multi)
                .or_else(decode_error),
            None => OpusDecoder::new(rate, usize::from(head.channels))
                .map(|decoder| Self::Plain {
                    decoder: Box::new(decoder),
                    other: None,
                })
                .or_else(decode_error),
        }
    }

    fn decode(
        &mut self,
        packet: &[u8],
        channels: usize,
        scratch: &mut [f32],
        out: &mut [f32],
    ) -> std::result::Result<usize, &'static str> {
        match self {
            Self::Multi(decoder) => decoder.decode(packet, LONGEST_PACKET_FRAMES, out),
            Self::Plain { decoder, other } => {
                let carried = packet
                    .first()
                    .map_or(channels, |toc| if toc & STEREO_FLAG == 0 { 1 } else { 2 });
                if carried == channels {
                    return decoder.decode(packet, LONGEST_PACKET_FRAMES, out);
                }
                let other = match other {
                    Some(other) => other,
                    None => other.insert(Box::new(OpusDecoder::new(OPUS_RATE as i32, carried)?)),
                };
                let frames = other.decode(packet, LONGEST_PACKET_FRAMES, scratch)?;
                fold_to(carried, channels, &scratch[..frames * carried], out);
                Ok(frames)
            }
        }
    }
}

fn fold_to(carried: usize, channels: usize, from: &[f32], into: &mut [f32]) {
    for (frame, to) in from
        .chunks_exact(carried)
        .zip(into.chunks_exact_mut(channels))
    {
        let mean = frame.iter().sum::<f32>() / carried as f32;
        to.fill(mean);
    }
}

pub(crate) struct Opus {
    params: AudioCodecParameters,
    head: Head,
    streams: Streams,
    planes: [usize; MOST_CHANNELS],
    amplitude: Option<f32>,
    interleaved: Vec<f32>,
    scratch: Vec<f32>,
    buffer: AudioBuffer<f32>,
}

impl Opus {
    fn try_new(params: &AudioCodecParameters) -> Result<Self> {
        let Some(channels) = params.channels.clone() else {
            return unsupported_error("opus: the channels are not declared");
        };
        let head = match params.extra_data.as_deref() {
            Some(extra) => match Head::read(extra) {
                Some(head) => head,
                None => return unsupported_error("opus: the identification header is unreadable"),
            },
            None => Head {
                channels: u8::try_from(channels.count()).unwrap_or(u8::MAX),
                pre_skip: 0,
                gain: 0,
                table: None,
            },
        };
        let count = usize::from(head.channels);
        if count != channels.count() || count > MOST_CHANNELS || count == 0 {
            return unsupported_error("opus: the header and the track disagree about the channels");
        }

        Ok(Self {
            params: params.clone(),
            streams: Streams::of(&head)?,
            planes: planes_of(&channels, count),
            amplitude: head.amplitude(),
            head,
            interleaved: vec![0.0; LONGEST_PACKET_FRAMES * count],
            scratch: vec![0.0; LONGEST_PACKET_FRAMES * 2],
            buffer: AudioBuffer::new(AudioSpec::new(OPUS_RATE, channels), LONGEST_PACKET_FRAMES),
        })
    }

    fn channels(&self) -> usize {
        usize::from(self.head.channels)
    }
}

fn planes_of(channels: &Channels, count: usize) -> [usize; MOST_CHANNELS] {
    let mut planes = [0, 1, 2, 3, 4, 5, 6, 7];
    let Channels::Positioned(held) = channels else {
        return planes;
    };
    let order = VORBIS_ORDER[count - 1];
    if order.iter().any(|position| !held.contains(*position)) {
        return planes;
    }
    for (plane, position) in planes.iter_mut().zip(order) {
        *plane = (held.bits() & (position.bits() - 1)).count_ones() as usize;
    }
    planes
}

impl AudioDecoder for Opus {
    fn reset(&mut self) {
        match Streams::of(&self.head) {
            Ok(streams) => self.streams = streams,
            Err(error) => tracing::warn!(%error, "an Opus decoder could not be built again"),
        }
        self.buffer.clear();
    }

    fn codec_info(&self) -> &CodecInfo {
        &Self::supported_codecs()[0].info
    }

    fn codec_params(&self) -> &AudioCodecParameters {
        &self.params
    }

    fn decode_ref(
        &mut self,
        packet: &PacketRef<'_>,
    ) -> Result<symphonia::core::audio::GenericAudioBufferRef<'_>> {
        let channels = self.channels();
        let frames = match self.streams.decode(
            packet.data,
            channels,
            &mut self.scratch,
            &mut self.interleaved,
        ) {
            Ok(frames) => frames,
            Err(reason) => {
                self.buffer.clear();
                return decode_error(reason);
            }
        };

        self.buffer.clear();
        self.buffer.render_uninit(Some(frames));
        let decoded = &self.interleaved[..frames * channels];
        let amplitude = self.amplitude.unwrap_or(1.0);
        for (channel, plane_at) in self.planes[..channels].iter().enumerate() {
            let Some(plane) = self.buffer.plane_mut(*plane_at) else {
                continue;
            };
            for (sample, frame) in plane.iter_mut().zip(decoded.chunks_exact(channels)) {
                *sample = frame[channel] * amplitude;
            }
        }

        Ok(self.buffer.as_generic_audio_buffer_ref())
    }

    fn finalize(&mut self) -> FinalizeResult {
        FinalizeResult::default()
    }

    fn last_decoded(&self) -> symphonia::core::audio::GenericAudioBufferRef<'_> {
        self.buffer.as_generic_audio_buffer_ref()
    }
}

impl RegisterableAudioDecoder for Opus {
    fn try_registry_new(
        params: &AudioCodecParameters,
        _options: &AudioDecoderOptions,
    ) -> Result<Box<dyn AudioDecoder>> {
        Ok(Box::new(Self::try_new(params)?))
    }

    fn supported_codecs() -> &'static [SupportedAudioCodec] {
        &[SupportedAudioCodec {
            id: CODEC_ID_OPUS,
            info: CodecInfo {
                short_name: "opus",
                long_name: "Opus",
                profiles: &[],
            },
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(version: u8, channels: u8, pre_skip: [u8; 2], gain: [u8; 2], family: u8) -> Vec<u8> {
        let mut bytes = HEAD_MAGIC.to_vec();
        bytes.extend_from_slice(&[version, channels]);
        bytes.extend_from_slice(&pre_skip);
        bytes.extend_from_slice(&48_000_u32.to_le_bytes());
        bytes.extend_from_slice(&gain);
        bytes.push(family);
        bytes
    }

    #[test]
    fn an_ogg_head_is_read_little_endian_and_an_mp4_one_big_endian() {
        let ogg = Head::read(&head(
            1,
            2,
            312_u16.to_le_bytes(),
            (-256_i16).to_le_bytes(),
            0,
        ));
        let mp4 = Head::read(&head(
            0,
            2,
            312_u16.to_be_bytes(),
            (-256_i16).to_be_bytes(),
            0,
        ));

        for read in [ogg, mp4] {
            let read = read.expect("a well-formed head");
            assert_eq!(read.pre_skip, 312);
            assert_eq!(read.gain, -256);
            assert_eq!(read.table, None);
        }
    }

    #[test]
    fn the_output_gain_is_an_amplitude_and_none_where_it_is_nothing() {
        let quieter = Head::read(&head(1, 1, [0, 0], (-1_536_i16).to_le_bytes(), 0))
            .expect("a well-formed head");
        let plain = Head::read(&head(1, 1, [0, 0], [0, 0], 0)).expect("a well-formed head");

        let amplitude = quieter.amplitude().expect("a gain to apply");
        assert!((amplitude - 10_f32.powf(-6.0 / 20.0)).abs() < 1e-6);
        assert_eq!(plain.amplitude(), None);
    }

    #[test]
    fn a_surround_head_carries_its_mapping_table_and_a_wide_plain_one_is_refused() {
        let mut surround = head(1, 6, [0, 0], [0, 0], 1);
        surround.extend_from_slice(&[4, 2, 0, 4, 1, 2, 3, 5]);

        let read = Head::read(&surround).expect("a surround head");
        let table = read.table.expect("a mapping table");
        assert_eq!((table.stream_count, table.coupled_count), (4, 2));
        assert_eq!(table.mapping, vec![0, 4, 1, 2, 3, 5]);

        assert_eq!(Head::read(&head(1, 6, [0, 0], [0, 0], 0)), None);
        let mut past_what_is_decoded = surround.clone();
        past_what_is_decoded[HEAD_BYTES + 2] = 6;
        assert_eq!(Head::read(&past_what_is_decoded), None);
        let mut more_coupled_than_streams = surround.clone();
        more_coupled_than_streams[HEAD_BYTES + 1] = 5;
        assert_eq!(Head::read(&more_coupled_than_streams), None);
        assert_eq!(Head::read(&surround[..HEAD_BYTES + 3]), None);
        assert_eq!(Head::read(b"OpusTags"), None);
    }

    #[test]
    fn a_vorbis_ordered_channel_lands_on_the_plane_its_position_takes() {
        let five_one = Position::FRONT_LEFT
            | Position::FRONT_RIGHT
            | Position::FRONT_CENTER
            | Position::LFE1
            | Position::REAR_LEFT
            | Position::REAR_RIGHT;

        assert_eq!(
            planes_of(&Channels::Positioned(five_one), 6)[..6],
            [0, 2, 1, 4, 5, 3]
        );
        assert_eq!(
            planes_of(
                &Channels::Positioned(Position::FRONT_LEFT | Position::FRONT_RIGHT),
                2
            )[..2],
            [0, 1]
        );
    }

    #[test]
    fn a_packet_carrying_fewer_channels_than_the_stream_is_heard_on_all_of_them() {
        let mut stereo = [0.0; 4];
        fold_to(1, 2, &[0.5, -0.25], &mut stereo);
        assert_eq!(stereo, [0.5, 0.5, -0.25, -0.25]);

        let mut mono = [0.0; 2];
        fold_to(2, 1, &[0.5, 0.25, -1.0, 0.0], &mut mono);
        assert_eq!(mono, [0.375, -0.5]);
    }
}
