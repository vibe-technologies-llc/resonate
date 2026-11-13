use std::{collections::BTreeSet, time::Duration};

use gpui::{ClickEvent, Context, Div, SharedString, Stateful, div, prelude::*, px};
use resonate_core::{SampleFormat, SampleRate, StreamSpec};
use resonate_engine::{
    BluetoothWake, Command, HardwareVolume, Learning, Learnt, NodeName, Plays, Plugged, SinkId,
    SinkInfo, Trial, TrialVerdict, Unlearnt, Unplayed,
};

use crate::{
    ResonateApp, Setting, format,
    icons::Icon,
    system::sound_system,
    theme,
    views::{
        focus::{self, by_the_pointer},
        hint::Names,
        kit,
        root::RootView,
        settings::{Choice, Rows, note, switch_row},
        spoken,
    },
    words,
};

const SECONDS_A_MINUTE: u64 = 60;

const OR: &str = ", ";

fn known_as(sink: &SinkInfo) -> String {
    words::settings_output::known_as(sink.name.to_string())
}

fn sink_spoken(routed: &[String], formats: &[String]) -> String {
    let takes = if formats.is_empty() {
        words::settings_output::unadvertised().to_owned()
    } else {
        words::settings_output::takes(formats.join(", "))
    };

    routed
        .iter()
        .map(String::as_str)
        .chain([takes.as_str()])
        .collect::<Vec<_>>()
        .join(". ")
}

fn routes_to(sinks: &[SinkInfo]) -> String {
    sinks.iter().find(|sink| sink.is_default).map_or_else(
        || words::settings_output::routed(sound_system!()),
        |sink| words::settings_output::routed_now(&sink.description, sound_system!()),
    )
}

fn routed(sink: &SinkInfo) -> Vec<String> {
    sink.port
        .as_ref()
        .map(|port| port.description.clone())
        .into_iter()
        .chain(
            sink.profile
                .as_ref()
                .map(|profile| profile.description.clone()),
        )
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
        HardwareVolume::Yes | HardwareVolume::Always => {
            Some(words::settings_output::on_the_hardware().to_owned())
        }
        HardwareVolume::No => Some(words::settings_output::in_software().to_owned()),
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
        .map(SampleRate::kilohertz)
        .collect();

    (!offered.is_empty()).then(|| words::settings_output::kilohertz(offered.join(OR)))
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
            Self::MatchTheFile => words::settings_output::match_the_file(),
            Self::FollowTheGraph => words::settings_output::follow_the_graph(),
        }
    }

    fn meaning(self) -> SharedString {
        match self {
            Self::MatchTheFile => {
                SharedString::new_static(words::settings_output::the_stream_opens_at_the_files())
            }
            Self::FollowTheGraph => SharedString::from(
                words::settings_output::following_the_graph_meaning(sound_system!()),
            ),
        }
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
            Self::AskToSwitch => words::settings_output::ask_the_graph_to_switch(),
            Self::LeaveItToTheDaemon => words::settings_output::leave_it_to_the_daemon(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::AskToSwitch => words::settings_output::the_stream_asks_pipewire_to_run(),
            Self::LeaveItToTheDaemon => words::settings_output::pipewire_keeps_the_rate_it_chose(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BufferDepth(Duration);

impl Choice for BufferDepth {
    const ALL: &'static [Self] = &[
        Self(Duration::from_millis(100)),
        Self(Duration::from_millis(250)),
        Self(Duration::from_millis(500)),
        Self(Duration::from_millis(1_000)),
    ];

    fn label(self) -> &'static str {
        match self.0.as_millis() {
            100 => words::settings_output::n_100_ms(),
            250 => words::settings_output::n_250_ms(),
            500 => words::settings_output::n_500_ms(),
            _ => "1 s",
        }
    }

    fn in_force(self) -> SharedString {
        in_milliseconds(self.0)
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self.0.as_millis() {
            100 => words::settings_output::a_volume_or_equaliser_change_is(),
            250 => words::settings_output::quick_to_answer_with_room_for(),
            500 => words::settings_output::room_for_a_busy_machine_or(),
            _ => words::settings_output::the_most_room_for_a_busy(),
        })
    }
}

