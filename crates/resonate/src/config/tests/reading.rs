use super::*;

#[test]
fn the_settings_a_command_line_names_are_found_before_the_grammar_reads_it() {
    let apart = spoken(&["resonate", "scan", "--config", "/elsewhere.toml"]);
    let joined = spoken(&["resonate", "--config=/joined.toml", "play", "a.flac"]);
    let after_the_end = spoken(&["resonate", "play", "--", "--config", "/a-file.toml"]);
    let unnamed = spoken(&["resonate", "play", "a.flac"]);

    assert_eq!(
        named_on_the_command_line(&apart),
        Some(PathBuf::from("/elsewhere.toml"))
    );
    assert_eq!(
        named_on_the_command_line(&joined),
        Some(PathBuf::from("/joined.toml"))
    );
    assert_eq!(named_on_the_command_line(&after_the_end), None);
    assert_eq!(named_on_the_command_line(&unnamed), None);
}

#[test]
fn a_region_named_in_the_file_settles_its_figures_and_a_tag_naming_none_is_refused() {
    let german = read("region = \"de-DE\"").expect("a region");

    assert_eq!(
        german.region(),
        RegionChoice::parse("de-DE").expect("a tag")
    );

    resonate_words::settle(&german.region().conventions());

    assert_eq!(resonate_words::grouped(12_345), "12.345");
    assert_eq!(
        resonate_words::day_month_and_year(2026, 11, 13),
        "13 Nov 2026"
    );

    assert_eq!(read("").expect("no region").region(), RegionChoice::System);
    assert_eq!(
        read("region = \"system\"").expect("the system's").region(),
        RegionChoice::System
    );

    for refused in ["region = \"de\"", "region = \"klingon\"", "region = \"C\""] {
        assert!(
            matches!(
                read(refused),
                Err(Error::ConfigValue {
                    key: ConfigKey::Region,
                    ..
                })
            ),
            "{refused}"
        );
    }

    assert!(matches!(
        read("region = 49"),
        Err(Error::ConfigType {
            key: ConfigKey::Region,
            ..
        })
    ));
}

#[test]
fn the_language_a_named_settings_file_chooses_is_spoken_before_the_grammar() {
    let settings = Scratch::new().seed("language = \"en\"\n");
    let arguments = spoken(&[
        "resonate",
        "--config",
        settings.path.to_str().expect("a UTF-8 path"),
    ]);

    assert_eq!(spoken_before_the_grammar(&arguments), Language::English);
}

#[test]
fn a_setting_read_from_the_file_is_the_setting_the_pane_writes() {
    let folder = std::env::temp_dir().join(format!(
        "resonate-youtube-music-settings-{}",
        std::process::id()
    ));

    fs::create_dir_all(&folder).expect("a folder");

    let file = folder.join("config.toml");

    store(&file, ConfigKey::YoutubeMusic, true).expect("the switch stored");
    store(&file, ConfigKey::YoutubeMusicCookies, "firefox").expect("the cookies stored");
    store(&file, ConfigKey::YtDlp, "/opt/yt-dlp").expect("the program stored");
    store(&file, ConfigKey::YoutubeMusicKeepOrder, false).expect("the order stored");

    let text = fs::read_to_string(&file).expect("the file written");

    fs::remove_dir_all(&folder).expect("the folder removed");

    let config = read(&text).expect("a well formed document");

    assert!(text.contains("youtube-music = true"), "{text}");
    assert!(text.contains("youtube-music-keep-order = false"), "{text}");
    assert_eq!(config.switches.said(Switch::YoutubeMusic), Some(true));
    assert_eq!(config.youtube_music_cookies.as_deref(), Some("firefox"));
    assert_eq!(config.yt_dlp.as_deref(), Some("/opt/yt-dlp"));
    assert_eq!(
        config.switches.said(Switch::YoutubeMusicKeepOrder),
        Some(false)
    );
}

