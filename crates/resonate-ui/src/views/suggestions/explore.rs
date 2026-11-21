use std::{collections::BTreeSet, sync::Arc, time::SystemTime};

use gpui::{
    AnyElement, Context, Div, ElementId, FontWeight, ScrollHandle, SharedString, Stateful, Task,
    div, prelude::*, px, rgb,
};
use resonate_core::{Calendar, MediaLocation};
use resonate_engine::{Command, Placement, QueueItem, Unclaimed};
use resonate_library::{
    ExploreBlueprint, ExploreCache, ExploreOffer, ExplorePlaylist, ExplorePlaylistKind,
    ExplorePlaylists, ExploreResult, MixEntry, PICTURED_BY_AT_MOST, SavedQuery, Track,
    artists_for_explore, weave_mix,
};
use resonate_providers::SongCatalog;

use super::SuggestionsPane;
use crate::{
    Drawn, Notice, ResonateApp, format,
    icons::{self, Icon},
    models::Picture,
    theme,
    views::{
        hint::Names,
        kit::{self, Casts as _, EndsInAnEllipsis as _, Press, Tone},
        listing,
        mosaic::{self, Mosaic},
        pointed::{self, LitUnderThePointer as _},
        root::{RootView, framed_cover, row},
        scrollbar::Scrollbars,
        spoken::{self, Spoken as _},
    },
    words,
};

const EXPLORE_CARD: &str = "explore-card";
const QUEUE_SUGGESTIONS_AT_MOST: usize = 4;
const QUEUE_WEAVE_EVERY: usize = 3;

#[derive(Default)]
pub(super) struct ExplorePane {
    cache: Arc<ExploreCache>,
    blueprints: Arc<[ExploreBlueprint]>,
    blueprints_key: Option<(i64, u64)>,
    blueprints_task: Option<Task<()>>,
    playlists_asked: Option<PlaylistsRequest>,
    playlists_page: PlaylistsPage,
    playlists_task: Option<Task<()>>,
    playlists_retry: bool,
    opened_playlist: Option<String>,
    opened_scroll: ScrollHandle,
    mix_mode: bool,
    mix_asked: Option<MixRequest>,
    mix_page: MixPage,
    mix_task: Option<Task<()>>,
    mix_retry: bool,
    dismissed: BTreeSet<MediaLocation>,
}

struct PlaylistsRequest {
    day: i64,
    revision: u64,
    sources: Vec<SongCatalog>,
}

impl PlaylistsRequest {
    fn same(&self, other: &Self) -> bool {
        self.day == other.day
            && self.revision == other.revision
            && same_sources(&self.sources, &other.sources)
    }
}

struct MixRequest {
    day: i64,
    revision: u64,
    mix: SavedQuery,
    sources: Vec<SongCatalog>,
}

impl MixRequest {
    fn same(&self, other: &Self) -> bool {
        self.day == other.day
            && self.revision == other.revision
            && self.mix == other.mix
            && same_sources(&self.sources, &other.sources)
    }
}

fn same_sources(one: &[SongCatalog], other: &[SongCatalog]) -> bool {
    one.len() == other.len()
        && one
            .iter()
            .zip(other)
            .all(|(left, right)| left.same_service(right))
}

#[derive(Clone, Default)]
enum PlaylistsPage {
    #[default]
    Loading,
    Offline,
    Failed,
    Ready(Arc<ExplorePlaylists>),
}

#[derive(Clone, Default)]
pub(super) enum MixPage {
    #[default]
    Idle,
    Loading,
    Offline,
    Failed,
    Ready(Arc<ExploreResult>),
}

impl ExplorePane {
    pub(super) fn opened_playlist(&self) -> Option<&str> {
        self.opened_playlist.as_deref()
    }

    pub(super) fn open_playlist(&mut self, id: Option<String>) {
        self.opened_playlist = id;
    }

    pub(super) const fn mix_mode(&self) -> bool {
        self.mix_mode
    }

    pub(super) fn toggle_mix_mode(&mut self) {
        self.mix_mode = !self.mix_mode;
        if !self.mix_mode {
            self.mix_task = None;
        }
    }

    pub(super) fn mix_changed(&mut self) {
        self.mix_asked = None;
        self.mix_task = None;
        self.mix_page = MixPage::Idle;
    }

    pub(super) fn dismiss(&mut self, location: MediaLocation) {
        self.dismissed.insert(location);
    }

    pub(super) fn is_dismissed(&self, location: &MediaLocation) -> bool {
        self.dismissed.contains(location)
    }

