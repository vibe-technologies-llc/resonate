use std::{cell::Cell, cmp::Ordering, rc::Rc, sync::Arc, time::Duration};

use ahash::{AHashMap, AHashSet};
use gpui::{
    AnyElement, App, ClickEvent, Context, Div, EventEmitter, Pixels, SharedString, Task,
    UniformListScrollHandle, div, prelude::*, px, rgb, uniform_list,
};
use resonate_core::{AlbumId, FrameSpan, MediaLocation, Span, TrackId};
use resonate_engine::{Command, Placement, Player, QueueItem, QueueRows, Queued};
use resonate_library::{
    CatalogStamp, Cut, Direction, Favoured, Library, Lit, NamesMoved, RowOrder, Track,
    folded_letters,
};
use smallvec::{SmallVec, smallvec};

use crate::{
    Notice, ResonateApp, Selection, format,
    icons::Icon,
    models::queued_rows,
    theme,
    views::{
        browser::ROW_CONTROLS,
        hint,
        kit::{self, EndsInAnEllipsis, Tone},
        listing::{self, Pictured},
        menu::{self, Called, Menu},
        playlists::{Held, ROW_GROUP},
        reorder::{self, Carried, CarriedOver, Chosen, LiftedTo, Shift, Step},
        root::{Following, PlayingRow, RootView, empty, row},
        scrollbar::Scrollbars,
        sorting::{self, Ordered},
        spoken::{self, Placed, Spoken},
    },
    words,
};

#[derive(Default)]
struct QueueMeasure {
    held: Option<QueueLength>,
    asked: Option<QueueLength>,
    reading: Option<Task<()>>,
}

#[derive(Default)]
struct QueueNames {
    named: Option<(NamedAt, Arc<[Named]>)>,
    asked: Option<NamedAt>,
    reading: Option<Task<()>>,
}

pub(crate) struct QueuePane {
    rows: UniformListScrollHandle,
    height: Rc<Cell<Pixels>>,
    order: RowOrder,
    reading: Direction,
    following: Following,
    took_out: TakenBack,
    length: QueueMeasure,
    names: QueueNames,
    ordering: Task<()>,
}

pub(crate) struct NamesRead;

pub(crate) struct PutInOrder {
    revision: u64,
    rows: Vec<usize>,
}

impl EventEmitter<NamesRead> for QueuePane {}

impl EventEmitter<PutInOrder> for QueuePane {}

struct Over {
    queued: Queued,
    player: Arc<Player>,
    library: Arc<Library>,
}

impl Over {
    fn named_at(&self) -> NamedAt {
        NamedAt {
            queue: self.queued.revision,
            names: self.library.names_stamp(),
            media: self.player.media_revision(),
        }
    }
}

impl QueuePane {
    pub(crate) fn new() -> Self {
        Self {
            rows: UniformListScrollHandle::default(),
            height: Rc::default(),
            order: RowOrder::default(),
            reading: Direction::default(),
            following: Following::default(),
            took_out: TakenBack::default(),
            length: QueueMeasure::default(),
            names: QueueNames::default(),
            ordering: Task::ready(()),
        }
    }

    pub(crate) fn rows(&self) -> UniformListScrollHandle {
        self.rows.clone()
    }

    pub(crate) const fn order(&self) -> (RowOrder, Direction) {
        (self.order, self.reading)
    }

    pub(crate) fn follows(&mut self, playing: Option<PlayingRow>, shown: bool) -> Option<usize> {
        self.following.follows(playing, shown)
    }

    pub(crate) fn follow_afresh(&mut self) {
        self.following = Following::default();
    }

    pub(crate) fn keeping_all(&mut self, queue: &QueueRows, runs: &[Span]) -> Option<usize> {
        self.took_out.keeping_all(queue, runs)
    }

    pub(crate) fn another_zone_shown(&mut self) {
        self.names = QueueNames::default();
        self.took_out = TakenBack::default();
    }

    fn names(&mut self, over: Over, cx: &mut Context<Self>) -> Option<Arc<[Named]>> {
        let revision = over.named_at();
        let held = self
            .names
            .named
            .as_ref()
            .map(|(named, names)| (*named, Arc::clone(names)));
        if held.as_ref().is_some_and(|(named, _)| {
            named.queue == revision.queue && named.names_still_hold_at(&revision)
        }) {
            return held.map(|(_, names)| names);
        }
        self.name(revision, over, cx);
        held.filter(|(named, _)| named.queue == revision.queue)
            .map(|(_, names)| names)
    }

    fn name(&mut self, revision: NamedAt, over: Over, cx: &mut Context<Self>) {
        if self.names.asked == Some(revision) {
            return;
        }
        self.names.asked = Some(revision);
        let Over {
            queued,
            player,
            library,
        } = over;
        let standing = self
            .names
            .named
            .as_ref()
            .filter(|(named, _)| named.media == revision.media)
            .map(|(named, names)| Standing {
                names: Arc::clone(names),
                moved: library.names_moved_since(named.names),
            });

        self.names.reading = Some(cx.spawn(async move |this, cx| {
            let names: Arc<[Named]> = cx
                .background_executor()
                .spawn(async move {
                    named_again(
                        &library,
                        &player,
                        queued.rows.iter().map(Arc::as_ref),
                        standing,
                    )
                })
                .await;
            let landed = this.update(cx, |this, cx| {
                this.names.named = Some((revision, names));
                cx.emit(NamesRead);
                cx.notify();
            });
            let _ = landed;
        }));
    }

    fn length(&mut self, over: Over, cx: &mut Context<Self>) -> Option<(Duration, usize)> {
        let measured = QueueLength {
            queue: over.queued.revision,
            rows: over.library.rows_stamp(),
            total: Duration::ZERO,
            unmeasured: 0,
        };
        if let Some(held) = self.length.held
            && held.measures(measured)
        {
            return Some((held.total, held.unmeasured));
        }

        self.measure(measured, over, cx);
        self.length.held.map(|held| (held.total, held.unmeasured))
    }

    fn measure(&mut self, measured: QueueLength, over: Over, cx: &mut Context<Self>) {
        if self
            .length
            .asked
            .is_some_and(|asked| asked.measures(measured))
        {
            return;
        }
        self.length.asked = Some(measured);
        let Over {
            queued, library, ..
        } = over;

        self.length.reading = Some(cx.spawn(async move |this, cx| {
            let (total, unmeasured) = cx
                .background_executor()
                .spawn(async move {
                    let rows = queued_rows(&library, queued.rows.iter().map(Arc::as_ref));
                    let lengths: Vec<Option<Duration>> = rows
                        .iter()
                        .map(|track| {
                            let track = track.as_ref()?;
                            track
                                .duration
                                .map(|frames| frames.to_duration(track.spec.rate))
                        })
                        .collect();
                    let unmeasured = lengths.iter().filter(|length| length.is_none()).count();
                    (lengths.into_iter().flatten().sum::<Duration>(), unmeasured)
                })
                .await;
            let landed = this.update(cx, |this, cx| {
                this.length.held = Some(QueueLength {
                    total,
                    unmeasured,
                    ..measured
                });
                cx.notify();
            });
            let _ = landed;
        }));
    }