#[test]
fn youtube_music_is_off_and_keeps_youtubes_order_unless_the_file_says_otherwise() {
    let config = Config::default();

    assert!(!config.is_on(Switch::YoutubeMusic));
    assert!(config.is_on(Switch::YoutubeMusicKeepOrder));
}

#[test]
fn sync_reads_its_keys_and_is_off_wherever_online_is() {
    let config = read(
        "sync-server = \" sync.home.arpa:7443 \"\nsync-device = \"Desk\"\n\
         sync-server-certificate = \"/etc/resonate/sync.pem\"\n\
         sync-songs = [\"favourites\", \"playlists\"]\n",
    )
    .expect("a well formed document");

    assert!(config.syncs());
    assert_eq!(config.sync_server.as_deref(), Some("sync.home.arpa:7443"));
    assert_eq!(config.sync_device.as_deref(), Some("Desk"));
    assert_eq!(
        config.sync_server_certificate.as_deref(),
        Some(Path::new("/etc/resonate/sync.pem"))
    );

    let songs = config.sync_songs.expect("songs followed");

    assert!(songs.favourites && songs.playlists && !songs.everything);

    assert!(
        !read("online = false\nsync-nearby-only = false\n")
            .expect("read")
            .syncs()
    );
    assert!(!read("sync = false\n").expect("read").syncs());
    assert!(!config.is_on(Switch::SyncNearbyOnly));

    let hosted = read("online = false\nsync-nearby-only = false\n").expect("read");

    assert!(!hosted.syncs() && !hosted.is_on(Switch::SyncNearbyOnly));

    let defaulted = read("online = false\n").expect("read");

    assert!(!defaulted.syncs() && !defaulted.is_on(Switch::SyncNearbyOnly));

    let nearby = read("online = false\nsync-nearby-only = true\n").expect("read");

    assert!(nearby.syncs() && nearby.is_on(Switch::SyncNearbyOnly));
    assert!(
        !read("sync = false\nsync-nearby-only = true\n")
            .expect("read")
            .syncs()
    );
    assert!(read("sync-songs = [\"all\"]\n").is_err());
}

#[test]
fn a_party_asks_the_time_of_the_server_named_none_when_blank_and_none_while_offline() {
    let defaulted = read("").expect("read");
    let named = read("party-time-server = \" time.example.org:123 \"\n").expect("read");
    let blank = read("party-time-server = \"\"\n").expect("read");
    let offline = read("online = false\nparty-time-server = \"time.example.org\"\n").expect("read");

    assert_eq!(
        defaulted.party_time_server().as_deref(),
        Some(resonate_core::PARTY_TIME_SERVER)
    );
    assert_eq!(
        named.party_time_server().as_deref(),
        Some("time.example.org:123")
    );
    assert_eq!(blank.party_time_server(), None);
    assert_eq!(offline.party_time_server(), None);
}

#[test]
fn a_value_that_will_not_read_is_left_at_its_default_and_the_rest_are_read() {
    let mut refusals = Vec::new();

    let config = parse(
        Path::new("/config.toml"),
        "last-tab = \"a pane of another build\"\nquality = \"fast\"\n\
         window-size = \"wide\"\nonline = false\n",
        |error| refusals.push(error),
    )
    .expect("a file whose values will not all read still reads");

    assert_eq!(refusals.len(), 2, "{refusals:?}");
    assert_eq!(config.last_tab, None);
    assert_eq!(config.window_size, None);
    assert_eq!(config.quality, Some(Quality::LowLatency));
    assert_eq!(config.switches.said(Switch::Online), Some(false));
}

#[test]
fn an_empty_document_leaves_every_setting_unset() {
    assert_eq!(read("").expect("empty is valid"), Config::default());
}

