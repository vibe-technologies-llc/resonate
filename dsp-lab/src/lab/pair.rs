use std::fmt;

use resonate_core::SampleRate;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Pair {
    pub from: SampleRate,
    pub to: SampleRate,
}

impl Pair {
    pub const fn new(from: SampleRate, to: SampleRate) -> Self {
        Self { from, to }
    }

    pub const fn at(rate: SampleRate) -> Self {
        Self::new(rate, rate)
    }

    pub fn input_hz(self) -> f64 {
        f64::from(self.from.hz())
    }

    pub fn output_hz(self) -> f64 {
        f64::from(self.to.hz())
    }

    pub fn output_nyquist(self) -> f64 {
        self.output_hz() / 2.0
    }

    pub fn input_nyquist(self) -> f64 {
        self.input_hz() / 2.0
    }

    pub fn band_edge(self) -> f64 {
        self.input_nyquist().min(self.output_nyquist())
    }

    pub fn is_down(self) -> bool {
        self.to.hz() < self.from.hz()
    }

    pub fn converts(self) -> bool {
        self.to.hz() != self.from.hz()
    }

    pub fn slug(self) -> String {
        if self.converts() {
            format!("{}-to-{}", self.from.kilohertz(), self.to.kilohertz())
        } else {
            self.from.kilohertz()
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        let rate = |kilohertz: &str| {
            let hz = kilohertz.parse::<f64>().ok()? * 1_000.0;
            SampleRate::new(hz.round() as u32).ok()
        };

        match slug.split_once("-to-") {
            Some((from, to)) => Some(Self::new(rate(from)?, rate(to)?)),
            None => rate(slug).map(Self::at),
        }
    }
}

impl fmt::Display for Pair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.converts() {
            write!(
                f,
                "{} kHz to {} kHz",
                self.from.kilohertz(),
                self.to.kilohertz()
            )
        } else {
            write!(f, "{} kHz", self.from.kilohertz())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pair_reads_back_from_its_slug() {
        for pair in [
            Pair::new(SampleRate::HZ_44100, SampleRate::HZ_48000),
            Pair::new(SampleRate::HZ_192000, SampleRate::HZ_48000),
            Pair::at(SampleRate::HZ_96000),
        ] {
            assert_eq!(Pair::from_slug(&pair.slug()), Some(pair));
        }
    }
}
