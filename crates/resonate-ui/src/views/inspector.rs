use std::{borrow::Cow, time::SystemTime};

use gpui::{
    AnyElement, Bounds, Canvas, Context, Div, FontWeight, Pixels, Point, SharedString, canvas, div,
    linear_color_stop, linear_gradient, point, prelude::*, px, rgb,
};
use resonate_core::{AppliedGain, Decibels, Frames, SampleRate};
use resonate_engine::{
    BitRate, BoxLayout, Codec, Container, Faststart, MediaInfo, OutputMode, OutputStatus,
    PacketSpan, Packing, PlayerState, ReplayGain, ReplayGainMode, StreamDigest, TagSet,
    TopLevelBox, WINDOW, Words,
};

use crate::{
    Selection, format,
    icons::Icon,
    theme,
    views::{
        kit::{self, EndsInAnEllipsis, Tone},
        plot,
        root::RootView,
        scrollbar::Scrollbars,
        spoken::Spoken as _,
        transport::{Heard, copied_on_a_right_click},
    },
    words,
};

const GRAPH_COLUMNS: usize = 240;
const GRAPH_HEIGHT: f32 = 160.0;
const GRAPH_LINE: f32 = 1.75;
const CARD_WIDTH: f32 = 232.0;
const ATOM_KIND_WIDTH: f32 = 56.0;
const ATOM_AT_WIDTH: f32 = 112.0;
const NOTHING_DECLARED: &str = "The file declares none.";

const CODES: [&str; 9] = [
    "ISRC",
    "MusicBrainz track",
    "MusicBrainz album",
    "MusicBrainz artist",
    "MusicBrainz album artist",
    "MusicBrainz release group",
    "MusicBrainz release track",
    "Barcode",
    "Catalog number",
];

const GRAPH_HINT: &str = "Draw the bitrate over the decoded span as a line, one point per window";

const GRAPH_OPEN_HINT: &str = "Hide the bitrate graph";

impl RootView {
    fn inspected(&self, state: &PlayerState, digest: &StreamDigest, cx: &mut Context<Self>) -> Div {
        let playing = self.playing(state, Some(digest), cx);

        div()
            .flex()
            .flex_col()
            .relative()
            .gap_1()
            .child(kit::measures_its_width(self.inspected_room.clone()))
            .child(kit::linked_title(copied_on_a_right_click(
                self.opens(
                    "inspected-title",
                    kit::cut_to_fit(
                        playing.title.clone(),
                        self.inspected_room.get(),
                        theme::ui(FontWeight::SEMIBOLD),
                        px(theme::text_xl()),
                        cx,
                    ),
                    words::browser::open_album_hint(),
                    playing.cover.album.map(Selection::Album),
                    cx,
                ),
                playing.title.clone(),
            )))
            .child(self.by_line(
                "inspected-artist",
                "inspected-album",
                &playing,
                self.inspected_room.get(),
                cx,
            ))
    }