    pub(super) fn cache(&self) -> Arc<ExploreCache> {
        Arc::clone(&self.cache)
    }
}

impl SuggestionsPane {
    fn read_blueprints(&mut self, day: i64, revision: u64, cx: &mut Context<Self>) {
        if self.explore.blueprints_key == Some((day, revision)) {
            return;
        }
        self.explore.blueprints_key = Some((day, revision));

        let library = Arc::clone(&self.catalog);

        self.explore.blueprints_task = Some(cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { library.explore_blueprints() })
                .await;
            if let Ok(blueprints) = read {
                let _ = this.update(cx, |this, cx| {
                    this.explore.blueprints = blueprints.into();
                    cx.notify();
                });
            }
        }));
    }

    fn read_explore_playlists(
        &mut self,
        request: Option<PlaylistsRequest>,
        cx: &mut Context<Self>,
    ) -> PlaylistsPage {
        let Some(request) = request else {
            self.explore.playlists_asked = None;
            self.explore.playlists_task = None;
            self.explore.playlists_page = PlaylistsPage::Offline;
            return PlaylistsPage::Offline;
        };

        if self
            .explore
            .playlists_asked
            .as_ref()
            .is_some_and(|held| held.same(&request))
        {
            return self.explore.playlists_page.clone();
        }

        let refresh = std::mem::take(&mut self.explore.playlists_retry);
        let library = Arc::clone(&self.catalog);
        let cache = Arc::clone(&self.explore.cache);
        let sources = request.sources.clone();

        self.explore.playlists_asked = Some(request);
        if refresh || !matches!(self.explore.playlists_page, PlaylistsPage::Ready(_)) {
            self.explore.playlists_page = PlaylistsPage::Loading;
        }
        self.explore.playlists_task = Some(cx.spawn(async move |this, cx| {
            let page = cx
                .background_executor()
                .spawn(async move { library.explore_playlists(&sources, &cache, refresh) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.explore.playlists_page = match page {
                    Ok(found) => PlaylistsPage::Ready(Arc::new(found)),
                    Err(error) => {
                        tracing::warn!(%error, "the explore playlists could not be read");
                        PlaylistsPage::Failed
                    }
                };
                cx.notify();
            });
        }));
        self.explore.playlists_page.clone()
    }

    fn read_mix_explore(&mut self, request: Option<MixRequest>, cx: &mut Context<Self>) -> MixPage {
        let Some(request) = request else {
            self.explore.mix_asked = None;
            self.explore.mix_task = None;
            self.explore.mix_page = MixPage::Offline;
            return MixPage::Offline;
        };

        if self
            .explore
            .mix_asked
            .as_ref()
            .is_some_and(|held| held.same(&request))
        {
            return self.explore.mix_page.clone();
        }

        let refresh = std::mem::take(&mut self.explore.mix_retry);
        let library = Arc::clone(&self.catalog);
        let cache = Arc::clone(&self.explore.cache);
        let mix = request.mix.clone();
        let sources = request.sources.clone();

        self.explore.mix_asked = Some(request);
        if refresh || !matches!(self.explore.mix_page, MixPage::Ready(_)) {
            self.explore.mix_page = MixPage::Loading;
        }
        self.explore.mix_task = Some(cx.spawn(async move |this, cx| {
            let page = cx
                .background_executor()
                .spawn(async move { library.explore(Some(&mix), &sources, &cache, refresh) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.explore.mix_page = match page {
                    Ok(found) => MixPage::Ready(Arc::new(found)),
                    Err(error) => {
                        tracing::warn!(%error, "mix explore suggestions could not be read");
                        MixPage::Failed
                    }
                };
                cx.notify();
            });
        }));
        self.explore.mix_page.clone()
    }
}

impl RootView {
    pub(crate) fn explore_enabled(&self, cx: &Context<Self>) -> bool {
        !cx.global::<ResonateApp>().online.explore_disabled
    }

    pub(crate) fn clear_explore(&mut self, cx: &mut Context<Self>) {
        self.suggestions
            .update(cx, |pane, _| pane.explore = ExplorePane::default());
    }

    pub(crate) fn explore_can_open(&self, offer: &ExploreOffer, cx: &Context<Self>) -> bool {
        let app = cx.global::<ResonateApp>();
        !app.online.explore_disabled
            && app.online.enabled
            && app
                .sourcing
                .providers(&app.online, crate::settings::Asking::EveryProvider)
                .song_catalogs()
                .iter()
                .any(|catalog| {
                    catalog.namespace() == offer.catalog
                        && catalog.service().scheme() == offer.location.source()
                })
    }

