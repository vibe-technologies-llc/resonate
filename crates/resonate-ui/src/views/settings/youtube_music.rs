use std::sync::Arc;

use gpui::{
    AppContext as _, BorrowAppContext as _, Context, Div, Entity, ParentElement as _, Stateful,
    Task, Window,
};

use crate::{
    Notice, ResonateApp, Setting,
    icons::Icon,
    settings::{Online, YoutubeMusicConnection, YoutubeMusicSignIn},
    views::{
        field::{Field, Submitted},
        kit,
        root::RootView,
        settings::{Rows, action, note, switch_row},
    },
};

const TERMS_NOTE: &str = "Saving songs this way breaks YouTube's terms, even on a paid account, \
                          and uses this account's cookies.";

const YOUTUBE_MUSIC_NOTE: &str = "Nothing is asked of YouTube until this is on, and only while \
                                  Online is on. yt-dlp is a program of its own, installed apart. \
                                  The cookies name a browser as yt-dlp does, as firefox or \
                                  firefox:/path/to/profile for a fork such as Zen, or the path \
                                  of a cookies file. With none, songs come at the free tier.";

const CHECK_LABEL: &str = "Check connection";

const CHECKING_LABEL: &str = "Asking YouTube…";

const PREMIUM: &str = "Premium formats are available";

const WITHOUT_PREMIUM: &str = "Signed in, but no Premium formats are available";

const NOT_SIGNED_IN: &str = "Not signed in to YouTube";

const TOOL_MISSING: &str = "yt-dlp is missing";

const CHECK_SONG_GONE: &str = "The song this check asks about has left YouTube, so nothing could \
                               be told about the account";

const COULD_NOT_CHECK: &str = "YouTube could not be asked just now, so nothing is known of the \
                               account. Try again in a while";

const NO_ANSWER: &str = "YouTube Music is checked only while it and Online are on";

const SIGN_IN_LABEL: &str = "Sign in to YouTube Music";

const SIGN_IN_AGAIN_LABEL: &str = "Sign in again";

const SIGNING_IN_LABEL: &str = "Signing in…";

const SIGN_OUT_LABEL: &str = "Sign out";

const SIGNED_IN: &str = "Signed in to YouTube Music";

const SIGN_IN_CLOSED: &str = "The sign-in window was closed, nothing kept";

const SIGN_IN_PROGRAM_MISSING: &str =
    "resonate-signin is missing, so the sign-in window cannot open";

const SIGN_IN_FAILED: &str = "The sign-in window stopped without signing in";

const SIGNED_OUT: &str = "Signed out of YouTube Music";

const SIGN_OUT_FAILED: &str = "What the sign-in window kept could not be taken away";

const SIGN_IN_NOTE: &str = "Sign in opens a window on Google's sign-in and keeps that account's \
                            cookies for yt-dlp alone, in Resonate's own folder. Naming a browser \
                            or a cookies file below is the other way in.";

const NO_SIGN_IN_NOTE: &str = "Install resonate-signin to sign in from here. Until then, name a \
                               browser or a cookies file below.";

const fn told(connection: YoutubeMusicConnection) -> &'static str {
    match connection {
        YoutubeMusicConnection::Premium => PREMIUM,
        YoutubeMusicConnection::SignedInWithoutPremium => WITHOUT_PREMIUM,
        YoutubeMusicConnection::NotSignedIn => NOT_SIGNED_IN,
        YoutubeMusicConnection::ToolMissing => TOOL_MISSING,
        YoutubeMusicConnection::CheckSongGone => CHECK_SONG_GONE,
        YoutubeMusicConnection::CouldNotCheck => COULD_NOT_CHECK,
    }
}

fn notice_of(answered: Option<YoutubeMusicConnection>) -> Notice {
    match answered {
        Some(connection @ YoutubeMusicConnection::Premium) => {
            Notice::Done(told(connection).to_owned())
        }
        Some(
            connection @ (YoutubeMusicConnection::SignedInWithoutPremium
            | YoutubeMusicConnection::NotSignedIn),
        ) => Notice::Noted(told(connection).to_owned()),
        Some(
            connection @ (YoutubeMusicConnection::ToolMissing
            | YoutubeMusicConnection::CheckSongGone
            | YoutubeMusicConnection::CouldNotCheck),
        ) => Notice::Trouble(told(connection).to_owned()),
        None => Notice::Trouble(NO_ANSWER.to_owned()),
    }
}

pub(crate) struct CheckingYoutubeMusic {
    asking: bool,
    task: Task<()>,
}