    pub(crate) fn inspector(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.player.read(cx);
        let Some(digest) = model.digest() else {
            return kit::empty(
                Icon::Inspector,
                "Nothing playing",
                Some("Play a track and the inspector follows it from the decoder to the output."),
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

        let mut column = div()
            .flex()
            .flex_col()
            .w_full()
            .max_w(px(theme::inspector_column()))
            .mx_auto()
            .gap_4()
            .child(div().pb_1().child(heading))
            .child(signal_path(&digest, output, sink))
            .child(source(&digest.info))
            .child(sink_card(output))
            .child(bitrate(self, &digest, graphing, cx));

        if graphing && digest.profile.is_some() {
            let series = self
                .player
                .update(cx, |model, _| model.condensed(GRAPH_COLUMNS));
            column = column.child(graph(&digest, &series));
        }

        column = column
            .child(replay_gain(
                digest.replay_gain_mode,
                digest.replay_gain,
                &digest.info.tags.heard_gain(),
            ))
            .child(heard_card(heard.as_ref()));
        if let Some(layout) = digest.layout.as_ref() {
            column = column.child(container(layout));
        }

        let carried = self.player.update(cx, |model, _| model.carried_lines());
        let pane = div()
            .id("inspector")
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .p_6()
            .overflow_y_scroll()
            .track_scroll(&self.inspector_scroll)
            .child(column.child(tags(&digest.info.tags, carried)));

        Scrollbars::of(cx)
            .around("inspector-scrollbar", self.inspector_scroll.clone(), pane)
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
    let (mode_label, mode_colour) = output.map_or(("Closed", theme::faint()), |output| {
        format::mode(output.mode)
    });
    let (processing, done_to) = output.map_or_else(
        || ("No stream".to_owned(), None),
        |output| match output.mode {
            OutputMode::BitPerfect => ("Untouched".to_owned(), None),
            OutputMode::Repacked => ("Repacked".to_owned(), None),
            OutputMode::Dithered => (
                "Dithered".to_owned(),
                Some(format!("To {}", format::depth(output.negotiated.format))),
            ),
            OutputMode::Converted => (
                if output.negotiated.rate == info.spec.rate {
                    "Converted".to_owned()
                } else {
                    "Resampled".to_owned()
                },
                Some(match graph_converting(output) {
                    Some(graph) => format!("To {graph} kHz by the graph"),
                    None => format!("To {}", format::quality(output.negotiated)),
                }),
            ),
        },
    );
    let gain = match (digest.replay_gain.gain, digest.replay_gain.is_capped()) {
        (None, false) => "No gain".to_owned(),
        _ => format!("Gain {}", format::applied(digest.replay_gain)),
    };

    kit::card()
        .flex_row()
        .flex_wrap()
        .gap_x_8()
        .gap_y_4()
        .child(stage(
            "Source",
            codec.as_str().to_owned(),
            source_colour,
            vec![
                format::quality(info.spec),
                format!("{} · {container}", info.spec.channels),
            ],
        ))
        .child(stage(
            "Processing",
            processing,
            theme::text(),
            done_to
                .into_iter()
                .chain([
                    gain,
                    format!(
                        "ReplayGain {}",
                        mode_text(digest.replay_gain_mode).to_lowercase()
                    ),
                ])
                .collect(),
        ))
        .child(stage(
            "Output",
            mode_label.to_owned(),
            mode_colour,
            vec![
                output.map_or_else(
                    || "Stream closed".to_owned(),
                    |output| format::quality(output.negotiated),
                ),
                sink.unwrap_or_else(|| "No sink".to_owned()),
            ],
        ))
}

fn stage(label: &'static str, figure: String, colour: u32, lines: Vec<String>) -> Div {
    let mut column = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(theme::stage_width()))
        .gap_0p5()
        .child(
            div()
                .id(label)
                .headed(3, label)
                .pb_1()
                .text_size(px(theme::text_sm()))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(theme::muted()))
                .child(label),
        )
        .child(
            div()
                .font(theme::ui(FontWeight::SEMIBOLD))
                .text_size(px(theme::text_lg()))
                .line_height(px(theme::text_lg() * 1.3))
                .text_color(rgb(colour))
                .truncate()
                .ends_in_an_ellipsis()
                .child(SharedString::from(figure)),
        );
    for line in lines {
        column = column.child(
            div()
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::muted()))
                .truncate()
                .ends_in_an_ellipsis()
                .child(SharedString::from(line)),
        );
    }
    column
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
            ("Sample rate", Some(reading)),
            (
                "Decoded depth",
                Some(format::depth(info.spec.format).to_owned()),
            ),
            ("Coded depth", coded),
            ("Channels", Some(info.spec.channels.to_string())),
            (
                "Duration",
                info.duration.map(|frames| format::clock(frames, rate)),
            ),
            (
                "Encoder delay",
                (info.encoder_delay > 0 || info.encoder_padding > 0)
                    .then(|| format!("{} / {} frames", info.encoder_delay, info.encoder_padding)),
            ),
            ("Seekable", Some(yes_no(info.is_seekable))),
        ],
    )
}

fn heard_card(heard: Option<&Heard>) -> Div {
    let now = SystemTime::now();
    let Some(heard) = heard else {
        return fields(
            summary("Heard"),
            vec![("Plays", Some("No library row".to_owned()))],
        );
    };

    fields(
        summary("Heard"),
        vec![
            (
                "Plays",
                Some(match heard.plays {
                    0 => "Never".to_owned(),
                    plays => format::counted(plays as usize, "time", "times"),
                }),
            ),
            (
                "Last played",
                heard.played.map(|played| format::since(played, now)),
            ),
            ("Added", Some(format::since(heard.added, now))),
        ],
    )
}

fn graph_converting(output: OutputStatus) -> Option<String> {
    output
        .graph_rate
        .filter(|_| output.converted_by_the_graph())
        .map(SampleRate::kilohertz)
}

