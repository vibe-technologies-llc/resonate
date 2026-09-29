use std::{
    fs::{self, File},
    io::{self, Write as _},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use resonate_codec::Codec;
use resonate_core::{Chromaprint, FrameSpan, MediaLocation, SampleFormat, SampleRate};

use crate::{
    Analysis, Envelope, Examined, Levels, Loudness, Spectrogram, Spectrum, Stereo, Study,
    envelope::ENVELOPE_LANES,
    spectrogram::SPECTROGRAM_ROWS,
    verdict::{Weighed, judged},
};

const MAGIC: &[u8; 4] = b"RSAN";

const WRITTEN_AS: u8 = 1;

const KEPT_AS: &str = "analysis";

const STAGED_AS: &str = "staged";

pub const KEPT_BYTES_AT_MOST: u64 = 256 * 1024 * 1024;

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

static STAGED: AtomicU64 = AtomicU64::new(0);

pub struct KeptAnalyses {
    dir: PathBuf,
    at_most: u64,
}

impl KeptAnalyses {
    pub fn at(dir: PathBuf) -> Self {
        Self {
            dir,
            at_most: KEPT_BYTES_AT_MOST,
        }
    }

    pub fn holding_at_most(self, at_most: u64) -> Self {
        Self {
            dir: self.dir,
            at_most,
        }
    }

    pub fn recalled(&self, location: &MediaLocation, span: Option<FrameSpan>) -> Option<Analysis> {
        let path = self.path_of(location, span)?;
        let bytes = fs::read(&path).ok()?;
        let recalled = read(&bytes);
        match &recalled {
            Some(_) => {
                if let Ok(file) = File::options().append(true).open(&path) {
                    let _ = file.set_modified(SystemTime::now());
                }
            }
            None => {
                tracing::debug!(path = %path.display(), "a kept analysis could not be read back");
                let _ = fs::remove_file(&path);
            }
        }
        recalled
    }

    pub fn keep(&self, location: &MediaLocation, span: Option<FrameSpan>, analysis: &Analysis) {
        let Some(path) = self.path_of(location, span) else {
            return;
        };
        if let Err(error) = self.written(&path, &written(analysis)) {
            tracing::debug!(%error, path = %path.display(), "an analysis could not be kept");
            return;
        }
        self.trim();
    }

    fn path_of(&self, location: &MediaLocation, span: Option<FrameSpan>) -> Option<PathBuf> {
        let file = location.as_path()?;
        let held = fs::metadata(file).ok()?;
        let modified = held.modified().ok()?;
        let mut hash = FNV_OFFSET_BASIS;
        let mut take = |bytes: &[u8]| {
            for byte in bytes {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(FNV_PRIME);
            }
        };
        take(file.as_os_str().as_encoded_bytes());
        take(&span.map_or(0, |span| span.start().get()).to_le_bytes());
        take(
            &span
                .and_then(FrameSpan::frames)
                .map_or(u64::MAX, |frames| frames.get())
                .to_le_bytes(),
        );
        take(&held.len().to_le_bytes());
        take(&nanos_since_the_epoch(modified).to_le_bytes());
        Some(self.dir.join(format!("{hash:016x}.{KEPT_AS}")))
    }

    fn written(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let staged = self.dir.join(format!(
            "{}-{}.{STAGED_AS}",
            process::id(),
            STAGED.fetch_add(1, Ordering::Relaxed)
        ));
        let landed = File::create(&staged)
            .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
            .and_then(|()| fs::rename(&staged, path));
        if landed.is_err() {
            let _ = fs::remove_file(&staged);
        }
        landed
    }

    fn trim(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let mut kept: Vec<(SystemTime, u64, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|kind| kind == KEPT_AS))
            .filter_map(|entry| {
                let held = entry.metadata().ok()?;
                Some((held.modified().ok()?, held.len(), entry.path()))
            })
            .collect();
        let mut total: u64 = kept.iter().map(|(_, size, _)| size).sum();
        kept.sort_by_key(|(modified, ..)| *modified);
        for (_, size, path) in kept {
            if total <= self.at_most {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(size);
            }
        }
    }
}

fn nanos_since_the_epoch(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH).map_or(0, |since| {
        u64::try_from(since.as_nanos()).unwrap_or(u64::MAX)
    })
}

struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn maybe<T>(&mut self, value: Option<T>, then: impl FnOnce(&mut Self, T)) {
        match value {
            Some(value) => {
                self.u8(1);
                then(self, value);
            }
            None => self.u8(0),
        }
    }

    fn duration(&mut self, value: Duration) {
        self.u64(u64::try_from(value.as_nanos()).unwrap_or(u64::MAX));
    }

    fn bytes(&mut self, value: &[u8]) {
        self.u32(u32::try_from(value.len()).unwrap_or(u32::MAX));
        self.0.extend_from_slice(value);
    }

    fn levels(&mut self, values: &[f32]) {
        self.u32(u32::try_from(values.len()).unwrap_or(u32::MAX));
        for value in values {
            self.f32(*value);
        }
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn taken(&mut self, count: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.0.split_at_checked(count)?;
        self.0 = rest;
        Some(taken)
    }

    fn u8(&mut self) -> Option<u8> {
        self.taken(1)?.first().copied()
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.taken(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.taken(8)?.try_into().ok()?))
    }

    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.taken(4)?.try_into().ok()?))
    }

    fn maybe<T>(&mut self, then: impl FnOnce(&mut Self) -> Option<T>) -> Option<Option<T>> {
        match self.u8()? {
            0 => Some(None),
            1 => then(self).map(Some),
            _ => None,
        }
    }

    fn duration(&mut self) -> Option<Duration> {
        self.u64().map(Duration::from_nanos)
    }

    fn bytes(&mut self) -> Option<&'a [u8]> {
        let count = usize::try_from(self.u32()?).ok()?;
        self.taken(count)
    }

    fn levels(&mut self) -> Option<Vec<f32>> {
        let count = usize::try_from(self.u32()?).ok()?;
        if count.checked_mul(4)? > self.0.len() {
            return None;
        }
        (0..count).map(|_| self.f32()).collect()
    }
}

fn written(analysis: &Analysis) -> Vec<u8> {
    let mut writer = Writer(Vec::new());
    writer.0.extend_from_slice(MAGIC);
    writer.u8(WRITTEN_AS);

    let study = &analysis.study;
    let examined = &study.examined;
    writer.bytes(examined.codec.as_str().as_bytes());
    writer.u32(examined.rate.hz());
    writer.u8(examined.channels);
    writer.u8(format_code(examined.format));
    writer.maybe(examined.declared_bits, Writer::u8);
    writer.duration(examined.length);

    let levels = &study.levels;
    writer.f32(levels.peak);
    writer.f32(levels.rms);
    writer.f32(levels.dc);
    writer.u64(levels.clipped);
    writer.u64(levels.clipped_runs);
    writer.maybe(levels.bits_in_use, Writer::u8);
    writer.maybe(levels.stereo, |writer, stereo| {
        writer.u8(u8::from(stereo.identical));
        writer.maybe(stereo.correlation, Writer::f32);
    });

    let loudness = &study.loudness;
    writer.maybe(loudness.integrated, Writer::f32);
    writer.maybe(loudness.range, Writer::f32);
    writer.maybe(loudness.momentary_max, Writer::f32);
    writer.maybe(loudness.short_term_max, Writer::f32);
    writer.maybe(loudness.dynamic_range, Writer::u8);
    writer.f32(loudness.true_peak);

    let spectrum = &study.spectrum;
    writer.f32(spectrum.bin_hz());
    writer.levels(spectrum.levels());
    writer.levels(spectrum.typical());
    writer.duration(spectrum.heard());

    writer.maybe(study.print.as_ref(), |writer, print| {
        writer.bytes(print.encoded().as_bytes());
        writer.duration(print.length());
    });

    let envelope = &analysis.envelope;
    writer.u8(u8::try_from(envelope.lanes).unwrap_or(u8::MAX));
    writer.u64(envelope.frames_per_column);
    writer.u64(envelope.frames);
    writer.u32(u32::try_from(envelope.columns.len()).unwrap_or(u32::MAX));
    for column in &envelope.columns {
        for reach in column {
            writer.f32(reach.low);
            writer.f32(reach.high);
            writer.f32(reach.rms);
        }
    }

    let spectrogram = &analysis.spectrogram;
    writer.u32(u32::try_from(spectrogram.columns).unwrap_or(u32::MAX));
    writer.u32(spectrogram.nyquist_hz);
    writer.u64(spectrogram.frames_per_column);
    writer.u64(spectrogram.frames);
    writer.bytes(&spectrogram.levels);

    writer.0
}

