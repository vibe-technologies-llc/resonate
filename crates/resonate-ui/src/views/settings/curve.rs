use gpui::{Bounds, Pixels};
use resonate_core::eq::{Band, BandGain, Frequency, Preamp, RESPONSE_FROM_HZ, RESPONSE_TO_HZ};

use crate::equaliser::Placed;

pub(crate) const CURVE_LINE: f32 = 1.5;
pub(crate) const HANDLE_REACH: f32 = 12.0;
pub(crate) const PIXELS_PER_NOTCH: f32 = 24.0;

const DECIBEL_STEPS: f64 = 10.0;
const MILLIBELS_PER_DECIBEL: f64 = 1_000.0;
const SIGNIFICANT_BELOW_THE_LEADING_DIGIT: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Plotted {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) widest: f32,
    pub(crate) lift: Preamp,
}

impl Default for Plotted {
    fn default() -> Self {
        Self {
            bounds: Bounds::default(),
            widest: 1.0,
            lift: Preamp::NONE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HeldBand {
    pub(crate) row: usize,
    pub(crate) widest: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Plot {
    left: f32,
    top: f32,
    wide: f32,
    down: f32,
    widest: f32,
    lift: f64,
}

impl Plot {
    pub(crate) fn within(bounds: Bounds<Pixels>, widest: f32, lift: Preamp) -> Self {
        Self::spanning(
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
            widest,
        )
        .lifted_by(lift)
    }

    pub(crate) fn of(plotted: Plotted) -> Self {
        Self::within(plotted.bounds, plotted.widest, plotted.lift)
    }

    fn spanning(left: f32, top: f32, width: f32, height: f32, widest: f32) -> Self {
        Self {
            left,
            top: top + CURVE_LINE / 2.0,
            wide: width.max(0.0),
            down: (height - CURVE_LINE).max(0.0),
            widest: widest.max(f32::EPSILON),
            lift: 0.0,
        }
    }

    fn lifted_by(self, lift: Preamp) -> Self {
        Self {
            lift: lift.decibels(),
            ..self
        }
    }

    pub(crate) fn x_of(self, hertz: f64) -> f32 {
        self.left + across_at(hertz) * self.wide
    }

    pub(crate) fn y_of(self, decibels: f64) -> f32 {
        self.top + down_at(decibels, self.widest) * self.down
    }

    pub(crate) fn hertz_at(self, x: f32) -> f64 {
        let fraction = if self.wide > 0.0 {
            ((x - self.left) / self.wide).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let decades = (RESPONSE_TO_HZ / RESPONSE_FROM_HZ).log10();
        RESPONSE_FROM_HZ * 10.0_f64.powf(decades * f64::from(fraction))
    }

    pub(crate) fn decibels_at(self, y: f32) -> f64 {
        let fraction = if self.down > 0.0 {
            ((y - self.top) / self.down).clamp(0.0, 1.0)
        } else {
            0.5
        };
        f64::from((0.5 - fraction) * 2.0 * self.widest)
    }

    pub(crate) fn placed(self, x: f32, y: f32) -> Placed {
        let hertz = stepped_hertz(self.hertz_at(x)).clamp(RESPONSE_FROM_HZ, RESPONSE_TO_HZ);
        let reach = f64::from(BandGain::WIDEST_MILLIBELS) / MILLIBELS_PER_DECIBEL;
        let decibels = ((self.decibels_at(y) - self.lift).clamp(-reach, reach) * DECIBEL_STEPS)
            .round()
            / DECIBEL_STEPS;

        Placed {
            frequency: Frequency::from_hertz(hertz).unwrap_or(Frequency::LOWEST),
            gain: BandGain::from_decibels(decibels).unwrap_or(BandGain::FLAT),
        }
    }

    pub(crate) fn handle(self, band: Band) -> (f32, f32) {
        let level = if band.kind.uses_gain() {
            band.gain.decibels()
        } else {
            0.0
        };
        (
            self.x_of(band.frequency.hertz()),
            self.y_of(level + self.lift),
        )
    }

    pub(crate) fn nearest(self, bands: &[Band], x: f32, y: f32) -> Option<usize> {
        let reach = HANDLE_REACH * HANDLE_REACH;
        bands
            .iter()
            .enumerate()
            .map(|(row, band)| {
                let (across, down) = self.handle(*band);
                (row, (across - x).powi(2) + (down - y).powi(2))
            })
            .filter(|(_, distance)| *distance <= reach)
            .fold(
                None,
                |closest: Option<(usize, f32)>, (row, distance)| match closest {
                    Some((_, held)) if held < distance => closest,
                    _ => Some((row, distance)),
                },
            )
            .map(|(row, _)| row)
    }
}

pub(crate) fn across_at(hertz: f64) -> f32 {
    let decades = (RESPONSE_TO_HZ / RESPONSE_FROM_HZ).log10();

    #[expect(
        clippy::cast_possible_truncation,
        reason = "a fraction of the width fits an f32"
    )]
    let fraction = ((hertz / RESPONSE_FROM_HZ).log10() / decades) as f32;

    fraction.clamp(0.0, 1.0)
}

pub(crate) fn down_at(decibels: f64, widest: f32) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a decibel reading fits an f32"
    )]
    let held = (decibels as f32).clamp(-widest, widest);
    0.5 - held / (widest * 2.0)
}

