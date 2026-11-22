use super::*;

#[test]
fn every_settings_category_an_earlier_build_remembered_still_reads() {
    for remembered in [
        "output",
        "processing",
        "equaliser",
        "library",
        "filters",
        "enrichment",
        "accounts",
        "online",
        "recognition",
        "scrobbling",
        "sync",
        "desktop",
        "appearance",
        "about",
    ] {
        let read = read(&format!("last-settings-category = \"{remembered}\""))
            .unwrap_or_else(|error| panic!("{remembered} was refused: {error:?}"));

        assert_eq!(read.last_settings_category.as_deref(), Some(remembered));
    }

    assert!(settings_category("scrobble").is_none());
}

#[cfg(feature = "ui")]
#[test]
fn a_remembered_category_whose_settings_moved_opens_the_category_holding_them_now() {
    use resonate_ui::SettingsCategory;

    let opened = |remembered: &str| {
        read(&format!("last-settings-category = \"{remembered}\""))
            .expect("a remembered category")
            .remembered_settings_category()
    };

    assert_eq!(opened("enrichment"), SettingsCategory::Library);
    assert_eq!(opened("scrobbling"), SettingsCategory::Accounts);
    assert_eq!(opened("recognition"), SettingsCategory::Recognition);
    assert_eq!(opened("window"), SettingsCategory::Window);
}

#[cfg(feature = "ui")]
#[test]
fn motion_is_kept_until_the_file_asks_for_it_reduced() {
    assert!(
        !read("")
            .expect("empty is valid")
            .reduces_motion_where_the_desktop_asks(false)
    );
    assert!(
        read("reduced-motion = true")
            .expect("a boolean is valid")
            .reduces_motion_where_the_desktop_asks(false)
    );
}

#[cfg(feature = "ui")]
#[test]
fn the_volume_turns_with_the_wheel_until_the_file_says_it_does_not() {
    assert!(
        read("")
            .expect("empty is valid")
            .is_on(Switch::ScrollVolume)
    );
    assert!(
        !read("scroll-volume = false")
            .expect("a boolean is valid")
            .is_on(Switch::ScrollVolume)
    );
}

#[cfg(feature = "ui")]
#[test]
#[cfg(feature = "ui")]
fn the_artists_are_drawn_as_a_grid_unless_the_file_says_a_list() {
    let drawn = |text: &str| read(text).expect("a layout is valid").artists_drawn();

    assert_eq!(drawn(""), ArtistsDrawn::Grid);
    assert_eq!(drawn("artists-drawn = \"list\""), ArtistsDrawn::List);
    assert_eq!(drawn("artists-drawn = \"grid\""), ArtistsDrawn::Grid);
    assert!(read("artists-drawn = \"mosaic\"").is_err());
}

#[cfg(feature = "ui")]
#[test]
fn scrollbars_hide_until_scrolled_unless_the_file_names_another_mode() {
    let mode = |text: &str| read(text).expect("a mode is valid").scrollbars();

    assert_eq!(mode(""), ScrollbarMode::AutoHidden);
    assert_eq!(mode("scrollbars = \"shown\""), ScrollbarMode::Shown);
    assert_eq!(
        mode("scrollbars = \"auto-hide\""),
        ScrollbarMode::AutoHidden
    );
    assert_eq!(mode("scrollbars = \"hidden\""), ScrollbarMode::Hidden);
    assert!(read("scrollbars = \"sometimes\"").is_err());
}

#[cfg(feature = "ui")]
#[test]
fn the_live_spectrum_reads_as_built_unless_the_file_names_another_reading() {
    assert_eq!(
        read("").expect("an empty file").spectral(),
        resonate_core::Spectral::default()
    );

    let read_back = read(
        "spectrum-tilt = \"flat\"\nspectrum-floor = \"96\"\nspectrum-bands = \"thirds\"\n\
         spectrum-falls = \"quickly\"",
    )
    .expect("a well formed document")
    .spectral();

    assert_eq!(
        read_back,
        resonate_core::Spectral {
            tilt: SpectrumTilt::Flat,
            floor: SpectrumFloor::Deep,
            bands: SpectrumBands::Thirds,
            falls: SpectrumFalls::Quickly,
        }
    );
    assert!(read("spectrum-floor = \"-78\"").is_err());
}

#[cfg(feature = "ui")]
#[test]
fn a_scrollbars_switch_written_before_the_modes_still_reads() {
    let mode = |text: &str| read(text).expect("a boolean is valid").scrollbars();

    assert_eq!(mode("scrollbars = true"), ScrollbarMode::Shown);
    assert_eq!(mode("scrollbars = false"), ScrollbarMode::Hidden);
}

