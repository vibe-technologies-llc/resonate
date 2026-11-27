use super::*;

#[test]
fn storing_a_setting_leaves_the_rest_of_the_file_as_the_user_wrote_it() {
    let scratch = Scratch::new()
        .seed("# my listening setup\nsink = \"auto_null\"\n\n# resampler\nquality = \"fast\"\n");

    store(&scratch.path, ConfigKey::Quality, "high").expect("a writable file");

    let text = scratch.text();

    assert!(text.contains("# my listening setup"), "{text}");
    assert!(text.contains("# resampler"), "{text}");
    assert!(text.contains("quality = \"high\""), "{text}");
    assert_eq!(
        load(Some(&scratch.path)).expect("it reads back").quality,
        Some(Quality::High)
    );
}

#[test]
fn storing_a_setting_creates_the_file_and_its_directory() {
    let scratch = Scratch::new();

    store(&scratch.path, ConfigKey::ReplayGain, "album").expect("a writable file");

    assert_eq!(
        load(Some(&scratch.path))
            .expect("it reads back")
            .replay_gain,
        Some(ReplayGainMode::Album)
    );
}

#[test]
fn storing_a_setting_twice_replaces_it_rather_than_repeating_it() {
    let scratch = Scratch::new();

    store(&scratch.path, ConfigKey::Dither, "none").expect("a writable file");
    store(&scratch.path, ConfigKey::Dither, "triangular").expect("a writable file");

    let text = scratch.text();

    assert_eq!(text.matches("dither").count(), 1, "{text}");
    assert_eq!(
        load(Some(&scratch.path)).expect("it reads back").dither,
        Some(DitherKind::Triangular)
    );
}

#[test]
fn clearing_a_setting_drops_the_key_and_keeps_the_rest() {
    let scratch = Scratch::new().seed("sink = \"auto_null\"\n\n# resampler\nquality = \"high\"\n");

    clear(&scratch.path, ConfigKey::Sink).expect("a writable file");

    let text = scratch.text();

    assert!(text.contains("# resampler"), "{text}");
    assert!(!text.contains("sink"), "{text}");

    let config = load(Some(&scratch.path)).expect("it reads back");

    assert_eq!(config.sink, None);
    assert_eq!(config.quality, Some(Quality::High));
}

#[test]
fn clearing_a_setting_the_file_never_had_leaves_it_alone() {
    let scratch = Scratch::new().seed("quality = \"fast\"\n");

    clear(&scratch.path, ConfigKey::Sink).expect("a writable file");

    assert_eq!(scratch.text(), "quality = \"fast\"\n");
}

#[test]
#[cfg(unix)]
fn the_settings_file_is_its_owners_alone_from_the_first_write_and_after_a_wider_one() {
    use std::os::unix::fs::PermissionsExt as _;

    let mode = |path: &Path| {
        fs::metadata(path)
            .expect("the settings file")
            .permissions()
            .mode()
            & 0o777
    };
    let scratch = Scratch::new();
    let folder = scratch.path.parent().expect("a folder").to_path_buf();

    fs::create_dir_all(&folder).expect("a writable temporary directory");

    store(&scratch.path, ConfigKey::AuddToken, "a secret").expect("a writable file");

    assert_eq!(mode(&scratch.path), 0o600);

    fs::set_permissions(&scratch.path, fs::Permissions::from_mode(0o644)).expect("a mode");
    store(&scratch.path, ConfigKey::AuddToken, "another").expect("a writable file");

    assert_eq!(mode(&scratch.path), 0o600);
}

#[test]
#[cfg(unix)]
fn a_link_to_a_file_not_yet_made_is_written_through_to_where_it_points() {
    use std::os::unix::fs::symlink;

    let scratch = Scratch::new();
    let folder = scratch.path.parent().expect("a folder").to_path_buf();

    fs::create_dir_all(&folder).expect("a folder");

    let kept = folder.join("dotfiles").join("config.toml");

    symlink("dotfiles/config.toml", &scratch.path).expect("a relative dangling link");

    store(&scratch.path, ConfigKey::Dither, "none").expect("a writable file");

    assert!(
        fs::symlink_metadata(&scratch.path)
            .expect("the link stands")
            .file_type()
            .is_symlink(),
        "the dangling link was replaced by a plain file"
    );
    assert!(
        fs::read_to_string(&kept)
            .expect("the target was made")
            .contains("dither")
    );
}

#[test]
#[cfg(unix)]
fn a_link_that_points_back_at_itself_is_given_up_on_rather_than_followed_for_ever() {
    use std::os::unix::fs::symlink;

    let scratch = Scratch::new();

    fs::create_dir_all(scratch.path.parent().expect("a folder")).expect("a folder");
    symlink(&scratch.path, &scratch.path).expect("a link to itself");

    assert!(followed(&scratch.path).ends_with(scratch.path.file_name().expect("a name")));
}

