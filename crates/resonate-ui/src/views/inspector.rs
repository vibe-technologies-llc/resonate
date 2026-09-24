use std::{borrow::Cow, time::SystemTime};

use gpui::{
    AnyElement, Bounds, Canvas, Context, Div, FontWeight, PathBuilder, Pixels, Point, SharedString,
    canvas, div, linear_color_stop, linear_gradient, point, prelude::*, px, rgb,
};
use resonate_core::{AppliedGain, Decibels, Frames, SampleRate};
use resonate_engine::{
    BitRate, BoxLayout, Codec, Container, Faststart, MediaInfo, OutputMode, OutputStatus,
    PacketSpan, Packing, PlayerState, ReplayGain, ReplayGainMode, StreamDigest, TagSet,
    TopLevelBox, WINDOW,
};

use crate::{
    Selection, format,
    icons::{self, Icon},
    theme,
    views::{
        browser::OPEN_ALBUM_HINT,
        kit::{self, EndsInAnEllipsis, Tone},
        root::RootView,
        scrollbar::Scrollbars,
        transport::Heard,
    },
};

const GRAPH_COLUMNS: usize = 240;
const GRAPH_HEIGHT: f32 = 160.0;
const GRAPH_LINE: f32 = 1.75;
const CARD_WIDTH: f32 = 232.0;
const NOTHING_DECLARED: &str = "The file declares none.";

const GRAPH_HINT: &str = "Draw the bitrate over the decoded span as a line, one point per window";

const GRAPH_OPEN_HINT: &str = "Hide the bitrate graph";

impl RootView {
    fn inspected(&self, state: &PlayerState, digest: &StreamDigest, cx: &mut Context<Self>) -> Div {
        let playing = self.playing(state, Some(digest), cx);

        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(kit::eyebrow("INSPECTOR"))
            .child(kit::linked_title(self.opens(
                "inspected-title",
                playing.title.clone(),
                OPEN_ALBUM_HINT,
                playing.cover.album.map(Selection::Album),
                cx,
            )))
            .child(self.by_line("inspected-artist", "inspected-album", &playing, cx))
    }

    pub(crate) fn inspector(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.player.read(cx);
        let Some(digest) = model.digest() else {
            return kit::empty(
                Icon::Inspector,
                "Nothing is playing.",
                Some(
                    "The inspector follows the decoder and the sink, so it fills as soon as a track starts.",
                ),
            );
        };
        let output = model.state().output;
        let sink = output
            .and_then(|output| model.sinks().iter().find(|sink| sink.id == output.sink))
            .map(|sink| sink.description.clone());
        let graphing = self.bitrate_graph;
        let state = model.state().clone();
        let heard = self.playing(&state, Some(&digest), cx).heard;
        let heading = self.inspected(&state, &digest, cx);

        let mut pane = div()
            .id("inspector")
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .gap_5()
            .px_6()
            .py_5()
            .overflow_y_scroll()
            .track_scroll(&self.inspector_scroll)
            .child(heading)
            .child(signal_path(&digest, output, sink))
            .child(beside(vec![
                source(&digest.info).into_any_element(),
                heard_card(heard.as_ref()).into_any_element(),
                sink_card(output).into_any_element(),
                bitrate(&digest, graphing, cx).into_any_element(),
                replay_gain(
                    digest.replay_gain_mode,
                    digest.replay_gain,
                    &digest.info.tags.replay_gain,
                )
                .into_any_element(),
            ]));

        if graphing && digest.profile.is_some() {
            let series = self
                .player
                .update(cx, |model, _| model.condensed(GRAPH_COLUMNS));
            pane = pane.child(graph(&digest, &series));
        }
        if let Some(layout) = digest.layout.as_ref() {
            pane = pane.child(container(layout));
        }

        let carried = self.player.update(cx, |model, _| model.carried_lines());

        Scrollbars::of(cx)
            .around(
                "inspector-scrollbar",
                self.inspector_scroll.clone(),
                pane.child(tags(&digest.info.tags, carried)),
            )
            .into_any_element()
    }
}