fn stepped_hertz(hertz: f64) -> f64 {
    let step = 10.0_f64
        .powf(hertz.log10().floor() - SIGNIFICANT_BELOW_THE_LEADING_DIGIT)
        .max(1.0);
    (hertz / step).round() * step
}

#[cfg(test)]
mod tests {
    use resonate_core::eq::{BandKind, MAX_BANDS, Profile, Q};

    use super::*;
    use crate::equaliser::{a_band_at, chosen_after_dropping, moved, narrowed};

    const WIDE: f32 = 600.0;
    const TALL: f32 = 168.0;
    const WIDEST: f32 = 15.0;

    fn plot() -> Plot {
        Plot::spanning(40.0, 100.0, WIDE, TALL, WIDEST)
    }

    fn hertz(hertz: f64) -> Frequency {
        Frequency::from_hertz(hertz).expect("a frequency in range")
    }

    fn decibels(decibels: f64) -> BandGain {
        BandGain::from_decibels(decibels).expect("a gain in range")
    }

    #[test]
    fn a_point_on_the_curve_reads_back_as_the_frequency_and_level_it_was_drawn_at() {
        let plot = plot();
        for at in [
            20.0, 31.5, 100.0, 440.0, 1_000.0, 3_150.0, 10_000.0, 20_000.0,
        ] {
            let read = plot.hertz_at(plot.x_of(at));
            assert!(
                (read / at - 1.0).abs() < 1e-4,
                "{at} Hz read back as {read}"
            );
        }
        for level in [-15.0, -6.0, -0.5, 0.0, 3.2, 15.0] {
            let read = plot.decibels_at(plot.y_of(level));
            assert!(
                (read - level).abs() < 1e-4,
                "{level} dB read back as {read}"
            );
        }

        assert!((plot.x_of(RESPONSE_FROM_HZ) - 40.0).abs() < 1e-4);
        assert!((plot.x_of(RESPONSE_TO_HZ) - (40.0 + WIDE)).abs() < 1e-3);
        assert!(
            plot.y_of(0.0) > plot.y_of(6.0),
            "a boost is drawn below the zero line"
        );
    }

    #[test]
    fn the_width_is_read_backwards_the_way_the_sweep_lays_it_out() {
        let plot = plot();
        let columns: Vec<f64> = resonate_core::eq::sweep(7).collect();
        for (at, hertz) in columns.iter().enumerate() {
            let x = 40.0 + WIDE * at as f32 / 6.0;
            let read = plot.hertz_at(x);
            assert!(
                (read / hertz - 1.0).abs() < 1e-4,
                "{hertz} Hz read as {read}"
            );
        }
    }

    #[test]
    fn a_press_is_placed_on_a_step_a_pointer_can_mean() {
        let plot = plot();

        let low = plot.placed(plot.x_of(57.3), plot.y_of(0.0));
        assert_eq!(low.frequency, hertz(57.0));
        assert_eq!(low.gain, BandGain::FLAT);

        let middle = plot.placed(plot.x_of(1_234.0), plot.y_of(4.44));
        assert_eq!(middle.frequency, hertz(1_230.0));
        assert_eq!(middle.gain, decibels(4.4));

        let high = plot.placed(plot.x_of(12_345.0), plot.y_of(-7.06));
        assert_eq!(high.frequency, hertz(12_300.0));
        assert_eq!(high.gain, decibels(-7.1));
    }

