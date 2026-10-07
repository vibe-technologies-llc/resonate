use std::{sync::Arc, time::Duration};

use gpui::{Context, Image, ImageFormat, Task};
use resonate_core::folded_letters;
use resonate_listen::{
    Error as ListenError, Heard, Hearing, Listener, Listening, PictureFormat, Recognisers,
    Recognition,
};

use crate::ResonateApp;

const PROGRESS_EVERY: Duration = Duration::from_millis(100);
const HEARD_KEPT: usize = 8;
pub(crate) const TRIES_ON_A_MISS: u8 = 3;
pub(crate) const FOLLOWED_EVERY: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub struct Listens {
    pub listener: Listener,
    pub recognisers: Arc<Recognisers>,
    pub tell: Arc<dyn Fn(&Heard) + Send + Sync>,
    pub from: Listening,
    pub length: Duration,
}

#[derive(Clone)]
pub(crate) struct Found {
    pub(crate) heard: Arc<Heard>,
    pub(crate) picture: Option<Arc<Image>>,
}

#[derive(Clone)]
pub(crate) enum Stage {
    Idle,
    Recording {
        hearing: Arc<Hearing>,
        from: Listening,
        nth: u8,
    },
    Asking,
    Found(Found),
    Unknown,
    Silent,
    CaptureFailed,
    Unreached,
    Refused,
    Offline,
    NoService,
}

#[derive(Clone)]
pub(crate) enum Following {
    Off,
    Waiting,
    Listening { hearing: Arc<Hearing> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Purpose {
    Naming { nth: u8 },
    Following,
}

#[derive(Debug)]
enum Answer {
    Named(Heard),
    Missed,
    Fell(ListenError),
}

impl Answer {
    fn of(recognition: Result<Recognition, ListenError>) -> Self {
        match recognition {
            Ok(Recognition {
                heard: Some(heard), ..
            }) => Self::Named(heard),
            Ok(Recognition { refused, .. }) if refused.is_empty() => Self::Missed,
            Ok(Recognition { refused, .. }) => Self::Fell(ListenError::Unreachable {
                service: refused.into_iter().next().expect("a refusal was counted"),
            }),
            Err(error) => Self::Fell(error),
        }
    }
}

#[derive(Debug)]
enum Then {
    Tell(Heard),
    ListenAgain { nth: u8 },
    Settle(Stage),
    KeepWhatWasNamed,
}

impl std::fmt::Debug for Stage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Idle => "Idle",
            Self::Recording { .. } => "Recording",
            Self::Asking => "Asking",
            Self::Found(_) => "Found",
            Self::Unknown => "Unknown",
            Self::Silent => "Silent",
            Self::CaptureFailed => "CaptureFailed",
            Self::Unreached => "Unreached",
            Self::Refused => "Refused",
            Self::Offline => "Offline",
            Self::NoService => "NoService",
        })
    }
}

fn then(purpose: Purpose, answer: Answer, standing: Option<&Heard>) -> Then {
    match (purpose, answer) {
        (Purpose::Following, Answer::Named(heard))
            if standing.is_some_and(|standing| names_the_same_song(standing, &heard)) =>
        {
            Then::KeepWhatWasNamed
        }
        (_, Answer::Named(heard)) => Then::Tell(heard),
        (Purpose::Following, Answer::Missed | Answer::Fell(_)) => Then::KeepWhatWasNamed,
        (Purpose::Naming { nth }, Answer::Missed | Answer::Fell(ListenError::NothingHeard))
            if nth < TRIES_ON_A_MISS =>
        {
            Then::ListenAgain { nth: nth + 1 }
        }
        (Purpose::Naming { .. }, Answer::Missed) => Then::Settle(Stage::Unknown),
        (Purpose::Naming { .. }, Answer::Fell(error)) => Then::Settle(stage_after(&error)),
    }
}

fn names_the_same_song(one: &Heard, other: &Heard) -> bool {
    let folded = |text: Option<&str>| folded_letters(text.unwrap_or_default());
    folded(Some(&one.title)) == folded(Some(&other.title))
        && folded(one.artist.as_deref()) == folded(other.artist.as_deref())
}

