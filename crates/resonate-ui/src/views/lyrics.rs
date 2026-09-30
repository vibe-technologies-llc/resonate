use std::{
    ops::Range,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, Bounds, Canvas, Context, Div, FontWeight, HighlightStyle, Hsla, MouseMoveEvent,
    Pixels, Point, ScrollWheelEvent, SharedString, StyledText, canvas, div, fill,
    linear_color_stop, linear_gradient, point, prelude::*, px, rgb, size,
};
use resonate_engine::{PlayerState, StreamDigest};
use resonate_lyrics::{Credits, Detail, Sweep, Voice, Wanted};

use crate::{
    Selection, clipboard,
    icons::Icon,
    lyrics::{Asked, Heard, Look, Measures, Reading, rising},
    theme,
    views::{
        browser::{OPEN_ALBUM_HINT, OPEN_ARTIST_HINT},
        hint::{self, Names},
        kit::{self, Tone},
        menu::{self, Menu},
        root::RootView,
        scrollbar::Scrollbars,
        transport::Playing,
    },
};

const COPY_LINE: &str = "Copy this line";

const COPY_SHEET: &str = "Copy the whole sheet";

const SEEK_TO_IT: &str = "Play from this line";

const NOTHING_PLAYING: &str = "Nothing is playing, so there is nothing to follow.";

const SEARCHING: &str = "Looking for lyrics…";

const UNSOURCED: &str = "No lyric source is configured, so nothing is looked up.";

const MISSING: &str = "No source had lyrics for this track.";

const SIDECAR_HINT: &str = "An .lrc or .txt beside the file or in a Lyrics folder next to it, or \
                            lyrics embedded in its tags, is what the pane reads.";

const FOLLOW_HINT: &str = "Follow the track again — a scroll of your own holds the pane where you \
                           left it";

const PRESS_HINT: &str = "Press any line to jump the transport to the moment it is sung at. A \
                          scroll of your own holds the pane where you leave it until Follow puts \
                          it back on the track.";

const WORD_SYNCED: &str = "WORD-SYNCED";

const SYNCED: &str = "SYNCED";

const UNSYNCED: &str = "UNSYNCED";

const BY: &str = "by";

const WORDS_BY: &str = "Words by";

const SHEET_BY: &str = "Sheet by";

const LAID_DOWN_WITH: &str = "Laid down with";

const FROM_THE_RECORD: &str = "From";

const END_OF_LYRICS: &str = "END OF LYRICS";

const BREATH_DOTS: usize = 3;

const DIM_DOT: f32 = 0.22;

const DOT_SWELL: f32 = 1.6;

const A_VOICE_OF_TWO_SPANS: f32 = 0.84;

const DOWNWARDS: f32 = 180.0;

const UNSUNG_SHARE: f32 = 0.38;

struct Attributed {
    timed: &'static str,
    source: SharedString,
    credit: Option<Credit>,
}

struct Credit {
    shown: SharedString,
    whole: SharedString,
}

struct Line {
    index: usize,
    text: SharedString,
    voice: Voice,
    show_voice: bool,
    two_voices: bool,
    at: Option<Duration>,
    standing: f32,
    lead: f32,
    offset: Pixels,
    width: Pixels,
    breathes: bool,
    breath: Option<Breath>,
    sweep: Option<Sweep>,
    resting: Option<Pixels>,
}

#[derive(Clone, Copy)]
struct Ending {
    standing: f32,
    offset: Pixels,
    width: Pixels,
}

#[derive(Clone, Copy)]
struct Breath {
    through: f32,
    swell: f32,
    opacity: f32,
}

impl RootView {
    pub(crate) fn look_for_lyrics(&mut self, cx: &mut Context<Self>) {
        let model = self.player.read(cx);
        let state = model.state().clone();
        let digest = model.digest();
        let asked = Asked::of(&state, digest.as_deref());
        if !self.lyrics.read(cx).asks_again(asked) {
            return;
        }

        let wanted = self.wanted_lyrics(&state, digest.as_deref(), cx);
        self.lyrics
            .update(cx, |model, cx| model.follow(asked, wanted, cx));
    }

    pub(crate) fn lyrics_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.look_for_lyrics(cx);

