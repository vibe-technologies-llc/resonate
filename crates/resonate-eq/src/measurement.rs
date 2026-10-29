use resonate_core::text;

use crate::{Error, Result, apo::read_number};

pub const LARGEST_MEASUREMENT: usize = 4 * 1024 * 1024;
pub const POINTS_AT_LEAST: usize = 8;

const FLAT_FROM_HZ: f64 = 20.0;
const FLAT_TO_HZ: f64 = 20_000.0;
const HIGHEST_HERTZ_READ: f64 = 1_000_000.0;
const LOUDEST_DB_READ: f64 = 400.0;
const REW_COMMENT: char = '*';
const COMMENT: char = '#';
const LEVEL_COLUMN_BY_DEFAULT: usize = 1;
const LEVEL_COLUMN_NAMES: [&str; 7] = [
    "raw",
    "spl",
    "magnitude",
    "level",
    "response",
    "amplitude",
    "gain",
];
const FREQUENCY_COLUMN_START: &str = "freq";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub hertz: f64,
    pub decibels: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Rew,
    Headed,
    Plain,
}

impl Layout {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rew => "REW text",
            Self::Headed => "headed CSV",
            Self::Plain => "plain text",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    points: Vec<Point>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub response: Response,
    pub layout: Layout,
    pub passed_over: usize,
}

impl Response {
    pub fn flat() -> Self {
        let level = |hertz| Point {
            hertz,
            decibels: 0.0,
        };
        Self {
            points: vec![level(FLAT_FROM_HZ), level(FLAT_TO_HZ)],
        }
    }

    pub fn from_points(mut points: Vec<Point>) -> Option<Self> {
        points.retain(|point| {
            point.hertz.is_finite()
                && point.hertz > 0.0
                && point.hertz <= HIGHEST_HERTZ_READ
                && point.decibels.is_finite()
                && point.decibels.abs() <= LOUDEST_DB_READ
        });
        points.sort_by(|a, b| a.hertz.total_cmp(&b.hertz));
        points.dedup_by(|later, earlier| later.hertz == earlier.hertz);
        (points.len() >= 2).then_some(Self { points })
    }

    pub fn from_levels(levels: impl IntoIterator<Item = (f64, f64)>) -> Result<Self> {
        let points = levels
            .into_iter()
            .map(|(hertz, decibels)| Point { hertz, decibels })
            .collect();
        Self::from_points(points).ok_or(Error::NoMeasuredPoints)
    }

    pub fn calibrated_by(&self, microphone: &Self) -> Result<Self> {
        Self::from_levels(
            self.points
                .iter()
                .map(|point| (point.hertz, point.decibels - microphone.at(point.hertz))),
        )
    }

    pub fn points(&self) -> &[Point] {
        &self.points
    }

    pub fn lowest_hertz(&self) -> f64 {
        self.points.first().map_or(0.0, |point| point.hertz)
    }

    pub fn highest_hertz(&self) -> f64 {
        self.points.last().map_or(0.0, |point| point.hertz)
    }

    pub fn at(&self, hertz: f64) -> f64 {
        let (Some(first), Some(last)) = (self.points.first(), self.points.last()) else {
            return 0.0;
        };
        if hertz <= first.hertz {
            return first.decibels;
        }
        if hertz >= last.hertz {
            return last.decibels;
        }

        let above = self.points.partition_point(|point| point.hertz < hertz);
        let (Some(below), Some(above)) = (
            above.checked_sub(1).and_then(|at| self.points.get(at)),
            self.points.get(above),
        ) else {
            return last.decibels;
        };
        let along = (hertz / below.hertz).ln() / (above.hertz / below.hertz).ln();
        below.decibels + along * (above.decibels - below.decibels)
    }

