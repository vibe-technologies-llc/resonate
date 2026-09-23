use std::{
    collections::HashMap,
    mem::ManuallyDrop,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    thread,
};

use resonate_core::MediaLocation;
use resonate_engine::{Command, TagSet};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::Value,
};

use crate::{BusOp, Error, Host, Result, interfaces::Shared};

const SERVICE: &str = "org.freedesktop.Notifications";
const OBJECT_PATH: &str = "/org/freedesktop/Notifications";
const BODY_MARKUP: &str = "body-markup";
const TAKES_ACTIONS: &str = "actions";
const ACTION_INVOKED: &str = "ActionInvoked";
const PREVIOUS: &str = "previous";
const PLAY_PAUSE: &str = "play-pause";
const NEXT: &str = "next";
const ACTIONS: [&str; 6] = [PREVIOUS, "Previous", PLAY_PAUSE, "Play/Pause", NEXT, "Next"];
const NOTHING_TO_REPLACE: u32 = 0;
const SERVER_TIMEOUT: i32 = -1;
const LOW_URGENCY: u8 = 0;
const NO_ACTIONS: [&str; 0] = [];
const NO_ICON: &str = "";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shown {
    summary: String,
    body: String,
    art: Option<String>,
}

impl Shown {
    pub(crate) const fn told(summary: String, body: String, art: Option<String>) -> Self {
        Self { summary, body, art }
    }
}

pub(crate) fn shown(tags: &TagSet, location: &MediaLocation, art: Option<String>) -> Option<Shown> {
    let summary = tags
        .title
        .clone()
        .or_else(|| location.stem().map(|stem| stem.into_owned()))?;

    Some(Shown {
        summary,
        body: underneath(tags),
        art,
    })
}

fn underneath(tags: &TagSet) -> String {
    match (tags.artist.as_deref(), tags.album.as_deref()) {
        (Some(artist), Some(album)) => format!("{artist}\n{album}"),
        (Some(alone), None) | (None, Some(alone)) => alone.to_owned(),
        (None, None) => String::new(),
    }
}

pub(crate) struct Notifier {
    proxy: Proxy<'static>,
    application: String,
    entry: Option<String>,
    markup: bool,
    actions: bool,
    raised: Arc<AtomicU32>,
    told: AtomicU32,
}

impl Notifier {
    pub(crate) fn start(connection: &Connection, host: &dyn Host) -> Option<Self> {
        match Self::new(connection, host) {
            Ok(notifier) => Some(notifier),
            Err(error) => {
                tracing::warn!(%error, "a track change will not be told to the desktop");
                None
            }
        }
    }

    fn new(connection: &Connection, host: &dyn Host) -> Result<Self> {
        let proxy = Proxy::new(connection, SERVICE, OBJECT_PATH, SERVICE)
            .map_err(|source| Error::bus(BusOp::Connect, source))?;
        let capabilities = capabilities(&proxy);

        Ok(Self {
            proxy,
            application: host.identity(),
            entry: host.desktop_entry(),
            markup: declares(&capabilities, BODY_MARKUP),
            actions: declares(&capabilities, TAKES_ACTIONS),
            raised: Arc::new(AtomicU32::new(NOTHING_TO_REPLACE)),
            told: AtomicU32::new(NOTHING_TO_REPLACE),
        })
    }

    pub(crate) fn listen(&self, shared: &Arc<Shared>) {
        if !self.actions {
            return;
        }
        let proxy = self.proxy.clone();
        let raised = Arc::clone(&self.raised);
        let shared = Arc::clone(shared);

        let listening = thread::Builder::new()
            .name("resonate-presses".to_owned())
            .spawn(move || answer(&proxy, &raised, &shared));

        if listening.is_err() {
            tracing::warn!("what a notification offers will not reach the transport");
        }
    }

    pub(crate) fn show(&self, shown: &Shown) {
        self.notify(shown, &self.raised, self.offered(), true);
    }

    pub(crate) fn tell(&self, shown: &Shown) {
        self.notify(shown, &self.told, &NO_ACTIONS, false);
    }

    fn notify(&self, shown: &Shown, slot: &AtomicU32, actions: &[&str], transient: bool) {
        let body = if self.markup {
            escaped(&shown.body)
        } else {
            shown.body.clone()
        };
        let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
        hints.insert("urgency", Value::U8(LOW_URGENCY));
        hints.insert("transient", Value::Bool(transient));
        if let Some(entry) = self.entry.as_deref() {
            hints.insert("desktop-entry", Value::from(entry));
        }
        if let Some(art) = shown.art.as_deref() {
            hints.insert("image-path", Value::from(art));
        }

        let told = self.proxy.call::<_, _, u32>(
            "Notify",
            &(
                self.application.as_str(),
                slot.load(Ordering::Acquire),
                self.entry.as_deref().unwrap_or(NO_ICON),
                shown.summary.as_str(),
                body.as_str(),
                actions,
                hints,
                SERVER_TIMEOUT,
            ),
        );

        match told {
            Ok(raised) => slot.store(raised, Ordering::Release),
            Err(source) => {
                let error = Error::bus(BusOp::Notify, source);
                tracing::debug!(%error, "a track change did not reach the desktop");
            }
        }
    }

    const fn offered(&self) -> &[&str] {
        if self.actions { &ACTIONS } else { &NO_ACTIONS }
    }

