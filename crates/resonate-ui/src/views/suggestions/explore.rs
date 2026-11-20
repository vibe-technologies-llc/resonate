use std::{sync::Arc, time::SystemTime};

use gpui::{AnyElement, Context, Div, SharedString, Task, div, prelude::*, px};
use resonate_core::Calendar;
use resonate_library::{ExploreCache, ExploreOffer, ExploreResult, SavedQuery, Suggestion};
use resonate_providers::SongCatalog;

use super::SuggestionsPane;
use crate::{
    ResonateApp,
    icons::Icon,
    theme,
    views::{
        kit::{self, EndsInAnEllipsis as _, Tone},
        root::RootView,
    },
    words,
};

#[derive(Default)]
pub(super) struct ExplorePane {
    exploring_mix: bool,
    cache: Arc<ExploreCache>,
    asked: Option<Request>,
    page: Page,
    task: Option<Task<()>>,
    retry: bool,
}

struct Request {
    day: i64,
    revision: u64,
    mix: Option<SavedQuery>,
    sources: Vec<SongCatalog>,
}

impl Request {
    fn same(&self, other: &Self) -> bool {
        self.day == other.day
            && self.revision == other.revision
            && self.mix == other.mix
            && self.sources.len() == other.sources.len()
            && self
                .sources
                .iter()
                .zip(&other.sources)
                .all(|(one, other)| one.same_service(other))
    }
}

#[derive(Clone, Default)]
enum Page {
    #[default]
    Loading,
    Offline,
    Failed,
    Ready(Arc<ExploreResult>),
}

impl ExplorePane {
    pub(super) const fn exploring_mix(&self) -> bool {
        self.exploring_mix
    }

    pub(super) fn library(&mut self) {
        self.exploring_mix = false;
        self.asked = None;
        self.task = None;
    }
}

impl SuggestionsPane {
    fn read_explore(&mut self, request: Option<Request>, cx: &mut Context<Self>) -> Page {
        let Some(request) = request else {
            self.explore.asked = None;
            self.explore.task = None;
            self.explore.page = Page::Offline;
            return Page::Offline;
        };
        if self
            .explore
            .asked
            .as_ref()
            .is_some_and(|held| held.same(&request))
        {
            return self.explore.page.clone();
        }
        let refresh = std::mem::take(&mut self.explore.retry);
        let library = Arc::clone(&self.catalog);
        let cache = Arc::clone(&self.explore.cache);
        let mix = request.mix.clone();
        let sources = request.sources.clone();
        self.explore.asked = Some(request);
        self.explore.page = Page::Loading;
        self.explore.task = Some(cx.spawn(async move |this, cx| {
            let page = cx
                .background_executor()
                .spawn(async move { library.explore(mix.as_ref(), &sources, &cache, refresh) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.explore.page = match page {
                    Ok(found) => Page::Ready(Arc::new(found)),
                    Err(error) => {
                        tracing::warn!(%error, "the explore shelf could not be read");
                        Page::Failed
                    }
                };
                cx.notify();
            });
        }));
        Page::Loading
    }
}

impl RootView {
    pub(super) fn explore_enabled(&self, cx: &Context<Self>) -> bool {
        !cx.global::<ResonateApp>().online.explore_disabled
    }

    pub(crate) fn clear_explore(&mut self, cx: &mut Context<Self>) {
        self.suggestions
            .update(cx, |pane, _| pane.explore = ExplorePane::default());
    }

    pub(super) fn explore_tabs(&mut self, explore: bool, cx: &mut Context<Self>) -> Div {
        let tabs = kit::segmented()
            .child(self.in_the_pane_ring(
                kit::segment("mix-local", words::suggestions::library_songs(), !explore),
                |this, _, cx| {
                    this.suggestions
                        .update(cx, |pane, _| pane.explore.library());
                    cx.notify();
                },
                cx,
            ))
            .child(self.in_the_pane_ring(
                kit::segment("mix-explore", words::suggestions::explore(), explore),
                |this, _, cx| {
                    this.suggestions
                        .update(cx, |pane, _| pane.explore.exploring_mix = true);
                    cx.notify();
                },
                cx,
            ));
        div().flex().flex_none().px_6().pb_3().child(tabs)
    }