        let model = self.player.read(cx);
        let state = model.state().clone();
        let digest = model.digest();

        let Some(current) = state.current else {
            return kit::empty(Icon::Lyrics, NOTHING_PLAYING, None);
        };
        let playing = self.playing(&state, digest.as_deref(), cx);
        let now = Instant::now();
        let heard = Heard::of(&state);
        let position = match heard {
            Some(heard) => self
                .lyrics
                .update(cx, |model, _| model.keep_time(heard, now)),
            None => current.position.to_duration(current.source.rate),
        };
        let sounding = heard.is_some_and(|heard| heard.playing);

        let moving = self.lyrics.update(cx, |model, _| {
            model.follow_the_track(position, now);
            let placing = model.place(now);

            placing || model.is_turning(now)
        });

        let (look, synced, following, sourced, quiet) = {
            let model = self.lyrics.read(cx);
            (
                model.look().clone(),
                model.is_synced(),
                model.following(now),
                model.has_a_source(),
                model.looks_quietly(now),
            )
        };
        let heading = self.lyrics_heading(&playing, &look, synced, following, cx);

        let body = match &look {
            Look::Searching if quiet => quietly(moving),
            Look::Searching => kit::empty(Icon::Lyrics, SEARCHING, None),
            Look::Nothing | Look::Missing if sourced => {
                kit::empty(Icon::Lyrics, MISSING, Some(SIDECAR_HINT))
            }
            Look::Nothing | Look::Missing => kit::empty(Icon::Lyrics, UNSOURCED, None),
            Look::Refused(notice) => notice_of(notice.clone()),
            Look::Found(_) => {
                self.lyric_lines(position, now, synced, moving || (synced && sounding), cx)
            }
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .child(heading)
            .child(body)
            .into_any_element()
    }

