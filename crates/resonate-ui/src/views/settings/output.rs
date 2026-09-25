use std::time::Duration;

use gpui::{Context, Div, SharedString, Stateful, div, prelude::*, px, rgb};
use resonate_core::{SampleFormat, StreamSpec};
use resonate_engine::{
    BluetoothWake, Command, HardwareVolume, NodeName, Plugged, SinkId, SinkInfo,
};

use crate::{
    Setting, format, theme,
    views::{
        hint::Names,
        kit,
        root::RootView,
        settings::{Choice, note, switch_row},
    },
};

const NO_SINKS: &str = "No sinks are present in the graph.";

const BLUETOOTH_NOTE: &str = "Experimental, and only for a device PipeWire names as Bluetooth. \
                              It spends the headphones' battery to keep their radio up through \
                              a pause.";

const DEVICE_NOTE: &str = "A device chosen here is played through whatever the desktop routes \
                           other sound to; following the system default moves with the \
                           desktop's own choice.";

const AWAY: &str = "The device named in the settings file is not in the graph, so the system \
                    default is playing until it appears.";

const DOP_WARNING: &str = "A DAC that does not decode DoP plays it as full-scale white noise. \
                           Turn it on only for hardware you know takes it.";

const UNADVERTISED: &str = "Advertises no format of its own";

const ROUTED: &str = "Whatever PipeWire routes to at the time";

const ROUTED_NOW: &str = "Whatever PipeWire routes to, which is";

const UNPLUGGED: &str = "UNPLUGGED";

const DEFAULT: &str = "DEFAULT";

const IN_USE: &str = "IN USE";

const CONVERTS: &str = "CONVERTS";

const FOLLOW: &str = "Follow the system default";

const ON_THE_HARDWARE: &str = "Volume on the hardware";

const IN_SOFTWARE: &str = "Volume in software";

const OR: &str = ", ";

fn known_as(sink: &SinkInfo) -> String {
    format!("The graph knows it as {}", sink.name)
}

fn routes_to(sinks: &[SinkInfo]) -> String {
    sinks.iter().find(|sink| sink.is_default).map_or_else(
        || ROUTED.to_owned(),
        |sink| format!("{ROUTED_NOW} {} now", sink.description),
    )
}

fn routed(sink: &SinkInfo) -> Vec<String> {
    sink.port
        .as_ref()
        .map(|port| port.description.clone())
        .into_iter()
        .chain(sink.profile.clone())
        .chain(volume_of(sink))
        .collect()
}

fn takes(sink: &SinkInfo) -> Vec<String> {
    depths_of(sink).into_iter().chain(rates_of(sink)).collect()
}

fn unplugged(sink: &SinkInfo) -> bool {
    sink.port
        .as_ref()
        .is_some_and(|port| port.plugged == Plugged::No)
}

fn volume_of(sink: &SinkInfo) -> Option<String> {
    match sink.port.as_ref()?.hardware_volume {
        HardwareVolume::Unsaid => None,
        HardwareVolume::Yes => Some(ON_THE_HARDWARE.to_owned()),
        HardwareVolume::No => Some(IN_SOFTWARE.to_owned()),
    }
}

fn depths_of(sink: &SinkInfo) -> Option<String> {
    let mut offered: Vec<SampleFormat> = Vec::new();
    for entry in &sink.formats {
        if !offered.contains(&entry.format) {
            offered.push(entry.format);
        }
    }
    let widest: Vec<&str> = offered.iter().copied().map(format::depth).collect();

    (!widest.is_empty()).then(|| widest.join(OR))
}