    fn put_in_order(
        &mut self,
        order: RowOrder,
        reading: Direction,
        over: Over,
        cx: &mut Context<Self>,
    ) {
        self.order = order;
        self.reading = reading;

        let Over {
            queued,
            player,
            library,
        } = over;
        if queued.rows.len() < 2 {
            return;
        }

        self.ordering = cx.spawn(async move |this, cx| {
            let revision = queued.revision;
            let rows = queued.rows.clone();
            let ordered: Vec<usize> = cx
                .background_executor()
                .spawn(async move { ordered_rows(&library, &player, &rows, order, reading) })
                .await;
            let landed = this.update(cx, |_, cx| {
                cx.emit(PutInOrder {
                    revision,
                    rows: ordered,
                });
            });
            let _ = landed;
        });
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Named {
    item: QueueItem,
    track: Option<TrackId>,
    folded: String,
}

impl AsRef<str> for Named {
    fn as_ref(&self) -> &str {
        &self.folded
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NamedAt {
    queue: u64,
    names: CatalogStamp,
    media: u64,
}

impl NamedAt {
    fn names_still_hold_at(&self, now: &Self) -> bool {
        self.names.still_holds_at(now.names) && self.media == now.media
    }
}

impl RootView {
    fn queue_over(&self, cx: &App) -> Over {
        let player = self.player.read(cx);
        Over {
            queued: player.queued(),
            player: player.engine(),
            library: self.library.read(cx).catalog(),
        }
    }

    pub(crate) fn names_in_the_queue(&mut self, cx: &mut Context<Self>) -> Option<Arc<[Named]>> {
        let over = self.queue_over(cx);
        self.queue.update(cx, |queue, cx| queue.names(over, cx))
    }

    pub(crate) fn queue_put_in_order(&self, ordered: &PutInOrder, cx: &mut Context<Self>) {
        if !self.player.read(cx).is_queued_at(ordered.revision) {
            return;
        }
        self.send_by_position_at(ordered.revision, Command::Order(ordered.rows.clone()), cx);
        cx.notify();
    }
}

struct Standing {
    names: Arc<[Named]>,
    moved: NamesMoved,
}

fn named_again<'a>(
    library: &Library,
    player: &Player,
    rows: impl IntoIterator<Item = &'a QueueItem>,
    standing: Option<Standing>,
) -> Arc<[Named]> {
    let rows: Vec<&QueueItem> = rows.into_iter().collect();
    let known: AHashMap<&QueueItem, &Named> = standing
        .iter()
        .flat_map(|standing| {
            standing
                .names
                .iter()
                .filter(|named| !standing.moved.names(named.track))
        })
        .map(|named| (&named.item, named))
        .collect();
    let unknown: Vec<QueueItem> = rows
        .iter()
        .filter(|item| !known.contains_key(*item))
        .map(|item| QueueItem::clone(item))
        .collect();
    let read: AHashMap<&QueueItem, Named> = queued_rows(library, &unknown)
        .into_iter()
        .zip(unknown.iter())
        .map(|(track, item)| {
            let catalog = track.as_ref().map(|track| track.id);
            let named = Named {
                item: item.clone(),
                track: catalog,
                folded: folded_letters(&queued_name(track, player, item)),
            };
            (item, named)
        })
        .collect();

    rows.into_iter()
        .map(|item| {
            known
                .get(item)
                .map(|named| (*named).clone())
                .or_else(|| read.get(item).cloned())
                .unwrap_or_else(|| Named {
                    item: item.clone(),
                    track: None,
                    folded: String::new(),
                })
        })
        .collect()
}

fn queued_name(track: Option<Track>, player: &Player, item: &QueueItem) -> String {
    match track {
        Some(track) => track.title,
        None => player
            .media(&item.location, item.span)
            .and_then(|info| info.tags.title.clone())
            .unwrap_or_else(|| format::stem(&item.location)),
    }
}

fn queued_cut(item: &QueueItem) -> Cut {
    Cut {
        location: item.location.clone(),
        span: item.span,
    }
}

const HANDOFFS_OFFERED: usize = 2;

const KEPT_GESTURES: usize = 16;

const HEARD_FADED: f32 = 0.55;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Heard,
    Playing,
    Next,
    Rest,
}

impl Part {
    fn of(row: usize, playing: Option<usize>, next: Option<Span>) -> Self {
        match playing {
            Some(at) if row < at => Self::Heard,
            Some(at) if row == at => Self::Playing,
            _ if next.is_some_and(|next| next.holds(row)) => Self::Next,
            _ => Self::Rest,
        }
    }

    fn named(self) -> &'static str {
        match self {
            Self::Heard => words::queue::history(),
            Self::Playing => words::queue::now_playing(),
            Self::Next => words::queue::playing_next(),
            Self::Rest => words::queue::continue_playing(),
        }
    }

    const fn is_counted(self) -> bool {
        !matches!(self, Self::Playing)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Run {
    part: Part,
    rows: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Line {
    Heading(Run),
    Row(usize),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct QueueParts {
    rows: usize,
    playing: Option<usize>,
    next: Option<Span>,
    runs: SmallVec<[Run; 5]>,
}

impl QueueParts {
    fn of(rows: usize, playing: Option<usize>, next: Option<Span>) -> Self {
        let playing = playing.filter(|at| *at < rows);
        let mut edges: SmallVec<[usize; 6]> = smallvec![0, rows];
        edges.extend(playing.into_iter().flat_map(|at| [at, at + 1]));
        edges.extend(
            next.into_iter()
                .flat_map(|next| [next.first(), next.last() + 1]),
        );
        edges.retain(|edge| *edge <= rows);
        edges.sort_unstable();
        edges.dedup();

        let mut runs: SmallVec<[Run; 5]> = SmallVec::new();
        for pair in edges.windows(2) {
            let &[first, end] = pair else {
                continue;
            };
            let part = Part::of(first, playing, next);
            match runs.last_mut() {
                Some(run) if run.part == part => {
                    run.rows = Span::between(run.rows.first(), end - 1)
                }
                _ => runs.push(Run {
                    part,
                    rows: Span::between(first, end - 1),
                }),
            }
        }
        if runs.len() < 2 {
            runs.clear();
        }

        Self {
            rows,
            playing,
            next,
            runs,
        }
    }

    fn lines(&self) -> usize {
        self.rows + self.runs.len()
    }

    fn line(&self, item: usize) -> Option<Line> {
        if self.runs.is_empty() {
            return (item < self.rows).then_some(Line::Row(item));
        }

        let mut headed = 0;
        for run in &self.runs {
            if item == run.rows.first() + headed {
                return Some(Line::Heading(*run));
            }
            headed += 1;
            if item <= run.rows.last() + headed {
                return Some(Line::Row(item - headed));
            }
        }
        None
    }

    pub(crate) fn opens_at(&self) -> Option<usize> {
        let line = self.line_of(self.playing?);
        match self.runs.is_empty() {
            true => Some(line),
            false => line.checked_sub(1),
        }
    }

    fn room_below(&self, shown: usize) -> usize {
        match self.opens_at() {
            Some(top) if top > 0 => shown.saturating_sub(self.lines() - top),
            Some(_) | None => 0,
        }
    }

    pub(crate) fn line_of(&self, row: usize) -> usize {
        row + self
            .runs
            .iter()
            .filter(|run| run.rows.first() <= row)
            .count()
    }

    fn part_of(&self, row: usize) -> Part {
        Part::of(row, self.playing, self.next)
    }
}

pub(crate) struct TakenOut {
    rows: Vec<QueueItem>,
    at: usize,
    after: Option<TrackId>,
}

impl TakenOut {
    fn of(queue: &QueueRows, rows: Span) -> Option<Self> {
        let taken = queue.run(rows)?;
        if taken.is_empty() {
            return None;
        }

        Some(Self {
            rows: taken.iter().map(|row| QueueItem::clone(row)).collect(),
            at: rows.first(),
            after: rows
                .first()
                .checked_sub(1)
                .and_then(|before| queue.get(before))
                .map(|item| item.id),
        })
    }

    fn stands_over(&self, queue: &QueueRows) -> bool {
        let taken: AHashSet<TrackId> = self.rows.iter().map(|taken| taken.id).collect();
        queue.iter().all(|item| !taken.contains(&item.id))
    }

    fn landing_in(&self, queue: &QueueRows) -> usize {
        match self.after {
            None => 0,
            Some(after) => queue
                .iter()
                .position(|item| item.id == after)
                .map_or(self.at.min(queue.len()), |before| before + 1),
        }
    }

    fn standing_in(&self, queue: &QueueRows) -> Option<Span> {
        let first = self.rows.first()?;
        let at = queue.iter().position(|item| item.id == first.id)?;
        let held = queue.run(Span::between(at, at + self.rows.len() - 1))?;
        held.iter()
            .map(|item| item.id)
            .eq(self.rows.iter().map(|item| item.id))
            .then(|| Span::between(at, at + self.rows.len() - 1))
    }
}

pub(crate) struct PuttingBack {
    rows: Vec<QueueItem>,
    at: usize,
}

#[derive(Default)]
pub(crate) struct TakenBack {
    steps: Vec<TakenOut>,
    put_back: Vec<TakenOut>,
}

impl TakenBack {
    pub(crate) fn keeping_all(&mut self, queue: &QueueRows, runs: &[Span]) -> Option<usize> {
        let taken: Vec<TakenOut> = runs
            .iter()
            .filter_map(|rows| TakenOut::of(queue, *rows))
            .collect();
        if taken.is_empty() {
            return None;
        }

        let kept = taken.iter().map(|taken| taken.rows.len()).sum();
        self.put_back.clear();
        for taken in taken {
            self.stack(taken);
        }
        Some(kept)
    }

    fn stack(&mut self, taken: TakenOut) {
        if self.steps.len() == KEPT_GESTURES {
            self.steps.remove(0);
        }
        self.steps.push(taken);
    }

    fn standing(&self, queue: &QueueRows) -> Option<&TakenOut> {
        self.steps.last().filter(|taken| taken.stands_over(queue))
    }

    fn standing_again(&self, queue: &QueueRows) -> Option<&TakenOut> {
        self.put_back
            .last()
            .filter(|taken| taken.standing_in(queue).is_some())
    }

    fn offered(&self, queue: &QueueRows) -> Option<Offer> {
        self.standing(queue).map(|taken| Offer {
            rows: taken.rows.len(),
            behind: self.steps.len().saturating_sub(1),
        })
    }

    fn offered_again(&self, queue: &QueueRows) -> Option<Offer> {
        self.standing_again(queue).map(|taken| Offer {
            rows: taken.rows.len(),
            behind: self.put_back.len().saturating_sub(1),
        })
    }

    pub(crate) fn take(&mut self, queue: &QueueRows) -> Option<PuttingBack> {
        self.standing(queue)?;
        let taken = self.steps.pop()?;
        let putting = PuttingBack {
            rows: taken.rows.clone(),
            at: taken.landing_in(queue),
        };
        self.put_back.push(taken);
        Some(putting)
    }

    pub(crate) fn take_again(&mut self, queue: &QueueRows) -> Option<Span> {
        let rows = self.standing_again(queue)?.standing_in(queue)?;
        let taken = self.put_back.pop()?;
        self.stack(taken);
        Some(rows)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Offer {
    rows: usize,
    behind: usize,
}

struct Reaching {
    rows: Span,
    holding: Arc<[Cut]>,
}

impl Reaching {
    fn of(queue: &QueueRows, rows: Span) -> Self {
        Self {
            rows,
            holding: queue
                .run(rows)
                .unwrap_or_default()
                .iter()
                .map(|item| queued_cut(item))
                .collect(),
        }
    }

    fn acting_on(reaching: Option<&Self>, rows: Span) -> Option<&Self> {
        reaching.filter(|reaching| reaching.rows == rows)
    }
}

impl RootView {
    pub(crate) fn queue_parts(&self, cx: &App) -> QueueParts {
        let player = self.player.read(cx);
        QueueParts::of(
            player.queue().len(),
            player.state().queue_position,
            player.queued().next,
        )
    }

    fn playing_from(&self, cx: &App) -> Option<SharedString> {
        let playlist = self.playing_playlist(cx)?;
        let library = self.library.read(cx);
        library
            .saved_playlists()
            .iter()
            .chain(library.lists().iter())
            .find(|held| held.id == playlist)
            .map(|held| SharedString::from(held.name.clone()))
    }

    pub(crate) fn queue_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let _ = self.names_in_the_queue(cx);
        let queue = self.player.read(cx).queue();
        if queue.is_empty() {
            let nothing = empty(
                Icon::Queue,
                words::queue::empty(),
                Some(words::queue::how_to_fill()),
            );
            let heading = self
                .holds_what_was_taken_out(&queue, cx)
                .then(|| self.queue_heading(&queue, cx));

            return reorder::takes_a_lift(
                div()
                    .id("empty-queue")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0)),
                LiftedTo::Queue(Placement::Queued),
                cx,
            )
            .children(heading)
            .child(nothing)
            .into_any_element();
        }

        let parts = self.queue_parts(cx);
        let from = self.playing_from(cx);
        let queued = queue.len();
        let (scroll, measured) = {
            let pane = self.queue.read(cx);
            (pane.rows(), Rc::clone(&pane.height))
        };
        let height = measured.get();
        let shown = (f32::from(height) / theme::row_height()) as usize;
        let lines = parts.lines() + parts.room_below(shown);
        let heading = self.queue_heading(&queue, cx);
        let drawn_at = self.player.read(cx).queued().revision;
        let reaching = self
            .reaching(Shift::Queue, cx)
            .map(|rows| Reaching::of(&queue, rows));

        kit::pane()
            .child(heading)
            .child(listing::columns(
                "",
                true,
                ROW_CONTROLS,
                sorting::queue_sorted(self, cx),
                &self.rooms.columns_fit,
                &self.controls,
                cx,
            ))
            .child(
                reorder::follows_a_drag(
                    reorder::takes_a_lift(
                        div()
                            .id("queue-rows")
                            .relative()
                            .flex()
                            .flex_1()
                            .min_h(px(0.0)),
                        LiftedTo::Queue(Placement::Queued),
                        cx,
                    ),
                    scroll.clone(),
                    lines,
                    cx,
                )
                .child(kit::measures_its_height(measured))
                .when(height > px(0.0), |rows| {
                    rows.child(
                        uniform_list(
                            "queue",
                            lines,
                            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                let mut rows = Vec::new();
                                for drawn in range {
                                    let index = match parts.line(drawn) {
                                        Some(Line::Row(index)) => index,
                                        Some(Line::Heading(run)) => {
                                            rows.push(part_heading(drawn, run, from.clone()));
                                            continue;
                                        }
                                        None => {
                                            rows.push(
                                                div().h(px(theme::row_height())).into_any_element(),
                                            );
                                            continue;
                                        }
                                    };
                                    let Some(item) = queue.get(index) else {
                                        continue;
                                    };
                                    let track = this
                                        .library
                                        .update(cx, |library, _| library.track_of(item));
                                    let part = parts.part_of(index);
                                    let current = part == Part::Playing;
                                    let waiting = part == Part::Next;
                                    let heard = part == Part::Heard;
                                    let drawn = match track {
                                        Some(track) => {
                                            let favourite =
                                                this.library.read(cx).favours_track(&track);
                                            listing::scanned(track, favourite)
                                        }
                                        None => this
                                            .player
                                            .read(cx)
                                            .media(&item.location, item.span)
                                            .map_or_else(
                                                || listing::unread(&item.location),
                                                |info| listing::read(&info, &item.location),
                                            ),
                                    };

                                    let cover = this.cover(
                                        Pictured::Track {
                                            album: drawn.album,
                                            file: &item.location,
                                        },
                                        cx,
                                    );
                                    let reached = this.reaches(Shift::Queue, index, cx);
                                    let acting_on = this.acting_on(Shift::Queue, index, cx);
                                    let holding: Arc<[Cut]> =
                                        match Reaching::acting_on(reaching.as_ref(), acting_on) {
                                            Some(reaching) => Arc::clone(&reaching.holding),
                                            None => Arc::from([queued_cut(item)]),
                                        };
                                    let carried = Carried {
                                        shift: Shift::Queue,
                                        rows: Chosen::of(acting_on),
                                        title: drawn.title.clone(),
                                        over: CarriedOver::Queue { drawn_at },
                                    };
                                    let album = drawn.album;
                                    let artist_id = drawn.artist_id;
                                    let scanned = drawn.track;
                                    let favourite = drawn.favourite;
                                    let location = item.location.clone();
                                    let span = item.span;
                                    let title = drawn.title.clone();
                                    let artist = drawn.artist.clone();
                                    let menued = Arc::clone(&holding);
                                    let queued_as = item.id.get();
                                    let fitted = this.rooms.columns_fit.shown();
                                    let listed = row(current)
                                        .id(("queued", queued_as))
                                        .debug_selector(move || format!("queued-{queued_as}"))
                                        .listed_as(|| {
                                            spoken::song(&drawn.title, Some(&drawn.artist))
                                        })
                                        .placed(Placed {
                                            at: index,
                                            of: queue.len(),
                                        })
                                        .chosen(reached)
                                        .group(ROW_GROUP)
                                        .cursor_pointer()
                                        .when(heard, |entry| entry.opacity(HEARD_FADED))
                                        .hover(move |entry| {
                                            let entry = entry.bg(rgb(theme::hover()));
                                            if heard { entry.opacity(1.0) } else { entry }
                                        })
                                        .child(if current {
                                            listing::playing_mark()
                                        } else if waiting {
                                            listing::playing_next_mark()
                                        } else {
                                            listing::number_cell(SharedString::from(
                                                (index + 1).to_string(),
                                            ))
                                        })
                                        .child(cover)
                                        .child(listing::title_cell(
                                            drawn.title,
                                            Lit::new(),
                                            current,
                                        ))
                                        .child(listing::artist_cell(
                                            this.opens(
                                                ("queue-artist", index),
                                                drawn.artist,
                                                words::browser::open_artist_hint(),
                                                drawn.artist_id.map(Selection::Artist),
                                                cx,
                                            )
                                            .flex_shrink(1.0)
                                            .ends_in_an_ellipsis(),
                                        ))
                                        .when(fitted.format, |row| {
                                            row.child(listing::format_cell(drawn.shape))
                                        })
                                        .when(fitted.heard, |row| {
                                            row.child(listing::heard(
                                                drawn.plays,
                                                drawn.played,
                                                this.drawn_at(),
                                            ))
                                        })
                                        .child(listing::length_cell(drawn.length))
                                        .child(
                                            this.row_controls(("queue-controls", index), cx)
                                                .child(this.mover(
                                                    "raise",
                                                    Step::Above,
                                                    index,
                                                    queued,
                                                    Shift::Queue,
                                                    cx,
                                                ))
                                                .child(this.mover(
                                                    "lower",
                                                    Step::Below,
                                                    index,
                                                    queued,
                                                    Shift::Queue,
                                                    cx,
                                                ))
                                                .child(this.add_control(
                                                    ("queue-to-playlist", index),
                                                    if acting_on.is_one_row() {
                                                        words::playlists::add_hint()
                                                    } else {
                                                        words::playlists::add_reached_hint()
                                                    },
                                                    move || Held::of(Arc::clone(&holding)),
                                                    cx,
                                                ))
                                                .child(this.in_the_pane_ring(
                                                    kit::icon_button(
                                                        ("remove", index),
                                                        Icon::Close,
                                                        if acting_on.is_one_row() {
                                                            words::queue::remove_hint()
                                                        } else {
                                                            words::queue::remove_reached_hint()
                                                        },
                                                    ),
                                                    move |this, _, cx| {
                                                        cx.stop_propagation();
                                                        if this
                                                            .player
                                                            .read(cx)
                                                            .is_queued_at(drawn_at)
                                                        {
                                                            this.drop_rows(
                                                                Shift::Queue,
                                                                acting_on,
                                                                cx,
                                                            );
                                                        }
                                                    },
                                                    cx,
                                                )),
                                        )
                                        .on_click(cx.listener(
                                            move |this, event: &ClickEvent, _, cx| {
                                                let extending = event.modifiers().shift;
                                                this.reach_at(Shift::Queue, index, extending, cx);
                                                if !extending {
                                                    this.send_by_position_at(
                                                        drawn_at,
                                                        Command::JumpTo(index),
                                                        cx,
                                                    );
                                                }
                                            },
                                        ));
                                    let reach = this.at_the_reach(Shift::Queue, index, cx);
                                    let listed = menu::opens_a_reachable_menu(
                                        listed,
                                        reach,
                                        move |this, at, cx| {
                                            let taken = this.acting_on(Shift::Queue, index, cx);
                                            let put = Arc::clone(&menued);
                                            let names = Called {
                                                title: title.clone(),
                                                artist: artist.clone(),
                                                album: this.album_named(album, &location, span, cx),
                                            };

                                            Menu::at(at)
                                                .under(
                                                    Icon::Play,
                                                    words::menu::play(),
                                                    "enter",
                                                    move |this, _, cx| {
                                                        this.send_by_position_at(
                                                            drawn_at,
                                                            Command::JumpTo(index),
                                                            cx,
                                                        );
                                                    },
                                                )
                                                .apart()
                                                .holds(move || Held::of(Arc::clone(&put)))
                                                .when_some(scanned, |menu, track| {
                                                    menu.favours(Favoured::Track(track), favourite)
                                                })
                                                .reaches(scanned, album, artist_id)
                                                .apart()
                                                .when_some(scanned, Menu::shares)
                                                .offers_the_file(&location, names)
                                                .apart()
                                                .under(
                                                    Icon::Discard,
                                                    words::menu::take_out(),
                                                    "delete",
                                                    move |this, _, cx| {
                                                        if this
                                                            .player
                                                            .read(cx)
                                                            .is_queued_at(drawn_at)
                                                        {
                                                            this.drop_rows(Shift::Queue, taken, cx);
                                                        }
                                                    },
                                                )
                                        },
                                        cx,
                                    );

                                    rows.push(
                                        reorder::movable(listed, index, carried, reached, cx)
                                            .into_any_element(),
                                    );
                                }
                                rows
                            }),
                        )
                        .track_scroll(&scroll)
                        .h_full()
                        .w_full(),
                    )
                })
                .child(Scrollbars::of(cx).vertical("queue-scrollbar", scroll)),
            )
            .into_any_element()
    }

    fn holds_what_was_taken_out(&self, queue: &QueueRows, cx: &App) -> bool {
        self.queue.read(cx).took_out.offered(queue).is_some()
    }

    fn queue_length(&mut self, cx: &mut Context<Self>) -> Option<(Duration, usize)> {
        let over = self.queue_over(cx);
        self.queue.update(cx, |queue, cx| queue.length(over, cx))
    }

    fn queue_heading(&mut self, queue: &QueueRows, cx: &mut Context<Self>) -> Div {
        let position = self.player.read(cx).state().queue_position;
        let total = self.queue_length(cx);
        let mut under: format::Parts<String> = smallvec![words::queue::tracks(queue.len())];
        if let Some((total, unmeasured)) = total.filter(|(total, _)| !total.is_zero()) {
            under.push(if unmeasured > 0 {
                words::queue::at_least(format::spanned(total))
            } else {
                format::spanned(total)
            });
        }
        if let Some(position) = position {
            under.push(words::queue::on_row(position + 1));
        }
        let waiting = waiting_to_play(self.player.read(cx).queued().next, position);
        if waiting > 0 {
            under.push(words::queue::waiting(waiting));
        }

        let (taken_out, put_back) = {
            let took_out = &self.queue.read(cx).took_out;
            (took_out.offered(queue), took_out.offered_again(queue))
        };
        let queued = !queue.is_empty();
        let handed_off = cx
            .global::<ResonateApp>()
            .syncing
            .as_ref()
            .map(|syncing| syncing.handed_off())
            .unwrap_or_default();
        let continuing = handed_off
            .into_iter()
            .take(HANDOFFS_OFFERED)
            .enumerate()
            .map(|(at, handed)| {
                let device = handed.device.clone();
                self.in_the_pane_ring(
                    kit::button(
                        ("continue-from", at),
                        Some(Icon::Play),
                        words::queue::continue_from(handed.device.as_str()),
                        words::queue::continue_from_hint(handed.device.as_str(), handed.songs),
                        Tone::Ghost,
                    ),
                    move |this, _, cx| this.continue_from(&device, cx),
                    cx,
                )
            })
            .collect::<Vec<_>>();

        kit::heading().child(
            kit::heading_row()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(theme::heading_name()))
                        .gap_1()
                        .child(kit::title(words::queue::queue()))
                        .child(kit::subtitle(under.join(" · "))),
                )
                .child(
                    kit::actions()
                        .children(continuing)
                        .child(hint::explains("queue-moving", reorder::moving_hint()))
                        .when(queued, |bar| {
                            bar.child(self.orders_a_listing("order-queue", Ordered::Queue, cx))
                                .child(self.in_the_pane_ring(
                                    kit::button(
                                        "clear-queue",
                                        Some(Icon::Discard),
                                        words::queue::clear(),
                                        words::queue::clear_hint(),
                                        Tone::Ghost,
                                    ),
                                    |this, _, cx| {
                                        let queued = this.player.read(cx).queue();
                                        let Some(last) = queued.len().checked_sub(1) else {
                                            return;
                                        };
                                        this.drop_rows(Shift::Queue, Span::between(0, last), cx);
                                    },
                                    cx,
                                ))
                                .child(self.in_the_pane_ring(
                                    kit::button(
                                        "queue-to-playlist-all",
                                        Some(Icon::Plus),
                                        words::queue::save_as_a_playlist(),
                                        words::queue::queued_hint(),
                                        Tone::Outlined,
                                    ),
                                    |this, window, cx| {
                                        let queued: Arc<[Cut]> = this
                                            .player
                                            .read(cx)
                                            .queue()
                                            .iter()
                                            .map(|item| queued_cut(item))
                                            .collect();
                                        this.hold_for_a_playlist(Held::of(queued), window, cx);
                                    },
                                    cx,
                                ))
                        })
                        .when(taken_out.is_some(), |bar| {
                            bar.child(self.in_the_pane_ring(
                                kit::button(
                                    "put-the-queue-back",
                                    Some(Icon::Undo),
                                    words::queue::put_back(),
                                    puts_back(taken_out),
                                    Tone::Ghost,
                                ),
                                |this, _, cx| {
                                    this.put_the_queue_back(cx);
                                },
                                cx,
                            ))
                        })
                        .when(put_back.is_some(), |bar| {
                            bar.child(self.in_the_pane_ring(
                                kit::button(
                                    "take-the-queue-out-again",
                                    Some(Icon::Redo),
                                    words::queue::take_out_again(),
                                    takes_out_again(put_back),
                                    Tone::Ghost,
                                ),
                                |this, _, cx| {
                                    this.take_the_queue_out_again(cx);
                                },
                                cx,
                            ))
                        }),
                ),
        )
    }

