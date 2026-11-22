use super::*;

#[test]
fn an_equaliser_measurer_is_read_and_a_blank_one_is_none() {
    let config = read("equaliser-measurer = \" Rtings \"").expect("a well formed document");

    assert_eq!(config.equaliser_measurer(), Some("Rtings"));

    let config = read("equaliser-measurer = \"  \"").expect("a well formed document");

    assert_eq!(config.equaliser_measurer(), None);

    assert_eq!(Config::default().equaliser_measurer(), None);
}

#[test]
fn a_measurer_is_preferred_for_one_device_and_the_one_for_every_device_serves_the_rest() {
    let config = read(
        r#"
        equaliser-measurer = "Rtings"

        [equaliser-measurer-for]
        "alsa_output.usb-Topping_E30-00.analog-stereo" = " crinacle "
        "auto_null" = "  "
        "bluez_output.one" = 7
        "#,
    )
    .expect("a well formed document");
    let topping = NodeName::new("alsa_output.usb-Topping_E30-00.analog-stereo");

    assert_eq!(
        config.equaliser_measurer_for(Some(&topping)),
        Some("crinacle")
    );
    assert_eq!(
        config.equaliser_measurer_for(Some(&NodeName::new("auto_null"))),
        Some("Rtings")
    );
    assert_eq!(
        config.equaliser_measurer_for(Some(&NodeName::new("bluez_output.one"))),
        Some("Rtings")
    );
    assert_eq!(config.equaliser_measurer_for(None), Some("Rtings"));
    assert_eq!(config.equaliser_measurer_for.len(), 1);
}

#[test]
fn a_config_of_the_first_release_has_no_per_device_measurers_and_reads_as_it_did() {
    let config = read("equaliser-measurer = \"Rtings\"").expect("a well formed document");

    assert!(config.equaliser_measurer_for.is_empty());
    assert_eq!(
        config.equaliser_measurer_for(Some(&NodeName::new("any"))),
        Some("Rtings")
    );
    assert_eq!(Config::default().equaliser_measurer_for(None), None);
}

#[test]
fn what_was_learnt_of_each_device_reads_back_and_a_spelling_this_build_does_not_know_is_passed_over()
 {
    let config = read(
        r#"[formats-learnt-for]
"upnp:uuid:one" = ["flac-192000", "channels-6", "channels-12"]
"cast:abc" = []
"cast:def" = "flac-192000"
"#,
    )
    .expect("a well formed document");

    assert_eq!(
        config
            .formats_learnt_for()
            .of([&NodeName::new("upnp:uuid:one")]),
        Some(&Plays::from_iter([Trial::FlacAt192kHz, Trial::SixChannels]))
    );
    assert_eq!(
        config.formats_learnt_for().of([&NodeName::new("cast:abc")]),
        Some(&Plays::default())
    );
    assert_eq!(
        config.formats_learnt_for().of([&NodeName::new("cast:def")]),
        None
    );
    assert!(Config::default().formats_learnt_for().is_empty());
    assert!(matches!(
        read("formats-learnt-for = [7]").expect_err("a list is not a table"),
        Error::ConfigType {
            key: ConfigKey::FormatsLearntFor,
            ..
        }
    ));
}

#[test]
fn what_was_learnt_is_written_in_the_spellings_it_is_read_back_from() {
    let scratch = Scratch::new();
    let plays = Plays::from_iter([Trial::FlacAt176kHz, Trial::EightChannels]);

    store_in_table(
        &scratch.path,
        ConfigKey::FormatsLearntFor,
        "upnp:uuid:one",
        learnt_written(&plays),
    )
    .expect("a writable file");

    let config = load(Some(&scratch.path)).expect("the file reads back");

    assert_eq!(
        config
            .formats_learnt_for()
            .of([&NodeName::new("upnp:uuid:one")]),
        Some(&plays)
    );
}

#[test]
fn a_measurers_table_that_is_not_a_table_is_refused() {
    let error = read("equaliser-measurer-for = \"Rtings\"").expect_err("a string is not a table");

    assert!(matches!(
        error,
        Error::ConfigType {
            key: ConfigKey::EqualiserMeasurerFor,
            ..
        }
    ));
}

#[test]
fn a_binding_is_read_per_sink_with_a_fallback_for_the_rest() {
    let config = read(
        r#"
        equaliser = true
        equaliser-profile = "Harman over-ear"

        [equaliser-for]
        "alsa_output.usb-Topping_E30-00.analog-stereo" = "Sennheiser HD 650"
        "auto_null" = "  "
        "#,
    )
    .expect("a well formed document");

    assert_eq!(config.switches.said(Switch::Equaliser), Some(true));
    assert!(config.is_on(Switch::Equaliser));

    let bindings = config.equaliser_for.expect("a table");

    assert_eq!(bindings.fallback(), Some(&kept("Harman over-ear")));
    assert_eq!(
        bound(
            &bindings,
            Some(&NodeName::new(
                "alsa_output.usb-Topping_E30-00.analog-stereo"
            ))
        ),
        Some(&kept("Sennheiser HD 650"))
    );
    assert_eq!(
        bound(
            &bindings,
            Some(&NodeName::new("alsa_output.somewhere-else"))
        ),
        Some(&kept("Harman over-ear"))
    );
    assert_eq!(bound(&bindings, None), Some(&kept("Harman over-ear")));
    assert_eq!(bindings.by_sink().count(), 1);
}

