use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Window};

use crate::{
    ResonateApp,
    views::{
        field::{Blurred, Field, Submitted},
        settings::{Account, LastfmAccount, Service, TidalAccount, YoutubeMusicField, spotify},
    },
    words,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entered {
    Contact,
    AcoustidKey,
    ReplicationToken,
    AuddToken,
    ListenbrainzToken,
    Account(Service, Account),
    Tidal(TidalAccount),
    Lastfm(LastfmAccount),
    SpotifyClient,
    YoutubeMusic(YoutubeMusicField),
    DiscordApp,
    DiscordIcon,
    SyncServer,
    SyncDevice,
    SharedName,
    SyncCode,
    MinimumLength,
    Layout,
    MusicFolder,
    Filter,
    Figure,
    Looking,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Left {
    Contact,
    AcoustidKey,
    ReplicationToken,
    AuddToken,
    ListenbrainzToken,
    Layout,
    MusicFolder,
    Figure,
}

pub(crate) struct SettingsFields {
    pub(crate) contact: Entity<Field>,
    pub(crate) acoustid: Entity<Field>,
    pub(crate) replication: Entity<Field>,
    pub(crate) audd: Entity<Field>,
    pub(crate) listenbrainz: Entity<Field>,
    pub(crate) subsonic: [Entity<Field>; 3],
    pub(crate) jellyfin: [Entity<Field>; 3],
    pub(crate) tidal: [Entity<Field>; 6],
    pub(crate) lastfm: [Entity<Field>; 2],
    pub(crate) spotify_client: Entity<Field>,
    pub(crate) youtube_music: [Entity<Field>; 2],
    pub(crate) discord_app: Entity<Field>,
    pub(crate) discord_icon: Entity<Field>,
    pub(crate) sync_server: Entity<Field>,
    pub(crate) sync_device: Entity<Field>,
    pub(crate) shared_name: Entity<Field>,
    pub(crate) sync_code: Entity<Field>,
    pub(crate) organising: Entity<Field>,
    pub(crate) minimum_length: Entity<Field>,
    pub(crate) root: Entity<Field>,
    pub(crate) finding: Entity<Field>,
    pub(crate) figure: Entity<Field>,
    pub(crate) looking: Entity<Field>,
    pub(crate) measured_as: Entity<Field>,
}

impl EventEmitter<Entered> for SettingsFields {}

impl EventEmitter<Left> for SettingsFields {}

fn holding(
    placeholder: fn() -> &'static str,
    held: String,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Field> {
    cx.new(|cx| {
        let mut field = Field::new(placeholder, window, cx);
        field.hold(held, cx);
        field
    })
}

fn holding_a_secret(
    placeholder: fn() -> &'static str,
    held: String,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Field> {
    cx.new(|cx| {
        let mut field = Field::new(placeholder, window, cx).masked();
        field.hold(held, cx);
        field
    })
}

fn empty(placeholder: fn() -> &'static str, window: &mut Window, cx: &mut App) -> Entity<Field> {
    cx.new(|cx| Field::new(placeholder, window, cx))
}

impl SettingsFields {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let global = cx.global::<ResonateApp>();
        let online = global.online.clone();
        let presence = global.presence.clone();
        let synchronised = global.synchronised.clone();
        let organise_as = global.organise_as.clone();
        let minimum_length = global
            .library
            .music_filters()
            .minimum_length
            .seconds()
            .to_string();

        let fields = Self {
            contact: holding(
                words::root::contact_placeholder,
                online.contact.clone(),
                window,
                cx,
            ),
            acoustid: holding_a_secret(
                words::root::acoustid_placeholder,
                online.acoustid_key.clone(),
                window,
                cx,
            ),
            replication: holding_a_secret(
                words::root::replication_token_placeholder,
                online.musicbrainz_replication_token.clone(),
                window,
                cx,
            ),
            audd: holding_a_secret(
                words::root::audd_placeholder,
                online.audd_token.clone(),
                window,
                cx,
            ),
            listenbrainz: holding_a_secret(
                words::root::listenbrainz_placeholder,
                online.listenbrainz_token.clone(),
                window,
                cx,
            ),
            subsonic: Account::fields(Service::Subsonic, &online, window, cx),
            jellyfin: Account::fields(Service::Jellyfin, &online, window, cx),
            tidal: TidalAccount::fields(&online, window, cx),
            lastfm: LastfmAccount::fields(&online, window, cx),
            spotify_client: spotify::field(&online, window, cx),
            youtube_music: YoutubeMusicField::fields(&online, window, cx),
            discord_app: holding(
                words::root::discord_app_placeholder,
                presence.app.map(|app| app.to_string()).unwrap_or_default(),
                window,
                cx,
            ),
            discord_icon: holding(
                words::root::discord_icon_placeholder,
                presence
                    .icon
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                window,
                cx,
            ),
            sync_server: holding(
                words::settings_sync::sync_server_placeholder,
                synchronised.server.clone(),
                window,
                cx,
            ),
            sync_device: holding(
                words::settings_sync::sync_device_placeholder,
                synchronised.device.clone(),
                window,
                cx,
            ),
            shared_name: holding(
                words::settings_sync::shared_name_placeholder,
                synchronised.shared_name.clone(),
                window,
                cx,
            ),
            sync_code: empty(words::settings_sync::sync_code_placeholder, window, cx),
            organising: holding(words::root::organising_placeholder, organise_as, window, cx),
            minimum_length: holding(
                words::root::seconds_from_0_to_600,
                minimum_length,
                window,
                cx,
            ),
            root: empty(words::root::typed_root_placeholder, window, cx),
            finding: empty(words::settings_pane::filter_placeholder, window, cx),
            figure: empty(words::root::figure_placeholder, window, cx),
            looking: empty(words::root::looking_placeholder, window, cx),
            measured_as: empty(words::root::measured_as_placeholder, window, cx),
        };

        for (field, entered) in [
            (&fields.contact, Entered::Contact),
            (&fields.acoustid, Entered::AcoustidKey),
            (&fields.replication, Entered::ReplicationToken),
            (&fields.audd, Entered::AuddToken),
            (&fields.listenbrainz, Entered::ListenbrainzToken),
            (&fields.discord_app, Entered::DiscordApp),
            (&fields.discord_icon, Entered::DiscordIcon),
            (&fields.sync_server, Entered::SyncServer),
            (&fields.sync_device, Entered::SyncDevice),
            (&fields.shared_name, Entered::SharedName),
            (&fields.sync_code, Entered::SyncCode),
            (&fields.minimum_length, Entered::MinimumLength),
            (&fields.organising, Entered::Layout),
            (&fields.root, Entered::MusicFolder),
            (&fields.finding, Entered::Filter),
            (&fields.figure, Entered::Figure),
            (&fields.looking, Entered::Looking),
        ] {
            Self::says_when_entered(field, entered, cx);
        }
        for (field, left) in [
            (&fields.contact, Left::Contact),
            (&fields.acoustid, Left::AcoustidKey),
            (&fields.replication, Left::ReplicationToken),
            (&fields.audd, Left::AuddToken),
            (&fields.listenbrainz, Left::ListenbrainzToken),
            (&fields.organising, Left::Layout),
            (&fields.root, Left::MusicFolder),
            (&fields.figure, Left::Figure),
        ] {
            cx.subscribe(field, move |_, _, _: &Blurred, cx| cx.emit(left))
                .detach();
        }

        fields
    }

    pub(crate) fn says_when_entered(
        field: &Entity<Field>,
        entered: Entered,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(field, move |_, _, _: &Submitted, cx| cx.emit(entered))
            .detach();
    }

    pub(crate) fn each(&self) -> impl Iterator<Item = &Entity<Field>> {
        [
            &self.contact,
            &self.acoustid,
            &self.replication,
            &self.audd,
            &self.listenbrainz,
            &self.discord_app,
            &self.discord_icon,
            &self.sync_server,
            &self.sync_device,
            &self.shared_name,
            &self.sync_code,
            &self.organising,
            &self.minimum_length,
            &self.root,
            &self.finding,
            &self.figure,
            &self.looking,
            &self.spotify_client,
        ]
        .into_iter()
        .chain(&self.subsonic)
        .chain(&self.jellyfin)
        .chain(&self.tidal)
        .chain(&self.lastfm)
        .chain(&self.youtube_music)
    }
}