fn rates_of(sink: &SinkInfo) -> Option<String> {
    let offered: Vec<String> = sink
        .allowed_rates
        .iter()
        .copied()
        .map(format::kilohertz)
        .collect();

    (!offered.is_empty()).then(|| format!("{} kHz", offered.join(OR)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceRate {
    MatchTheFile,
    FollowTheGraph,
}

impl SourceRate {
    const fn of(prefer_bit_perfect: bool) -> Self {
        if prefer_bit_perfect {
            Self::MatchTheFile
        } else {
            Self::FollowTheGraph
        }
    }

    const fn matches_the_file(self) -> bool {
        matches!(self, Self::MatchTheFile)
    }
}

impl Choice for SourceRate {
    const ALL: &'static [Self] = &[Self::MatchTheFile, Self::FollowTheGraph];

    fn label(self) -> &'static str {
        match self {
            Self::MatchTheFile => "Match the file",
            Self::FollowTheGraph => "Follow the graph",
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::MatchTheFile => {
                "The stream opens at the file's own rate wherever the device takes it, so a \
                 44.1 kHz album reaches a DAC untouched rather than resampled to 48 kHz."
            }
            Self::FollowTheGraph => {
                "The stream opens at the rate PipeWire is already running at, and a file at \
                 any other rate is resampled here, with the filter chosen under Processing."
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GraphRate {
    AskToSwitch,
    LeaveItToTheDaemon,
}

impl GraphRate {
    const fn of(force_graph_rate: bool) -> Self {
        if force_graph_rate {
            Self::AskToSwitch
        } else {
            Self::LeaveItToTheDaemon
        }
    }

    const fn asks_to_switch(self) -> bool {
        matches!(self, Self::AskToSwitch)
    }
}

impl Choice for GraphRate {
    const ALL: &'static [Self] = &[Self::AskToSwitch, Self::LeaveItToTheDaemon];

    fn label(self) -> &'static str {
        match self {
            Self::AskToSwitch => "Ask the graph to switch",
            Self::LeaveItToTheDaemon => "Leave it to the daemon",
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::AskToSwitch => {
                "The stream asks PipeWire to run the whole graph at its rate while it plays, so \
                 nothing between here and the device resamples it. Other sounds playing at the \
                 same time are resampled to match instead."
            }
            Self::LeaveItToTheDaemon => {
                "PipeWire keeps the rate it chose, and anything it cannot take as it stands is \
                 converted, here or by the daemon."
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BufferDepth {
    Tight,
    Short,
    Standard,
    Deep,
}

impl BufferDepth {
    const fn held(self) -> Duration {
        Duration::from_millis(match self {
            Self::Tight => 100,
            Self::Short => 250,
            Self::Standard => 500,
            Self::Deep => 1_000,
        })
    }

    fn of(held: Duration) -> Option<Self> {
        Self::ALL.iter().copied().find(|depth| depth.held() == held)
    }
}

impl Choice for BufferDepth {
    const ALL: &'static [Self] = &[Self::Tight, Self::Short, Self::Standard, Self::Deep];

    fn label(self) -> &'static str {
        match self {
            Self::Tight => "100 ms",
            Self::Short => "250 ms",
            Self::Standard => "500 ms",
            Self::Deep => "1 s",
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Tight => {
                "A volume or equaliser change is heard almost at once, but a busy machine has \
                 the least room before the sound breaks up."
            }
            Self::Short => "Quick to answer, with room for a moment's load on the machine.",
            Self::Standard => {
                "Room for a busy machine or a slow disc without a break in the sound; a volume \
                 change is still heard within half a second."
            }
            Self::Deep => {
                "The most room for a busy machine or a network share, at the cost of up to a \
                 second before a volume or equaliser change is heard."
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LeadIn(Duration);

impl Choice for LeadIn {
    const ALL: &'static [Self] = &[
        Self(Duration::from_millis(250)),
        Self(Duration::from_millis(500)),
        Self(Duration::from_millis(750)),
        Self(Duration::from_millis(1_000)),
        Self(Duration::from_millis(1_500)),
    ];

    fn label(self) -> &'static str {
        match self.0.as_millis() {
            250 => "250 ms",
            500 => "500 ms",
            750 => "750 ms",
            1_000 => "1 s",
            _ => "1.5 s",
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::from(format!(
            "A stream that starts after the link has slept opens on {} of silence before the \
             track. Raise it if the first words are still cut, lower it if the wait is felt.",
            self.label()
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KeptAwake(Duration);

impl Choice for KeptAwake {
    const ALL: &'static [Self] = &[
        Self(Duration::from_secs(60)),
        Self(Duration::from_secs(5 * 60)),
        Self(Duration::from_secs(15 * 60)),
        Self(Duration::from_secs(60 * 60)),
    ];

    fn label(self) -> &'static str {
        match self.0.as_secs() {
            60 => "1 min",
            300 => "5 min",
            900 => "15 min",
            _ => "1 hour",
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::from(format!(
            "A pause keeps the headphones fed with silence for {}, so playing again is heard at \
             once; after that they are let go to sleep and save their battery.",
            self.label()
        ))
    }
}

impl RootView {
    pub(super) fn bluetooth_group(&mut self, cx: &mut Context<Self>) -> Div {
        let wake = self.player.read(cx).output_settings().bluetooth;
        let chosen = |held: Duration, table: &[Duration]| table.contains(&held);

        kit::section_body()
            .child(self.in_the_ring(
                "bluetooth-wake",
                switch_row(
                    "Keep Bluetooth headphones from cutting the start",
                    "Off, a paused Bluetooth device is let go and wakes on the music",
                    wake.on,
                    "bluetooth-wake",
                ),
                move |this, _, cx| {
                    this.send(
                        Command::SetBluetoothWake(BluetoothWake {
                            on: !wake.on,
                            ..wake
                        }),
                        cx,
                    );
                    this.store(&Setting::BluetoothWake(!wake.on), cx);
                },
                cx,
            ))
            .when(wake.on, |body| {
                body.child(kit::field(
                    "Lead-in",
                    self.choices(
                        "bluetooth-lead",
                        chosen(
                            wake.lead,
                            &LeadIn::ALL.iter().map(|lead| lead.0).collect::<Vec<_>>(),
                        )
                        .then_some(LeadIn(wake.lead)),
                        cx,
                        move |this, lead: LeadIn, cx| {
                            let wake = this.player.read(cx).output_settings().bluetooth;
                            this.send(
                                Command::SetBluetoothWake(BluetoothWake {
                                    lead: lead.0,
                                    ..wake
                                }),
                                cx,
                            );
                            this.store(&Setting::BluetoothLead(lead.0), cx);
                        },
                    ),
                ))
                .child(kit::field(
                    "Kept awake through a pause",
                    self.choices(
                        "bluetooth-awake",
                        chosen(
                            wake.awake_for,
                            &KeptAwake::ALL
                                .iter()
                                .map(|awake| awake.0)
                                .collect::<Vec<_>>(),
                        )
                        .then_some(KeptAwake(wake.awake_for)),
                        cx,
                        move |this, awake: KeptAwake, cx| {
                            let wake = this.player.read(cx).output_settings().bluetooth;
                            this.send(
                                Command::SetBluetoothWake(BluetoothWake {
                                    awake_for: awake.0,
                                    ..wake
                                }),
                                cx,
                            );
                            this.store(&Setting::BluetoothAwake(awake.0), cx);
                        },
                    ),
                ))
            })
            .child(note(BLUETOOTH_NOTE))
    }

    pub(super) fn device_group(&mut self, cx: &mut Context<Self>) -> Div {
        let model = self.player.read(cx);
        let sinks = model.sinks().to_vec();
        let wanted = model.output_settings().sink.clone();
        let state = model.state();
        let open = state.output.map(|output| output.sink);
        let source = state.current.map(|track| track.source);

        let mut listed = div().flex().flex_col().gap_2().child(self.default_device(
            wanted.is_none(),
            routes_to(&sinks),
            cx,
        ));
        for sink in &sinks {
            listed = listed.child(self.device(
                sink,
                wanted.as_ref() == Some(&sink.name),
                open,
                source,
                cx,
            ));
        }

        let away = wanted
            .as_ref()
            .is_some_and(|name| !sinks.iter().any(|sink| &sink.name == name));

        kit::section_body()
            .child(listed)
            .when(sinks.is_empty(), |body| body.child(note(NO_SINKS)))
            .when(away, |body| body.child(note(AWAY)))
            .when(!sinks.is_empty() && !away, |body| {
                body.child(note(DEVICE_NOTE))
            })
    }

    fn default_device(
        &self,
        chosen: bool,
        routes_to: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.in_the_ring(
            "follow-the-default",
            kit::choice_row("follow-the-default", chosen)
                .child(kit::level_with_the_choice(kit::radio(chosen)))
                .child(
                    described(kit::choice_name(FOLLOW, true))
                        .child(kit::details([kit::detail(routes_to)])),
                ),
            |this, _, cx| {
                this.send(Command::SetSink(None), cx);
                this.store(&Setting::Sink(None), cx);
            },
            cx,
        )
    }

    fn device(
        &self,
        sink: &SinkInfo,
        chosen: bool,
        open: Option<SinkId>,
        source: Option<StreamSpec>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let exact = source.is_some_and(|spec| sink.supports(spec));
        let playing = open == Some(sink.id);
        let unplugged = unplugged(sink);
        let reached = SharedString::from(sink.name.as_str().to_owned());
        let routed = routed(sink);
        let formats = takes(sink);

        let verdict = source.map(|spec| {
            if exact {
                kit::badge(
                    format!("TAKES {} AS IS", format::quality(spec)),
                    theme::bit_perfect(),
                )
            } else {
                kit::badge(CONVERTS, theme::converted())
            }
        });

        let named = div()
            .flex()
            .items_center()
            .gap_1p5()
            .child(kit::choice_name(sink.description.clone(), !unplugged))
            .when(sink.is_default, |line| {
                line.child(kit::badge(DEFAULT, theme::faint()))
            })
            .when(unplugged, |line| {
                line.child(kit::badge(UNPLUGGED, theme::faint()))
            })
            .when(playing, |line| {
                line.child(kit::badge(IN_USE, theme::accent()))
            })
            .children(verdict);

        let takes = if formats.is_empty() {
            kit::details([kit::detail(UNADVERTISED)])
        } else {
            kit::details(formats.into_iter().map(kit::figure))
        };

        self.device_in_the_ring(
            reached.clone(),
            kit::choice_row(reached, chosen)
                .child(kit::level_with_the_choice(kit::radio(chosen)))
                .child(
                    described(named)
                        .when(!routed.is_empty(), |text| {
                            text.child(kit::details(routed.into_iter().map(kit::detail)))
                        })
                        .child(takes),
                )
                .names(known_as(sink)),
            sink.name.clone(),
            cx,
        )
    }

    fn device_in_the_ring(
        &self,
        reached: SharedString,
        row: Stateful<Div>,
        name: NodeName,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let clicked = name.clone();

        row.track_focus(&self.controls.at(reached, cx))
            .tab_stop(true)
            .key_context(crate::app::CONTROL_CONTEXT)
            .focus(|row| row.border_color(rgb(theme::accent())))
            .on_action(
                cx.listener(move |this, _: &crate::app::PressControl, _, cx| {
                    this.send(Command::SetSink(Some(name.clone())), cx);
                    this.store(&Setting::Sink(Some(name.clone())), cx);
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.send(Command::SetSink(Some(clicked.clone())), cx);
                this.store(&Setting::Sink(Some(clicked.clone())), cx);
            }))
    }

    pub(super) fn sample_rate_group(&mut self, cx: &mut Context<Self>) -> Div {
        let prefer = self.player.read(cx).output_settings().prefer_bit_perfect;

        kit::section_body().child(self.choices(
            "sample-rate",
            Some(SourceRate::of(prefer)),
            cx,
            |this, rate: SourceRate, cx| {
                this.send(Command::SetBitPerfect(rate.matches_the_file()), cx);
                this.store(&Setting::BitPerfect(rate.matches_the_file()), cx);
            },
        ))
    }

    pub(super) fn graph_rate_group(&mut self, cx: &mut Context<Self>) -> Div {
        let forced = self.player.read(cx).output_settings().force_graph_rate;

        kit::section_body().child(self.choices(
            "graph-rate",
            Some(GraphRate::of(forced)),
            cx,
            |this, rate: GraphRate, cx| {
                this.send(Command::SetForceGraphRate(rate.asks_to_switch()), cx);
                this.store(&Setting::ForceGraphRate(rate.asks_to_switch()), cx);
            },
        ))
    }

    pub(super) fn buffer_group(&mut self, cx: &mut Context<Self>) -> Div {
        let held = self.player.read(cx).output_settings().buffer;
        let depth = BufferDepth::of(held);

        kit::section_body()
            .child(
                self.choices("buffer", depth, cx, |this, depth: BufferDepth, cx| {
                    this.send(Command::SetBuffer(depth.held()), cx);
                    this.store(&Setting::Buffer(depth.held()), cx);
                }),
            )
            .when(depth.is_none(), |body| {
                body.child(note(format!(
                    "{} ms is in force, which is none of these.",
                    held.as_millis()
                )))
            })
    }

    pub(super) fn dop_group(&mut self, cx: &mut Context<Self>) -> Div {
        let marked = self.player.read(cx).output_settings().dop;

        kit::section_body()
            .child(self.in_the_ring(
                "dop",
                switch_row(
                    "Hand DSD to the device as DoP",
                    "Decimate it to PCM when this is off",
                    marked,
                    "dop",
                ),
                move |this, _, cx| {
                    this.send(Command::SetDop(!marked), cx);
                    this.store(&Setting::Dop(!marked), cx);
                },
                cx,
            ))
            .child(div().child(note(DOP_WARNING)))
    }
}

fn described(name: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .gap_1()
        .child(name)
}
