use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui::{App, Context, Pixels, Point, Window};
use resonate_core::{AlbumId, ArtistId, ReleaseTrackId, TrackId, WantId};
use resonate_engine::Placement;
use resonate_library::{
    Cut, Favoured, Found, Mbid, Recording, RecordingRelease, Track, in_the_order_worth_offering,
};

use super::Chooses;
use crate::{
    ResonateApp, Selection,
    held::Rows,
    icons::Icon,
    models::Notice,
    toast,
    views::{
        menu::Menu,
        playlists::Held,
        reorder::{Listed, Shift},
        root::{Deleting, RootView, listed},
    },
    words,
};

const RELEASES_OFFERED: usize = 10;

pub(super) fn album_menu(at: Point<Pixels>, album: AlbumId) -> Menu {
    Menu::at(at)
        .does(Icon::Play, words::menu::play(), move |this, window, cx| {
            this.plays_everything_in(Selection::Album(album), Placement::Queued, true, window, cx);
        })
        .does(
            Icon::QueueNext,
            words::menu::play_next(),
            move |this, window, cx| {
                this.plays_everything_in(
                    Selection::Album(album),
                    Placement::Next,
                    false,
                    window,
                    cx,
                );
            },
        )
        .does(
            Icon::QueueLast,
            words::menu::add_to_queue(),
            move |this, window, cx| {
                this.plays_everything_in(
                    Selection::Album(album),
                    Placement::Queued,
                    false,
                    window,
                    cx,
                );
            },
        )
}

pub(super) fn album_menu_ending(menu: Menu, album: AlbumId, owner: Option<ArtistId>) -> Menu {
    menu.apart()
        .does(
            Icon::Albums,
            words::menu::open_album(),
            move |this, _, cx| {
                this.opened(Selection::Album(album), cx);
            },
        )
        .when_some(owner, |menu, owner| {
            menu.does(
                Icon::Artists,
                words::menu::show_the_artist(),
                move |this, _, cx| {
                    this.opened(Selection::Artist(owner), cx);
                },
            )
        })
}

pub(super) fn artist_menu_among(
    at: Point<Pixels>,
    artist: ArtistId,
    favourite: bool,
    chooses: Chooses,
    this: &RootView,
    cx: &App,
) -> Menu {
    let chosen = this.acting_on_chosen(chooses.shift, chooses.row, cx);

    if chosen.count() > 1 {
        let artists = this.artists_chosen(chooses.shift, &chosen, cx);

        return chosen_artists_menu(at, chooses.shift, artists, this, cx);
    }

    artist_menu_favoured(at, artist, favourite)
}

fn artist_menu(at: Point<Pixels>, artist: ArtistId) -> Menu {
    Menu::at(at)
        .does(Icon::Play, words::menu::play(), move |this, window, cx| {
            this.plays_everything_in(
                Selection::Artist(artist),
                Placement::Queued,
                true,
                window,
                cx,
            );
        })
        .does(
            Icon::QueueNext,
            words::menu::play_next(),
            move |this, window, cx| {
                this.plays_everything_in(
                    Selection::Artist(artist),
                    Placement::Next,
                    false,
                    window,
                    cx,
                );
            },
        )
        .does(
            Icon::QueueLast,
            words::menu::add_to_queue(),
            move |this, window, cx| {
                this.plays_everything_in(
                    Selection::Artist(artist),
                    Placement::Queued,
                    false,
                    window,
                    cx,
                );
            },
        )
}

fn artist_menu_ending(menu: Menu, artist: ArtistId) -> Menu {
    menu.apart().does(
        Icon::Artists,
        words::menu::open_artist(),
        move |this, _, cx| {
            this.opened(Selection::Artist(artist), cx);
        },
    )
}

pub(super) fn artist_menu_favoured(at: Point<Pixels>, artist: ArtistId, favourite: bool) -> Menu {
    artist_menu_ending(
        artist_menu(at, artist)
            .apart()
            .favours(Favoured::Artist(artist), favourite),
        artist,
    )
}

