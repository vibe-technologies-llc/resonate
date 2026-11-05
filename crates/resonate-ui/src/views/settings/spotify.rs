use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui::{
    AppContext as _, BorrowAppContext as _, Context, Div, Entity, ParentElement as _, Stateful,
    Styled as _, Task, Window, div,
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
        settings::{action, note},
    },
};

const ACCOUNT_NOTE: &str = "Register an app at developer.spotify.com and give its client ID here, \
                            with http://127.0.0.1:43879/spotify among the app's redirect URIs. \
                            The sign-in happens in your browser and only a refresh token is kept, \
                            in the settings file. Spotify lets an app in development mode read \
                            only the playlists the account owns or collaborates on, so a playlist \
                            it merely follows is read from its public page: the first hundred \
                            songs at most, and not at all where it is private.";
const SIGNED_IN_NOTE: &str = "Follow keeps the liked songs and every playlist in step, each a \
                              playlist here, read again every six hours; Move copies them once \
                              and follows nothing. A song the library lacks is wanted, found by \
                              its ISRC or a strict search, and one neither names is left out.";
const OFFLINE: &str = "Spotify is signed in to only while Online is on";
const NO_CLIENT: &str = "Give the client ID of your Spotify app first";
const SIGNED_IN: &str = "Signed in to Spotify";
const NO_ACCOUNT: &str = "Sign in to Spotify again: it no longer knows this account";

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

const fn turned_away(failure: SpotifySignInFailure) -> &'static str {
    match failure {
        SpotifySignInFailure::NoPlaceToComeBack => {
            "The sign-in could not listen for its answer: is another sign-in open?"
        }
        SpotifySignInFailure::Lapsed => "The Spotify sign-in was not approved in time",
        SpotifySignInFailure::Denied => "Spotify was told not to sign in",
        SpotifySignInFailure::Refused => {
            "Spotify refused the sign-in: is the redirect listed on the app?"
        }
        SpotifySignInFailure::Unreached => "Spotify could not be reached to sign in",
    }
}