    pub(crate) fn put_the_queue_back(&mut self, cx: &mut Context<Self>) -> bool {
        let queued = self.player.read(cx).queued();
        let Some(taken) = self
            .queue
            .update(cx, |queue, _| queue.took_out.take(&queued.rows))
        else {
            return false;
        };
        let items = self.with_the_versions_chosen(taken.rows, cx);
        self.send_by_position_at(
            queued.revision,
            Command::Insert {
                items,
                at: Placement::At(taken.at),
                play: false,
            },
            cx,
        );
        true
    }

    pub(crate) fn is_in_the_queue(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
        cx: &App,
    ) -> bool {
        !self.rows_of_in_the_queue(location, span, cx).is_empty()
    }

    pub(crate) fn take_out_of_the_queue(
        &mut self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
        cx: &mut Context<Self>,
    ) {
        let rows = self.rows_of_in_the_queue(location, span, cx);
        let runs = runs_of(&rows);
        if !runs.is_empty() {
            self.drop_queue_runs(runs, cx);
        }
    }

    fn rows_of_in_the_queue(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
        cx: &App,
    ) -> Vec<usize> {
        self.player
            .read(cx)
            .queue()
            .iter()
            .enumerate()
            .filter(|(_, item)| &item.location == location && item.span == span)
            .map(|(row, _)| row)
            .collect()
    }