    pub(super) fn explored_suggestion(
        &mut self,
        suggestion: &Suggestion,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        kit::pane()
            .child(
                kit::heading()
                    .child(self.in_the_pane_ring(
                        kit::way_back(
                            "explore-suggestions-back",
                            words::suggestions::suggestions(),
                            words::suggestions::back_hint(),
                        ),
                        |this, _, cx| {
                            this.suggestions.update(cx, |pane, cx| pane.open(None, cx));
                            cx.notify();
                        },
                        cx,
                    ))
                    .child(kit::title(SharedString::from(suggestion.name.clone()))),
            )
            .child(self.explore_tabs(true, cx))
            .child(
                div()
                    .id("explore-mix-songs")
                    .flex_1()
                    .overflow_y_scroll()
                    .px_6()
                    .pb_4()
                    .child(self.explore_shelf(Some(suggestion.query.clone()), cx)),
            )
            .into_any_element()
    }

    pub(super) fn explore_shelf(&mut self, mix: Option<SavedQuery>, cx: &mut Context<Self>) -> Div {
        let app = cx.global::<ResonateApp>();
        let revision = self.library.read(cx).revision();
        let request = app.online.enabled.then(|| Request {
            day: Calendar::local().day_of(SystemTime::now()),
            revision,
            mix,
            sources: app
                .sourcing
                .providers(&app.online, crate::settings::Asking::EveryProvider)
                .song_catalogs()
                .to_vec(),
        });
        let page = self
            .suggestions
            .update(cx, |pane, cx| pane.read_explore(request, cx));
        let mut shelf = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(kit::group_title(words::suggestions::explore()));
        let state = match page {
            Page::Loading => Some(words::suggestions::explore_loading()),
            Page::Offline => Some(words::suggestions::explore_offline()),
            Page::Failed => {
                shelf = shelf.child(self.explore_retry(cx));
                Some(words::suggestions::explore_failed())
            }
            Page::Ready(found) => {
                if found.services == 0 {
                    Some(words::suggestions::explore_no_services())
                } else {
                    for (index, offer) in found.songs.iter().enumerate() {
                        shelf = shelf.child(self.explore_row(index, offer, cx));
                    }
                    if found.failed > 0 {
                        shelf = shelf.child(self.explore_retry(cx));
                        Some(words::suggestions::explore_failed())
                    } else {
                        found
                            .songs
                            .is_empty()
                            .then(words::suggestions::explore_empty)
                    }
                }
            }
        };
        shelf.children(state.map(kit::figure))
    }

    fn explore_retry(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let label = words::suggestions::explore_retry();
        self.in_the_pane_ring(
            kit::button("retry-explore", None, label, label, Tone::Outlined),
            |this, _, cx| {
                this.suggestions.update(cx, |pane, _| {
                    pane.explore.asked = None;
                    pane.explore.retry = true;
                });
                cx.notify();
            },
            cx,
        )
        .into_any_element()
    }

    fn explore_row(&mut self, index: usize, offer: &ExploreOffer, cx: &mut Context<Self>) -> Div {
        let playing = offer.clone();
        let saving = offer.clone();
        let title = SharedString::from(offer.song.title.clone());
        let artist = SharedString::from(offer.song.artist.clone().unwrap_or_default());
        let source = SharedString::from(offer.service.clone());
        div()
            .flex()
            .items_center()
            .gap_3()
            .p_2()
            .rounded_md()
            .bg(gpui::rgb(theme::raised()))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .min_w(px(0.0))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(title.clone()),
                    )
                    .child(kit::figure(artist).truncate().ends_in_an_ellipsis()),
            )
            .child(kit::figure(source))
            .child(self.in_the_pane_ring(
                kit::icon_button(
                    ("explore-play", index),
                    Icon::Play,
                    words::suggestions::explore_play_song(title.as_str()),
                ),
                move |this, _, cx| this.play_explored(&playing, cx),
                cx,
            ))
            .child(self.in_the_pane_ring(
                kit::icon_button(
                    ("explore-download", index),
                    Icon::Download,
                    words::suggestions::explore_download_song(title.as_str()),
                ),
                move |this, _, cx| {
                    if this.explore_can_open(&saving, cx) {
                        this.library
                            .update(cx, |library, cx| library.keep_explored(saving.clone(), cx));
                    }
                },
                cx,
            ))
    }

    fn explore_can_open(&self, offer: &ExploreOffer, cx: &Context<Self>) -> bool {
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

    fn play_explored(&mut self, offer: &ExploreOffer, cx: &mut Context<Self>) {
        if self.explore_can_open(offer, cx) {
            self.server
                .update(cx, |server, _| server.remember_explore(offer));
            self.play_served(vec![offer.location.clone()], 0, cx);
        }
    }
}