#[test]
#[cfg(unix)]
fn a_symlinked_file_is_written_through_the_link_and_keeps_its_mode() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    let scratch = Scratch::new().seed("quality = \"fast\"\n");
    let folder = scratch.path.parent().expect("a folder").to_path_buf();
    let kept = folder.join("dotfiles.toml");

    fs::rename(&scratch.path, &kept).expect("the seed moves");
    fs::set_permissions(&kept, fs::Permissions::from_mode(0o600)).expect("a mode");
    symlink(&kept, &scratch.path).expect("a link");

    store(&scratch.path, ConfigKey::Dither, "none").expect("a writable file");

    assert!(
        fs::symlink_metadata(&scratch.path)
            .expect("the link stands")
            .file_type()
            .is_symlink(),
        "the link was replaced by a plain file"
    );
    assert!(
        fs::read_to_string(&kept)
            .expect("the target")
            .contains("dither")
    );
    assert_eq!(
        fs::metadata(&kept)
            .expect("the target")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let staged: Vec<_> = fs::read_dir(&folder)
        .expect("the folder")
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .ends_with(STAGING_SUFFIX)
        })
        .collect();

    assert!(staged.is_empty(), "a staging file was left behind");
}

#[test]
fn what_a_killed_writer_left_is_swept_by_the_next_and_nothing_else() {
    let scratch = Scratch::new().seed("quality = \"fast\"\n");
    let folder = scratch.path.parent().expect("a folder").to_path_buf();
    let left = [
        folder.join("config.toml.lock"),
        folder.join("config.toml.999999-3.new"),
    ];
    let kept = [
        folder.join("config.toml.notes"),
        folder.join("config.toml.9x-3.new"),
        folder.join("other.toml.1-1.new"),
    ];

    for path in left.iter().chain(&kept) {
        fs::write(path, "").expect("a writable folder");
    }

    store(&scratch.path, ConfigKey::Dither, "none").expect("a writable file");

    for path in &left {
        assert!(!path.exists(), "{} was left", path.display());
    }
    for path in &kept {
        assert!(path.exists(), "{} was swept", path.display());
    }

    assert!(scratch.text().contains("dither"));
}

#[test]
fn writers_at_once_each_keep_what_they_wrote() {
    const WRITERS: usize = 4;
    const EACH: usize = 25;

    let scratch = Scratch::new().seed("");

    std::thread::scope(|scope| {
        for writer in 0..WRITERS {
            let path = &scratch.path;

            scope.spawn(move || {
                for entry in 0..EACH {
                    store_in_table(
                        path,
                        ConfigKey::EqualiserFor,
                        &format!("sink-{writer}-{entry}"),
                        "kept",
                    )
                    .expect("a writable file");
                }
            });
        }
    });

    let document = document(&scratch.path, &scratch.text()).expect("a well formed file");
    let held = document
        .get(ConfigKey::EqualiserFor.as_str())
        .and_then(Item::as_table_like)
        .map_or(0, |table| table.len());

    assert_eq!(held, WRITERS * EACH, "a write was lost to another");
}

#[test]
fn clearing_a_setting_with_no_file_writes_nothing() {
    let scratch = Scratch::new();

    clear(&scratch.path, ConfigKey::Sink).expect("a missing file is nothing to clear");

    assert!(!scratch.path.exists());
}

#[test]
fn a_binding_written_into_the_table_keeps_the_rest_of_the_file() {
    let scratch = Scratch::new().seed("# my listening setup\nquality = \"fast\"\n");
    let sink = "alsa_output.pci-0000_00_1f.3.analog-stereo";

    store_in_table(
        &scratch.path,
        ConfigKey::EqualiserFor,
        sink,
        "Sennheiser HD 650",
    )
    .expect("a writable file");
    store(
        &scratch.path,
        ConfigKey::EqualiserProfile,
        "Harman over-ear",
    )
    .expect("a writable file");

    let text = scratch.text();

    assert!(text.contains("# my listening setup"), "{text}");
    assert!(text.contains("[equaliser-for]"), "{text}");
    assert!(
        text.contains(&format!("\"{sink}\" = \"Sennheiser HD 650\"")),
        "a dotted node name was written as a path rather than one key: {text}"
    );

    let config = load(Some(&scratch.path)).expect("it reads back");
    let bindings = config.equaliser_for.expect("a table");

    assert_eq!(
        bound(&bindings, Some(&NodeName::new(sink))),
        Some(&kept("Sennheiser HD 650"))
    );
    assert_eq!(bindings.fallback(), Some(&kept("Harman over-ear")));
}

