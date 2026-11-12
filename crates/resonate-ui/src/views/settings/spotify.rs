use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui::{
    AppContext as _, BorrowAppContext as _, Context, Div, Entity, ParentElement as _, Stateful,
    Task, Window,
};
use resonate_library::{Error, LookupOp, ProfileLink, SpotifySignInFailure, Upkeep};

use crate::{
    Notice, ResonateApp, Setting,
    icons::Icon,
    settings::{Online, SpotifySignIn},
    toast,
    views::{
        field::{Field, Submitted},
        kit,
        root::RootView,
        settings::{Rows, action},
    },
    words,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SpotifySigning {
    Idle,
    Asking,
    Waiting(String),
}

pub(crate) struct SpotifyState {
    pub(crate) shown: SpotifySigning,
    pub(crate) reading: bool,
    stopped: Arc<AtomicBool>,
    task: Task<()>,
}

impl Default for SpotifyState {
    fn default() -> Self {
        Self {
            shown: SpotifySigning::Idle,
            reading: false,
            stopped: Arc::new(AtomicBool::new(false)),
            task: Task::ready(()),
        }
    }
}

fn turned_away(failure: SpotifySignInFailure) -> &'static str {
    match failure {
        SpotifySignInFailure::NoPlaceToComeBack => {
            words::settings_spotify::the_sign_in_could_not_listen()
        }
        SpotifySignInFailure::Lapsed => words::settings_spotify::the_spotify_sign_in_was_not(),
        SpotifySignInFailure::Denied => words::settings_spotify::spotify_was_told_not_to_sign(),
        SpotifySignInFailure::Refused => words::settings_spotify::spotify_refused_the_sign_in_is(),
        SpotifySignInFailure::Unreached => {
            words::settings_spotify::spotify_could_not_be_reached_to()
        }
    }
}

pub(crate) fn field(
    online: &Online,
    window: &mut Window,
    cx: &mut Context<RootView>,
) -> Entity<Field> {
    let field = cx.new(|cx| {
        let mut field = Field::new(
            words::settings_spotify::the_client_id_of_your_app(),
            window,
            cx,
        );
        field.hold(online.spotify_client_id.clone(), cx);
        field
    });
    cx.subscribe_in(&field, window, |this, _, _: &Submitted, window, cx| {
        this.spotify_client_given(window, cx);
    })
    .detach();
    field
}

