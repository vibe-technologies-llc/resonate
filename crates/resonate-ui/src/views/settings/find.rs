use std::{cmp::Reverse, sync::OnceLock};

pub(crate) use crate::SettingsCategory as Category;
use crate::{ConfigKey, icons::Icon, system::sound_system, words};

impl Category {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Output => words::settings_find::output(),
            Self::Processing => words::settings_find::processing(),
            Self::Equaliser => words::settings_find::equaliser(),
            Self::Playback => words::settings_find::playback(),
            Self::Library => words::settings_find::library(),
            Self::Filters => words::settings_find::filters(),
            Self::Online => words::settings_find::online(),
            Self::Recognition => words::settings_find::recognition(),
            Self::Accounts => words::settings_find::accounts(),
            Self::Sync => words::settings_find::sync(),
            Self::Desktop => words::settings_find::desktop(),
            Self::Appearance => words::settings_find::appearance(),
            Self::Window => words::settings_find::window(),
            Self::About => words::settings_find::about(),
        }
    }

    pub(crate) fn about(self) -> &'static str {
        match self {
            Self::Output => words::settings_find::the_device_the_rate_the_stream(),
            Self::Processing => words::settings_find::what_may_touch_the_samples_between(),
            Self::Equaliser => words::settings_find::the_correction_applied_to_each_device(),
            Self::Playback => words::settings_find::what_the_queue_keeps_between_runs(),
            Self::Library => words::settings_find::the_folders_a_scan_walks_keeping(),
            Self::Filters => words::settings_find::which_file_extensions_and_track_lengths(),
            Self::Online => words::settings_find::whether_the_player_may_reach_the(),
            Self::Recognition => words::settings_find::studying_tracks_and_naming_a_song(),
            Self::Accounts => words::settings_find::music_services_sign_ins_server_credentials(),
            Self::Sync => words::settings_find::your_other_devices_kept_in_step(),
            Self::Desktop => words::settings_find::what_this_player_tells_the_rest(),
            Self::Appearance => words::settings_find::the_palette_the_window_is_painted(),
            Self::Window => words::settings_find::what_the_titlebar_and_the_sidebar(),
            Self::About => words::settings_find::what_this_build_is_where_it(),
        }
    }

    pub(crate) const fn icon(self) -> Icon {
        match self {
            Self::Output => Icon::Volume,
            Self::Processing => Icon::Inspector,
            Self::Equaliser => Icon::Equaliser,
            Self::Playback => Icon::Play,
            Self::Library => Icon::Folder,
            Self::Filters => Icon::Search,
            Self::Online => Icon::Globe,
            Self::Recognition => Icon::Listen,
            Self::Accounts => Icon::Server,
            Self::Sync => Icon::Sync,
            Self::Desktop => Icon::Computer,
            Self::Appearance => Icon::Appearance,
            Self::Window => Icon::WindowMaximise,
            Self::About => Icon::Info,
        }
    }

    pub(crate) fn groups(self) -> impl Iterator<Item = Group> {
        Group::ALL
            .into_iter()
            .filter(move |group| group.category() == self && group.is_offered())
    }

    pub(crate) fn shown_on(self, page: Option<Page>) -> impl Iterator<Item = Group> {
        self.groups().filter(move |group| group.page() == page)
    }

    pub(crate) fn drawn(self) -> Vec<Drawn> {
        let mut drawn: Vec<Drawn> = Vec::new();
        for group in self.groups() {
            match group.page() {
                None => drawn.push(Drawn::Group(group)),
                Some(page) => {
                    let gathered = drawn.iter().any(|held| match held {
                        Drawn::Pages(pages) => pages.contains(&page),
                        Drawn::Group(_) => false,
                    });
                    if gathered {
                        continue;
                    }
                    match drawn.last_mut() {
                        Some(Drawn::Pages(pages))
                            if pages.first().is_some_and(|first| {
                                first.gathered_under() == page.gathered_under()
                            }) =>
                        {
                            pages.push(page);
                        }
                        _ => drawn.push(Drawn::Pages(vec![page])),
                    }
                }
            }
        }
        drawn
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Drawn {
    Group(Group),
    Pages(Vec<Page>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Page {
    HeadphoneCorrection,
    OwnMeasurement,
    RoomMeasurement,
    RoomCorrection,
    ListeningTest,
    Enrichment,
    Identical,
    Devices,
}

impl Page {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 8] = [
        Self::HeadphoneCorrection,
        Self::OwnMeasurement,
        Self::RoomMeasurement,
        Self::RoomCorrection,
        Self::ListeningTest,
        Self::Enrichment,
        Self::Identical,
        Self::Devices,
    ];

    pub(crate) const fn category(self) -> Category {
        match self {
            Self::HeadphoneCorrection
            | Self::OwnMeasurement
            | Self::RoomMeasurement
            | Self::RoomCorrection
            | Self::ListeningTest => Category::Equaliser,
            Self::Enrichment | Self::Identical => Category::Library,
            Self::Devices => Category::Sync,
        }
    }

    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::HeadphoneCorrection => words::settings_find::headphone_correction(),
            Self::OwnMeasurement => words::settings_find::your_own_measurement(),
            Self::RoomMeasurement => words::settings_find::measure_with_a_microphone(),
            Self::RoomCorrection => words::settings_find::room_correction(),
            Self::ListeningTest => words::settings_find::listening_test(),
            Self::Enrichment => words::settings_find::rescan_refresh_and_retag(),
            Self::Identical => words::settings_identical::identical_songs(),
            Self::Devices => words::settings_find::your_devices(),
        }
    }

    pub(crate) fn about(self) -> &'static str {
        match self {
            Self::HeadphoneCorrection => {
                words::settings_find::autoeqs_measured_corrections_the_one_for()
            }
            Self::OwnMeasurement => {
                words::settings_find::a_response_measured_elsewhere_fitted_toward()
            }
            Self::RoomMeasurement => words::settings_find::a_sweep_played_through_the_output(),
            Self::RoomCorrection => words::settings_find::an_impulse_response_or_speakers_placed(),
            Self::ListeningTest => words::settings_find::hear_whether_a_setting_can_be(),
            Self::Enrichment => words::settings_find::read_the_files_again_ask_what(),
            Self::Identical => words::settings_identical::page_about(),
            Self::Devices => words::settings_find::who_shares_the_code_and_driving(),
        }
    }

    pub(crate) const fn icon(self) -> Icon {
        match self {
            Self::HeadphoneCorrection => Icon::Download,
            Self::OwnMeasurement => Icon::Analysis,
            Self::RoomMeasurement => Icon::Listen,
            Self::ListeningTest => Icon::Check,
            Self::RoomCorrection => Icon::Volume,
            Self::Enrichment => Icon::Redo,
            Self::Identical => Icon::Fold,
            Self::Devices => Icon::Phone,
        }
    }

    pub(crate) fn gathered_under(self) -> &'static str {
        match self {
            Self::HeadphoneCorrection
            | Self::OwnMeasurement
            | Self::RoomMeasurement
            | Self::RoomCorrection
            | Self::ListeningTest => words::settings_find::corrections_and_tests(),
            Self::Enrichment | Self::Identical => {
                words::settings_find::keeping_the_catalog_current()
            }
            Self::Devices => words::settings_find::your_other_devices(),
        }
    }

    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::HeadphoneCorrection => "headphone-correction",
            Self::OwnMeasurement => "own-measurement",
            Self::RoomMeasurement => "room-measurement",
            Self::RoomCorrection => "room-correction",
            Self::ListeningTest => "listening-test",
            Self::Enrichment => "enrichment",
            Self::Identical => "identical",
            Self::Devices => "devices",
        }
    }

    #[cfg(test)]
    pub(crate) fn groups(self) -> impl Iterator<Item = Group> {
        Group::ALL
            .into_iter()
            .filter(move |group| group.page() == Some(self) && group.is_offered())
    }
}