#[derive(Clone)]
pub(super) struct MissingOnAnAlbum {
    pub(super) release_track: ReleaseTrackId,
    pub(super) want: Option<WantId>,
    pub(super) recording: Option<Mbid>,
}

impl MissingOnAnAlbum {
    pub(super) fn menu(&self, at: Point<Pixels>) -> Menu {
        let release_track = self.release_track;
        let download = move |this: &mut RootView, _: &mut Window, cx: &mut Context<RootView>| {
            this.library
                .update(cx, |library, cx| library.want(release_track, cx));
        };

        match (self.want, &self.recording) {
            (Some(want), _) => Menu::at(at).does(
                Icon::Wanted,
                words::menu::stop_wanting(),
                move |this, _, cx| {
                    this.library
                        .update(cx, |library, cx| library.unwant(want, cx));
                },
            ),
            (None, Some(_)) => Menu::at(at)
                .does(
                    Icon::Play,
                    words::menu::download_and_play(),
                    move |this, _, cx| {
                        let zone = this.zone_acted_on();

                        this.library.update(cx, |library, cx| {
                            library.want_and_play(release_track, zone, cx);
                        });
                    },
                )
                .apart()
                .does(Icon::Download, words::menu::download(), download),
            (None, None) => Menu::at(at).does(Icon::Download, words::menu::download(), download),
        }
    }
}

pub(super) fn found_menu(at: Point<Pixels>, found: &Found) -> Menu {
    let played = found.clone();
    let downloaded = found.clone();

    Menu::at(at)
        .does(
            Icon::Play,
            words::menu::download_and_play(),
            move |this, _, cx| {
                let wanted = played.clone();
                let zone = this.zone_acted_on();

                this.library
                    .update(cx, |library, cx| library.fetch_and_play(wanted, zone, cx));
            },
        )
        .apart()
        .does(
            Icon::Download,
            words::menu::download(),
            move |this, _, cx| {
                let wanted = downloaded.clone();

                this.library
                    .update(cx, |library, cx| library.want_found(wanted, cx));
            },
        )
        .nests(Icon::Disc, words::menu::download_from_a_release(), |menu| {
            releases_offered(menu, found)
        })
}

pub(super) fn releases_to_want(at: Point<Pixels>, found: &Found) -> Menu {
    releases_offered(Menu::at(at), found)
}

fn releases_offered(mut menu: Menu, found: &Found) -> Menu {
    for release in found
        .in_the_order_worth_offering()
        .into_iter()
        .take(RELEASES_OFFERED)
    {
        let wanted = found.from(release);

        menu = menu.does(Icon::Disc, release_line(release), move |this, _, cx| {
            let wanted = wanted.clone();

            this.library
                .update(cx, |library, cx| library.want_found(wanted, cx));
        });
    }
    menu
}

fn releases_to_place_on(at: Point<Pixels>, track: TrackId, recordings: Vec<Recording>) -> Menu {
    let mut menu = Menu::at(at);

    for (recording, release) in releases_beside_their_recordings(recordings) {
        let placed = release.id.clone();
        let line = release_line(&release);

        menu = menu.does(Icon::Disc, line, move |this, _, cx| {
            let placing = Arc::clone(&recording);
            let placed = placed.clone();

            this.library.update(cx, |library, cx| {
                library.place_on(track, placing, placed, cx)
            });
        });
    }
    menu
}

fn releases_beside_their_recordings(
    recordings: Vec<Recording>,
) -> Vec<(Arc<Recording>, RecordingRelease)> {
    let mut offered: Vec<(Arc<Recording>, RecordingRelease)> = Vec::new();

    for recording in recordings.into_iter().map(Arc::new) {
        for release in in_the_order_worth_offering(&recording.releases) {
            if offered.len() == RELEASES_OFFERED {
                return offered;
            }
            if offered.iter().all(|(_, held)| held.id != release.id) {
                offered.push((Arc::clone(&recording), release.clone()));
            }
        }
    }
    offered
}