#[test]
fn music_filters_keep_defaults_and_refuse_unsupported_extensions_or_lengths() {
    assert_eq!(
        read("").expect("defaults").music_filters(),
        MusicFilters::default()
    );

    let selected = read("music-extensions = [\".MP3\", \"flac\", \".m4a\"]\nminimum-length-s = 600\nlast-settings-category = \"filters\"").expect("valid filters");

    assert_eq!(
        selected
            .music_filters()
            .extensions
            .selected()
            .collect::<Vec<_>>(),
        ["flac", "m4a", "mp3"]
    );
    assert_eq!(selected.music_filters().minimum_length.seconds(), 600);

    for invalid in [
        "minimum-length-s = -1",
        "minimum-length-s = 601",
        "minimum-length-s = 1.5",
    ] {
        assert!(matches!(
            read(invalid),
            Err(Error::ConfigValue {
                key: ConfigKey::MinimumLength,
                ..
            }) | Err(Error::ConfigType {
                key: ConfigKey::MinimumLength,
                ..
            })
        ));
    }
    for invalid in [
        "music-extensions = [\".exe\"]",
        "music-extensions = [1]",
        "music-extensions = \"mp3\"",
    ] {
        assert!(matches!(
            read(invalid),
            Err(Error::ConfigValue {
                key: ConfigKey::MusicExtensions,
                ..
            }) | Err(Error::ConfigType {
                key: ConfigKey::MusicExtensions,
                ..
            })
        ));
    }

    assert_eq!(
        read("music-extensions = []")
            .expect("exclude all")
            .music_filters()
            .extensions
            .selected()
            .count(),
        0
    );
}

#[test]
fn a_blank_path_is_the_default_and_a_leading_tilde_is_the_home_folder() {
    let config = read(
        r#"
        library = ""
        vault = "  "
        music-folder = "~/Music"
        inbox = "~"
        convolution = "~elsewhere/room.wav"
        "#,
    )
    .expect("a well formed document");
    let home = env::var_os("HOME").map(PathBuf::from);

    assert_eq!(config.library, None);
    assert_eq!(config.vault, None);
    assert_eq!(
        config.convolution,
        Some(PathBuf::from("~elsewhere/room.wav"))
    );
    if let Some(home) = home {
        assert_eq!(config.music_folder, Some(home.join("Music")));
        assert_eq!(config.inbox, Some(home));
    }
}

#[test]
fn every_setting_reads_back_as_its_domain_type() {
    let config = read(
        r#"
        sink = "alsa_output.pci-0000_00_1f.3.analog-stereo"
        library = "/music/library.db"
        quality = "high"
        dither = "rectangular"
        noise-shaping = "lipshitz"
        replay-gain = "album"
        bit-perfect = false
        force-graph-rate = false
        volume = 0.25
        buffer-ms = 750
        theme = "catppuccin-mocha"
        accent = "teal"
        text-size = "large"
        dop = true
        "#,
    )
    .expect("a well formed document");

    assert_eq!(
        config.sink,
        Some(NodeName::new("alsa_output.pci-0000_00_1f.3.analog-stereo"))
    );
    assert_eq!(config.library, Some(PathBuf::from("/music/library.db")));
    assert_eq!(config.quality, Some(Quality::High));
    assert_eq!(config.dither, Some(DitherKind::Rectangular));
    assert_eq!(config.noise_shaping, Some(NoiseShaping::Lipshitz));
    assert_eq!(config.replay_gain, Some(ReplayGainMode::Album));
    assert_eq!(config.bit_perfect, Some(false));
    assert_eq!(config.force_graph_rate, Some(false));
    assert_eq!(config.volume, Volume::new(0.25).ok());
    assert_eq!(config.buffer, Some(Duration::from_millis(750)));
    assert_eq!(config.theme, Some(Theme::CatppuccinMocha));
    assert_eq!(config.accent, Some(Accent::Teal));
    assert_eq!(config.text_size, Some(TextSize::Large));
    assert_eq!(config.dop, Some(true));
    assert_eq!(
        config.appearance(),
        Appearance {
            theme: Theme::CatppuccinMocha,
            accent: Some(Accent::Teal),
            text_size: TextSize::Large,
            follows: Following {
                scheme: false,
                text: false,
            },
        }
    );
}