    pub(crate) fn hush(&self) {
        let raised = self.raised.swap(NOTHING_TO_REPLACE, Ordering::AcqRel);
        if raised == NOTHING_TO_REPLACE {
            return;
        }
        if let Err(source) = self.proxy.call::<_, _, ()>("CloseNotification", &(raised,)) {
            let error = Error::bus(BusOp::Shutdown, source);
            tracing::debug!(%error, "a notification outlived the player that raised it");
        }
    }
}

fn answer(proxy: &Proxy<'static>, raised: &AtomicU32, shared: &Shared) {
    let presses = match proxy.receive_signal(ACTION_INVOKED) {
        Ok(presses) => presses,
        Err(source) => {
            let error = Error::bus(BusOp::Listen, source);
            tracing::warn!(%error, "what a notification offers will not reach the transport");
            return;
        }
    };
    let mut presses = ManuallyDrop::new(presses);

    for press in presses.by_ref() {
        let body = press.body();
        let Ok((notification, key)) = body.deserialize::<(u32, String)>() else {
            continue;
        };
        if notification != raised.load(Ordering::Acquire) {
            continue;
        }
        if let Some(command) = pressed(&key) {
            let _ = shared.settle(command);
        }
    }
}

fn pressed(key: &str) -> Option<Command> {
    match key {
        PREVIOUS => Some(Command::Previous),
        PLAY_PAUSE => Some(Command::TogglePlayPause),
        NEXT => Some(Command::Next),
        _ => None,
    }
}

fn capabilities(proxy: &Proxy<'_>) -> Vec<String> {
    match proxy.call::<_, _, Vec<String>>("GetCapabilities", &()) {
        Ok(capabilities) => capabilities,
        Err(source) => {
            let error = Error::bus(BusOp::Notify, source);
            tracing::debug!(%error, "the desktop did not say what a notification may carry");
            Vec::new()
        }
    }
}

fn declares(capabilities: &[String], capability: &str) -> bool {
    capabilities.iter().any(|declared| declared == capability)
}

fn escaped(body: &str) -> String {
    let mut held = String::with_capacity(body.len());
    for character in body.chars() {
        match character {
            '&' => held.push_str("&amp;"),
            '<' => held.push_str("&lt;"),
            '>' => held.push_str("&gt;"),
            other => held.push(other),
        }
    }
    held
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tagged(title: Option<&str>, artist: Option<&str>, album: Option<&str>) -> TagSet {
        TagSet {
            title: title.map(str::to_owned),
            artist: artist.map(str::to_owned),
            album: album.map(str::to_owned),
            ..TagSet::default()
        }
    }

    fn echoes() -> MediaLocation {
        MediaLocation::local("/music/Pink Floyd/Echoes.flac")
    }

    #[test]
    fn a_track_is_told_as_its_title_over_the_artist_and_the_album() {
        let tags = tagged(Some("Echoes"), Some("Pink Floyd"), Some("Meddle"));
        let shown = shown(&tags, &echoes(), Some("file:///tmp/cover.png".to_owned()))
            .expect("a titled track is worth telling");

        assert_eq!(shown.summary, "Echoes");
        assert_eq!(shown.body, "Pink Floyd\nMeddle");
        assert_eq!(shown.art.as_deref(), Some("file:///tmp/cover.png"));
    }

    #[test]
    fn a_track_carrying_no_title_is_told_as_the_file_it_came_out_of() {
        let shown = shown(&TagSet::default(), &echoes(), None).expect("a file names itself");

        assert_eq!(shown.summary, "Echoes");
        assert_eq!(shown.body, "");
    }

    #[test]
    fn one_line_is_told_where_one_of_the_artist_and_the_album_is_all_there_is() {
        let artist = shown(
            &tagged(Some("Echoes"), Some("Pink Floyd"), None),
            &echoes(),
            None,
        )
        .expect("a titled track");
        let album = shown(
            &tagged(Some("Echoes"), None, Some("Meddle")),
            &echoes(),
            None,
        )
        .expect("a titled track");

        assert_eq!(artist.body, "Pink Floyd");
        assert_eq!(album.body, "Meddle");
    }

    #[test]
    fn a_body_is_escaped_for_a_server_that_reads_it_as_markup() {
        assert_eq!(
            escaped("Simon & Garfunkel <live>"),
            "Simon &amp; Garfunkel &lt;live&gt;"
        );
        assert_eq!(escaped("Pink Floyd"), "Pink Floyd");
    }

    #[test]
    fn each_button_a_notification_offers_is_a_transport_command() {
        let keys: Vec<&str> = ACTIONS.iter().step_by(2).copied().collect();

        assert_eq!(keys, vec![PREVIOUS, PLAY_PAUSE, NEXT]);
        assert_eq!(pressed(PREVIOUS), Some(Command::Previous));
        assert_eq!(pressed(PLAY_PAUSE), Some(Command::TogglePlayPause));
        assert_eq!(pressed(NEXT), Some(Command::Next));
        assert_eq!(pressed("default"), None);
    }

    #[test]
    fn a_label_is_carried_beside_every_key_the_desktop_may_press() {
        assert_eq!(ACTIONS.len() % 2, 0);
        for label in ACTIONS.iter().skip(1).step_by(2) {
            assert!(!label.is_empty());
        }
    }

    #[test]
    fn buttons_are_offered_only_where_the_server_says_it_draws_them() {
        let capabilities = ["body".to_owned(), TAKES_ACTIONS.to_owned()];

        assert!(declares(&capabilities, TAKES_ACTIONS));
        assert!(!declares(&capabilities, BODY_MARKUP));
        assert!(!declares(&[], TAKES_ACTIONS));
    }
}