fn in_milliseconds(held: Duration) -> SharedString {
    SharedString::from(words::settings_output::milliseconds(
        held.as_millis().to_string(),
    ))
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
            250 => words::settings_output::n_250_ms(),
            500 => words::settings_output::n_500_ms(),
            750 => words::settings_output::n_750_ms(),
            1_000 => "1 s",
            _ => "1.5 s",
        }
    }

    fn in_force(self) -> SharedString {
        in_milliseconds(self.0)
    }

    fn meaning(self) -> SharedString {
        SharedString::from(words::settings_output::lead_in_meaning(self.label()))
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
            60 => words::settings_output::n_1_min(),
            300 => words::settings_output::n_5_min(),
            900 => words::settings_output::n_15_min(),
            _ => words::settings_output::n_1_hour(),
        }
    }

    fn in_force(self) -> SharedString {
        let seconds = self.0.as_secs();
        SharedString::from(if seconds.is_multiple_of(SECONDS_A_MINUTE) {
            words::settings_output::minutes(seconds / SECONDS_A_MINUTE)
        } else {
            words::settings_output::seconds(seconds)
        })
    }

    fn meaning(self) -> SharedString {
        SharedString::from(words::settings_output::kept_awake_meaning(self.label()))
    }
}

struct Lying {
    open: Option<SinkId>,
    source: Option<StreamSpec>,
    held_by: Option<SharedString>,
}

fn held_by_another(zone: &str) -> String {
    words::settings_output::held_by_another(zone)
}

impl RootView {
    pub(super) fn bluetooth_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let wake = self.player.read(cx).output_settings().bluetooth;

        kit::rows()
            .child(self.in_the_ring(
                "bluetooth-wake",
                switch_row(
                    words::settings_output::keep_bluetooth_headphones_from_cutting_the(),
                    words::settings_output::off_a_paused_bluetooth_device_is(),
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
                    words::settings_output::lead_in(),
                    self.choices(
                        "bluetooth-lead",
                        LeadIn(wake.lead),
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
                    words::settings_output::kept_awake_through_a_pause(),
                    self.choices(
                        "bluetooth-awake",
                        KeptAwake(wake.awake_for),
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
            .note(words::settings_output::bluetooth_note(sound_system!()))
    }

    pub(super) fn device_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let model = self.player.read(cx);
        let sinks = model.sinks().to_vec();
        let wanted = model.output_settings().sink.clone();
        let state = model.state();
        let open = state.output.map(|output| output.sink);
        let source = state.current.map(|track| track.source);

        let mut listed =
            kit::rows().child(self.default_device(wanted.is_none(), routes_to(&sinks), cx));
        let held = Self::zones(cx).holding(self.switcher.selected());
        for sink in &sinks {
            let held_by = held
                .iter()
                .find(|held| held.sink == sink.id)
                .and_then(|held| self.switcher.name_of(held.by))
                .map(|zone| SharedString::from(zone.to_string()));
            listed = listed.child(self.device(
                sink,
                wanted.as_ref() == Some(&sink.name),
                Lying {
                    open,
                    source,
                    held_by,
                },
                cx,
            ));
        }

        let away = wanted
            .as_ref()
            .is_some_and(|name| !sinks.iter().any(|sink| sink.is_named(name)));
        let looking = cx
            .global::<ResonateApp>()
            .renderers
            .as_ref()
            .map(|renderers| renderers.looking());
        listed
            .when(sinks.is_empty(), |body| {
                body.child(note(words::settings_output::no_sinks(sound_system!())))
            })
            .when(away, |body| {
                body.child(note(words::settings_output::away()))
            })
            .when_some(looking, |body, looking| {
                body.child(self.in_the_ring(
                    "renderers",
                    switch_row(
                        words::settings_output::look_for_renderers_on_the_network(),
                        words::settings_output::off_only_this_computers_own_devices(),
                        looking,
                        "renderers",
                    ),
                    move |this, _, cx| this.look_for_renderers(!looking, cx),
                    cx,
                ))
                .note(words::settings_output::renderers_note())
            })
            .when(!sinks.is_empty() && !away, |body| {
                body.note(words::settings_output::device_note())
            })
    }

    pub(super) fn flac_renderers_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let named = cx.global::<ResonateApp>().flac_above_96k_for.clone();
        let listed: Vec<(NodeName, String)> = self
            .player
            .read(cx)
            .sinks()
            .iter()
            .filter(|sink| sink.name.names_a_upnp_renderer())
            .map(|sink| (sink.name.clone(), sink.description.clone()))
            .collect();
        let away: Vec<NodeName> = named
            .iter()
            .filter(|name| listed.iter().all(|(listed, _)| listed != *name))
            .cloned()
            .collect();

        let mut body = kit::rows();
        for (name, description) in listed {
            let vouched = named.contains(&name);
            body = body.child(self.vouching_row(name, description, vouched, cx));
        }
        for name in away {
            body = body.child(self.vouching_row(name.clone(), name.as_str().to_owned(), true, cx));
        }
        body.when(named.is_empty() && !self.a_renderer_is_listed(cx), |body| {
            body.child(note(words::settings_output::no_renderers_to_name()))
        })
        .note(words::settings_output::flac_renderers_note())
    }

    fn a_renderer_is_listed(&self, cx: &Context<Self>) -> bool {
        self.player
            .read(cx)
            .sinks()
            .iter()
            .any(|sink| sink.name.names_a_upnp_renderer())
    }

    fn vouching_row(
        &self,
        name: NodeName,
        description: String,
        vouched: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = format!("flac-above-96k-{}", name.as_str());
        let under = if self.a_renderer_is_listed_as(&name, cx) {
            name.as_str().to_owned()
        } else {
            words::settings_output::not_on_the_network().to_owned()
        };

        self.in_the_ring_at(
            id.clone(),
            switch_row(description, under, vouched, id),
            move |this, _, cx| this.vouch_for(name.clone(), !vouched, cx),
            cx,
        )
    }

    fn a_renderer_is_listed_as(&self, name: &NodeName, cx: &Context<Self>) -> bool {
        self.player
            .read(cx)
            .sinks()
            .iter()
            .any(|sink| sink.is_named(name))
    }

    pub(crate) fn vouch_for(&mut self, renderer: NodeName, vouched: bool, cx: &mut Context<Self>) {
        let mut named = cx.global::<ResonateApp>().flac_above_96k_for.clone();
        if vouched {
            named.insert(renderer);
        } else {
            named.remove(&renderer);
        }
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.flac_above_96k_for.clone_from(&named);
        });
        self.vouch_now(&named, cx);
        self.store(&Setting::FlacAbove96kFor(named), cx);
        cx.notify();
    }

