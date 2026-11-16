use std::{collections::BTreeMap, path::PathBuf};

use gpui::{Context, Pixels, Point, ScrollHandle, Task};
use resonate_engine::{Learning, NodeName, Unlearnt};

use crate::{
    Newer,
    views::{
        dropping::FolderStanding,
        focus::Controls,
        pulse::Pulse,
        remote::RemoteSong,
        settings::{
            Armed, Category, CheckingYoutubeMusic, Fitting, LastfmSigning, ListeningTest, Page,
            QuickConnecting, Setup, SigningIn, SigningInToYoutubeMusic, SpotifyState, Tending,
            sync::DRIVEN_ROWS_AT_FIRST,
        },
        slider::Rail,
    },
};

pub(crate) struct Remotely {
    pub(in crate::views) songs: Vec<RemoteSong>,
    pub(crate) collapsed: bool,
    pub(crate) fits_from: Option<Pixels>,
    pub(crate) expanded_by_hand: bool,
    pub(crate) heading_pressed_at: Option<Point<Pixels>>,
    pub(crate) controls: Controls,
    pub(crate) driving: Option<(String, String)>,
    pub(crate) driving_redraws: Pulse,
    pub(crate) mirror_redraws: Pulse,
    pub(crate) driving_rails: BTreeMap<u32, Rail>,
    pub(crate) driving_rows: usize,
    pub(crate) scroll: ScrollHandle,
}

pub(crate) struct Accounts {
    pub(crate) spotify: SpotifyState,
    pub(crate) youtube_music_check: CheckingYoutubeMusic,
    pub(crate) youtube_music_sign_in: SigningInToYoutubeMusic,
    pub(crate) jellyfin_quick: QuickConnecting,
    pub(crate) signing_in: SigningIn,
    pub(crate) lastfm: LastfmSigning,
    pub(crate) _token_checked: Task<()>,
}

pub(crate) struct SettingsShown {
    pub(crate) category: Category,
    pub(crate) page: Option<Page>,
    pub(crate) scroll: ScrollHandle,
    pub(crate) rail_scroll: ScrollHandle,
    pub(crate) music_folder_standing: FolderStanding,
    pub(crate) looking_at_the_music_folder: Task<()>,
    pub(crate) reading_the_room: Task<()>,
    pub(crate) looking_at_the_room: Task<()>,
    pub(crate) room_shape: Option<(PathBuf, bool)>,
    pub(crate) audition: Option<ListeningTest>,
    pub(crate) test_setup: Setup,
    pub(crate) fitting_the_preamp: Fitting,
    pub(crate) making_the_vault: Task<()>,
    pub(crate) tending: Tending,
    pub(crate) looking_at_the_typed_root: Task<()>,
    pub(crate) armed: Option<Armed>,
    pub(crate) code_regenerated: bool,
    pub(crate) pairing_shown: bool,
    pub(crate) own_server_asked: bool,
    pub(crate) reset_everything_landed: bool,
    pub(crate) vault_named: Option<PathBuf>,
    pub(crate) newer: Option<Newer>,
    pub(crate) looking_for_updates: Task<()>,
    pub(crate) trying: Option<(NodeName, Task<()>)>,
    pub(crate) heard: Option<(NodeName, Result<Learning, Unlearnt>)>,
}

pub(crate) struct SettingsPane {
    pub(crate) shown: SettingsShown,
    pub(crate) accounts: Accounts,
    pub(crate) remotely: Remotely,
}

impl SettingsPane {
    pub(crate) fn new(category: Category, cx: &mut Context<Self>) -> Self {
        Self::watch_remote_playback(cx);

        let mut pane = Self {
            shown: SettingsShown {
                category,
                page: None,
                scroll: ScrollHandle::new(),
                rail_scroll: ScrollHandle::new(),
                music_folder_standing: FolderStanding::default(),
                looking_at_the_music_folder: Task::ready(()),
                reading_the_room: Task::ready(()),
                looking_at_the_room: Task::ready(()),
                room_shape: None,
                audition: None,
                test_setup: Setup::default(),
                fitting_the_preamp: Fitting::idle(),
                making_the_vault: Task::ready(()),
                tending: Tending::default(),
                looking_at_the_typed_root: Task::ready(()),
                armed: None,
                code_regenerated: false,
                pairing_shown: false,
                own_server_asked: false,
                reset_everything_landed: false,
                vault_named: None,
                newer: None,
                looking_for_updates: Task::ready(()),
                trying: None,
                heard: None,
            },
            accounts: Accounts {
                spotify: SpotifyState::default(),
                youtube_music_check: CheckingYoutubeMusic::default(),
                youtube_music_sign_in: SigningInToYoutubeMusic::default(),
                jellyfin_quick: QuickConnecting::default(),
                signing_in: SigningIn::default(),
                lastfm: LastfmSigning::default(),
                _token_checked: Task::ready(()),
            },
            remotely: Remotely {
                songs: Vec::new(),
                collapsed: false,
                fits_from: None,
                expanded_by_hand: false,
                heading_pressed_at: None,
                controls: Controls::default(),
                driving: None,
                driving_redraws: Pulse::default(),
                mirror_redraws: Pulse::default(),
                driving_rails: BTreeMap::new(),
                driving_rows: DRIVEN_ROWS_AT_FIRST,
                scroll: ScrollHandle::new(),
            },
        };
        pane.look_for_a_newer_release(cx);
        pane
    }

    pub(crate) fn disarm(&mut self) {
        self.lower_every_armed_press();
        self.shown.reset_everything_landed = false;
    }

    pub(crate) fn lower_every_armed_press(&mut self) {
        self.shown.armed = None;
        self.shown.pairing_shown = false;
    }

    pub(crate) fn is_armed(&self, armed: &Armed) -> bool {
        self.shown.armed.as_ref() == Some(armed)
    }

    pub(crate) fn arm(&mut self, armed: Armed) {
        self.shown.armed = Some(armed);
    }

    pub(crate) fn lower(&mut self, armed: &Armed) {
        if self.is_armed(armed) {
            self.shown.armed = None;
        }
    }
}
