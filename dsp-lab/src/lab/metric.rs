#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Better {
    Higher,
    Lower,
    Same,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerance {
    pub absolute: f64,
    pub relative: f64,
}

impl Tolerance {
    pub const EXACT: Self = Self::within(0.0);
    pub const DECIBELS: Self = Self::within(0.5);
    pub const FINE_DECIBELS: Self = Self::within(0.01);
    pub const AMONG_DITHER: Self = Self::within(10.0);
    pub const LEVEL: Self = Self {
        absolute: 0.5,
        relative: 0.02,
    };
    pub const HERTZ: Self = Self::within(1.0);
    pub const SECONDS: Self = Self::within(1e-6);
    pub const SHARE: Self = Self {
        absolute: 0.0,
        relative: 0.5,
    };

    pub const fn within(absolute: f64) -> Self {
        Self {
            absolute,
            relative: 0.0,
        }
    }

    fn allows(self, base: f64) -> f64 {
        self.absolute.max(self.relative * base.abs())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shown {
    Decibels,
    Digits(usize),
    Scientific,
    Kilohertz,
    Milliseconds,
    Microseconds,
    Count,
    YesOrNo,
}

#[derive(Debug, PartialEq)]
pub struct Metric {
    pub key: &'static str,
    pub title: &'static str,
    pub better: Better,
    pub tolerance: Tolerance,
    pub shown: Shown,
}

impl Metric {
    pub const fn new(
        key: &'static str,
        title: &'static str,
        better: Better,
        tolerance: Tolerance,
        shown: Shown,
    ) -> Self {
        Self {
            key,
            title,
            better,
            tolerance,
            shown,
        }
    }

    pub fn is_worse(&self, now: f64, then: f64) -> bool {
        if now == then || (now.is_nan() && then.is_nan()) {
            return false;
        }
        if now.is_nan() || then.is_nan() {
            return true;
        }

        let allowed = self.tolerance.allows(then);
        match self.better {
            Better::Higher => now < then && (then - now > allowed || then.is_infinite()),
            Better::Lower => now > then && (now - then > allowed || then.is_infinite()),
            Better::Same => (now - then).abs() > allowed || now.is_infinite() || then.is_infinite(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    pub metric: &'static Metric,
    pub value: f64,
}

impl Reading {
    pub const fn of(metric: &'static Metric, value: f64) -> Self {
        Self { metric, value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVEL: Metric = Metric::new(
        "level",
        "Level",
        Better::Lower,
        Tolerance::DECIBELS,
        Shown::Decibels,
    );
    const FLAT: Metric = Metric::new(
        "flat",
        "Flat",
        Better::Lower,
        Tolerance::SHARE,
        Shown::Scientific,
    );
    const AT: Metric = Metric::new("at", "At", Better::Same, Tolerance::HERTZ, Shown::Kilohertz);

    #[test]
    fn a_reading_is_worse_only_past_its_tolerance_and_in_its_direction() {
        assert!(!LEVEL.is_worse(-120.2, -120.5));
        assert!(LEVEL.is_worse(-119.0, -120.5));
        assert!(!LEVEL.is_worse(-140.0, -120.5));
        assert!(!FLAT.is_worse(1.4e-9, 1.0e-9));
        assert!(FLAT.is_worse(1.6e-9, 1.0e-9));
        assert!(AT.is_worse(21_000.0, 21_002.0));
        assert!(!AT.is_worse(21_001.5, 21_002.0));
    }

    #[test]
    fn a_bound_once_reached_is_kept_whatever_the_tolerance() {
        assert!(LEVEL.is_worse(-600.0, f64::NEG_INFINITY));
        assert!(!LEVEL.is_worse(f64::NEG_INFINITY, f64::NEG_INFINITY));
        assert!(!LEVEL.is_worse(f64::NEG_INFINITY, -600.0));
        assert!(LEVEL.is_worse(f64::NAN, -600.0));
        assert!(!LEVEL.is_worse(f64::NAN, f64::NAN));
    }
}