    pub(crate) fn take_the_queue_out_again(&mut self, cx: &mut Context<Self>) -> bool {
        let queued = self.player.read(cx).queue();
        let Some(rows) = self
            .queue
            .update(cx, |queue, _| queue.took_out.take_again(&queued))
        else {
            return false;
        };
        self.send_by_position(Command::Remove(rows), cx);
        true
    }

    pub(crate) fn queue_in_order(&self, cx: &mut Context<Self>) -> Div {
        let (order, reading) = self.queue.read(cx).order();
        self.order_choices(
            order,
            reading,
            |this, order, cx| this.put_the_queue_in_order(order, Direction::Ascending, cx),
            |this, reading, cx| {
                let (order, _) = this.queue.read(cx).order();
                this.put_the_queue_in_order(order, reading, cx);
            },
            cx,
        )
    }

    pub(crate) fn put_the_queue_in_order(
        &mut self,
        order: RowOrder,
        reading: Direction,
        cx: &mut Context<Self>,
    ) {
        let over = self.queue_over(cx);
        self.queue.update(cx, |queue, cx| {
            queue.put_in_order(order, reading, over, cx);
        });
        cx.notify();
    }
}

fn ordered_rows(
    library: &Library,
    player: &Player,
    rows: &QueueRows,
    order: RowOrder,
    reading: Direction,
) -> Vec<usize> {
    let tracks = queued_rows(library, rows.iter().map(Arc::as_ref));
    let albums: Vec<AlbumId> = tracks
        .iter()
        .flatten()
        .filter_map(|track| track.album_id)
        .collect::<AHashSet<_>>()
        .into_iter()
        .collect();
    let titles: AHashMap<AlbumId, String> = match library.album_titles(&albums) {
        Ok(titles) => titles.into_iter().collect(),
        Err(error) => {
            tracing::warn!(%error, "the queued albums could not be read");
            AHashMap::new()
        }
    };

    let mut keyed: Vec<Keyed> = rows
        .iter()
        .zip(tracks)
        .enumerate()
        .map(|(row, (item, track))| Keyed::of(row, item, track, &titles, player))
        .collect();
    keyed.sort_by(|left, right| {
        let weighed = left.weighed(order, right);
        match reading {
            Direction::Ascending => weighed,
            Direction::Descending => weighed.reverse(),
        }
    });
    keyed.into_iter().map(|key| key.row).collect()
}