fn stage_after(error: &ListenError) -> Stage {
    match error {
        ListenError::Stopped => Stage::Idle,
        ListenError::NothingHeard => Stage::Silent,
        ListenError::Capture { .. } => Stage::CaptureFailed,
        ListenError::Unreachable { .. } => Stage::Unreached,
        ListenError::Refused { .. }
        | ListenError::Unreadable { .. }
        | ListenError::TooLarge { .. } => Stage::Refused,
    }
}

pub(crate) struct ListenModel {
    listens: Listens,
    from: Listening,
    length: Duration,
    stage: Stage,
    heard: Vec<Found>,
    microphones: Vec<(String, String)>,
    keeps_listening: bool,
    following: Following,
    running: Task<()>,
    ticking: Task<()>,
    listing: Task<()>,
}

impl ListenModel {
    pub(crate) fn new(listens: Listens) -> Self {
        Self {
            from: listens.from.clone(),
            length: listens.length,
            listens,
            stage: Stage::Idle,
            heard: Vec::new(),
            microphones: Vec::new(),
            keeps_listening: false,
            following: Following::Off,
            running: Task::ready(()),
            ticking: Task::ready(()),
            listing: Task::ready(()),
        }
    }

    pub(crate) fn stage(&self) -> Stage {
        self.stage.clone()
    }

    pub(crate) fn from(&self) -> &Listening {
        &self.from
    }

    pub(crate) const fn length(&self) -> Duration {
        self.length
    }

    pub(crate) fn heard(&self) -> &[Found] {
        &self.heard
    }

    pub(crate) fn microphones(&self) -> &[(String, String)] {
        &self.microphones
    }

    pub(crate) const fn keeps_listening(&self) -> bool {
        self.keeps_listening
    }

    pub(crate) fn following(&self) -> Following {
        self.following.clone()
    }

    #[cfg(test)]
    pub(crate) fn recording_without_a_device(&mut self) {
        self.stage = Stage::Recording {
            hearing: Hearing::new(),
            from: self.from.clone(),
            nth: 1,
        };
    }

    #[cfg(test)]
    pub(crate) fn hearing_of(&mut self, microphones: Vec<(String, String)>) {
        self.microphones = microphones;
        self.listing = Task::ready(());
    }

    pub(crate) const fn is_listening(&self) -> bool {
        matches!(self.stage, Stage::Recording { .. } | Stage::Asking)
    }

    pub(crate) fn choose(&mut self, from: Listening, cx: &mut Context<Self>) {
        self.from = from;
        cx.notify();
    }

    pub(crate) fn listen_for(&mut self, length: Duration, cx: &mut Context<Self>) {
        self.length = length;
        cx.notify();
    }

    pub(crate) fn keep_listening(&mut self, keeps: bool, cx: &mut Context<Self>) {
        self.keeps_listening = keeps;
        if !keeps {
            self.stop_following();
        } else if matches!(self.stage, Stage::Found(_) | Stage::Unknown) {
            self.follow_after(FOLLOWED_EVERY, cx);
        }
        cx.notify();
    }

    pub(crate) fn look_for_microphones(&mut self, cx: &mut Context<Self>) {
        let listener = self.listens.listener.clone();
        let found = cx
            .background_executor()
            .spawn(async move { listener.microphones() });
        self.listing = cx.spawn(async move |this, cx| {
            let found = match found.await {
                Ok(found) => found,
                Err(error) => {
                    tracing::debug!(%error, "the microphones could not be listed");
                    return;
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.microphones = found
                    .into_iter()
                    .map(|microphone| (microphone.name.to_string(), microphone.description))
                    .collect();
                cx.notify();
            });
        });
    }

    pub(crate) fn listen(&mut self, online: bool, cx: &mut Context<Self>) {
        if self.is_listening() {
            return;
        }
        if !online {
            self.stop_following();
            self.stage = Stage::Offline;
            cx.notify();
            return;
        }
        if self.listens.recognisers.is_empty() {
            self.stop_following();
            self.stage = Stage::NoService;
            cx.notify();
            return;
        }
        self.stop_following();
        self.record_and_ask(Purpose::Naming { nth: 1 }, cx);
    }