    pub fn smoothed_at(&self, hertz: f64, half_width_octaves: f64) -> f64 {
        let reach = 2.0_f64.powf(half_width_octaves);
        let from = self
            .points
            .partition_point(|point| point.hertz < hertz / reach);
        let to = self
            .points
            .partition_point(|point| point.hertz <= hertz * reach);
        let within = self.points.get(from..to).unwrap_or_default();
        if within.is_empty() {
            return self.at(hertz);
        }
        within.iter().map(|point| point.decibels).sum::<f64>() / within.len() as f64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Columns {
    level: usize,
}

fn fields_of(line: &str) -> Vec<&str> {
    if line.contains('\t') {
        return line.split('\t').map(str::trim).collect();
    }
    if line.contains(';') {
        return line.split(';').map(str::trim).collect();
    }

    let one_token = line.split_whitespace().nth(1).is_none();
    if line.contains(',') && (one_token || line.contains(", ")) {
        return line.split(',').map(str::trim).collect();
    }
    line.split_whitespace().collect()
}

fn header_columns(fields: &[&str]) -> Columns {
    let level = LEVEL_COLUMN_NAMES
        .iter()
        .find_map(|wanted| {
            fields
                .iter()
                .skip(1)
                .position(|field| field.to_ascii_lowercase().starts_with(wanted))
        })
        .map_or(LEVEL_COLUMN_BY_DEFAULT, |at| at + 1);
    Columns { level }
}

fn is_a_header(fields: &[&str]) -> bool {
    fields.first().is_some_and(|first| {
        first
            .to_ascii_lowercase()
            .starts_with(FREQUENCY_COLUMN_START)
    })
}

fn point_of(fields: &[&str], columns: Columns) -> Option<Point> {
    let hertz = fields.first().copied().and_then(read_number)?;
    let decibels = fields.get(columns.level).copied().and_then(read_number)?;
    Some(Point { hertz, decibels })
}

pub fn read_response(bytes: &[u8]) -> Result<Reading> {
    if bytes.len() > LARGEST_MEASUREMENT {
        return Err(Error::MeasurementTooLarge {
            limit: LARGEST_MEASUREMENT,
        });
    }

    let (text, _) = text::decoded(bytes);
    let mut columns = Columns {
        level: LEVEL_COLUMN_BY_DEFAULT,
    };
    let mut points = Vec::new();
    let mut passed_over = 0;
    let mut saw_rew = false;
    let mut saw_header = false;

    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if line.starts_with(REW_COMMENT) {
            saw_rew = true;
            continue;
        }
        if line.starts_with(COMMENT) {
            continue;
        }

        let fields = fields_of(line);
        match point_of(&fields, columns) {
            Some(point) => points.push(point),
            None if points.is_empty() && !saw_header && is_a_header(&fields) => {
                saw_header = true;
                columns = header_columns(&fields);
            }
            None => passed_over += 1,
        }
    }

    let found = points.len();
    let response = Response::from_points(points).ok_or(Error::NoMeasuredPoints)?;
    if response.points().len() < POINTS_AT_LEAST {
        return Err(Error::TooFewMeasuredPoints {
            found,
            least: POINTS_AT_LEAST,
        });
    }

    let layout = match (saw_rew, saw_header) {
        (true, _) => Layout::Rew,
        (false, true) => Layout::Headed,
        (false, false) => Layout::Plain,
    };
    Ok(Reading {
        response,
        layout,
        passed_over,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const REW: &str = "* Measurement data measured by REW V5.31\n\
                       * Source: Left Channel, 10/28/2026\n\
                       * Freq(Hz), SPL(dB), Phase(degrees)\n\
                       20.000\t60.1\t-10.0\n\
                       25.200\t61.0\t-14.0\n\
                       31.700\t62.5\t-20.0\n\
                       40.000\t64.0\t-31.0\n\
                       100.00\t70.0\t-45.0\n\
                       250.00\t71.5\t-60.0\n\
                       1000.0\t72.0\t-90.0\n\
                       4000.0\t75.0\t-120.0\n\
                       10000\t68.0\t-150.0\n";

    const AUTOEQ: &str = "frequency,raw,error,smoothed\n\
                          20.0,1.0,0.1,0.9\n\
                          40.0,2.0,0.2,1.9\n\
                          80.0,3.0,0.3,2.9\n\
                          160.0,4.0,0.4,3.9\n\
                          320.0,5.0,0.5,4.9\n\
                          640.0,6.0,0.6,5.9\n\
                          1280.0,7.0,0.7,6.9\n\
                          2560.0,8.0,0.8,7.9\n";

    const PLAIN: &str = "20 -1.5\n\
                         50 0\n\
                         100 1,5\n\
                         200 2\n\
                         500 2.5\n\
                         1000 3\n\
                         2000 2\n\
                         5000 -4\n\
                         10000 -8\n";

    fn read(text: &str) -> Reading {
        read_response(text.as_bytes()).expect("a readable measurement")
    }

    #[test]
    fn rews_exported_text_is_read_past_its_star_lines_with_the_level_from_the_second_column() {
        let reading = read(REW);

        assert_eq!(reading.layout, Layout::Rew);
        assert_eq!(reading.passed_over, 0);
        assert_eq!(reading.response.points().len(), 9);
        assert_eq!(reading.response.points()[0].hertz, 20.0);
        assert_eq!(reading.response.points()[0].decibels, 60.1);
        assert_eq!(reading.response.points()[8].decibels, 68.0);
    }

    #[test]
    fn autoeqs_headed_csv_is_read_from_the_raw_column_whatever_follows_it() {
        let reading = read(AUTOEQ);

        assert_eq!(reading.layout, Layout::Headed);
        assert_eq!(reading.response.points().len(), 8);
        assert_eq!(reading.response.points()[2].decibels, 3.0);
    }

    #[test]
    fn a_headed_file_names_its_level_column_whichever_place_it_stands_in() {
        let reading = read(
            "Frequency (Hz);Phase;SPL (dB)\n20;0;1\n40;0;2\n80;0;3\n160;0;4\n320;0;5\n640;0;6\n\
             1280;0;7\n2560;0;8\n",
        );

        assert_eq!(reading.layout, Layout::Headed);
        assert_eq!(reading.response.points()[0].decibels, 1.0);
        assert_eq!(reading.response.points()[7].decibels, 8.0);
    }

    #[test]
    fn two_plain_columns_are_read_with_comma_decimals_and_no_header() {
        let reading = read(PLAIN);

        assert_eq!(reading.layout, Layout::Plain);
        assert_eq!(reading.response.points().len(), 9);
        assert_eq!(reading.response.points()[2].decibels, 1.5);
    }

    #[test]
    fn a_csv_of_two_columns_without_a_header_is_read_as_one() {
        let reading = read("20,1\n40,2\n80,3\n160,4\n320,5\n640,6\n1280,7\n2560,8\n5120,9\n");

        assert_eq!(reading.layout, Layout::Plain);
        assert_eq!(reading.response.points().len(), 9);
        assert_eq!(reading.response.points()[1].decibels, 2.0);
    }

    #[test]
    fn a_csv_with_spaces_after_its_commas_is_split_on_the_commas() {
        let reading = read("20.5, 1.5\n40, 2\n80, 3\n160, 4\n320, 5\n640, 6\n1280, 7\n2560, 8\n");

        assert_eq!(reading.response.points()[0].hertz, 20.5);
        assert_eq!(reading.response.points()[0].decibels, 1.5);
    }

    #[test]
    fn a_file_in_utf16_is_read_as_the_text_it_holds() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in REW.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }

        let reading = read_response(&bytes).expect("readable");

        assert_eq!(reading.layout, Layout::Rew);
        assert_eq!(reading.response.points().len(), 9);
    }

    #[test]
    fn points_arrive_sorted_and_a_repeated_frequency_keeps_its_first_level() {
        let reading = read("100 1\n50 2\n50 9\n200 3\n400 4\n800 5\n1600 6\n3200 7\n6400 8\n");

        let hertz: Vec<f64> = reading.response.points().iter().map(|p| p.hertz).collect();
        assert_eq!(
            hertz,
            [50.0, 100.0, 200.0, 400.0, 800.0, 1600.0, 3200.0, 6400.0]
        );
        assert_eq!(reading.response.points()[0].decibels, 2.0);
    }

    #[test]
    fn a_line_that_is_not_a_point_is_passed_over_and_counted() {
        let reading = read(&format!("{PLAIN}wat\n300\nnan 4\n"));

        assert_eq!(reading.passed_over, 3);
        assert_eq!(reading.response.points().len(), 9);
    }

    #[test]
    fn a_curve_is_read_along_the_log_of_the_frequency_and_held_past_its_ends() {
        let curve = Response::from_points(vec![
            Point {
                hertz: 100.0,
                decibels: 0.0,
            },
            Point {
                hertz: 400.0,
                decibels: 12.0,
            },
        ])
        .expect("two points");

        assert!((curve.at(200.0) - 6.0).abs() < 1e-9);
        assert_eq!(curve.at(10.0), 0.0);
        assert_eq!(curve.at(10_000.0), 12.0);
    }

    #[test]
    fn smoothing_averages_what_lies_within_the_window_and_interpolates_where_nothing_does() {
        let curve = Response::from_points(
            (0..40)
                .map(|step| Point {
                    hertz: 100.0 * 1.01_f64.powi(step),
                    decibels: if step % 2 == 0 { 3.0 } else { -3.0 },
                })
                .collect(),
        )
        .expect("points");

        assert!(curve.smoothed_at(110.0, 1.0 / 12.0).abs() < 0.5);

        let sparse = Response::from_points(vec![
            Point {
                hertz: 100.0,
                decibels: 0.0,
            },
            Point {
                hertz: 400.0,
                decibels: 12.0,
            },
        ])
        .expect("two points");
        assert!((sparse.smoothed_at(200.0, 1.0 / 12.0) - 6.0).abs() < 1e-9);
    }

    #[test]
    fn a_file_with_no_line_that_reads_as_a_point_is_refused() {
        for empty in [
            "",
            "\n\n",
            "* REW comments only\n# and another\n",
            "frequency,raw\n",
        ] {
            assert!(
                matches!(
                    read_response(empty.as_bytes()),
                    Err(Error::NoMeasuredPoints)
                ),
                "{empty:?} was taken"
            );
        }
        assert!(matches!(
            read_response(b"this is not a measurement\nnor is this one\n"),
            Err(Error::NoMeasuredPoints)
        ));
    }

    #[test]
    fn a_file_of_fewer_points_than_a_response_needs_is_refused_with_how_many_it_held() {
        assert!(matches!(
            read_response(b"100 1\n200 2\n400 3\n"),
            Err(Error::TooFewMeasuredPoints {
                found: 3,
                least: POINTS_AT_LEAST
            })
        ));
    }

    #[test]
    fn points_that_cannot_be_a_response_do_not_count_toward_one() {
        let nonsense = "0 1\n-5 2\n2000000 3\n100 1000\n100 nan\n100 inf\n";

        assert!(matches!(
            read_response(nonsense.as_bytes()),
            Err(Error::NoMeasuredPoints)
        ));
    }

    #[test]
    fn a_file_larger_than_a_measurement_may_be_is_refused_before_it_is_read() {
        let huge = vec![b'1'; LARGEST_MEASUREMENT + 1];

        assert!(matches!(
            read_response(&huge),
            Err(Error::MeasurementTooLarge {
                limit: LARGEST_MEASUREMENT
            })
        ));
    }

    #[test]
    fn levels_become_a_response_and_a_calibration_is_taken_off_point_by_point() {
        let heard = Response::from_levels((0..40).map(|step| (20.0 * 1.2_f64.powi(step), 3.0)))
            .expect("a response");
        let microphone =
            Response::from_levels((0..40).map(|step| (20.0 * 1.2_f64.powi(step), 1.0)))
                .expect("a response");

        let calibrated = heard.calibrated_by(&microphone).expect("a response");

        assert!(
            calibrated
                .points()
                .iter()
                .all(|point| (point.decibels - 2.0).abs() < 1e-9)
        );
        assert!(matches!(
            Response::from_levels([(100.0, f64::NAN)]),
            Err(Error::NoMeasuredPoints)
        ));
    }
}
