use resonate_core::{Gain, Volume};

const PERCENT_A_WHOLE_SLIDER: f32 = 100.0;
const WHOLE_SLIDER_PERCENT: u8 = 100;

pub fn percent_of(gain: Gain) -> u8 {
    let percent = (Volume::heard_at(gain).get() * PERCENT_A_WHOLE_SLIDER).round();
    percent.clamp(0.0, f32::from(WHOLE_SLIDER_PERCENT)) as u8
}

pub fn gain_of(percent: u8) -> Gain {
    let position = f32::from(percent.min(WHOLE_SLIDER_PERCENT)) / PERCENT_A_WHOLE_SLIDER;
    Volume::new(position).unwrap_or(Volume::MAX).to_gain()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain(linear: f32) -> Gain {
        Gain::new(linear).expect("a finite gain")
    }

    #[test]
    fn the_windows_slider_stands_where_ours_does() {
        assert_eq!(percent_of(Gain::UNITY), 100);
        assert_eq!(percent_of(Gain::SILENT), 0);
        assert_eq!(percent_of(gain(0.125)), 50);
        assert_eq!(percent_of(gain(4.0)), 100);
    }

    #[test]
    fn a_percentage_read_back_is_the_gain_our_slider_sends_there() {
        assert_eq!(gain_of(100), Gain::UNITY);
        assert_eq!(gain_of(0), Gain::SILENT);
        assert_eq!(gain_of(50), gain(0.125));
        assert_eq!(gain_of(250), Gain::UNITY);
        for percent in 0..=100 {
            assert_eq!(percent_of(gain_of(percent)), percent);
        }
    }
}