    pub(crate) fn download_explored(&mut self, offer: &ExploreOffer, cx: &mut Context<Self>) {
        if self.explore_can_open(offer, cx) {
            self.library
                .update(cx, |library, cx| library.keep_explored(offer.clone(), cx));
        }
    }

    pub(super) fn mix_explore_button(&self, active: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        self.in_the_pane_ring(
            kit::button(
                "explore-mix-toggle",
                Some(Icon::Suggestions),
                words::suggestions::explore_mix_mode(),
                words::suggestions::explore_mix_mode_hint(),
                if active {
                    Tone::Primary
                } else {
                    Tone::Outlined
                },
            ),
            |this, _, cx| {
                this.suggestions.update(cx, |pane, _| {
                    pane.explore.toggle_mix_mode();
                });
                cx.notify();
            },
            cx,
        )
    }

    pub(super) fn mix_explore_state(
        &mut self,
        query: &SavedQuery,
        cx: &mut Context<Self>,
    ) -> (MixPage, Vec<ExploreOffer>) {
        let app = cx.global::<ResonateApp>();
        let revision = self.library.read(cx).revision();

        let request = app.online.enabled.then(|| MixRequest {
            day: Calendar::local().day_of(SystemTime::now()),
            revision,
            mix: query.clone(),
            sources: app
                .sourcing
                .providers(&app.online, crate::settings::Asking::EveryProvider)
                .song_catalogs()
                .to_vec(),
        });
        let page = self
            .suggestions
            .update(cx, |pane, cx| pane.read_mix_explore(request, cx));
        let offers = match &page {
            MixPage::Ready(found) => {
                let pane = self.suggestions.read(cx);
                found
                    .songs
                    .iter()
                    .filter(|offer| !pane.explore.is_dismissed(&offer.location))
                    .cloned()
                    .collect()
            }
            _ => Vec::new(),
        };
        (page, offers)
    }

