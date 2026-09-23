use std::time::Duration;

use resonate_codec::{Codec, DecodeStatus, Decoder, MediaInfo, Sources};
use resonate_core::{
    AudioBuffer, Chromaprint, FrameSpan, Frames, MediaLocation, SampleData, SampleFormat,
    SampleRate, StreamSpec,
};

use crate::{
    AnalysisOp, Envelope, Error, Judgement, Levels, Loudness, Result, Spectrogram, Spectrum,
    Watching,
    envelope::Enveloping,
    levels::Leveling,
    loudness::Metering,
    print::Printing,
    spectrogram::Spectrographing,
    spectrum::Transforming,
    verdict::{Weighed, judged},
};

const SIXTEEN_BIT_CEILING: u8 = 16;

const TWENTY_FOUR_BIT_CEILING: u8 = 24;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Examined {
    pub codec: Codec,
    pub rate: SampleRate,
    pub channels: u8,
    pub format: SampleFormat,
    pub declared_bits: Option<u8>,
    pub length: Duration,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Study {
    pub examined: Examined,
    pub levels: Levels,
    pub loudness: Loudness,
    pub spectrum: Spectrum,
    pub judgement: Judgement,
    pub print: Option<Chromaprint>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    pub study: Study,
    pub envelope: Envelope,
    pub spectrogram: Spectrogram,
}

pub fn study(
    sources: &Sources,
    location: &MediaLocation,
    span: Option<FrameSpan>,
    watch: &dyn Watching,
) -> Result<Study> {
    walked(sources, location, span, watch, |_, _| Unseen).map(|(study, Unseen)| study)
}

pub fn analyse(
    sources: &Sources,
    location: &MediaLocation,
    span: Option<FrameSpan>,
    watch: &dyn Watching,
) -> Result<Analysis> {
    let (study, drawn) = walked(sources, location, span, watch, |examined, points| Drawn {
        envelope: Enveloping::new(usize::from(examined.channels)),
        spectrogram: Spectrographing::new(points, examined.rate.hz()),
    })?;

    Ok(Analysis {
        study,
        envelope: drawn.envelope.finished(),
        spectrogram: drawn.spectrogram.finished(),
    })
}

trait Drawing {
    fn note_block(&mut self, interleaved: &[f32]);

    fn note_transform(&mut self, powers: &[f32]);
}

struct Unseen;

impl Drawing for Unseen {
    fn note_block(&mut self, _interleaved: &[f32]) {}

    fn note_transform(&mut self, _powers: &[f32]) {}
}

struct Drawn {
    envelope: Enveloping,
    spectrogram: Spectrographing,
}

impl Drawing for Drawn {
    fn note_block(&mut self, interleaved: &[f32]) {
        self.envelope.note(interleaved);
    }

    fn note_transform(&mut self, powers: &[f32]) {
        self.spectrogram.note(powers);
    }
}

fn opened(
    sources: &Sources,
    location: &MediaLocation,
    span: Option<FrameSpan>,
) -> Result<(Decoder, MediaInfo)> {
    match span {
        Some(span) => Decoder::open_span(sources, location, span),
        None => Decoder::open(sources, location),
    }
    .map_err(|source| Error::codec(AnalysisOp::Open, source))
}

fn native_format(info: &MediaInfo, codec: Codec) -> SampleFormat {
    if codec == Codec::Dsd || info.spec.format.is_float() {
        return SampleFormat::F32;
    }
    let bits = info
        .bits_per_coded_sample
        .unwrap_or_else(|| info.spec.format.valid_bits());
    if bits <= SIXTEEN_BIT_CEILING {
        SampleFormat::S16
    } else if bits <= TWENTY_FOUR_BIT_CEILING {
        SampleFormat::S24
    } else {
        SampleFormat::S32
    }
}

fn walked<D: Drawing>(
    sources: &Sources,
    location: &MediaLocation,
    span: Option<FrameSpan>,
    watch: &dyn Watching,
    drawing: impl FnOnce(&Examined, usize) -> D,
) -> Result<(Study, D)> {
    let (mut decoder, info) = opened(sources, location, span)?;
    let codec = Codec::from_id(info.codec);
    let format = native_format(&info, codec);
    decoder.set_output_format(format);

    let rate = info.spec.rate;
    let channels = usize::from(info.spec.channels.count().get());
    let mut examined = Examined {
        codec,
        rate,
        channels: info.spec.channels.count().get(),
        format,
        declared_bits: info.bits_per_coded_sample,
        length: Duration::ZERO,
    };

    let mut transforming = Transforming::new(rate.hz());
    let mut drawn = drawing(&examined, transforming.points());
    let mut leveling = Leveling::new(channels, format);
    let mut metering = Metering::new(
        &info.speakers.placements(info.spec.channels.count().get()),
        rate.hz(),
    );
    let mut printing = Printing::new(rate.hz(), channels);

    let mut block = AudioBuffer::empty(StreamSpec::new(rate, info.spec.channels, format));
    let mut normalised = Vec::new();
    let mut mono = Vec::new();
    let mut frames = 0_u64;
    loop {
        if watch.stopped() {
            return Err(Error::Stopped);
        }
        let status = decoder
            .next_block(&mut block)
            .map_err(|source| Error::codec(AnalysisOp::Decode, source))?;
        if status == DecodeStatus::EndOfStream {
            break;
        }

        normalise(block.data(), &mut normalised);
        mix_down(&normalised, channels, &mut mono);
        leveling.note(block.data(), &normalised);
        metering.note(&normalised);
        printing.note(&normalised);
        drawn.note_block(&normalised);
        transforming.note(&mono, |powers| drawn.note_transform(powers));

        frames += block.frames() as u64;
        watch.reached(Frames(frames), info.duration);
    }

    examined.length = Frames(frames).to_duration(rate);
    let spectrum = transforming.finished();
    let levels = leveling.finished();
    let judgement = judged(Weighed {
        codec,
        rate: rate.hz(),
        declared_bits: info.bits_per_coded_sample,
        float: format.is_float(),
        spectrum: &spectrum,
        levels: &levels,
    });

    Ok((
        Study {
            examined,
            levels,
            loudness: metering.finished(),
            spectrum,
            judgement,
            print: printing.finished(examined.length),
        },
        drawn,
    ))
}

fn normalise(native: &SampleData, into: &mut Vec<f32>) {
    into.clear();
    let scale = native.format().full_scale();
    match native {
        SampleData::S16(samples) => {
            into.extend(samples.iter().map(|sample| f32::from(*sample) / scale));
        }
        SampleData::S24(samples) | SampleData::S32(samples) => {
            into.extend(samples.iter().map(|sample| *sample as f32 / scale));
        }
        SampleData::F32(samples) => into.extend_from_slice(samples),
    }
}

fn mix_down(interleaved: &[f32], channels: usize, into: &mut Vec<f32>) {
    into.clear();
    let share = 1.0 / channels.max(1) as f32;
    into.extend(
        interleaved
            .chunks_exact(channels.max(1))
            .map(|frame| frame.iter().sum::<f32>() * share),
    );
}
