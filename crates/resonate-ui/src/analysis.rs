use std::{cell::Cell, num::NonZeroUsize, rc::Rc, sync::Arc, time::Duration};

use gpui::{Bounds, Context, Image, Pixels, Task};
use resonate_core::{FrameSpan, MediaLocation};
use resonate_engine::{Analysis, AnalysisError, Player, Reach, Watch};
use resonate_library::{Agreement, Fingerprinters, HeardAs, Library, Sounded, Studied};

use crate::{
    analysis_plot::{SPECTRUM_COLUMNS, WAVEFORM_COLUMNS, lanes_of, ramp_through, traced},
    models::{Forget, painted},
    recent::Recent,
};

const ANALYSES_KEPT: NonZeroUsize = NonZeroUsize::new(6).expect("six is not zero");
const RECOGNITIONS_KEPT: NonZeroUsize = NonZeroUsize::new(64).expect("sixty-four is not zero");
const PROGRESS_EVERY: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Row {
    pub(crate) location: MediaLocation,
    pub(crate) span: Option<FrameSpan>,
}

pub(crate) struct Drawn {
    pub(crate) analysis: Analysis,
    pub(crate) lanes: Vec<Vec<Reach>>,
    pub(crate) spectrum: Vec<f32>,
}

impl Drawn {
    fn of(analysis: Analysis) -> Self {
        Self {
            lanes: lanes_of(&analysis.envelope, WAVEFORM_COLUMNS),
            spectrum: traced(&analysis.study.spectrum, SPECTRUM_COLUMNS),
            analysis,
        }
    }
}

#[derive(Clone)]
pub(crate) enum Studying {
    Idle,
    Running(Arc<Watch>),
    Done(Arc<Drawn>),
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Hearing {
    Unasked,
    Unserved,
    Unprinted,
    Asking,
    Refused,
    Heard {
        matches: Arc<[HeardAs]>,
        agreement: Option<Agreement>,
    },
}

impl Hearing {
    fn recalled(studied: &Studied) -> Option<Self> {
        studied.recognised?;
        Some(Self::Heard {
            matches: studied.heard_as.iter().cloned().collect(),
            agreement: studied.agreement,
        })
    }
}

struct Painted {
    row: Row,
    stops: Vec<u32>,
    image: Arc<Image>,
}

pub(crate) struct AnalysisModel {
    player: Arc<Player>,
    library: Arc<Library>,
    fingerprinters: Arc<Fingerprinters>,
    following: Option<Row>,
    studying: Studying,
    hearing: Hearing,
    kept: Recent<Row, Arc<Drawn>>,
    heard: Recent<Row, Hearing>,
    painted: Option<Painted>,
    painting: Option<(Row, Vec<u32>)>,
    pub(crate) plotted: Rc<Cell<Bounds<Pixels>>>,
    running: Task<()>,
    ticking: Task<()>,
    asking: Task<()>,
    drawing: Task<()>,
}

impl AnalysisModel {
    pub(crate) fn new(
        player: Arc<Player>,
        library: Arc<Library>,
        fingerprinters: Arc<Fingerprinters>,
    ) -> Self {
        Self {
            player,
            library,
            fingerprinters,
            following: None,
            studying: Studying::Idle,
            hearing: Hearing::Unasked,
            kept: Recent::new(ANALYSES_KEPT),
            heard: Recent::new(RECOGNITIONS_KEPT),
            painted: None,
            painting: None,
            plotted: Rc::default(),
            running: Task::ready(()),
            ticking: Task::ready(()),
            asking: Task::ready(()),
            drawing: Task::ready(()),
        }
    }

    pub(crate) fn studying(&self) -> Studying {
        self.studying.clone()
    }

    pub(crate) fn hearing(&self) -> Hearing {
        self.hearing.clone()
    }

    pub(crate) fn recognises(&self) -> bool {
        self.fingerprinters.has_a_source()
    }

