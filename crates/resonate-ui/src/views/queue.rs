use std::{cmp::Ordering, sync::Arc, time::Duration};

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, SharedString, Task, div, prelude::*, px, rgb,
    uniform_list,
};
use resonate_core::{Span, TrackId};
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
                    queued
                        .rows
                        .iter()
                        .map(|item| queued_name(&library, &player, item))
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

fn queued_name(library: &Library, player: &Player, item: &QueueItem) -> String {
    match queued_row(library, item) {
        Some(track) => track.title,
        None => player
            .media(&item.location, item.span)
            .and_then(|info| info.tags.title.clone())
            .unwrap_or_else(|| format::stem(&item.location)),
    }
}

fn queued_row(library: &Library, item: &QueueItem) -> Option<Track> {
    match library.track(item.id) {
        Ok(Some(track)) if track.location == item.location => return Some(track),
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "a queued track could not be read by id"),
    }
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
    left: Vec<TrackId>,
}

impl TakenOut {
    fn of(queue: &[QueueItem], rows: Span) -> Option<Self> {
        let taken = queue.get(rows.range())?;
        if taken.is_empty() {
            return None;
        }

        let mut left: Vec<TrackId> = queue.iter().map(|item| item.id).collect();
        left.drain(rows.range());
        Some(Self {
            rows: taken.to_vec(),
            at: rows.first(),
            left,
        })
    }

    fn stands_over(&self, queue: &[QueueItem]) -> bool {
        queue.len() == self.left.len()
            && queue
                .iter()
                .map(|item| item.id)
                .eq(self.left.iter().copied())
    }
}

#[derive(Default)]
pub(crate) struct TakenBack {
    steps: Vec<TakenOut>,
}

impl TakenBack {
    pub(crate) fn keeping(&mut self, queue: &[QueueItem], rows: Span) -> Option<usize> {
        let taken = TakenOut::of(queue, rows)?;
        let kept = taken.rows.len();
        if self.standing(queue).is_none() {
            self.steps.clear();
        }
        if self.steps.len() == KEPT_GESTURES {
            self.steps.remove(0);
        }
        self.steps.push(taken);
        Some(kept)
    }

    fn standing(&self, queue: &[QueueItem]) -> Option<&TakenOut> {
        self.steps.last().filter(|taken| taken.stands_over(queue))
    }

    fn offered(&self, queue: &[QueueItem]) -> Option<Offer> {
        self.standing(queue).map(|taken| Offer {
            rows: taken.rows.len(),
            behind: self.steps.len().saturating_sub(1),
        })
    }

    fn take(&mut self, queue: &[QueueItem]) -> Option<TakenOut> {
        self.standing(queue)?;
        self.steps.pop()
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
        let lines = parts.lines();
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
                .child(
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
                                    None => continue,
                                };
                                let Some(item) = queue.get(index) else {
                                    continue;
                                };
                                let track =
                                    this.library.update(cx, |library, _| library.track_of(item));
                                let part = parts.part_of(index);
                                let current = part == Part::Playing;
                                let waiting = part == Part::Next;
                                let heard = part == Part::Heard;
                                let drawn = match track {
                                    Some(track) => listing::scanned(track),
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
                                let listed = row(current)
                                    .id(index)
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
                                    .child(listing::title_cell(drawn.title, Lit::new(), current))
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
                                            .reaches(album, artist_id)
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
                .child(Scrollbars::of(cx).vertical("queue-scrollbar", self.queue_rows.clone())),
            )
            .into_any_element()
    }

    fn holds_what_was_taken_out(&self, queue: &[QueueItem]) -> bool {
        self.took_out.offered(queue).is_some()
    }

    fn queue_length(&mut self, queue: &[QueueItem], cx: &mut Context<Self>) -> Duration {
        let measured = QueueLength {
            queue: self.player.read(cx).queued().revision,
            library: self.library.read(cx).revision(),
            total: Duration::ZERO,
        };
        if let Some(held) = self.queue_length
            && held.measures(measured)
        {
            return held.total;
        }

        let total = queue
            .iter()
            .filter_map(|item| {
                self.library
                    .update(cx, |library, _| library.track_of(item))
                    .and_then(|track| {
                        track
                            .duration
                            .map(|frames| frames.to_duration(track.spec.rate))
                    })
            })
            .sum();
        self.queue_length = Some(QueueLength { total, ..measured });
        total
    }

    fn queue_heading(&mut self, queue: &[QueueItem], cx: &mut Context<Self>) -> Div {
        let position = self.player.read(cx).state().queue_position;
        let total = self.queue_length(queue, cx);
        let mut under: format::Parts<String> =
            smallvec![format::counted(queue.len(), "track", "tracks")];
        if !total.is_zero() {
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
                                            let queued = this.player.read(cx).queue();
                                            let Some(taken) = this.took_out.take(&queued) else {
                                                return;
                                            };
                                            this.send(
                                                Command::Insert {
                                                    items: taken.rows,
                                                    at: Placement::At(taken.at),
                                                    play: false,
                                                },
                                                cx,
                                            );
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

        let queue = self.player.read(cx).queue();
        if queue.len() < 2 {
            cx.notify();
            return;
        }

        let mut keyed: Vec<Keyed> = queue
            .iter()
            .enumerate()
            .map(|(row, item)| self.keyed(row, item, cx))
            .collect();
        keyed.sort_by(|left, right| {
            let weighed = left.weighed(order, right);
            match reading {
                Direction::Ascending => weighed,
                Direction::Descending => weighed.reverse(),
            }
        });

        self.send(
            Command::Order(keyed.into_iter().map(|key| key.row).collect()),
            cx,
        );
        cx.notify();
    }

    fn keyed(&self, row: usize, item: &QueueItem, cx: &mut Context<Self>) -> Keyed {
        let track = self.library.update(cx, |library, _| library.track_of(item));
        let album = track
            .as_ref()
            .and_then(|track| track.album_id)
            .and_then(|id| {
                self.library
                    .update(cx, |library, _| library.album_title(id))
            });

        match track {
            Some(track) => Keyed {
                row,
                album: folded(album.as_deref()),
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
                let info = self.player.read(cx).media(&item.location, item.span);
                let tags = info.as_ref().map(|info| &info.tags);

                Keyed {
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

#[cfg(test)]
mod tests {
    use resonate_core::{MediaLocation, Span, TrackId};

    use super::{
        KEPT_GESTURES, Line, Offer, Part, QueueItem, QueueParts, Run, TakenBack, puts_back,
        took_out,
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
    fn a_queue_that_moved_under_the_offer_takes_the_whole_walk_with_it() {
        let mut queue = queued(&[1, 2, 3]);
        let mut kept = TakenBack::default();

        back(&mut kept, &mut queue, Span::between(2, 2));
        queue.push(queued(&[9])[0].clone());

        assert_eq!(
            kept.offered(&queue),
            None,
            "a moved queue was still offered"
        );
        assert!(kept.take(&queue).is_none());

        back(&mut kept, &mut queue, Span::between(0, 0));
        let offer = kept.offered(&queue).expect("the fresh gesture is offered");
        assert_eq!(
            offer.behind, 0,
            "a walk was carried over a queue that had moved"
        );
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