fn read(bytes: &[u8]) -> Option<Analysis> {
    let mut reader = Reader(bytes);
    if reader.taken(MAGIC.len())? != MAGIC || reader.u8()? != WRITTEN_AS {
        return None;
    }

    let named = std::str::from_utf8(reader.bytes()?).ok()?;
    let codec = Codec::ALL
        .into_iter()
        .find(|codec| codec.as_str() == named)?;
    let examined = Examined {
        codec,
        rate: SampleRate::new(reader.u32()?).ok()?,
        channels: reader.u8()?,
        format: format_of(reader.u8()?)?,
        declared_bits: reader.maybe(Reader::u8)?,
        length: reader.duration()?,
    };

    let levels = Levels {
        peak: reader.f32()?,
        rms: reader.f32()?,
        dc: reader.f32()?,
        clipped: reader.u64()?,
        clipped_runs: reader.u64()?,
        bits_in_use: reader.maybe(Reader::u8)?,
        stereo: reader.maybe(|reader| {
            Some(Stereo {
                identical: reader.u8()? != 0,
                correlation: reader.maybe(Reader::f32)?,
            })
        })?,
    };

    let loudness = Loudness {
        integrated: reader.maybe(Reader::f32)?,
        range: reader.maybe(Reader::f32)?,
        momentary_max: reader.maybe(Reader::f32)?,
        short_term_max: reader.maybe(Reader::f32)?,
        dynamic_range: reader.maybe(Reader::u8)?,
        true_peak: reader.f32()?,
    };

    let bin_hz = reader.f32()?;
    let spectrum_levels = reader.levels()?;
    let typical = reader.levels()?;
    let spectrum = Spectrum::new(bin_hz, spectrum_levels, reader.duration()?).with_typical(typical);

    let print = reader.maybe(|reader| {
        let encoded = std::str::from_utf8(reader.bytes()?).ok()?.to_owned();
        Chromaprint::new(&encoded, reader.duration()?).ok()
    })?;

    let lanes = usize::from(reader.u8()?);
    let frames_per_column = reader.u64()?;
    let frames = reader.u64()?;
    if !(1..=ENVELOPE_LANES).contains(&lanes) || (frames_per_column == 0 && frames > 0) {
        return None;
    }
    let count = usize::try_from(reader.u32()?).ok()?;
    if count.checked_mul(ENVELOPE_LANES * 12)? > reader.0.len() {
        return None;
    }
    let mut columns = Vec::with_capacity(count);
    for _ in 0..count {
        let mut column = [crate::Reach::default(); ENVELOPE_LANES];
        for reach in &mut column {
            *reach = crate::Reach {
                low: reader.f32()?,
                high: reader.f32()?,
                rms: reader.f32()?,
            };
        }
        columns.push(column);
    }
    let envelope = Envelope {
        lanes,
        frames_per_column,
        frames,
        columns,
    };

    let spectrogram_columns = usize::try_from(reader.u32()?).ok()?;
    let nyquist_hz = reader.u32()?;
    let spectrogram_frames_per_column = reader.u64()?;
    let spectrogram_frames = reader.u64()?;
    let spectrogram_levels = reader.bytes()?.to_vec();
    if spectrogram_levels.len() != spectrogram_columns.checked_mul(SPECTROGRAM_ROWS)? {
        return None;
    }
    let spectrogram = Spectrogram {
        columns: spectrogram_columns,
        levels: spectrogram_levels,
        nyquist_hz,
        frames_per_column: spectrogram_frames_per_column,
        frames: spectrogram_frames,
    };
    if !reader.0.is_empty() {
        return None;
    }

    let judgement = judged(Weighed {
        codec,
        rate: examined.rate.hz(),
        declared_bits: examined.declared_bits,
        float: examined.format.is_float(),
        spectrum: &spectrum,
        levels: &levels,
    });

    Some(Analysis {
        study: Study {
            examined,
            levels,
            loudness,
            spectrum,
            judgement,
            print,
        },
        envelope,
        spectrogram,
    })
}

const fn format_code(format: SampleFormat) -> u8 {
    match format {
        SampleFormat::S16 => 16,
        SampleFormat::S24 => 24,
        SampleFormat::S32 => 32,
        SampleFormat::F32 => 0xf3,
    }
}

fn format_of(code: u8) -> Option<SampleFormat> {
    SampleFormat::ALL
        .into_iter()
        .find(|format| format_code(*format) == code)
}

#[cfg(test)]
mod tests {
    use std::{env, sync::atomic::AtomicU32};

    use super::*;

    static NEXT: AtomicU32 = AtomicU32::new(0);

    struct Folder(PathBuf);