    fn lyrics_heading(
        &self,
        playing: &Playing,
        look: &Look,
        synced: bool,
        following: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let chosen = self.lyrics.read(cx).reading();

        kit::heading().child(
            kit::heading_row()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap_1()
                        .child(kit::eyebrow("LYRICS"))
                        .child(kit::linked_title(self.opens(
                            "lyrics-title",
                            playing.title.clone(),
                            OPEN_ALBUM_HINT,
                            playing.cover.album.map(Selection::Album),
                            cx,
                        )))
                        .child(kit::linked_subtitle(self.opens(
                            "lyrics-artist",
                            playing.artist.clone(),
                            OPEN_ARTIST_HINT,
                            playing.artist_id.map(Selection::Artist),
                            cx,
                        ))),
                )
                .child(
                    kit::actions()
                        .when(synced && !following, |actions| {
                            actions.child(
                                kit::button(
                                    "follow-lyrics",
                                    Some(Icon::Lyrics),
                                    "Follow",
                                    FOLLOW_HINT,
                                    Tone::Outlined,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.lyrics.update(cx, |model, _| model.follow_again());
                                        cx.notify();
                                    },
                                )),
                            )
                        })
                        .when(synced, |actions| {
                            actions.children(Reading::ALL.into_iter().enumerate().map(
                                |(index, reading)| {
                                    kit::chip(
                                        ("lyric-reading", index),
                                        reading.label(),
                                        reading == chosen,
                                    )
                                    .names(reading.about())
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.lyrics
                                                .update(cx, |model, _| model.read_as(reading));
                                            cx.notify();
                                        },
                                    ))
                                },
                            ))
                        })
                        .when(synced, |actions| {
                            actions.child(hint::explains("lyric-press", PRESS_HINT))
                        })
                        .when_some(attribution(look), |actions, attributed| {
                            actions
                                .child(kit::badge(attributed.timed, theme::muted()))
                                .child(kit::figure(attributed.source))
                                .when_some(attributed.credit, |actions, credit| {
                                    actions.child(
                                        kit::figure(credit.shown)
                                            .id("lyric-credit")
                                            .names(credit.whole),
                                    )
                                })
                        }),
                ),
        )
    }

    fn lyric_lines(
        &self,
        position: Duration,
        now: Instant,
        synced: bool,
        moving: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (scroll, edge, measures, shown, drawn, ending, moved_by_hand) = {
            let model = self.lyrics.read(cx);
            let text = model.text();
            let moments = model.moments();
            let voices = model.voices();
            let breathes = model.breathes();
            let has_two_voices = model.has_two_voices();
            let waiting = model.waiting_at(position);
            let in_play = model.in_play(position);
            let swell = model.breath(now);
            let width = model.column_width();

            let drawn: Vec<Line> = (0..text.len())
                .map(|index| {
                    let breath = model.breath_at(waiting, index, now).map(|breath| Breath {
                        through: breath.through,
                        swell,
                        opacity: breath.opacity,
                    });
                    let standing = model.standing(index, now);

                    Line {
                        index,
                        text: text[index].clone(),
                        voice: voices[index],
                        show_voice: has_two_voices
                            && !text[index].is_empty()
                            && (index == 0
                                || text[index - 1].is_empty()
                                || voices[index - 1] != voices[index]),
                        two_voices: has_two_voices,
                        at: synced
                            .then(|| moments.get(index).copied().flatten())
                            .flatten(),
                        standing: breath
                            .map_or(standing, |breath| rising(standing, breath.through)),
                        lead: model.lead(index, now),
                        offset: model.inset(index, now),
                        width,
                        breathes: breathes.get(index).copied().unwrap_or(false),
                        breath,
                        sweep: in_play
                            .contains(&Some(index))
                            .then(|| model.sweep(index, position))
                            .flatten(),
                        resting: model.resting_height(index),
                    }
                })
                .collect();

            let ending = synced.then(|| {
                let end = model.end_of_the_sheet();
                Ending {
                    standing: model.standing(end, now),
                    offset: model.inset(end, now),
                    width,
                }
            });
            let shown = if model.is_placed() {
                model.arrival(now)
            } else {
                0.0
            };

            (
                model.scroll().clone(),
                model.edge(),
                model.measures(),
                shown,
                drawn,
                ending,
                model.moved_by_hand_lately(now),
            )
        };

        let sweeping = drawn.iter().any(|line| {
            line.sweep
                .as_ref()
                .is_some_and(|sweep| sweep.singing.is_some())
        });
        let lines: Vec<AnyElement> = drawn
            .into_iter()
            .map(|line| self.lyric(line, measures, cx))
            .collect();

        let column = div()
            .id("lyric-sheet")
            .track_scroll(&scroll)
            .overflow_y_scroll()
            .on_scroll_wheel(cx.listener(|this, _: &ScrollWheelEvent, _, cx| {
                this.lyrics
                    .update(cx, |model, _| model.led_by_hand(Instant::now()));
                cx.notify();
            }))
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .items_center()
            .gap(measures.spacing)
            .px(theme::width(theme::lyric_gutter()))
            .when(synced, |sheet| sheet.pt(edge))
            .when(!synced, |sheet| sheet.py(measures.margin))
            .children(lines)
            .when_some(ending, |sheet, ending| {
                sheet.child(end_of_the_words(ending))
            })
            .when(synced, |sheet| sheet.child(reaches_the_middle(edge)));

        div()
            .id("lyric-body")
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .opacity(shown)
            .child(column)
            .child(Scrollbars::of(cx).vertical_while("lyrics-scrollbar", scroll, moved_by_hand))
            .child(dissolving(true))
            .child(dissolving(false))
            .child(self.follows_the_pointer(cx))
            .child(asks_for_a_frame(moving || sweeping))
            .into_any_element()
    }

    fn follows_the_pointer(&self, cx: &mut Context<Self>) -> Canvas<()> {
        let watching = cx.entity();

        canvas(
            |_, _, _| (),
            move |_, (), window, _| {
                let moved = watching.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, _, _, cx| {
                    let at = event.position;
                    moved.update(cx, |this, cx| {
                        let near = this.lyrics.read(cx).opened_by(at);
                        if this.lyrics.update(cx, |model, _| model.open_out(near)) {
                            cx.notify();
                        }
                    });
                });
            },
        )
        .absolute()
        .size(px(0.0))
    }

    fn lyric(&self, line: Line, measures: Measures, cx: &mut Context<Self>) -> AnyElement {
        if let Some(height) = line.resting {
            return div().flex_none().w(line.width).h(height).into_any_element();
        }
        let second = line.voice == Voice::Two;
        let row = div()
            .flex()
            .flex_col()
            .w(line.width)
            .flex_none()
            .when(second, |row| row.items_end())
            .when(line.two_voices && !second, |row| row.items_start())
            .when(!line.two_voices, |row| row.items_center());
        if line.text.is_empty() {
            return row.h(measures.pause).into_any_element();
        }
        let carried = div()
            .relative()
            .top(line.offset)
            .w_full()
            .flex()
            .flex_col()
            .when(line.breathes, |carried| {
                carried.child(breath_room(
                    line.breath,
                    second,
                    line.two_voices,
                    measures.breath,
                ))
            });

        let words = line.text.clone();
        let pad = theme::lyric_pad();
        let inside = (f32::from(line.width) - pad * 2.0).max(0.0);
        let share = if line.two_voices {
            A_VOICE_OF_TWO_SPANS
        } else {
            1.0
        };
        let drawn = div()
            .w_full()
            .flex()
            .flex_col()
            .when(second, |line| line.items_end())
            .when(line.two_voices && !second, |line| line.items_start())
            .when(!line.two_voices, |line| line.items_center())
            .gap(measures.padding)
            .px(theme::width(pad))
            .py(measures.padding)
            .rounded_xl()
            .opacity(line.standing)
            .when(line.show_voice, |line| {
                line.child(
                    div()
                        .text_size(px(theme::text_xs()))
                        .line_height(measures.label)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::muted()))
                        .child(if second { "VOICE 2" } else { "VOICE 1" }),
                )
            })
            .child(
                div()
                    .w(px(inside * share))
                    .when(second, |words| words.text_right())
                    .when(line.two_voices && !second, |words| words.text_left())
                    .when(!line.two_voices, |words| words.text_center())
                    .text_size(px(theme::text_lyric()))
                    .line_height(measures.leading)
                    .font_weight(FontWeight::BOLD)
                    .map(|words| {
                        let lit = if second {
                            theme::accent()
                        } else {
                            theme::text()
                        };
                        let sung = mixed(theme::muted(), lit, line.lead);
                        match line.sweep {
                            Some(sweep) => {
                                let waiting = mixed(theme::background(), lit, UNSUNG_SHARE);
                                let unsung = mixed(theme::muted(), waiting, line.lead);
                                words
                                    .text_color(rgb(unsung))
                                    .child(swept(line.text, &sweep, unsung, sung))
                            }
                            None => words.text_color(rgb(sung)).child(line.text),
                        }
                    }),
            );

        let Some(at) = line.at else {
            return row
                .child(carried.child(menu::opens_a_menu(
                    drawn.id(("lyric", line.index)),
                    move |this, where_at, cx| this.copies_a_lyric(where_at, &words, cx),
                    cx,
                )))
                .into_any_element();
        };

        let sung = menu::opens_a_menu(
            drawn
                .id(("lyric", line.index))
                .cursor_pointer()
                .hover(|line| line.bg(theme::tinted(theme::text(), 0x0e)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.lyrics.update(cx, |model, _| model.follow_again());
                    this.seek_to_moment(at, cx);
                })),
            move |this, where_at, cx| {
                this.copies_a_lyric(where_at, &words, cx).apart().does(
                    Icon::Lyrics,
                    SEEK_TO_IT,
                    move |this, _, cx| {
                        this.lyrics.update(cx, |model, _| model.follow_again());
                        this.seek_to_moment(at, cx);
                    },
                )
            },
            cx,
        );

        row.child(carried.child(sung)).into_any_element()
    }

    fn copies_a_lyric(&self, at: Point<Pixels>, line: &SharedString, cx: &Context<Self>) -> Menu {
        let words = line.to_string();
        let whole = self
            .lyrics
            .read(cx)
            .text()
            .iter()
            .map(SharedString::to_string)
            .collect::<Vec<String>>()
            .join("\n");

        Menu::at(at)
            .when_some(
                (!words.trim().is_empty()).then_some(words),
                |menu, words| {
                    menu.does(Icon::Export, COPY_LINE, move |_, _, cx| {
                        clipboard::copy(words.clone(), cx);
                    })
                },
            )
            .does(Icon::Export, COPY_SHEET, move |_, _, cx| {
                clipboard::copy(whole.clone(), cx);
            })
    }

    fn wanted_lyrics(
        &self,
        state: &PlayerState,
        digest: Option<&StreamDigest>,
        cx: &mut Context<Self>,
    ) -> Option<Wanted> {
        let current = state.current?;
        let item = self.queued_row(state, cx)?;
        let tags = digest
            .filter(|digest| digest.track == current.id)
            .map(|digest| &digest.info.tags);
        let title = tags.and_then(|tags| tags.title.clone());
        let artist = tags.and_then(|tags| tags.artist.clone());
        let album = tags.and_then(|tags| tags.album.clone());
        let carried = tags.and_then(|tags| tags.lyrics.clone());
        let (scanned, scanned_album) = self.library.update(cx, |library, _| {
            let scanned = library.track_of(&item);
            let album = scanned
                .as_ref()
                .and_then(|track| track.album_id)
                .and_then(|album| library.album_title(album));
            (scanned, album)
        });
        let scanned_duration = scanned.as_ref().and_then(|track| {
            track
                .duration
                .map(|duration| duration.to_duration(track.spec.rate))
        });

        Some(Wanted {
            location: item.location,
            span: item.span,
            rate: Some(current.source.rate),
            title: title.or_else(|| scanned.as_ref().map(|track| track.title.clone())),
            artist: artist.or_else(|| scanned.as_ref().and_then(|track| track.artist.clone())),
            album: album.or(scanned_album),
            duration: current
                .duration
                .map(|duration| duration.to_duration(current.source.rate))
                .or(scanned_duration),
            carried,
        })
    }
}

