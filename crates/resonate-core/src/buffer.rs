use std::mem;

use bytemuck::cast_slice;

use crate::{SampleFormat, StreamSpec, quantise};

#[derive(Clone, Debug, PartialEq)]
pub enum SampleData {
    S16(Vec<i16>),
    S24(Vec<i32>),
    S32(Vec<i32>),
    F32(Vec<f32>),
}

impl SampleData {
    pub fn empty(format: SampleFormat) -> Self {
        Self::with_capacity(format, 0)
    }

    pub fn with_capacity(format: SampleFormat, samples: usize) -> Self {
        match format {
            SampleFormat::S16 => Self::S16(Vec::with_capacity(samples)),
            SampleFormat::S24 => Self::S24(Vec::with_capacity(samples)),
            SampleFormat::S32 => Self::S32(Vec::with_capacity(samples)),
            SampleFormat::F32 => Self::F32(Vec::with_capacity(samples)),
        }
    }

    pub fn silence(format: SampleFormat, samples: usize) -> Self {
        match format {
            SampleFormat::S16 => Self::S16(vec![0; samples]),
            SampleFormat::S24 => Self::S24(vec![0; samples]),
            SampleFormat::S32 => Self::S32(vec![0; samples]),
            SampleFormat::F32 => Self::F32(vec![0.0; samples]),
        }
    }