#[test]
fn a_value_written_again_keeps_the_comments_around_it() {
    let sink = "alsa_output.usb";
    let scratch = Scratch::new().seed(&format!(
        "quality = \"fast\" # the laptop's fan\n\n[equaliser-for]\n# my DAC\n\"{sink}\" = \"Flat\" # for now\n"
    ));

    store(&scratch.path, ConfigKey::Quality, "high").expect("a writable file");
    store_in_table(
        &scratch.path,
        ConfigKey::EqualiserFor,
        sink,
        "Sennheiser HD 650",
    )
    .expect("a writable file");

    let text = scratch.text();

    assert!(
        text.contains("quality = \"high\" # the laptop's fan"),
        "{text}"
    );
    assert!(text.contains("# my DAC\n"), "{text}");
    assert!(
        text.contains(&format!("\"{sink}\" = \"Sennheiser HD 650\" # for now")),
        "{text}"
    );
}

#[test]
fn clearing_the_last_binding_takes_the_table_away_with_it() {
    let scratch = Scratch::new();

    store_in_table(&scratch.path, ConfigKey::EqualiserFor, "one", "a").expect("writable");
    store_in_table(&scratch.path, ConfigKey::EqualiserFor, "two", "b").expect("writable");

    clear_in_table(&scratch.path, ConfigKey::EqualiserFor, "one").expect("writable");

    assert!(
        scratch.text().contains("[equaliser-for]"),
        "{}",
        scratch.text()
    );

    clear_in_table(&scratch.path, ConfigKey::EqualiserFor, "two").expect("writable");

    assert!(
        !scratch.text().contains("equaliser-for"),
        "an empty table was left behind: {}",
        scratch.text()
    );

    clear_in_table(&scratch.path, ConfigKey::EqualiserFor, "two")
        .expect("nothing to clear is not a failure");
}

#[test]
fn a_table_written_over_a_value_that_is_not_one_refuses_rather_than_destroying_it() {
    let scratch = Scratch::new().seed("equaliser-for = \"harman\"\n");

    assert!(matches!(
        store_in_table(&scratch.path, ConfigKey::EqualiserFor, "one", "a"),
        Err(Error::ConfigType { .. })
    ));
    assert_eq!(scratch.text(), "equaliser-for = \"harman\"\n");
}

#[test]
fn writing_or_forgetting_the_download_switch_takes_the_stall_spelling_out_of_the_file() {
    let folder = env::temp_dir().join(format!("resonate-config-retired-{}", process::id()));
    let path = folder.join("config.toml");

    fs::create_dir_all(&folder).expect("a temporary folder");
    fs::write(&path, "online = true\nstall-downloads-on-metered = true\n").expect("an older file");

    let before = load(Some(&path)).expect("the older file reads");

    store(&path, ConfigKey::DownloadOnMetered, true).expect("written");

    let stored = fs::read_to_string(&path).expect("the file");

    fs::write(&path, "stall-downloads-on-metered = true\n").expect("an older file again");
    clear(&path, ConfigKey::DownloadOnMetered).expect("forgotten");

    let cleared = fs::read_to_string(&path).expect("the file");
    let after = load(Some(&path)).expect("the file reads");
    let _ = fs::remove_dir_all(&folder);

    assert!(!before.is_on(Switch::DownloadOnMetered));
    assert!(stored.contains("download-on-metered = true"));
    assert!(!stored.contains("stall-downloads-on-metered"));
    assert!(stored.contains("online = true"));
    assert!(!cleared.contains("stall-downloads-on-metered"));
    assert_eq!(after.switches.said(Switch::DownloadOnMetered), None);
    assert!(after.is_on(Switch::DownloadOnMetered));
}

#[test]
fn a_removed_contact_is_ignored_when_read_and_dropped_on_the_next_edit() {
    let scratch = Scratch::new().seed("contact = 7\nonline = false\n");

    let config = read(&scratch.text()).expect("an obsolete setting reads without a value error");

    assert!(!config.is_on(Switch::Online));
    assert!(ConfigKey::parse("contact").is_none());

    store(&scratch.path, ConfigKey::EnrichOnMetered, true).expect("written");

    assert_eq!(scratch.text(), "online = false\nenrich-on-metered = true\n");
}

fn seeded_with_bytes(bytes: &[u8]) -> Scratch {
    let scratch = Scratch::new();

    fs::create_dir_all(scratch.path.parent().expect("a folder")).expect("a writable folder");
    fs::write(&scratch.path, bytes).expect("a writable folder");
    scratch
}

