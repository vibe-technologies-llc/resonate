use std::time::Duration;

use resonate_codec::{DecodeStatus, Decoder, Sources, read_hrir};
use resonate_core::{
    AudioBuffer, Frames, MediaLocation, SampleData, SampleFormat, SpeakerAngle, StreamSpec,
};
use resonate_dsp::{Impulse, Speaker};
use thiserror::Error;

pub const LONGEST_IMPULSE: Duration = Duration::from_secs(10);

#[derive(Debug, Error)]
pub enum PairedReadError {
    #[error("the {speaker} speaker's response could not be read")]
    Read {
        speaker: Speaker,
        #[source]
        source: resonate_codec::Error,
    },

    #[error("the {speaker} speaker's file holds no response")]
    Empty { speaker: Speaker },

    #[error(transparent)]
    Pair(#[from] resonate_dsp::Error),
}

pub fn read_impulse(
    sources: &Sources,
    location: &MediaLocation,
    angle: SpeakerAngle,
) -> resonate_codec::Result<Option<Impulse>> {
    Ok(read_unscaled(sources, location, angle)?.map(Impulse::with_headroom))
}

pub fn read_binaural_pair(
    sources: &Sources,
    left: &MediaLocation,
    right: &MediaLocation,
    angle: SpeakerAngle,
) -> Result<Impulse, PairedReadError> {
    let heard = |speaker, location| {
        read_unscaled(sources, location, angle)
            .map_err(|source| PairedReadError::Read { speaker, source })?
            .ok_or(PairedReadError::Empty { speaker })
    };
    let left = heard(Speaker::Left, left)?;
    let right = heard(Speaker::Right, right)?;
    Ok(Impulse::binaural(&left, &right)?.with_headroom())
}

const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const QUAD_SPEAKERS: u32 = 0x33;
const IEEE_FLOAT_SUBFORMAT: [u8; 16] = [
    3, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71,
];
const FLOAT_BITS: u16 = 32;

pub fn binaural_wave(impulse: &Impulse) -> Result<Vec<u8>, PairedReadError> {
    let taps = impulse.at(impulse.rate())?;
    let channels = u16::try_from(taps.len()).unwrap_or(u16::MAX);
    let frames = impulse.frames();
    let block_align = channels * (FLOAT_BITS / 8);
    let data_bytes = u32::try_from(frames * usize::from(block_align)).unwrap_or(u32::MAX);
    let rate = impulse.rate().hz();

    let mut file = Vec::with_capacity(68 + data_bytes as usize);
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(60 + data_bytes).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&40_u32.to_le_bytes());
    file.extend_from_slice(&WAVE_FORMAT_EXTENSIBLE.to_le_bytes());
    file.extend_from_slice(&channels.to_le_bytes());
    file.extend_from_slice(&rate.to_le_bytes());
    file.extend_from_slice(&(rate * u32::from(block_align)).to_le_bytes());
    file.extend_from_slice(&block_align.to_le_bytes());
    file.extend_from_slice(&FLOAT_BITS.to_le_bytes());
    file.extend_from_slice(&22_u16.to_le_bytes());
    file.extend_from_slice(&FLOAT_BITS.to_le_bytes());
    file.extend_from_slice(&QUAD_SPEAKERS.to_le_bytes());
    file.extend_from_slice(&IEEE_FLOAT_SUBFORMAT);
    file.extend_from_slice(b"data");
    file.extend_from_slice(&data_bytes.to_le_bytes());
    for frame in 0..frames {
        for channel in taps.iter() {
            let tap = channel.get(frame).copied().unwrap_or(0.0);
            file.extend_from_slice(&(tap as f32).to_le_bytes());
        }
    }
    Ok(file)
}