    fn record_and_ask(&mut self, purpose: Purpose, cx: &mut Context<Self>) {
        let listens = self.listens.clone();
        let hearing = Hearing::new();
        let from = self.from.clone();
        match purpose {
            Purpose::Naming { nth } => {
                self.stage = Stage::Recording {
                    hearing: Arc::clone(&hearing),
                    from: from.clone(),
                    nth,
                };
            }
            Purpose::Following => {
                self.following = Following::Listening {
                    hearing: Arc::clone(&hearing),
                };
            }
        }
        let length = self.length;
        let recognisers = Arc::clone(&listens.recognisers);
        let heard_through = Arc::clone(&hearing);
        let recorded = cx
            .background_executor()
            .spawn(async move { listens.listener.record(&from, length, &heard_through) });
        self.running = cx.spawn(async move |this, cx| {
            let clip = match recorded.await {
                Ok(clip) => clip,
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        this.answered(purpose, Answer::Fell(error), cx);
                    });
                    return;
                }
            };
            if matches!(purpose, Purpose::Naming { .. }) {
                let _ = this.update(cx, |this, cx| {
                    this.stage = Stage::Asking;
                    cx.notify();
                });
            }
            let recognition = cx
                .background_executor()
                .spawn(async move { recognisers.recognise(&clip) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.answered(purpose, Answer::of(recognition), cx);
            });
        });
        self.tick(cx);
        cx.notify();
    }

    fn answered(&mut self, purpose: Purpose, answer: Answer, cx: &mut Context<Self>) {
        if let Answer::Fell(error) = &answer {
            noted(error);
        }
        let standing = match &self.stage {
            Stage::Found(found) => Some(Arc::clone(&found.heard)),
            _ => None,
        };
        match then(purpose, answer, standing.as_deref()) {
            Then::Tell(heard) => self.landed(heard, cx),
            Then::ListenAgain { nth } => {
                if online(cx) {
                    self.record_and_ask(Purpose::Naming { nth }, cx);
                    return;
                }
                self.stage = Stage::Offline;
            }
            Then::Settle(stage) => self.stage = stage,
            Then::KeepWhatWasNamed => {}
        }
        if matches!(self.stage, Stage::Found(_) | Stage::Unknown) && self.keeps_listening {
            self.follow_after(FOLLOWED_EVERY, cx);
        } else {
            self.following = Following::Off;
        }
        cx.notify();
    }

    fn follow_after(&mut self, wait: Duration, cx: &mut Context<Self>) {
        self.following = Following::Waiting;
        self.running = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |this, cx| {
                if !this.keeps_listening || !online(cx) {
                    this.following = Following::Off;
                    cx.notify();
                    return;
                }
                this.record_and_ask(Purpose::Following, cx);
            });
        });
    }

    fn stop_following(&mut self) {
        if let Following::Listening { hearing } = &self.following {
            hearing.stop();
        }
        if !matches!(self.following, Following::Off) {
            self.running = Task::ready(());
        }
        self.following = Following::Off;
    }

    pub(crate) fn stop(&mut self, cx: &mut Context<Self>) {
        if let Stage::Recording { hearing, .. } = &self.stage {
            hearing.stop();
        }
        self.stop_following();
        if self.is_listening() {
            self.running = Task::ready(());
            self.stage = Stage::Idle;
        }
        cx.notify();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        self.ticking = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PROGRESS_EVERY).await;
                let moving = this.update(cx, |this, cx| {
                    let moving = matches!(this.stage, Stage::Recording { .. })
                        || matches!(this.following, Following::Listening { .. });
                    if moving {
                        cx.notify();
                    }
                    moving
                });
                if !matches!(moving, Ok(true)) {
                    return;
                }
            }
        });
    }

    fn landed(&mut self, heard: Heard, cx: &mut Context<Self>) {
        (self.listens.tell)(&heard);
        let picture = heard.picture.as_ref().map(|picture| {
            let format = match picture.format {
                PictureFormat::Jpeg => ImageFormat::Jpeg,
                PictureFormat::Png => ImageFormat::Png,
            };
            Arc::new(Image::from_bytes(format, picture.bytes.clone()))
        });
        let found = Found {
            heard: Arc::new(heard),
            picture,
        };
        self.heard.insert(0, found.clone());
        self.heard.truncate(HEARD_KEPT);
        self.stage = Stage::Found(found);
        cx.notify();
    }
}

fn online(cx: &Context<ListenModel>) -> bool {
    cx.try_global::<ResonateApp>()
        .is_none_or(|app| app.online.enabled)
}