#[test]
fn every_noise_shaping_is_read_by_its_name() {
    for (text, shaping) in [
        ("flat", NoiseShaping::None),
        ("lipshitz", NoiseShaping::Lipshitz),
        ("threshold", NoiseShaping::Threshold),
    ] {
        let config = read(&format!("noise-shaping = \"{text}\"")).expect("a well formed document");

        assert_eq!(config.noise_shaping, Some(shaping), "{text}");
    }
}

#[test]
fn an_inbox_is_read_as_a_folder_and_a_blank_one_is_none() {
    let config = read("inbox = \"/music/inbox\"").expect("a well formed document");

    assert_eq!(config.inbox.as_deref(), Some(Path::new("/music/inbox")));

    let config = read("inbox = \"  \"").expect("a well formed document");

    assert_eq!(config.inbox, None);

    assert_eq!(Config::default().inbox, None);
}

#[test]
fn a_music_folder_is_read_as_a_folder_and_a_blank_one_is_none() {
    let config = read("music-folder = \"/music/library\"").expect("a well formed document");

    assert_eq!(
        config.music_folder.as_deref(),
        Some(Path::new("/music/library"))
    );

    let config = read("music-folder = \"  \"").expect("a well formed document");

    assert_eq!(config.music_folder, None);

    assert_eq!(Config::default().music_folder, None);
}

#[cfg(feature = "ui")]
#[test]
fn what_is_dropped_is_filed_unless_the_file_says_not() {
    let kept = read("file-dropped = false").expect("a well formed document");

    assert!(Config::default().is_on(Switch::FileDropped));
    assert!(!kept.is_on(Switch::FileDropped));
    assert!(read("file-dropped = \"no\"").is_err());
}

#[test]
fn where_and_how_long_to_listen_are_read() {
    let config =
        read("listen-from = \"microphone\"\nlisten-for = 20").expect("a well formed document");

    assert_eq!(config.listen_from, Some(Listening::Microphone(None)));
    assert_eq!(config.listen_for, Some(Duration::from_secs(20)));
    assert!(read("listen-for = 0").is_err());
}

#[test]
fn discord_is_shown_nothing_unless_it_is_switched_on_and_given_an_application() {
    assert!(!Config::default().presence().active());
    assert_eq!(Config::default().presence(), Presence::OFF);
    assert!(
        !read("discord = true")
            .expect("well formed")
            .presence()
            .active()
    );
    assert!(
        !read("discord-app = \"1234567890123456789\"")
            .expect("well formed")
            .presence()
            .active()
    );
}

#[test]
fn every_discord_setting_is_read_into_the_presence() {
    let config = read(
        "discord = true\n\
         discord-app = \" 1234567890123456789 \"\n\
         discord-shows = \"track\"\n\
         discord-art = \"icon\"\n\
         discord-icon = \"resonate\"\n\
         discord-progress = false\n\
         discord-paused = true\n",
    )
    .expect("a well formed document");

    assert_eq!(
        config.presence(),
        Presence {
            enabled: true,
            app: AppId::parse("1234567890123456789"),
            shown: Shown::Track,
            pictured: Pictured::Icon,
            icon: Icon::parse("resonate"),
            progress: false,
            while_paused: true,
        }
    );
}

#[test]
fn a_blank_discord_application_or_icon_is_none() {
    let config = read("discord-app = \"  \"\ndiscord-icon = \"\"").expect("well formed");

    assert_eq!(config.discord_app, None);
    assert_eq!(config.discord_icon, None);
}

#[test]
fn a_discord_application_that_is_not_a_snowflake_is_refused_by_name() {
    for written in [
        "discord-app = \"resonate\"",
        "discord-app = \"12345\"",
        "discord-shows = \"everything\"",
        "discord-art = \"portrait\"",
        "discord-icon = \"two words\"",
    ] {
        let error = read(written).expect_err("not a value the key takes");
        let named = written.split(' ').next().and_then(ConfigKey::parse);

        assert!(
            matches!(&error, Error::ConfigValue { key, .. } if Some(*key) == named),
            "{written}: {error:?}"
        );
    }
}