impl Keyed {
    fn of(
        row: usize,
        item: &QueueItem,
        track: Option<Track>,
        titles: &AHashMap<AlbumId, String>,
        player: &Player,
    ) -> Self {
        let album = track
            .as_ref()
            .and_then(|track| track.album_id)
            .and_then(|id| titles.get(&id))
            .map(String::as_str);

        match track {
            Some(track) => Self {
                row,
                album: folded(album),
                disc: track.disc_number.unwrap_or(u32::MAX),
                number: track.track_number.unwrap_or(u32::MAX),
                artist: folded(track.artist.as_deref()),
                title: folded(Some(&track.title)),
                length: track
                    .duration
                    .map(|frames| frames.to_duration(track.spec.rate)),
                file: format::stem(&item.location),
            },
            None => {
                let info = player.media(&item.location, item.span);
                let tags = info.as_ref().map(|info| &info.tags);

                Self {
                    row,
                    album: folded(tags.and_then(|tags| tags.album.as_deref())),
                    disc: u32::MAX,
                    number: u32::MAX,
                    artist: folded(tags.and_then(|tags| tags.artist.as_deref())),
                    title: folded(tags.and_then(|tags| tags.title.as_deref())),
                    length: info
                        .as_ref()
                        .and_then(|info| info.duration.map(|f| f.to_duration(info.spec.rate))),
                    file: format::stem(&item.location),
                }
            }
        }
    }
}

fn folded(text: Option<&str>) -> String {
    text.unwrap_or_default().to_lowercase()
}

struct Keyed {
    row: usize,
    album: String,
    disc: u32,
    number: u32,
    artist: String,
    title: String,
    length: Option<Duration>,
    file: String,
}

impl Keyed {
    fn weighed(&self, order: RowOrder, other: &Self) -> Ordering {
        match order {
            RowOrder::Album => (&self.album, self.disc, self.number, &self.title).cmp(&(
                &other.album,
                other.disc,
                other.number,
                &other.title,
            )),
            RowOrder::Artist => (&self.artist, &self.album, self.disc, self.number).cmp(&(
                &other.artist,
                &other.album,
                other.disc,
                other.number,
            )),
            RowOrder::Title => self.title.cmp(&other.title),
            RowOrder::Length => self
                .length
                .is_none()
                .cmp(&other.length.is_none())
                .then_with(|| self.length.cmp(&other.length)),
            RowOrder::File => self.file.cmp(&other.file),
        }
    }
}