#[test]
fn a_sink_named_default_is_bound_on_its_own_rather_than_for_the_rest() {
    let config = read(
        r#"
        equaliser-profile = "Harman over-ear"

        [equaliser-for]
        default = "Sennheiser HD 650"
        "#,
    )
    .expect("a well formed document");

    let bindings = config.equaliser_for.expect("a table");

    assert_eq!(
        bound(&bindings, Some(&NodeName::new("default"))),
        Some(&kept("Sennheiser HD 650")),
        "a device named default was read as the fallback"
    );
    assert_eq!(bound(&bindings, None), Some(&kept("Harman over-ear")));
    assert_eq!(bindings.by_sink().count(), 1);
}

#[test]
fn a_fallback_and_the_devices_are_read_whichever_order_they_are_written_in() {
    let profile_first = read(
        r#"
        equaliser-profile = "Harman over-ear"
        equaliser-for = { "alsa_output.one" = "HD 650" }
        "#,
    )
    .expect("a well formed document");
    let devices_first = read(
        r#"
        equaliser-for = { "alsa_output.one" = "HD 650" }
        equaliser-profile = "Harman over-ear"
        "#,
    )
    .expect("a well formed document");

    for config in [profile_first, devices_first] {
        let bindings = config.equaliser_for.expect("a table");

        assert_eq!(bindings.fallback(), Some(&kept("Harman over-ear")));
        assert_eq!(
            bound(&bindings, Some(&NodeName::new("alsa_output.one"))),
            Some(&kept("HD 650"))
        );
    }
}

#[test]
fn a_build_that_says_nothing_about_the_equaliser_leaves_it_off_and_unbound() {
    let config = read("").expect("empty is valid");

    assert_eq!(config.switches.said(Switch::Equaliser), None);
    assert!(!config.is_on(Switch::Equaliser));
    assert_eq!(config.equaliser_for, None);
}

#[test]
fn a_binding_that_is_not_a_table_names_the_key_and_the_type_it_wanted() {
    let error = read("equaliser-for = \"harman\"").expect_err("a string is not a table");

    assert!(
        matches!(
            error,
            Error::ConfigType {
                key: ConfigKey::EqualiserFor,
                expected: ValueKind::Table,
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn a_device_bound_to_its_own_curve_is_answered_as_its_own_and_not_the_fallbacks() {
    let config = read(
        r#"
        equaliser-profile = true

        [equaliser-for]
        "bluez_output.headphones" = true
        "alsa_output.one" = "HD 650"
        "alsa_output.two" = false
        "#,
    )
    .expect("a well formed document");

    let bindings = config.equaliser_for.expect("a table");
    let headphones = NodeName::new("bluez_output.headphones");

    assert_eq!(
        bindings.for_sink(Some(&headphones)),
        Some((Some(&headphones), &Binding::Own))
    );
    assert_eq!(
        bindings.for_sink(Some(&NodeName::new("alsa_output.elsewhere"))),
        Some((None, &Binding::Own)),
        "a device naming nothing was handed a curve of its own rather than the fallback's"
    );
    assert_eq!(
        bound(&bindings, Some(&NodeName::new("alsa_output.one"))),
        Some(&kept("HD 650"))
    );
    assert_eq!(bindings.by_sink().count(), 2, "false was read as a binding");
}

#[test]
fn a_fallback_that_is_false_is_refused_rather_than_read_as_nothing() {
    let error = read("equaliser-profile = false").expect_err("false names nothing");

    assert!(
        matches!(
            error,
            Error::ConfigValue {
                key: ConfigKey::EqualiserProfile,
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn an_own_curve_written_into_the_file_reads_back_as_one() {
    let scratch = Scratch::new();
    let sink = "alsa_output.pci-0000_00_1f.3.analog-stereo";

    store_in_table(
        &scratch.path,
        ConfigKey::EqualiserFor,
        sink,
        written(&Binding::Own),
    )
    .expect("a writable file");
    store(
        &scratch.path,
        ConfigKey::EqualiserProfile,
        written(&kept("Harman over-ear")),
    )
    .expect("a writable file");

    let text = scratch.text();

    assert!(text.contains(&format!("\"{sink}\" = true")), "{text}");

    let bindings = load(Some(&scratch.path))
        .expect("it reads back")
        .equaliser_for
        .expect("a table");

    assert_eq!(
        bound(&bindings, Some(&NodeName::new(sink))),
        Some(&Binding::Own)
    );
    assert_eq!(bindings.fallback(), Some(&kept("Harman over-ear")));
}