fn swept(text: SharedString, sweep: &Sweep, unsung: u32, sung: u32) -> StyledText {
    let coloured = |colour: u32| HighlightStyle {
        color: Some(rgb(colour).into()),
        ..HighlightStyle::default()
    };
    let wiped = wiped(&text, sweep);
    let mut runs = Vec::with_capacity(2);
    if wiped.sung_to > 0 {
        runs.push((0..wiped.sung_to, coloured(sung)));
    }
    if let Some((letter, share)) = wiped.blending {
        runs.push((letter, coloured(mixed(unsung, sung, share))));
    }

    StyledText::new(text).with_highlights(runs)
}

#[derive(Debug, PartialEq)]
struct Wiped {
    sung_to: usize,
    blending: Option<(Range<usize>, f32)>,
}

fn wiped(text: &str, sweep: &Sweep) -> Wiped {
    let Some(singing) = sweep
        .singing
        .as_ref()
        .filter(|singing| !singing.word.is_empty())
    else {
        return Wiped {
            sung_to: sweep.sung,
            blending: None,
        };
    };
    let start = singing.word.start;
    let letters: Vec<(usize, char)> = text
        .get(singing.word.clone())
        .unwrap_or_default()
        .trim_end()
        .char_indices()
        .collect();
    let across = singing.through.clamp(0.0, 1.0) * letters.len() as f32;
    let whole = across.floor() as usize;
    let share = across - whole as f32;
    let Some(&(at, letter)) = letters.get(whole) else {
        let end = letters
            .last()
            .map_or(start, |&(at, letter)| start + at + letter.len_utf8());
        return Wiped {
            sung_to: end,
            blending: None,
        };
    };
    let letter = start + at..start + at + letter.len_utf8();

    Wiped {
        sung_to: letter.start,
        blending: (share > 0.0).then_some((letter, share)),
    }
}

