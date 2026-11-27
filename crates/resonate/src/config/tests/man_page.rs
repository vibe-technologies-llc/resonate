use super::*;

const CONFIG_MAN_PAGE: &str = include_str!("../../../../../packaging/resonate-config.toml.5");

fn documented_keys() -> BTreeSet<&'static str> {
    CONFIG_MAN_PAGE
        .lines()
        .zip(CONFIG_MAN_PAGE.lines().skip(1))
        .filter(|(before, _)| *before == ".TP")
        .filter_map(|(_, entry)| entry.strip_prefix(".B "))
        .collect()
}

fn entry_of(key: &str) -> String {
    let opening = format!(".B {key}");
    let mut lines = CONFIG_MAN_PAGE.lines().skip_while(|line| *line != opening);

    lines.next();
    lines
        .take_while(|line| !line.starts_with(".TP") && !line.starts_with(".SH"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn spelled_by<Choice: clap::ValueEnum>() -> Vec<String> {
    Choice::value_variants()
        .iter()
        .filter_map(Choice::to_possible_value)
        .map(|spelling| spelling.get_name().to_owned())
        .collect()
}

#[test]
fn every_setting_the_file_reads_is_documented_in_its_man_page_and_nothing_else_is() {
    let read: BTreeSet<&str> = ConfigKey::ALL.into_iter().map(ConfigKey::as_str).collect();

    assert_eq!(documented_keys(), read);
}

#[test]
fn the_man_page_gives_every_switch_the_default_the_table_does() {
    for switch in Switch::ALL {
        let entry = entry_of(switch.as_str());
        let default = format!("Default: {}.", switch.on_unless_said());

        assert!(entry.starts_with("Boolean."), "{switch}: {entry}");
        assert!(entry.contains(&default), "{switch} reads {entry}");
    }
}

#[test]
fn the_man_page_gives_every_spelling_a_setting_takes() {
    let spelled: [(ConfigKey, Vec<String>); 20] = [
        (ConfigKey::Quality, spelled_by::<QualityArg>()),
        (ConfigKey::FilterPhase, spelled_by::<FilterPhaseArg>()),
        (ConfigKey::Dither, spelled_by::<DitherArg>()),
        (ConfigKey::NoiseShaping, spelled_by::<NoiseShapingArg>()),
        (
            ConfigKey::RestoreLossy,
            Restoration::ALL
                .map(|kind| kind.as_str().to_owned())
                .to_vec(),
        ),
        (
            ConfigKey::Crossfeed,
            Crossfeed::ALL.map(|kind| kind.as_str().to_owned()).to_vec(),
        ),
        (
            ConfigKey::Theme,
            Theme::ALL.map(|theme| theme.as_str().to_owned()).to_vec(),
        ),
        (
            ConfigKey::Accent,
            Accent::ALL
                .map(|accent| accent.as_str().to_owned())
                .to_vec(),
        ),
        (
            ConfigKey::TextSize,
            TextSize::ALL.map(|size| size.as_str().to_owned()).to_vec(),
        ),
        (
            ConfigKey::Language,
            LanguageChoice::ALL
                .map(|choice| choice.as_str().to_owned())
                .to_vec(),
        ),
        (
            ConfigKey::Scrollbars,
            ScrollbarMode::ALL
                .map(|mode| mode.as_str().to_owned())
                .to_vec(),
        ),
        (
            ConfigKey::SyncControl,
            RemoteTrust::ALL
                .map(|trust| trust.as_str().to_owned())
                .to_vec(),
        ),
        (
            ConfigKey::ShuffleStyle,
            ShuffleStyle::ALL
                .map(|style| style.as_str().to_owned())
                .to_vec(),
        ),
        (
            ConfigKey::ArtistsDrawn,
            ArtistsDrawn::ALL
                .map(|drawn| drawn.as_str().to_owned())
                .to_vec(),
        ),
        (
            ConfigKey::SpectrumTilt,
            SpectrumTilt::ALL
                .map(|tilt| tilt.as_str().to_owned())
                .to_vec(),
        ),
        (
            ConfigKey::SpectrumFloor,
            SpectrumFloor::ALL
                .map(|floor| floor.as_str().to_owned())
                .to_vec(),
        ),
        (
            ConfigKey::SpectrumBands,
            SpectrumBands::ALL
                .map(|bands| bands.as_str().to_owned())
                .to_vec(),
        ),
        (
            ConfigKey::SpectrumFalls,
            SpectrumFalls::ALL
                .map(|falls| falls.as_str().to_owned())
                .to_vec(),
        ),
        (
            ConfigKey::DiscordShows,
            Shown::ALL.map(|shown| shown.as_str().to_owned()).to_vec(),
        ),
        (
            ConfigKey::RecompressTo,
            Ceiling::ALL
                .map(|ceiling| ceiling.as_str().to_owned())
                .to_vec(),
        ),
    ];

    for (key, spellings) in &spelled {
        let entry = entry_of(key.as_str());
        for spelling in spellings {
            assert!(
                entry.contains(spelling.as_str()),
                "{key} takes {spelling}, which its man page entry does not give"
            );
        }
    }
}

#[test]
fn the_man_page_names_every_settings_category_the_file_remembers() {
    let categories = entry_of(ConfigKey::LastSettingsCategory.as_str());

    for category in SETTINGS_CATEGORIES
        .into_iter()
        .chain(SETTINGS_CATEGORIES_SPELT_BEFORE)
    {
        assert!(settings_category(category).is_some());
        assert!(
            categories.contains(category),
            "{category} is not in the man page"
        );
    }
}

#[cfg(feature = "ui")]
#[test]
fn every_tab_the_window_remembers_is_read_back_and_named_by_the_man_page() {
    let tabs = entry_of(ConfigKey::LastTab.as_str());

    for pane in resonate_ui::Pane::ALL {
        let tab_named = pane.as_str();

        assert_eq!(tab(tab_named), Some(tab_named));
        assert!(
            tabs.contains(tab_named),
            "{tab_named} is not in the man page"
        );

        let text = format!("last-tab = \"{tab_named}\"\n");

        assert_eq!(read(&text).unwrap().last_tab.as_deref(), Some(tab_named));
    }
}