fn signal_path(digest: &StreamDigest, output: Option<OutputStatus>, sink: Option<String>) -> Div {
    let info = &digest.info;
    let codec = Codec::from_id(info.codec);
    let container = Container::from_id(info.container);
    let source_colour = if codec.is_lossless() {
        theme::lossless()
    } else {
        theme::lossy()
    };
    let (mode_label, mode_colour) = output.map_or(("closed", theme::faint()), |output| {
        format::mode(output.mode)
    });
    let processing = output.map_or_else(
        || "no stream".to_owned(),
        |output| match output.mode {
            OutputMode::BitPerfect => "untouched".to_owned(),
            OutputMode::Repacked => "repacked".to_owned(),
            OutputMode::Dithered => format!("→ {}", format::depth(output.negotiated.format)),
            OutputMode::Converted => format!("→ {}", format::quality(output.negotiated)),
        },
    );
    let gain = format::applied(digest.replay_gain);

    div()
        .flex()
        .gap_2()
        .child(stage(
            "SOURCE",
            codec.as_str().to_owned(),
            source_colour,
            vec![
                format::quality(info.spec),
                format!("{} · {container}", info.spec.channels),
            ],
        ))
        .child(joint())
        .child(stage(
            "PROCESSING",
            processing,
            theme::text(),
            vec![
                format!("gain {gain}"),
                format!("replaygain {}", mode_text(digest.replay_gain_mode)),
            ],
        ))
        .child(joint())
        .child(stage(
            "OUTPUT",
            mode_label.to_owned(),
            mode_colour,
            vec![
                output.map_or_else(
                    || "stream closed".to_owned(),
                    |output| format::quality(output.negotiated),
                ),
                sink.unwrap_or_else(|| "no sink".to_owned()),
            ],
        ))
}

fn stage(label: &'static str, figure: String, colour: u32, lines: Vec<String>) -> Div {
    let mut card = kit::card()
        .flex_1()
        .min_w(px(theme::stage_width()))
        .gap_1()
        .child(kit::eyebrow(label))
        .child(
            div()
                .font(theme::mono(FontWeight::MEDIUM))
                .text_size(px(theme::text_xl()))
                .line_height(px(theme::text_xl() * 1.2))
                .text_color(rgb(colour))
                .truncate()
                .ends_in_an_ellipsis()
                .child(SharedString::from(figure)),
        );
    for line in lines {
        card = card.child(
            div()
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::muted()))
                .truncate()
                .ends_in_an_ellipsis()
                .child(SharedString::from(line)),
        );
    }
    card
}

fn joint() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .child(icons::icon(Icon::Link, 16.0, theme::faint()))
}

fn source(info: &MediaInfo) -> Div {
    let rate = info.spec.rate;
    let coded = info.bits_per_coded_sample.map(|bits| format!("{bits}-bit"));
    let reading = match info.packing {
        Packing::DopMarked(dsd) => format!("{dsd} ({rate} carrier)"),
        Packing::Samples => rate.to_string(),
    };

    fields(
        summary("Source"),
        vec![
            ("sample rate", Some(reading)),
            (
                "decoded depth",
                Some(format::depth(info.spec.format).to_owned()),
            ),
            ("coded depth", coded),
            ("channels", Some(info.spec.channels.to_string())),
            (
                "duration",
                info.duration.map(|frames| format::clock(frames, rate)),
            ),
            (
                "encoder delay",
                (info.encoder_delay > 0 || info.encoder_padding > 0)
                    .then(|| format!("{} / {} frames", info.encoder_delay, info.encoder_padding)),
            ),
            ("seekable", Some(yes_no(info.is_seekable).to_owned())),
        ],
    )
}

fn heard_card(heard: Option<&Heard>) -> Div {
    let now = SystemTime::now();
    let Some(heard) = heard else {
        return fields(
            summary("Heard"),
            vec![("plays", Some("no library row".to_owned()))],
        );
    };

    fields(
        summary("Heard"),
        vec![
            (
                "plays",
                Some(match heard.plays {
                    0 => "never".to_owned(),
                    plays => format::counted(plays as usize, "time", "times"),
                }),
            ),
            (
                "last",
                heard.played.map(|played| format::since(played, now)),
            ),
            ("added", Some(format::since(heard.added, now))),
        ],
    )
}

