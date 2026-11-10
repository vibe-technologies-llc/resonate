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
    words,
};

fn told(connection: YoutubeMusicConnection) -> &'static str {
    match connection {
        YoutubeMusicConnection::Premium => words::settings_youtube_music::premium(),
        YoutubeMusicConnection::SignedInWithoutPremium => {
            words::settings_youtube_music::without_premium()
        }
        YoutubeMusicConnection::NotSignedIn => words::settings_youtube_music::not_signed_in(),
        YoutubeMusicConnection::ToolMissing => words::settings_youtube_music::tool_missing(),
        YoutubeMusicConnection::CheckSongGone => words::settings_youtube_music::check_song_gone(),
        YoutubeMusicConnection::CouldNotCheck => words::settings_youtube_music::could_not_check(),
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
        None => Notice::Trouble(words::settings_youtube_music::no_answer().to_owned()),
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
        YoutubeMusicSignIn::SignedIn { .. } => {
            Notice::Done(words::settings_youtube_music::signed_in().to_owned())
        }
        YoutubeMusicSignIn::Closed => {
            Notice::Noted(words::settings_youtube_music::sign_in_closed().to_owned())
        }
        YoutubeMusicSignIn::ProgramMissing => {
            Notice::Trouble(words::settings_youtube_music::sign_in_program_missing().to_owned())
        }
        YoutubeMusicSignIn::Failed => {
            Notice::Trouble(words::settings_youtube_music::sign_in_failed().to_owned())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YoutubeMusicField {
    Cookies,
    Program,
}

impl YoutubeMusicField {
    pub(crate) const ALL: [Self; 2] = [Self::Cookies, Self::Program];

    fn placeholder(self) -> &'static str {
        match self {
            Self::Cookies => words::settings_youtube_music::a_browser_or_a_cookies_file(),
            Self::Program => words::settings_youtube_music::where_yt_dlp_is_blank_looks(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Cookies => words::settings_youtube_music::cookies(),
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
            (YoutubeMusicField::Cookies, true) => {
                words::settings_youtube_music::youtube_music_is_asked_with_no()
            }
            (YoutubeMusicField::Cookies, false) => {
                words::settings_youtube_music::youtube_music_is_asked_with_these()
            }
            (YoutubeMusicField::Program, true) => {
                words::settings_youtube_music::yt_dlp_is_looked_for_on()
            }
            (YoutubeMusicField::Program, false) => {
                words::settings_youtube_music::yt_dlp_is_run_from_there()
            }
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
                    Notice::Done(words::settings_youtube_music::signed_out().to_owned())
                } else {
                    Notice::Trouble(words::settings_youtube_music::sign_out_failed().to_owned())
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
            (true, _) => words::settings_youtube_music::signing_in_label(),
            (false, true) => words::settings_youtube_music::sign_in_again_label(),
            (false, false) => words::settings_youtube_music::sign_in_label(),
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
            words::settings_youtube_music::sign_out_label(),
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
            words::settings_youtube_music::sign_in_note()
        } else {
            words::settings_youtube_music::no_sign_in_note()
        })
    }

    fn check_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let app = cx.global::<ResonateApp>();
        let reachable =
            app.checks_youtube_music.is_some() && app.online.enabled && app.online.youtube_music;
        let asking = self.youtube_music_check.asking;

        action(
            "youtube-music-check",
            if asking {
                words::settings_youtube_music::checking_label()
            } else {
                words::settings_youtube_music::check_label()
            },
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
                    words::settings_youtube_music::use_youtube_music(),
                    words::settings_youtube_music::off_nothing_is_asked_of_youtube(),
                    on,
                    "youtube-music",
                ),
                move |this, _, cx| this.turn_youtube_music(!on, cx),
                cx,
            ))
            .child(note(words::settings_youtube_music::terms_note()));
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
                    words::settings_youtube_music::keep_a_playlist_in_youtubes_order(),
                    words::settings_youtube_music::off_its_songs_stand_in_the(),
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
        body.note(words::settings_youtube_music::youtube_music_note())
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
            words::settings_youtube_music::terms_note(),
            "Saving songs this way breaks YouTube's terms, even on a paid account, and uses this \
             account's cookies."
        );
    }
}