impl RootView {
    fn spotify_client_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let given = self.typed.spotify_client.read(cx).text().trim().to_owned();
        self.typed
            .spotify_client
            .clone()
            .update(cx, |field, cx| field.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.spotify_client_id = given.clone();
        });
        let said = if given.is_empty() {
            words::settings_spotify::no_spotify_app_is_named_no()
        } else {
            words::settings_spotify::kept_for_signing_in_to_spotify()
        };
        self.store(&Setting::SpotifyClientId(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn give_what_is_typed_of_the_spotify_client(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let field = self.typed.spotify_client.read(cx);
        let typed_apart = field.is_focused(window)
            && field.text().trim() != cx.global::<ResonateApp>().online.spotify_client_id;
        if typed_apart {
            self.spotify_client_given(window, cx);
        }
        typed_apart
    }

    pub(super) fn put_back_the_spotify_client(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stored = cx.global::<ResonateApp>().online.spotify_client_id.clone();
        let field = self.typed.spotify_client.clone();
        if field.read(cx).is_focused(window) {
            field.update(cx, |field, cx| field.hold(stored, cx));
        }
    }

    pub(crate) fn follow_the_spotify_sign_in_made_elsewhere(
        &mut self,
        signed_in: SpotifySignIn,
        cx: &mut Context<Self>,
    ) {
        let SpotifySignIn {
            client_id,
            refresh_token,
        } = signed_in;
        self.typed
            .spotify_client
            .clone()
            .update(cx, |field, cx| field.hold(client_id.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.spotify_client_id = client_id;
            global.online.spotify_refresh_token = refresh_token;
        });
        cx.notify();
    }

    pub(crate) fn forget_the_spotify_account(&mut self, cx: &mut Context<Self>) {
        self.sign_out_of_spotify(cx);
        self.typed
            .spotify_client
            .clone()
            .update(cx, |field, cx| field.hold(String::new(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.spotify_client_id = String::new();
        });
        self.store(&Setting::SpotifyClientId(String::new()), cx);
    }

    pub(crate) fn sign_in_to_spotify(&mut self, cx: &mut Context<Self>) {
        let app = cx.global::<ResonateApp>();
        let Some(signs_in) = app.spotify.clone().filter(|_| app.online.enabled) else {
            self.report(
                Notice::Trouble(words::settings_spotify::offline().to_owned()),
                cx,
            );
            return;
        };
        let client_id = app.online.spotify_client_id.trim().to_owned();
        if client_id.is_empty() {
            self.report(
                Notice::Trouble(words::settings_spotify::no_client().to_owned()),
                cx,
            );
            return;
        }

        self.accounts.spotify.stopped.store(true, Ordering::Relaxed);
        let stopped = Arc::new(AtomicBool::new(false));
        self.accounts.spotify.stopped = Arc::clone(&stopped);
        self.accounts.spotify.shown = SpotifySigning::Asking;
        cx.notify();

        self.accounts.spotify.task = cx.spawn(async move |this, cx| {
            let authorizing = cx
                .background_executor()
                .spawn(async move { signs_in.authorizing(&client_id) })
                .await;
            let approval = match authorizing {
                Ok(approval) => approval,
                Err(failure) => {
                    let told = this.update(cx, |this, cx| this.spotify_answered(Err(failure), cx));
                    let _ = told;
                    return;
                }
            };
            let waiting = approval.address().to_owned();
            let shown = this.update(cx, |this, cx| {
                this.accounts.spotify.shown = SpotifySigning::Waiting(waiting.clone());
                cx.open_url(&waiting);
                cx.notify();
            });
            if shown.is_err() {
                return;
            }
            let answered = cx
                .background_executor()
                .spawn(async move { approval.approved(&move || stopped.load(Ordering::Relaxed)) })
                .await;
            let told = this.update(cx, |this, cx| this.spotify_answered(answered, cx));
            let _ = told;
        });
    }

    fn spotify_answered(
        &mut self,
        answered: Result<Option<String>, SpotifySignInFailure>,
        cx: &mut Context<Self>,
    ) {
        self.accounts.spotify.shown = SpotifySigning::Idle;
        match answered {
            Ok(Some(token)) => {
                cx.update_global::<ResonateApp, _>(|global, _| {
                    global.online.spotify_refresh_token = token.clone();
                });
                self.store(&Setting::SpotifyRefreshToken(token), cx);
                self.report(
                    Notice::Done(words::settings_spotify::signed_in().to_owned()),
                    cx,
                );
            }
            Ok(None) => {}
            Err(failure) => {
                self.report(Notice::Trouble(turned_away(failure).to_owned()), cx);
            }
        }
        cx.notify();
    }

    fn stop_signing_in_to_spotify(&mut self, cx: &mut Context<Self>) {
        self.accounts.spotify.stopped.store(true, Ordering::Relaxed);
        self.accounts.spotify.shown = SpotifySigning::Idle;
        self.accounts.spotify.task = Task::ready(());
        cx.notify();
    }

    fn sign_out_of_spotify(&mut self, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.spotify_refresh_token = String::new();
        });
        self.store(&Setting::SpotifyRefreshToken(String::new()), cx);
        let library = self.library.read(cx).catalog();
        cx.spawn(async move |this, cx| {
            let let_go = cx
                .background_executor()
                .spawn(async move { library.let_go_of_the_profile(&ProfileLink::SpotifyAccount) })
                .await;
            let _ = this.update(cx, |this, cx| match let_go {
                Ok(_) => {
                    this.library
                        .update(cx, |library, cx| library.reload_playlists(cx));
                    toast::tell(
                        Notice::Done(
                            words::settings_spotify::signed_out_of_spotify_the_playlists()
                                .to_owned(),
                        ),
                        cx,
                    );
                }
                Err(error) => toast::tell(
                    toast::could_not(words::settings_spotify::let_the_account_go(), &error),
                    cx,
                ),
            });
        })
        .detach();
    }

    fn read_the_spotify_account(&mut self, upkeep: Upkeep, cx: &mut Context<Self>) {
        if self.accounts.spotify.reading {
            return;
        }
        let Some((library, reference)) = self.library.read(cx).follows_links() else {
            self.report(
                Notice::Trouble(words::settings_spotify::offline().to_owned()),
                cx,
            );
            return;
        };
        self.accounts.spotify.reading = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let followed = cx
                .background_executor()
                .spawn(async move {
                    library.follow_profile(reference.as_ref(), &ProfileLink::SpotifyAccount, upkeep)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.accounts.spotify.reading = false;
                match followed {
                    Err(Error::Refused {
                        op: LookupOp::SpotifyAccount,
                        ..
                    }) => this.report(
                        Notice::Trouble(words::settings_spotify::no_account().to_owned()),
                        cx,
                    ),
                    followed => this.followed_the_account(followed, upkeep, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn spotify_button(
        &self,
        id: &'static str,
        label: &'static str,
        icon: Icon,
        held_back: bool,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(id, label, icon, held_back, press, self, cx)
    }

    pub(super) fn spotify_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let app = cx.global::<ResonateApp>();
        let reachable = app.spotify.is_some() && app.online.enabled;
        let client_given = !app.online.spotify_client_id.trim().is_empty();
        let signed_in = !app.online.spotify_refresh_token.trim().is_empty();
        let reading = self.accounts.spotify.reading;

        let body = kit::rows().child(kit::field(
            words::settings_spotify::client_id(),
            self.text_field(
                "spotify-client-id",
                &self.typed.spotify_client,
                |this, window, cx| this.leave_the_account(window, cx),
                cx,
            ),
        ));
        if signed_in && client_given {
            return body
                .child(
                    kit::action_row()
                        .child(self.spotify_button(
                            "spotify-follow",
                            if reading {
                                words::settings_spotify::reading_the_account()
                            } else {
                                words::settings_spotify::follow_the_account()
                            },
                            Icon::Link,
                            !reachable || reading,
                            |this, _, cx| this.read_the_spotify_account(Upkeep::InStep, cx),
                            cx,
                        ))
                        .child(self.spotify_button(
                            "spotify-move",
                            words::settings_spotify::move_to_resonate_once(),
                            Icon::Download,
                            !reachable || reading,
                            |this, _, cx| this.read_the_spotify_account(Upkeep::Once, cx),
                            cx,
                        ))
                        .child(self.spotify_button(
                            "spotify-sign-out",
                            words::settings_spotify::sign_out_of_spotify(),
                            Icon::Close,
                            false,
                            |this, _, cx| this.sign_out_of_spotify(cx),
                            cx,
                        )),
                )
                .note(words::settings_spotify::signed_in_note())
                .note(words::settings_spotify::account_note());
        }

        let signing: Div = match self.accounts.spotify.shown.clone() {
            SpotifySigning::Idle => kit::action_row().child(self.spotify_button(
                "spotify-sign-in",
                words::settings_spotify::sign_in_to_spotify(),
                Icon::Link,
                !reachable || !client_given,
                |this, _, cx| this.sign_in_to_spotify(cx),
                cx,
            )),
            SpotifySigning::Asking => kit::action_row().child(self.spotify_button(
                "spotify-sign-in",
                words::settings_spotify::opening_the_sign_in(),
                Icon::Link,
                true,
                |_, _, _| {},
                cx,
            )),
            SpotifySigning::Waiting(address) => kit::action_row()
                .child(self.spotify_button(
                    "spotify-open-page",
                    words::settings_spotify::open_the_page_again(),
                    Icon::Globe,
                    false,
                    move |_, _, cx| cx.open_url(&address),
                    cx,
                ))
                .child(self.spotify_button(
                    "spotify-stop-sign-in",
                    words::settings_spotify::stop(),
                    Icon::Stop,
                    false,
                    |this, _, cx| this.stop_signing_in_to_spotify(cx),
                    cx,
                )),
        };
        body.child(signing)
            .note(words::settings_spotify::account_note())
    }
}