fn sink_card(output: Option<OutputStatus>) -> Div {
    let Some(output) = output else {
        return fields(
            summary("Output"),
            vec![("stream", Some("closed".to_owned()))],
        );
    };
    let (label, colour) = format::mode(output.mode);
    let latency = output
        .latency
        .to_duration(output.negotiated.rate)
        .as_millis();

    fields(
        summary("Output").child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(kit::mode_dot(colour))
                .child(kit::badge(label, colour)),
        ),
        vec![
            ("negotiated", Some(format::quality(output.negotiated))),
            ("latency", Some(format!("{latency} ms"))),
            ("underruns", Some(output.underruns.to_string())),
            (
                "went without",
                (output.went_without > Frames::ZERO).then(|| {
                    let lost = output.went_without.to_duration(output.negotiated.rate);
                    format!("{} ms", lost.as_millis())
                }),
            ),
        ],
    )
}

fn bitrate(digest: &StreamDigest, graphing: bool, cx: &mut Context<RootView>) -> impl IntoElement {
    let rate = digest.info.spec.rate;
    let Some(profile) = digest.profile.as_ref() else {
        return fields(
            summary("Bitrate"),
            vec![("profile", Some("decoding".to_owned()))],
        )
        .into_any_element();
    };

    fields(
        summary("Bitrate"),
        vec![
            ("overall", Some(kbps(profile.overall))),
            ("now", Some(instant(digest.packet, rate))),
            ("variability", Some(profile.variability.to_string())),
            ("mean", Some(kbps(profile.mean))),
            ("deviation", Some(kbps(profile.deviation))),
            (
                "range",
                Some(format!("{} — {}", kbps(profile.min), kbps(profile.max))),
            ),
            ("packets", Some(profile.packets.to_string())),
            (
                "profiled",
                Some(format!(
                    "{} of {}",
                    format::clock(digest.decoded.saturating_sub(digest.profiled_from), rate),
                    format::clock(digest.decoded, rate)
                )),
            ),
        ],
    )
    .child(
        div().mt_1().child(
            kit::button(
                "bitrate-graph",
                Some(Icon::Inspector),
                if graphing { "Hide graph" } else { "Show graph" },
                if graphing {
                    GRAPH_OPEN_HINT
                } else {
                    GRAPH_HINT
                },
                Tone::Outlined,
            )
            .on_click(cx.listener(|this, _, _, cx| this.toggle_bitrate_graph(cx))),
        ),
    )
    .into_any_element()
}

fn replay_gain(mode: ReplayGainMode, applied: AppliedGain, gain: &ReplayGain) -> Div {
    let declared = gain.track_gain.is_some()
        || gain.track_peak.is_some()
        || gain.album_gain.is_some()
        || gain.album_peak.is_some();

    fields(
        summary("ReplayGain"),
        vec![
            ("mode", declared.then(|| mode_text(mode).to_owned())),
            ("track gain", gain.track_gain.map(decibels)),
            ("track peak", gain.track_peak.map(peak)),
            ("album gain", gain.album_gain.map(decibels)),
            ("album peak", gain.album_peak.map(peak)),
            ("applied", declared.then(|| format::applied(applied))),
            ("headroom", declared.then(|| format::headroom(applied))),
        ],
    )
}

fn graph(digest: &StreamDigest, series: &[BitRate]) -> Div {
    let top = series.iter().map(|rate| rate.get()).fold(0.0_f64, f64::max);
    let floor = series
        .iter()
        .map(|rate| rate.get())
        .fold(f64::INFINITY, f64::min);
    let span = (top - floor).max(f64::EPSILON);
    let rate = digest.info.spec.rate;

    let mut shares: Vec<f32> = series
        .iter()
        .map(|point| (((point.get() - floor) / span) as f32).clamp(0.0, 1.0))
        .collect();
    if let [level] = shares.as_slice() {
        shares.push(*level);
    }

    card("Bitrate over time")
        .child(
            div()
                .h(px(GRAPH_HEIGHT))
                .w_full()
                .mt_2()
                .child(traced(shares)),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .child(kit::figure(format::clock(digest.profiled_from, rate)))
                .child(kit::figure(format!(
                    "{} — {} over {WINDOW:.0}s windows",
                    kbps(BitRate::new(floor).unwrap_or_default()),
                    kbps(BitRate::new(top).unwrap_or_default())
                )))
                .child(kit::figure(format::clock(digest.decoded, rate))),
        )
}