    pub const fn format(&self) -> SampleFormat {
        match self {
            Self::S16(_) => SampleFormat::S16,
            Self::S24(_) => SampleFormat::S24,
            Self::S32(_) => SampleFormat::S32,
            Self::F32(_) => SampleFormat::F32,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::S16(v) => v.len(),
            Self::S24(v) | Self::S32(v) => v.len(),
            Self::F32(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::S16(v) => cast_slice(v),
            Self::S24(v) | Self::S32(v) => cast_slice(v),
            Self::F32(v) => cast_slice(v),
        }
    }

    pub fn write_f32(&mut self, samples: &[f32]) {
        match self {
            Self::S16(store) => each(samples, store, quantise::s16),
            Self::S24(store) => each(samples, store, quantise::s24),
            Self::S32(store) => each(samples, store, quantise::s32),
            Self::F32(store) => each(samples, store, |sample| sample),
        }
    }

    pub fn write_f64(&mut self, samples: &[f64]) {
        match self {
            Self::S16(store) => each(samples, store, quantise::wide_s16),
            Self::S24(store) => each(samples, store, quantise::wide_s24),
            Self::S32(store) => each(samples, store, quantise::wide_s32),
            Self::F32(store) => each(samples, store, |sample| sample as f32),
        }
    }

    pub fn widen_into(&self, from: usize, wide: &mut [f64]) -> usize {
        match self {
            Self::S16(samples) => widened(samples, from, wide, |sample| {
                f64::from(sample) / f64::from(SampleFormat::S16.full_scale())
            }),
            Self::S24(samples) => widened(samples, from, wide, |sample| {
                f64::from(sample) / f64::from(SampleFormat::S24.full_scale())
            }),
            Self::S32(samples) => widened(samples, from, wide, |sample| {
                f64::from(sample) / f64::from(SampleFormat::S32.full_scale())
            }),
            Self::F32(samples) => widened(samples, from, wide, f64::from),
        }
    }

    fn resize(&mut self, samples: usize) {
        match self {
            Self::S16(v) => v.resize(samples, 0),
            Self::S24(v) | Self::S32(v) => v.resize(samples, 0),
            Self::F32(v) => v.resize(samples, 0.0),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioBuffer {
    spec: StreamSpec,
    frames: usize,
    data: SampleData,
}

impl AudioBuffer {
    pub fn empty(spec: StreamSpec) -> Self {
        Self {
            spec,
            frames: 0,
            data: SampleData::empty(spec.format),
        }
    }

    pub fn with_capacity(spec: StreamSpec, frames: usize) -> Self {
        Self {
            spec,
            frames: 0,
            data: SampleData::with_capacity(
                spec.format,
                frames * spec.channel_count().get() as usize,
            ),
        }
    }

    pub fn silence(spec: StreamSpec, frames: usize) -> Self {
        Self {
            spec,
            frames,
            data: SampleData::silence(spec.format, frames * spec.channel_count().get() as usize),
        }
    }

    pub const fn spec(&self) -> StreamSpec {
        self.spec
    }

    pub const fn frames(&self) -> usize {
        self.frames
    }

    pub const fn is_empty(&self) -> bool {
        self.frames == 0
    }

    pub const fn data(&self) -> &SampleData {
        &self.data
    }

    pub const fn data_mut(&mut self) -> &mut SampleData {
        &mut self.data
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.data.as_bytes()
    }

    pub fn as_f32(&self) -> Option<&[f32]> {
        match &self.data {
            SampleData::F32(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_f32_mut(&mut self) -> Option<&mut [f32]> {
        match &mut self.data {
            SampleData::F32(v) => Some(v),
            _ => None,
        }
    }

    pub fn set_frames(&mut self, frames: usize) {
        self.frames = frames;
        self.data
            .resize(frames * self.spec.channel_count().get() as usize);
    }

    pub fn set_spec(&mut self, spec: StreamSpec) {
        if self.spec == spec {
            return;
        }
        self.retype(spec.format);
        self.spec = spec;
        self.set_frames(self.frames);
    }

    pub fn retype(&mut self, format: SampleFormat) {
        if self.spec.format == format {
            return;
        }

        let previous = mem::replace(&mut self.data, SampleData::empty(format));
        self.data = match (previous, format) {
            (SampleData::S24(mut samples), SampleFormat::S32) => {
                for sample in &mut samples {
                    *sample <<= 8;
                }
                SampleData::S32(samples)
            }
            (SampleData::S32(mut samples), SampleFormat::S24) => {
                for sample in &mut samples {
                    *sample >>= 8;
                }
                SampleData::S24(samples)
            }
            (previous, format) => {
                let mut next = SampleData::silence(format, previous.len());
                convert(&previous, &mut next);
                next
            }
        };
        self.spec.format = format;
    }

    pub fn convert_into(&self, dst: &mut AudioBuffer) {
        dst.spec = StreamSpec::new(self.spec.rate, self.spec.channels, dst.spec.format);
        dst.set_frames(self.frames);
        convert(&self.data, &mut dst.data);
    }
}

fn widened<S: Copy>(src: &[S], from: usize, wide: &mut [f64], widen: impl Fn(S) -> f64) -> usize {
    let taken = src.get(from..).unwrap_or_default();
    each(taken, wide, widen);
    taken.len().min(wide.len())
}

fn each<S: Copy, T>(src: &[S], dst: &mut [T], convert: impl Fn(S) -> T) {
    for (slot, sample) in dst.iter_mut().zip(src) {
        *slot = convert(*sample);
    }
}

fn convert(src: &SampleData, dst: &mut SampleData) {
    use SampleData as D;

    match (src, dst) {
        (D::F32(src), dst) => dst.write_f32(src),

        (D::S16(src), D::S16(dst)) => each(src, dst, |sample| sample),
        (D::S24(src), D::S24(dst)) | (D::S32(src), D::S32(dst)) => each(src, dst, |sample| sample),

        (D::S16(src), D::S24(dst)) => each(src, dst, |sample| i32::from(sample) << 8),
        (D::S16(src), D::S32(dst)) => each(src, dst, |sample| i32::from(sample) << 16),
        (D::S24(src), D::S32(dst)) => each(src, dst, |sample| sample << 8),

        (D::S24(src), D::S16(dst)) => each(src, dst, |sample| (sample >> 8) as i16),
        (D::S32(src), D::S16(dst)) => each(src, dst, |sample| (sample >> 16) as i16),
        (D::S32(src), D::S24(dst)) => each(src, dst, |sample| sample >> 8),

        (D::S16(src), D::F32(dst)) => each(src, dst, |sample| {
            f32::from(sample) / SampleFormat::S16.full_scale()
        }),
        (D::S24(src), D::F32(dst)) => each(src, dst, |sample| {
            sample as f32 / SampleFormat::S24.full_scale()
        }),
        (D::S32(src), D::F32(dst)) => each(src, dst, |sample| {
            sample as f32 / SampleFormat::S32.full_scale()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChannelLayout, SampleRate};

    const S24_MIN: i32 = -8_388_608;
    const S24_MAX: i32 = 8_388_607;

    fn spec(format: SampleFormat) -> StreamSpec {
        StreamSpec::new(SampleRate::HZ_44100, ChannelLayout::Stereo, format)
    }

    fn mono(format: SampleFormat) -> StreamSpec {
        StreamSpec::new(SampleRate::HZ_44100, ChannelLayout::Mono, format)
    }

    fn from_s16(values: &[i16]) -> AudioBuffer {
        let mut buffer = AudioBuffer::silence(mono(SampleFormat::S16), values.len());
        match buffer.data_mut() {
            SampleData::S16(store) => store.copy_from_slice(values),
            other => panic!("expected S16, got {other:?}"),
        }
        buffer
    }

    fn from_i32(format: SampleFormat, values: &[i32]) -> AudioBuffer {
        let mut buffer = AudioBuffer::silence(mono(format), values.len());
        match buffer.data_mut() {
            SampleData::S24(store) | SampleData::S32(store) => store.copy_from_slice(values),
            other => panic!("expected an integer format, got {other:?}"),
        }
        buffer
    }

    fn from_f32(values: &[f32]) -> AudioBuffer {
        let mut buffer = AudioBuffer::silence(mono(SampleFormat::F32), values.len());
        match buffer.data_mut() {
            SampleData::F32(store) => store.copy_from_slice(values),
            other => panic!("expected F32, got {other:?}"),
        }
        buffer
    }

    fn as_s16(buffer: &AudioBuffer) -> &[i16] {
        match buffer.data() {
            SampleData::S16(store) => store,
            other => panic!("expected S16, got {other:?}"),
        }
    }

    fn as_i32(buffer: &AudioBuffer) -> &[i32] {
        match buffer.data() {
            SampleData::S24(store) | SampleData::S32(store) => store,
            other => panic!("expected an integer format, got {other:?}"),
        }
    }

    fn through(source: &AudioBuffer, via: SampleFormat, back: SampleFormat) -> AudioBuffer {
        let mut intermediate = AudioBuffer::empty(mono(via));
        source.convert_into(&mut intermediate);
        let mut result = AudioBuffer::empty(mono(back));
        intermediate.convert_into(&mut result);
        result
    }

    #[test]
    fn silence_sizes_the_backing_store_by_frames_times_channels() {
        let buffer = AudioBuffer::silence(spec(SampleFormat::S24), 128);
        assert_eq!(buffer.frames(), 128);
        assert_eq!(buffer.data().len(), 256);
    }

    #[test]
    fn byte_length_matches_the_spec_stride() {
        for format in [
            SampleFormat::S16,
            SampleFormat::S24,
            SampleFormat::S32,
            SampleFormat::F32,
        ] {
            let spec = spec(format);
            let buffer = AudioBuffer::silence(spec, 64);
            assert_eq!(
                buffer.as_bytes().len() as u64,
                spec.frames_to_bytes(crate::Frames(64)),
                "{format} stride disagrees with StreamSpec"
            );
        }
    }

    #[test]
    fn set_frames_keeps_the_store_and_the_frame_count_in_step() {
        let mut buffer = AudioBuffer::silence(spec(SampleFormat::S16), 16);
        buffer.set_frames(4);
        assert_eq!(buffer.data().len(), 8);
        buffer.set_frames(32);
        assert_eq!(buffer.data().len(), 64);
    }

    #[test]
    fn every_s16_value_survives_a_round_trip_through_f32() {
        let values: Vec<i16> = (i16::MIN..=i16::MAX).collect();
        let result = through(&from_s16(&values), SampleFormat::F32, SampleFormat::S16);

        assert_eq!(as_s16(&result), values.as_slice());
    }

    #[test]
    fn s24_survives_a_round_trip_through_f32() {
        let values = [S24_MIN, -7_654_321, -1, 0, 1, 1_234_567, S24_MAX];
        let source = from_i32(SampleFormat::S24, &values);
        let result = through(&source, SampleFormat::F32, SampleFormat::S24);

        assert_eq!(as_i32(&result), values.as_slice());
    }

    #[test]
    fn s32_does_not_survive_a_round_trip_through_f32() {
        let source = from_i32(SampleFormat::S32, &[16_777_217]);
        let result = through(&source, SampleFormat::F32, SampleFormat::S32);

        assert_eq!(as_i32(&result), &[16_777_216]);
    }

    #[test]
    fn widening_scales_to_the_wider_full_scale_and_narrows_back_exactly() {
        let values = [i16::MAX, i16::MIN, 0, 1];
        let source = from_s16(&values);

        let mut wide = AudioBuffer::empty(mono(SampleFormat::S32));
        source.convert_into(&mut wide);
        assert_eq!(as_i32(&wide), &[32_767 << 16, -32_768 << 16, 0, 1 << 16]);

        let mut narrow = AudioBuffer::empty(mono(SampleFormat::S16));
        wide.convert_into(&mut narrow);
        assert_eq!(as_s16(&narrow), values.as_slice());
    }

    #[test]
    fn float_samples_beyond_full_scale_clamp_rather_than_wrap() {
        let source = from_f32(&[2.0, -2.0, 1.0, -1.0]);

        let mut narrowed = AudioBuffer::empty(mono(SampleFormat::S24));
        source.convert_into(&mut narrowed);
        assert_eq!(as_i32(&narrowed), &[S24_MAX, S24_MIN, S24_MAX, S24_MIN]);

        let mut narrowed = AudioBuffer::empty(mono(SampleFormat::S16));
        source.convert_into(&mut narrowed);
        assert_eq!(as_s16(&narrowed), &[i16::MAX, i16::MIN, i16::MAX, i16::MIN]);
    }

    #[test]
    fn converting_floats_rounds_each_sample_to_the_nearest_step_rather_than_toward_zero() {
        for format in [SampleFormat::S16, SampleFormat::S24, SampleFormat::S32] {
            let three_quarters_of_a_step = 0.75 / format.full_scale();
            let source = from_f32(&[three_quarters_of_a_step, -three_quarters_of_a_step]);

            let mut converted = AudioBuffer::empty(mono(format));
            source.convert_into(&mut converted);

            let rounded = match converted.data() {
                SampleData::S16(store) => store.iter().copied().map(i32::from).collect(),
                SampleData::S24(store) | SampleData::S32(store) => store.clone(),
                other => panic!("expected an integer format, got {other:?}"),
            };
            assert_eq!(rounded, [1, -1], "{format}");
        }
    }

    #[test]
    fn float_samples_beyond_full_scale_clamp_rather_than_wrap_in_thirty_two_bits_as_well() {
        let source = from_f32(&[2.0, -2.0, 1.0, -1.0, f32::NAN]);

        let mut narrowed = AudioBuffer::empty(mono(SampleFormat::S32));
        source.convert_into(&mut narrowed);
        assert_eq!(
            as_i32(&narrowed),
            &[i32::MAX, i32::MIN, i32::MAX, i32::MIN, 0]
        );
    }

    #[test]
    fn retyping_between_s24_and_s32_keeps_the_allocation() {
        let mut buffer = from_i32(SampleFormat::S24, &[1_000, -1_000]);
        let store = as_i32(&buffer).as_ptr();

        buffer.retype(SampleFormat::S32);
        assert_eq!(buffer.spec().format, SampleFormat::S32);
        assert_eq!(as_i32(&buffer), &[256_000, -256_000]);
        assert_eq!(
            as_i32(&buffer).as_ptr(),
            store,
            "the Vec<i32> was reallocated"
        );

        buffer.retype(SampleFormat::S24);
        assert_eq!(as_i32(&buffer), &[1_000, -1_000]);
        assert_eq!(
            as_i32(&buffer).as_ptr(),
            store,
            "the Vec<i32> was reallocated"
        );
    }

    #[test]
    fn set_spec_retargets_a_reused_buffer_without_moving_its_store() {
        let mut buffer = AudioBuffer::silence(spec(SampleFormat::S24), 64);
        let store = as_i32(&buffer).as_ptr();

        buffer.set_spec(StreamSpec::new(
            SampleRate::HZ_96000,
            ChannelLayout::Stereo,
            SampleFormat::S32,
        ));

        assert_eq!(buffer.spec().rate, SampleRate::HZ_96000);
        assert_eq!(buffer.spec().format, SampleFormat::S32);
        assert_eq!(buffer.frames(), 64);
        assert_eq!(buffer.data().len(), 128);
        assert_eq!(
            as_i32(&buffer).as_ptr(),
            store,
            "the Vec<i32> was reallocated"
        );
    }

    #[test]
    fn set_spec_resizes_the_store_when_the_channel_count_changes() {
        let mut buffer = AudioBuffer::silence(spec(SampleFormat::S16), 32);
        buffer.set_spec(mono(SampleFormat::S16));

        assert_eq!(buffer.frames(), 32);
        assert_eq!(buffer.data().len(), 32);
    }

    #[test]
    fn retyping_to_the_current_format_is_a_no_op() {
        let mut buffer = from_i32(SampleFormat::S32, &[7, 8, 9]);
        buffer.retype(SampleFormat::S32);

        assert_eq!(as_i32(&buffer), &[7, 8, 9]);
    }

    #[test]
    fn convert_into_reuses_the_destination_allocation() {
        let source = from_s16(&[1, 2, 3, 4]);
        let mut destination = AudioBuffer::empty(mono(SampleFormat::S32));

        source.convert_into(&mut destination);
        let store = as_i32(&destination).as_ptr();

        source.convert_into(&mut destination);
        assert_eq!(as_i32(&destination).as_ptr(), store);
        assert_eq!(as_i32(&destination), &[1 << 16, 2 << 16, 3 << 16, 4 << 16]);
    }

    #[test]
    fn convert_into_takes_rate_and_layout_from_the_source_but_format_from_the_destination() {
        let source = AudioBuffer::silence(
            StreamSpec::new(
                SampleRate::HZ_96000,
                ChannelLayout::Stereo,
                SampleFormat::S24,
            ),
            32,
        );
        let mut destination = AudioBuffer::empty(mono(SampleFormat::S32));

        source.convert_into(&mut destination);

        assert_eq!(
            destination.spec(),
            StreamSpec::new(
                SampleRate::HZ_96000,
                ChannelLayout::Stereo,
                SampleFormat::S32
            )
        );
        assert_eq!(destination.frames(), 32);
        assert_eq!(destination.data().len(), 64);
    }

    #[test]
    fn every_integer_word_survives_a_trip_through_f64_exactly() {
        let words = [
            i32::MIN,
            i32::MIN + 1,
            -1,
            0,
            1,
            0x1234_5679,
            i32::MAX - 1,
            i32::MAX,
        ];
        let source = SampleData::S32(words.to_vec());
        let mut wide = vec![0.0; words.len()];
        assert_eq!(source.widen_into(0, &mut wide), words.len());

        let mut back = SampleData::S32(vec![0; words.len()]);
        back.write_f64(&wide);
        assert_eq!(back, source, "a 32-bit word lost its low byte");

        let every_s16: Vec<i16> = (i16::MIN..=i16::MAX).collect();
        let source = SampleData::S16(every_s16.clone());
        let mut wide = vec![0.0; every_s16.len()];
        source.widen_into(0, &mut wide);
        let mut back = SampleData::S16(vec![0; every_s16.len()]);
        back.write_f64(&wide);
        assert_eq!(back, source);
    }

    #[test]
    fn a_wide_sample_rounds_half_to_even_and_saturates_like_a_narrow_one() {
        let step = 1.0 / f64::from(SampleFormat::S16.full_scale());
        let mut words = SampleData::S16(vec![0; 6]);
        words.write_f64(&[0.5 * step, 1.5 * step, -2.5 * step, 2.0, -2.0, f64::NAN]);
        assert_eq!(
            words,
            SampleData::S16(vec![0, 2, -2, i16::MAX, i16::MIN, 0])
        );
    }
}