fn sink_card(output: Option<OutputStatus>) -> Div {
    let Some(output) = output else {
        return fields(
            summary("Output"),
            vec![("Stream", Some("Closed".to_owned()))],
        );
    };
    let (label, colour) = format::mode(output.mode);
    let latency = output
        .latency
        .to_duration(output.negotiated.rate)
        .as_millis();

    fields(
        summary_beside("Output", kit::badge(label, colour)),
        vec![
            ("Negotiated", Some(format::quality(output.negotiated))),
            (
                "Graph runs at",
                graph_converting(output).map(|graph| format!("{graph} kHz")),
            ),
            (
                "On the wire",
                output
                    .words
                    .filter(|words| *words != Words::Whole)
                    .map(|words| words.spelled(output.negotiated.format)),
            ),
            ("Latency", Some(format!("{latency} ms"))),
            ("Underruns", Some(output.underruns.to_string())),
            (
                "Clamped samples",
                (output.overs > 0).then(|| output.overs.to_string()),
            ),
            (
                "Went without",
                (output.went_without > Frames::ZERO).then(|| {
                    let lost = output.went_without.to_duration(output.negotiated.rate);
                    format!("{} ms", lost.as_millis())
                }),
            ),
        ],
    )
}

fn bitrate(
    root: &RootView,
    digest: &StreamDigest,
    graphing: bool,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let rate = digest.info.spec.rate;
    let Some(profile) = digest.profile.as_ref() else {
        return fields(
            summary("Bitrate"),
            vec![("Profile", Some("Decoding…".to_owned()))],
        )
        .into_any_element();
    };

    fields(
        summary("Bitrate"),
        vec![
            ("Overall", Some(kbps(profile.overall))),
            ("Now", Some(instant(digest.packet, rate))),
            ("Variability", Some(profile.variability.to_string())),
            ("Mean", Some(kbps(profile.mean))),
            ("Deviation", Some(kbps(profile.deviation))),
            ("Range", Some(kbps_between(profile.min, profile.max))),
            ("Packets", Some(profile.packets.to_string())),
            (
                "Profiled",
                Some(format!(
                    "{} of {}",
                    format::clock(digest.decoded.saturating_sub(digest.profiled_from), rate),
                    format::clock(digest.decoded, rate)
                )),
            ),
        ],
    )
    .child(div().mt_1().child(root.in_the_pane_ring(
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
        ),
        |this, _, cx| this.toggle_bitrate_graph(cx),
        cx,
    )))
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
            ("Mode", declared.then(|| mode_text(mode).to_owned())),
            ("Track gain", gain.track_gain.map(decibels)),
            ("Track peak", gain.track_peak.map(peak)),
            ("Album gain", gain.album_gain.map(decibels)),
            ("Album peak", gain.album_peak.map(peak)),
            ("Applied", declared.then(|| format::applied(applied))),
            ("Headroom", declared.then(|| format::headroom(applied))),
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
                .id("bitrate-graph")
                .pictured_as("Bitrate over time", || {
                    format!(
                        "{} to {} over {WINDOW:.0}s windows",
                        kbps(BitRate::new(floor).unwrap_or_default()),
                        kbps(BitRate::new(top).unwrap_or_default())
                    )
                })
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
                    "{} over {WINDOW:.0} s windows",
                    kbps_between(
                        BitRate::new(floor).unwrap_or_default(),
                        BitRate::new(top).unwrap_or_default()
                    )
                )))
                .child(kit::figure(format::clock(digest.decoded, rate))),
        )
}

fn traced(shares: Vec<f32>) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let plotted = plotted(&shares, bounds);
            plot::wash(
                window,
                &plotted,
                bounds.bottom(),
                linear_gradient(
                    180.0,
                    linear_color_stop(theme::tinted(theme::accent(), 0x4d), 0.0),
                    linear_color_stop(theme::tinted(theme::accent(), 0x00), 1.0),
                ),
            );
            plot::stroke(window, &plotted, GRAPH_LINE, rgb(theme::accent()));
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
        || "No moov and mdat pair".to_owned(),
        |state| match state {
            Faststart::Ready => "Plays before the whole file arrives".to_owned(),
            Faststart::Trailing => "The index is at the end".to_owned(),
        },
    );

    let mut panel = fields(card("Container layout"), vec![("Faststart", Some(verdict))]);
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
        .py_1()
        .text_size(px(theme::text_sm()))
        .child(
            kit::code(entry.kind.to_string())
                .w(px(ATOM_KIND_WIDTH))
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::text())),
        )
        .child(
            kit::figure(format!("at {}", entry.at))
                .w(px(ATOM_AT_WIDTH))
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::faint())),
        )
        .child(
            kit::figure(format::bytes(entry.bytes))
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::muted())),
        )
}