fn traced(shares: Vec<f32>) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let plotted = plotted(&shares, bounds);
            let (Some(first), Some(last)) = (plotted.first(), plotted.last()) else {
                return;
            };
            if plotted.len() < 2 {
                return;
            }

            let mut under = PathBuilder::fill();
            under.move_to(point(first.x, bounds.bottom()));
            for at in &plotted {
                under.line_to(*at);
            }
            under.line_to(point(last.x, bounds.bottom()));
            under.close();

            match under.build() {
                Ok(shaded) => window.paint_path(
                    shaded,
                    linear_gradient(
                        180.0,
                        linear_color_stop(theme::tinted(theme::accent(), 0x4d), 0.0),
                        linear_color_stop(theme::tinted(theme::accent(), 0x00), 1.0),
                    ),
                ),
                Err(error) => {
                    tracing::debug!(%error, "the bitrate graph's wash would not tessellate")
                }
            }

            let mut line = PathBuilder::stroke(px(GRAPH_LINE));
            line.move_to(*first);
            for at in plotted.iter().skip(1) {
                line.line_to(*at);
            }

            match line.build() {
                Ok(drawn) => window.paint_path(drawn, rgb(theme::accent())),
                Err(error) => {
                    tracing::debug!(%error, "the bitrate graph's line would not tessellate")
                }
            }
        },
    )
    .size_full()
}

fn plotted(shares: &[f32], bounds: Bounds<Pixels>) -> Vec<Point<Pixels>> {
    let across = f32::from(bounds.size.width) - GRAPH_LINE;
    let down = f32::from(bounds.size.height) - GRAPH_LINE;
    let left = f32::from(bounds.origin.x) + GRAPH_LINE / 2.0;
    let top = f32::from(bounds.origin.y) + GRAPH_LINE / 2.0;
    let steps = shares.len().saturating_sub(1).max(1) as f32;

    shares
        .iter()
        .enumerate()
        .map(|(column, share)| {
            point(
                px(left + column as f32 / steps * across),
                px(top + (1.0 - share) * down),
            )
        })
        .collect()
}

fn container(layout: &BoxLayout) -> Div {
    let verdict = layout.faststart.map_or_else(
        || "no moov/mdat pair".to_owned(),
        |state| match state {
            Faststart::Ready => format!("{state} — plays before the whole file arrives"),
            Faststart::Trailing => format!("{state} — the index is at the end"),
        },
    );

    let mut panel = fields(card("Container layout"), vec![("faststart", Some(verdict))]);
    for entry in &layout.boxes {
        panel = panel.child(atom(*entry));
    }
    panel
}

fn atom(entry: TopLevelBox) -> Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(
            kit::figure(entry.kind.to_string())
                .w(px(56.0))
                .text_color(rgb(theme::text())),
        )
        .child(
            kit::figure(format!("at {}", entry.at))
                .w(px(112.0))
                .text_color(rgb(theme::faint())),
        )
        .child(kit::figure(format::bytes(entry.bytes)))
}

