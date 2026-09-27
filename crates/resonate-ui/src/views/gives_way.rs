const CLUSTER_GAP: f32 = 4.0;
const VOLUME_LEAD: f32 = 8.0;
const VOLUME_GAP: f32 = 8.0;
const SLEEP_PADDING: f32 = 16.0;
const SLEEP_GAP: f32 = 6.0;
const PATH_GAP: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct StatusWidths {
    pub(crate) toggle: f32,
    pub(crate) icon: f32,
    pub(crate) sleep_reading: Option<f32>,
    pub(crate) rail: f32,
    pub(crate) reading: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StatusKept {
    pub(crate) shuffle: bool,
    pub(crate) repeat: bool,
    pub(crate) sleep: bool,
    pub(crate) rail: bool,
    pub(crate) reading: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusPart {
    Reading,
    Sleep,
    Repeat,
    Shuffle,
    Rail,
}

const STATUS_GIVES_WAY: [StatusPart; 5] = [
    StatusPart::Reading,
    StatusPart::Sleep,
    StatusPart::Repeat,
    StatusPart::Shuffle,
    StatusPart::Rail,
];

impl StatusKept {
    pub(crate) const EVERYTHING: Self = Self {
        shuffle: true,
        repeat: true,
        sleep: true,
        rail: true,
        reading: true,
    };

    fn without(self, part: StatusPart) -> Self {
        match part {
            StatusPart::Reading => Self {
                reading: false,
                ..self
            },
            StatusPart::Sleep => Self {
                sleep: false,
                ..self
            },
            StatusPart::Repeat => Self {
                repeat: false,
                ..self
            },
            StatusPart::Shuffle => Self {
                shuffle: false,
                ..self
            },
            StatusPart::Rail => Self {
                rail: false,
                ..self
            },
        }
    }

    fn width(self, widths: StatusWidths) -> f32 {
        let sleep = widths.sleep_reading.map_or(widths.toggle, |reading| {
            widths
                .toggle
                .max(SLEEP_PADDING + widths.icon + SLEEP_GAP + reading)
        });
        let beside = |kept: bool, width: f32| if kept { CLUSTER_GAP + width } else { 0.0 };
        let inside = |kept: bool, width: f32| if kept { VOLUME_GAP + width } else { 0.0 };

        widths.toggle
            + beside(self.shuffle, widths.toggle)
            + beside(self.repeat, widths.toggle)
            + beside(self.sleep, sleep)
            + CLUSTER_GAP
            + VOLUME_LEAD
            + widths.icon
            + inside(self.rail, widths.rail)
            + inside(self.reading, widths.reading)
    }
}

pub(crate) fn status_kept(room: f32, widths: StatusWidths) -> StatusKept {
    if room <= 0.0 {
        return StatusKept::EVERYTHING;
    }

    STATUS_GIVES_WAY
        .into_iter()
        .fold(StatusKept::EVERYTHING, |kept, part| {
            if kept.width(widths) <= room {
                kept
            } else {
                kept.without(part)
            }
        })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SignalWidths {
    pub(crate) lead: f32,
    pub(crate) quality: Option<f32>,
    pub(crate) dot: f32,
    pub(crate) label: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SignalKept {
    pub(crate) quality: bool,
    pub(crate) dot: bool,
    pub(crate) label: bool,
}

impl SignalKept {
    pub(crate) const EVERYTHING: Self = Self {
        quality: true,
        dot: true,
        label: true,
    };

    fn width(self, widths: SignalWidths) -> f32 {
        let after = |kept: bool, width: Option<f32>| match (kept, width) {
            (true, Some(width)) => PATH_GAP + width,
            _ => 0.0,
        };
        let output = widths.label.map(|_| widths.dot);

        widths.lead
            + after(self.quality, widths.quality)
            + after(self.dot, output)
            + after(self.label, widths.label)
    }
}

pub(crate) fn signal_kept(room: f32, widths: SignalWidths) -> SignalKept {
    if room <= 0.0 {
        return SignalKept::EVERYTHING;
    }
    let steps = [
        SignalKept {
            label: false,
            ..SignalKept::EVERYTHING
        },
        SignalKept {
            quality: false,
            dot: true,
            label: false,
        },
        SignalKept {
            quality: false,
            dot: false,
            label: false,
        },
    ];

    std::iter::once(SignalKept::EVERYTHING)
        .chain(steps)
        .find(|kept| kept.width(widths) <= room)
        .unwrap_or(steps[steps.len() - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: StatusWidths = StatusWidths {
        toggle: 32.0,
        icon: 16.0,
        sleep_reading: None,
        rail: 100.0,
        reading: 34.0,
    };

    const SIGNAL: SignalWidths = SignalWidths {
        lead: 40.0,
        quality: Some(110.0),
        dot: 7.0,
        label: Some(70.0),
    };

    #[test]
    fn a_status_cluster_with_room_keeps_everything_and_one_not_yet_measured_does_too() {
        assert_eq!(status_kept(1_000.0, STATUS), StatusKept::EVERYTHING);
        assert_eq!(status_kept(0.0, STATUS), StatusKept::EVERYTHING);
        assert_eq!(
            status_kept(StatusKept::EVERYTHING.width(STATUS), STATUS),
            StatusKept::EVERYTHING
        );
    }

    #[test]
    fn the_status_cluster_gives_way_reading_first_and_never_the_queue_or_the_speaker() {
        let whole = StatusKept::EVERYTHING.width(STATUS);

        let first = status_kept(whole - 1.0, STATUS);
        assert_eq!(
            first,
            StatusKept {
                reading: false,
                ..StatusKept::EVERYTHING
            }
        );

        let least = status_kept(1.0, STATUS);
        assert_eq!(
            least,
            StatusKept {
                shuffle: false,
                repeat: false,
                sleep: false,
                rail: false,
                reading: false,
            }
        );
    }

    #[test]
    fn whatever_the_cluster_keeps_fits_the_room_it_was_given_where_it_can() {
        let floor = status_kept(1.0, STATUS).width(STATUS);
        for room in (0..400).map(|room| room as f32) {
            let kept = status_kept(room, STATUS);
            if room >= floor {
                assert!(kept.width(STATUS) <= room, "{kept:?} overran {room}");
            }
        }
    }

    #[test]
    fn a_running_timer_widens_the_sleep_control_it_is_weighed_as() {
        let sleeping = StatusWidths {
            sleep_reading: Some(34.0),
            ..STATUS
        };

        assert!(StatusKept::EVERYTHING.width(sleeping) > StatusKept::EVERYTHING.width(STATUS));
    }

    #[test]
    fn the_signal_path_drops_the_mode_name_then_the_quality_then_the_dot_and_keeps_the_codec() {
        let whole = SignalKept::EVERYTHING.width(SIGNAL);

        assert_eq!(signal_kept(whole, SIGNAL), SignalKept::EVERYTHING);
        assert_eq!(
            signal_kept(whole - 1.0, SIGNAL),
            SignalKept {
                label: false,
                ..SignalKept::EVERYTHING
            }
        );
        assert_eq!(
            signal_kept(SIGNAL.lead + 20.0, SIGNAL),
            SignalKept {
                quality: false,
                dot: true,
                label: false,
            }
        );
        assert_eq!(
            signal_kept(SIGNAL.lead, SIGNAL),
            SignalKept {
                quality: false,
                dot: false,
                label: false,
            }
        );
    }
}