    pub(crate) fn vouch_for_none(&mut self, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.flac_above_96k_for.clear());
        self.vouch_now(&BTreeSet::new(), cx);
        cx.notify();
    }

    fn vouch_now(&self, named: &BTreeSet<NodeName>, cx: &Context<Self>) {
        if let Some(renderers) = cx.global::<ResonateApp>().renderers.clone() {
            renderers.vouch(named);
        }
    }

    pub(super) fn look_for_renderers(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(renderers) = cx.global::<ResonateApp>().renderers.clone() {
            renderers.look(on);
        }
        self.store(&Setting::Renderers(on), cx);
        cx.notify();
    }

    fn default_device(
        &self,
        chosen: bool,
        routes_to: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.in_the_ring(
            "follow-the-default",
            kit::choice_row(
                "follow-the-default",
                words::settings_output::follow(),
                || routes_to.clone(),
                chosen,
            )
            .child(kit::level_with_the_choice(kit::radio(chosen)))
            .child(
                described(kit::choice_name(words::settings_output::follow(), true))
                    .child(kit::details([kit::detail(routes_to)])),
            ),
            |this, _, cx| {
                this.route_to(None, cx);
            },
            cx,
        )
    }

    fn device(
        &self,
        sink: &SinkInfo,
        chosen: bool,
        lying: Lying,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let Lying {
            open,
            source,
            held_by,
        } = lying;
        let exact = source.is_some_and(|spec| sink.supports(spec));
        let playing = open == Some(sink.id);
        let unplugged = unplugged(sink);
        let reached = SharedString::from(sink.name.as_str().to_owned());
        let routed = routed(sink);
        let formats = takes(sink);

        let verdict = source.map(|spec| {
            if exact {
                kit::badge(
                    words::settings_output::takes_as_is(format::quality(spec)),
                    theme::bit_perfect(),
                )
            } else {
                kit::badge(words::settings_output::converts(), theme::converted())
            }
        });

        let states: Vec<Div> = [
            sink.is_default
                .then(|| kit::badge(words::settings_output::default(), theme::faint())),
            unplugged.then(|| kit::badge(words::settings_output::unplugged(), theme::faint())),
            playing.then(|| kit::badge(words::settings_output::in_use(), theme::accent())),
            held_by
                .as_ref()
                .map(|zone| kit::badge(held_by_another(zone), theme::faint())),
            verdict,
        ]
        .into_iter()
        .flatten()
        .collect();
        let named = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x_2()
            .child(kit::choice_name(sink.description.clone(), !unplugged))
            .when(!states.is_empty(), |line| line.child(kit::details(states)));

        let saying = if spoken::listening() {
            let said = sink_spoken(&routed, &formats);
            match held_by.as_ref() {
                Some(zone) => words::settings_output::said_and_held_by(&said, zone.as_str()),
                None => said,
            }
        } else {
            String::new()
        };

        let takes = if formats.is_empty() {
            kit::details([kit::detail(words::settings_output::unadvertised())])
        } else {
            kit::details(formats.into_iter().map(kit::figure))
        };

        self.device_in_the_ring(
            reached.clone(),
            kit::choice_row(reached, &sink.description, || saying, chosen)
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
            .key_context(crate::app::CONTROL_CONTEXT)
            .focus_visible(focus::lit)
            .on_action(
                cx.listener(move |this, _: &crate::app::PressControl, _, cx| {
                    this.route_to(Some(name.clone()), cx);
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, click: &ClickEvent, _, cx| {
                if by_the_pointer(click) {
                    this.route_to(Some(clicked.clone()), cx);
                }
            }))
    }

    pub(super) fn sample_rate_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let prefer = self.player.read(cx).output_settings().prefer_bit_perfect;

        kit::rows().child(self.choices(
            "sample-rate",
            SourceRate::of(prefer),
            cx,
            |this, rate: SourceRate, cx| {
                this.send(Command::SetBitPerfect(rate.matches_the_file()), cx);
                this.store(&Setting::BitPerfect(rate.matches_the_file()), cx);
            },
        ))
    }

    pub(super) fn graph_rate_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let forced = self.player.read(cx).output_settings().force_graph_rate;

        kit::rows().child(self.choices(
            "graph-rate",
            GraphRate::of(forced),
            cx,
            |this, rate: GraphRate, cx| {
                this.send(Command::SetForceGraphRate(rate.asks_to_switch()), cx);
                this.store(&Setting::ForceGraphRate(rate.asks_to_switch()), cx);
            },
        ))
    }

    pub(super) fn buffer_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let held = BufferDepth(self.player.read(cx).output_settings().buffer);

        kit::rows().child(
            self.choices("buffer", held, cx, |this, depth: BufferDepth, cx| {
                this.send(Command::SetBuffer(depth.0), cx);
                this.store(&Setting::Buffer(depth.0), cx);
            }),
        )
    }

    pub(super) fn device_volume_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let player = self.player.read(cx);
        let handed = player.output_settings().device_volume;
        let bound = player.state().output.map(|output| output.sink);
        let turns_its_own = bound.and_then(|bound| {
            player
                .sinks()
                .iter()
                .find(|sink| sink.id == bound)
                .map(SinkInfo::turns_its_own_volume)
        });

        kit::rows()
            .child(self.in_the_ring(
                "device-volume",
                switch_row(
                    words::settings_output::turn_the_devices_own_volume(),
                    words::settings_output::scale_the_samples_when_this_is(),
                    handed,
                    "device-volume",
                ),
                move |this, _, cx| {
                    this.send(Command::SetDeviceVolume(!handed), cx);
                    this.store(&Setting::DeviceVolume(!handed), cx);
                },
                cx,
            ))
            .when_some(turns_its_own, |body, turns_its_own| {
                body.note(if turns_its_own {
                    words::settings_output::the_device_playing_now_has_a()
                } else {
                    words::settings_output::the_device_playing_now_has_no()
                })
            })
    }

    pub(super) fn dop_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let marked = self.player.read(cx).output_settings().dop;

        kit::rows()
            .child(self.in_the_ring(
                "dop",
                switch_row(
                    words::settings_output::hand_dsd_to_the_device_as(),
                    words::settings_output::decimate_it_to_pcm_when_this(),
                    marked,
                    "dop",
                ),
                move |this, _, cx| {
                    this.send(Command::SetDop(!marked), cx);
                    this.store(&Setting::Dop(!marked), cx);
                },
                cx,
            ))
            .child(self.dsd_level_switch(cx))
            .note(words::settings_output::dop_warning())
    }

    fn dsd_level_switch(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let raised = self.player.read(cx).output_settings().dsd_like_pcm;

        self.in_the_ring(
            "dsd-like-pcm",
            switch_row(
                words::settings_output::decimate_dsd_to_the_level_of(),
                words::settings_output::off_a_dsd_master_plays_some(),
                raised,
                "dsd-like-pcm",
            ),
            move |this, _, cx| {
                this.send(Command::SetDsdLikePcm(!raised), cx);
                this.store(&Setting::DsdLikePcm(!raised), cx);
            },
            cx,
        )
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

impl RootView {
    pub(super) fn learnt_formats_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let learnt = cx.global::<ResonateApp>().formats_learnt_for.clone();
        let listed: Vec<(NodeName, String)> = self
            .player
            .read(cx)
            .sinks()
            .iter()
            .filter(|sink| sink.name.names_a_network_device())
            .map(|sink| (sink.name.clone(), sink.description.clone()))
            .collect();
        let away: Vec<NodeName> = learnt
            .iter()
            .map(|(name, _)| name)
            .filter(|name| listed.iter().all(|(listed, _)| listed != *name))
            .cloned()
            .collect();
        let none_listed = listed.is_empty();

        let mut body = kit::rows();
        for (name, description) in listed {
            body = body.child(self.learning_row(name, description, true, cx));
        }
        for name in away {
            body = body.child(self.learning_row(name.clone(), name.as_str().to_owned(), false, cx));
        }
        body.when(none_listed && learnt.is_empty(), |body| {
            body.child(note(words::settings_output::no_devices_to_learn()))
        })
        .note(words::settings_output::learnt_formats_note())
    }

    fn learning_row(
        &self,
        name: NodeName,
        description: String,
        listed: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let plays = cx
            .global::<ResonateApp>()
            .formats_learnt_for
            .of([&name])
            .cloned();
        let trying = self.settings_shown.trying.as_ref().map(|(name, _)| name);
        let this_tried = trying == Some(&name);
        let heard = self
            .settings_shown
            .heard
            .as_ref()
            .filter(|(heard, _)| *heard == name)
            .map(|(_, heard)| heard.clone());
        let under = if listed {
            name.as_str().to_owned()
        } else {
            words::settings_output::not_on_the_network().to_owned()
        };
        let learn_id = format!("learn-what-it-plays-{}", name.as_str());
        let forget_id = format!("forget-what-was-learnt-{}", name.as_str());

        let learn = if trying.is_some() || !listed {
            kit::button_when(
                kit::Press::Greyed,
                learn_id,
                Some(Icon::Analysis),
                if this_tried {
                    words::settings_output::trying_it()
                } else {
                    words::settings_output::learn_what_it_plays()
                },
                words::settings_output::learn_what_it_plays(),
                kit::Tone::Outlined,
            )
        } else {
            let asked = name.clone();
            self.in_the_ring_at(
                learn_id.clone(),
                kit::button(
                    learn_id,
                    Some(Icon::Analysis),
                    words::settings_output::learn_what_it_plays(),
                    words::settings_output::learn_what_it_plays(),
                    kit::Tone::Outlined,
                ),
                move |this, _, cx| this.learn_what_it_plays(asked.clone(), cx),
                cx,
            )
        };
        let forget = plays.is_some().then(|| {
            let forgotten = name.clone();
            self.in_the_ring_at(
                forget_id.clone(),
                kit::button(
                    forget_id,
                    Some(Icon::Discard),
                    words::settings_output::forget_what_was_learnt(),
                    words::settings_output::forget_what_was_learnt(),
                    kit::Tone::Outlined,
                ),
                move |this, _, cx| this.forget_what_was_learnt(forgotten.clone(), cx),
                cx,
            )
        });

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                described(kit::choice_name(description, true)).child(kit::details([
                    kit::detail(under),
                    kit::detail(plays_said(plays.as_ref())),
                ])),
            )
            .child(kit::action_row().child(learn).children(forget))
            .when_some(heard, |row, heard| row.child(note(heard_said(&heard))))
    }

    pub(crate) fn learn_what_it_plays(&mut self, device: NodeName, cx: &mut Context<Self>) {
        let Some(renderers) = cx.global::<ResonateApp>().renderers.clone() else {
            return;
        };
        let asked = device.clone();
        let tried = cx.spawn(async move |this, cx| {
            let heard = cx
                .background_executor()
                .spawn({
                    let asked = asked.clone();
                    async move { renderers.learn(&asked) }
                })
                .await;
            let _ = this.update(cx, |this, cx| this.heard_what_it_plays(asked, heard, cx));
        });
        self.settings_shown.heard = None;
        self.settings_shown.trying = Some((device, tried));
        cx.notify();
    }

    fn heard_what_it_plays(
        &mut self,
        device: NodeName,
        heard: Result<Learning, Unlearnt>,
        cx: &mut Context<Self>,
    ) {
        self.settings_shown.trying = None;
        if let Ok(learning) = &heard {
            let plays = learning.plays();
            cx.update_global::<ResonateApp, _>(|global, _| {
                global
                    .formats_learnt_for
                    .insert(device.clone(), plays.clone());
            });
            self.store(
                &Setting::FormatsLearntFor {
                    sink: device.clone(),
                    plays: Some(plays),
                },
                cx,
            );
        }
        self.settings_shown.heard = Some((device, heard));
        cx.notify();
    }

    pub(crate) fn forget_what_was_learnt(&mut self, device: NodeName, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.formats_learnt_for.remove(&device);
        });
        self.teach_now(cx);
        self.store(
            &Setting::FormatsLearntFor {
                sink: device,
                plays: None,
            },
            cx,
        );
        cx.notify();
    }

    pub(crate) fn forget_everything_learnt(&mut self, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.formats_learnt_for = Learnt::default();
        });
        self.settings_shown.heard = None;
        self.teach_now(cx);
        cx.notify();
    }

    fn teach_now(&self, cx: &Context<Self>) {
        let app = cx.global::<ResonateApp>();
        if let Some(renderers) = app.renderers.clone() {
            renderers.teach(&app.formats_learnt_for);
        }
    }
}

