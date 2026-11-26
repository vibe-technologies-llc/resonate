use std::fmt::Write as _;

use crate::lab::{
    metric::{Metric, Reading},
    pair::Pair,
    subject::{Family, SubjectName},
};

pub struct Curve {
    pub name: &'static str,
    pub points: Vec<(f64, f64)>,
}

pub struct Measured {
    pub suite: &'static str,
    pub subject: SubjectName,
    pub family: Family,
    pub pair: Pair,
    pub readings: Vec<Reading>,
    pub curves: Vec<Curve>,
}

impl Measured {
    pub fn value(&self, metric: &Metric) -> f64 {
        self.readings
            .iter()
            .find(|reading| reading.metric.key == metric.key)
            .map_or(f64::NAN, |reading| reading.value)
    }

    pub fn curve(&self, name: &str) -> Option<&Curve> {
        self.curves.iter().find(|curve| curve.name == name)
    }

    pub fn is_ours(&self) -> bool {
        self.family == Family::Ours
    }
}

pub fn find<'a>(
    measured: &'a [Measured],
    subject: &SubjectName,
    pair: Pair,
) -> Option<&'a Measured> {
    measured
        .iter()
        .find(|measured| &measured.subject == subject && measured.pair == pair)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Kept {
    pub suite: String,
    pub pair: Pair,
    pub subject: SubjectName,
    pub key: String,
    pub value: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnreadableLine {
    pub line: usize,
}

pub fn written(measured: &[Measured]) -> String {
    let mut text = String::new();

    for case in measured {
        for reading in &case.readings {
            let _ = writeln!(
                text,
                "{}\t{}\t{}\t{}\t{}",
                case.suite,
                case.pair.slug(),
                case.subject,
                reading.metric.key,
                reading.value
            );
        }
    }
    text
}

pub fn read(text: &str) -> Result<Vec<Kept>, UnreadableLine> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            let unreadable = UnreadableLine { line: index + 1 };
            let mut fields = line.split('\t');

            let mut next = || fields.next().ok_or(unreadable);

            let suite = next()?.to_owned();
            let pair = Pair::from_slug(next()?).ok_or(unreadable)?;
            let subject = SubjectName(next()?.to_owned());
            let key = next()?.to_owned();
            let value = next()?.parse().map_err(|_| unreadable)?;

            Ok(Kept {
                suite,
                pair,
                subject,
                key,
                value,
            })
        })
        .collect()
}

pub fn in_scope(kept: Vec<Kept>, suites: &[&str], only: Option<&str>) -> Vec<Kept> {
    kept.into_iter()
        .filter(|then| suites.contains(&then.suite.as_str()))
        .filter(|then| only.is_none_or(|words| then.subject.0.contains(words)))
        .collect()
}

pub struct Regression<'a> {
    pub case: &'a Measured,
    pub metric: &'static Metric,
    pub then: f64,
    pub now: f64,
}

pub struct Weighed<'a> {
    pub compared: usize,
    pub regressions: Vec<Regression<'a>>,
    pub gone: Vec<&'a Kept>,
}

pub fn weighed<'a>(measured: &'a [Measured], kept: &'a [Kept]) -> Weighed<'a> {
    let mut weighed = Weighed {
        compared: 0,
        regressions: Vec::new(),
        gone: Vec::new(),
    };

    for then in kept {
        let Some(case) = measured.iter().find(|case| {
            case.suite == then.suite && case.pair == then.pair && case.subject == then.subject
        }) else {
            weighed.gone.push(then);
            continue;
        };
        let Some(reading) = case
            .readings
            .iter()
            .find(|reading| reading.metric.key == then.key)
        else {
            weighed.gone.push(then);
            continue;
        };

        weighed.compared += 1;
        if reading.metric.is_worse(reading.value, then.value) {
            weighed.regressions.push(Regression {
                case,
                metric: reading.metric,
                then: then.value,
                now: reading.value,
            });
        }
    }
    weighed
}

#[cfg(test)]
mod tests {
    use resonate_core::SampleRate;

    use super::*;
    use crate::lab::metric::{Better, Shown, Tolerance};

    const LEVEL: Metric = Metric::new(
        "level",
        "Level",
        Better::Lower,
        Tolerance::DECIBELS,
        Shown::Decibels,
    );

    fn case(value: f64) -> Measured {
        Measured {
            suite: "test",
            subject: SubjectName("Resonate High".to_owned()),
            family: Family::Ours,
            pair: Pair::new(SampleRate::HZ_44100, SampleRate::HZ_48000),
            readings: vec![Reading::of(&LEVEL, value)],
            curves: Vec::new(),
        }
    }

    #[test]
    fn what_is_written_reads_back_and_weighs_level_with_itself() {
        let measured = [case(-243.25), case(f64::NEG_INFINITY)];
        let kept = read(&written(&measured)).unwrap_or_default();

        assert_eq!(kept.len(), 2);
        assert_eq!(kept[1].value, f64::NEG_INFINITY);
        assert!(weighed(&measured[..1], &kept[..1]).regressions.is_empty());
    }

    #[test]
    fn a_reading_worse_than_kept_is_a_regression() {
        let kept = read(&written(&[case(-243.25)])).unwrap_or_default();
        let worse = [case(-200.0)];

        let weighed_worse = weighed(&worse, &kept);

        assert_eq!(weighed_worse.compared, 1);
        assert_eq!(weighed_worse.regressions.len(), 1);
        assert!(weighed_worse.gone.is_empty());
    }

    #[test]
    fn a_case_no_longer_measured_is_gone_and_compares_nothing() {
        let kept = read(&written(&[case(-243.25)])).unwrap_or_default();

        let weighed_nothing = weighed(&[], &kept);

        assert_eq!(weighed_nothing.compared, 0);
        assert_eq!(weighed_nothing.gone.len(), 1);
    }

    #[test]
    fn a_metric_no_longer_measured_in_a_case_that_still_is_gone() {
        let kept = read(&written(&[case(-243.25)])).unwrap_or_default();
        let mut bare = case(-243.25);

        bare.readings.clear();

        let measured = [bare];
        let weighed_bare = weighed(&measured, &kept);

        assert_eq!(weighed_bare.compared, 0);
        assert_eq!(weighed_bare.gone.len(), 1);
    }

    #[test]
    fn only_what_a_partial_run_could_have_measured_is_held() {
        let mut other = case(-243.25);

        other.suite = "other";
        other.subject = SubjectName("Reference".to_owned());

        let kept = read(&written(&[case(-243.25), other])).unwrap_or_default();

        assert_eq!(in_scope(kept.clone(), &["test", "other"], None).len(), 2);
        assert_eq!(in_scope(kept.clone(), &["test"], None).len(), 1);
        assert_eq!(
            in_scope(kept.clone(), &["test", "other"], Some("High")).len(),
            1
        );
        assert!(in_scope(kept, &["test"], Some("Reference")).is_empty());
    }

    #[test]
    fn a_line_short_of_a_field_is_named() {
        assert_eq!(
            read("test\t44.1-to-48\tResonate High\tlevel\n"),
            Err(UnreadableLine { line: 1 })
        );
    }
}