pub(crate) struct Described {
    pub(crate) category: Category,
    pub(crate) page: Option<Page>,
    pub(crate) title: &'static str,
    pub(crate) hint: &'static str,
    pub(crate) also_called: &'static str,
    pub(crate) keys: &'static [ConfigKey],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Group {
    Device,
    DeviceVolume,
    SampleRate,
    GraphRate,
    Buffer,
    Dop,
    Bluetooth,
    FlacRenderers,
    Resampler,
    Dither,
    NoiseShaping,
    ReplayGain,
    TruePeak,
    LossySources,
    Crossfeed,
    Equalising,
    BoundTo,
    Bands,
    AutoEq,
    Measurers,
    OwnMeasurement,
    RoomMeasurement,
    RoomCorrection,
    Speakers,
    Abx,
    Resuming,
    Shuffling,
    Repeating,
    PreviousButton,
    Folders,
    Rescanning,
    LookingUp,
    Retagging,
    Identical,
    Organising,
    Vault,
    Inbox,
    History,
    MusicExtensions,
    MinimumLength,
    Hidden,
    Lookups,
    AfterScan,
    Metered,
    Lyrics,
    Contact,
    Studies,
    Recognition,
    Listening,
    ResonateKey,
    Subsonic,
    Jellyfin,
    Tidal,
    YoutubeMusic,
    Spotify,
    Lastfm,
    Submitting,
    Syncing,
    Pairing,
    SyncDevices,
    SyncControl,
    SyncConnection,
    SyncedSongs,
    Notifications,
    Discord,
    DiscordShows,
    Colour,
    Layout,
    Language,
    Motion,
    Scrollbars,
    LiveSpectrum,
    WindowButtons,
    Tabs,
    WindowState,
    VolumeWheel,
    MouseNavigation,
    Build,
    Updates,
    Places,
    Everything,
}

impl Group {
    pub(crate) const ALL: [Self; 81] = [
        Self::Device,
        Self::DeviceVolume,
        Self::SampleRate,
        Self::GraphRate,
        Self::Buffer,
        Self::Dop,
        Self::Bluetooth,
        Self::FlacRenderers,
        Self::Resampler,
        Self::Dither,
        Self::NoiseShaping,
        Self::ReplayGain,
        Self::TruePeak,
        Self::LossySources,
        Self::Crossfeed,
        Self::Equalising,
        Self::BoundTo,
        Self::Bands,
        Self::AutoEq,
        Self::Measurers,
        Self::OwnMeasurement,
        Self::RoomMeasurement,
        Self::RoomCorrection,
        Self::Speakers,
        Self::Abx,
        Self::Resuming,
        Self::Shuffling,
        Self::Repeating,
        Self::PreviousButton,
        Self::Folders,
        Self::Rescanning,
        Self::LookingUp,
        Self::Retagging,
        Self::Identical,
        Self::Organising,
        Self::Vault,
        Self::Inbox,
        Self::History,
        Self::MusicExtensions,
        Self::MinimumLength,
        Self::Hidden,
        Self::Lookups,
        Self::AfterScan,
        Self::Metered,
        Self::Lyrics,
        Self::Contact,
        Self::Studies,
        Self::Recognition,
        Self::Listening,
        Self::ResonateKey,
        Self::Subsonic,
        Self::Jellyfin,
        Self::Tidal,
        Self::YoutubeMusic,
        Self::Spotify,
        Self::Lastfm,
        Self::Submitting,
        Self::Syncing,
        Self::Pairing,
        Self::SyncDevices,
        Self::SyncControl,
        Self::SyncConnection,
        Self::SyncedSongs,
        Self::Notifications,
        Self::Discord,
        Self::DiscordShows,
        Self::Colour,
        Self::Layout,
        Self::Language,
        Self::Motion,
        Self::Scrollbars,
        Self::LiveSpectrum,
        Self::WindowButtons,
        Self::Tabs,
        Self::WindowState,
        Self::VolumeWheel,
        Self::MouseNavigation,
        Self::Build,
        Self::Updates,
        Self::Places,
        Self::Everything,
    ];

    pub(crate) const fn is_offered(self) -> bool {
        self.is_offered_where(
            !cfg!(any(windows, target_os = "macos")),
            cfg!(target_os = "macos"),
        )
    }

    const fn is_offered_where(self, played_through_pipewire: bool, on_a_mac: bool) -> bool {
        let is_pipewires_own = matches!(self, Self::GraphRate | Self::Bluetooth);
        let says_nothing_on_a_mac = matches!(
            self,
            Self::Notifications | Self::Metered | Self::WindowButtons
        );

        (played_through_pipewire || !is_pipewires_own) && !(on_a_mac && says_nothing_on_a_mac)
    }

    pub(crate) const fn is_experimental(self) -> bool {
        matches!(self, Self::LossySources)
    }

    pub(crate) fn category(self) -> Category {
        self.described().category
    }

    pub(crate) fn page(self) -> Option<Page> {
        self.described().page
    }

