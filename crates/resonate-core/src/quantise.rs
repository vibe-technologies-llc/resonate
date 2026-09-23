use crate::SampleFormat;

const EVERY_F32_IS_WHOLE_FROM: f32 = (1_u32 << (f32::MANTISSA_DIGITS - 1)) as f32;
const EVERY_F64_IS_WHOLE_FROM: f64 = (1_u64 << (f64::MANTISSA_DIGITS - 1)) as f64;
const ROUNDS_A_SIGNED_F64_TO_WHOLE: f64 = 1.5 * EVERY_F64_IS_WHOLE_FROM;

pub(crate) fn s16(sample: f32) -> i16 {
    rounded_f32(held_f32(SampleFormat::S16, sample)) as i16
}

pub(crate) fn s24(sample: f32) -> i32 {
    rounded_f32(held_f32(SampleFormat::S24, sample))
}

pub(crate) fn s32(sample: f32) -> i32 {
    wide_s32(f64::from(sample))
}

pub(crate) fn wide_s16(sample: f64) -> i16 {
    rounded_f64(held_f64(SampleFormat::S16, sample)) as i16
}

pub(crate) fn wide_s24(sample: f64) -> i32 {
    rounded_f64(held_f64(SampleFormat::S24, sample)) as i32
}

pub(crate) fn wide_s32(sample: f64) -> i32 {
    rounded_f64(held_f64(SampleFormat::S32, sample)) as i32
}

fn held_f32(format: SampleFormat, sample: f32) -> f32 {
    let full_scale = format.full_scale();
    let scaled = sample * full_scale;
    if scaled.is_nan() {
        0.0
    } else {
        scaled.clamp(-full_scale, full_scale - 1.0)
    }
}

fn held_f64(format: SampleFormat, sample: f64) -> f64 {
    let full_scale = f64::from(format.full_scale());
    let scaled = sample * full_scale;
    if scaled.is_nan() {
        0.0
    } else {
        scaled.clamp(-full_scale, full_scale - 1.0)
    }
}

fn rounded_f32(held: f32) -> i32 {
    let rounded_magnitude = (held.abs() + EVERY_F32_IS_WHOLE_FROM).to_bits();
    let magnitude = (rounded_magnitude - EVERY_F32_IS_WHOLE_FROM.to_bits()) as i32;
    if held.is_sign_negative() {
        -magnitude
    } else {
        magnitude
    }
}