fn mixed(from: u32, to: u32, share: f32) -> u32 {
    let channel = |shift: u32| {
        let one = ((from >> shift) & 0xff) as f32;
        let other = ((to >> shift) & 0xff) as f32;

        ((other - one).mul_add(share, one).round() as u32) << shift
    };

    channel(16) | channel(8) | channel(0)
}

fn attribution(look: &Look) -> Option<Attributed> {
    let Look::Found(lyrics) = look else {
        return None;
    };
    let timed = match lyrics.detail() {
        Detail::Words => WORD_SYNCED,
        Detail::Lines => SYNCED,
        Detail::Unsynced => UNSYNCED,
    };

    Some(Attributed {
        timed,
        source: SharedString::from(lyrics.source().to_string()),
        credit: credited(lyrics.credits()),
    })
}

fn credited(credits: &Credits) -> Option<Credit> {
    let laid_down = laid_down_with(credits);
    let shown = credits
        .sheet_by
        .as_deref()
        .or(credits.words_by.as_deref())
        .map(str::to_owned)
        .or_else(|| laid_down.clone())
        .map(|whoever| format!("{BY} {whoever}"))?;
    let whole: Vec<String> = [
        credits
            .words_by
            .as_deref()
            .map(|words| format!("{WORDS_BY} {words}.")),
        credits
            .sheet_by
            .as_deref()
            .map(|sheet| format!("{SHEET_BY} {sheet}.")),
        laid_down.map(|editor| format!("{LAID_DOWN_WITH} {editor}.")),
        credits
            .album
            .as_deref()
            .map(|album| format!("{FROM_THE_RECORD} {album}.")),
    ]
    .into_iter()
    .flatten()
    .collect();

    Some(Credit {
        shown: shown.into(),
        whole: whole.join(" ").into(),
    })
}

