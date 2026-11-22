use std::{slice, sync::Arc};

use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, SharedString, Stateful, div, prelude::*, px,
    rgb, uniform_list,
};
use resonate_core::Span;
use resonate_engine::Placement;
use resonate_library::{Column, Cut, Favoured, HeldMedium, Lit, PlaylistEntry, Track};

use super::{
    Chooses, Plays, Rowed, TRACK_ADD_CONTROLS, TRACK_CONTROLS,
    heading::ArtistShows,
    menus::{chosen_tracks_menu, lyric_sheet_of, syncs_with_other_devices},
    missing_rows::Unheld,
    rows::trailing_controls,
};
use crate::{
    ListedRow, ResonateApp, Selection, format,
    held::Rows,
    icons::Icon,
    theme,
    views::{
        kit,
        kit::EndsInAnEllipsis,
        listing,
        listing::Pictured,
        menu,
        menu::{Called, Menu},
        playlists::{Held, ROW_GROUP},
        reorder,
        reorder::{Chosen, Lifted, Listed, Shift},
        root::{Deleting, Pane, RootView, empty, listed, row},
        scrollbar::Scrollbars,
        sorting, spoken,
        spoken::Spoken,
    },
    words,
};

const COVERS_AHEAD: usize = 48;

impl RootView {
    pub(crate) fn tracks(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tracks = self.library.read(cx).listing();
        let narrowed = self.library.read(cx).narrowing().is_some();
        let scoped = self.library.read(cx).selection() != Selection::Everything;

        if tracks.is_empty() && !narrowed && !scoped {
            return if self.library.read(cx).unreadable() {
                kit::trouble(
                    Icon::Tracks,
                    words::browser::library_unreadable(),
                    Some(words::browser::library_unreadable_more()),
                )
            } else if cx.global::<ResonateApp>().music_folder.is_none() {
                self.add_songs(cx)
            } else {
                empty(
                    Icon::Tracks,
                    words::browser::no_tracks_yet(),
                    Some(words::browser::drop_songs_more()),
                )
            };
        }

        let playing = self.playing_now(cx).track;
        let rows = self.library.read(cx).rows();
        let nothing = tracks.is_empty() && rows.is_empty();
        let unreadable = self.library.read(cx).unreadable();

        let found_nothing = nothing.then(|| {
            self.nothing_beyond(cx).unwrap_or_else(|| {
                let said = if unreadable {
                    words::browser::library_unreadable()
                } else {
                    words::browser::no_tracks_match()
                };
                self.nothing_matched(Icon::Tracks, said, None, cx)
            })
        });
        let heading = self.heading(cx);
        let in_an_album = matches!(self.library.read(cx).selection(), Selection::Album(_));
        let credited: Option<String> = match self.library.read(cx).selection() {
            Selection::Album(album) => self
                .library
                .read(cx)
                .album_of(album)
                .and_then(|album| album.artist.clone()),
            Selection::Everything | Selection::Artist(_) => None,
        };
        let rowed = !rows.is_empty();
        let release_tracks = self.library.read(cx).release_tracks();
        let found = self.library.read(cx).found();
        let media: Arc<[HeldMedium]> = self
            .library
            .read(cx)
            .release()
            .map(|release| release.media.clone())
            .unwrap_or_default()
            .into();
        let held = tracks.len();
        let listed = self.library.read(cx).listed_rows();

        self.land_where_it_was_left(listed, cx);

        let records = match self.library.read(cx).selection() {
            Selection::Artist(artist) => {
                let library = self.library.read(cx);
                let held = self.around_the_rows(cx).records.len() + library.albums_not_held().len();
                (self.shows.artist.within(held) == ArtistShows::Records)
                    .then(|| self.artist_records(artist, cx))
            }
            Selection::Everything | Selection::Album(_) => None,
        };
        let listing_shown = records.is_none();
        let nothing = nothing && listing_shown;
        let found_nothing = found_nothing.filter(|_| listing_shown);

        kit::pane()
            .child(heading.flex_none())
            .when_some(records, |pane, records| {
                pane.child(Scrollbars::of(cx).around(
                    "artist-records-scrollbar",
                    self.scrolls(cx).artist_records.clone(),
                    records,
                ))
            })
            .when(!nothing && listing_shown, |pane| {
                pane.child(listing::columns(
                    "#",
                    !in_an_album,
                    if self.adding_songs_to.is_some() {
                        TRACK_ADD_CONTROLS
                    } else {
                        TRACK_CONTROLS
                    },
                    sorting::tracks_sorted(self, cx),
                    &self.rooms.columns_fit,
                    &self.controls,
                    cx,
                ))
            })
            .when_some(found_nothing, |pane, nothing| pane.child(nothing))
            .when(!nothing && listing_shown, |pane| {
                pane.child(
                    Scrollbars::of(cx).around(
                        "track-scrollbar",
                        self.scrolls(cx).track_rows.clone(),
                        uniform_list(
                            "tracks",
                            listed,
                            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                this.reach_further(range.end, held, cx);

                                let mut drawn = Vec::new();

                                for index in range.clone() {
                                    let listed_row = if rowed {
                                        rows.get(index).copied()
                                    } else {
                                        Some(ListedRow::Held(index))
                                    };
                                    match listed_row {
                                        Some(ListedRow::Disc(disc)) => {
                                            drawn.push(
                                                disc_heading(disc, &media).into_any_element(),
                                            );
                                        }
                                        Some(ListedRow::Held(held)) => {
                                            let Some(track) = tracks.get(held) else {
                                                drawn.push(row(false).into_any_element());
                                                continue;
                                            };
                                            let shift = Shift::Listing(Listed::Tracks);

                                            let row = this.track_row(
                                                &tracks,
                                                held,
                                                track,
                                                Rowed {
                                                    playing: playing == Some(track.id),
                                                    plays: Plays::AsTheListingIsDrawn {
                                                        in_an_album,
                                                    },
                                                    reach: this.at_the_reach(shift, index, cx),
                                                    chooses: Some(Chooses::among(
                                                        Listed::Tracks,
                                                        index,
                                                    )),
                                                },
                                                cx,
                                            );

                                            drawn.push(
                                                this.marked_where_reached(shift, index, row, cx)
                                                    .into_any_element(),
                                            );
                                        }
                                        Some(ListedRow::Missing(missing)) => {
                                            let Some(row) = release_tracks.get(missing) else {
                                                continue;
                                            };
                                            let unheld =
                                                Unheld::from(row).credited_as(credited.as_deref());

                                            drawn.push(
                                                this.unheld_row(missing, unheld, cx)
                                                    .into_any_element(),
                                            );
                                        }
                                        Some(ListedRow::NotHeld(songs)) => {
                                            drawn.push(
                                                Self::songs_not_held_heading(songs)
                                                    .into_any_element(),
                                            );
                                        }
                                        Some(ListedRow::Found(at)) => {
                                            let Some(row) = found.get(at) else {
                                                continue;
                                            };

                                            drawn.push(
                                                this.found_row(at, row, cx).into_any_element(),
                                            );
                                        }
                                        None => {}
                                    }
                                }
                                if !in_an_album {
                                    let before =
                                        range.start.saturating_sub(COVERS_AHEAD)..range.start;
                                    let after = range.end..range.end + COVERS_AHEAD;

                                    let albums = after.chain(before.rev()).filter_map(|index| {
                                        let held = if rowed {
                                            match rows.get(index).copied()? {
                                                ListedRow::Held(held) => held,
                                                _ => return None,
                                            }
                                        } else {
                                            index
                                        };
                                        tracks.get(held)?.album_id
                                    });

                                    this.covers_ahead(albums, cx);
                                }
                                drawn
                            }),
                        )
                        .track_scroll(&self.scrolls(cx).track_rows)
                        .h_full()
                        .w_full(),
                    ),
                )
            })
            .into_any_element()
    }

    pub(crate) fn track_row(
        &self,
        tracks: &Rows<Track>,
        index: usize,
        track: &Track,
        rowed: Rowed,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let Rowed {
            playing,
            plays,
            reach,
            chooses,
        } = rowed;
        let in_an_album = plays == Plays::AsTheListingIsDrawn { in_an_album: true };
        let played = tracks.clone();
        let menued = tracks.clone();
        let pressed = tracks.clone();
        let adding_to = self.adding_songs_to;
        let id = track.id;
        let menued_row = menu::lit_by_its_menu(("track", id.get()), cx);
        let favourite = self
            .library
            .read(cx)
            .favours(Favoured::Track(id), track.favourite.is_some());
        let (lit_title, lit_artist) = {
            let search = self.library.read(cx).search();
            (
                search.lit(&track.title, Column::Title),
                track
                    .artist
                    .as_deref()
                    .map_or_else(Lit::new, |name| search.lit(name, Column::Artist)),
            )
        };
        let fitted = self.rooms.columns_fit.shown();
        let number = match (in_an_album, track.track_number) {
            (true, Some(number)) => SharedString::from(number.to_string()),
            (true, None) => SharedString::new_static(""),
            (false, _) => SharedString::from((index + 1).to_string()),
        };

        let row = row(playing)
            .id(("track", id.get()))
            .debug_selector(move || format!("track-{}", id.get()))
            .listed_as(|| spoken::song(&track.title, track.artist.as_deref()))
            .group(ROW_GROUP)
            .cursor_pointer()
            .hover(|entry| entry.bg(rgb(theme::hover())))
            .child(if playing {
                listing::playing_mark()
            } else {
                listing::number_cell(number)
            })
            .when(!in_an_album, |row| {
                row.child(self.cover(
                    Pictured::Track {
                        album: track.album_id,
                        file: &track.location,
                    },
                    cx,
                ))
            })
            .child(listing::title_cell(
                SharedString::from(track.title.clone()),
                lit_title,
                playing,
            ))
            .child(listing::artist_cell(
                self.opens(
                    ("track-artist", index),
                    listing::matched(
                        SharedString::from(track.artist.clone().unwrap_or_default()),
                        lit_artist,
                    ),
                    words::browser::open_artist_hint(),
                    track.artist_id.map(Selection::Artist),
                    cx,
                )
                .flex_shrink(1.0)
                .ends_in_an_ellipsis(),
            ))
            .when(fitted.format, |row| {
                row.child(listing::format_cell(Some((track.codec, track.spec))))
            })
            .when(fitted.heard, |row| {
                row.child(listing::heard(track.plays, track.played, self.drawn_at()))
            })
            .child(listing::length_cell(SharedString::from(
                track
                    .duration
                    .map(|duration| format::clock(duration, track.spec.rate))
                    .unwrap_or_default(),
            )))
            .child(
                trailing_controls()
                    .child(
                        self.favour_mark_under(
                            ("track-favourite", index),
                            Favoured::Track(id),
                            favourite,
                            ROW_GROUP,
                            cx,
                        )
                        .when(menued_row, |mark| mark.opacity(1.0)),
                    )
                    .child(
                        self.row_controls(("track-controls", index), cx)
                            .when_some(adding_to, |controls, target| {
                                let cut = Cut::of(track);
                                controls.child(self.in_the_pane_ring(
                                    kit::icon_button(
                                        ("add-song-to-playlist", index),
                                        Icon::Plus,
                                        words::playlists::add_song_hint(),
                                    ),
                                    move |this, _, cx| {
                                        cx.stop_propagation();
                                        this.library.update(cx, |library, cx| {
                                            library.add_to_playlist(target, vec![cut.clone()], cx);
                                        });
                                    },
                                    cx,
                                ))
                            })
                            .when(adding_to.is_none(), |controls| {
                                controls
                                    .child(self.queue_track_control(
                                        ("track-next", index),
                                        Placement::Next,
                                        tracks,
                                        index,
                                        chooses,
                                        cx,
                                    ))
                                    .child(self.queue_track_control(
                                        ("track-last", index),
                                        Placement::Queued,
                                        tracks,
                                        index,
                                        chooses,
                                        cx,
                                    ))
                            }),
                    ),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if let Some(Chooses { shift, row }) = chooses
                    && this.reach_by_click(shift, row, event.modifiers(), cx)
                {
                    return;
                }
                this.play_the_row(plays, &played, index, window, cx);
            }));
        let lifted = match chooses.filter(|chooses| chooses.row == index && !in_an_album) {
            Some(Chooses { shift, row }) => Lifted::tracks(
                tracks,
                self.choosable(shift, self.acting_on_chosen(shift, row, cx), cx),
            ),
            None => Lifted::tracks(tracks, Chosen::of(Span::one(index))),
        };

        let row = reorder::liftable(row, lifted);

        menu::opens_a_reachable_menu(
            row,
            reach,
            move |this, at, cx| {
                if let Some(Chooses { shift, row }) = chooses {
                    let chosen = this.choosable(shift, this.acting_on_chosen(shift, row, cx), cx);
                    if chosen.count() > 1 {
                        let tracks = this.tracks_chosen(shift, &chosen, cx);

                        return chosen_tracks_menu(at, tracks, this, cx);
                    }
                }

                let Some(track) = menued.get(index) else {
                    return Menu::at(at);
                };
                let track_id = track.id;
                let hidden = track.hidden;
                let (icon, hiding) = if hidden {
                    (Icon::Undo, words::menu::show_each())
                } else {
                    (Icon::Discard, words::menu::hide_each())
                };
                let favourite = this
                    .library
                    .read(cx)
                    .favours(Favoured::Track(track.id), track.favourite.is_some());
                let queued = listed(slice::from_ref(track));
                let held = Cut::of(track);
                let delivered = track.delivered.then(|| track.clone());
                let names = Called {
                    title: SharedString::from(track.title.clone()),
                    artist: SharedString::from(track.artist.clone().unwrap_or_default()),
                    album: this.album_named(track.album_id, &track.location, track.span, cx),
                };
                let others = match (track.alternatives > 0)
                    .then(|| this.library.read(cx).copies_held(track.id))
                {
                    None => None,
                    Some(Some(copies)) => Some((track.clone(), copies.to_vec())),
                    Some(None) => return Menu::at(at).awaiting_the_copies_of(track.id),
                };

                let variants = match this.library.read(cx).has_variants(track.id) {
                    false => None,
                    true => match this.library.read(cx).variants_held(track.id) {
                        Some(variants) => variants,
                        None => return Menu::at(at).awaiting_the_variants_of(track.id),
                    },
                };
                let placeable = this.library.read(cx).can_enrich().then_some(track_id);
                let matched = SharedString::from(track.title.clone());
                let looked_up = track.title.clone();
                let refreshed = track.title.clone();
                let deleting = Deleting::of(track);
                let everywhere =
                    syncs_with_other_devices(cx).then(|| deleting.clone().on_every_device());
                let in_the_queue = (this.pane != Pane::Tracks
                    && this.is_in_the_queue(&track.location, track.span, cx))
                .then(|| (track.location.clone(), track.span));

                let pressed = pressed.clone();

                Menu::at(at)
                    .queues(
                        move || Arc::clone(&queued),
                        move |this, window, cx| {
                            this.play_the_row(plays, &pressed, index, window, cx);
                        },
                    )
                    .apart()
                    .holds(move || Held::of(Arc::from([held.clone()])))
                    .favours(Favoured::Track(track.id), favourite)
                    .reaches(Some(track.id), track.album_id, track.artist_id)
                    .when_some(others, Menu::offers_the_other_copies)
                    .when_some(variants, Menu::offers_the_variants)
                    .looks_for_versions(track.clone(), at)
                    .apart()
                    .shares(track.id)
                    .offers_the_file(&track.location, names)
                    .saves_the_lyrics(lyric_sheet_of(track).into_iter().collect())
                    .when_some(placeable, |menu, track| {
                        menu.apart()
                            .nests(Icon::Globe, words::menu::identify(), move |menu| {
                                menu.does(
                                    Icon::Globe,
                                    words::browser::look_it_up(),
                                    move |this, _, cx| {
                                        let title = looked_up.clone();

                                        this.library.update(cx, |library, cx| {
                                            library.look_up_the_song(track, title, cx);
                                        });
                                    },
                                )
                                .does(
                                    Icon::Undo,
                                    words::browser::refresh_what_was_fetched(),
                                    move |this, _, cx| {
                                        let title = refreshed.clone();

                                        this.library.update(cx, |library, cx| {
                                            library.refresh_the_song(track, title, cx);
                                        });
                                    },
                                )
                                .does(
                                    Icon::Rename,
                                    words::matching::match_the_song(),
                                    move |this, window, cx| {
                                        this.ask_to_match(track, matched.clone(), window, cx);
                                    },
                                )
                                .does(
                                    Icon::Disc,
                                    words::browser::place_on(),
                                    move |this, _, cx| {
                                        this.offer_releases_to_place_on(track, at, cx);
                                    },
                                )
                            })
                    })
                    .apart()
                    .when_some(in_the_queue, |menu, (location, span)| {
                        menu.does(
                            Icon::Discard,
                            words::menu::take_out(),
                            move |this, _, cx| {
                                this.take_out_of_the_queue(&location, span, cx);
                            },
                        )
                    })
                    .nests(Icon::Delete, words::menu::remove(), move |menu| {
                        menu.does(icon, hiding, move |this, _, cx| {
                            this.library.update(cx, |library, cx| {
                                library.hide_track(track_id, !hidden, cx);
                            });
                        })
                        .when_some(delivered, |menu, delivered| {
                            menu.does(
                                Icon::Close,
                                words::menu::forget_delivery(),
                                move |this, _, cx| {
                                    this.library.update(cx, |library, cx| {
                                        library.forget_delivered(&delivered, cx);
                                    });
                                },
                            )
                        })
                        .does(
                            Icon::Delete,
                            words::menu::delete_from_disk(),
                            move |this, _, cx| {
                                this.ask_to_delete(deleting.clone(), cx);
                            },
                        )
                        .when_some(everywhere, |menu, everywhere| {
                            menu.does(
                                Icon::Delete,
                                words::menu::delete_from_every_device(),
                                move |this, _, cx| {
                                    this.ask_to_delete(everywhere.clone(), cx);
                                },
                            )
                        })
                    })
            },
            cx,
        )
    }
}

fn disc_heading(disc: u32, media: &[HeldMedium]) -> Div {
    let named = media
        .iter()
        .find(|medium| medium.position == disc)
        .and_then(|medium| medium.title.clone().or_else(|| medium.format.clone()));
    let spelt = match named {
        Some(named) => words::browser::disc_named(disc, named),
        None => words::browser::disc(disc),
    };

    run_heading(SharedString::from(spelt))
}

pub(crate) fn run_heading(named: impl IntoElement) -> Div {
    row(false).child(
        div()
            .flex()
            .flex_1()
            .min_w(px(0.0))
            .border_b_1()
            .border_color(rgb(theme::border()))
            .text_size(px(theme::text_xs()))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(theme::faint()))
            .child(named),
    )
}

pub(super) fn queueing(
    tracks: &Rows<Track>,
    index: usize,
) -> impl Fn() -> Arc<[PlaylistEntry]> + 'static {
    let tracks = tracks.clone();
    move || {
        tracks
            .get(index)
            .map_or_else(|| Arc::from([]), |track| listed(slice::from_ref(track)))
    }
}