pub(crate) fn field(
    online: &Online,
    window: &mut Window,
    cx: &mut Context<RootView>,
) -> Entity<Field> {
    let field = cx.new(|cx| {
        let mut field = Field::new("The client ID of your app, then press enter", window, cx);
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
        let given = self.spotify_client.read(cx).text().trim().to_owned();
        self.spotify_client
            .clone()
            .update(cx, |field, cx| field.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.spotify_client_id = given.clone();
        });
        let said = if given.is_empty() {
            "No Spotify app is named: no account is offered or asked"
        } else {
            "Kept for signing in to Spotify"
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
        let field = self.spotify_client.read(cx);
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
        let field = self.spotify_client.clone();
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
        self.spotify_client
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
        self.spotify_client
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
            self.report(Notice::Trouble(OFFLINE.to_owned()), cx);
            return;
        };
        let client_id = app.online.spotify_client_id.trim().to_owned();
        if client_id.is_empty() {
            self.report(Notice::Trouble(NO_CLIENT.to_owned()), cx);
            return;
        }

        self.spotify_state.stopped.store(true, Ordering::Relaxed);
        let stopped = Arc::new(AtomicBool::new(false));
        self.spotify_state.stopped = Arc::clone(&stopped);
        self.spotify_state.shown = SpotifySigning::Asking;
        cx.notify();

        self.spotify_state.task = cx.spawn(async move |this, cx| {
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
                this.spotify_state.shown = SpotifySigning::Waiting(waiting.clone());
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
        self.spotify_state.shown = SpotifySigning::Idle;
        match answered {
            Ok(Some(token)) => {
                cx.update_global::<ResonateApp, _>(|global, _| {
                    global.online.spotify_refresh_token = token.clone();
                });
                self.store(&Setting::SpotifyRefreshToken(token), cx);
                self.report(Notice::Done(SIGNED_IN.to_owned()), cx);
            }
            Ok(None) => {}
            Err(failure) => {
                self.report(Notice::Trouble(turned_away(failure).to_owned()), cx);
            }
        }
        cx.notify();
    }

    fn stop_signing_in_to_spotify(&mut self, cx: &mut Context<Self>) {
        self.spotify_state.stopped.store(true, Ordering::Relaxed);
        self.spotify_state.shown = SpotifySigning::Idle;
        self.spotify_state.task = Task::ready(());
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
                            "Signed out of Spotify: the playlists keep their rows and no longer \
                             follow it"
                                .to_owned(),
                        ),
                        cx,
                    );
                }
                Err(error) => toast::tell(toast::could_not("let the account go", &error), cx),
            });
        })
        .detach();
    }

    fn read_the_spotify_account(&mut self, upkeep: Upkeep, cx: &mut Context<Self>) {
        if self.spotify_state.reading {
            return;
        }
        let Some((library, reference)) = self.library.read(cx).follows_links() else {
            self.report(Notice::Trouble(OFFLINE.to_owned()), cx);
            return;
        };
        self.spotify_state.reading = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let followed = cx
                .background_executor()
                .spawn(async move {
                    library.follow_profile(reference.as_ref(), &ProfileLink::SpotifyAccount, upkeep)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.spotify_state.reading = false;
                match followed {
                    Err(Error::Refused {
                        op: LookupOp::SpotifyAccount,
                        ..
                    }) => this.report(Notice::Trouble(NO_ACCOUNT.to_owned()), cx),
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

    pub(super) fn spotify_group(&mut self, cx: &mut Context<Self>) -> Div {
        let app = cx.global::<ResonateApp>();
        let reachable = app.spotify.is_some() && app.online.enabled;
        let client_given = !app.online.spotify_client_id.trim().is_empty();
        let signed_in = !app.online.spotify_refresh_token.trim().is_empty();
        let reading = self.spotify_state.reading;

        let body = kit::section_body().child(kit::field(
            "Client ID",
            self.key_field(
                "spotify-client-id",
                &self.spotify_client,
                |this, window, cx| this.leave_the_account(window, cx),
                cx,
            ),
        ));
        if signed_in && client_given {
            return body
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(self.spotify_button(
                            "spotify-follow",
                            if reading {
                                "Reading the account…"
                            } else {
                                "Follow the account"
                            },
                            Icon::Link,
                            !reachable || reading,
                            |this, _, cx| this.read_the_spotify_account(Upkeep::InStep, cx),
                            cx,
                        ))
                        .child(self.spotify_button(
                            "spotify-move",
                            "Move to Resonate once",
                            Icon::Download,
                            !reachable || reading,
                            |this, _, cx| this.read_the_spotify_account(Upkeep::Once, cx),
                            cx,
                        ))
                        .child(self.spotify_button(
                            "spotify-sign-out",
                            "Sign out of Spotify",
                            Icon::Close,
                            false,
                            |this, _, cx| this.sign_out_of_spotify(cx),
                            cx,
                        )),
                )
                .child(note(SIGNED_IN_NOTE))
                .child(note(ACCOUNT_NOTE));
        }

        let signing: Div = match self.spotify_state.shown.clone() {
            SpotifySigning::Idle => div().child(self.spotify_button(
                "spotify-sign-in",
                "Sign in to Spotify",
                Icon::Link,
                !reachable || !client_given,
                |this, _, cx| this.sign_in_to_spotify(cx),
                cx,
            )),
            SpotifySigning::Asking => div().child(self.spotify_button(
                "spotify-sign-in",
                "Opening the sign-in…",
                Icon::Link,
                true,
                |_, _, _| {},
                cx,
            )),
            SpotifySigning::Waiting(address) => div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(self.spotify_button(
                    "spotify-open-page",
                    "Open the page again",
                    Icon::Globe,
                    false,
                    move |_, _, cx| cx.open_url(&address),
                    cx,
                ))
                .child(self.spotify_button(
                    "spotify-stop-sign-in",
                    "Stop",
                    Icon::Stop,
                    false,
                    |this, _, cx| this.stop_signing_in_to_spotify(cx),
                    cx,
                )),
        };
        body.child(signing).child(note(ACCOUNT_NOTE))
    }
}
