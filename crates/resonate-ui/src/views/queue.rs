use std::{cmp::Ordering, rc::Rc, sync::Arc, time::Duration};

use ahash::{AHashMap, AHashSet};
use gpui::{
    AnyElement, App, ClickEvent, Context, Div, SharedString, Task, div, prelude::*, px, rgb,
    uniform_list,
};
use resonate_core::{AlbumId, Span, TrackId};
use resonate_engine::{Command, Placement, Player, QueueItem};
use resonate_library::{Cut, Direction, Favoured, Library, Lit, RowOrder, Track};
use smallvec::{SmallVec, smallvec};

use crate::{
    Notice, Selection, format,
    icons::Icon,
    theme,
    views::{
        browser::{OPEN_ARTIST_HINT, ROW_CONTROLS, row_controls},
        hint,
        kit::{self, EndsInAnEllipsis, Tone},
        listing::{self, Pictured},
        menu::{self, Called, Menu},
        playlists::{self, Held, ROW_GROUP},
        reorder::{self, Carried, MOVING_HINT, Shift, Step},
        root::{RootView, empty, row},
        scrollbar::Scrollbars,
        sorting,
    },
};

#[derive(Default)]
pub(crate) struct QueueMeasure {
    held: Option<QueueLength>,
    asked: Option<QueueLength>,
    reading: Option<Task<()>>,
}

#[derive(Default)]
pub(crate) struct QueueNames {
    named: Option<(u64, Arc<[String]>)>,
    asked: Option<u64>,
    reading: Option<Task<()>>,
}

impl RootView {
    pub(crate) fn names_in_the_queue(&mut self, cx: &mut Context<Self>) -> Option<Arc<[String]>> {
        let revision = self.player.read(cx).queued().revision;
        if let Some((named, names)) = self.queue_names.named.as_ref()
            && *named == revision
        {
            return Some(Arc::clone(names));
        }
        self.name_the_queue(revision, cx);
        None
    }

    fn name_the_queue(&mut self, revision: u64, cx: &mut Context<Self>) {
        if self.queue_names.asked == Some(revision) {
            return;
        }
        self.queue_names.asked = Some(revision);
        let queued = self.player.read(cx).queued();
        let player = self.player.read(cx).engine();
        let library = self.library.read(cx).catalog();

        self.queue_names.reading = Some(cx.spawn(async move |this, cx| {
            let names: Arc<[String]> = cx
                .background_executor()
                .spawn(async move {
                    queued_rows(&library, &queued.rows)
                        .into_iter()
                        .zip(queued.rows.iter())
                        .map(|(track, item)| queued_name(track, &player, item))
                        .collect()
                })
                .await;
            let landed = this.update(cx, |this, cx| {
                this.queue_names.named = Some((revision, names));
                this.jump_where_typed(cx);
                cx.notify();
            });
            let _ = landed;
        }));
    }
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

fn queued_rows(library: &Library, rows: &[QueueItem]) -> Vec<Option<Track>> {
    let ids: Vec<TrackId> = rows.iter().map(|item| item.id).collect();
    let by_id: AHashMap<TrackId, Track> = match library.tracks_with_ids(&ids) {
        Ok(read) => read.into_iter().map(|track| (track.id, track)).collect(),
        Err(error) => {
            tracing::warn!(%error, "the queued tracks could not be read by id");
            AHashMap::new()
        }
    };

    rows.iter()
        .map(|item| match by_id.get(&item.id) {
            Some(track) if track.location == item.location => Some(track.clone()),
            _ => queued_at(library, item),
        })
        .collect()
}

fn queued_at(library: &Library, item: &QueueItem) -> Option<Track> {
    library
        .track_at(item.location.as_path()?, item.span)
        .inspect_err(|error| tracing::warn!(%error, "a queued track could not be read by path"))
        .ok()
        .flatten()
}

fn queued_cut(item: &QueueItem) -> Cut {
    Cut {
        location: item.location.clone(),
        span: item.span,
    }
}

const REMOVE_HINT: &str = "Take this row out of the queue";

const REMOVE_REACHED_HINT: &str = "Take every reached row out of the queue";

const QUEUED_HINT: &str = "Put every queued row in a playlist";

const CLEAR_HINT: &str = "Take every row out of the queue and stop";

const PUT_BACK_HINT: &str = "Put the rows last taken out of the queue back where they were";

const TAKE_AGAIN_HINT: &str = "Take the rows last put back out of the queue again";

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

