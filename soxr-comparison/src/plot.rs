use std::{fmt::Write as _, fs, path::Path};

use resonate_dsp::{FilterPhase, Quality};
use soxr::params::QualityRecipe;

use crate::{
    contender::{Contender, Pair, SoxrPhase},
    error::{Error, Result},
    measure::{AUDIBLE_HZ, Tone},
    summary::Measured,
};

const WIDTH: f64 = 960.0;
const HEIGHT: f64 = 460.0;
const LEFT: f64 = 64.0;
const RIGHT: f64 = 20.0;
const TOP: f64 = 64.0;
const BOTTOM: f64 = 52.0;
const SERIES_CLASSES: [&str; 4] = ["one", "two", "three", "four"];

pub const PLOTTED: [Contender; 4] = [
    Contender::Ours {
        quality: Quality::High,
        phase: FilterPhase::Linear,
    },
    Contender::Ours {
        quality: Quality::VeryHigh,
        phase: FilterPhase::Linear,
    },
    Contender::Soxr {
        recipe: QualityRecipe::Bits28,
        phase: SoxrPhase::Linear,
        steep: false,
    },
    Contender::Soxr {
        recipe: QualityRecipe::Bits28,
        phase: SoxrPhase::Linear,
        steep: true,
    },
];

const STYLE: &str = r#"<style>
.surface { fill: #fcfcfb; }
text { fill: #52514e; font: 12px system-ui, sans-serif; }
.title { fill: #0b0b0b; font-size: 15px; font-weight: 600; }
.grid { stroke: #e4e3de; stroke-width: 1; }
.marker { stroke: #52514e; stroke-width: 1; stroke-dasharray: 4 4; }
.line { fill: none; stroke-width: 2; stroke-linejoin: round; stroke-linecap: round; }
.one { stroke: #2a78d6; fill: #2a78d6; }
.two { stroke: #eb6834; fill: #eb6834; }
.three { stroke: #1baf7a; fill: #1baf7a; }
.four { stroke: #eda100; fill: #eda100; }
.line.one, .line.two, .line.three, .line.four { fill: none; }
@media (prefers-color-scheme: dark) {
  .surface { fill: #1a1a19; }
  text { fill: #c3c2b7; }
  .title { fill: #f0efec; }
  .grid { stroke: #34332f; }
  .marker { stroke: #c3c2b7; }
  .one { stroke: #3987e5; fill: #3987e5; }
  .two { stroke: #d95926; fill: #d95926; }
  .three { stroke: #199e70; fill: #199e70; }
  .four { stroke: #c98500; fill: #c98500; }
  .line.one, .line.two, .line.three, .line.four { fill: none; }
}
</style>"#;

struct Frame {
    from_hz: f64,
    to_hz: f64,
    floor_db: f64,
    ceiling_db: f64,
}

impl Frame {
    fn x(&self, hz: f64) -> f64 {
        LEFT + (hz - self.from_hz) / (self.to_hz - self.from_hz) * (WIDTH - LEFT - RIGHT)
    }

    fn y(&self, db: f64) -> f64 {
        let clamped = db.clamp(self.floor_db, self.ceiling_db);
        TOP + (self.ceiling_db - clamped) / (self.ceiling_db - self.floor_db)
            * (HEIGHT - TOP - BOTTOM)
    }
}

fn response(tones: &[Tone]) -> Vec<(f64, f64)> {
    let mut points: Vec<(f64, f64)> = tones
        .iter()
        .flat_map(|tone| {
            let heard = tone.heard.map(|heard| (tone.hz, heard.gain_db));
            let leak = tone.leak.map(|leak| (tone.hz, leak.level_db));
            let image = tone.image.map(|image| (image.at_hz, image.level_db));
            [heard, leak, image].into_iter().flatten()
        })
        .collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    points
}

fn tick_step(span: f64, wanted: f64, steps: &[f64]) -> f64 {
    steps
        .iter()
        .copied()
        .find(|step| span / step <= wanted)
        .unwrap_or(span)
}

fn chart(title: &str, frame: &Frame, pair: Pair, series: &[(&Measured, &str)]) -> String {
    let mut svg = String::new();
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {WIDTH} {HEIGHT}" width="{WIDTH}" height="{HEIGHT}" role="img" aria-label="{title}">{STYLE}<rect class="surface" width="{WIDTH}" height="{HEIGHT}" rx="8"/><text class="title" x="{LEFT}" y="24">{title}</text>"#
    );

    let mut legend_x = LEFT;
    for (measured, class) in series {
        let name = measured.contender.to_string();
        let _ = write!(
            svg,
            r#"<rect class="{class}" x="{legend_x}" y="38" width="14" height="4" rx="2"/><text x="{}" y="44">{name}</text>"#,
            legend_x + 20.0
        );
        legend_x += 30.0 + 7.0 * name.len() as f64;
    }

    let db_span = frame.ceiling_db - frame.floor_db;
    let db_step = tick_step(
        db_span,
        12.0,
        &[0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 40.0],
    );
    let mut db = (frame.floor_db / db_step).ceil() * db_step;
    while db <= frame.ceiling_db + 1e-9 {
        let y = frame.y(db);
        let label = if db_step < 1.0 {
            format!("{db:.2}")
        } else {
            format!("{db:.0}")
        };
        let _ = write!(
            svg,
            r#"<line class="grid" x1="{LEFT}" x2="{}" y1="{y:.1}" y2="{y:.1}"/><text x="{}" y="{:.1}" text-anchor="end">{label}</text>"#,
            WIDTH - RIGHT,
            LEFT - 8.0,
            y + 4.0
        );
        db += db_step;
    }

    let khz_span = (frame.to_hz - frame.from_hz) / 1_000.0;
    let khz_step = tick_step(khz_span, 12.0, &[0.5, 1.0, 2.0, 4.0, 5.0, 10.0, 20.0, 25.0]);
    let mut khz = (frame.from_hz / 1_000.0 / khz_step).ceil() * khz_step;
    while khz * 1_000.0 <= frame.to_hz + 1e-6 {
        let x = frame.x(khz * 1_000.0);
        let _ = write!(
            svg,
            r#"<text x="{x:.1}" y="{}" text-anchor="middle">{khz}</text>"#,
            HEIGHT - BOTTOM + 18.0
        );
        khz += khz_step;
    }
    let _ = write!(
        svg,
        r#"<text x="{}" y="{}" text-anchor="middle">frequency (kHz)</text><text x="16" y="{}" transform="rotate(-90 16 {})" text-anchor="middle">level (dB)</text>"#,
        (LEFT + WIDTH - RIGHT) / 2.0,
        HEIGHT - 10.0,
        (TOP + HEIGHT - BOTTOM) / 2.0,
        (TOP + HEIGHT - BOTTOM) / 2.0,
    );

    let markers = [
        (AUDIBLE_HZ, "20 kHz"),
        (pair.output_nyquist(), "output Nyquist"),
        (pair.input_nyquist(), "input Nyquist"),
    ];
    for (row, (hz, label)) in markers
        .into_iter()
        .filter(|&(hz, _)| hz > frame.from_hz && hz < frame.to_hz)
        .enumerate()
    {
        let x = frame.x(hz);
        let _ = write!(
            svg,
            r#"<line class="marker" x1="{x:.1}" x2="{x:.1}" y1="{TOP}" y2="{}"/><text x="{:.1}" y="{}">{label}</text>"#,
            HEIGHT - BOTTOM,
            x + 4.0,
            TOP + 12.0 + 14.0 * row as f64
        );
    }

    for (measured, class) in series {
        let points: String = response(&measured.tones)
            .into_iter()
            .filter(|&(hz, _)| hz >= frame.from_hz && hz <= frame.to_hz)
            .map(|(hz, db)| format!("{:.1},{:.1} ", frame.x(hz), frame.y(db)))
            .collect();
        let _ = write!(
            svg,
            r#"<polyline class="line {class}" points="{}"/>"#,
            points.trim_end()
        );
    }

    svg.push_str("</svg>\n");
    svg
}

fn written(path: &Path, svg: &str) -> Result<()> {
    fs::write(path, svg).map_err(|source| Error::Write {
        path: path.to_owned(),
        source,
    })
}

pub fn plotted(directory: &Path, pair: Pair, measured: &[Measured]) -> Result<()> {
    let series: Vec<(&Measured, &str)> = PLOTTED
        .iter()
        .zip(SERIES_CLASSES)
        .filter_map(|(contender, class)| {
            measured
                .iter()
                .find(|measured| measured.contender == *contender && measured.pair == pair)
                .map(|measured| (measured, class))
        })
        .collect();

    let reach = series
        .iter()
        .flat_map(|(measured, _)| response(&measured.tones))
        .map(|(hz, _)| hz)
        .fold(pair.band_edge(), f64::max);
    let whole = Frame {
        from_hz: 0.0,
        to_hz: (reach / 1_000.0).ceil() * 1_000.0,
        floor_db: -340.0,
        ceiling_db: 10.0,
    };
    written(
        &directory.join(format!("response-{}.svg", pair.slug())),
        &chart(
            &format!("Magnitude response and rejection, {pair}"),
            &whole,
            pair,
            &series,
        ),
    )?;

    let edge = pair.band_edge();
    let passband = Frame {
        from_hz: 0.6 * edge,
        to_hz: edge,
        floor_db: -3.5,
        ceiling_db: 0.5,
    };
    written(
        &directory.join(format!("passband-{}.svg", pair.slug())),
        &chart(
            &format!("Top of the passband, {pair}"),
            &passband,
            pair,
            &series,
        ),
    )
}
