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
const STAGES: usize = 3;
const PART_MARGIN: f32 = 176.0;
const A_PART_IS_NAMED_BESIDE_ITS_FIRST_ROW: f32 = 18.0;
const ATOM_KIND_WIDTH: f32 = 56.0;
const ATOM_AT_WIDTH: f32 = 112.0;

fn codes() -> [&'static str; 9] {
    [
        words::inspector::isrc(),
        words::inspector::musicbrainz_track(),
        words::inspector::musicbrainz_album(),
        words::inspector::musicbrainz_artist(),
        words::inspector::musicbrainz_album_artist(),
        words::inspector::musicbrainz_release_group(),
        words::inspector::musicbrainz_release_track(),
        words::inspector::barcode(),
        words::inspector::catalog_number(),
    ]
}

impl RootView {
    fn inspected(&self, state: &PlayerState, digest: &StreamDigest, cx: &mut Context<Self>) -> Div {
        let playing = self.playing(state, Some(digest), cx);

        let named = div()
            .flex()
            .flex_col()
            .relative()
            .flex_1()
            .min_w(px(theme::heading_name()))
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
            ));

        kit::heading().child(kit::heading_row().child(named))
    }

    pub(crate) fn inspector(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.player.read(cx);
        let Some(digest) = model.digest() else {
            return kit::empty(
                Icon::Inspector,
                words::inspector::nothing_playing(),
                Some(words::inspector::nothing_playing_about()),
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
            .child(signal_path(&digest, output, sink))
            .child(source(&digest.info).mt_3().border_t_0())
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

        kit::pane()
            .child(heading)
            .child(Scrollbars::of(cx).around(
                "inspector-scrollbar",
                self.inspector_scroll.clone(),
                pane,
            ))
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
    let (mode_label, mode_colour) = output
        .map_or((words::inspector::closed(), theme::faint()), |output| {
            format::mode(output.mode)
        });
    let (processing, done_to) = output.map_or_else(
        || (words::inspector::no_stream().to_owned(), None),
        |output| match output.mode {
            OutputMode::BitPerfect => (words::inspector::untouched().to_owned(), None),
            OutputMode::Repacked => (words::inspector::repacked().to_owned(), None),
            OutputMode::Dithered => (
                words::inspector::dithered().to_owned(),
                Some(words::inspector::to(format::depth(
                    output.negotiated.format,
                ))),
            ),
            OutputMode::Converted => (
                if output.negotiated.rate == info.spec.rate {
                    words::inspector::converted().to_owned()
                } else {
                    words::inspector::resampled().to_owned()
                },
                Some(match graph_converting(output) {
                    Some(graph) => words::inspector::to_by_the_graph(graph),
                    None => words::inspector::to(format::quality(output.negotiated)),
                }),
            ),
        },
    );
    let gain = match (digest.replay_gain.gain, digest.replay_gain.is_capped()) {
        (None, false) => words::inspector::no_gain().to_owned(),
        _ => words::inspector::gain(format::applied(digest.replay_gain)),
    };

    kit::ledger(
        STAGES,
        vec![
            stage(
                words::inspector::source(),
                codec.as_str().to_owned(),
                source_colour,
                vec![
                    format::quality(info.spec),
                    format!("{} · {container}", info.spec.channels),
                ],
            )
            .into_any_element(),
            stage(
                words::inspector::processing(),
                processing,
                theme::text(),
                done_to
                    .into_iter()
                    .chain([gain, replay_gain_said(digest.replay_gain_mode).to_owned()])
                    .collect(),
            )
            .into_any_element(),
            stage(
                words::inspector::output(),
                mode_label.to_owned(),
                mode_colour,
                vec![
                    output.map_or_else(
                        || words::inspector::stream_closed().to_owned(),
                        |output| format::quality(output.negotiated),
                    ),
                    sink.unwrap_or_else(|| words::inspector::no_sink().to_owned()),
                ],
            )
            .into_any_element(),
        ],
    )
}

fn stage(label: &'static str, figure: String, colour: u32, lines: Vec<String>) -> Div {
    let mut column = div()
        .flex()
        .flex_col()
        .min_w(px(0.0))
        .gap_0p5()
        .child(
            div()
                .id(label)
                .headed(3, label)
                .pb_1()
                .text_size(px(theme::text_xs()))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme::faint()))
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
    let coded = info
        .bits_per_coded_sample
        .map(words::inspector::coded_depth_bits);
    let reading = match info.packing {
        Packing::DopMarked(dsd) => words::inspector::dop_carried(dsd.to_string(), rate.to_string()),
        Packing::Samples => rate.to_string(),
    };

    fields(
        summary(words::inspector::source()),
        vec![
            (words::inspector::sample_rate(), Some(reading)),
            (
                words::inspector::decoded_depth(),
                Some(format::depth(info.spec.format).to_owned()),
            ),
            (words::inspector::coded_depth(), coded),
            (
                words::inspector::channels(),
                Some(info.spec.channels.to_string()),
            ),
            (
                words::inspector::duration(),
                info.duration.map(|frames| format::clock(frames, rate)),
            ),
            (
                words::inspector::encoder_delay(),
                (info.encoder_delay > 0 || info.encoder_padding > 0).then(|| {
                    words::inspector::encoder_delay_frames(
                        info.encoder_delay.to_string(),
                        info.encoder_padding.to_string(),
                    )
                }),
            ),
            (words::inspector::seekable(), Some(yes_no(info.is_seekable))),
        ],
    )
}

fn heard_card(heard: Option<&Heard>) -> Div {
    let now = SystemTime::now();
    let Some(heard) = heard else {
        return fields(
            summary(words::inspector::heard()),
            vec![(
                words::inspector::plays(),
                Some(words::inspector::no_library_row().to_owned()),
            )],
        );
    };

    fields(
        summary(words::inspector::heard()),
        vec![
            (
                words::inspector::plays(),
                Some(match heard.plays {
                    0 => words::inspector::never().to_owned(),
                    plays => words::inspector::times(plays),
                }),
            ),
            (
                words::inspector::last_played(),
                heard.played.map(|played| format::since(played, now)),
            ),
            (
                words::inspector::added(),
                Some(format::since(heard.added, now)),
            ),
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
            summary(words::inspector::output()),
            vec![(
                words::inspector::stream(),
                Some(words::inspector::closed().to_owned()),
            )],
        );
    };
    let (label, colour) = format::mode(output.mode);
    let latency = output
        .latency
        .to_duration(output.negotiated.rate)
        .as_millis();

    fields(
        summary_beside(words::inspector::output(), kit::badge(label, colour)),
        vec![
            (
                words::inspector::negotiated(),
                Some(format::quality(output.negotiated)),
            ),
            (
                words::inspector::graph_runs_at(),
                graph_converting(output).map(words::inspector::kilohertz),
            ),
            (
                words::inspector::on_the_wire(),
                output
                    .words
                    .filter(|words| *words != Words::Whole)
                    .map(|words| words.spelled(output.negotiated.format)),
            ),
            (
                words::inspector::latency(),
                Some(words::inspector::milliseconds(latency.to_string())),
            ),
            (
                words::inspector::underruns(),
                Some(output.underruns.to_string()),
            ),
            (
                words::inspector::clamped_samples(),
                (output.overs > 0).then(|| output.overs.to_string()),
            ),
            (
                words::inspector::went_without(),
                (output.went_without > Frames::ZERO).then(|| {
                    let lost = output.went_without.to_duration(output.negotiated.rate);
                    words::inspector::milliseconds(lost.as_millis().to_string())
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
            summary(words::inspector::bitrate()),
            vec![(
                words::inspector::profile(),
                Some(words::inspector::decoding().to_owned()),
            )],
        )
        .into_any_element();
    };

    fields(
        summary(words::inspector::bitrate()),
        vec![
            (words::inspector::overall(), Some(kbps(profile.overall))),
            (words::inspector::now(), Some(instant(digest.packet, rate))),
            (
                words::inspector::variability(),
                Some(profile.variability.to_string()),
            ),
            (words::inspector::mean(), Some(kbps(profile.mean))),
            (words::inspector::deviation(), Some(kbps(profile.deviation))),
            (
                words::inspector::range(),
                Some(kbps_between(profile.min, profile.max)),
            ),
            (
                words::inspector::packets(),
                Some(profile.packets.to_string()),
            ),
            (
                words::inspector::profiled(),
                Some(words::inspector::profiled_of(
                    format::clock(digest.decoded.saturating_sub(digest.profiled_from), rate),
                    format::clock(digest.decoded, rate),
                )),
            ),
        ],
    )
    .child(div().mt_2().mb_1().child(root.in_the_pane_ring(
        kit::button(
            "bitrate-graph",
            Some(Icon::Inspector),
            if graphing {
                words::inspector::hide_graph()
            } else {
                words::inspector::show_graph()
            },
            if graphing {
                words::inspector::graph_open_hint()
            } else {
                words::inspector::graph_hint()
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
        summary(words::inspector::replay_gain()),
        vec![
            (
                words::inspector::mode(),
                declared.then(|| mode_text(mode).to_owned()),
            ),
            (
                words::inspector::track_gain(),
                gain.track_gain.map(decibels),
            ),
            (words::inspector::track_peak(), gain.track_peak.map(peak)),
            (
                words::inspector::album_gain(),
                gain.album_gain.map(decibels),
            ),
            (words::inspector::album_peak(), gain.album_peak.map(peak)),
            (
                words::inspector::applied(),
                declared.then(|| format::applied(applied)),
            ),
            (
                words::inspector::headroom(),
                declared.then(|| format::headroom(applied)),
            ),
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

    summary(words::inspector::bitrate_over_time())
        .child(
            div()
                .id("bitrate-graph")
                .pictured_as(words::inspector::bitrate_over_time(), || {
                    words::inspector::bitrate_over_time_said(
                        kbps(BitRate::new(floor).unwrap_or_default()),
                        kbps(BitRate::new(top).unwrap_or_default()),
                        format!("{WINDOW:.0}"),
                    )
                })
                .h(px(GRAPH_HEIGHT))
                .w_full()
                .my_2()
                .child(traced(shares)),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .child(kit::figure(format::clock(digest.profiled_from, rate)))
                .child(kit::figure(words::inspector::bitrate_over_windows(
                    kbps_between(
                        BitRate::new(floor).unwrap_or_default(),
                        BitRate::new(top).unwrap_or_default(),
                    ),
                    format!("{WINDOW:.0}"),
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
        || words::inspector::no_moov_and_mdat().to_owned(),
        |state| match state {
            Faststart::Ready => words::inspector::faststart_ready().to_owned(),
            Faststart::Trailing => words::inspector::faststart_trailing().to_owned(),
        },
    );

    let mut panel = fields(
        summary(words::inspector::container_layout()),
        vec![(words::inspector::faststart(), Some(verdict))],
    );
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
            kit::figure(words::inspector::at(entry.at.to_string()))
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
    use words::inspector as said;

    let numbered = |value: Option<u32>, total: Option<u32>| match (value, total) {
        (Some(value), Some(total)) => Some(Cow::Owned(words::inspector::numbered(total, value))),
        (Some(value), None) => Some(Cow::Owned(value.to_string())),
        _ => None,
    };
    let borrowed = |value: &'a Option<String>| value.as_deref().map(Cow::Borrowed);

    let named = [
        (said::title(), borrowed(&set.title)),
        (said::artist(), borrowed(&set.artist)),
        (said::album(), borrowed(&set.album)),
        (said::album_artist(), borrowed(&set.album_artist)),
        (said::track(), numbered(set.track_number, set.track_total)),
        (said::disc(), numbered(set.disc_number, set.disc_total)),
        (said::date(), borrowed(&set.date)),
        (said::genre(), borrowed(&set.genre)),
        (
            said::compilation(),
            set.compilation.then_some(Cow::Borrowed(said::yes())),
        ),
        (said::grouping(), borrowed(&set.grouping)),
        (said::collection(), borrowed(&set.collection)),
        (said::edition(), borrowed(&set.edition)),
        (said::composer(), borrowed(&set.credits.composer)),
        (said::conductor(), borrowed(&set.credits.conductor)),
        (said::lyricist(), borrowed(&set.credits.lyricist)),
        (said::performer(), borrowed(&set.credits.performer)),
        (said::remixer(), borrowed(&set.credits.remixer)),
        (said::producer(), borrowed(&set.credits.producer)),
        (said::engineer(), borrowed(&set.credits.engineer)),
        (said::label(), borrowed(&set.label)),
        (said::isrc(), borrowed(&set.isrc)),
        (
            said::tempo(),
            set.beats_per_minute
                .map(|beats| Cow::Owned(said::beats_per_minute(beats))),
        ),
        (said::copyright(), borrowed(&set.copyright)),
        (said::encoder(), borrowed(&set.encoder)),
        (said::comment(), borrowed(&set.comment)),
        (
            said::musicbrainz_track(),
            borrowed(&set.musicbrainz_track_id),
        ),
        (
            said::musicbrainz_album(),
            borrowed(&set.musicbrainz_album_id),
        ),
        (
            said::musicbrainz_artist(),
            borrowed(&set.musicbrainz_artist_id),
        ),
        (
            said::musicbrainz_album_artist(),
            borrowed(&set.musicbrainz_album_artist_id),
        ),
        (
            said::musicbrainz_release_group(),
            borrowed(&set.musicbrainz_release_group_id),
        ),
        (
            said::musicbrainz_release_track(),
            borrowed(&set.musicbrainz_release_track_id),
        ),
        (said::barcode(), borrowed(&set.barcode)),
        (said::catalog_number(), borrowed(&set.catalog_number)),
        (
            said::lyrics(),
            carried.map(|lines| Cow::Owned(said::lyric_lines(lines))),
        ),
    ];

    fields(summary(said::tags()), named.into())
}

fn summary(title: &'static str) -> Div {
    part().child(margin().child(kit::card_title(title)))
}

fn summary_beside(title: &'static str, beside: Div) -> Div {
    part().child(margin().child(kit::card_title(title)).child(beside))
}

fn part() -> Div {
    div()
        .relative()
        .flex()
        .flex_col()
        .min_w(px(0.0))
        .pl(px(PART_MARGIN))
        .py_3()
        .border_t_1()
        .border_color(rgb(theme::border()))
}

fn margin() -> Div {
    div()
        .absolute()
        .left_0()
        .top(px(A_PART_IS_NAMED_BESIDE_ITS_FIRST_ROW))
        .w(px(PART_MARGIN))
        .flex()
        .flex_col()
        .items_start()
        .gap_1()
        .pr_4()
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
                .child(words::inspector::nothing_declared()),
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
    codes().contains(&name)
}

fn instant(packet: Option<PacketSpan>, rate: SampleRate) -> String {
    packet
        .and_then(|packet| packet.rate(rate))
        .map_or_else(unknown, kbps)
}

fn mode_text(mode: ReplayGainMode) -> &'static str {
    match mode {
        ReplayGainMode::Off => words::inspector::off(),
        ReplayGainMode::Track => words::inspector::track(),
        ReplayGainMode::Album => words::inspector::album(),
    }
}

fn replay_gain_said(mode: ReplayGainMode) -> &'static str {
    match mode {
        ReplayGainMode::Off => words::inspector::replay_gain_off(),
        ReplayGainMode::Track => words::inspector::replay_gain_track(),
        ReplayGainMode::Album => words::inspector::replay_gain_album(),
    }
}

fn kbps(rate: BitRate) -> String {
    words::inspector::kbps(format!("{:.0}", rate.kilobits()))
}

fn kbps_between(low: BitRate, high: BitRate) -> String {
    words::inspector::kbps_between(
        format!("{:.0}", high.kilobits()),
        format!("{:.0}", low.kilobits()),
    )
}

fn decibels(gain: Decibels) -> String {
    words::inspector::decibels(format!("{:+.2}", gain.get()))
}

fn peak(value: f32) -> String {
    format!("{value:.4}")
}

fn yes_no(value: bool) -> String {
    if value {
        words::inspector::yes()
    } else {
        words::inspector::no()
    }
    .to_owned()
}

fn unknown() -> String {
    words::inspector::unknown().to_owned()
}