    const fn named(self) -> &'static str {
        match self {
            Self::Heard => "HISTORY",
            Self::Playing => "NOW PLAYING",
            Self::Next => "PLAYING NEXT",
            Self::Rest => "CONTINUE PLAYING",
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
    fn of(queue: &[QueueItem], rows: Span) -> Option<Self> {
        let taken = queue.get(rows.range())?;
        if taken.is_empty() {
            return None;
        }

        Some(Self {
            rows: taken.to_vec(),
            at: rows.first(),
            after: rows
                .first()
                .checked_sub(1)
                .and_then(|before| queue.get(before))
                .map(|item| item.id),
        })
    }

    fn stands_over(&self, queue: &[QueueItem]) -> bool {
        self.rows
            .iter()
            .all(|taken| queue.iter().all(|item| item.id != taken.id))
    }

    fn landing_in(&self, queue: &[QueueItem]) -> usize {
        match self.after {
            None => 0,
            Some(after) => queue
                .iter()
                .position(|item| item.id == after)
                .map_or(self.at.min(queue.len()), |before| before + 1),
        }
    }

    fn standing_in(&self, queue: &[QueueItem]) -> Option<Span> {
        let first = self.rows.first()?;
        let at = queue.iter().position(|item| item.id == first.id)?;
        let held = queue.get(at..at + self.rows.len())?;
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
    pub(crate) fn keeping(&mut self, queue: &[QueueItem], rows: Span) -> Option<usize> {
        let taken = TakenOut::of(queue, rows)?;
        let kept = taken.rows.len();
        self.put_back.clear();
        self.stack(taken);
        Some(kept)
    }

    fn stack(&mut self, taken: TakenOut) {
        if self.steps.len() == KEPT_GESTURES {
            self.steps.remove(0);
        }
        self.steps.push(taken);
    }

    fn standing(&self, queue: &[QueueItem]) -> Option<&TakenOut> {
        self.steps.last().filter(|taken| taken.stands_over(queue))
    }

    fn standing_again(&self, queue: &[QueueItem]) -> Option<&TakenOut> {
        self.put_back
            .last()
            .filter(|taken| taken.standing_in(queue).is_some())
    }

    fn offered(&self, queue: &[QueueItem]) -> Option<Offer> {
        self.standing(queue).map(|taken| Offer {
            rows: taken.rows.len(),
            behind: self.steps.len().saturating_sub(1),
        })
    }

    fn offered_again(&self, queue: &[QueueItem]) -> Option<Offer> {
        self.standing_again(queue).map(|taken| Offer {
            rows: taken.rows.len(),
            behind: self.put_back.len().saturating_sub(1),
        })
    }

    pub(crate) fn take(&mut self, queue: &[QueueItem]) -> Option<PuttingBack> {
        self.standing(queue)?;
        let taken = self.steps.pop()?;
        let putting = PuttingBack {
            rows: taken.rows.clone(),
            at: taken.landing_in(queue),
        };
        self.put_back.push(taken);
        Some(putting)
    }

    pub(crate) fn take_again(&mut self, queue: &[QueueItem]) -> Option<Span> {
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
    fn of(queue: &[QueueItem], rows: Span) -> Self {
        Self {
            rows,
            holding: queue
                .get(rows.range())
                .unwrap_or_default()
                .iter()
                .map(queued_cut)
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
                "The queue is empty.",
                Some("Play something from Albums or Tracks, or add rows with the queue marks."),
            );
            if !self.holds_what_was_taken_out(&queue) {
                return nothing;
            }

            return div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.0))
                .child(self.queue_heading(&queue, cx))
                .child(nothing)
                .into_any_element();
        }

        let parts = self.queue_parts(cx);
        let from = self.playing_from(cx);
        let queued = queue.len();
        let height = self.queue_height.get();
        let shown = (f32::from(height) / theme::row_height()) as usize;
        let lines = parts.lines() + parts.room_below(shown);
        let heading = self.queue_heading(&queue, cx);
        let scroll = self.queue_rows.clone();
        let reaching = self
            .reaching(Shift::Queue)
            .map(|rows| Reaching::of(&queue, rows));

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .child(heading)
            .child(listing::columns(
                "",
                true,
                ROW_CONTROLS,
                sorting::queue_sorted(self),
                cx,
            ))
            .child(
                reorder::follows_a_drag(
                    div()
                        .id("queue-rows")
                        .relative()
                        .flex()
                        .flex_1()
                        .min_h(px(0.0)),
                    scroll.clone(),
                    lines,
                    cx,
                )
                .child(kit::measures_its_height(Rc::clone(&self.queue_height)))
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
                                    let reached = this.reaches(Shift::Queue, index);
                                    let acting_on = this.acting_on(Shift::Queue, index);
                                    let holding: Arc<[Cut]> =
                                        match Reaching::acting_on(reaching.as_ref(), acting_on) {
                                            Some(reaching) => Arc::clone(&reaching.holding),
                                            None => Arc::from([queued_cut(item)]),
                                        };
                                    let carried = Carried {
                                        shift: Shift::Queue,
                                        rows: acting_on,
                                        title: drawn.title.clone(),
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
                                    let listed = row(current)
                                        .id(("queued", queued_as))
                                        .debug_selector(move || format!("queued-{queued_as}"))
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
                                                OPEN_ARTIST_HINT,
                                                drawn.artist_id.map(Selection::Artist),
                                                cx,
                                            )
                                            .flex_shrink()
                                            .ends_in_an_ellipsis(),
                                        ))
                                        .child(listing::format_cell(drawn.shape))
                                        .child(listing::heard(
                                            drawn.plays,
                                            drawn.played,
                                            this.drawn_at(),
                                        ))
                                        .child(listing::length_cell(drawn.length))
                                        .child(
                                            row_controls()
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
                                                        playlists::ADD_HINT
                                                    } else {
                                                        playlists::ADD_REACHED_HINT
                                                    },
                                                    move || Held::of(Arc::clone(&holding)),
                                                    cx,
                                                ))
                                                .child(
                                                    kit::icon_button(
                                                        ("remove", index),
                                                        Icon::Close,
                                                        if acting_on.is_one_row() {
                                                            REMOVE_HINT
                                                        } else {
                                                            REMOVE_REACHED_HINT
                                                        },
                                                    )
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        cx.stop_propagation();
                                                        this.drop_rows(Shift::Queue, acting_on, cx);
                                                    })),
                                                ),
                                        )
                                        .on_click(cx.listener(
                                            move |this, event: &ClickEvent, _, cx| {
                                                let extending = event.modifiers().shift;
                                                this.reach_at(Shift::Queue, index, extending, cx);
                                                if !extending {
                                                    this.send(Command::JumpTo(index), cx);
                                                }
                                            },
                                        ));
                                    let listed = menu::opens_a_menu(
                                        listed,
                                        move |this, at, cx| {
                                            let taken = this.acting_on(Shift::Queue, index);
                                            let put = Arc::clone(&menued);
                                            let names = Called {
                                                title: title.clone(),
                                                artist: artist.clone(),
                                                album: this.album_named(album, &location, span, cx),
                                            };

                                            Menu::at(at)
                                                .under(
                                                    Icon::Play,
                                                    menu::PLAY,
                                                    "enter",
                                                    move |this, _, cx| {
                                                        this.send(Command::JumpTo(index), cx);
                                                    },
                                                )
                                                .holds(move || Held::of(Arc::clone(&put)))
                                                .reaches(scanned, album, artist_id)
                                                .when_some(scanned, |menu, track| {
                                                    menu.favours(Favoured::Track(track), favourite)
                                                })
                                                .offers_the_file(&location, names)
                                                .when_some(scanned, Menu::shares)
                                                .apart()
                                                .under(
                                                    Icon::Discard,
                                                    menu::TAKE_OUT,
                                                    "delete",
                                                    move |this, _, cx| {
                                                        this.drop_rows(Shift::Queue, taken, cx);
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
                        .track_scroll(scroll)
                        .h_full()
                        .w_full(),
                    )
                })
                .child(Scrollbars::of(cx).vertical("queue-scrollbar", self.queue_rows.clone())),
            )
            .into_any_element()
    }

    fn holds_what_was_taken_out(&self, queue: &[QueueItem]) -> bool {
        self.took_out.offered(queue).is_some()
    }

    fn queue_length(&mut self, cx: &mut Context<Self>) -> Option<Duration> {
        let measured = QueueLength {
            queue: self.player.read(cx).queued().revision,
            library: self.library.read(cx).revision(),
            total: Duration::ZERO,
        };
        if let Some(held) = self.queue_length.held
            && held.measures(measured)
        {
            return Some(held.total);
        }

        self.measure_the_queue(measured, cx);
        self.queue_length.held.map(|held| held.total)
    }

    fn measure_the_queue(&mut self, measured: QueueLength, cx: &mut Context<Self>) {
        if self
            .queue_length
            .asked
            .is_some_and(|asked| asked.measures(measured))
        {
            return;
        }
        self.queue_length.asked = Some(measured);
        let queued = self.player.read(cx).queued();
        let library = self.library.read(cx).catalog();

        self.queue_length.reading = Some(cx.spawn(async move |this, cx| {
            let total: Duration = cx
                .background_executor()
                .spawn(async move {
                    queued_rows(&library, &queued.rows)
                        .into_iter()
                        .flatten()
                        .filter_map(|track| {
                            track
                                .duration
                                .map(|frames| frames.to_duration(track.spec.rate))
                        })
                        .sum()
                })
                .await;
            let landed = this.update(cx, |this, cx| {
                this.queue_length.held = Some(QueueLength { total, ..measured });
                cx.notify();
            });
            let _ = landed;
        }));
    }

    fn queue_heading(&mut self, queue: &[QueueItem], cx: &mut Context<Self>) -> Div {
        let position = self.player.read(cx).state().queue_position;
        let total = self.queue_length(cx);
        let mut under: format::Parts<String> =
            smallvec![format::counted(queue.len(), "track", "tracks")];
        if let Some(total) = total.filter(|total| !total.is_zero()) {
            under.push(format::spanned(total));
        }
        if let Some(position) = position {
            under.push(format!("on row {}", position + 1));
        }
        let waiting = waiting_to_play(self.player.read(cx).queued().next, position);
        if waiting > 0 {
            under.push(format!("{waiting} to play next"));
        }

        let taken_out = self.took_out.offered(queue);
        let put_back = self.took_out.offered_again(queue);
        let queued = !queue.is_empty();

        kit::heading()
            .child(
                kit::heading_row()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(theme::heading_name()))
                            .gap_1()
                            .child(kit::eyebrow("NOW PLAYING"))
                            .child(kit::title("Queue"))
                            .child(kit::subtitle(under.join(" · "))),
                    )
                    .child(
                        kit::actions()
                            .child(hint::explains("queue-moving", MOVING_HINT))
                            .when(queued, |bar| {
                                bar.child(self.orders_a_listing("order-queue", cx))
                                    .child(
                                        kit::button(
                                            "clear-queue",
                                            Some(Icon::Discard),
                                            "Clear",
                                            CLEAR_HINT,
                                            Tone::Ghost,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                let queued = this.player.read(cx).queue();
                                                let Some(last) = queued.len().checked_sub(1) else {
                                                    return;
                                                };
                                                this.drop_rows(
                                                    Shift::Queue,
                                                    Span::between(0, last),
                                                    cx,
                                                );
                                            }),
                                        ),
                                    )
                                    .child(
                                        kit::button(
                                            "queue-to-playlist-all",
                                            Some(Icon::Plus),
                                            "Save as a playlist",
                                            QUEUED_HINT,
                                            Tone::Outlined,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                let queued: Arc<[Cut]> = this
                                                    .player
                                                    .read(cx)
                                                    .queue()
                                                    .iter()
                                                    .map(queued_cut)
                                                    .collect();
                                                this.hold_for_a_playlist(
                                                    Held::of(queued),
                                                    window,
                                                    cx,
                                                );
                                            }),
                                        ),
                                    )
                            })
                            .when(taken_out.is_some(), |bar| {
                                bar.child(
                                    kit::button(
                                        "put-the-queue-back",
                                        Some(Icon::Undo),
                                        "Put back",
                                        puts_back(taken_out),
                                        Tone::Ghost,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.put_the_queue_back(cx);
                                        },
                                    )),
                                )
                            })
                            .when(put_back.is_some(), |bar| {
                                bar.child(
                                    kit::button(
                                        "take-the-queue-out-again",
                                        Some(Icon::Redo),
                                        "Take out again",
                                        takes_out_again(put_back),
                                        Tone::Ghost,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.take_the_queue_out_again(cx);
                                        },
                                    )),
                                )
                            }),
                    ),
            )
            .when(self.ordering, |heading| {
                heading.child(self.queue_in_order(cx))
            })
    }

    pub(crate) fn put_the_queue_back(&mut self, cx: &mut Context<Self>) -> bool {
        let queued = self.player.read(cx).queue();
        let Some(taken) = self.took_out.take(&queued) else {
            return false;
        };
        self.send(
            Command::Insert {
                items: taken.rows,
                at: Placement::At(taken.at),
                play: false,
            },
            cx,
        );
        true
    }

    pub(crate) fn take_the_queue_out_again(&mut self, cx: &mut Context<Self>) -> bool {
        let queued = self.player.read(cx).queue();
        let Some(rows) = self.took_out.take_again(&queued) else {
            return false;
        };
        self.send(Command::Remove(rows), cx);
        true
    }

    fn queue_in_order(&self, cx: &mut Context<Self>) -> Div {
        sorting::order_row(
            "queue-order",
            "queue-reading",
            self.queue_order,
            self.queue_reading,
            |this, order, cx| this.put_the_queue_in_order(order, Direction::Ascending, cx),
            |this, reading, cx| this.put_the_queue_in_order(this.queue_order, reading, cx),
            cx,
        )
    }

    pub(crate) fn put_the_queue_in_order(
        &mut self,
        order: RowOrder,
        reading: Direction,
        cx: &mut Context<Self>,
    ) {
        self.queue_order = order;
        self.queue_reading = reading;

        let queued = self.player.read(cx).queued();
        if queued.rows.len() < 2 {
            cx.notify();
            return;
        }
        let player = self.player.read(cx).engine();
        let library = self.library.read(cx).catalog();

        self.queue_ordered = Some(cx.spawn(async move |this, cx| {
            let rows = Arc::clone(&queued.rows);
            let ordered: Vec<usize> = cx
                .background_executor()
                .spawn(async move { ordered_rows(&library, &player, &rows, order, reading) })
                .await;
            let landed = this.update(cx, |this, cx| {
                if this.player.read(cx).queued().revision != queued.revision {
                    return;
                }
                this.send(Command::Order(ordered), cx);
                cx.notify();
            });
            let _ = landed;
        }));
        cx.notify();
    }
}