fn names_beside(scratch: &Scratch) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(scratch.path.parent().expect("a folder"))
        .expect("the folder")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    names.sort();
    names
}

fn every_write(path: &Path) -> Vec<Result<()>> {
    vec![
        store(path, ConfigKey::Quality, "high"),
        clear(path, ConfigKey::Sink),
        store_in_table(path, ConfigKey::EqualiserFor, "one", "a"),
        clear_in_table(path, ConfigKey::EqualiserFor, "one"),
        edit(path, |editing| {
            editing.store(ConfigKey::Dither, "none");
            Ok(())
        }),
    ]
}

fn assert_left_as_it_was(scratch: &Scratch, bytes: &[u8], modified_then: std::time::SystemTime) {
    assert_eq!(fs::read(&scratch.path).expect("the file stands"), bytes);
    assert_eq!(modified(scratch), modified_then);
    assert_eq!(names_beside(scratch), ["config.toml"]);
}

fn modified(scratch: &Scratch) -> std::time::SystemTime {
    fs::metadata(&scratch.path)
        .expect("the file stands")
        .modified()
        .expect("a modification time")
}

#[test]
fn a_write_against_a_file_that_is_not_toml_is_refused_and_leaves_it_byte_for_byte() {
    let broken = [
        "quality = \"high\nsink = \"auto_null\"\n",
        "quality =\n",
        "quality = \"high\"\nquality = \"fast\"\n",
        "[equaliser-for\none = \"a\"\n",
        "sink = [\"a\", \"b\"\n",
        "= \"high\"\n",
        "# my setup\nquality = high\n",
        "quality = \"fast\"\n\u{0}\n",
        "[equaliser-for]\none = \"a\"\n[equaliser-for]\ntwo = \"b\"\n",
    ];

    for text in broken {
        let scratch = Scratch::new().seed(text);
        let before = modified(&scratch);

        for written in every_write(&scratch.path) {
            assert!(
                matches!(&written, Err(Error::ConfigSyntax { path, .. }) if path == &scratch.path),
                "{text:?} was not refused as a syntax error: {written:?}"
            );
        }

        assert_left_as_it_was(&scratch, text.as_bytes(), before);
    }
}

#[test]
fn a_write_against_a_file_that_is_not_utf_8_is_refused_and_leaves_it_byte_for_byte() {
    let utf_16_little_endian: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain(
            "quality = \"fast\"\n"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        )
        .collect();
    let utf_16_big_endian: Vec<u8> = [0xFE, 0xFF]
        .into_iter()
        .chain(
            "quality = \"fast\"\n"
                .encode_utf16()
                .flat_map(u16::to_be_bytes),
        )
        .collect();

    let broken: [&[u8]; 4] = [
        b"sink = \"caf\xE9\"\n",
        b"\xFF\xFE\xFD",
        &utf_16_little_endian,
        &utf_16_big_endian,
    ];

    for bytes in broken {
        let scratch = seeded_with_bytes(bytes);
        let before = modified(&scratch);

        for written in every_write(&scratch.path) {
            assert!(
                matches!(&written, Err(Error::ReadConfig { path, .. }) if path == &scratch.path),
                "{bytes:?} was not refused as unreadable: {written:?}"
            );
        }

        assert_left_as_it_was(&scratch, bytes, before);
    }
}

#[test]
fn a_write_into_a_table_that_is_no_table_is_refused_and_leaves_the_file_byte_for_byte() {
    let broken = [
        "equaliser-for = \"harman\"\n",
        "# kept\nequaliser-for = 7\nquality = \"fast\"\n",
        "equaliser-for = [\"one\", \"two\"]\n",
        "equaliser-for = true # my choice\n",
    ];

    for text in broken {
        let scratch = Scratch::new().seed(text);
        let before = modified(&scratch);

        let written = store_in_table(&scratch.path, ConfigKey::EqualiserFor, "one", "a");

        assert!(
            matches!(
                &written,
                Err(Error::ConfigType {
                    key: ConfigKey::EqualiserFor,
                    expected: ValueKind::Table,
                    ..
                })
            ),
            "{text:?} was not refused as a wrong type: {written:?}"
        );

        assert_left_as_it_was(&scratch, text.as_bytes(), before);
    }
}

#[test]
fn a_file_that_is_no_file_is_refused_a_write_and_nothing_is_made_beside_it() {
    let scratch = Scratch::new();

    fs::create_dir_all(&scratch.path).expect("a folder where the file would be");

    assert!(matches!(
        store(&scratch.path, ConfigKey::Quality, "high"),
        Err(Error::ReadConfig { .. })
    ));
    assert!(scratch.path.is_dir());
    assert_eq!(names_beside(&scratch), ["config.toml"]);
}