fn rounded_f64(held: f64) -> i64 {
    let rounded = (held + ROUNDS_A_SIGNED_F64_TO_WHOLE).to_bits() as i64;
    rounded - ROUNDS_A_SIGNED_F64_TO_WHOLE.to_bits() as i64
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const INTEGER_FORMATS: [SampleFormat; 3] =
        [SampleFormat::S16, SampleFormat::S24, SampleFormat::S32];
    const S24_LEAST: i32 = -8_388_608;
    const S24_GREATEST: i32 = 8_388_607;

    fn quantised(format: SampleFormat, sample: f32) -> i32 {
        match format {
            SampleFormat::S16 => i32::from(s16(sample)),
            SampleFormat::S24 => s24(sample),
            SampleFormat::S32 => s32(sample),
            SampleFormat::F32 => panic!("a float sample is not quantised"),
        }
    }

    fn range(format: SampleFormat) -> (i32, i32) {
        match format {
            SampleFormat::S16 => (i16::MIN.into(), i16::MAX.into()),
            SampleFormat::S24 => (S24_LEAST, S24_GREATEST),
            SampleFormat::S32 => (i32::MIN, i32::MAX),
            SampleFormat::F32 => panic!("a float sample has no integer range"),
        }
    }

    fn parts_of_a_step(format: SampleFormat, parts: i32, per_step: f32) -> f32 {
        parts as f32 / (per_step * format.full_scale())
    }

    fn rounded_half_to_even_in_f64(format: SampleFormat, sample: f32) -> i32 {
        let (least, greatest) = range(format);
        let scaled = (f64::from(sample) * f64::from(format.full_scale())).round_ties_even();
        if scaled.is_nan() {
            0
        } else {
            scaled.clamp(f64::from(least), f64::from(greatest)) as i32
        }
    }

    #[test]
    fn every_half_step_between_two_s16_values_rounds_to_the_even_one() {
        for below in i16::MIN..i16::MAX {
            let between = parts_of_a_step(SampleFormat::S16, 2 * i32::from(below) + 1, 2.0);
            let even = if below % 2 == 0 { below } else { below + 1 };
            assert_eq!(s16(between), even, "half a step above {below}");
        }
    }

    #[test]
    fn a_half_step_between_two_s24_values_rounds_to_the_even_one() {
        for (halves, even) in [
            (5, 2),
            (7, 4),
            (-5, -2),
            (-7, -4),
            (2 * 8_388_605 + 1, 8_388_606),
            (2 * 8_388_606 + 1, 8_388_606),
            (-2 * 8_388_607 - 1, S24_LEAST),
        ] {
            assert_eq!(
                s24(parts_of_a_step(SampleFormat::S24, halves, 2.0)),
                even,
                "{halves} half steps"
            );
        }
    }

    #[test]
    fn a_half_step_between_two_small_s32_values_rounds_to_the_even_one() {
        for (halves, even) in [(5, 2), (7, 4), (-5, -2), (-7, -4)] {
            assert_eq!(s32(parts_of_a_step(SampleFormat::S32, halves, 2.0)), even);
        }
    }

    #[test]
    fn three_quarters_of_a_step_rounds_away_from_zero_where_truncation_zeroed_it() {
        for format in INTEGER_FORMATS {
            assert_eq!(
                quantised(format, parts_of_a_step(format, 3, 4.0)),
                1,
                "{format}"
            );
            assert_eq!(
                quantised(format, parts_of_a_step(format, -3, 4.0)),
                -1,
                "{format}"
            );
            assert_eq!(
                quantised(format, parts_of_a_step(format, 1, 4.0)),
                0,
                "{format}"
            );
            assert_eq!(
                quantised(format, parts_of_a_step(format, -1, 4.0)),
                0,
                "{format}"
            );
        }
    }

    #[test]
    fn full_scale_lands_on_the_ends_of_each_format_and_what_is_past_it_holds_there() {
        for format in INTEGER_FORMATS {
            let (least, greatest) = range(format);
            for (sample, wanted) in [
                (1.0, greatest),
                (-1.0, least),
                (2.0, greatest),
                (-2.0, least),
                (f32::MAX, greatest),
                (f32::MIN, least),
                (f32::INFINITY, greatest),
                (f32::NEG_INFINITY, least),
            ] {
                assert_eq!(quantised(format, sample), wanted, "{format} at {sample}");
            }
        }
    }

    #[test]
    fn not_a_number_is_silence_in_every_format() {
        for format in INTEGER_FORMATS {
            assert_eq!(quantised(format, f32::NAN), 0, "{format}");
            assert_eq!(quantised(format, -f32::NAN), 0, "{format}");
        }
    }

    proptest! {
        #[test]
        fn a_sample_near_full_scale_rounds_as_rounding_half_to_even_in_f64_would(
            sample in -1.5_f32..1.5,
        ) {
            for format in INTEGER_FORMATS {
                prop_assert_eq!(
                    quantised(format, sample),
                    rounded_half_to_even_in_f64(format, sample)
                );
            }
        }

        #[test]
        fn any_f32_at_all_rounds_as_rounding_half_to_even_in_f64_would(sample in any::<f32>()) {
            for format in INTEGER_FORMATS {
                prop_assert_eq!(
                    quantised(format, sample),
                    rounded_half_to_even_in_f64(format, sample)
                );
            }
        }

        #[test]
        fn every_s24_step_and_half_step_rounds_as_rounding_half_to_even_in_f64_would(
            halves in -(1_i32 << 24)..=(1 << 24),
        ) {
            let sample = parts_of_a_step(SampleFormat::S24, halves, 2.0);
            prop_assert_eq!(
                s24(sample),
                rounded_half_to_even_in_f64(SampleFormat::S24, sample)
            );
        }
    }
}