fn read_unscaled(
    sources: &Sources,
    location: &MediaLocation,
    angle: SpeakerAngle,
) -> resonate_codec::Result<Option<Impulse>> {
    if let Some(hrir) = read_hrir(sources, location, LONGEST_IMPULSE, angle)? {
        return Ok(Impulse::crossed(hrir.rate, hrir.responses));
    }

    let (mut decoder, info) = Decoder::open(sources, location)?;
    decoder.set_output_format(SampleFormat::F32);
    let rate = info.spec.rate;
    let channels = usize::from(info.spec.channel_count().get());
    let most = usize::try_from(Frames::from_duration(LONGEST_IMPULSE, rate).get()).unwrap_or(0);
    let mut taps: Vec<Vec<f64>> = vec![Vec::new(); channels];
    let mut block =
        AudioBuffer::empty(StreamSpec::new(rate, info.spec.channels, SampleFormat::F32));

    while taps.first().is_some_and(|first| first.len() < most) {
        if decoder.next_block(&mut block)? == DecodeStatus::EndOfStream {
            break;
        }
        let SampleData::F32(samples) = block.data() else {
            continue;
        };
        for frame in samples.chunks_exact(channels) {
            for (channel, sample) in taps.iter_mut().zip(frame) {
                channel.push(f64::from(*sample));
            }
        }
    }
    for channel in &mut taps {
        channel.truncate(most);
    }

    let impulse = if channels == resonate_dsp::CROSSED_RESPONSES {
        Impulse::crossed(rate, taps)
    } else {
        Impulse::new(rate, taps)
    };
    Ok(impulse)
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

    use hdf5_writer::{AttributeBuilder, DatasetBuilder, Hdf5Builder, WriteOptions};
    use resonate_core::SampleRate;

    use super::*;

    fn wave(rate: u32, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        file.extend_from_slice(b"WAVEfmt ");
        file.extend_from_slice(&16_u32.to_le_bytes());
        file.extend_from_slice(&1_u16.to_le_bytes());
        file.extend_from_slice(&1_u16.to_le_bytes());
        file.extend_from_slice(&rate.to_le_bytes());
        file.extend_from_slice(&(rate * 2).to_le_bytes());
        file.extend_from_slice(&2_u16.to_le_bytes());
        file.extend_from_slice(&16_u16.to_le_bytes());
        file.extend_from_slice(b"data");
        file.extend_from_slice(&(data.len() as u32).to_le_bytes());
        file.extend_from_slice(&data);
        file
    }

    #[test]
    fn a_response_is_read_out_of_a_file_as_the_taps_it_holds() {
        let path = env::temp_dir().join(format!("resonate-impulse-{}.wav", process::id()));
        fs::write(&path, wave(48_000, &[16_384, 0, -8_192])).expect("a scratch response");

        let read = read_impulse(
            &Sources::local(),
            &MediaLocation::local(&path),
            SpeakerAngle::STANDARD,
        )
        .expect("a readable response")
        .expect("a response with taps");
        let _ = fs::remove_file(&path);

        assert_eq!(read.channels(), 1);
        assert_eq!(read.frames(), 3);
        assert_eq!(read.rate().hz(), 48_000);
    }

    fn wave_of(channels: u16, rate: u32, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        file.extend_from_slice(b"WAVEfmt ");
        file.extend_from_slice(&16_u32.to_le_bytes());
        file.extend_from_slice(&1_u16.to_le_bytes());
        file.extend_from_slice(&channels.to_le_bytes());
        file.extend_from_slice(&rate.to_le_bytes());
        file.extend_from_slice(&(rate * 2 * u32::from(channels)).to_le_bytes());
        file.extend_from_slice(&(2 * channels).to_le_bytes());
        file.extend_from_slice(&16_u16.to_le_bytes());
        file.extend_from_slice(b"data");
        file.extend_from_slice(&(data.len() as u32).to_le_bytes());
        file.extend_from_slice(&data);
        file
    }

    #[test]
    fn a_four_channel_file_is_read_as_a_two_by_two_response() {
        let path = env::temp_dir().join(format!("resonate-binaural-{}.wav", process::id()));
        fs::write(
            &path,
            wave_of(4, 48_000, &[16_384, 0, 0, 16_384, 8_192, 0, 0, 8_192]),
        )
        .expect("a scratch response");

        let read = read_impulse(
            &Sources::local(),
            &MediaLocation::local(&path),
            SpeakerAngle::STANDARD,
        )
        .expect("a readable response")
        .expect("a response with taps");
        let _ = fs::remove_file(&path);

        assert!(read.is_crossed());
        assert_eq!(read.channels(), 4);
        assert_eq!(read.frames(), 2);
    }

    fn scratch(name: &str, channels: u16, rate: u32, samples: &[i16]) -> std::path::PathBuf {
        let path = env::temp_dir().join(format!("resonate-{name}-{}.wav", process::id()));
        fs::write(&path, wave_of(channels, rate, samples)).expect("a scratch response");
        path
    }

    #[test]
    fn two_stereo_files_are_read_as_one_two_by_two_response() {
        let left = scratch("pair-left", 2, 48_000, &[16_384, 8_192, 0, 0]);
        let right = scratch("pair-right", 2, 48_000, &[4_096, 16_384, 0, 0]);

        let pair = read_binaural_pair(
            &Sources::local(),
            &MediaLocation::local(&left),
            &MediaLocation::local(&right),
            SpeakerAngle::STANDARD,
        )
        .expect("a pair");
        let _ = fs::remove_file(&left);
        let _ = fs::remove_file(&right);

        assert!(pair.is_crossed());
        assert_eq!(pair.channels(), 4);
        assert_eq!(pair.frames(), 2);
    }

    #[test]
    fn a_pair_of_files_at_two_rates_or_two_lengths_or_not_stereo_is_refused() {
        let left = scratch("pair-refused-left", 2, 48_000, &[16_384, 0, 0, 0]);
        let slower = scratch("pair-refused-rate", 2, 44_100, &[16_384, 0, 0, 0]);
        let longer = scratch("pair-refused-length", 2, 48_000, &[16_384, 0, 0, 0, 0, 0]);
        let mono = scratch("pair-refused-mono", 1, 48_000, &[16_384, 0]);
        let sources = Sources::local();
        let at = |path: &std::path::Path| MediaLocation::local(path);

        let rates = read_binaural_pair(&sources, &at(&left), &at(&slower), SpeakerAngle::STANDARD);
        let lengths =
            read_binaural_pair(&sources, &at(&left), &at(&longer), SpeakerAngle::STANDARD);
        let channels = read_binaural_pair(&sources, &at(&left), &at(&mono), SpeakerAngle::STANDARD);
        for path in [&left, &slower, &longer, &mono] {
            let _ = fs::remove_file(path);
        }

        assert!(matches!(
            rates,
            Err(PairedReadError::Pair(
                resonate_dsp::Error::SpeakerRatesDiffer { .. }
            ))
        ));
        assert!(matches!(
            lengths,
            Err(PairedReadError::Pair(
                resonate_dsp::Error::SpeakerLengthsDiffer { left: 2, right: 3 }
            ))
        ));
        assert!(matches!(
            channels,
            Err(PairedReadError::Pair(
                resonate_dsp::Error::SpeakerNotStereo {
                    speaker: Speaker::Right,
                    channels: 1
                }
            ))
        ));
    }

    #[test]
    fn an_assembled_pair_written_as_a_wave_reads_back_as_the_same_two_by_two_response() {
        let left = scratch("wave-left", 2, 48_000, &[16_384, 8_192, 0, 4_096]);
        let right = scratch("wave-right", 2, 48_000, &[4_096, 16_384, 2_048, 0]);
        let sources = Sources::local();
        let pair = read_binaural_pair(
            &sources,
            &MediaLocation::local(&left),
            &MediaLocation::local(&right),
            SpeakerAngle::STANDARD,
        )
        .expect("a pair");
        let _ = fs::remove_file(&left);
        let _ = fs::remove_file(&right);

        let written = env::temp_dir().join(format!("resonate-wave-both-{}.wav", process::id()));
        fs::write(&written, binaural_wave(&pair).expect("a wave")).expect("a scratch file");
        let read = read_impulse(
            &sources,
            &MediaLocation::local(&written),
            SpeakerAngle::STANDARD,
        )
        .expect("a readable response")
        .expect("a response with taps");
        let _ = fs::remove_file(&written);

        assert!(read.is_crossed());
        assert_eq!(read.frames(), pair.frames());
        let before = pair.at(pair.rate()).expect("taps");
        let after = read.at(read.rate()).expect("taps");
        for (a, b) in before.iter().flatten().zip(after.iter().flatten()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn a_response_boosting_past_unity_is_read_with_the_headroom_it_needs() {
        let path = env::temp_dir().join(format!("resonate-boosting-{}.wav", process::id()));
        fs::write(&path, wave(48_000, &[16_384, 16_384, 16_384])).expect("a scratch response");

        let read = read_impulse(
            &Sources::local(),
            &MediaLocation::local(&path),
            SpeakerAngle::STANDARD,
        )
        .expect("a readable response")
        .expect("a response with taps");
        let _ = fs::remove_file(&path);

        assert!(
            (read.loudest_gain() - 1.0).abs() < 1e-9,
            "{}",
            read.loudest_gain()
        );
    }

    const SOFA_FRAMES: usize = 64;

    fn sofa_set(rate: f64) -> Vec<u8> {
        let tap = |measurement: usize, ear: usize, frame: usize| {
            if frame == 0 {
                0.1 * (1 + measurement * 2 + ear) as f64
            } else {
                0.0
            }
        };
        let mut impulses = Vec::new();
        for measurement in 0..3 {
            for ear in 0..2 {
                for frame in 0..SOFA_FRAMES {
                    impulses.push(tap(measurement, ear, frame));
                }
            }
        }
        let positions = [0.0, 0.0, 1.0, 30.0, 0.0, 1.0, 330.0, 0.0, 1.0];
        Hdf5Builder::new()
            .dataset(
                DatasetBuilder::typed_data("Data.IR", vec![3, 2, SOFA_FRAMES as u64], &impulses)
                    .expect("impulses"),
            )
            .dataset(
                DatasetBuilder::typed_data("SourcePosition", vec![3, 3], &positions)
                    .expect("positions")
                    .attribute(AttributeBuilder::fixed_string("Type", "spherical")),
            )
            .dataset(
                DatasetBuilder::typed_data("Data.SamplingRate", vec![1], &[rate]).expect("rate"),
            )
            .into_plan()
            .expect("a plan")
            .encode(WriteOptions::default())
            .expect("bytes")
    }

    #[test]
    fn a_set_of_head_related_responses_is_read_as_the_two_by_two_response_of_the_speakers() {
        let path = env::temp_dir().join(format!("resonate-hrir-{}.sofa", process::id()));
        fs::write(&path, sofa_set(44_100.0)).expect("a scratch set");

        let read = read_impulse(
            &Sources::local(),
            &MediaLocation::local(&path),
            SpeakerAngle::STANDARD,
        )
        .expect("a readable set")
        .expect("a set with taps");
        let _ = fs::remove_file(&path);

        assert!(read.is_crossed());
        assert_eq!(read.channels(), 4);
        assert_eq!(read.frames(), SOFA_FRAMES);
        assert_eq!(read.rate().hz(), 44_100);
        let taps = read.at(read.rate()).expect("taps");
        let firsts: Vec<f64> = taps.iter().map(|channel| channel[0]).collect();
        for (heard, wanted) in firsts.iter().zip([0.3, 0.4, 0.5, 0.6]) {
            assert!((heard - wanted).abs() < 1e-9, "{firsts:?}");
        }
    }

    #[test]
    fn a_speaker_angle_the_listener_sets_moves_the_measurements_a_set_is_read_at() {
        let path = env::temp_dir().join(format!("resonate-hrir-angle-{}.sofa", process::id()));
        fs::write(&path, sofa_set(48_000.0)).expect("a scratch set");
        let narrow = SpeakerAngle::from_degrees(5).expect("an angle");

        let read = read_impulse(&Sources::local(), &MediaLocation::local(&path), narrow)
            .expect("a readable set")
            .expect("a set with taps");
        let _ = fs::remove_file(&path);

        let taps = read.at(read.rate()).expect("taps");
        let firsts: Vec<f64> = taps.iter().map(|channel| channel[0]).collect();
        for (heard, wanted) in firsts.iter().zip([0.1, 0.2, 0.1, 0.2]) {
            assert!((heard - wanted).abs() < 1e-9, "{firsts:?}");
        }
    }

    #[test]
    fn a_set_is_redrawn_at_the_rate_of_the_stream_like_any_other_response() {
        let path = env::temp_dir().join(format!("resonate-hrir-rate-{}.sofa", process::id()));
        fs::write(&path, sofa_set(44_100.0)).expect("a scratch set");

        let read = read_impulse(
            &Sources::local(),
            &MediaLocation::local(&path),
            SpeakerAngle::STANDARD,
        )
        .expect("a readable set")
        .expect("a set with taps");
        let _ = fs::remove_file(&path);

        let at_stream = read.at(SampleRate::HZ_96000).expect("taps");
        assert_eq!(at_stream.len(), 4);
        let expected = SOFA_FRAMES as f64 * 96_000.0 / 44_100.0;
        for channel in at_stream.iter() {
            assert!(
                (channel.len() as f64 - expected).abs() <= 2.0,
                "{}",
                channel.len()
            );
        }
    }

    #[test]
    fn a_set_is_not_one_of_the_two_stereo_files_of_a_pair() {
        let path = env::temp_dir().join(format!("resonate-hrir-pair-{}.sofa", process::id()));
        fs::write(&path, sofa_set(48_000.0)).expect("a scratch set");
        let stereo = scratch("hrir-pair-right", 2, 48_000, &[4_096, 16_384, 0, 0]);

        let refused = read_binaural_pair(
            &Sources::local(),
            &MediaLocation::local(&path),
            &MediaLocation::local(&stereo),
            SpeakerAngle::STANDARD,
        );
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&stereo);

        assert!(matches!(refused, Err(PairedReadError::Pair(_))));
    }
}