fn ordered_rows(
    library: &Library,
    player: &Player,
    rows: &[QueueItem],
    order: RowOrder,
    reading: Direction,
) -> Vec<usize> {
    let tracks = queued_rows(library, rows);
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
                    heading.child(kit::figure(run.rows.rows().to_string()))
                })
                .when_some(from, |heading, from| {
                    heading.child(
                        div()
                            .flex_shrink()
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

pub(crate) fn took_out(rows: usize) -> Notice {
    Notice::Done(format!(
        "Took {} out of the queue",
        format::counted(rows, "track", "tracks")
    ))
}

fn puts_back(offer: Option<Offer>) -> SharedString {
    let behind = offer.map_or(0, |offer| offer.behind);
    let more = match behind {
        0 => "It is the last one there is to put back",
        1 => "One more gesture is behind it",
        _ => "More gestures are behind it",
    };

    SharedString::from(format!("{PUT_BACK_HINT}. {more}"))
}

fn takes_out_again(offer: Option<Offer>) -> SharedString {
    let behind = offer.map_or(0, |offer| offer.behind);
    let more = match behind {
        0 => "It is the last one there is to take out again",
        1 => "One more put back is behind it",
        _ => "More put backs are behind it",
    };

    SharedString::from(format!("{TAKE_AGAIN_HINT}. {more}"))
}

#[cfg(test)]
mod tests {
    use resonate_core::{MediaLocation, Span, TrackId};

    use super::{
        KEPT_GESTURES, Line, Offer, Part, QueueItem, QueueParts, Run, TakenBack, puts_back,
        takes_out_again, took_out,
    };

    fn queued(ids: &[u64]) -> Vec<QueueItem> {
        ids.iter()
            .map(|id| QueueItem {
                id: TrackId::new(*id).expect("a namable track"),
                location: MediaLocation::local(format!("/music/{id}.flac")),
                span: None,
            })
            .collect()
    }

    fn without(queue: &[QueueItem], rows: Span) -> Vec<QueueItem> {
        let mut left = queue.to_vec();
        left.drain(rows.range());
        left
    }

    fn back(kept: &mut TakenBack, queue: &mut Vec<QueueItem>, rows: Span) {
        kept.keeping(queue, rows);
        *queue = without(queue, rows);
    }

    fn put_back(kept: &mut TakenBack, queue: &mut Vec<QueueItem>) -> bool {
        let Some(taken) = kept.take(queue) else {
            return false;
        };
        queue.splice(taken.at..taken.at, taken.rows);
        true
    }

    fn take_again(kept: &mut TakenBack, queue: &mut Vec<QueueItem>) -> bool {
        let Some(rows) = kept.take_again(queue) else {
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
        let offer = kept.offered_again(&queue).expect("the put back is offered");
        assert_eq!((offer.rows, offer.behind), (1, 0));

        queue.insert(0, queued(&[9])[0].clone());
        assert!(kept.offered_again(&queue).is_some());
        assert!(take_again(&mut kept, &mut queue));
        assert_eq!(queue, queued(&[9, 2, 3]));
        assert!(put_back(&mut kept, &mut queue));

        back(&mut kept, &mut queue, Span::between(2, 2));
        assert_eq!(
            kept.offered_again(&queue),
            None,
            "a fresh gesture kept what was put back before it"
        );
    }

    #[test]
    fn the_offer_counts_what_is_behind_it() {
        let mut queue = queued(&[1, 2, 3, 4]);
        let mut kept = TakenBack::default();

        assert_eq!(kept.offered(&queue), None);

        back(&mut kept, &mut queue, Span::between(3, 3));
        let offer = kept.offered(&queue).expect("the gesture is offered back");
        assert_eq!((offer.rows, offer.behind), (1, 0));

        back(&mut kept, &mut queue, Span::between(0, 1));
        let offer = kept.offered(&queue).expect("the gesture is offered back");
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

        let offer = kept.offered(&queue).expect("the walk still stands");
        assert_eq!((offer.rows, offer.behind), (1, 1));
        assert!(put_back(&mut kept, &mut queue));
        assert_eq!(queue, queued(&[1, 9, 4]));
        assert!(put_back(&mut kept, &mut queue));
        assert_eq!(queue, queued(&[1, 2, 3, 9, 4]));
    }

    #[test]
    fn a_row_already_back_in_the_queue_is_not_put_back_twice() {
        let mut queue = queued(&[1, 2, 3]);
        let mut kept = TakenBack::default();

        back(&mut kept, &mut queue, Span::between(2, 2));
        queue.push(queued(&[3])[0].clone());

        assert_eq!(kept.offered(&queue), None);
        assert!(kept.take(&queue).is_none());
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

    #[test]
    fn the_walk_is_bounded_and_drops_its_oldest_step_first() {
        let held: Vec<u64> = (1..=(KEPT_GESTURES as u64 + 4)).collect();
        let mut queue = queued(&held);
        let mut kept = TakenBack::default();

        for _ in 0..held.len() - 1 {
            back(&mut kept, &mut queue, Span::between(0, 0));
        }

        let offer = kept.offered(&queue).expect("the last gesture is offered");
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
    library: u64,
    total: Duration,
}

impl QueueLength {
    const fn measures(self, now: Self) -> bool {
        self.queue == now.queue && self.library == now.library
    }
}
