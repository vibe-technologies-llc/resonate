use std::{cmp::Ordering, sync::Arc, time::Duration};

use gpui::{
    AnyElement, ClickEvent, Context, Div, SharedString, div, prelude::*, px, rgb, uniform_list,
};
use resonate_core::{Span, TrackId};
use resonate_engine::{Command, Placement, QueueItem};
use resonate_library::{Cut, Direction, Favoured, RowOrder};

use crate::{
    Notice, Selection, format,
    icons::Icon,
    theme,
    views::{
        browser::{OPEN_ARTIST_HINT, row_controls},
        hint,
        kit::{self, KeepsItsWidth, Tone},
        listing::{self, Pictured},
        menu::{self, Menu},
        playlists::{self, Held, ROW_GROUP},
        reorder::{self, Carried, MOVING_HINT, Shift, Step},
        root::{RootView, empty, row},
        sorting,
    },
};

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
    pub(crate) fn keeping(&mut self, queue: &[QueueItem], rows: Span) {
        let Some(taken) = TakenOut::of(queue, rows) else {
            return;
        };
        if self.standing(queue).is_none() {
            self.steps.clear();
        }
        if self.steps.len() == KEPT_GESTURES {
            self.steps.remove(0);
        }
        self.steps.push(taken);
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
    pub(crate) fn queue_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
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

        let position = self.player.read(cx).state().queue_position;
        let queued = queue.len();
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
            .child(listing::columns("", true, sorting::queue_sorted(self), cx))
            .child(
                reorder::follows_a_drag(
                    div().id("queue-rows").flex().flex_1().min_h(px(0.0)),
                    scroll.clone(),
                    queued,
                    cx,
                )
                .child(
                    uniform_list(
                        "queue",
                        queue.len(),
                        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                            let mut rows = Vec::new();
                            for index in range {
                                let Some(item) = queue.get(index) else {
                                    continue;
                                };
                                let track =
                                    this.library.update(cx, |library, _| library.track_of(item));
                                let current = position == Some(index);
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
                                let menued = Arc::clone(&holding);
                                let listed = row(current)
                                    .id(index)
                                    .group(ROW_GROUP)
                                    .cursor_pointer()
                                    .hover(|entry| entry.bg(rgb(theme::hover())))
                                    .child(if current {
                                        listing::playing_mark()
                                    } else {
                                        listing::number_cell(SharedString::from(
                                            (index + 1).to_string(),
                                        ))
                                    })
                                    .child(cover)
                                    .child(listing::title_cell(drawn.title, Vec::new(), current))
                                    .child(listing::artist_cell(
                                        this.opens(
                                            ("queue-artist", index),
                                            drawn.artist,
                                            OPEN_ARTIST_HINT,
                                            drawn.artist_id.map(Selection::Artist),
                                            cx,
                                        )
                                        .keeps_its_width(),
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
                                            if !menu::pressed(event) {
                                                return;
                                            }
                                            let extending = event.modifiers().shift;
                                            this.reach_at(Shift::Queue, index, extending, cx);
                                            if !extending {
                                                this.send(Command::JumpTo(index), cx);
                                            }
                                        },
                                    ));
                                let listed = menu::opens_a_menu(
                                    listed,
                                    move |this, at, _| {
                                        let taken = this.acting_on(Shift::Queue, index);
                                        let put = Arc::clone(&menued);

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
                                            .offers_the_file(location.clone())
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

                                rows.push(reorder::movable(listed, index, carried, reached, cx));
                            }
                            rows
                        }),
                    )
                    .track_scroll(scroll)
                    .h_full()
                    .w_full(),
                ),
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
        let mut under = vec![format::counted(queue.len(), "track", "tracks")];
        if !total.is_zero() {
            under.push(format::spanned(total));
        }
        if let Some(position) = position {
            under.push(format!("on row {}", position + 1));
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
            .when_some(taken_out, |heading, offer| {
                heading.child(listing::noticed(&Notice::Done(took_out(offer))))
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

fn took_out(offer: Offer) -> String {
    let taken = format!(
        "Took {} out of the queue",
        format::counted(offer.rows, "track", "tracks")
    );

    match offer.behind {
        0 => taken,
        behind => format!(
            "{taken} · {} to walk back through",
            format::counted(behind + 1, "gesture", "gestures")
        ),
    }
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

    use super::{KEPT_GESTURES, Offer, QueueItem, TakenBack, puts_back, took_out};

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
        assert_eq!(
            took_out(Offer { rows: 3, behind: 0 }),
            "Took 3 tracks out of the queue"
        );
        assert_eq!(
            took_out(Offer { rows: 1, behind: 2 }),
            "Took 1 track out of the queue · 3 gestures to walk back through"
        );
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