fn tags<'a>(set: &'a TagSet, carried: Option<usize>) -> Div {
    let numbered = |value: Option<u32>, total: Option<u32>| match (value, total) {
        (Some(value), Some(total)) => Some(Cow::Owned(format!("{value} of {total}"))),
        (Some(value), None) => Some(Cow::Owned(value.to_string())),
        _ => None,
    };
    let borrowed = |value: &'a Option<String>| value.as_deref().map(Cow::Borrowed);

    let named = [
        ("Title", borrowed(&set.title)),
        ("Artist", borrowed(&set.artist)),
        ("Album", borrowed(&set.album)),
        ("Album artist", borrowed(&set.album_artist)),
        ("Track", numbered(set.track_number, set.track_total)),
        ("Disc", numbered(set.disc_number, set.disc_total)),
        ("Date", borrowed(&set.date)),
        ("Genre", borrowed(&set.genre)),
        (
            "Compilation",
            set.compilation.then_some(Cow::Borrowed("Yes")),
        ),
        ("Grouping", borrowed(&set.grouping)),
        ("Collection", borrowed(&set.collection)),
        ("Edition", borrowed(&set.edition)),
        ("Composer", borrowed(&set.credits.composer)),
        ("Conductor", borrowed(&set.credits.conductor)),
        ("Lyricist", borrowed(&set.credits.lyricist)),
        ("Performer", borrowed(&set.credits.performer)),
        ("Remixer", borrowed(&set.credits.remixer)),
        ("Producer", borrowed(&set.credits.producer)),
        ("Engineer", borrowed(&set.credits.engineer)),
        ("Label", borrowed(&set.label)),
        ("ISRC", borrowed(&set.isrc)),
        (
            "Tempo",
            set.beats_per_minute
                .map(|beats| Cow::Owned(format!("{beats} bpm"))),
        ),
        ("Copyright", borrowed(&set.copyright)),
        ("Encoder", borrowed(&set.encoder)),
        ("Comment", borrowed(&set.comment)),
        ("MusicBrainz track", borrowed(&set.musicbrainz_track_id)),
        ("MusicBrainz album", borrowed(&set.musicbrainz_album_id)),
        ("MusicBrainz artist", borrowed(&set.musicbrainz_artist_id)),
        (
            "MusicBrainz album artist",
            borrowed(&set.musicbrainz_album_artist_id),
        ),
        (
            "MusicBrainz release group",
            borrowed(&set.musicbrainz_release_group_id),
        ),
        (
            "MusicBrainz release track",
            borrowed(&set.musicbrainz_release_track_id),
        ),
        ("Barcode", borrowed(&set.barcode)),
        ("Catalog number", borrowed(&set.catalog_number)),
        (
            "Lyrics",
            carried.map(|lines| Cow::Owned(format!("{lines} lines"))),
        ),
    ];

    fields(card("Tags"), named.into())
}

fn summary(title: &'static str) -> Div {
    card(title)
}

fn summary_beside(title: &'static str, beside: Div) -> Div {
    kit::card().gap_1p5().min_w(px(0.0)).child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .pb_1()
            .child(kit::card_title(title))
            .child(beside),
    )
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

    let last = told.len() - 1;
    told.into_iter()
        .enumerate()
        .fold(panel, |panel, (at, (name, value))| {
            panel.child(property(name, value, at < last))
        })
}

fn property(name: &'static str, value: SharedString, ruled: bool) -> Div {
    let value = if is_a_code(name) {
        kit::code(value)
    } else {
        kit::figure(value)
    };

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .py_1p5()
        .when(ruled, |row| {
            row.border_b_1().border_color(rgb(theme::soft_edge()))
        })
        .child(
            div()
                .flex_none()
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::muted()))
                .whitespace_nowrap()
                .child(name),
        )
        .child(
            value
                .min_w(px(0.0))
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::text()))
                .truncate()
                .ends_in_an_ellipsis(),
        )
}

fn is_a_code(name: &str) -> bool {
    CODES.contains(&name)
}

fn instant(packet: Option<PacketSpan>, rate: SampleRate) -> String {
    packet
        .and_then(|packet| packet.rate(rate))
        .map_or_else(unknown, kbps)
}

const fn mode_text(mode: ReplayGainMode) -> &'static str {
    match mode {
        ReplayGainMode::Off => "Off",
        ReplayGainMode::Track => "Track",
        ReplayGainMode::Album => "Album",
    }
}

fn kbps(rate: BitRate) -> String {
    format!("{:.0} kbps", rate.kilobits())
}

fn kbps_between(low: BitRate, high: BitRate) -> String {
    format!("{:.0} – {:.0} kbps", low.kilobits(), high.kilobits())
}

fn decibels(gain: Decibels) -> String {
    format!("{:+.2} dB", gain.get())
}

fn peak(value: f32) -> String {
    format!("{value:.4}")
}

fn yes_no(value: bool) -> String {
    if value { "Yes" } else { "No" }.to_owned()
}

fn unknown() -> String {
    "Unknown".to_owned()
}