    pub(super) fn mix_explore_banner(
        &mut self,
        page: &MixPage,
        count: usize,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let (text, retry, refresh) = match page {
            MixPage::Idle => return None,
            MixPage::Loading => (
                words::suggestions::explore_loading().to_owned(),
                false,
                false,
            ),
            MixPage::Offline => (
                words::suggestions::explore_offline().to_owned(),
                false,
                false,
            ),
            MixPage::Failed => (words::suggestions::explore_failed().to_owned(), true, false),
            MixPage::Ready(found) if found.services == 0 => (
                words::suggestions::explore_no_services().to_owned(),
                false,
                false,
            ),
            MixPage::Ready(found) if count == 0 && found.failed > 0 => {
                (words::suggestions::explore_failed().to_owned(), true, false)
            }
            MixPage::Ready(_) if count == 0 => {
                (words::suggestions::explore_empty().to_owned(), false, true)
            }
            MixPage::Ready(_) => (words::suggestions::explore_mix_active(count), false, true),
        };
        let mut bar = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap_3()
            .px_6()
            .pb_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icons::icon(
                        Icon::Suggestions,
                        theme::text_sm(),
                        theme::accent(),
                    ))
                    .child(kit::figure(text)),
            );

        if retry {
            let label = words::suggestions::explore_retry();

            bar = bar.child(self.in_the_pane_ring(
                kit::button("mix-explore-retry", None, label, label, Tone::Ghost),
                |this, _, cx| {
                    this.suggestions.update(cx, |pane, _| {
                        pane.explore.mix_asked = None;
                        pane.explore.mix_retry = true;
                    });
                    cx.notify();
                },
                cx,
            ));
        } else if refresh {
            bar = bar.child(self.in_the_pane_ring(
                kit::button(
                    "mix-explore-refresh",
                    Some(Icon::Undo),
                    words::suggestions::explore_refresh(),
                    words::suggestions::explore_refresh_hint(),
                    Tone::Ghost,
                ),
                |this, _, cx| {
                    this.suggestions.update(cx, |pane, _| {
                        pane.explore.dismissed.clear();
                        pane.explore.mix_asked = None;
                        pane.explore.mix_retry = true;
                    });
                    cx.notify();
                },
                cx,
            ));
        }
        Some(bar)
    }

    pub(super) fn woven_mix_row(
        &mut self,
        entries: &[MixEntry<Track>],
        index: usize,
        offer: &ExploreOffer,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let queued = self.library.read(cx).is_explored_queued(&offer.location);
        let art = self.server.update(cx, |server, cx| {
            server.cover_of_offer(offer, Drawn::InARow, cx)
        });
        let cover = framed_cover(art, theme::row_cover());
        let title = SharedString::from(offer.song.title.clone());
        let artist = SharedString::from(offer.song.artist.clone().unwrap_or_default());
        let badge = SharedString::from(words::suggestions::explore_suggested_badge(
            offer.service.as_str(),
        ));
        let length = offer.song.length.map(format::spanned).unwrap_or_default();
        let all_entries: Arc<[MixEntry<Track>]> = entries.to_vec().into();
        let saving = offer.clone();
        let dismissing = offer.location.clone();
        let fitted = self.rooms.columns_fit.shown();

        row(false)
            .id(("woven-explore", index))
            .debug_selector(move || format!("woven-explore-{index}"))
            .listed_as(|| spoken::song(&title, Some(&artist)))
            .cursor_pointer()
            .hover(|entry| entry.bg(rgb(theme::hover())))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w(px(theme::row_number()))
                    .child(icons::icon(
                        Icon::Suggestions,
                        theme::text_sm(),
                        theme::accent(),
                    )),
            )
            .child(cover)
            .child(listing::title_cell(
                title.clone(),
                resonate_library::Lit::new(),
                false,
            ))
            .child(listing::artist_cell(
                div().truncate().ends_in_an_ellipsis().child(artist),
            ))
            .when(fitted.format, |entry| {
                entry.child(
                    div()
                        .w(px(theme::row_format()))
                        .flex_none()
                        .truncate()
                        .ends_in_an_ellipsis()
                        .child(kit::figure(badge).text_color(rgb(theme::accent()))),
                )
            })
            .when(fitted.heard, |entry| {
                entry.child(listing::heard(0, None, self.drawn_at()))
            })
            .child(listing::length_cell(SharedString::from(length)))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .child(if queued {
                        kit::mark_when(
                            Press::Greyed,
                            ("woven-explore-queued", index),
                            Icon::Check,
                            words::suggestions::explore_downloaded_song(title.as_str()),
                            "woven-explore-row",
                        )
                    } else {
                        self.in_the_pane_ring(
                            kit::icon_button(
                                ("woven-explore-download", index),
                                Icon::Download,
                                words::suggestions::explore_download_song(title.as_str()),
                            ),
                            move |this, _, cx| {
                                cx.stop_propagation();
                                this.download_explored(&saving, cx);
                            },
                            cx,
                        )
                    })
                    .child(self.in_the_pane_ring(
                        kit::icon_button(
                            ("woven-explore-dismiss", index),
                            Icon::Close,
                            words::suggestions::explore_dismiss_song(title.as_str()),
                        ),
                        move |this, _, cx| {
                            cx.stop_propagation();
                            this.suggestions.update(cx, |pane, _| {
                                pane.explore.dismiss(dismissing.clone());
                            });
                            cx.notify();
                        },
                        cx,
                    )),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.play_mixed(&all_entries, index, false, cx);
            }))
    }

    pub(super) fn explore_playlists_shelf(&mut self, cx: &mut Context<Self>) -> Div {
        let app = cx.global::<ResonateApp>();
        let day = Calendar::local().day_of(SystemTime::now());
        let revision = self.library.read(cx).revision();

        let request = app.online.enabled.then(|| PlaylistsRequest {
            day,
            revision,
            sources: app
                .sourcing
                .providers(&app.online, crate::settings::Asking::EveryProvider)
                .song_catalogs()
                .to_vec(),
        });
        let (page, blueprints) = self.suggestions.update(cx, |pane, cx| {
            pane.read_blueprints(day, revision, cx);

            let page = pane.read_explore_playlists(request, cx);
            (page, Arc::clone(&pane.explore.blueprints))
        });

        let mut heading = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap_3()
            .child(kit::group_title(words::suggestions::explore()));

        if matches!(page, PlaylistsPage::Ready(_)) {
            heading = heading.child(self.in_the_pane_ring(
                kit::button(
                    "explore-refresh",
                    Some(Icon::Undo),
                    words::suggestions::explore_refresh(),
                    words::suggestions::explore_refresh_hint(),
                    Tone::Ghost,
                ),
                |this, _, cx| {
                    this.suggestions.update(cx, |pane, _| {
                        pane.explore.playlists_asked = None;
                        pane.explore.playlists_retry = true;
                    });
                    cx.notify();
                },
                cx,
            ));
        }

        let mut shelf = div().flex().flex_col().gap_4().child(heading);

        match page {
            PlaylistsPage::Offline => {
                shelf = shelf.child(kit::figure(words::suggestions::explore_offline()));
            }
            PlaylistsPage::Loading => {
                if !blueprints.is_empty() {
                    let cards: Vec<AnyElement> = blueprints
                        .iter()
                        .enumerate()
                        .map(|(index, blueprint)| {
                            self.explore_blueprint_card(index, blueprint, cx)
                                .into_any_element()
                        })
                        .collect();

                    shelf = shelf.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .content_start()
                            .gap(px(theme::grid_gap()))
                            .children(cards),
                    );
                } else {
                    shelf = shelf.child(kit::figure(words::suggestions::explore_loading()));
                }
            }
            PlaylistsPage::Failed => {
                shelf = shelf
                    .child(kit::figure(words::suggestions::explore_failed()))
                    .child(self.explore_retry(cx));
            }
            PlaylistsPage::Ready(found) => {
                if found.services == 0 {
                    shelf = shelf.child(kit::figure(words::suggestions::explore_no_services()));
                } else {
                    if !found.playlists.is_empty() {
                        let cards: Vec<AnyElement> = found
                            .playlists
                            .iter()
                            .enumerate()
                            .map(|(index, playlist)| {
                                self.explore_playlist_card(index, playlist, cx)
                                    .into_any_element()
                            })
                            .collect();

                        shelf = shelf.child(
                            div()
                                .flex()
                                .flex_wrap()
                                .content_start()
                                .gap(px(theme::grid_gap()))
                                .children(cards),
                        );
                    }
                    if found.failed > 0 {
                        shelf = shelf
                            .child(kit::figure(words::suggestions::explore_failed()))
                            .child(self.explore_retry(cx));
                    } else if found.playlists.is_empty() {
                        shelf = shelf.child(kit::figure(words::suggestions::explore_empty()));
                    }
                }
            }
        }

        shelf
    }

    pub(super) fn opened_explore_playlist(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (opened_id, scroll, page) = {
            let pane = self.suggestions.read(cx);
            let id = pane.explore.opened_playlist()?.to_owned();
            (
                id,
                pane.explore.opened_scroll.clone(),
                pane.explore.playlists_page.clone(),
            )
        };

        let playlist = match page {
            PlaylistsPage::Ready(found) => found
                .playlists
                .iter()
                .find(|playlist| playlist.id == opened_id)
                .cloned(),
            _ => None,
        };
        let Some(playlist) = playlist else {
            self.suggestions
                .update(cx, |pane, _| pane.explore.open_playlist(None));
            return None;
        };

        let art = self.explore_mosaic(&playlist, kit::hero_side(), Drawn::OnThePage, cx);
        let offers = playlist.songs.clone();
        let playing = offers.clone();
        let shuffling = offers.clone();
        let queueing = offers.clone();
        let downloading = offers.clone();

        let actions = kit::action_row()
            .flex_nowrap()
            .pt_2()
            .child(self.in_the_pane_ring(
                kit::button(
                    "explore-playlist-play",
                    Some(Icon::Play),
                    words::suggestions::play(),
                    words::suggestions::play_hint(),
                    Tone::Primary,
                ),
                move |this, _, cx| this.play_explored_playlist(&playing, 0, false, cx),
                cx,
            ))
            .child(self.in_the_pane_ring(
                kit::button(
                    "explore-playlist-shuffle",
                    Some(Icon::Shuffle),
                    words::suggestions::shuffle(),
                    words::suggestions::shuffle_hint(),
                    Tone::Outlined,
                ),
                move |this, _, cx| this.play_explored_playlist(&shuffling, 0, true, cx),
                cx,
            ))
            .child(self.in_the_pane_ring(
                kit::icon_button(
                    "explore-playlist-queue",
                    Icon::QueueLast,
                    words::suggestions::explore_queue_hint(),
                ),
                move |this, _, cx| {
                    this.queue_explored_playlist(&queueing, Placement::Queued, cx);
                },
                cx,
            ))
            .child(self.in_the_pane_ring(
                kit::button(
                    "explore-playlist-download-all",
                    Some(Icon::Download),
                    words::suggestions::explore_download_all(),
                    words::suggestions::explore_download_all_hint(),
                    Tone::Outlined,
                ),
                move |this, _, cx| {
                    let allowed: Vec<_> = downloading
                        .iter()
                        .filter(|offer| this.explore_can_open(offer, cx))
                        .cloned()
                        .collect();

                    this.library
                        .update(cx, |library, cx| library.keep_explored_all(allowed, cx));
                },
                cx,
            ))
            .child(self.in_the_pane_ring(
                kit::icon_button(
                    "explore-playlist-refresh",
                    Icon::Undo,
                    words::suggestions::explore_refresh_hint(),
                ),
                |this, _, cx| {
                    this.suggestions.update(cx, |pane, _| {
                        pane.explore.playlists_asked = None;
                        pane.explore.playlists_retry = true;
                    });
                    cx.notify();
                },
                cx,
            ));

        let heading = kit::heading()
            .child(self.in_the_pane_ring(
                kit::way_back(
                    "explore-playlist-back",
                    words::suggestions::suggestions(),
                    words::suggestions::back_hint(),
                ),
                |this, _, cx| {
                    this.suggestions
                        .update(cx, |pane, _| pane.explore.open_playlist(None));
                    cx.notify();
                },
                cx,
            ))
            .child(
                kit::hero(self.rooms.hero_row.clone())
                    .child(kit::hero_art(art))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.0))
                            .gap_1()
                            .child(kit::eyebrow(words::suggestions::explore_playlist_eyebrow()))
                            .child(kit::title(SharedString::from(playlist.name.clone())))
                            .child(kit::subtitle(SharedString::from(playlist.subtitle.clone())))
                            .child(kit::figure(explore_measured(&playlist)))
                            .child(actions),
                    ),
            );

        let mut rows = div()
            .id("explore-playlist-rows")
            .flex()
            .flex_col()
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&scroll);

        for (index, offer) in offers.iter().enumerate() {
            rows = rows.child(self.explore_playlist_row(index, &offers, offer, cx));
        }

        Some(
            kit::pane()
                .child(heading)
                .child(
                    div()
                        .relative()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h(px(0.0))
                        .child(Scrollbars::of(cx).around(
                            "explore-playlist-scrollbar",
                            scroll,
                            rows,
                        )),
                )
                .into_any_element(),
        )
    }

    fn explore_playlist_card(
        &mut self,
        index: usize,
        playlist: &ExplorePlaylist,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let side = self.grid_side();
        let art = self.explore_mosaic(playlist, side, Drawn::InAGrid, cx);
        let cell_id = ElementId::from(("explore-playlist-card", index));
        let pointed = pointed::is_pointed_at(&cell_id);
        let opening = playlist.id.clone();
        let playing = playlist.songs.clone();
        let queueing = playlist.songs.clone();
        let spoken_label = spoken::joined([
            playlist.name.as_str(),
            playlist.subtitle.as_str(),
            explore_measured(playlist).as_str(),
        ]);

        div()
            .id(cell_id.clone())
            .debug_selector(move || format!("explore-playlist-open-{index}"))
            .follows_the_pointer(cell_id)
            .group(EXPLORE_CARD)
            .listed_as(move || spoken_label.clone())
            .flex()
            .flex_none()
            .flex_col()
            .gap_2p5()
            .w(px(side))
            .cursor_pointer()
            .names(words::suggestions::open_hint())
            .child(
                div()
                    .relative()
                    .rounded(px(mosaic::ART_ROUNDING))
                    .group_hover(EXPLORE_CARD, |frame| frame.casting(kit::Cast::Short))
                    .child(art)
                    .child(
                        div()
                            .absolute()
                            .top_1()
                            .right_1()
                            .flex()
                            .rounded_md()
                            .bg(kit::cover_veil())
                            .opacity(0.0)
                            .group_hover(EXPLORE_CARD, |marks| marks.opacity(1.0))
                            .child(self.in_the_pane_ring(
                                kit::icon_button(
                                    ("explore-card-queue", index),
                                    Icon::QueueLast,
                                    words::suggestions::explore_queue_hint(),
                                ),
                                move |this, _, cx| {
                                    cx.stop_propagation();
                                    this.queue_explored_playlist(&queueing, Placement::Queued, cx);
                                },
                                cx,
                            )),
                    )
                    .child(
                        kit::play_on_the_art(
                            ("explore-card-play", index),
                            words::suggestions::play_hint(),
                        )
                        .absolute()
                        .bottom_2()
                        .right_2()
                        .opacity(0.0)
                        .group_hover(EXPLORE_CARD, |mark| mark.opacity(1.0))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.play_explored_playlist(&playing, 0, false, cx);
                        })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .min_w(px(0.0))
                    .child(
                        div()
                            .text_size(px(theme::text_sm()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(if pointed {
                                theme::accent()
                            } else {
                                theme::text()
                            }))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(SharedString::from(playlist.name.clone())),
                    )
                    .child(
                        div()
                            .text_size(px(theme::text_xs()))
                            .text_color(rgb(theme::muted()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(SharedString::from(playlist.subtitle.clone())),
                    )
                    .child(kit::figure(explore_measured(playlist)).text_color(rgb(theme::faint()))),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.suggestions.update(cx, |pane, _| {
                    pane.explore.open_playlist(Some(opening.clone()))
                });
                cx.notify();
            }))
    }

    fn explore_blueprint_card(
        &self,
        index: usize,
        blueprint: &ExploreBlueprint,
        _cx: &Context<Self>,
    ) -> Stateful<Div> {
        let side = self.grid_side();
        let art = Mosaic {
            drawn: &[],
            mark: explore_icon(blueprint.kind),
            name: &blueprint.name,
        }
        .drawn(side);
        div()
            .id(("explore-blueprint", index))
            .flex()
            .flex_none()
            .flex_col()
            .gap_2p5()
            .w(px(side))
            .opacity(kit::GREYED)
            .child(art)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .min_w(px(0.0))
                    .child(
                        div()
                            .text_size(px(theme::text_sm()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(theme::text()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(SharedString::from(blueprint.name.clone())),
                    )
                    .child(
                        div()
                            .text_size(px(theme::text_xs()))
                            .text_color(rgb(theme::muted()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(SharedString::from(blueprint.subtitle.clone())),
                    )
                    .child(
                        kit::figure(words::suggestions::explore_loading())
                            .text_color(rgb(theme::faint())),
                    ),
            )
    }

    fn explore_playlist_row(
        &mut self,
        index: usize,
        all_offers: &[ExploreOffer],
        offer: &ExploreOffer,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let queued = self.library.read(cx).is_explored_queued(&offer.location);
        let art = self.server.update(cx, |server, cx| {
            server.cover_of_offer(offer, Drawn::InARow, cx)
        });
        let cover = framed_cover(art, theme::row_cover());
        let title = SharedString::from(offer.song.title.clone());
        let artist = SharedString::from(offer.song.artist.clone().unwrap_or_default());
        let source = SharedString::from(offer.service.clone());
        let length = offer.song.length.map(format::spanned).unwrap_or_default();
        let offers = all_offers.to_vec();
        let saving = offer.clone();
        let fitted = self.rooms.columns_fit.shown();

        row(false)
            .id(("explore-song", index))
            .debug_selector(move || format!("explore-song-{index}"))
            .listed_as(|| spoken::song(&title, Some(&artist)))
            .cursor_pointer()
            .hover(|entry| entry.bg(rgb(theme::hover())))
            .child(listing::number_cell(SharedString::from(
                (index + 1).to_string(),
            )))
            .child(cover)
            .child(listing::title_cell(
                title.clone(),
                resonate_library::Lit::new(),
                false,
            ))
            .child(listing::artist_cell(
                div().truncate().ends_in_an_ellipsis().child(artist),
            ))
            .when(fitted.format, |entry| {
                entry.child(
                    div()
                        .w(px(theme::row_format()))
                        .flex_none()
                        .truncate()
                        .ends_in_an_ellipsis()
                        .child(kit::figure(source)),
                )
            })
            .child(listing::length_cell(SharedString::from(length)))
            .child(if queued {
                kit::mark_when(
                    Press::Greyed,
                    ("explore-queued", index),
                    Icon::Check,
                    words::suggestions::explore_downloaded_song(title.as_str()),
                    "explore-row",
                )
            } else {
                self.in_the_pane_ring(
                    kit::icon_button(
                        ("explore-download", index),
                        Icon::Download,
                        words::suggestions::explore_download_song(title.as_str()),
                    ),
                    move |this, _, cx| {
                        cx.stop_propagation();
                        this.download_explored(&saving, cx);
                    },
                    cx,
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.play_explored_playlist(&offers, index, false, cx);
            }))
    }

    fn explore_mosaic(
        &mut self,
        playlist: &ExplorePlaylist,
        side: f32,
        drawn_at: Drawn,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut seen = BTreeSet::new();

        let drawn: Vec<Picture> = playlist
            .songs
            .iter()
            .filter(|offer| {
                offer
                    .song
                    .cover
                    .as_ref()
                    .is_some_and(|cover| seen.insert(cover.as_str().to_owned()))
            })
            .take(PICTURED_BY_AT_MOST)
            .filter_map(|offer| {
                self.server
                    .update(cx, |server, cx| server.cover_of_offer(offer, drawn_at, cx))
            })
            .collect();
        Mosaic {
            drawn: &drawn,
            mark: explore_icon(playlist.kind),
            name: &playlist.name,
        }
        .drawn(side)
    }

    fn explore_retry(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let label = words::suggestions::explore_retry();
        self.in_the_pane_ring(
            kit::button("retry-explore", None, label, label, Tone::Outlined),
            |this, _, cx| {
                this.suggestions.update(cx, |pane, _| {
                    pane.explore.playlists_asked = None;
                    pane.explore.playlists_retry = true;
                });
                cx.notify();
            },
            cx,
        )
        .into_any_element()
    }

    pub(crate) fn mix_suggestions_into_queue(&mut self, cx: &mut Context<Self>) {
        let app = cx.global::<ResonateApp>();

        if app.online.explore_disabled || !app.online.enabled {
            return;
        }

        let sources = app
            .sourcing
            .providers(&app.online, crate::settings::Asking::EveryProvider)
            .song_catalogs()
            .to_vec();

        if sources.is_empty() {
            return;
        }

        let queued = self.player.read(cx).queued();
        let position = self.player.read(cx).state().queue_position.unwrap_or(0);
        let revision = queued.revision;
        let already_queued: BTreeSet<MediaLocation> = queued
            .rows
            .iter()
            .map(|item| item.location.clone())
            .collect();
        let mut artist_names = Vec::new();

        for item in queued.rows.iter() {
            if let Some(track) = self.library.update(cx, |library, _| library.track_of(item))
                && let Some(artist) = track.artist
            {
                artist_names.push(artist);
            }
        }

        let direct = artists_for_explore(artist_names.iter().map(String::as_str));

        if direct.is_empty() {
            self.report(
                Notice::Noted(words::queue::no_queue_suggestions().to_owned()),
                cx,
            );
            return;
        }

        let catalog = self.library.read(cx).catalog();
        let cache = self.suggestions.read(cx).explore.cache();
        let zone = self.zone_acted_on();
        let upcoming_len = queued.rows.len().saturating_sub(position + 1);

        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { catalog.explore_for_artists(&direct, &sources, &cache, false) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Ok(found) = found else {
                    return;
                };
                let fresh: Vec<ExploreOffer> = found
                    .songs
                    .into_iter()
                    .filter(|offer| !already_queued.contains(&offer.location))
                    .take(QUEUE_SUGGESTIONS_AT_MOST)
                    .collect();

                if fresh.is_empty() {
                    this.report(
                        Notice::Noted(words::queue::no_queue_suggestions().to_owned()),
                        cx,
                    );
                    return;
                }
                this.server
                    .update(cx, |server, _| server.remember_explored(&fresh));

                let current_queue = this.player.read(cx).queued();

                if current_queue.revision != revision {
                    return;
                }

                let mut minting = Unclaimed::beside(current_queue.rows.iter().map(Arc::as_ref));
                let mut inserted = 0usize;
                let step = if upcoming_len >= QUEUE_WEAVE_EVERY {
                    QUEUE_WEAVE_EVERY
                } else {
                    1
                };

                for (nth, offer) in fresh.into_iter().enumerate() {
                    let target = (position + 1 + (nth + 1) * step + inserted)
                        .min(current_queue.rows.len() + inserted);
                    let item = QueueItem::whole(minting.mint(), offer.location);

                    this.act_on_the_zone(zone, |this| {
                        this.send(
                            Command::Insert {
                                items: vec![item],
                                at: Placement::At(target),
                                play: false,
                            },
                            cx,
                        );
                    });
                    inserted += 1;
                }
                if inserted > 0 {
                    this.report(
                        Notice::Done(words::queue::mixed_in_suggestions(inserted)),
                        cx,
                    );
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

pub(super) fn weave_suggestion_tracks(
    tracks: &[Track],
    offers: Vec<ExploreOffer>,
) -> Vec<MixEntry<Track>> {
    weave_mix(tracks.to_vec(), |track| track.artist.as_deref(), offers)
}

fn explore_measured(playlist: &ExplorePlaylist) -> String {
    let mut parts = vec![words::suggestions::tracks(playlist.songs.len() as u64)];

    if let Some(length) = playlist.length {
        parts.push(format::spanned(length));
    }
    parts.join(" · ")
}

const fn explore_icon(kind: ExplorePlaylistKind) -> Icon {
    match kind {
        ExplorePlaylistKind::DiscoverMix => Icon::Suggestions,
        ExplorePlaylistKind::BeyondArtist => Icon::Artists,
        ExplorePlaylistKind::GenreDiscovery => Icon::Tracks,
        ExplorePlaylistKind::FavouritesExpansion => Icon::Favourited,
        ExplorePlaylistKind::RediscoverBeyond => Icon::Albums,
    }
}