fn laid_down_with(credits: &Credits) -> Option<String> {
    let editor = credits.editor.as_deref()?;

    Some(match credits.version.as_deref() {
        Some(version) => format!("{editor} {version}"),
        None => editor.to_owned(),
    })
}

fn reaches_the_middle(edge: Pixels) -> Div {
    div().flex_none().w_full().h(edge)
}

fn end_of_the_words(ending: Ending) -> Div {
    let rule = || {
        div()
            .h(px(1.0))
            .w(theme::width(theme::lyric_end_rule()))
            .bg(rgb(theme::faint()))
    };

    div()
        .flex()
        .flex_col()
        .items_center()
        .flex_none()
        .w(ending.width)
        .child(
            div()
                .relative()
                .top(ending.offset)
                .flex()
                .items_center()
                .justify_center()
                .gap_3()
                .py_4()
                .opacity(ending.standing)
                .child(rule())
                .child(
                    div()
                        .text_size(px(theme::text_xs()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::muted()))
                        .child(END_OF_LYRICS),
                )
                .child(rule()),
        )
}

fn breather(breath: Breath) -> Div {
    let widest = theme::lyric_dot() + DOT_SWELL;
    let apart = widest + theme::lyric_dot_gap();
    #[expect(clippy::cast_precision_loss, reason = "three dots fit an f32 exactly")]
    let dots = BREATH_DOTS as f32;
    let accent = Hsla::from(rgb(theme::accent()));

    let swelling = canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, (), window, _| {
            let middle = bounds.center();
            for dot in 0..BREATH_DOTS {
                #[expect(clippy::cast_precision_loss, reason = "three dots fit an f32 exactly")]
                let dot = dot as f32;
                let lit = breath.through.mul_add(dots, -dot).clamp(0.0, 1.0);
                let side =
                    px((DOT_SWELL * breath.swell)
                        .mul_add(lit.mul_add(0.5, 0.5), theme::lyric_dot()));
                let centre = point(middle.x + px(apart * (dot - (dots - 1.0) / 2.0)), middle.y);
                let shade = accent.opacity((1.0 - DIM_DOT).mul_add(lit, DIM_DOT));
                window.paint_quad(
                    fill(Bounds::centered_at(centre, size(side, side)), shade)
                        .corner_radii(side / 2.0),
                );
            }
        },
    )
    .w(px(apart.mul_add(dots - 1.0, widest)))
    .h(px(widest));

    div().opacity(breath.opacity).child(swelling)
}

fn breath_room(breath: Option<Breath>, second: bool, two_voices: bool, height: Pixels) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(height)
        .px(theme::width(theme::lyric_pad()))
        .when(second, |room| room.justify_end())
        .when(two_voices && !second, |room| room.justify_start())
        .when(!two_voices, |room| room.justify_center())
        .when_some(breath, |room, breath| room.child(breather(breath)))
}

fn dissolving(from_the_top: bool) -> Div {
    let ground = theme::background();
    let (near, far) = if from_the_top {
        (theme::tinted(ground, 0xff), theme::tinted(ground, 0x00))
    } else {
        (theme::tinted(ground, 0x00), theme::tinted(ground, 0xff))
    };
    let edge = div()
        .absolute()
        .left_0()
        .right_0()
        .h(px(theme::lyric_edge()))
        .bg(linear_gradient(
            DOWNWARDS,
            linear_color_stop(near, 0.0),
            linear_color_stop(far, 1.0),
        ));

    if from_the_top {
        edge.top_0()
    } else {
        edge.bottom_0()
    }
}