    #[test]
    fn a_pointer_carried_past_the_box_is_held_at_its_edge() {
        let plot = plot();

        let beyond = plot.placed(-5_000.0, -5_000.0);
        assert_eq!(beyond.frequency, hertz(RESPONSE_FROM_HZ));
        assert_eq!(beyond.gain, decibels(f64::from(WIDEST)));

        let under = plot.placed(5_000.0, 5_000.0);
        assert_eq!(under.frequency, hertz(RESPONSE_TO_HZ));
        assert_eq!(under.gain, decibels(-f64::from(WIDEST)));
    }

    #[test]
    fn a_handle_rides_on_the_curve_the_preamp_lowers_and_a_press_takes_the_preamp_back_off() {
        let lift = Preamp::from_decibels(-4.5).expect("in range");
        let plot = plot().lifted_by(lift);
        let band = a_band_at(Placed {
            frequency: hertz(1_000.0),
            gain: decibels(3.0),
        });

        let (x, y) = plot.handle(band);
        assert!(
            (y - plot.y_of(-1.5)).abs() < 1e-3,
            "a handle drawn off the curve"
        );

        let placed = plot.placed(x, y);
        assert_eq!(placed.gain, decibels(3.0));
        assert_eq!(placed.frequency, hertz(1_000.0));

        let dropped = plot.placed(x, plot.y_of(-f64::from(WIDEST)));
        assert_eq!(
            dropped.gain,
            decibels(-f64::from(WIDEST) + 4.5),
            "the bottom of the box read as the gain it is drawn at rather than the band's"
        );
    }

    #[test]
    fn a_range_drawn_wider_than_a_band_holds_is_held_to_what_a_band_holds() {
        let plot = Plot::spanning(0.0, 0.0, WIDE, TALL, 60.0);
        let top = plot.placed(WIDE / 2.0, 0.0);
        assert_eq!(top.gain.millibels(), BandGain::WIDEST_MILLIBELS);
        let bottom = plot.placed(WIDE / 2.0, TALL);
        assert_eq!(bottom.gain.millibels(), -BandGain::WIDEST_MILLIBELS);
    }

    #[test]
    fn a_box_that_has_not_been_laid_out_places_nothing_outside_the_vocabulary() {
        let plot = Plot::within(Bounds::default(), 0.0, Preamp::NONE);
        let placed = plot.placed(100.0, 100.0);
        assert_eq!(placed.frequency, hertz(RESPONSE_FROM_HZ));
        assert_eq!(placed.gain, BandGain::FLAT);
    }

    #[test]
    fn a_press_finds_the_handle_it_is_on_and_nothing_it_is_not_near() {
        let plot = plot();
        let bands = [
            a_band_at(plot.placed(plot.x_of(100.0), plot.y_of(6.0))),
            a_band_at(plot.placed(plot.x_of(1_000.0), plot.y_of(-3.0))),
            Band::new(
                BandKind::Notch,
                hertz(5_000.0),
                decibels(9.0),
                Q::BUTTERWORTH,
            ),
        ];

        let (x, y) = plot.handle(bands[1]);
        assert_eq!(plot.nearest(&bands, x + 3.0, y - 4.0), Some(1));
        assert_eq!(plot.nearest(&bands, x + HANDLE_REACH + 1.0, y), None);

        let (x, y) = plot.handle(bands[2]);
        assert!(
            (y - plot.y_of(0.0)).abs() < 1e-4,
            "a notch drawn off the zero line"
        );
        assert_eq!(plot.nearest(&bands, x, y), Some(2));

        assert_eq!(plot.nearest(&[], x, y), None);
    }

    #[test]
    fn of_two_handles_in_reach_the_nearer_is_taken_and_a_tie_goes_to_the_one_drawn_on_top() {
        let plot = plot();
        let under = a_band_at(plot.placed(plot.x_of(1_000.0), plot.y_of(0.0)));
        let over = under;
        let (x, y) = plot.handle(under);
        assert_eq!(plot.nearest(&[under, over], x, y), Some(1));

        let beside = a_band_at(plot.placed(plot.x_of(1_100.0), plot.y_of(0.0)));
        let (beside_x, _) = plot.handle(beside);
        assert_eq!(plot.nearest(&[beside, under], beside_x - 1.0, y), Some(0));
    }

