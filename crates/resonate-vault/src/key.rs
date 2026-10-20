use std::fmt;

use resonate_codec::Speakers;
use resonate_core::{SampleFormat, StreamSpec};

use crate::{
    error::{Error, Result},
    form::Form,
    pcm::Digest,
};

pub(crate) const KEY_BYTES: usize = 16;
const KEY_LETTERS: usize = KEY_BYTES * 2;
const FANOUT_LETTERS: usize = 2;
const SOUND_KEYED_AS: &[u8] = b"resonate-vault sound";
const MASK_PAST_WAVE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sound {
    rate: u32,
    channels: u8,
    format: SampleFormat,
    speakers: Speakers,
}

impl Sound {
    pub(crate) fn placed(spec: StreamSpec, speakers: Speakers, form: Form) -> Self {
        let channels = spec.channel_count().get();
        let speakers = if speakers.is_named() {
            speakers
        } else {
            match form {
                Form::Flac => Speakers::in_flac_order(channels),
                Form::Wave | Form::Kept => Speakers::in_wave_order(channels),
            }
            .unwrap_or(Speakers::UNNAMED)
        };
        Self {
            rate: spec.rate.hz(),
            channels,
            format: spec.format,
            speakers,
        }
    }

    pub(crate) fn keyed(self, pcm: VaultKey) -> VaultKey {
        let mut digest = Digest::default();
        digest.note(SOUND_KEYED_AS);
        digest.note(pcm.as_bytes());
        digest.note(&self.rate.to_le_bytes());
        digest.note(&[self.channels, format_code(self.format)]);
        digest.note(
            &self
                .speakers
                .wave_mask()
                .unwrap_or(MASK_PAST_WAVE)
                .to_le_bytes(),
        );
        digest.settled()
    }
}

const fn format_code(format: SampleFormat) -> u8 {
    match format {
        SampleFormat::S16 => 16,
        SampleFormat::S24 => 24,
        SampleFormat::S32 => 32,
        SampleFormat::F32 => 0xf3,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VaultKey([u8; KEY_BYTES]);

impl VaultKey {
    pub const fn of(bytes: [u8; KEY_BYTES]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }

    pub fn read(text: &str) -> Result<Self> {
        if text.len() != KEY_LETTERS || !text.bytes().all(|letter| letter.is_ascii_hexdigit()) {
            return Err(Error::NotAKey);
        }

        let mut bytes = [0_u8; KEY_BYTES];
        for (at, byte) in bytes.iter_mut().enumerate() {
            let pair = text.get(at * 2..at * 2 + 2).ok_or(Error::NotAKey)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| Error::NotAKey)?;
        }
        Ok(Self(bytes))
    }

    pub fn fanout(&self) -> String {
        self.to_string()[..FANOUT_LETTERS].to_owned()
    }
}

impl fmt::Display for VaultKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_samples_at_another_rate_count_depth_or_placing_are_keyed_apart() {
        use resonate_core::{ChannelCount, ChannelLayout, SampleRate};

        let silence = VaultKey::of([0x5a; KEY_BYTES]);
        let spec = |rate: u32, channels: u8, format: SampleFormat| {
            StreamSpec::new(
                SampleRate::new(rate).expect("a rate"),
                ChannelLayout::from_count(ChannelCount::new(u16::from(channels)).expect("a count")),
                format,
            )
        };
        let keyed = |spec: StreamSpec, speakers: Speakers, form: Form| {
            Sound::placed(spec, speakers, form).keyed(silence)
        };
        let stereo = keyed(
            spec(44_100, 2, SampleFormat::S16),
            Speakers::UNNAMED,
            Form::Flac,
        );

        assert_ne!(stereo, silence);
        assert_ne!(
            stereo,
            keyed(
                spec(48_000, 2, SampleFormat::S16),
                Speakers::UNNAMED,
                Form::Flac
            )
        );
        assert_ne!(
            stereo,
            keyed(
                spec(44_100, 1, SampleFormat::S16),
                Speakers::UNNAMED,
                Form::Flac
            )
        );
        assert_ne!(
            keyed(
                spec(44_100, 2, SampleFormat::S32),
                Speakers::UNNAMED,
                Form::Wave
            ),
            keyed(
                spec(44_100, 2, SampleFormat::F32),
                Speakers::UNNAMED,
                Form::Wave
            )
        );
        assert_ne!(
            keyed(
                spec(44_100, 4, SampleFormat::S16),
                Speakers::UNNAMED,
                Form::Flac
            ),
            keyed(
                spec(44_100, 4, SampleFormat::S16),
                Speakers::UNNAMED,
                Form::Wave
            )
        );
        assert_eq!(
            stereo,
            keyed(
                spec(44_100, 2, SampleFormat::S16),
                Speakers::in_flac_order(2).expect("a stereo placing"),
                Form::Flac
            )
        );
    }

    #[test]
    fn a_key_written_out_reads_back_as_itself() {
        let key = VaultKey::of([
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ]);
        let written = key.to_string();

        assert_eq!(written, "00112233445566778899aabbccddeeff");
        assert_eq!(VaultKey::read(&written).unwrap(), key);
        assert_eq!(key.fanout(), "00");
    }

    #[test]
    fn a_signed_or_short_or_uppercase_run_of_letters_is_weighed_as_what_it_is() {
        assert!(VaultKey::read("+0112233445566778899aabbccddeeff").is_err());
        assert!(VaultKey::read("00112233445566778899aabbccddeef").is_err());
        assert!(VaultKey::read("00112233445566778899aabbccddeefff").is_err());
        assert!(VaultKey::read(" 0112233445566778899aabbccddeeff").is_err());
        assert_eq!(
            VaultKey::read("00112233445566778899AABBCCDDEEFF").unwrap(),
            VaultKey::read("00112233445566778899aabbccddeeff").unwrap()
        );
    }
}
