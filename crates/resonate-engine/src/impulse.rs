use std::time::Duration;

use resonate_codec::{DecodeStatus, Decoder, Sources};
use resonate_core::{AudioBuffer, Frames, MediaLocation, SampleData, SampleFormat, StreamSpec};
use resonate_dsp::Impulse;

pub const LONGEST_IMPULSE: Duration = Duration::from_secs(10);

pub fn read_impulse(
    sources: &Sources,
    location: &MediaLocation,
) -> resonate_codec::Result<Option<Impulse>> {
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

    Ok(Impulse::new(rate, taps).map(Impulse::with_headroom))
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

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

        let read = read_impulse(&Sources::local(), &MediaLocation::local(&path))
            .expect("a readable response")
            .expect("a response with taps");
        let _ = fs::remove_file(&path);

        assert_eq!(read.channels(), 1);
        assert_eq!(read.frames(), 3);
        assert_eq!(read.rate().hz(), 48_000);
    }

    #[test]
    fn a_response_boosting_past_unity_is_read_with_the_headroom_it_needs() {
        let path = env::temp_dir().join(format!("resonate-boosting-{}.wav", process::id()));
        fs::write(&path, wave(48_000, &[16_384, 16_384, 16_384])).expect("a scratch response");

        let read = read_impulse(&Sources::local(), &MediaLocation::local(&path))
            .expect("a readable response")
            .expect("a response with taps");
        let _ = fs::remove_file(&path);

        assert!(
            (read.loudest_gain() - 1.0).abs() < 1e-9,
            "{}",
            read.loudest_gain()
        );
    }
}