fn tags<'a>(set: &'a TagSet, carried: Option<usize>) -> Div {
    let numbered = |value: Option<u32>, total: Option<u32>| match (value, total) {
        (Some(value), Some(total)) => Some(Cow::Owned(format!("{value} of {total}"))),
        (Some(value), None) => Some(Cow::Owned(value.to_string())),
        _ => None,
    };
    let borrowed = |value: &'a Option<String>| value.as_deref().map(Cow::Borrowed);

    let named = [
        ("title", borrowed(&set.title)),
        ("artist", borrowed(&set.artist)),
        ("album", borrowed(&set.album)),
        ("album artist", borrowed(&set.album_artist)),
        ("track", numbered(set.track_number, set.track_total)),
        ("disc", numbered(set.disc_number, set.disc_total)),
        ("date", borrowed(&set.date)),
        ("genre", borrowed(&set.genre)),
        (
            "compilation",
            set.compilation.then_some(Cow::Borrowed("yes")),
        ),
        ("grouping", borrowed(&set.grouping)),
        ("collection", borrowed(&set.collection)),
        ("edition", borrowed(&set.edition)),
        ("composer", borrowed(&set.credits.composer)),
        ("conductor", borrowed(&set.credits.conductor)),
        ("lyricist", borrowed(&set.credits.lyricist)),
        ("performer", borrowed(&set.credits.performer)),
        ("remixer", borrowed(&set.credits.remixer)),
        ("producer", borrowed(&set.credits.producer)),
        ("engineer", borrowed(&set.credits.engineer)),
        ("label", borrowed(&set.label)),
        ("isrc", borrowed(&set.isrc)),
        (
            "tempo",
            set.beats_per_minute
                .map(|beats| Cow::Owned(format!("{beats} bpm"))),
        ),
        ("copyright", borrowed(&set.copyright)),
        ("encoder", borrowed(&set.encoder)),
        ("comment", borrowed(&set.comment)),
        ("musicbrainz track", borrowed(&set.musicbrainz_track_id)),
        ("musicbrainz album", borrowed(&set.musicbrainz_album_id)),
        ("musicbrainz artist", borrowed(&set.musicbrainz_artist_id)),
        (
            "musicbrainz album artist",
            borrowed(&set.musicbrainz_album_artist_id),
        ),
        (
            "musicbrainz release group",
            borrowed(&set.musicbrainz_release_group_id),
        ),
        (
            "musicbrainz release track",
            borrowed(&set.musicbrainz_release_track_id),
        ),
        ("barcode", borrowed(&set.barcode)),
        ("catalog number", borrowed(&set.catalog_number)),
        (
            "lyrics",
            carried.map(|lines| Cow::Owned(format!("{lines} lines"))),
        ),
    ];

    fields(card("Tags"), named.into())
}

fn summary(title: &'static str) -> Div {
    card(title).min_w(px(0.0))
}

pub(crate) fn beside(cards: Vec<AnyElement>) -> Div {
    let mut left = column();
    let mut right = column();
    for (at, card) in cards.into_iter().enumerate() {
        if at % 2 == 0 {
            left = left.child(card);
        } else {
            right = right.child(card);
        }
    }

    div().flex().flex_wrap().gap_3().child(left).child(right)
}

fn column() -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .gap_3()
        .min_w(px(CARD_WIDTH))
}

pub(crate) fn card(title: &'static str) -> Div {
    kit::card().gap_1p5().child(kit::card_title(title).pb_1())
}

pub(crate) fn fields<'a, Told: Into<Cow<'a, str>>>(
    panel: Div,
    rows: Vec<(&'static str, Option<Told>)>,
) -> Div {
    let told: Vec<(&'static str, SharedString)> = rows
        .into_iter()
        .filter_map(|(name, value)| value.map(|value| (name, SharedString::new(value.into()))))
        .collect();
    if told.is_empty() {
        return panel.child(
            div()
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::faint()))
                .child(NOTHING_DECLARED),
        );
    }

    told.into_iter().fold(panel, |panel, (name, value)| {
        panel.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .text_size(px(theme::text_sm()))
                        .text_color(rgb(theme::faint()))
                        .whitespace_nowrap()
                        .child(name),
                )
                .child(kit::figure(value).text_color(rgb(theme::text())).truncate()),
        )
    })
}

fn instant(packet: Option<PacketSpan>, rate: SampleRate) -> String {
    packet
        .and_then(|packet| packet.rate(rate))
        .map_or_else(unknown, kbps)
}

const fn mode_text(mode: ReplayGainMode) -> &'static str {
    match mode {
        ReplayGainMode::Off => "off",
        ReplayGainMode::Track => "track",
        ReplayGainMode::Album => "album",
    }
}

fn kbps(rate: BitRate) -> String {
    format!("{:.0} kbps", rate.kilobits())
}

fn decibels(gain: Decibels) -> String {
    format!("{:+.2} dB", gain.get())
}

fn peak(value: f32) -> String {
    format!("{value:.4}")
}

fn yes_no(value: bool) -> String {
    if value { "yes" } else { "no" }.to_owned()
}

fn unknown() -> String {
    "unknown".to_owned()
}