impl Default for CheckingYoutubeMusic {
    fn default() -> Self {
        Self {
            asking: false,
            task: Task::ready(()),
        }
    }
}

pub(crate) struct SigningInToYoutubeMusic {
    asking: bool,
    task: Task<()>,
}

impl Default for SigningInToYoutubeMusic {
    fn default() -> Self {
        Self {
            asking: false,
            task: Task::ready(()),
        }
    }
}

fn notice_of_the_sign_in(answered: &YoutubeMusicSignIn) -> Notice {
    match answered {
        YoutubeMusicSignIn::SignedIn { .. } => Notice::Done(SIGNED_IN.to_owned()),
        YoutubeMusicSignIn::Closed => Notice::Noted(SIGN_IN_CLOSED.to_owned()),
        YoutubeMusicSignIn::ProgramMissing => Notice::Trouble(SIGN_IN_PROGRAM_MISSING.to_owned()),
        YoutubeMusicSignIn::Failed => Notice::Trouble(SIGN_IN_FAILED.to_owned()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YoutubeMusicField {
    Cookies,
    Program,
}

impl YoutubeMusicField {
    pub(crate) const ALL: [Self; 2] = [Self::Cookies, Self::Program];

    const fn placeholder(self) -> &'static str {
        match self {
            Self::Cookies => "A browser or a cookies file, then press enter",
            Self::Program => "Where yt-dlp is; blank looks on the path",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Cookies => "Cookies",
            Self::Program => "yt-dlp",
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::Cookies => "youtube-music-cookies",
            Self::Program => "yt-dlp",
        }
    }

    const fn nth(self) -> usize {
        match self {
            Self::Cookies => 0,
            Self::Program => 1,
        }
    }

    fn held(self, online: &Online) -> String {
        match self {
            Self::Cookies => online.youtube_music_cookies.clone(),
            Self::Program => online.yt_dlp.clone(),
        }
    }

    fn hold(self, online: &mut Online, given: String) {
        match self {
            Self::Cookies => online.youtube_music_cookies = given,
            Self::Program => online.yt_dlp = given,
        }
    }

    fn setting(self, given: String) -> Setting {
        match self {
            Self::Cookies => Setting::YoutubeMusicCookies(given),
            Self::Program => Setting::YtDlp(given),
        }
    }

    pub(crate) fn fields(
        online: &Online,
        window: &mut Window,
        cx: &mut Context<RootView>,
    ) -> [Entity<Field>; 2] {
        Self::ALL.map(|named| {
            let field = cx.new(|cx| {
                let field = Field::new(named.placeholder(), window, cx);
                let mut field = match named {
                    Self::Cookies => field.masked(),
                    Self::Program => field,
                };
                field.hold(named.held(online), cx);
                field
            });
            cx.subscribe_in(&field, window, move |this, _, _: &Submitted, window, cx| {
                this.youtube_music_given(named, window, cx);
            })
            .detach();
            field
        })
    }
}

impl RootView {
    fn youtube_music_field(&self, named: YoutubeMusicField) -> &Entity<Field> {
        &self.youtube_music[named.nth()]
    }

    fn youtube_music_given(
        &mut self,
        named: YoutubeMusicField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let given = self
            .youtube_music_field(named)
            .read(cx)
            .text()
            .trim()
            .to_owned();
        self.youtube_music_field(named)
            .clone()
            .update(cx, |field, cx| field.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            named.hold(&mut global.online, given.clone());
        });

        let said = match (named, given.is_empty()) {
            (YoutubeMusicField::Cookies, true) => "YouTube Music is asked with no account",
            (YoutubeMusicField::Cookies, false) => "YouTube Music is asked with these cookies",
            (YoutubeMusicField::Program, true) => "yt-dlp is looked for on the path",
            (YoutubeMusicField::Program, false) => "yt-dlp is run from there",
        };
        self.store(&named.setting(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
        self.the_sources_moved(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn give_what_is_typed_of_youtube_music(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        for named in YoutubeMusicField::ALL {
            let field = self.youtube_music_field(named).read(cx);
            let typed_apart = field.is_focused(window)
                && field.text().trim() != named.held(&cx.global::<ResonateApp>().online);
            if typed_apart {
                self.youtube_music_given(named, window, cx);
                return true;
            }
        }
        false
    }

    pub(super) fn put_back_youtube_music(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for named in YoutubeMusicField::ALL {
            let stored = named.held(&cx.global::<ResonateApp>().online);
            let field = self.youtube_music_field(named).clone();
            if field.read(cx).is_focused(window) {
                field.update(cx, |field, cx| field.hold(stored, cx));
            }
        }
    }

    pub(crate) fn forget_youtube_music(&mut self, cx: &mut Context<Self>) {
        for named in YoutubeMusicField::ALL {
            self.youtube_music_field(named)
                .clone()
                .update(cx, |field, cx| field.hold(String::new(), cx));
            cx.update_global::<ResonateApp, _>(|global, _| {
                named.hold(&mut global.online, String::new());
            });
            self.store(&named.setting(String::new()), cx);
        }
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.youtube_music = false;
            global.online.youtube_music_keep_order = true;
        });
        self.store(&Setting::YoutubeMusic(false), cx);
        self.store(&Setting::YoutubeMusicKeepOrder(true), cx);
        self.the_sources_moved(cx);
    }

    fn turn_youtube_music(&mut self, on: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.online.youtube_music = on);
        self.store(&Setting::YoutubeMusic(on), cx);
        self.the_sources_moved(cx);
        cx.notify();
    }

    fn keep_youtubes_order(&mut self, keeps: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.youtube_music_keep_order = keeps;
        });
        self.store(&Setting::YoutubeMusicKeepOrder(keeps), cx);
        cx.notify();
    }

    fn check_the_youtube_music_connection(&mut self, cx: &mut Context<Self>) {
        let app = cx.global::<ResonateApp>();
        let Some(checks) = app.checks_youtube_music.clone() else {
            return;
        };
        let online = app.online.clone();

        self.youtube_music_check.asking = true;
        cx.notify();
        self.youtube_music_check.task = cx.spawn(async move |this, cx| {
            let answered = cx
                .background_executor()
                .spawn(async move { checks(&online) })
                .await;
            let told = this.update(cx, |this, cx| {
                this.youtube_music_check.asking = false;
                this.report(notice_of(answered), cx);
                cx.notify();
            });
            let _ = told;
        });
    }

    fn hold_the_youtube_music_cookies(&mut self, cookies: String, cx: &mut Context<Self>) {
        let named = YoutubeMusicField::Cookies;
        self.youtube_music_field(named)
            .clone()
            .update(cx, |field, cx| field.hold(cookies.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            named.hold(&mut global.online, cookies.clone());
        });
        self.store(&named.setting(cookies), cx);
        self.the_sources_moved(cx);
    }

    fn sign_in_to_youtube_music(&mut self, cx: &mut Context<Self>) {
        let Some(signs_in) = cx.global::<ResonateApp>().signs_in_to_youtube_music.clone() else {
            return;
        };

        self.youtube_music_sign_in.asking = true;
        cx.notify();
        self.youtube_music_sign_in.task = cx.spawn(async move |this, cx| {
            let answered = cx
                .background_executor()
                .spawn(async move { signs_in.sign_in() })
                .await;
            let told = this.update(cx, |this, cx| {
                this.youtube_music_sign_in.asking = false;
                this.report(notice_of_the_sign_in(&answered), cx);
                if let YoutubeMusicSignIn::SignedIn { cookies } = answered {
                    this.hold_the_youtube_music_cookies(cookies, cx);
                    this.check_the_youtube_music_connection(cx);
                }
                cx.notify();
            });
            let _ = told;
        });
    }

    fn sign_out_of_youtube_music(&mut self, cx: &mut Context<Self>) {
        let Some(signs_in) = cx.global::<ResonateApp>().signs_in_to_youtube_music.clone() else {
            return;
        };

        self.youtube_music_sign_in.asking = true;
        cx.notify();
        self.youtube_music_sign_in.task = cx.spawn(async move |this, cx| {
            let gone = cx
                .background_executor()
                .spawn({
                    let signs_in = Arc::clone(&signs_in);
                    async move { signs_in.sign_out() }
                })
                .await;
            let told = this.update(cx, |this, cx| {
                this.youtube_music_sign_in.asking = false;
                let held = cx
                    .global::<ResonateApp>()
                    .online
                    .youtube_music_cookies
                    .clone();
                if signs_in.names_the_window_jar(&held) {
                    this.hold_the_youtube_music_cookies(String::new(), cx);
                }
                let said = if gone {
                    Notice::Done(SIGNED_OUT.to_owned())
                } else {
                    Notice::Trouble(SIGN_OUT_FAILED.to_owned())
                };
                this.report(said, cx);
                cx.notify();
            });
            let _ = told;
        });
    }

    fn sign_in_buttons(&self, row: Div, cx: &mut Context<Self>) -> Div {
        let app = cx.global::<ResonateApp>();
        let Some(signs_in) = app.signs_in_to_youtube_music.clone() else {
            return row;
        };
        if !signs_in.is_offered() {
            return row;
        }
        let reachable = app.online.enabled && app.online.youtube_music;
        let kept = signs_in.names_the_window_jar(&app.online.youtube_music_cookies);
        let asking = self.youtube_music_sign_in.asking;
        let label = match (asking, kept) {
            (true, _) => SIGNING_IN_LABEL,
            (false, true) => SIGN_IN_AGAIN_LABEL,
            (false, false) => SIGN_IN_LABEL,
        };

        let row = row.child(action(
            "youtube-music-sign-in",
            label,
            Icon::Link,
            asking || !reachable,
            |this, _, cx| this.sign_in_to_youtube_music(cx),
            self,
            cx,
        ));
        if !kept {
            return row;
        }
        row.child(action(
            "youtube-music-sign-out",
            SIGN_OUT_LABEL,
            Icon::Close,
            asking,
            |this, _, cx| this.sign_out_of_youtube_music(cx),
            self,
            cx,
        ))
    }

    fn sign_in_note(cx: &Context<Self>) -> Option<&'static str> {
        let signs_in = cx
            .global::<ResonateApp>()
            .signs_in_to_youtube_music
            .as_ref()?;
        Some(if signs_in.is_offered() {
            SIGN_IN_NOTE
        } else {
            NO_SIGN_IN_NOTE
        })
    }

    fn check_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let app = cx.global::<ResonateApp>();
        let reachable =
            app.checks_youtube_music.is_some() && app.online.enabled && app.online.youtube_music;
        let asking = self.youtube_music_check.asking;

        action(
            "youtube-music-check",
            if asking { CHECKING_LABEL } else { CHECK_LABEL },
            Icon::Link,
            asking || !reachable,
            |this, _, cx| this.check_the_youtube_music_connection(cx),
            self,
            cx,
        )
    }

    pub(super) fn youtube_music_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let online = &cx.global::<ResonateApp>().online;
        let on = online.youtube_music;
        let keeps = online.youtube_music_keep_order;

        let mut body = kit::rows()
            .child(self.in_the_ring(
                "youtube-music",
                switch_row(
                    "Use YouTube Music",
                    "Off, nothing is asked of YouTube",
                    on,
                    "youtube-music",
                ),
                move |this, _, cx| this.turn_youtube_music(!on, cx),
                cx,
            ))
            .child(note(TERMS_NOTE));
        for named in YoutubeMusicField::ALL {
            body = body.child(kit::field(
                named.label(),
                self.text_field(
                    named.id(),
                    self.youtube_music_field(named),
                    |this, window, cx| this.leave_the_account(window, cx),
                    cx,
                ),
            ));
        }
        let body = body
            .child(self.in_the_ring(
                "youtube-music-keep-order",
                switch_row(
                    "Keep a playlist in YouTube's order",
                    "Off, its songs stand in the order they land",
                    keeps,
                    "youtube-music-keep-order",
                ),
                move |this, _, cx| this.keep_youtubes_order(!keeps, cx),
                cx,
            ))
            .child({
                let row = self.sign_in_buttons(kit::action_row(), cx);
                row.child(self.check_button(cx))
            });
        let body = match Self::sign_in_note(cx) {
            Some(said) => body.note(said),
            None => body,
        };
        body.note(YOUTUBE_MUSIC_NOTE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_says_each_standing_in_words_of_its_own() {
        let connections = [
            YoutubeMusicConnection::Premium,
            YoutubeMusicConnection::SignedInWithoutPremium,
            YoutubeMusicConnection::NotSignedIn,
            YoutubeMusicConnection::ToolMissing,
            YoutubeMusicConnection::CheckSongGone,
            YoutubeMusicConnection::CouldNotCheck,
        ];

        let mut said = connections.map(told);
        said.sort_unstable();

        assert!(said.windows(2).all(|pair| pair[0] != pair[1]));
        assert!(matches!(
            notice_of(Some(YoutubeMusicConnection::Premium)),
            Notice::Done(_)
        ));
        assert!(matches!(
            notice_of(Some(YoutubeMusicConnection::ToolMissing)),
            Notice::Trouble(_)
        ));
        assert!(matches!(
            notice_of(Some(YoutubeMusicConnection::CheckSongGone)),
            Notice::Trouble(_)
        ));
        assert!(matches!(
            notice_of(Some(YoutubeMusicConnection::CouldNotCheck)),
            Notice::Trouble(_)
        ));
    }

    #[test]
    fn the_terms_are_said_as_the_listener_was_told_them() {
        assert_eq!(
            TERMS_NOTE,
            "Saving songs this way breaks YouTube's terms, even on a paid account, and uses this \
             account's cookies."
        );
    }
}
