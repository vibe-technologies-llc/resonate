use std::time::{Duration, SystemTime, UNIX_EPOCH};

use resonate_core::{Mbid, Pictured, Presence, Shown};
use serde::Serialize;

const LISTENING: u8 = 2;
const LONGEST_TEXT: usize = 128;
const SHORTEST_TEXT: usize = 2;
const PADDING: char = '\u{200b}';
const PLAYER: &str = "Resonate";
const PAUSED: &str = "Paused";
const ALBUM_AFTER_ARTIST: &str = " — ";
const COVER_ART_ARCHIVE: &str = "https://coverartarchive.org";
const COVER_SIZE: &str = "front-500";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Cover {
    Release(Mbid),
    Group(Mbid),
}

impl Cover {
    pub fn url(&self) -> String {
        match self {
            Self::Release(release) => format!("{COVER_ART_ARCHIVE}/release/{release}/{COVER_SIZE}"),
            Self::Group(group) => {
                format!("{COVER_ART_ARCHIVE}/release-group/{group}/{COVER_SIZE}")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Playing {
    pub(crate) title: String,
    pub(crate) artist: Option<String>,
    pub(crate) album: Option<String>,
    pub(crate) cover: Option<Cover>,
    pub(crate) position: Duration,
    pub(crate) duration: Option<Duration>,
    pub(crate) paused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Activity {
    #[serde(rename = "type")]
    kind: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamps: Option<Timestamps>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assets: Option<Assets>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct Timestamps {
    start: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Assets {
    #[serde(skip_serializing_if = "Option::is_none")]
    large_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    large_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    small_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    small_text: Option<String>,
}

impl Activity {
    pub(crate) fn of(presence: &Presence, playing: &Playing, now: SystemTime) -> Option<Self> {
        if !presence.active() || playing.paused && !presence.while_paused {
            return None;
        }

        let cover = match (presence.pictured, presence.shown) {
            (Pictured::Cover, Shown::Track | Shown::Album) => {
                playing.cover.as_ref().map(Cover::url)
            }
            (Pictured::Cover, Shown::Application) | (Pictured::Icon | Pictured::Nothing, _) => None,
        };
        let icon = match presence.pictured {
            Pictured::Cover | Pictured::Icon => presence.icon.as_ref().map(|icon| icon.to_string()),
            Pictured::Nothing => None,
        };
        let (large_image, small_image) = match cover {
            Some(cover) => (Some(cover), icon),
            None => (icon, None),
        };

        let album = match presence.shown {
            Shown::Album => playing.album.as_deref(),
            Shown::Application | Shown::Track => None,
        };
        let (details, state, large_text) = match presence.shown {
            Shown::Application => (None, None, None),
            Shown::Track | Shown::Album => {
                let artist = playing.artist.as_deref();
                let state = match (artist, album, large_image.is_some()) {
                    (Some(artist), Some(album), false) => {
                        Some(format!("{artist}{ALBUM_AFTER_ARTIST}{album}"))
                    }
                    (None, Some(album), false) => Some(album.to_owned()),
                    (artist, _, _) => artist.map(str::to_owned),
                };
                let large_text = album.filter(|_| large_image.is_some());
                (
                    fitted(&playing.title),
                    state.as_deref().and_then(fitted),
                    large_text.and_then(fitted),
                )
            }
        };
        let small_text = small_image
            .as_ref()
            .map(|_| if playing.paused { PAUSED } else { PLAYER })
            .and_then(fitted);

        let timestamps = (presence.progress && !playing.paused)
            .then(|| Timestamps::of(playing, now))
            .flatten();
        let assets = (large_image.is_some() || small_image.is_some()).then_some(Assets {
            large_image,
            large_text,
            small_image,
            small_text,
        });

        Some(Self {
            kind: LISTENING,
            details,
            state,
            timestamps,
            assets,
        })
    }

    pub(crate) fn alike(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.details == other.details
            && self.state == other.state
            && self.assets == other.assets
            && self.timestamps.is_some() == other.timestamps.is_some()
    }

    pub(crate) fn drifted(&self, other: &Self, allowed: Duration) -> bool {
        let (Some(was), Some(now)) = (self.timestamps, other.timestamps) else {
            return false;
        };
        let apart = |a: u64, b: u64| u128::from(a.abs_diff(b)) > allowed.as_millis();
        let ends = match (was.end, now.end) {
            (Some(a), Some(b)) => apart(a, b),
            (None, None) => false,
            (Some(_), None) | (None, Some(_)) => true,
        };
        apart(was.start, now.start) || ends
    }
}

impl Timestamps {
    fn of(playing: &Playing, now: SystemTime) -> Option<Self> {
        let started = now.checked_sub(playing.position)?;
        let start = millis(started)?;
        let end = playing
            .duration
            .and_then(|duration| started.checked_add(duration))
            .and_then(millis);
        Some(Self { start, end })
    }
}

fn millis(at: SystemTime) -> Option<u64> {
    let since = at.duration_since(UNIX_EPOCH).ok()?;
    u64::try_from(since.as_millis()).ok()
}

fn fitted(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut fitted: String = trimmed.chars().take(LONGEST_TEXT).collect();
    while fitted.chars().count() < SHORTEST_TEXT {
        fitted.push(PADDING);
    }
    Some(fitted)
}

#[cfg(test)]
mod tests {
    use resonate_core::{AppId, Icon};
    use serde_json::{Value, json};

    use super::*;

    const RELEASE: &str = "0a7f4b1c-2d3e-4f50-8a6b-7c8d9e0f1a2b";

    fn presence(shown: Shown, pictured: Pictured) -> Presence {
        Presence {
            enabled: true,
            app: AppId::parse("1234567890123456789"),
            shown,
            pictured,
            icon: Icon::parse("resonate"),
            progress: true,
            while_paused: false,
        }
    }

    fn playing() -> Playing {
        Playing {
            title: "Echoes".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            album: Some("Meddle".to_owned()),
            cover: Some(Cover::Release(Mbid::new(RELEASE).expect("an mbid"))),
            position: Duration::from_secs(60),
            duration: Some(Duration::from_secs(1_411)),
            paused: false,
        }
    }

    fn at() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    fn written(presence: &Presence, playing: &Playing) -> Value {
        let activity = Activity::of(presence, playing, at()).expect("an activity");
        serde_json::to_value(activity).expect("serialised")
    }

    #[test]
    fn nothing_is_shown_unless_the_presence_is_switched_on_and_names_an_application() {
        let playing = playing();

        assert_eq!(Activity::of(&Presence::OFF, &playing, at()), None);
        let unnamed = Presence {
            app: None,
            ..presence(Shown::Album, Pictured::Cover)
        };
        assert_eq!(Activity::of(&unnamed, &playing, at()), None);
    }

    #[test]
    fn the_whole_of_it_is_the_title_the_artist_the_album_the_cover_and_the_bar() {
        let shown = written(&presence(Shown::Album, Pictured::Cover), &playing());

        assert_eq!(
            shown,
            json!({
                "type": 2,
                "details": "Echoes",
                "state": "Pink Floyd",
                "timestamps": { "start": 1_799_999_940_000_u64, "end": 1_800_001_351_000_u64 },
                "assets": {
                    "large_image": format!("https://coverartarchive.org/release/{RELEASE}/front-500"),
                    "large_text": "Meddle",
                    "small_image": "resonate",
                    "small_text": "Resonate",
                },
            })
        );
    }

    #[test]
    fn the_track_alone_leaves_the_album_out() {
        let shown = written(&presence(Shown::Track, Pictured::Cover), &playing());

        assert_eq!(shown["details"], "Echoes");
        assert_eq!(shown["state"], "Pink Floyd");
        assert!(shown["assets"].get("large_text").is_none());
    }

    #[test]
    fn just_the_player_says_nothing_about_what_is_playing() {
        let shown = written(&presence(Shown::Application, Pictured::Icon), &playing());

        assert!(shown.get("details").is_none());
        assert!(shown.get("state").is_none());
        assert_eq!(shown["assets"], json!({ "large_image": "resonate" }));
    }

    #[test]
    fn just_the_player_draws_no_cover_even_where_one_is_asked_for() {
        let shown = written(&presence(Shown::Application, Pictured::Cover), &playing());

        assert_eq!(shown["assets"], json!({ "large_image": "resonate" }));
    }

    #[test]
    fn the_icon_stands_in_where_no_cover_is_known_and_the_album_joins_the_artist() {
        let uncovered = Playing {
            cover: None,
            ..playing()
        };
        let shown = written(&presence(Shown::Album, Pictured::Cover), &uncovered);

        assert_eq!(shown["assets"]["large_image"], "resonate");
        assert_eq!(shown["assets"]["large_text"], "Meddle");
        assert!(shown["assets"].get("small_image").is_none());

        let bare = Presence {
            icon: None,
            ..presence(Shown::Album, Pictured::Cover)
        };
        let shown = written(&bare, &uncovered);
        assert!(shown.get("assets").is_none());
        assert_eq!(shown["state"], "Pink Floyd — Meddle");
    }

    #[test]
    fn no_picture_is_no_assets_at_all() {
        let shown = written(&presence(Shown::Track, Pictured::Nothing), &playing());

        assert!(shown.get("assets").is_none());
    }

    #[test]
    fn a_release_group_is_asked_for_by_its_own_path() {
        let group = Cover::Group(Mbid::new(RELEASE).expect("an mbid"));

        assert_eq!(
            group.url(),
            format!("https://coverartarchive.org/release-group/{RELEASE}/front-500")
        );
    }

    #[test]
    fn a_pause_clears_the_presence_unless_it_is_asked_to_stay() {
        let paused = Playing {
            paused: true,
            ..playing()
        };
        assert_eq!(
            Activity::of(&presence(Shown::Album, Pictured::Cover), &paused, at()),
            None
        );

        let staying = Presence {
            while_paused: true,
            ..presence(Shown::Album, Pictured::Cover)
        };
        let shown = written(&staying, &paused);
        assert!(shown.get("timestamps").is_none());
        assert_eq!(shown["assets"]["small_text"], "Paused");
    }

    #[test]
    fn no_bar_is_drawn_where_the_progress_is_turned_off_or_the_length_is_unknown() {
        let barless = Presence {
            progress: false,
            ..presence(Shown::Album, Pictured::Cover)
        };
        assert!(written(&barless, &playing()).get("timestamps").is_none());

        let endless = Playing {
            duration: None,
            ..playing()
        };
        let shown = written(&presence(Shown::Album, Pictured::Cover), &endless);
        assert_eq!(
            shown["timestamps"],
            json!({ "start": 1_799_999_940_000_u64 })
        );
    }

    #[test]
    fn text_is_cut_to_what_discord_holds_and_padded_to_what_it_asks_for() {
        let long = Playing {
            title: "ü".repeat(200),
            artist: Some("X".to_owned()),
            ..playing()
        };
        let shown = written(&presence(Shown::Track, Pictured::Nothing), &long);

        assert_eq!(
            shown["details"].as_str().map(|text| text.chars().count()),
            Some(LONGEST_TEXT)
        );
        assert_eq!(shown["state"], "X\u{200b}");
    }

    #[test]
    fn a_seek_is_a_drift_and_the_clock_moving_on_is_not() {
        let presence = presence(Shown::Album, Pictured::Cover);
        let before = Activity::of(&presence, &playing(), at()).expect("an activity");
        let ticked = Playing {
            position: Duration::from_secs(61),
            ..playing()
        };
        let a_second_later =
            Activity::of(&presence, &ticked, at() + Duration::from_secs(1)).expect("an activity");
        let sought = Playing {
            position: Duration::from_secs(300),
            ..playing()
        };
        let after_a_seek = Activity::of(&presence, &sought, at()).expect("an activity");

        assert!(before.alike(&a_second_later));
        assert!(!before.drifted(&a_second_later, Duration::from_secs(2)));
        assert!(before.alike(&after_a_seek));
        assert!(before.drifted(&after_a_seek, Duration::from_secs(2)));
    }
}
