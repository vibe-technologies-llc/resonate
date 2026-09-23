use std::time::Duration;

use resonate_core::Chromaprint;
use rusty_chromaprint::{Configuration, FingerprintCompressor, Fingerprinter};

pub const PRINTED_FOR: Duration = Duration::from_secs(120);

const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const PADDING: char = '=';

const SIXTEEN_BIT_SCALE: f32 = 32_768.0;

pub(crate) struct Printing {
    configuration: Configuration,
    printer: Option<Fingerprinter>,
    channels: usize,
    left: u64,
    scratch: Vec<i16>,
}

impl Printing {
    pub(crate) fn new(rate: u32, channels: usize) -> Self {
        let configuration = Configuration::preset_test2();
        let mut printer = Fingerprinter::new(&configuration);
        let started = match printer.start(rate, channels as u32) {
            Ok(()) => Some(printer),
            Err(error) => {
                tracing::debug!(%error, rate, channels, "the stream cannot be fingerprinted");
                None
            }
        };
        Self {
            configuration,
            printer: started,
            channels: channels.max(1),
            left: (PRINTED_FOR.as_secs_f64() * f64::from(rate)) as u64,
            scratch: Vec::new(),
        }
    }

    pub(crate) fn note(&mut self, interleaved: &[f32]) {
        let Some(printer) = &mut self.printer else {
            return;
        };
        if self.left == 0 {
            return;
        }

        let frames = (interleaved.len() / self.channels).min(self.left as usize);
        self.scratch.clear();
        self.scratch.extend(
            interleaved[..frames * self.channels]
                .iter()
                .map(|sample| sixteen_bit(*sample)),
        );
        printer.consume(&self.scratch);
        self.left -= frames as u64;
    }

    pub(crate) fn finished(self, length: Duration) -> Option<Chromaprint> {
        let mut printer = self.printer?;
        printer.finish();
        let points = printer.fingerprint();
        if points.is_empty() {
            return None;
        }

        let compressed = FingerprintCompressor::from(&self.configuration).compress(points);
        Chromaprint::new(&url_safe_base64(&compressed), length).ok()
    }
}

fn sixteen_bit(sample: f32) -> i16 {
    (sample * SIXTEEN_BIT_SCALE)
        .round()
        .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

pub fn print_clip(interleaved: &[f32], rate: u32, channels: usize) -> Option<Chromaprint> {
    let mut printing = Printing::new(rate, channels);
    printing.note(interleaved);
    let frames = interleaved.len() / channels.max(1);
    printing.finished(Duration::from_secs_f64(
        frames as f64 / f64::from(rate.max(1)),
    ))
}

fn url_safe_base64(bytes: &[u8]) -> String {
    base64(bytes, URL_SAFE, false)
}

pub(crate) fn standard_base64(bytes: &[u8]) -> String {
    base64(bytes, STANDARD, true)
}

fn base64(bytes: &[u8], alphabet: &[u8; 64], padded: bool) -> String {
    let mut written = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let joined = group.iter().enumerate().fold(0_u32, |joined, (nth, byte)| {
            joined | u32::from(*byte) << (16 - 8 * nth)
        });
        let letters = group.len() + 1;
        for nth in 0..letters {
            let index = (joined >> (18 - 6 * nth)) & 0x3f;
            written.push(char::from(alphabet[index as usize]));
        }
        if padded {
            for _ in letters..4 {
                written.push(PADDING);
            }
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use std::f32::consts::PI;

    use super::*;

    #[test]
    fn bytes_are_written_in_url_safe_base64_without_padding() {
        assert_eq!(url_safe_base64(b""), "");
        assert_eq!(url_safe_base64(b"f"), "Zg");
        assert_eq!(url_safe_base64(b"fo"), "Zm8");
        assert_eq!(url_safe_base64(b"foo"), "Zm9v");
        assert_eq!(url_safe_base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(url_safe_base64(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn a_sample_is_held_to_sixteen_bits() {
        assert_eq!(sixteen_bit(0.0), 0);
        assert_eq!(sixteen_bit(1.5), i16::MAX);
        assert_eq!(sixteen_bit(-1.0), i16::MIN);
        assert_eq!(sixteen_bit(0.5), 16_384);
    }

    #[test]
    fn a_minute_of_music_prints_and_the_print_says_how_long_the_track_is() {
        let rate = 44_100;
        let mut printing = Printing::new(rate, 2);
        let tones = [220.0, 277.18, 329.63, 440.0];
        let interleaved: Vec<f32> = (0..rate * 60)
            .flat_map(|frame| {
                let second = (frame / rate) as usize;
                let hz = tones[second % tones.len()];
                let sample = 0.4 * (frame as f32 * 2.0 * PI * hz / rate as f32).sin();
                [sample, sample]
            })
            .collect();
        for block in interleaved.chunks(8_192) {
            printing.note(block);
        }
        let length = Duration::from_secs(60);
        let print = printing.finished(length).expect("a print");

        assert_eq!(print.length(), length);
        assert!(print.encoded().starts_with("AQA"), "{}", print.encoded());
        assert!(print.encoded().len() > 100);
    }
}