    pub(crate) fn title(self) -> &'static str {
        self.described().title
    }

    pub(crate) fn hint(self) -> &'static str {
        self.described().hint
    }

    pub(crate) fn also_called(self) -> &'static str {
        self.described().also_called
    }

    pub(crate) fn keys(self) -> &'static [ConfigKey] {
        self.described().keys
    }

    pub(crate) fn described(self) -> Described {
        match self {
            Self::Device => Described {
                category: Category::Output,
                page: None,
                title: words::settings_find::playback_device(),
                hint: words::settings_find::device_hint(),
                also_called: words::settings_find::sink_output_card_dac_headphones_speakers(),
                keys: &[ConfigKey::Sink, ConfigKey::Renderers],
            },
            Self::DeviceVolume => Described {
                category: Category::Output,
                page: None,
                title: words::settings_find::hardware_volume(),
                hint: words::settings_find::device_volume_hint(),
                also_called: words::settings_find::hardware_mixer_amplifier_alsa_route_loudness(),
                keys: &[ConfigKey::DeviceVolume],
            },
            Self::SampleRate => Described {
                category: Category::Output,
                page: None,
                title: words::settings_find::sample_rate(),
                hint: sample_rate_hint(),
                also_called: words::settings_find::bit_perfect_khz_hz_resample_untouched(),
                keys: &[ConfigKey::BitPerfect],
            },
            Self::GraphRate => Described {
                category: Category::Output,
                page: None,
                title: words::settings_find::graph_rate(),
                hint: words::settings_find::graph_rate_hint(),
                also_called: words::settings_find::pipewire_daemon_switch_rate(),
                keys: &[ConfigKey::ForceGraphRate],
            },
            Self::Buffer => Described {
                category: Category::Output,
                page: None,
                title: words::settings_find::buffer(),
                hint: words::settings_find::buffer_hint(),
                also_called: words::settings_find::latency_milliseconds_ms_depth_ring_underrun(),
                keys: &[ConfigKey::BufferMs],
            },
            Self::Dop => Described {
                category: Category::Output,
                page: None,
                title: words::settings_find::dsd_over_pcm(),
                hint: words::settings_find::dop_hint(),
                also_called: words::settings_find::dsd_dsf_dff_sacd_marker_packing(),
                keys: &[ConfigKey::Dop, ConfigKey::DsdLikePcm],
            },
            Self::Bluetooth => Described {
                category: Category::Output,
                page: None,
                title: words::settings_find::bluetooth(),
                hint: words::settings_find::bluetooth_hint(),
                also_called: words::settings_find::headphones_earbuds_airpods_a2dp_wireless_cut(),
                keys: &[
                    ConfigKey::BluetoothWake,
                    ConfigKey::BluetoothLeadMs,
                    ConfigKey::BluetoothAwakeS,
                ],
            },
            Self::FlacRenderers => Described {
                category: Category::Output,
                page: None,
                title: words::settings_find::high_rate_flac_to_renderers(),
                hint: words::settings_find::flac_renderers_hint(),
                also_called: words::settings_find::upnp_dlna_renderer_streamer_network_flac(),
                keys: &[ConfigKey::FlacAbove96kFor],
            },
            Self::Resampler => Described {
                category: Category::Processing,
                page: None,
                title: words::settings_find::resampler(),
                hint: words::settings_find::resampler_hint(),
                also_called: words::settings_find::quality_sinc_kaiser_taps_filter_conversion(),
                keys: &[ConfigKey::Quality, ConfigKey::FilterPhase],
            },
            Self::Dither => Described {
                category: Category::Processing,
                page: None,
                title: words::settings_find::dither(),
                hint: words::settings_find::dither_hint(),
                also_called: words::settings_find::truncation_triangular_rectangular_noise(),
                keys: &[ConfigKey::Dither],
            },
            Self::NoiseShaping => Described {
                category: Category::Processing,
                page: None,
                title: words::settings_find::noise_shaping(),
                hint: words::settings_find::shaping_hint(),
                also_called: words::settings_find::lipshitz_threshold_hearing_terhardt_e_weighted(),
                keys: &[ConfigKey::NoiseShaping],
            },
            Self::ReplayGain => Described {
                category: Category::Processing,
                page: None,
                title: words::settings_find::replaygain(),
                hint: words::settings_find::replay_gain_hint(),
                also_called: words::settings_find::loudness_volume_normalisation_gain_album_track(),
                keys: &[
                    ConfigKey::ReplayGain,
                    ConfigKey::ReplayGainPreAmp,
                    ConfigKey::ReplayGainUntagged,
                ],
            },
            Self::TruePeak => Described {
                category: Category::Processing,
                page: None,
                title: words::settings_find::true_peak(),
                hint: words::settings_find::true_peak_hint(),
                also_called: words::settings_find::clipping_intersample_over_limiter_dbtp_headroom(
                ),
                keys: &[ConfigKey::TruePeak],
            },
            Self::LossySources => Described {
                category: Category::Processing,
                page: None,
                title: words::settings_find::artificially_enhance_lossy_files(),
                hint: words::settings_find::lossy_sources_hint(),
                also_called: words::settings_find::lossy_sources_mp3_aac_vorbis_restore(),
                keys: &[ConfigKey::RestoreLossy],
            },
            Self::Crossfeed => Described {
                category: Category::Processing,
                page: None,
                title: words::settings_find::crossfeed(),
                hint: words::settings_find::crossfeed_hint(),
                also_called: words::settings_find::crossfeed_bauer_headphones_stereo_image_binaural(
                ),
                keys: &[ConfigKey::Crossfeed],
            },
            Self::Equalising => Described {
                category: Category::Equaliser,
                page: None,
                title: words::settings_find::equaliser(),
                hint: words::settings_find::equalising_hint(),
                also_called:
                    words::settings_find::eq_equalizer_parametric_filter_correction_headphone(),
                keys: &[ConfigKey::Equaliser],
            },
            Self::BoundTo => Described {
                category: Category::Equaliser,
                page: None,
                title: words::settings_find::bound_to_each_device(),
                hint: words::settings_find::bound_to_hint(),
                also_called: words::settings_find::sink_device_per_device_default_binding(),
                keys: &[ConfigKey::EqualiserFor],
            },
            Self::Bands => Described {
                category: Category::Equaliser,
                page: None,
                title: words::settings_find::bands(),
                hint: words::settings_find::bands_hint(),
                also_called: words::settings_find::curve_response_graph_frequency_gain_q(),
                keys: &[],
            },
            Self::AutoEq => Described {
                category: Category::Equaliser,
                page: Some(Page::HeadphoneCorrection),
                title: "AutoEq",
                hint: words::settings_find::autoeq_hint(),
                also_called:
                    words::settings_find::autoeq_oratory1990_crinacle_harman_target_download(),
                keys: &[],
            },
            Self::Measurers => Described {
                category: Category::Equaliser,
                page: Some(Page::HeadphoneCorrection),
                title: words::settings_find::preferred_measurers(),
                hint: words::settings_find::measurers_hint(),
                also_called: words::settings_find::measurer_oratory1990_crinacle_rtings_rig_ranking(
                ),
                keys: &[
                    ConfigKey::EqualiserMeasurer,
                    ConfigKey::EqualiserMeasurerFor,
                ],
            },
            Self::OwnMeasurement => Described {
                category: Category::Equaliser,
                page: Some(Page::OwnMeasurement),
                title: words::settings_find::fit_a_measurement(),
                hint: words::settings_find::own_measurement_hint(),
                also_called: words::settings_find::own_measurement_rew_fit_csv_response(),
                keys: &[],
            },
            Self::RoomMeasurement => Described {
                category: Category::Equaliser,
                page: Some(Page::RoomMeasurement),
                title: words::settings_find::measure_your_room(),
                hint: words::settings_find::room_measurement_hint(),
                also_called: words::settings_find::microphone_mic_sweep_room_measure_calibration(),
                keys: &[],
            },
            Self::RoomCorrection => Described {
                category: Category::Equaliser,
                page: Some(Page::RoomCorrection),
                title: words::settings_find::the_rooms_response(),
                hint: words::settings_find::room_hint(),
                also_called: words::settings_find::convolution_impulse_response_ir_fir_room(),
                keys: &[ConfigKey::Convolution],
            },
            Self::Speakers => Described {
                category: Category::Equaliser,
                page: Some(Page::RoomCorrection),
                title: words::settings_find::where_the_speakers_stand(),
                hint: words::settings_find::speakers_hint(),
                also_called: words::settings_find::sofa_hrir_hrtf_angle_degrees_speaker(),
                keys: &[
                    ConfigKey::SpeakerAngle,
                    ConfigKey::SpeakerElevation,
                    ConfigKey::HeadYaw,
                ],
            },
            Self::Abx => Described {
                category: Category::Equaliser,
                page: Some(Page::ListeningTest),
                title: words::settings_find::a_blind_test(),
                hint: words::settings_find::abx_hint(),
                also_called: words::settings_find::abx_blind_test_listening_level_matched(),
                keys: &[],
            },
            Self::Resuming => Described {
                category: Category::Playback,
                page: None,
                title: words::settings_find::resuming(),
                hint: words::settings_find::resuming_hint(),
                also_called: words::settings_find::queue_restart_restore_carry_on_position(),
                keys: &[ConfigKey::Resume],
            },
            Self::Shuffling => Described {
                category: Category::Playback,
                page: None,
                title: words::settings_find::shuffle(),
                hint: words::settings_find::shuffling_hint(),
                also_called: words::settings_find::shuffle_smart_random_spread_artist_album(),
                keys: &[
                    ConfigKey::ShuffleStyle,
                    ConfigKey::ShuffleSpreadsArtists,
                    ConfigKey::ShuffleSpreadsAlbums,
                    ConfigKey::ShuffleFreshFirst,
                ],
            },
            Self::Repeating => Described {
                category: Category::Playback,
                page: None,
                title: words::settings_find::repeating_a_track(),
                hint: words::settings_find::repeating_hint(),
                also_called: words::settings_find::loop_repeat_one_single_track_skip(),
                keys: &[ConfigKey::SkipRepeatsQueue],
            },
            Self::PreviousButton => Described {
                category: Category::Playback,
                page: None,
                title: words::settings_find::the_previous_button(),
                hint: words::settings_find::previous_hint(),
                also_called: words::settings_find::restart_replay_beginning_start_over_back(),
                keys: &[ConfigKey::PreviousRestarts],
            },
            Self::Folders => Described {
                category: Category::Library,
                page: None,
                title: words::settings_find::music_folders(),
                hint: words::settings_find::folders_hint(),
                also_called: words::settings_find::roots_directories_add_scan_music_path(),
                keys: &[ConfigKey::MusicFolder, ConfigKey::FileDropped],
            },
            Self::Rescanning => Described {
                category: Category::Library,
                page: Some(Page::Enrichment),
                title: words::settings_find::the_files(),
                hint: words::settings_find::rescanning_hint(),
                also_called: words::settings_find::rescan_scan_stop_reindex_re_read(),
                keys: &[],
            },
            Self::LookingUp => Described {
                category: Category::Library,
                page: Some(Page::Enrichment),
                title: words::settings_find::what_online_knows(),
                hint: words::settings_find::looking_up_hint(),
                also_called: words::settings_find::enrich_look_up_refresh_all_force(),
                keys: &[],
            },
            Self::Retagging => Described {
                category: Category::Library,
                page: Some(Page::Enrichment),
                title: words::settings_find::the_tags_in_the_files(),
                hint: words::settings_find::retagging_hint(),
                also_called: words::settings_find::write_back_retag_tags_metadata_ids(),
                keys: &[],
            },
            Self::Identical => Described {
                category: Category::Library,
                page: Some(Page::Identical),
                title: words::settings_identical::identical_songs(),
                hint: words::settings_identical::hint(),
                also_called: words::settings_identical::also_called(),
                keys: &[],
            },
            Self::Organising => Described {
                category: Category::Library,
                page: None,
                title: words::settings_find::organising(),
                hint: words::settings_find::organising_hint(),
                also_called: words::settings_find::tidy_rename_move_layout_template_pattern(),
                keys: &[ConfigKey::OrganiseAs],
            },
            Self::Vault => Described {
                category: Category::Library,
                page: None,
                title: words::settings_find::the_vault(),
                hint: words::settings_find::vault_hint(),
                also_called: words::settings_find::archive_import_re_encode_recompress_flac(),
                keys: &[],
            },
            Self::Inbox => Described {
                category: Category::Library,
                page: None,
                title: words::settings_find::the_inbox(),
                hint: words::settings_find::inbox_hint(),
                also_called: words::settings_find::provider_poll_wants_missing_download_drop(),
                keys: &[ConfigKey::Inbox],
            },
            Self::History => Described {
                category: Category::Library,
                page: None,
                title: words::settings_find::listening_history(),
                hint: words::settings_find::history_hint(),
                also_called: words::settings_find::history_listens_plays_statistics_forget_age(),
                keys: &[ConfigKey::HistoryKept],
            },
            Self::MusicExtensions => Described {
                category: Category::Filters,
                page: None,
                title: words::settings_find::file_extensions(),
                hint: words::settings_find::choose_which_supported_file_extensions_appear(),
                also_called: words::settings_find::music_filters_formats_mp3_flac_m4a(),
                keys: &[ConfigKey::MusicExtensions],
            },
            Self::MinimumLength => Described {
                category: Category::Filters,
                page: None,
                title: words::settings_find::minimum_length(),
                hint: words::settings_find::only_show_tracks_at_least_this(),
                also_called: words::settings_find::music_filters_duration_seconds_minutes_short(),
                keys: &[ConfigKey::MinimumLength],
            },
            Self::Hidden => Described {
                category: Category::Filters,
                page: None,
                title: words::settings_find::hidden_songs(),
                hint: words::settings_find::the_songs_taken_out_of_the(),
                also_called: words::settings_find::hidden_hide_unhide_show_again_songs(),
                keys: &[],
            },
            Self::Lookups => Described {
                category: Category::Online,
                page: None,
                title: words::settings_find::reference_lookups(),
                hint: words::settings_find::online_hint(),
                also_called:
                    words::settings_find::musicbrainz_network_internet_covers_lyrics_lrclib(),
                keys: &[
                    ConfigKey::Online,
                    ConfigKey::MusicbrainzMirror,
                    ConfigKey::MusicbrainzReplicationToken,
                ],
            },
            Self::AfterScan => Described {
                category: Category::Online,
                page: None,
                title: words::settings_find::after_a_scan(),
                hint: words::settings_find::after_scan_hint(),
                also_called: words::settings_find::enrich_automatic_handover(),
                keys: &[ConfigKey::EnrichAfterScan],
            },
            Self::Metered => Described {
                category: Category::Online,
                page: None,
                title: words::settings_find::metered_connections(),
                hint: words::settings_find::metered_hint(),
                also_called: words::settings_find::metered_mobile_data_hotspot_tethering_bandwidth(
                ),
                keys: &[
                    ConfigKey::EnrichOnMetered,
                    ConfigKey::StallDownloadsOnMetered,
                ],
            },
            Self::Lyrics => Described {
                category: Category::Online,
                page: None,
                title: words::settings_find::fetching_lyrics(),
                hint: words::settings_find::lyrics_hint(),
                also_called: words::settings_find::lyrics_lrclib_better_lyrics_betterlyrics_ttml(),
                keys: &[ConfigKey::FetchLyrics, ConfigKey::LyricsLanguageFromLocale],
            },
            Self::Contact => Described {
                category: Category::Online,
                page: None,
                title: words::settings_find::contact(),
                hint: words::settings_find::contact_hint(),
                also_called: words::settings_find::user_agent_email_identity_request(),
                keys: &[ConfigKey::Contact],
            },
            Self::Studies => Described {
                category: Category::Recognition,
                page: None,
                title: words::settings_find::studying_tracks(),
                hint: words::settings_find::studies_hint(),
                also_called: words::settings_find::analyse_analysis_decode_fake_lossless_transcode(
                ),
                keys: &[ConfigKey::Study, ConfigKey::IdentifyBySound],
            },
            Self::Recognition => Described {
                category: Category::Recognition,
                page: None,
                title: words::settings_find::recognition(),
                hint: words::settings_find::recognition_hint(),
                also_called: words::settings_find::acoustid_audd_key_token_fingerprint_chromaprint(
                ),
                keys: &[ConfigKey::AcoustidKey, ConfigKey::AuddToken],
            },
            Self::Listening => Described {
                category: Category::Recognition,
                page: None,
                title: words::settings_find::listening(),
                hint: words::settings_find::listening_hint(),
                also_called: words::settings_find::listen_shazam_microphone_mic_desktop_monitor(),
                keys: &[ConfigKey::ListenFrom, ConfigKey::ListenFor],
            },
            Self::ResonateKey => Described {
                category: Category::Accounts,
                page: None,
                title: words::settings_find::resonate_server_key(),
                hint: words::settings_find::resonate_key_hint(),
                also_called: words::settings_find::resonate_server_key_secret_hifi_api(),
                keys: &[ConfigKey::HifiSecret],
            },
            Self::Subsonic => Described {
                category: Category::Accounts,
                page: None,
                title: words::settings_find::a_subsonic_server(),
                hint: words::settings_find::subsonic_hint(),
                also_called:
                    words::settings_find::navidrome_airsonic_gonic_server_provider_download(),
                keys: &[
                    ConfigKey::Subsonic,
                    ConfigKey::SubsonicUser,
                    ConfigKey::SubsonicPassword,
                ],
            },
            Self::Jellyfin => Described {
                category: Category::Accounts,
                page: None,
                title: words::settings_find::a_jellyfin_or_emby_server(),
                hint: words::settings_find::jellyfin_hint(),
                also_called: words::settings_find::emby_server_provider_stream_play_in(),
                keys: &[
                    ConfigKey::Jellyfin,
                    ConfigKey::JellyfinUser,
                    ConfigKey::JellyfinPassword,
                ],
            },
            Self::Tidal => Described {
                category: Category::Accounts,
                page: None,
                title: words::settings_find::a_tidal_account(),
                hint: words::settings_find::tidal_hint(),
                also_called:
                    words::settings_find::tidal_streaming_service_subscription_provider_download(),
                keys: &[
                    ConfigKey::TidalClientId,
                    ConfigKey::TidalClientSecret,
                    ConfigKey::TidalRefreshToken,
                    ConfigKey::HifiApi,
                    ConfigKey::Monochrome,
                    ConfigKey::HostedServices,
                ],
            },
            Self::YoutubeMusic => Described {
                category: Category::Accounts,
                page: None,
                title: "YouTube Music",
                hint: words::settings_find::youtube_music_hint(),
                also_called: words::settings_find::youtube_music_ytmusic_yt_dlp_ytdlp(),
                keys: &[
                    ConfigKey::YoutubeMusic,
                    ConfigKey::YoutubeMusicCookies,
                    ConfigKey::YtDlp,
                    ConfigKey::YoutubeMusicKeepOrder,
                ],
            },
            Self::Spotify => Described {
                category: Category::Accounts,
                page: None,
                title: words::settings_find::a_spotify_account(),
                hint: words::settings_find::spotify_hint(),
                also_called: words::settings_find::spotify_account_sign_in_playlists_liked(),
                keys: &[ConfigKey::SpotifyClientId, ConfigKey::SpotifyRefreshToken],
            },
            Self::Lastfm => Described {
                category: Category::Accounts,
                page: None,
                title: "Last.fm",
                hint: words::settings_find::lastfm_hint(),
                also_called:
                    words::settings_find::scrobble_scrobbling_last_fm_lastfm_audioscrobbler(),
                keys: &[
                    ConfigKey::LastfmKey,
                    ConfigKey::LastfmSecret,
                    ConfigKey::LastfmSession,
                ],
            },
            Self::Submitting => Described {
                category: Category::Accounts,
                page: None,
                title: "ListenBrainz",
                hint: words::settings_find::submitting_hint(),
                also_called: words::settings_find::scrobble_scrobbling_listens_history_submit_plays(
                ),
                keys: &[ConfigKey::ListenbrainzToken],
            },
            Self::Syncing => Described {
                category: Category::Sync,
                page: None,
                title: words::settings_find::device_sync(),
                hint: words::settings_find::syncing_hint(),
                also_called:
                    words::settings_find::sync_synchronise_synchronize_devices_playlists_favourites(
                    ),
                keys: &[ConfigKey::Sync],
            },
            Self::Pairing => Described {
                category: Category::Sync,
                page: None,
                title: words::settings_find::pairing(),
                hint: words::settings_find::pairing_hint(),
                also_called: words::settings_find::pair_pairing_code_qr_scan_camera(),
                keys: &[],
            },
            Self::SyncDevices => Described {
                category: Category::Sync,
                page: Some(Page::Devices),
                title: words::settings_find::devices(),
                hint: words::settings_find::sync_devices_hint(),
                also_called: words::settings_find::devices_phone_iphone_android_computer_remove(),
                keys: &[],
            },
            Self::SyncControl => Described {
                category: Category::Sync,
                page: Some(Page::Devices),
                title: resonate_core::remote_trust_title(),
                hint: words::settings_find::sync_control_hint(),
                also_called: words::settings_find::drive_driven_remote_control_another_device(),
                keys: &[ConfigKey::SyncControl],
            },
            Self::SyncConnection => Described {
                category: Category::Sync,
                page: None,
                title: words::settings_find::connection(),
                hint: words::settings_find::sync_connection_hint(),
                also_called: words::settings_find::server_self_hosted_own_nearby_network(),
                keys: &[
                    ConfigKey::SyncNearbyOnly,
                    ConfigKey::SyncServer,
                    ConfigKey::SyncDevice,
                    ConfigKey::SharedName,
                ],
            },
            Self::SyncedSongs => Described {
                category: Category::Sync,
                page: None,
                title: words::settings_find::songs_between_devices(),
                hint: words::settings_find::synced_songs_hint(),
                also_called: words::settings_find::sync_copy_transfer_download_songs_files(),
                keys: &[ConfigKey::SyncSongs],
            },
            Self::Notifications => Described {
                category: Category::Desktop,
                page: None,
                title: words::settings_find::notifications(),
                hint: words::settings_find::notifications_hint(),
                also_called: words::settings_find::notify_popup_toast_banner_song_track(),
                keys: &[ConfigKey::Notify],
            },
            Self::Discord => Described {
                category: Category::Desktop,
                page: None,
                title: "Discord",
                hint: words::settings_find::discord_hint(),
                also_called: words::settings_find::rich_presence_status_activity_listening_profile(
                ),
                keys: &[ConfigKey::Discord, ConfigKey::DiscordApp],
            },
            Self::DiscordShows => Described {
                category: Category::Desktop,
                page: None,
                title: words::settings_find::what_discord_shows(),
                hint: words::settings_find::discord_shows_hint(),
                also_called: words::settings_find::rich_presence_status_activity_cover_art(),
                keys: &[
                    ConfigKey::DiscordShows,
                    ConfigKey::DiscordArt,
                    ConfigKey::DiscordIcon,
                    ConfigKey::DiscordProgress,
                    ConfigKey::DiscordPaused,
                ],
            },
            Self::Colour => Described {
                category: Category::Appearance,
                page: None,
                title: words::settings_find::colour(),
                hint: words::settings_find::colour_hint(),
                also_called: words::settings_find::theme_accent_palette_dark_light_day(),
                keys: &[
                    ConfigKey::Theme,
                    ConfigKey::Accent,
                    ConfigKey::FollowDesktopScheme,
                ],
            },
            Self::Layout => Described {
                category: Category::Appearance,
                page: None,
                title: words::settings_find::layout(),
                hint: words::settings_find::layout_hint(),
                also_called: words::settings_find::text_size_type_scale_larger_largest(),
                keys: &[ConfigKey::TextSize, ConfigKey::FollowDesktopTextSize],
            },
            Self::Language => Described {
                category: Category::Appearance,
                page: None,
                title: words::settings_find::language(),
                hint: words::settings_find::language_hint(),
                also_called:
                    words::settings_find::language_locale_translation_translated_words_english(),
                keys: &[ConfigKey::Language],
            },
            Self::Motion => Described {
                category: Category::Appearance,
                page: None,
                title: words::settings_find::motion(),
                hint: words::settings_find::motion_hint(),
                also_called: words::settings_find::animation_animations_reduced_reduce_motion_fade(
                ),
                keys: &[ConfigKey::ReducedMotion],
            },
            Self::Scrollbars => Described {
                category: Category::Appearance,
                page: None,
                title: words::settings_find::scrollbars(),
                hint: words::settings_find::scrollbars_hint(),
                also_called: words::settings_find::scroll_bar_thumb_track_overlay_lists(),
                keys: &[ConfigKey::Scrollbars],
            },
            Self::LiveSpectrum => Described {
                category: Category::Appearance,
                page: None,
                title: words::settings_find::live_spectrum(),
                hint: words::settings_find::live_spectrum_hint(),
                also_called: words::settings_find::visualiser_analyser_analyzer_bars_tilt_slope(),
                keys: &[
                    ConfigKey::SpectrumTilt,
                    ConfigKey::SpectrumFloor,
                    ConfigKey::SpectrumBands,
                    ConfigKey::SpectrumFalls,
                ],
            },
            Self::WindowButtons => Described {
                category: Category::Window,
                page: None,
                title: words::settings_find::window_buttons(),
                hint: words::settings_find::window_buttons_hint(),
                also_called:
                    words::settings_find::titlebar_minimize_maximize_minimise_maximise_zoom(),
                keys: &[ConfigKey::MinimiseButton, ConfigKey::MaximiseButton],
            },
            Self::Tabs => Described {
                category: Category::Window,
                page: None,
                title: words::settings_find::sidebar_tabs(),
                hint: words::settings_find::tabs_hint(),
                also_called: words::settings_find::sidebar_panes_suggestions_missing_wanted_hide(),
                keys: &[
                    ConfigKey::SuggestionsTab,
                    ConfigKey::MissingTab,
                    ConfigKey::TabCounts,
                ],
            },
            Self::WindowState => Described {
                category: Category::Window,
                page: None,
                title: words::settings_find::window_state(),
                hint: words::settings_find::window_state_hint(),
                also_called: words::settings_find::remember_restore_last_tab_pane_window(),
                keys: &[
                    ConfigKey::RememberTab,
                    ConfigKey::RememberWindowSize,
                    ConfigKey::RememberSettingsCategory,
                ],
            },
            Self::VolumeWheel => Described {
                category: Category::Window,
                page: None,
                title: words::settings_find::the_volume_wheel(),
                hint: words::settings_find::volume_wheel_hint(),
                also_called: words::settings_find::scroll_mouse_wheel_touchpad_volume_slider(),
                keys: &[ConfigKey::ScrollVolume],
            },
            Self::MouseNavigation => Described {
                category: Category::Window,
                page: None,
                title: words::settings_find::mouse_navigation(),
                hint: words::settings_find::mouse_navigation_hint(),
                also_called: words::settings_find::mouse_side_buttons_back_forward_browser(),
                keys: &[ConfigKey::MouseNavigation],
            },
            Self::Build => Described {
                category: Category::About,
                page: None,
                title: words::settings_find::this_build(),
                hint: words::settings_find::build_hint(),
                also_called: words::settings_find::version_typeface_font_features_sinks(),
                keys: &[],
            },
            Self::Updates => Described {
                category: Category::About,
                page: None,
                title: words::settings_find::updates(),
                hint: words::settings_find::updates_hint(),
                also_called: words::settings_find::update_upgrade_new_version_release_newer(),
                keys: &[ConfigKey::CheckForUpdates, ConfigKey::UpdateDismissed],
            },
            Self::Places => Described {
                category: Category::About,
                page: None,
                title: words::settings_find::where_things_are_kept(),
                hint: words::settings_find::places_hint(),
                also_called: words::settings_find::config_toml_library_database_path_xdg(),
                keys: &[],
            },
            Self::Everything => Described {
                category: Category::About,
                page: None,
                title: words::settings_find::start_again(),
                hint: words::settings_find::everything_hint(),
                also_called: words::settings_find::reset_defaults_factory_put_back(),
                keys: &[],
            },
        }
    }

    fn matches(self, word: &str) -> bool {
        self.read_as(word).is_some()
    }

    fn read_as(self, word: &str) -> Option<AnsweredBy> {
        let reads = |text: &str| text.to_lowercase().contains(word);

        if reads(self.title()) {
            Some(AnsweredBy::Title)
        } else if reads(self.also_called()) {
            Some(AnsweredBy::AnotherName)
        } else if reads(self.category().label())
            || self.page().is_some_and(|page| reads(page.title()))
        {
            Some(AnsweredBy::Place)
        } else if reads(self.hint()) {
            Some(AnsweredBy::Hint)
        } else {
            None
        }
    }
}