fn plays_said(plays: Option<&Plays>) -> String {
    let Some(plays) = plays else {
        return words::settings_output::not_tried_yet().to_owned();
    };
    if plays.is_empty() {
        return words::settings_output::played_none_of_them().to_owned();
    }
    words::settings_output::plays(plays.trials().map(trial_named).collect::<Vec<_>>().join(OR))
}

fn heard_said(heard: &Result<Learning, Unlearnt>) -> String {
    match heard {
        Ok(learning) => learning
            .verdicts()
            .iter()
            .map(|(trial, verdict)| {
                words::settings_output::heard(trial_named(*trial), verdict_named(*verdict))
            })
            .collect::<Vec<_>>()
            .join(OR),
        Err(Unlearnt::NotListed) => words::settings_output::unlearnt_not_listed().to_owned(),
        Err(Unlearnt::InUse) => words::settings_output::unlearnt_in_use().to_owned(),
        Err(Unlearnt::Unreached) => words::settings_output::unlearnt_unreached().to_owned(),
    }
}

fn trial_named(trial: Trial) -> &'static str {
    match trial {
        Trial::FlacAt192kHz => words::settings_output::flac_at_192(),
        Trial::FlacAt176kHz => words::settings_output::flac_at_176(),
        Trial::SixChannels => words::settings_output::six_channels(),
        Trial::EightChannels => words::settings_output::eight_channels(),
    }
}

