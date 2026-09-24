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
    Recording(Arc<Hearing>),
    Asking,
    Found(Found),
    Unknown,
    Silent,
    Unreached,
    NoService,
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

    pub(crate) const fn is_listening(&self) -> bool {
        matches!(self.stage, Stage::Recording(_) | Stage::Asking)
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

    pub(crate) fn listen(&mut self, cx: &mut Context<Self>) {
        if self.is_listening() {
            return;
        }
        let listens = self.listens.clone();
        if listens.recognisers.is_empty() {
            self.stage = Stage::NoService;
            cx.notify();
            return;
        }

        let hearing = Hearing::new();
        self.stage = Stage::Recording(Arc::clone(&hearing));
        let from = self.from.clone();
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
        if let Stage::Recording(hearing) = &self.stage {
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
                    let recording = matches!(this.stage, Stage::Recording(_));
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
        self.stage = match error {
            ListenError::Stopped => Stage::Idle,
            ListenError::NothingHeard => Stage::Silent,
            ListenError::Capture { .. } => {
                tracing::warn!(%error, "nothing could be listened to");
                Stage::Silent
            }
            ListenError::Unreachable { .. }
            | ListenError::Refused { .. }
            | ListenError::Unreadable { .. }
            | ListenError::TooLarge { .. } => {
                tracing::warn!(%error, "what was heard could not be named");
                Stage::Unreached
            }
        };
        cx.notify();
    }
}