type Ranked = (
    Reverse<Option<AnsweredBy>>,
    u8,
    Reverse<Option<AnsweredBy>>,
    Option<usize>,
    Reverse<AnsweredBy>,
    Group,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum AnsweredBy {
    Hint,
    Place,
    AnotherName,
    Title,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Narrowing {
    words: Vec<String>,
}

impl Narrowing {
    pub(crate) fn of(query: &str) -> Self {
        Self {
            words: query
                .split_whitespace()
                .map(str::to_lowercase)
                .filter(|word| !word.is_empty())
                .collect(),
        }
    }

    pub(crate) fn narrows(&self) -> bool {
        !self.words.is_empty()
    }

    pub(crate) fn allows(&self, group: Group) -> bool {
        self.words.iter().all(|word| group.matches(word))
    }

    pub(crate) fn found(&self, category: Category) -> usize {
        category
            .groups()
            .filter(|group| self.allows(*group))
            .count()
    }

    pub(crate) fn anywhere(&self) -> impl Iterator<Item = Group> + '_ {
        let found: Vec<(Group, AnsweredBy)> = Group::ALL
            .into_iter()
            .filter(|group| group.is_offered())
            .filter_map(|group| self.reading_of(group).map(|reading| (group, reading)))
            .collect();
        let best_where = |held: &dyn Fn(Group) -> bool| {
            found
                .iter()
                .filter(|(group, _)| held(*group))
                .map(|(_, reading)| *reading)
                .max()
        };
        let mut ranked: Vec<Ranked> = found
            .iter()
            .map(|(group, reading)| {
                let category = group.category();
                let page = group.page();
                (
                    Reverse(best_where(&|other| other.category() == category)),
                    category as u8,
                    Reverse(best_where(&|other| {
                        other.category() == category && other.page() == page
                    })),
                    page.map(|page| page as usize),
                    Reverse(*reading),
                    *group,
                )
            })
            .collect();
        ranked.sort_by_key(|(best, category, best_on_the_page, page, reading, _)| {
            (*best, *category, *best_on_the_page, *page, *reading)
        });
        ranked.into_iter().map(|(.., group)| group)
    }

    fn reading_of(&self, group: Group) -> Option<AnsweredBy> {
        self.words
            .iter()
            .map(|word| group.read_as(word))
            .try_fold(AnsweredBy::Title, |weakest, reading| {
                reading.map(|reading| weakest.min(reading))
            })
    }
}

fn sample_rate_hint() -> &'static str {
    static HINT: OnceLock<String> = OnceLock::new();

    HINT.get_or_init(|| words::settings_find::sample_rate_hint(sound_system!()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_that_says_nothing_on_a_system_is_not_offered_there() {
        for group in [Group::GraphRate, Group::Bluetooth] {
            assert!(group.is_offered_where(true, false));
            assert!(!group.is_offered_where(false, false));
            assert!(!group.is_offered_where(false, true));
        }
        for group in [Group::Notifications, Group::Metered, Group::WindowButtons] {
            assert!(group.is_offered_where(true, false));
            assert!(group.is_offered_where(false, false));
            assert!(!group.is_offered_where(false, true));
        }
        for group in [Group::Device, Group::Colour, Group::Discord] {
            assert!(group.is_offered_where(false, true));
        }
    }

    #[test]
    fn every_group_stands_in_exactly_one_category_and_every_category_holds_one() {
        for category in Category::ALL {
            assert!(
                category.groups().count() > 0,
                "{} is a category with nothing in it",
                category.label()
            );
        }
        assert_eq!(
            Category::ALL
                .into_iter()
                .map(|category| category.groups().count())
                .sum::<usize>(),
            Group::ALL
                .into_iter()
                .filter(|group| group.is_offered())
                .count()
        );
    }

    #[test]
    fn the_metered_switches_sit_in_online_and_the_renderers_flac_beside_the_device() {
        assert_eq!(Group::Metered.category(), Category::Online);
        assert_eq!(Group::FlacRenderers.category(), Category::Output);
        assert_eq!(
            Group::Metered.keys(),
            &[
                ConfigKey::EnrichOnMetered,
                ConfigKey::StallDownloadsOnMetered
            ]
        );
        assert_eq!(Group::FlacRenderers.keys(), &[ConfigKey::FlacAbove96kFor]);
    }

    #[test]
    fn no_two_groups_share_a_title_and_none_is_written_without_a_hint() {
        for group in Group::ALL {
            let sharing = Group::ALL
                .into_iter()
                .filter(|other| other.title() == group.title())
                .count();
            assert_eq!(
                sharing,
                1,
                "{} is titled as another group is",
                group.title()
            );
            assert!(
                group.hint().len() > 40,
                "{} explains itself in {} characters",
                group.title(),
                group.hint().len()
            );
        }
    }

    #[test]
    fn a_setting_is_written_under_one_group_and_no_key_is_claimed_twice() {
        for group in Group::ALL {
            for key in group.keys() {
                let claiming = Group::ALL
                    .into_iter()
                    .filter(|other| other.keys().contains(key))
                    .count();
                assert_eq!(
                    claiming,
                    1,
                    "{:?} is put back by {} and by another group as well",
                    key,
                    group.title()
                );
            }
        }
    }

    #[test]
    fn an_empty_query_narrows_nothing_and_allows_every_group() {
        let narrowing = Narrowing::of("   ");

        assert!(!narrowing.narrows());
        for group in Group::ALL {
            assert!(narrowing.allows(group));
        }
        assert_eq!(
            narrowing.anywhere().count(),
            Group::ALL
                .into_iter()
                .filter(|group| group.is_offered())
                .count()
        );
    }

    #[test]
    fn a_setting_named_by_the_words_leads_one_whose_hint_merely_mentions_them() {
        for word in [
            "dither", "volume", "lyrics", "cover", "buffer", "discord", "accent",
        ] {
            let found: Vec<Group> = Narrowing::of(word).anywhere().collect();
            let titled = found
                .iter()
                .any(|group| group.title().to_lowercase().contains(word));
            if titled {
                assert!(
                    found[0].title().to_lowercase().contains(word),
                    "{word:?} led with {:?} ahead of a setting called by it",
                    found[0].title()
                );
            }
            let mut seen = Vec::new();
            for group in &found {
                let category = group.category();
                if seen.last() != Some(&category) {
                    assert!(
                        !seen.contains(&category),
                        "{word:?} drew {category:?} under two headings"
                    );
                    seen.push(category);
                }
            }
        }
    }

    #[test]
    fn a_query_reads_a_group_by_its_title_however_it_is_cased() {
        let narrowing = Narrowing::of("DITHER");

        assert!(narrowing.narrows());
        assert!(narrowing.allows(Group::Dither));
        assert!(
            narrowing.allows(Group::NoiseShaping),
            "the shaping hint names dither"
        );
        assert!(!narrowing.allows(Group::Colour));
    }

    #[test]
    fn a_group_is_found_by_a_word_nothing_on_screen_writes() {
        assert!(Narrowing::of("sink").allows(Group::Device));
        assert!(Narrowing::of("latency").allows(Group::Buffer));
        assert!(Narrowing::of("sacd").allows(Group::Dop));
        assert!(Narrowing::of("musicbrainz").allows(Group::Lookups));
        assert!(Narrowing::of("font").allows(Group::Build));
        assert!(Narrowing::of("maximize").allows(Group::WindowButtons));
        assert!(Narrowing::of("titlebar").allows(Group::WindowButtons));
    }

    #[test]
    fn every_word_has_to_land_for_a_group_to_be_allowed() {
        assert!(Narrowing::of("dither noise").allows(Group::Dither));
        assert!(!Narrowing::of("dither folders").allows(Group::Dither));
    }

    #[test]
    fn a_query_is_counted_against_every_category_rather_than_the_one_in_front() {
        let narrowing = Narrowing::of("rate");

        assert!(narrowing.found(Category::Output) >= 2);
        assert_eq!(narrowing.found(Category::Appearance), 0);
        assert_eq!(
            narrowing.anywhere().count(),
            Category::ALL
                .into_iter()
                .map(|category| narrowing.found(category))
                .sum::<usize>()
        );
    }

    #[test]
    fn every_page_holds_a_group_of_its_own_category_and_no_two_pages_share_a_name() {
        for page in Page::ALL {
            assert!(
                page.groups().next().is_some(),
                "{} is a page with nothing on it",
                page.title()
            );
            for group in page.groups() {
                assert_eq!(
                    group.category(),
                    page.category(),
                    "{} sits on {} under another category",
                    group.title(),
                    page.title()
                );
            }
            let sharing = Page::ALL
                .into_iter()
                .filter(|other| other.title() == page.title() || other.id() == page.id())
                .count();
            assert_eq!(sharing, 1, "{} is named as another page is", page.title());
        }
    }

    #[test]
    fn a_category_draws_its_own_groups_and_gathers_its_pages_where_the_first_of_them_stands() {
        assert_eq!(
            Category::Equaliser.drawn(),
            vec![
                Drawn::Group(Group::Equalising),
                Drawn::Group(Group::BoundTo),
                Drawn::Group(Group::Bands),
                Drawn::Pages(vec![
                    Page::HeadphoneCorrection,
                    Page::OwnMeasurement,
                    Page::RoomMeasurement,
                    Page::RoomCorrection,
                    Page::ListeningTest,
                ]),
            ]
        );
        assert_eq!(
            Category::Library.drawn(),
            vec![
                Drawn::Group(Group::Folders),
                Drawn::Pages(vec![Page::Enrichment, Page::Identical]),
                Drawn::Group(Group::Organising),
                Drawn::Group(Group::Vault),
                Drawn::Group(Group::Inbox),
                Drawn::Group(Group::History),
            ]
        );
        assert_eq!(
            Category::Sync.drawn(),
            vec![
                Drawn::Group(Group::Syncing),
                Drawn::Group(Group::Pairing),
                Drawn::Pages(vec![Page::Devices]),
                Drawn::Group(Group::SyncConnection),
                Drawn::Group(Group::SyncedSongs),
            ]
        );

        for category in Category::ALL {
            let mut shown: Vec<Group> = category.shown_on(None).collect();
            for drawn in category.drawn() {
                if let Drawn::Pages(pages) = drawn {
                    for page in pages {
                        shown.extend(category.shown_on(Some(page)));
                    }
                }
            }
            shown.sort_by_key(|group| *group as usize);
            assert_eq!(
                shown,
                category.groups().collect::<Vec<_>>(),
                "{} draws a group twice or leaves one out",
                category.label()
            );
        }
    }

    #[test]
    fn a_setting_on_a_page_of_its_own_is_found_by_its_name_and_by_the_pages() {
        assert_eq!(
            Narrowing::of("autoeq").anywhere().next(),
            Some(Group::AutoEq)
        );
        assert!(Narrowing::of("your devices").allows(Group::SyncDevices));
        assert!(Narrowing::of("room correction").allows(Group::Speakers));
        assert!(Narrowing::of("rescan").allows(Group::Rescanning));
        assert_eq!(
            Narrowing::of("listening test").anywhere().next(),
            Some(Group::Abx)
        );
    }

    #[test]
    fn listenbrainz_and_last_fm_are_both_accounts_and_both_answer_to_scrobbling() {
        let found: Vec<Group> = Narrowing::of("scrobble").anywhere().collect();

        assert!(found.contains(&Group::Lastfm));
        assert!(found.contains(&Group::Submitting));
        assert_eq!(Group::Lastfm.category(), Category::Accounts);
        assert_eq!(Group::Submitting.category(), Category::Accounts);
    }

    #[test]
    fn every_category_is_drawn_with_a_mark_of_its_own() {
        for category in Category::ALL {
            let sharing = Category::ALL
                .into_iter()
                .filter(|other| other.icon() == category.icon())
                .count();
            assert_eq!(
                sharing,
                1,
                "{} shares its mark with another category",
                category.label()
            );
        }
    }

    #[test]
    fn a_query_nothing_answers_to_finds_nothing_anywhere() {
        let narrowing = Narrowing::of("chartreuse");

        assert_eq!(narrowing.anywhere().count(), 0);
        for category in Category::ALL {
            assert_eq!(narrowing.found(category), 0);
        }
    }
}
