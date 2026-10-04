use std::{sync::Arc, time::Duration};

use gpui::{Context, Image, ImageFormat, Task};
use resonate_listen::{
    Error as ListenError, Heard, Hearing, Listener, Listening, PictureFormat, Recognisers,
};

const PROGRESS_EVERY: Duration = Duration::from_millis(100);
const HEARD_KEPT: usize = 8;

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

    #[cfg(test)]
    pub(crate) fn recording_without_a_device(&mut self) {
        self.stage = Stage::Recording {
            hearing: Hearing::new(),
            from: self.from.clone(),
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
            self.stage = Stage::Offline;
            cx.notify();
            return;
        }
        let listens = self.listens.clone();
        if listens.recognisers.is_empty() {
            self.stage = Stage::NoService;
            cx.notify();
            return;
        }

        let hearing = Hearing::new();
        let from = self.from.clone();
        self.stage = Stage::Recording {
            hearing: Arc::clone(&hearing),
            from: from.clone(),
        };
        let length = self.length;
        let recognisers = Arc::clone(&listens.recognisers);
        let heard_through = Arc::clone(&hearing);
        let recorded = cx
            .background_executor()
            .spawn(async move { listens.listener.record(&from, length, &heard_through) });
        self.running = cx.spawn(async move |this, cx| {
            let clip = recorded.await;
            let clip = match clip {
                Ok(clip) => clip,
                Err(error) => {
                    let _ = this.update(cx, |this, cx| this.fell_through(&error, cx));
                    return;
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.stage = Stage::Asking;
                cx.notify();
            });
            let recognition = cx
                .background_executor()
                .spawn(async move { recognisers.recognise(&clip) })
                .await;
            let _ = this.update(cx, |this, cx| match recognition {
                Ok(recognition) => match recognition.heard {
                    Some(heard) => this.landed(heard, cx),
                    None if recognition.refused.is_empty() => {
                        this.stage = Stage::Unknown;
                        cx.notify();
                    }
                    None => {
                        this.stage = Stage::Unreached;
                        cx.notify();
                    }
                },
                Err(error) => this.fell_through(&error, cx),
            });
        });
        self.tick(cx);
        cx.notify();
    }

    pub(crate) fn stop(&mut self, cx: &mut Context<Self>) {
        if let Stage::Recording { hearing, .. } = &self.stage {
            hearing.stop();
        }
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
                let recording = this.update(cx, |this, cx| {
                    let recording = matches!(this.stage, Stage::Recording { .. });
                    if recording {
                        cx.notify();
                    }
                    recording
                });
                if !matches!(recording, Ok(true)) {
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

    fn fell_through(&mut self, error: &ListenError, cx: &mut Context<Self>) {
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
        self.stage = stage_after(error);
        cx.notify();
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
}