    pub(crate) fn follow(&mut self, row: Row, cx: &mut Context<Self>) {
        if self.following.as_ref() == Some(&row) {
            return;
        }
        if let Studying::Running(watch) = &self.studying {
            watch.stop();
        }
        self.following = Some(row.clone());
        self.hearing = self.heard.get(&row).cloned().unwrap_or(Hearing::Unasked);
        self.asking = Task::ready(());

        if let Some(drawn) = self.kept.get(&row).cloned() {
            self.studying = Studying::Done(Arc::clone(&drawn));
            self.hear(row, drawn, cx);
            return;
        }

        self.recall(row.clone(), cx);
        let watch = Arc::new(Watch::default());
        self.studying = Studying::Running(Arc::clone(&watch));
        let player = Arc::clone(&self.player);
        let asked = row.clone();
        let studied = cx.background_executor().spawn(async move {
            player
                .analyse(&asked.location, asked.span, watch.as_ref())
                .map(|analysis| Arc::new(Drawn::of(analysis)))
        });
        self.running = cx.spawn(async move |this, cx| {
            let studied = studied.await;
            let _ = this.update(cx, |this, cx| this.landed(row, studied, cx));
        });
        self.tick(cx);
        cx.notify();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        self.ticking = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PROGRESS_EVERY).await;
                let still_running = this.update(cx, |this, cx| {
                    let running = matches!(this.studying, Studying::Running(_));
                    if running {
                        cx.notify();
                    }
                    running
                });
                if !matches!(still_running, Ok(true)) {
                    return;
                }
            }
        });
    }

    fn landed(
        &mut self,
        row: Row,
        studied: Result<Arc<Drawn>, AnalysisError>,
        cx: &mut Context<Self>,
    ) {
        match studied {
            Ok(drawn) => {
                self.kept.insert(row.clone(), Arc::clone(&drawn));
                if self.following.as_ref() == Some(&row) {
                    self.studying = Studying::Done(Arc::clone(&drawn));
                    self.hear(row, drawn, cx);
                }
            }
            Err(AnalysisError::Stopped) => {}
            Err(error) => {
                tracing::warn!(%error, location = %row.location, "the playing track could not be analysed");
                if self.following.as_ref() == Some(&row) {
                    self.studying = Studying::Failed;
                }
            }
        }
        cx.notify();
    }

    fn recall(&mut self, row: Row, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        let asked = row.clone();
        let recalled = cx.background_executor().spawn(async move {
            match library.study_of(&asked.location, asked.span) {
                Ok(held) => held.and_then(|(_, studied)| Hearing::recalled(&studied)),
                Err(error) => {
                    tracing::debug!(%error, "a study could not be read back");
                    None
                }
            }
        });
        self.asking = cx.spawn(async move |this, cx| {
            let Some(recalled) = recalled.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.following.as_ref() == Some(&row) && this.hearing == Hearing::Unasked {
                    this.heard.insert(row, recalled.clone());
                    this.hearing = recalled;
                    cx.notify();
                }
            });
        });
    }

    fn hear(&mut self, row: Row, drawn: Arc<Drawn>, cx: &mut Context<Self>) {
        if !matches!(self.hearing, Hearing::Unasked) {
            return;
        }
        self.hearing = Hearing::Asking;
        let library = Arc::clone(&self.library);
        let fingerprinters = Arc::clone(&self.fingerprinters);
        let asked = row.clone();
        let heard = cx
            .background_executor()
            .spawn(async move { settled(&library, &fingerprinters, &asked, &drawn.analysis) });
        self.asking = cx.spawn(async move |this, cx| {
            let heard = heard.await;
            let _ = this.update(cx, |this, cx| {
                this.heard.insert(row.clone(), heard.clone());
                if this.following.as_ref() == Some(&row) {
                    this.hearing = heard;
                    cx.notify();
                }
            });
        });
    }

    pub(crate) fn spectrogram(
        &mut self,
        stops: Vec<u32>,
        cx: &mut Context<Self>,
    ) -> Option<Arc<Image>> {
        let row = self.following.clone()?;
        if let Some(painted) = &self.painted
            && painted.row == row
            && painted.stops == stops
        {
            return Some(Arc::clone(&painted.image));
        }
        let Studying::Done(drawn) = &self.studying else {
            return None;
        };
        let wanted = (row.clone(), stops.clone());
        if self.painting.as_ref() == Some(&wanted) {
            return self
                .painted
                .as_ref()
                .map(|painted| Arc::clone(&painted.image));
        }

        self.painting = Some(wanted);
        let drawn = Arc::clone(drawn);
        let ramp = ramp_through(&stops);
        let image = cx.background_executor().spawn(async move {
            match drawn.analysis.spectrogram.painted(&ramp) {
                Ok(art) => Some(Arc::new(Image::from_bytes(painted(art.format), art.bytes))),
                Err(error) => {
                    tracing::debug!(%error, "the spectrogram could not be painted");
                    None
                }
            }
        });
        self.drawing = cx.spawn(async move |this, cx| {
            let image = image.await;
            let _ = this.update(cx, |this, cx| {
                this.painting = None;
                if let Some(image) = image {
                    this.painted
                        .replace(Painted { row, stops, image })
                        .map(|painted| painted.image)
                        .forget(cx);
                    cx.notify();
                }
            });
        });
        None
    }
}

fn settled(
    library: &Library,
    fingerprinters: &Fingerprinters,
    row: &Row,
    analysis: &Analysis,
) -> Hearing {
    let held = library
        .track_held(&row.location, row.span)
        .unwrap_or_else(|error| {
            tracing::debug!(%error, "whether the catalog holds the row could not be read");
            None
        });
    let stored = library
        .study_of(&row.location, row.span)
        .unwrap_or_else(|error| {
            tracing::debug!(%error, "a study could not be read back");
            None
        });
    if let Some(recalled) = stored
        .as_ref()
        .and_then(|(_, studied)| Hearing::recalled(studied))
    {
        return recalled;
    }
    if let (Some(track), None) = (held, stored.as_ref())
        && let Err(error) = library.note_study(track, &analysis.study)
    {
        tracing::warn!(%error, %track, "the pane's study was not kept");
    }

    if !fingerprinters.has_a_source() {
        return Hearing::Unserved;
    }
    let Some(print) = analysis.study.print.clone() else {
        return Hearing::Unprinted;
    };

    match held {
        Some(track) => match library.recognise(fingerprinters, track, print) {
            Some(heard) if heard.refused => Hearing::Refused,
            Some(heard) => Hearing::Heard {
                matches: heard.matches.iter().map(HeardAs::of).collect(),
                agreement: Some(heard.agreement),
            },
            None => Hearing::Refused,
        },
        None => {
            let recognition = fingerprinters.recognise(&Sounded {
                location: row.location.clone(),
                span: row.span,
                length: Some(analysis.study.examined.length),
                title: None,
                artist: None,
                print,
            });
            if recognition.refused && recognition.matches.is_empty() {
                return Hearing::Refused;
            }
            Hearing::Heard {
                matches: recognition.matches.iter().map(HeardAs::of).collect(),
                agreement: None,
            }
        }
    }
}