impl RootView {
    pub(super) fn offer_releases_to_place_on(
        &mut self,
        track: TrackId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let pressed = self.press_a_lookup();
        let Some(asking) = self.library.update(cx, |library, cx| {
            library.releases_it_could_sit_on(track, cx)
        }) else {
            return;
        };

        cx.spawn(async move |this, cx| {
            let asked = asking.await;
            let _ = this.update(cx, |this, cx| {
                if this.window_has_moved_on_from(pressed, cx) {
                    return;
                }
                match asked {
                    Ok(recordings)
                        if recordings
                            .iter()
                            .any(|recording| !recording.releases.is_empty()) =>
                    {
                        this.open_a_menu(releases_to_place_on(at, track, recordings), cx);
                    }
                    Ok(_) => this.report(
                        Notice::Trouble(words::browser::nothing_places_it().to_owned()),
                        cx,
                    ),
                    Err(error) => {
                        tracing::warn!(%error, "the releases a track is on could not be read");
                        this.report(
                            toast::could_not(words::browser::find_the_releases_it_is_on(), &error),
                            cx,
                        );
                    }
                }
            });
        })
        .detach();
    }
}

fn release_line(release: &RecordingRelease) -> String {
    let mut line = release.title.clone();

    if let Some(year) = release.date.as_deref().and_then(|date| date.get(..4)) {
        line.push_str(" · ");
        line.push_str(year);
    }
    if let Some(kind) = release.issued.kind.as_deref() {
        line.push_str(" · ");
        line.push_str(kind);
    }
    line
}

pub(super) fn lyric_sheet_of(track: &Track) -> Option<TrackId> {
    (track.span.is_none() && track.location.is_local() && track.location.as_path().is_some())
        .then_some(track.id)
}