fn noted(error: &ListenError) {
    match error {
        ListenError::Capture { .. } => {
            tracing::warn!(%error, "nothing could be listened to");
        }
        ListenError::Unreachable { .. }
        | ListenError::Refused { .. }
        | ListenError::Unreadable { .. }
        | ListenError::TooLarge { .. } => {
            tracing::warn!(%error, "what was heard could not be named");
        }
        ListenError::Stopped | ListenError::NothingHeard => {}
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::SourceId;

    use super::*;

    fn service() -> SourceId {
        SourceId::new("shazam").expect("a source name")
    }

    #[test]
    fn a_service_that_refused_the_clip_is_not_one_that_could_not_be_reached() {
        let refused = ListenError::Refused {
            service: service(),
            status: 400,
        };
        let unreadable = ListenError::Unreadable { service: service() };
        let too_large = ListenError::TooLarge { service: service() };
        let unreachable = ListenError::Unreachable { service: service() };

        assert!(matches!(stage_after(&refused), Stage::Refused));
        assert!(matches!(stage_after(&unreadable), Stage::Refused));
        assert!(matches!(stage_after(&too_large), Stage::Refused));
        assert!(matches!(stage_after(&unreachable), Stage::Unreached));
    }

    #[test]
    fn a_stopped_listening_is_idle_and_a_silent_one_is_silent() {
        assert!(matches!(stage_after(&ListenError::Stopped), Stage::Idle));
        assert!(matches!(
            stage_after(&ListenError::NothingHeard),
            Stage::Silent
        ));
    }

    fn heard(title: &str, artist: &str) -> Heard {
        Heard {
            title: title.to_owned(),
            artist: Some(artist.to_owned()),
            album: None,
            year: None,
            isrc: None,
            recording: None,
            picture: None,
            link: None,
            by: service(),
        }
    }

    #[test]
    fn a_miss_or_a_silent_clip_is_listened_to_again_until_the_tries_run_out() {
        assert!(matches!(
            then(Purpose::Naming { nth: 1 }, Answer::Missed, None),
            Then::ListenAgain { nth: 2 }
        ));
        assert!(matches!(
            then(
                Purpose::Naming { nth: 2 },
                Answer::Fell(ListenError::NothingHeard),
                None
            ),
            Then::ListenAgain { nth: 3 }
        ));
        assert!(matches!(
            then(
                Purpose::Naming {
                    nth: TRIES_ON_A_MISS
                },
                Answer::Missed,
                None
            ),
            Then::Settle(Stage::Unknown)
        ));
        assert!(matches!(
            then(
                Purpose::Naming {
                    nth: TRIES_ON_A_MISS
                },
                Answer::Fell(ListenError::NothingHeard),
                None
            ),
            Then::Settle(Stage::Silent)
        ));
    }

    #[test]
    fn a_service_out_of_reach_is_said_at_once_rather_than_asked_again() {
        assert!(matches!(
            then(
                Purpose::Naming { nth: 1 },
                Answer::Fell(ListenError::Unreachable { service: service() }),
                None
            ),
            Then::Settle(Stage::Unreached)
        ));
        assert!(matches!(
            then(
                Purpose::Naming { nth: 1 },
                Answer::of(Ok(Recognition {
                    heard: None,
                    refused: vec![service()],
                })),
                None
            ),
            Then::Settle(Stage::Unreached)
        ));
    }

    #[test]
    fn following_tells_a_new_song_and_keeps_quiet_about_the_same_one_or_a_miss() {
        let standing = heard("Echoes", "Pink Floyd");

        assert!(matches!(
            then(
                Purpose::Following,
                Answer::Named(heard("ECHOES", "pink floyd")),
                Some(&standing)
            ),
            Then::KeepWhatWasNamed
        ));
        assert!(matches!(
            then(
                Purpose::Following,
                Answer::Named(heard("Dogs", "Pink Floyd")),
                Some(&standing)
            ),
            Then::Tell(told) if told.title == "Dogs"
        ));
        assert!(matches!(
            then(Purpose::Following, Answer::Missed, Some(&standing)),
            Then::KeepWhatWasNamed
        ));
        assert!(matches!(
            then(
                Purpose::Following,
                Answer::Fell(ListenError::NothingHeard),
                Some(&standing)
            ),
            Then::KeepWhatWasNamed
        ));
        assert!(matches!(
            then(
                Purpose::Following,
                Answer::Named(heard("Dogs", "Pink Floyd")),
                None
            ),
            Then::Tell(_)
        ));
    }
}
