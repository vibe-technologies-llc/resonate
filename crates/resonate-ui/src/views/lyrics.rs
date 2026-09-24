use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Canvas, Context, Div, FontWeight, MouseMoveEvent, Pixels, Point, ScrollWheelEvent,
    SharedString, canvas, div, linear_color_stop, linear_gradient, prelude::*, px, rgb,
};
use resonate_engine::{PlayerState, StreamDigest};
use resonate_lyrics::{Credits, Timing, Voice, Waiting, Wanted};

use crate::{
    Selection, clipboard,
    icons::Icon,
    lyrics::{Asked, Look, Reading, rising},
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

const SYNCED: &str = "SYNCED";

const UNSYNCED: &str = "UNSYNCED";

const BY: &str = "by";

const WORDS_BY: &str = "Words by";

const SHEET_BY: &str = "Sheet by";

const LAID_DOWN_WITH: &str = "Laid down with";

const FROM_THE_RECORD: &str = "From";

const BREATH_DOTS: usize = 3;

const DIM_DOT: f32 = 0.22;

const DOT_SWELL: f32 = 1.6;

const LEADING: f32 = 1.28;

const SIZE_STEPS_PER_PIXEL: f32 = 8.0;

const DOWNWARDS: f32 = 180.0;

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
    breath: Option<Breath>,
}

#[derive(Clone, Copy)]
struct Breath {
    through: f32,
    swell: f32,
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
        let position = current.position.to_duration(current.source.rate);
        let now = Instant::now();

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
            Look::Found(_) => self.lyric_lines(position, now, synced, moving, cx),
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
                        .child(kit::title(self.opens(
                            "lyrics-title",
                            playing.title.clone(),
                            OPEN_ALBUM_HINT,
                            playing.cover.album.map(Selection::Album),
                            cx,
                        )))
                        .child(kit::subtitle(self.opens(
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
        let (scroll, edge, shown, drawn) = {
            let model = self.lyrics.read(cx);
            let text = model.text();
            let moments = model.moments();
            let voices = model.voices();
            let has_two_voices = model.has_two_voices();
            let waiting = model.waiting_at(position);
            let swell = model.breath(now);
            let width = model.column_width();

            let drawn: Vec<Line> = (0..text.len())
                .map(|index| {
                    let breath = breath_of(waiting, index).map(|through| Breath { through, swell });
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
                        offset: model.lag(index, now) + model.rise(index, now),
                        width,
                        breath,
                    }
                })
                .collect();
            let shown = if model.is_placed() {
                model.arrival(now)
            } else {
                0.0
            };

            (model.scroll().clone(), model.edge(), shown, drawn)
        };

        let lines: Vec<AnyElement> = drawn.into_iter().map(|line| self.lyric(line, cx)).collect();

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
            .gap_1()
            .px(theme::width(theme::lyric_gutter()))
            .when(synced, |sheet| sheet.pt(edge))
            .when(!synced, |sheet| sheet.py_12())
            .children(lines)
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
            .child(Scrollbars::of(cx).vertical("lyrics-scrollbar", scroll))
            .child(dissolving(true))
            .child(dissolving(false))
            .child(self.follows_the_pointer(cx))
            .child(asks_for_a_frame(moving))
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

    fn lyric(&self, line: Line, cx: &mut Context<Self>) -> AnyElement {
        let second = line.voice == Voice::Two;
        let row = div()
            .flex()
            .flex_col()
            .w(line.width)
            .flex_none()
            .when(second, |row| row.items_end())
            .when(line.two_voices && !second, |row| row.items_start())
            .when(!line.two_voices, |row| row.items_center());
        let carried = div().relative().top(line.offset).w_full();

        if line.text.is_empty() {
            return match line.breath {
                Some(breath) => row
                    .child(carried.child(breather(breath)))
                    .into_any_element(),
                None => row.h(theme::width(theme::lyric_break())).into_any_element(),
            };
        }

        let words = line.text.clone();
        let lit = theme::text_lyric_lead();
        let pad = theme::lyric_pad();
        let size = (lit - theme::text_lyric()).mul_add(line.lead, theme::text_lyric());
        let size = (size * SIZE_STEPS_PER_PIXEL).round() / SIZE_STEPS_PER_PIXEL;
        let inside = (f32::from(line.width) - pad * 2.0).max(0.0);
        let drawn = carried
            .flex()
            .flex_col()
            .when(second, |line| line.items_end())
            .when(line.two_voices && !second, |line| line.items_start())
            .when(!line.two_voices, |line| line.items_center())
            .gap_2()
            .px(theme::width(pad))
            .py_2()
            .rounded_xl()
            .opacity(line.standing)
            .when(line.show_voice, |line| {
                line.child(
                    div()
                        .text_size(px(theme::text_xs()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::muted()))
                        .child(if second { "VOICE 2" } else { "VOICE 1" }),
                )
            })
            .child(
                div()
                    .w(px(
                        inside * size / lit * if line.two_voices { 0.84 } else { 1.0 }
                    ))
                    .when(second, |words| words.text_right())
                    .when(line.two_voices && !second, |words| words.text_left())
                    .when(!line.two_voices, |words| words.text_center())
                    .text_size(px(size))
                    .line_height(px(lit * LEADING))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(mixed(
                        theme::muted(),
                        if second {
                            theme::accent()
                        } else {
                            theme::text()
                        },
                        line.lead,
                    )))
                    .child(line.text),
            )
            .when_some(line.breath, |line, breath| line.child(breather(breath)));

        let Some(at) = line.at else {
            return row
                .child(menu::opens_a_menu(
                    drawn.id(("lyric", line.index)),
                    move |this, where_at, cx| this.copies_a_lyric(where_at, &words, cx),
                    cx,
                ))
                .into_any_element();
        };

        let sung = menu::opens_a_menu(
            drawn
                .id(("lyric", line.index))
                .cursor_pointer()
                .hover(|line| line.bg(theme::tinted(theme::text(), 0x0e)))
                .on_click(cx.listener(move |this, event, _, cx| {
                    if !menu::pressed(event) {
                        return;
                    }
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

        row.child(sung).into_any_element()
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

fn breath_of(waiting: Option<Waiting>, index: usize) -> Option<f32> {
    waiting
        .filter(|waiting| waiting.next == index)
        .map(|waiting| waiting.through)
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
    let timed = match lyrics.timing() {
        Timing::Synced => SYNCED,
        Timing::Unsynced => UNSYNCED,
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

fn breather(breath: Breath) -> Div {
    let mut dots = div()
        .flex()
        .items_center()
        .justify_center()
        .h(px(theme::lyric_dot() + DOT_SWELL))
        .gap(px(theme::lyric_dot_gap()));

    for dot in 0..BREATH_DOTS {
        #[expect(clippy::cast_precision_loss, reason = "three dots fit an f32 exactly")]
        let lit = (breath.through * BREATH_DOTS as f32 - dot as f32).clamp(0.0, 1.0);
        let side = theme::lyric_dot() + DOT_SWELL * breath.swell * (0.5 + lit / 2.0);
        dots = dots.child(
            kit::mode_dot(theme::accent())
                .size(px(side))
                .opacity((1.0 - DIM_DOT).mul_add(lit, DIM_DOT)),
        );
    }

    div().flex().justify_center().px_4().py_2().child(dots)
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
    use super::*;

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