pub(super) fn chosen_tracks_menu(
    at: Point<Pixels>,
    tracks: Vec<Track>,
    this: &RootView,
    cx: &App,
) -> Menu {
    let queued = listed(&tracks);
    let cuts: Arc<[Cut]> = tracks.iter().map(Cut::of).collect();
    let ids: Vec<TrackId> = tracks.iter().map(|track| track.id).collect();
    let hidden = tracks.iter().all(|track| track.hidden);
    let hiding = ids.clone();
    let deleting = Deleting::each(&tracks);
    let everywhere = syncs_with_other_devices(cx).then(|| deleting.clone().on_every_device());
    let paths: Vec<PathBuf> = tracks
        .iter()
        .filter_map(|track| track.location.locator().as_path())
        .map(Path::to_path_buf)
        .collect();
    let sharing = ids.clone();
    let sheeted: Vec<TrackId> = tracks.iter().filter_map(lyric_sheet_of).collect();
    let favoured = tracks.iter().all(|track| {
        this.library
            .read(cx)
            .favours(Favoured::Track(track.id), track.favourite.is_some())
    });
    let played = Arc::<[Track]>::from(tracks);

    Menu::at(at)
        .queues(
            move || Arc::clone(&queued),
            move |this, _, cx| this.play(&played, 0, cx),
        )
        .apart()
        .holds(move || Held::of(Arc::clone(&cuts)))
        .does(
            if favoured {
                Icon::Favourited
            } else {
                Icon::Favourite
            },
            if favoured {
                words::menu::unfavour()
            } else {
                words::menu::favour()
            },
            move |this, _, cx| {
                this.library.update(cx, |library, cx| {
                    for id in &ids {
                        library.favour(Favoured::Track(*id), !favoured, cx);
                    }
                });
            },
        )
        .apart()
        .shares_each(sharing)
        .offers_the_files(paths)
        .saves_the_lyrics(sheeted)
        .apart()
        .nests(Icon::Delete, words::menu::remove(), move |menu| {
            menu.does(
                if hidden { Icon::Undo } else { Icon::Discard },
                if hidden {
                    words::menu::show_each()
                } else {
                    words::menu::hide_each()
                },
                move |this, _, cx| {
                    this.library.update(cx, |library, cx| {
                        library.hide_tracks(hiding.clone(), !hidden, cx);
                    });
                },
            )
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
}

pub(super) fn syncs_with_other_devices(cx: &App) -> bool {
    let app = cx.global::<ResonateApp>();
    app.syncing.is_some() && app.synchronised.enabled
}

fn artists_are_favoured(this: &RootView, shift: Shift, artists: &[ArtistId], cx: &App) -> bool {
    let library = this.library.read(cx);
    let among = match shift {
        Shift::Listing(Listed::FavouriteArtists) => Rows::from(library.favourite_artists()),
        _ => library.artists(),
    };

    artists.iter().all(|artist| {
        among.iter().any(|held| {
            held.id == *artist
                && library.favours(Favoured::Artist(*artist), held.favourite.is_some())
        })
    })
}

fn albums_are_favoured(this: &RootView, shift: Shift, albums: &[AlbumId], cx: &App) -> bool {
    let library = this.library.read(cx);
    let shelf = library.favourite_albums();

    albums.iter().all(|album| {
        let stored = match shift {
            Shift::Listing(Listed::FavouriteAlbums) => shelf
                .iter()
                .find(|held| held.id == *album)
                .map(|held| held.favourite.is_some()),
            _ => library
                .album_of(*album)
                .map(|held| held.favourite.is_some()),
        };

        stored.is_some_and(|stored| library.favours(Favoured::Album(*album), stored))
    })
}

fn chosen_artists_menu(
    at: Point<Pixels>,
    shift: Shift,
    artists: Vec<ArtistId>,
    this: &RootView,
    cx: &App,
) -> Menu {
    let favoured = artists_are_favoured(this, shift, &artists, cx);
    let scopes: Vec<Selection> = artists.iter().copied().map(Selection::Artist).collect();
    let (play, next, last) = (scopes.clone(), scopes.clone(), scopes);

    Menu::at(at)
        .does(Icon::Play, words::menu::play(), move |this, window, cx| {
            this.plays_everything_in_each(play.clone(), Placement::Queued, true, window, cx);
        })
        .does(
            Icon::QueueNext,
            words::menu::play_next(),
            move |this, window, cx| {
                this.plays_everything_in_each(next.clone(), Placement::Next, false, window, cx);
            },
        )
        .does(
            Icon::QueueLast,
            words::menu::add_to_queue(),
            move |this, window, cx| {
                this.plays_everything_in_each(last.clone(), Placement::Queued, false, window, cx);
            },
        )
        .apart()
        .does(
            if favoured {
                Icon::Favourited
            } else {
                Icon::Favourite
            },
            if favoured {
                words::menu::unfavour()
            } else {
                words::menu::favour()
            },
            move |this, _, cx| {
                this.library.update(cx, |library, cx| {
                    for artist in &artists {
                        library.favour(Favoured::Artist(*artist), !favoured, cx);
                    }
                });
            },
        )
}

pub(super) fn chosen_albums_menu(
    at: Point<Pixels>,
    shift: Shift,
    albums: Vec<AlbumId>,
    this: &RootView,
    cx: &App,
) -> Menu {
    let favoured = albums_are_favoured(this, shift, &albums, cx);
    let scopes: Vec<Selection> = albums.iter().copied().map(Selection::Album).collect();
    let (play, next, last) = (scopes.clone(), scopes.clone(), scopes);

    Menu::at(at)
        .does(Icon::Play, words::menu::play(), move |this, window, cx| {
            this.plays_everything_in_each(play.clone(), Placement::Queued, true, window, cx);
        })
        .does(
            Icon::QueueNext,
            words::menu::play_next(),
            move |this, window, cx| {
                this.plays_everything_in_each(next.clone(), Placement::Next, false, window, cx);
            },
        )
        .does(
            Icon::QueueLast,
            words::menu::add_to_queue(),
            move |this, window, cx| {
                this.plays_everything_in_each(last.clone(), Placement::Queued, false, window, cx);
            },
        )
        .apart()
        .does(
            if favoured {
                Icon::Favourited
            } else {
                Icon::Favourite
            },
            if favoured {
                words::menu::unfavour()
            } else {
                words::menu::favour()
            },
            move |this, _, cx| {
                this.library.update(cx, |library, cx| {
                    for album in &albums {
                        library.favour(Favoured::Album(*album), !favoured, cx);
                    }
                });
            },
        )
}