    #[test]
    fn a_band_pressed_onto_the_curve_is_a_peaking_band_where_it_was_pressed() {
        let plot = plot();
        let placed = plot.placed(plot.x_of(250.0), plot.y_of(-4.0));
        let band = a_band_at(placed);

        assert_eq!(band.kind, BandKind::Peaking);
        assert_eq!(band.frequency, hertz(250.0));
        assert_eq!(band.gain, decibels(-4.0));
        assert!(band.on);
        let (x, y) = plot.handle(band);
        assert!((x - plot.x_of(250.0)).abs() < 1e-3);
        assert!((y - plot.y_of(-4.0)).abs() < 1e-3);
    }

    #[test]
    fn a_curve_takes_bands_until_it_holds_as_many_as_the_stage_runs() {
        let plot = plot();
        let mut profile = Profile::new(Preamp::NONE, Vec::new()).expect("empty");
        for at in 0..MAX_BANDS {
            let x = 40.0 + WIDE * at as f32 / MAX_BANDS as f32;
            profile
                .push(a_band_at(plot.placed(x, plot.y_of(1.0))))
                .expect("room for another band");
        }
        assert!(
            profile.push(a_band_at(plot.placed(300.0, 150.0))).is_err(),
            "a curve took a band past MAX_BANDS"
        );
        assert_eq!(profile.bands().len(), MAX_BANDS);
    }

    #[test]
    fn a_dragged_band_keeps_its_kind_its_q_and_its_switch() {
        let plot = plot();
        let mut shelf = Band::new(
            BandKind::LowShelf,
            hertz(105.0),
            decibels(6.4),
            Q::from_units(0.7).expect("in range"),
        );
        shelf.on = false;

        let landed = moved(shelf, plot.placed(plot.x_of(80.0), plot.y_of(3.0)));
        assert_eq!(landed.kind, BandKind::LowShelf);
        assert_eq!(landed.q, shelf.q);
        assert!(!landed.on);
        assert_eq!(landed.frequency, hertz(80.0));
        assert_eq!(landed.gain, decibels(3.0));

        let notch = Band::new(
            BandKind::Notch,
            hertz(1_000.0),
            BandGain::FLAT,
            Q::BUTTERWORTH,
        );
        let landed = moved(notch, plot.placed(plot.x_of(2_000.0), plot.y_of(9.0)));
        assert_eq!(landed.frequency, hertz(2_000.0));
        assert_eq!(
            landed.gain,
            BandGain::FLAT,
            "a notch was dragged into holding a gain"
        );
    }

    #[test]
    fn the_wheel_narrows_and_widens_a_band_within_what_a_q_holds() {
        let start = Q::from_units(1.0).expect("in range");

        let narrower = narrowed(start, 1.0);
        let wider = narrowed(start, -1.0);
        assert!(narrower > start && wider < start);
        assert_eq!(narrower.milli() % 10, 0, "a turned Q is not on a hundredth");
        assert_eq!(narrowed(narrowed(start, 6.0), -6.0), start);

        assert_eq!(narrowed(start, 400.0).milli(), Q::NARROWEST_MILLI);
        assert_eq!(narrowed(start, -400.0).milli(), Q::WIDEST_MILLI);
        assert_eq!(narrowed(start, f64::NAN), start);
        assert_eq!(narrowed(start, 0.0), start);
    }

    #[test]
    fn a_small_turn_of_the_wheel_still_moves_the_q_a_step() {
        let widest = Q::from_units(0.1).expect("in range");
        assert_eq!(narrowed(widest, 0.01).milli(), 110);
        assert_eq!(narrowed(widest, -0.01), widest);

        let typed = Q::from_milli(707).expect("in range");
        assert_eq!(narrowed(typed, 0.01).milli(), 710);
        assert_eq!(narrowed(typed, -0.01).milli(), 700);
    }

    #[test]
    fn taking_a_band_out_keeps_the_choice_on_the_band_it_was_on() {
        assert_eq!(chosen_after_dropping(Some(3), 3), None);
        assert_eq!(chosen_after_dropping(Some(3), 1), Some(2));
        assert_eq!(chosen_after_dropping(Some(1), 3), Some(1));
        assert_eq!(chosen_after_dropping(None, 0), None);
    }
}