    impl Folder {
        fn new() -> Self {
            let root = env::temp_dir().join(format!(
                "resonate-kept-{}-{}",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).expect("a scratch folder");
            Self(root)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn analysed() -> Analysis {
        let spectrum = Spectrum::new(10.0, vec![-20.0, -40.5, -160.0], Duration::from_secs(9))
            .with_typical(vec![-30.0, -50.0]);
        let levels = Levels {
            peak: 0.9,
            rms: 0.2,
            dc: 0.001,
            clipped: 3,
            clipped_runs: 1,
            bits_in_use: Some(16),
            stereo: Some(Stereo {
                identical: false,
                correlation: Some(0.8),
            }),
        };
        let examined = Examined {
            codec: Codec::Flac,
            rate: SampleRate::HZ_44100,
            channels: 2,
            format: SampleFormat::S16,
            declared_bits: Some(16),
            length: Duration::from_millis(12_345),
        };
        Analysis {
            study: Study {
                judgement: judged(Weighed {
                    codec: examined.codec,
                    rate: examined.rate.hz(),
                    declared_bits: examined.declared_bits,
                    float: false,
                    spectrum: &spectrum,
                    levels: &levels,
                }),
                examined,
                levels,
                loudness: Loudness {
                    integrated: Some(-14.2),
                    range: None,
                    momentary_max: Some(-9.0),
                    short_term_max: None,
                    dynamic_range: Some(8),
                    true_peak: 0.95,
                },
                spectrum,
                print: Chromaprint::new("AQAA", Duration::from_secs(12)).ok(),
            },
            envelope: Envelope {
                lanes: 2,
                frames_per_column: 64,
                frames: 128,
                columns: vec![
                    [crate::Reach {
                        low: -0.5,
                        high: 0.5,
                        rms: 0.3,
                    }; ENVELOPE_LANES],
                ],
            },
            spectrogram: Spectrogram {
                columns: 1,
                levels: vec![7; SPECTROGRAM_ROWS],
                nyquist_hz: 22_050,
                frames_per_column: 128,
                frames: 128,
            },
        }
    }

    #[test]
    fn an_analysis_reads_back_as_it_was_written_and_a_truncated_one_does_not() {
        let analysis = analysed();
        let bytes = written(&analysis);

        assert_eq!(read(&bytes), Some(analysis));
        assert_eq!(read(&bytes[..bytes.len() - 1]), None);
        assert_eq!(read(b"RSAN\x02"), None);
    }

    #[test]
    fn a_kept_envelope_whose_shape_could_not_have_been_drawn_is_not_read_back() {
        let mut too_many_lanes = analysed();
        too_many_lanes.envelope.lanes = ENVELOPE_LANES + 1;
        let mut no_lanes = analysed();
        no_lanes.envelope.lanes = 0;
        let mut columns_of_nothing = analysed();
        columns_of_nothing.envelope.frames_per_column = 0;

        assert_eq!(read(&written(&too_many_lanes)), None);
        assert_eq!(read(&written(&no_lanes)), None);
        assert_eq!(read(&written(&columns_of_nothing)), None);
        assert!(columns_of_nothing.envelope.condensed(0, 16).is_empty());
        assert!(
            too_many_lanes
                .envelope
                .condensed(ENVELOPE_LANES, 16)
                .is_empty()
        );
    }

    #[test]
    fn an_analysis_is_kept_against_the_file_as_it_stands_and_forgotten_once_it_changes() {
        let folder = Folder::new();
        let file = folder.0.join("track.wav");
        fs::write(&file, b"audio").expect("a file");
        let location = MediaLocation::local(&file);
        let kept = KeptAnalyses::at(folder.0.join("kept"));

        assert_eq!(kept.recalled(&location, None), None);
        kept.keep(&location, None, &analysed());
        assert_eq!(kept.recalled(&location, None), Some(analysed()));
        let span = FrameSpan::starting(resonate_core::Frames(10));
        assert_eq!(kept.recalled(&location, Some(span)), None);

        fs::write(&file, b"other audio").expect("a changed file");
        assert_eq!(kept.recalled(&location, None), None);
    }

    #[test]
    fn what_is_kept_is_trimmed_to_its_bound_the_least_lately_used_first() {
        let folder = Folder::new();
        let kept = KeptAnalyses::at(folder.0.join("kept")).holding_at_most(1);
        let file = folder.0.join("track.wav");
        fs::write(&file, b"audio").expect("a file");
        let location = MediaLocation::local(&file);

        kept.keep(&location, None, &analysed());
        assert_eq!(kept.recalled(&location, None), None);
    }
}