#[test]
fn the_renderers_vouched_for_are_the_upnp_nodes_named_and_nothing_is_vouched_by_default() {
    let config =
        read(r#"flac-above-96k-for = ["upnp:uuid:one", " upnp:uuid:two ", "", "alsa_output.dac"]"#)
            .expect("a well formed document");

    assert_eq!(
        config.renderers_vouched_for(),
        Vouched::flac_at_any_rate([Udn::new("uuid:one"), Udn::new("uuid:two")])
    );
    assert_eq!(
        Config::default().renderers_vouched_for(),
        Vouched::default()
    );

    let error = read("flac-above-96k-for = [7]").expect_err("a number is not a name");

    assert!(matches!(
        error,
        Error::ConfigType {
            key: ConfigKey::FlacAbove96kFor,
            ..
        }
    ));
}

#[test]
fn the_renderer_addresses_are_the_http_addresses_named_and_nothing_is_held_by_default() {
    let config = read(
        r#"renderers-at = ["http://192.168.1.20:49152/desc.xml", " http://[fe80::1]:80/d ", "", "https://secure/", "renderer"]"#,
    )
    .expect("a well formed document");

    assert_eq!(
        config
            .renderers_at()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [
            "http://192.168.1.20:49152/desc.xml",
            "http://[fe80::1]:80/d"
        ]
    );
    assert!(Config::default().renderers_at().is_empty());

    let error = read("renderers-at = [7]").expect_err("a number is not an address");

    assert!(matches!(
        error,
        Error::ConfigType {
            key: ConfigKey::RenderersAt,
            ..
        }
    ));
}

#[test]
fn the_cast_addresses_are_ip_addresses_and_host_names_with_the_cast_port_unless_given() {
    let config = read(
        r#"cast-at = ["192.168.1.30", " 192.168.1.31:8010 ", "[fe80::1]", "", "Living-Room.local", "speaker.lan:8011", "not an address", "speaker.lan:x"]"#,
    )
    .expect("a well formed document");

    let mut held = config
        .cast_at()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    held.sort();

    assert_eq!(
        held,
        [
            "192.168.1.30:8009",
            "192.168.1.31:8010",
            "[fe80::1]:8009",
            "living-room.local:8009",
            "speaker.lan:8011",
        ]
    );
    assert!(Config::default().cast_at().is_empty());

    let error = read("cast-at = [7]").expect_err("a number is not an address");

    assert!(matches!(
        error,
        Error::ConfigType {
            key: ConfigKey::CastAt,
            ..
        }
    ));
}

#[test]
fn the_name_the_catalog_is_offered_under_is_what_was_typed_and_none_when_blank() {
    let named = read("offer-name = \"  Study  \"").expect("a well formed document");

    assert_eq!(named.offer_name().map(ServerName::as_str), Some("Study"));
    assert_eq!(Config::default().offer_name(), None);
    assert_eq!(
        read("offer-name = \"   \"")
            .expect("a blank name")
            .offer_name(),
        None
    );

    let long = format!("offer-name = \"{}\"", "é".repeat(100));
    let cut = read(&long).expect("a long name");

    assert_eq!(
        cut.offer_name().map(|name| name.as_str().chars().count()),
        Some(63)
    );
    assert!(matches!(
        read("offer-name = 7"),
        Err(Error::ConfigType {
            key: ConfigKey::OfferName,
            ..
        })
    ));
}

#[cfg(feature = "ui")]
#[test]
fn a_lyric_sheet_is_chosen_by_the_locale_only_once_asked_to_be() {
    assert!(
        !read("")
            .expect("empty is valid")
            .is_on(Switch::LyricsLanguageFromLocale)
    );
    assert!(
        read("lyrics-language-from-locale = true")
            .expect("a well formed document")
            .is_on(Switch::LyricsLanguageFromLocale)
    );
}

#[test]
fn studies_lyrics_and_a_skip_under_repeat_default_on_and_are_turned_off_by_their_keys() {
    let config = read("").expect("empty is valid");

    assert!(config.is_on(Switch::Study));
    assert!(config.is_on(Switch::FetchLyrics));
    assert_eq!(config.skip_under_repeat(), SkipUnderRepeat::RepeatsTheQueue);

    let config = read("study = false\nskip-repeats-queue = false\nfetch-lyrics = false")
        .expect("a well formed document");

    assert!(!config.is_on(Switch::Study));
    assert!(!config.is_on(Switch::FetchLyrics));
    assert_eq!(
        config.skip_under_repeat(),
        SkipUnderRepeat::KeepsRepeatingTheTrack
    );
}

#[test]
fn a_smart_shuffle_spreading_everything_unless_the_file_says_otherwise() {
    let config = read("").expect("an empty document");

    assert_eq!(config.smart_shuffle(), SmartShuffle::default());

    let config = read(
        "shuffle-style = \"albums\"\nshuffle-spreads-artists = false\nshuffle-spreads-albums = false\nshuffle-fresh-first = false",
    )
    .expect("a well formed document");

    assert_eq!(
        config.smart_shuffle(),
        SmartShuffle {
            style: ShuffleStyle::Albums,
            spreads_artists: false,
            spreads_albums: false,
            fresh_first: false,
        }
    );

    let config = read("shuffle-style = \"random\"").expect("a well formed document");

    assert_eq!(config.smart_shuffle().style, ShuffleStyle::Random);
}

#[test]
fn previous_restarts_by_default_and_is_turned_off_by_its_key() {
    let config = read("").expect("empty is valid");

    assert_eq!(
        config.previous_restarts(),
        PreviousRestarts::RestartsTheTrack
    );

    let config = read("previous-restarts = false").expect("a well formed document");

    assert_eq!(config.previous_restarts(), PreviousRestarts::AlwaysGoesBack);

    let config = read("previous-restarts = true").expect("a well formed document");

    assert_eq!(
        config.previous_restarts(),
        PreviousRestarts::RestartsTheTrack
    );
}

#[test]
fn a_loaded_file_says_where_it_was_read_from() {
    let scratch = Scratch::new().seed("online = false\n");

    let loaded = load(Some(&scratch.path)).expect("it loads");

    assert_eq!(loaded.read_from.as_deref(), Some(scratch.path.as_path()));
    assert!(!loaded.is_on(Switch::Online));
}

#[test]
fn the_history_is_kept_forever_until_the_file_names_a_span_of_days() {
    let kept = |text: &str| read(text).expect("a span is valid").history_kept();

    assert_eq!(kept(""), HistoryKept::Forever);
    assert_eq!(kept("history-kept = \"forever\""), HistoryKept::Forever);
    assert_eq!(
        kept("history-kept = 365"),
        HistoryKept::for_days(365).expect("a year of days")
    );
    assert!(read("history-kept = 0").is_err());
    assert!(read("history-kept = \"a while\"").is_err());
}

#[test]
fn a_whole_number_is_accepted_where_a_fraction_is_expected() {
    assert_eq!(
        read("volume = 1").expect("1 is in range").volume,
        Some(Volume::MAX)
    );
}

#[test]
fn a_setting_of_the_wrong_type_names_the_key_and_the_type_it_wanted() {
    let error = read("quality = 3").expect_err("an integer is not a quality");

    assert!(
        matches!(
            error,
            Error::ConfigType {
                key: ConfigKey::Quality,
                expected: ValueKind::Text,
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn a_setting_outside_its_domain_names_the_key() {
    for text in [
        "quality = \"perfect\"",
        "volume = 1.5",
        "buffer-ms = -1",
        "bluetooth-lead-ms = 750000",
        "bluetooth-lead-ms = -1",
        "bluetooth-awake-s = 86400",
        "listen-for = 3",
    ] {
        let error = read(text).expect_err("outside the domain");

        assert!(
            matches!(error, Error::ConfigValue { .. }),
            "{text}: {error:?}"
        );
    }
}

#[test]
fn an_unknown_key_is_ignored_rather_than_refused() {
    let config = read("sink = \"auto_null\"\nvolume-boost = true").expect("unknown keys pass");

    assert_eq!(config.sink, Some(NodeName::new("auto_null")));
}

#[test]
fn a_file_kept_from_when_the_level_was_called_fast_reads_as_low_latency() {
    let scratch = Scratch::new().seed("quality = \"fast\"\n");

    assert_eq!(
        load(Some(&scratch.path)).expect("it reads").quality,
        Some(Quality::LowLatency)
    );
}

#[test]
fn the_low_latency_level_is_spelt_the_way_its_flag_is() {
    let scratch = Scratch::new().seed("quality = \"high\"\n");

    store(&scratch.path, ConfigKey::Quality, "low-latency").expect("a writable file");

    assert!(scratch.text().contains("quality = \"low-latency\""));
    assert_eq!(
        load(Some(&scratch.path)).expect("it reads back").quality,
        Some(Quality::LowLatency)
    );
}

#[test]
fn the_very_high_level_is_spelt_the_way_its_flag_is() {
    let scratch = Scratch::new().seed("quality = \"fast\"\n");

    store(&scratch.path, ConfigKey::Quality, "very-high").expect("a writable file");

    assert!(scratch.text().contains("quality = \"very-high\""));
    assert_eq!(
        load(Some(&scratch.path)).expect("it reads back").quality,
        Some(Quality::VeryHigh)
    );
}

#[test]
fn a_speaker_angle_is_read_in_whole_degrees_and_held_to_what_a_pair_of_speakers_can_be() {
    let config = read("speaker-angle = 45").expect("a known angle");

    assert_eq!(config.speaker_angle().degrees(), 45);
    assert_eq!(Config::default().speaker_angle(), SpeakerAngle::STANDARD);

    for refused in [
        "speaker-angle = 4",
        "speaker-angle = 91",
        "speaker-angle = -30",
        "speaker-angle = \"wide\"",
    ] {
        assert!(read(refused).is_err(), "{refused}");
    }
}

#[test]
fn a_speaker_elevation_is_read_in_whole_degrees_above_or_below_the_ears_and_held_to_a_range() {
    let high = read("speaker-elevation = 30").expect("a known elevation");
    let low = read("speaker-elevation = -30").expect("a known elevation");

    assert_eq!(high.speaker_elevation().degrees(), 30);
    assert_eq!(low.speaker_elevation().degrees(), -30);
    assert_eq!(Config::default().speaker_elevation().degrees(), 0);
    assert_eq!(high.speakers().angle, SpeakerAngle::STANDARD);

    for refused in [
        "speaker-elevation = 61",
        "speaker-elevation = -31",
        "speaker-elevation = 100000",
        "speaker-elevation = \"up\"",
    ] {
        assert!(read(refused).is_err(), "{refused}");
    }
}

#[test]
fn a_head_yaw_is_read_in_whole_degrees_either_way_and_held_to_a_quarter_turn() {
    let left = read("head-yaw = 90").expect("a known yaw");
    let right = read("head-yaw = -90").expect("a known yaw");

    assert_eq!(left.head_yaw().degrees(), 90);
    assert_eq!(right.head_yaw().degrees(), -90);
    assert_eq!(Config::default().head_yaw(), HeadYaw::AHEAD);
    assert_eq!(left.speakers().yaw, left.head_yaw());
    assert_eq!(left.speakers().angle, SpeakerAngle::STANDARD);

    for refused in [
        "head-yaw = 91",
        "head-yaw = -91",
        "head-yaw = 100000",
        "head-yaw = \"left\"",
    ] {
        assert!(read(refused).is_err(), "{refused}");
    }
}

#[test]
fn a_crossfeed_is_read_by_name() {
    let config = read("crossfeed = \"moderate\"").expect("a known crossfeed");

    assert_eq!(config.crossfeed, Some(Crossfeed::Moderate));
    assert!(read("crossfeed = \"magic\"").is_err());
}

#[test]
fn a_lossy_restoration_is_read_by_name() {
    let config = read("restore-lossy = \"extend\"").expect("a known restoration");

    assert_eq!(config.restoration, Some(Restoration::Extend));
    assert!(read("restore-lossy = \"magic\"").is_err());
}

#[test]
fn a_filter_phase_is_spelt_the_way_its_flag_is() {
    let config = read("filter-phase = \"minimum\"").expect("a known phase");

    assert_eq!(config.filter_phase, Some(FilterPhase::Minimum));
    assert!(read("filter-phase = \"mixed\"").is_err());
}

#[test]
fn a_layout_the_parser_cannot_read_names_the_organise_as_key() {
    for text in [
        "organise-as = \"{album}/{genre}\"",
        "organise-as = \"{album}/{title\"",
        "organise-as = \"../{album}/{title}\"",
        "organise-as = \"\"",
    ] {
        let error = read(text).expect_err("a template the reader refuses");

        assert!(
            matches!(
                error,
                Error::ConfigValue {
                    key: ConfigKey::OrganiseAs,
                    ..
                }
            ),
            "{text}: {error:?}"
        );
    }

    let error = read("organise-as = 3").expect_err("an integer is not a template");

    assert!(
        matches!(
            error,
            Error::ConfigType {
                key: ConfigKey::OrganiseAs,
                expected: ValueKind::Text,
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn a_file_that_names_no_layout_organises_as_this_build_ships() {
    let config = read("volume = 0.5").expect("a well formed document");

    assert_eq!(config.organise_as, None);
    assert_eq!(config.organise_as(), Layout::default());

    let named = read("organise-as = \"{artist}/{year} {album}/{track} {title}\"")
        .expect("a well formed document");

    assert_eq!(
        named.organise_as(),
        Layout::read("{artist}/{year} {album}/{track} {title}")
            .expect("a layout this build writes is one it reads")
    );
    assert_ne!(named.organise_as(), Layout::default());
}

#[test]
fn malformed_toml_is_reported_against_its_path() {
    let error = read("quality = ").expect_err("truncated");

    assert!(matches!(error, Error::ConfigSyntax { .. }), "{error:?}");
}

#[test]
fn the_copy_of_musicbrainz_is_offered_unless_the_file_withholds_it() {
    let unsaid = read("").expect("empty is valid");
    let withheld = read("offer-musicbrainz-copy = false").expect("a well formed document");
    let offered = read("offer-musicbrainz-copy = true").expect("a well formed document");

    assert!(unsaid.is_on(Switch::OfferMusicbrainzCopy));
    assert_eq!(
        withheld.switches.said(Switch::OfferMusicbrainzCopy),
        Some(false)
    );
    assert!(!withheld.is_on(Switch::OfferMusicbrainzCopy));
    assert!(offered.is_on(Switch::OfferMusicbrainzCopy));
}

#[test]
fn a_file_holding_the_stall_switch_reads_it_as_downloads_allowed_the_other_way_round() {
    let stalled = read("stall-downloads-on-metered = true").expect("a well formed document");
    let let_through = read("stall-downloads-on-metered = false").expect("a well formed document");
    let both = read("stall-downloads-on-metered = true\ndownload-on-metered = true")
        .expect("a well formed document");
    let both_the_other_order =
        read("download-on-metered = false\nstall-downloads-on-metered = false")
            .expect("a well formed document");
    let mistyped = read("stall-downloads-on-metered = \"yes\"").expect("passed over");

    assert_eq!(
        stalled.switches.said(Switch::DownloadOnMetered),
        Some(false)
    );
    assert_eq!(
        let_through.switches.said(Switch::DownloadOnMetered),
        Some(true)
    );
    assert!(both.is_on(Switch::DownloadOnMetered));
    assert!(!both_the_other_order.is_on(Switch::DownloadOnMetered));
    assert_eq!(mistyped.switches.said(Switch::DownloadOnMetered), None);
}