#[cfg(feature = "ui")]
#[test]
fn suggestions_are_listed_and_missing_and_the_counts_are_not_until_the_file_says_otherwise() {
    let built = resonate_ui::Tabs {
        suggestions: true,
        missing: false,
        counts: false,
    };

    assert_eq!(read("").expect("empty is valid").tabs(), built);
    assert_eq!(
        read("suggestions-tab = false\nmissing-tab = true")
            .expect("booleans are valid")
            .tabs(),
        resonate_ui::Tabs {
            suggestions: false,
            missing: true,
            ..built
        }
    );
    assert_eq!(
        read("tab-counts = true")
            .expect("a boolean is valid")
            .tabs(),
        resonate_ui::Tabs {
            counts: true,
            ..built
        }
    );
}

#[cfg(feature = "ui")]
#[test]
fn the_window_buttons_are_shown_until_the_file_hides_them_one_at_a_time() {
    let shown = resonate_ui::WindowButtons::SHOWN;

    assert_eq!(read("").expect("empty is valid").window_buttons(), shown);
    assert_eq!(
        read("minimise-button = false")
            .expect("a well formed document")
            .window_buttons(),
        resonate_ui::WindowButtons {
            minimise: false,
            ..shown
        }
    );
    assert_eq!(
        read("maximise-button = false")
            .expect("a well formed document")
            .window_buttons(),
        resonate_ui::WindowButtons {
            maximise: false,
            ..shown
        }
    );

    let error = read("maximise-button = \"no\"").expect_err("a string is not a boolean");

    assert!(
        matches!(
            error,
            Error::ConfigType {
                key: ConfigKey::MaximiseButton,
                expected: ValueKind::Boolean,
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn a_file_that_names_no_palette_is_worn_as_resonate_drew_it() {
    let config = read("volume = 0.5").expect("a well formed document");

    assert_eq!(config.appearance(), Appearance::DEFAULT);
}

#[test]
fn a_desktop_asking_for_more_contrast_wears_high_contrast_where_no_theme_is_stored() {
    let unstored = read("volume = 0.5").expect("a well formed document");
    let stored = read("theme = \"nord\"").expect("a well formed document");

    assert_eq!(
        unstored.appearance_where_the_desktop_asks(true).theme,
        Theme::HighContrast
    );
    assert_eq!(
        unstored.appearance_where_the_desktop_asks(false),
        Appearance::DEFAULT
    );
    assert_eq!(
        stored.appearance_where_the_desktop_asks(true).theme,
        Theme::Nord
    );
}

#[test]
fn the_desktop_is_followed_for_colour_and_text_where_neither_is_stored_and_not_where_one_is() {
    let unstored = read("volume = 0.5").expect("a well formed document");
    let stored = read("theme = \"nord\"\ntext-size = \"large\"").expect("a well formed document");
    let kept =
        read("theme = \"nord\"\nfollow-desktop-scheme = true").expect("a well formed document");
    let refused = read("follow-desktop-text-size = false").expect("a well formed document");

    assert_eq!(unstored.appearance().follows, Following::BOTH);
    assert_eq!(
        stored.appearance().follows,
        Following {
            scheme: false,
            text: false,
        }
    );
    assert!(kept.appearance().follows.scheme);
    assert!(kept.appearance().follows.text);
    assert!(!refused.appearance().follows.text);
    assert!(refused.appearance().follows.scheme);
}

#[test]
fn the_light_themes_are_read_by_their_names() {
    assert_eq!(
        read("theme = \"resonate-light\"").expect("a theme").theme,
        Some(Theme::Light)
    );
    assert_eq!(
        read("theme = \"high-contrast-light\"")
            .expect("a theme")
            .theme,
        Some(Theme::HighContrastLight)
    );
}

#[test]
fn a_desktop_asking_for_reduced_motion_is_heard_only_where_no_setting_is_stored() {
    let unstored = read("volume = 0.5").expect("a well formed document");
    let kept_off = read("reduced-motion = false").expect("a boolean is valid");

    assert!(unstored.reduces_motion_where_the_desktop_asks(true));
    assert!(!unstored.reduces_motion_where_the_desktop_asks(false));
    assert!(!kept_off.reduces_motion_where_the_desktop_asks(true));
}

#[test]
fn a_palette_no_theme_accent_or_size_claims_names_the_key() {
    for text in [
        "theme = \"solarized\"",
        "accent = \"chartreuse\"",
        "text-size = \"enormous\"",
    ] {
        let error = read(text).expect_err("outside the domain");

        assert!(
            matches!(error, Error::ConfigValue { .. }),
            "{text}: {error:?}"
        );
    }
}