fn asks_for_a_frame(moving: bool) -> Canvas<()> {
    canvas(
        move |_, window, _| {
            if moving {
                window.request_animation_frame();
            }
        },
        |_, (), _, _| {},
    )
    .absolute()
    .size(px(0.0))
}

fn quietly(moving: bool) -> AnyElement {
    div()
        .flex_1()
        .child(asks_for_a_frame(moving))
        .into_any_element()
}

fn notice_of(text: SharedString) -> AnyElement {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .px_8()
        .text_color(rgb(theme::failure()))
        .child(text)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use resonate_lyrics::Singing;

    use super::*;

    fn singing(sung: usize, word: Range<usize>, through: f32) -> Sweep {
        Sweep {
            sung,
            singing: Some(Singing { word, through }),
        }
    }

    #[test]
    fn a_word_being_sung_is_wiped_across_letter_by_letter() {
        let text = "Stay until the morning";

        assert_eq!(
            wiped(text, &singing(5, 5..11, 0.0)),
            Wiped {
                sung_to: 5,
                blending: None
            }
        );
        assert_eq!(
            wiped(text, &singing(5, 5..11, 0.5)),
            Wiped {
                sung_to: 7,
                blending: Some((7..8, 0.5))
            }
        );
        assert_eq!(
            wiped(text, &singing(5, 5..11, 1.0)),
            Wiped {
                sung_to: 10,
                blending: None
            }
        );
    }

    #[test]
    fn a_letter_written_in_several_bytes_is_wiped_whole() {
        let text = "żółw";
        let wiped = wiped(text, &singing(0, 0..text.len(), 0.3));

        assert_eq!(wiped.sung_to, 2);
        let (letter, share) = wiped.blending.expect("a letter half sung");
        assert_eq!(&text[letter], "ó");
        assert!((share - 0.2).abs() < 1e-4);
    }

    fn crediting(credits: Credits) -> Option<Credit> {
        credited(&credits)
    }

    #[test]
    fn a_sheet_crediting_nobody_and_nothing_draws_no_credit_at_all() {
        assert!(crediting(Credits::default()).is_none());
        assert!(
            crediting(Credits {
                album: Some("Meddle".to_owned()),
                ..Credits::default()
            })
            .is_none()
        );
    }

    #[test]
    fn whoever_laid_the_sheet_down_is_what_the_row_draws_and_the_rest_is_behind_the_pointer() {
        let credit = crediting(Credits {
            album: Some("Meddle".to_owned()),
            words_by: Some("Roger Waters".to_owned()),
            sheet_by: Some("a stranger".to_owned()),
            editor: Some("LRCGET".to_owned()),
            version: Some("0.5".to_owned()),
        })
        .expect("a sheet that credits itself");

        assert_eq!(credit.shown, "by a stranger");
        assert_eq!(
            credit.whole,
            "Words by Roger Waters. Sheet by a stranger. Laid down with LRCGET 0.5. From Meddle."
        );
    }

    #[test]
    fn the_author_of_the_words_stands_in_where_nobody_signed_the_sheet() {
        let credit = crediting(Credits {
            words_by: Some("Roger Waters".to_owned()),
            ..Credits::default()
        })
        .expect("a sheet that credits itself");

        assert_eq!(credit.shown, "by Roger Waters");
        assert_eq!(credit.whole, "Words by Roger Waters.");
    }

    #[test]
    fn the_editor_stands_in_where_nobody_is_named_at_all() {
        let credit = crediting(Credits {
            editor: Some("LRCGET".to_owned()),
            ..Credits::default()
        })
        .expect("a sheet that names its editor");

        assert_eq!(credit.shown, "by LRCGET");
        assert_eq!(credit.whole, "Laid down with LRCGET.");
    }

    #[test]
    fn a_version_with_no_editor_behind_it_names_nothing() {
        assert!(
            crediting(Credits {
                version: Some("0.5".to_owned()),
                ..Credits::default()
            })
            .is_none()
        );
    }
}
