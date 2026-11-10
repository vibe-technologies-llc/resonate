use resonate_core::{Gain, Volume};

pub fn scalar_of(gain: Gain) -> f32 {
    Volume::heard_at(gain).get()
}

pub fn gain_of(scalar: f32) -> Gain {
    let position = if scalar.is_nan() {
        0.0
    } else {
        scalar.clamp(0.0, 1.0)
    };
    Volume::new(position).unwrap_or(Volume::MAX).to_gain()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain(linear: f32) -> Gain {
        Gain::new(linear).expect("a finite gain")
    }

    fn decibels_apart(one: Gain, other: Gain) -> f32 {
        (one.to_decibels().get() - other.to_decibels().get()).abs()
    }

    #[test]
    fn the_windows_slider_stands_where_ours_does() {
        assert_eq!(scalar_of(Gain::UNITY), 1.0);
        assert_eq!(scalar_of(Gain::SILENT), 0.0);
        assert!((scalar_of(gain(0.125)) - 0.5).abs() < 1e-6);
        assert_eq!(scalar_of(gain(4.0)), 1.0);
    }

    #[test]
    fn a_level_read_back_is_the_gain_our_slider_sends_there() {
        assert_eq!(gain_of(1.0), Gain::UNITY);
        assert_eq!(gain_of(0.0), Gain::SILENT);
        assert!(decibels_apart(gain_of(0.5), gain(0.125)) < 1e-3);
        assert_eq!(gain_of(2.5), Gain::UNITY);
        assert_eq!(gain_of(f32::NAN), Gain::SILENT);
    }

    #[test]
    fn a_level_sent_reads_back_far_within_half_a_devices_step_however_low() {
        for thousandth in 1..=1_000_u16 {
            let sent = gain_of(f32::from(thousandth) / 1_000.0);
            let heard = gain_of(scalar_of(sent));
            assert!(
                decibels_apart(sent, heard) < 0.01,
                "{thousandth}: {sent:?} came back as {heard:?}"
            );
        }
    }
}