fn verdict_named(verdict: TrialVerdict) -> &'static str {
    match verdict {
        TrialVerdict::Played => words::settings_output::played(),
        TrialVerdict::Untried => words::settings_output::untried(),
        TrialVerdict::Unplayed(Unplayed::Refused) => words::settings_output::refused(),
        TrialVerdict::Unplayed(Unplayed::NeverFetched) => words::settings_output::never_fetched(),
        TrialVerdict::Unplayed(Unplayed::StoppedFetching) => {
            words::settings_output::stopped_fetching()
        }
        TrialVerdict::Unplayed(Unplayed::Errored) => words::settings_output::errored(),
        TrialVerdict::Unplayed(Unplayed::NotPlaying) => words::settings_output::not_playing(),
        TrialVerdict::Unplayed(Unplayed::ClockStill) => words::settings_output::clock_still(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::settings::said_under;

    #[test]
    fn a_length_off_a_table_says_what_is_in_force_where_one_on_it_says_what_it_means() {
        assert_eq!(
            said_under(LeadIn(Duration::from_millis(1_200))),
            "1200 ms is in force, which is none of these."
        );
        assert_eq!(
            said_under(KeptAwake(Duration::from_secs(90))),
            "90 s is in force, which is none of these."
        );
        assert_eq!(
            said_under(KeptAwake(Duration::from_secs(120))),
            "2 min is in force, which is none of these."
        );
        assert_eq!(
            said_under(BufferDepth(Duration::from_millis(300))),
            "300 ms is in force, which is none of these."
        );

        for listed in LeadIn::ALL {
            assert_eq!(said_under(*listed), listed.meaning());
        }
        for listed in BufferDepth::ALL {
            assert_eq!(said_under(*listed), listed.meaning());
        }
    }
}