fn part_heading(drawn: usize, run: Run, from: Option<SharedString>) -> AnyElement {
    let from = from.filter(|_| run.part == Part::Rest);

    row(false)
        .id(("queue-part", drawn))
        .items_end()
        .pb_1()
        .child(
            div()
                .flex()
                .flex_1()
                .min_w(px(0.0))
                .items_center()
                .gap_2()
                .child(kit::eyebrow(run.part.named()))
                .when(run.part.is_counted(), |heading| {
                    heading.child(kit::figure(format::count(run.rows.rows())))
                })
                .when_some(from, |heading, from| {
                    heading.child(
                        div()
                            .flex_shrink(1.0)
                            .min_w(px(0.0))
                            .text_size(px(theme::text_xs()))
                            .text_color(rgb(theme::muted()))
                            .child(from)
                            .ends_in_an_ellipsis(),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w(px(theme::row_number()))
                        .h(px(1.0))
                        .bg(rgb(theme::border())),
                ),
        )
        .into_any_element()
}

fn waiting_to_play(next: Option<Span>, playing: Option<usize>) -> usize {
    next.map_or(0, |next| {
        playing.map_or(next.rows(), |at| {
            next.last().saturating_sub(at).min(next.rows())
        })
    })
}

fn runs_of(rows: &[usize]) -> Vec<Span> {
    let mut runs: Vec<Span> = Vec::new();
    for &row in rows {
        match runs.last_mut() {
            Some(run) if run.last() + 1 == row => *run = Span::between(run.first(), row),
            _ => runs.push(Span::one(row)),
        }
    }
    runs
}

pub(crate) fn took_out(rows: usize) -> Notice {
    Notice::Done(words::queue::took_out(rows))
}

fn puts_back(offer: Option<Offer>) -> SharedString {
    let behind = offer.map_or(0, |offer| offer.behind);

    SharedString::from(words::queue::puts_back(behind))
}

fn takes_out_again(offer: Option<Offer>) -> SharedString {
    let behind = offer.map_or(0, |offer| offer.behind);

    SharedString::from(words::queue::takes_out_again(behind))
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc, time::Duration};

    use gpui::{App, TestAppContext};
    use resonate_core::{MediaLocation, Span, TrackId, ZoneId};
    use resonate_engine::Command;
    use resonate_library::{Direction, Library, NamesMoved, RowOrder, Track, TrackQuery};

    use super::{
        KEPT_GESTURES, Line, Named, Offer, Part, PutInOrder, QueueItem, QueueLength, QueueParts,
        QueueRows, Run, Standing, TakenBack, named_again, puts_back, runs_of, takes_out_again,
        took_out,
    };
    use crate::{
        Pane,
        driven::{Driven, Folder},
        views::{
            reorder::{Carried, CarriedOver, Chosen, LiftedTo, Shift},
            root::RootView,
        },
    };

    fn length_at(queue: u64, library: &Library) -> QueueLength {
        QueueLength {
            queue,
            rows: library.rows_stamp(),
            total: std::time::Duration::ZERO,
            unmeasured: 0,
        }
    }

    #[test]
    fn a_length_measured_holds_until_the_queue_or_the_catalog_rows_move() {
        let library = Library::open_in_memory().expect("an in-memory catalog");
        let held = length_at(4, &library);

        assert!(held.measures(length_at(4, &library)));
        assert!(!held.measures(length_at(5, &library)));
    }

    #[test]
    fn a_queue_edit_names_only_the_rows_it_brought() {
        let library = Library::open_in_memory().expect("an in-memory catalog");
        let player = crate::driven::unplugged_player();
        let rows = queued(&[2, 3, 1]);

        let named = named_again(
            &library,
            &player,
            rows.iter().map(Arc::as_ref),
            Some(standing(&[1, 2], NamesMoved::Nothing)),
        );
        let folded: Vec<&str> = named.iter().map(|named| named.folded.as_str()).collect();

        assert_eq!(folded, ["kept 2", "3", "kept 1"]);
        assert!(
            named
                .iter()
                .map(|named| &named.item)
                .eq(rows.iter().map(Arc::as_ref))
        );
    }

    #[test]
    fn a_name_written_in_the_catalog_reads_again_only_the_rows_it_retitled() {
        let library = Library::open_in_memory().expect("an in-memory catalog");
        let player = crate::driven::unplugged_player();
        let rows = queued(&[1, 2, 3]);
        let moved = NamesMoved::These {
            retitled: [TrackId::new(2).expect("a namable track")]
                .into_iter()
                .collect(),
            added: false,
        };

        let named = named_again(
            &library,
            &player,
            rows.iter().map(Arc::as_ref),
            Some(standing(&[1, 2, 3], moved)),
        );
        let folded: Vec<&str> = named.iter().map(|named| named.folded.as_str()).collect();

        assert_eq!(folded, ["kept 1", "2", "kept 3"]);
    }

    fn standing(ids: &[u64], moved: NamesMoved) -> Standing {
        Standing {
            names: queued(ids)
                .into_iter()
                .map(|item| Named {
                    folded: format!("kept {}", item.id.get()),
                    track: Some(item.id),
                    item: QueueItem::clone(&item),
                })
                .collect(),
            moved,
        }
    }

    fn queued(ids: &[u64]) -> Vec<Arc<QueueItem>> {
        ids.iter()
            .map(|id| {
                Arc::new(QueueItem {
                    id: TrackId::new(*id).expect("a namable track"),
                    location: MediaLocation::local(format!("/music/{id}.flac")),
                    span: None,
                })
            })
            .collect()
    }

    fn rows_of(queue: &[Arc<QueueItem>]) -> QueueRows {
        queue.iter().cloned().collect()
    }

    fn without(queue: &[Arc<QueueItem>], rows: Span) -> Vec<Arc<QueueItem>> {
        let mut left = queue.to_vec();
        left.drain(rows.range());
        left
    }

    fn back(kept: &mut TakenBack, queue: &mut Vec<Arc<QueueItem>>, rows: Span) {
        kept.keeping_all(&rows_of(queue), &[rows]);
        *queue = without(queue, rows);
    }

    fn put_back(kept: &mut TakenBack, queue: &mut Vec<Arc<QueueItem>>) -> bool {
        let Some(taken) = kept.take(&rows_of(queue)) else {
            return false;
        };
        queue.splice(taken.at..taken.at, taken.rows.into_iter().map(Arc::new));
        true
    }

    fn take_again(kept: &mut TakenBack, queue: &mut Vec<Arc<QueueItem>>) -> bool {
        let Some(rows) = kept.take_again(&rows_of(queue)) else {
            return false;
        };
        queue.drain(rows.range());
        true
    }

    fn run(part: Part, first: usize, last: usize) -> Line {
        Line::Heading(Run {
            part,
            rows: Span::between(first, last),
        })
    }

    #[test]
    fn a_queue_is_parted_into_what_was_heard_what_plays_what_was_queued_and_the_rest() {
        let parts = QueueParts::of(6, Some(2), Some(Span::between(3, 4)));

        let drawn: Vec<Option<Line>> = (0..=parts.lines()).map(|item| parts.line(item)).collect();
        assert_eq!(
            drawn,
            [
                Some(run(Part::Heard, 0, 1)),
                Some(Line::Row(0)),
                Some(Line::Row(1)),
                Some(run(Part::Playing, 2, 2)),
                Some(Line::Row(2)),
                Some(run(Part::Next, 3, 4)),
                Some(Line::Row(3)),
                Some(Line::Row(4)),
                Some(run(Part::Rest, 5, 5)),
                Some(Line::Row(5)),
                None,
            ]
        );
    }

    #[test]
    fn a_queued_row_being_heard_is_playing_rather_than_queued() {
        let parts = QueueParts::of(5, Some(1), Some(Span::between(1, 2)));

        assert_eq!(parts.line(0), Some(run(Part::Heard, 0, 0)));
        assert_eq!(parts.line(2), Some(run(Part::Playing, 1, 1)));
        assert_eq!(parts.line(4), Some(run(Part::Next, 2, 2)));
        assert_eq!(parts.line(6), Some(run(Part::Rest, 3, 4)));
        assert_eq!(parts.part_of(1), Part::Playing);
        assert_eq!(parts.part_of(2), Part::Next);
    }

    #[test]
    fn a_queue_of_one_part_draws_no_headings() {
        let parts = QueueParts::of(4, None, None);

        assert_eq!(parts.lines(), 4);
        assert_eq!(parts.line(3), Some(Line::Row(3)));
        assert_eq!(parts.line(4), None);
        assert_eq!(parts.line_of(2), 2);
    }

    #[test]
    fn the_first_row_playing_heads_what_plays_and_what_follows() {
        let parts = QueueParts::of(3, Some(0), None);

        assert_eq!(parts.lines(), 5);
        assert_eq!(parts.line(0), Some(run(Part::Playing, 0, 0)));
        assert_eq!(parts.line(2), Some(run(Part::Rest, 1, 2)));
    }

    #[test]
    fn the_queue_opens_on_the_now_playing_heading_with_room_to_lift_it_to_the_top() {
        let playing_in_the_middle = QueueParts::of(8, Some(3), None);
        assert_eq!(playing_in_the_middle.opens_at(), Some(4));
        assert_eq!(
            playing_in_the_middle.line(4),
            Some(Line::Heading(Run {
                part: Part::Playing,
                rows: Span::one(3),
            }))
        );
        assert_eq!(playing_in_the_middle.room_below(20), 20 - (11 - 4));
        assert_eq!(playing_in_the_middle.room_below(3), 0);

        let nothing_heard = QueueParts::of(8, Some(0), None);
        assert_eq!(nothing_heard.opens_at(), Some(0));
        assert_eq!(nothing_heard.room_below(20), 0);

        let nothing_playing = QueueParts::of(8, None, None);
        assert_eq!(nothing_playing.opens_at(), None);
        assert_eq!(nothing_playing.room_below(20), 0);
    }

    #[test]
    fn every_row_is_shown_at_the_line_that_draws_it() {
        let shapes = [
            QueueParts::of(8, Some(3), Some(Span::between(4, 6))),
            QueueParts::of(8, Some(3), Some(Span::between(3, 6))),
            QueueParts::of(8, Some(7), None),
            QueueParts::of(8, None, Some(Span::between(0, 2))),
            QueueParts::of(8, None, None),
            QueueParts::of(8, Some(12), Some(Span::between(9, 10))),
        ];

        for parts in shapes {
            for row in 0..8 {
                assert_eq!(
                    parts.line(parts.line_of(row)),
                    Some(Line::Row(row)),
                    "row {row} of {parts:?}"
                );
            }
        }
    }

    #[test]
    fn a_run_of_gestures_is_walked_back_through_one_at_a_time() {
        let mut queue = queued(&[1, 2, 3, 4, 5]);
        let mut kept = TakenBack::default();

        back(&mut kept, &mut queue, Span::between(4, 4));
        back(&mut kept, &mut queue, Span::between(0, 1));
        assert_eq!(queue, queued(&[3, 4]));

        assert!(put_back(&mut kept, &mut queue));
        assert_eq!(queue, queued(&[1, 2, 3, 4]));
        assert!(put_back(&mut kept, &mut queue));
        assert_eq!(queue, queued(&[1, 2, 3, 4, 5]));
        assert!(!put_back(&mut kept, &mut queue), "a step was kept twice");
    }

    #[test]
    fn a_walk_put_back_is_taken_out_again_the_last_put_back_first() {
        let mut queue = queued(&[1, 2, 3, 4, 5]);
        let mut kept = TakenBack::default();

        back(&mut kept, &mut queue, Span::between(4, 4));
        back(&mut kept, &mut queue, Span::between(0, 1));
        assert!(put_back(&mut kept, &mut queue));
        assert!(put_back(&mut kept, &mut queue));
        assert_eq!(queue, queued(&[1, 2, 3, 4, 5]));

        assert!(take_again(&mut kept, &mut queue));
        assert_eq!(queue, queued(&[1, 2, 3, 4]));
        assert!(take_again(&mut kept, &mut queue));
        assert_eq!(queue, queued(&[3, 4]));
        assert!(!take_again(&mut kept, &mut queue), "a step was taken twice");

        assert!(put_back(&mut kept, &mut queue));
        assert_eq!(queue, queued(&[1, 2, 3, 4]));
    }

    #[test]
    fn what_was_put_back_is_taken_out_again_though_rows_were_queued_since_and_not_after_a_fresh_gesture()
     {
        let mut queue = queued(&[1, 2, 3]);
        let mut kept = TakenBack::default();

        back(&mut kept, &mut queue, Span::between(0, 0));
        assert!(put_back(&mut kept, &mut queue));
        let offer = kept
            .offered_again(&rows_of(&queue))
            .expect("the put back is offered");
        assert_eq!((offer.rows, offer.behind), (1, 0));

        queue.insert(0, queued(&[9])[0].clone());
        assert!(kept.offered_again(&rows_of(&queue)).is_some());
        assert!(take_again(&mut kept, &mut queue));
        assert_eq!(queue, queued(&[9, 2, 3]));
        assert!(put_back(&mut kept, &mut queue));

        back(&mut kept, &mut queue, Span::between(2, 2));
        assert_eq!(
            kept.offered_again(&rows_of(&queue)),
            None,
            "a fresh gesture kept what was put back before it"
        );
    }

    #[test]
    fn the_offer_counts_what_is_behind_it() {
        let mut queue = queued(&[1, 2, 3, 4]);
        let mut kept = TakenBack::default();

        assert_eq!(kept.offered(&rows_of(&queue)), None);

        back(&mut kept, &mut queue, Span::between(3, 3));
        let offer = kept
            .offered(&rows_of(&queue))
            .expect("the gesture is offered back");
        assert_eq!((offer.rows, offer.behind), (1, 0));

        back(&mut kept, &mut queue, Span::between(0, 1));
        let offer = kept
            .offered(&rows_of(&queue))
            .expect("the gesture is offered back");
        assert_eq!((offer.rows, offer.behind), (2, 1));
    }

    #[test]
    fn rows_queued_since_leave_the_walk_standing_and_the_rows_land_beside_the_one_they_followed() {
        let mut queue = queued(&[1, 2, 3, 4]);
        let mut kept = TakenBack::default();

        back(&mut kept, &mut queue, Span::between(1, 2));
        back(&mut kept, &mut queue, Span::between(0, 0));
        assert_eq!(queue, queued(&[4]));
        queue.insert(0, queued(&[9])[0].clone());

        let offer = kept
            .offered(&rows_of(&queue))
            .expect("the walk still stands");
        assert_eq!((offer.rows, offer.behind), (1, 1));
        assert!(put_back(&mut kept, &mut queue));
        assert_eq!(queue, queued(&[1, 9, 4]));
        assert!(put_back(&mut kept, &mut queue));
        assert_eq!(queue, queued(&[1, 2, 3, 9, 4]));
    }

    #[test]
    fn rows_are_grouped_into_the_runs_they_stand_in() {
        assert_eq!(runs_of(&[]), Vec::<Span>::new());
        assert_eq!(runs_of(&[4]), vec![Span::one(4)]);
        assert_eq!(
            runs_of(&[0, 1, 2, 5, 7, 8]),
            vec![Span::between(0, 2), Span::one(5), Span::between(7, 8)]
        );
    }

    #[test]
    fn a_gesture_over_two_runs_is_one_toast_of_both_and_each_run_is_put_back_where_it_stood() {
        let mut queue = queued(&[1, 2, 3, 4, 5]);
        let mut kept = TakenBack::default();
        let runs = [Span::one(1), Span::between(3, 4)];

        assert_eq!(kept.keeping_all(&rows_of(&queue), &runs), Some(3));
        queue = queued(&[1, 3]);

        assert!(put_back(&mut kept, &mut queue));
        assert_eq!(queue, queued(&[1, 3, 4, 5]));
        assert!(put_back(&mut kept, &mut queue));
        assert_eq!(queue, queued(&[1, 2, 3, 4, 5]));
        assert!(!put_back(&mut kept, &mut queue));
    }

    #[test]
    fn a_row_already_back_in_the_queue_is_not_put_back_twice() {
        let mut queue = queued(&[1, 2, 3]);
        let mut kept = TakenBack::default();

        back(&mut kept, &mut queue, Span::between(2, 2));
        queue.push(queued(&[3])[0].clone());

        assert_eq!(kept.offered(&rows_of(&queue)), None);
        assert!(kept.take(&rows_of(&queue)).is_none());
    }

    #[test]
    fn the_offer_says_how_far_back_it_reaches() {
        assert_eq!(took_out(3).text(), "Took 3 tracks out of the queue");
        assert_eq!(took_out(1).text(), "Took 1 track out of the queue");
        assert!(puts_back(None).ends_with("the last one there is to put back"));
        assert!(
            puts_back(Some(Offer { rows: 1, behind: 1 }))
                .ends_with("One more gesture is behind it")
        );
        assert!(takes_out_again(None).ends_with("the last one there is to take out again"));
        assert!(
            takes_out_again(Some(Offer { rows: 1, behind: 2 }))
                .ends_with("More put backs are behind it")
        );
    }

    fn queue_of(
        cx: &mut TestAppContext,
        folder: &Folder,
        seconds: &[u32],
    ) -> (Driven, Vec<PathBuf>) {
        let files: Vec<PathBuf> = seconds
            .iter()
            .zip(SPOKEN)
            .map(|(seconds, name)| folder.tone(&format!("{name}.wav"), *seconds))
            .collect();
        let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
        Driven::scanned(&library, folder);
        let mut driven = Driven::opened_in(cx, library, folder);

        driven.play(&files);
        driven.click("queue");
        driven.until(|root, _| root.pane == Pane::Queue);

        (driven, files)
    }

    const SPOKEN: [&str; 4] = ["charlie", "alpha", "delta", "bravo"];

    fn row_of(id: TrackId) -> &'static str {
        Box::leak(format!("queued-{}", id.get()).into_boxed_str())
    }

    fn held_length(root: &RootView, cx: &App) -> Option<QueueLength> {
        root.queue.read(cx).length.held
    }

    fn measured(driven: &mut Driven) -> Option<(Duration, usize)> {
        driven.read(|root, cx| held_length(root, cx).map(|held| (held.total, held.unmeasured)))
    }

    fn named_in_order(driven: &mut Driven) -> Vec<String> {
        driven.read(|root, cx| {
            root.player
                .read(cx)
                .queue()
                .iter()
                .map(|item| crate::format::stem(&item.location))
                .collect()
        })
    }

    fn ordered_by(driven: &mut Driven) -> (RowOrder, Direction) {
        driven.read(|root, cx| root.queue.read(cx).order())
    }

    #[gpui::test]
    fn the_queue_pane_draws_every_queued_row_and_measures_how_long_they_run(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, _) = queue_of(cx, &folder, &[1, 2, 3]);

        for id in driven.queue() {
            driven.until_drawn(row_of(id));
        }
        driven.until(|root, cx| held_length(root, cx).is_some());

        assert_eq!(measured(&mut driven), Some((Duration::from_secs(6), 0)));
        assert!(driven.cx.debug_bounds("clear-queue").is_some());
        assert!(driven.cx.debug_bounds("queue-to-playlist-all").is_some());
        assert!(driven.cx.debug_bounds("put-the-queue-back").is_none());
    }

    #[gpui::test]
    fn a_song_the_catalog_does_not_hold_leaves_the_length_measured_at_least(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, mut files) = queue_of(cx, &folder, &[2, 1]);
        files.push(folder.tone("unscanned.wav", 4));

        driven.play(&files);
        driven.until(|root, cx| root.player.read(cx).queue().len() == 3);
        driven.until(|root, cx| held_length(root, cx).is_some_and(|held| held.unmeasured == 1));

        assert_eq!(measured(&mut driven), Some((Duration::from_secs(3), 1)));
    }

    #[gpui::test]
    fn the_queue_cleared_is_offered_back_and_once_put_back_is_offered_out_again(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, _) = queue_of(cx, &folder, &[1, 1, 1]);
        let before = driven.queue();

        driven.click("clear-queue");
        driven.until(|root, cx| root.player.read(cx).queue().is_empty());
        driven.until_drawn("put-the-queue-back");

        assert!(driven.cx.debug_bounds("clear-queue").is_none());

        driven.click("put-the-queue-back");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 3);
        driven.until_drawn("take-the-queue-out-again");

        assert_eq!(driven.queue(), before);
        assert!(driven.cx.debug_bounds("put-the-queue-back").is_none());

        driven.click("take-the-queue-out-again");
        driven.until(|root, cx| root.player.read(cx).queue().is_empty());
        driven.until_drawn("put-the-queue-back");
    }

    fn behind_the_window(root: &RootView, cx: &App, command: Command) {
        root.player
            .read(cx)
            .engine()
            .send(command)
            .expect("the engine takes a command");
    }

    fn one_more_row_landed(driven: &mut Driven) {
        let queued = driven.read(|root, cx| root.player.read(cx).queue());
        let mut more = QueueItem::clone(queued.first().expect("a queued row"));
        more.id = resonate_engine::unclaimed_id(queued.iter().map(AsRef::as_ref));
        let rows = queued.len();

        let more = Command::Insert {
            items: vec![more],
            at: resonate_engine::Placement::At(rows),
            play: false,
        };
        driven.read(|root, cx| behind_the_window(root, cx, more));
        driven.until(|root, cx| root.player.read(cx).queue().len() > rows);
    }

    #[gpui::test]
    fn a_sort_keyed_before_another_reorder_landed_is_refused_rather_than_shuffling_the_new_order(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, _) = queue_of(cx, &folder, &[1, 1, 1, 1]);
        let revision = driven.read(|root, cx| root.player.read(cx).queued().revision);

        driven.root.update(&mut driven.cx, |root, cx| {
            behind_the_window(root, cx, Command::Order(vec![1, 0, 2, 3]));
            root.queue_put_in_order(
                &PutInOrder {
                    revision,
                    rows: vec![3, 2, 1, 0],
                },
                cx,
            );
        });
        one_more_row_landed(&mut driven);

        assert_eq!(
            named_in_order(&mut driven)[..4],
            ["alpha", "charlie", "delta", "bravo"]
        );
    }

    #[gpui::test]
    fn rows_put_back_after_another_reorder_landed_are_refused_rather_than_landing_elsewhere(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, files) = queue_of(cx, &folder, &[1, 1, 1, 1]);
        let alpha = MediaLocation::local(&files[1]);

        driven.root.update(&mut driven.cx, |root, cx| {
            root.take_out_of_the_queue(&alpha, None, cx);
        });
        driven.until(|root, cx| root.player.read(cx).queue().len() == 3);
        driven.root.update(&mut driven.cx, |root, cx| {
            behind_the_window(root, cx, Command::Order(vec![2, 1, 0]));
            assert!(root.put_the_queue_back(cx), "a step stood to put back");
        });
        one_more_row_landed(&mut driven);

        assert_eq!(
            named_in_order(&mut driven)[..3],
            ["bravo", "delta", "charlie"]
        );
    }

    #[gpui::test]
    fn rows_lifted_onto_a_queue_row_after_another_reorder_landed_are_refused_rather_than_landing_elsewhere(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, _) = queue_of(cx, &folder, &[1, 1, 1, 1]);
        let drawn_at = driven.read(|root, cx| root.player.read(cx).queued().revision);
        let lifted: Vec<Track> = driven.read(|root, cx| {
            root.library
                .read(cx)
                .catalog()
                .tracks(&TrackQuery::default())
                .expect("the catalog lists its tracks")
                .into_iter()
                .filter(|track| track.title == "charlie")
                .collect()
        });

        driven.root.update(&mut driven.cx, |root, cx| {
            let drawn = root.library.read(cx).playlist_as_drawn();
            behind_the_window(root, cx, Command::Order(vec![1, 0, 2, 3]));
            root.landed(&lifted, LiftedTo::QueueRow { row: 1, drawn_at }, drawn, cx);
        });
        one_more_row_landed(&mut driven);

        assert_eq!(lifted.len(), 1);
        assert_eq!(
            named_in_order(&mut driven)[..4],
            ["alpha", "charlie", "delta", "bravo"]
        );
        assert_eq!(named_in_order(&mut driven).len(), 5);
    }

    #[gpui::test]
    fn queue_rows_carried_before_another_reorder_landed_are_not_moved(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (mut driven, _) = queue_of(cx, &folder, &[1, 1, 1, 1]);
        let drawn_at = driven.read(|root, cx| root.player.read(cx).queued().revision);
        let carried = Carried {
            shift: Shift::Queue,
            rows: Chosen::of(Span::one(0)),
            title: "charlie".into(),
            over: CarriedOver::Queue { drawn_at },
        };

        assert!(driven.read(|root, cx| root.carried_over_what_is_drawn(&carried, cx)));

        driven.read(|root, cx| behind_the_window(root, cx, Command::Order(vec![1, 0, 2, 3])));
        driven.until(|root, cx| !root.player.read(cx).is_queued_at(drawn_at));

        assert!(!driven.read(|root, cx| root.carried_over_what_is_drawn(&carried, cx)));

        driven.root.update(&mut driven.cx, |root, cx| {
            root.shift_carried(&carried, 3, cx);
        });
        one_more_row_landed(&mut driven);

        assert_eq!(
            named_in_order(&mut driven)[..4],
            ["alpha", "charlie", "delta", "bravo"]
        );
    }

    #[gpui::test]
    fn the_queue_put_in_order_from_its_sort_pop_up_plays_in_that_order_and_keeps_the_choice(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, _) = queue_of(cx, &folder, &[1, 1, 1, 1]);
        let title = RowOrder::ALL
            .iter()
            .position(|order| *order == RowOrder::Title)
            .expect("the queue offers the title");

        assert_eq!(named_in_order(&mut driven), SPOKEN);

        driven.click("order-queue");
        driven.click(Box::leak(format!("order-choice-{title}").into_boxed_str()));
        driven.until(|root, cx| {
            root.player
                .read(cx)
                .queue()
                .first()
                .is_some_and(|item| crate::format::stem(&item.location) == "alpha")
        });

        assert_eq!(
            named_in_order(&mut driven),
            ["alpha", "bravo", "charlie", "delta"]
        );
        assert_eq!(
            ordered_by(&mut driven),
            (RowOrder::Title, Direction::Ascending)
        );

        driven.click("order-reading-1");
        driven.until(|root, cx| {
            root.player
                .read(cx)
                .queue()
                .first()
                .is_some_and(|item| crate::format::stem(&item.location) == "delta")
        });

        assert_eq!(
            named_in_order(&mut driven),
            ["delta", "charlie", "bravo", "alpha"]
        );
        assert_eq!(
            ordered_by(&mut driven),
            (RowOrder::Title, Direction::Descending)
        );
    }

    #[gpui::test]
    fn a_long_queue_is_scrolled_to_the_row_reached_and_to_the_row_that_starts_playing(
        cx: &mut TestAppContext,
    ) {
        const ROWS: usize = 90;
        let folder = Folder::new();
        let files: Vec<PathBuf> = (0..ROWS)
            .map(|row| folder.tone(&format!("{row:02}.wav"), 1))
            .collect();
        let mut driven = Driven::opened_in(
            cx,
            Arc::new(Library::open_in_memory().expect("a catalog in memory")),
            &folder,
        );
        driven.play(&files);
        driven.click("queue");
        let queued = driven.queue();
        driven.until_drawn(row_of(queued[0]));

        assert!(driven.cx.debug_bounds(row_of(queued[ROWS - 1])).is_none());

        let root = driven.root.clone();
        driven.cx.update(|window, cx| {
            let focus = root.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        driven.settle();
        driven.cx.simulate_keystrokes("end");
        driven.until_drawn(row_of(queued[ROWS - 1]));

        assert!(driven.cx.debug_bounds(row_of(queued[0])).is_none());

        driven.cx.simulate_keystrokes("home");
        driven.until_drawn(row_of(queued[0]));
        driven.root.update(&mut driven.cx, |root, cx| {
            root.send_by_position(Command::JumpTo(ROWS - 2), cx);
        });
        driven.until(|root, cx| root.player.read(cx).state().queue_position == Some(ROWS - 2));
        driven.until_drawn(row_of(queued[ROWS - 2]));
    }

    #[gpui::test]
    fn showing_another_zone_lets_go_of_the_rows_offered_back(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (mut driven, _) = queue_of(cx, &folder, &[1, 1]);
        driven.click("clear-queue");
        driven.until(|root, cx| root.player.read(cx).queue().is_empty());
        driven.until_drawn("put-the-queue-back");

        driven
            .root
            .update(&mut driven.cx, |root, cx| root.add_a_zone(cx));
        driven.until(|root, _| root.zones.len() == 2);
        driven.root.update(&mut driven.cx, |root, cx| {
            root.show_zone(ZoneId::FIRST, cx);
        });
        driven.settle();

        assert_eq!(
            driven.read(|root, _| root.switcher.selected()),
            ZoneId::FIRST
        );
        assert!(driven.cx.debug_bounds("put-the-queue-back").is_none());
    }

    #[test]
    fn the_walk_is_bounded_and_drops_its_oldest_step_first() {
        let held: Vec<u64> = (1..=(KEPT_GESTURES as u64 + 4)).collect();
        let mut queue = queued(&held);
        let mut kept = TakenBack::default();

        for _ in 0..held.len() - 1 {
            back(&mut kept, &mut queue, Span::between(0, 0));
        }

        let offer = kept
            .offered(&rows_of(&queue))
            .expect("the last gesture is offered");
        assert_eq!(offer.behind, KEPT_GESTURES - 1);

        let mut walked = 0;
        while put_back(&mut kept, &mut queue) {
            walked += 1;
        }
        assert_eq!(walked, KEPT_GESTURES);
        assert_eq!(queue, queued(&held[held.len() - 1 - KEPT_GESTURES..]));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct QueueLength {
    queue: u64,
    rows: CatalogStamp,
    total: Duration,
    unmeasured: usize,
}

impl QueueLength {
    fn measures(self, now: Self) -> bool {
        self.queue == now.queue && self.rows.still_holds_at(now.rows)
    }
}
