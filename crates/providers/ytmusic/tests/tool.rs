use resonate_providers::{Error, ProviderOp};
use resonate_ytmusic::{Cookies, Tier, Tool, VideoId};

const A_WORD_TOO_LONG_FOR_AN_ID: &str = "abcdefghijklmnopq";
const SONG: &str = "9RfVp-GhKfs";

fn song() -> VideoId {
    VideoId::read(SONG).expect("a video id")
}

#[test]
fn a_browser_with_a_profile_is_a_browser_and_any_other_path_is_a_file() {
    assert_eq!(
        Cookies::read("firefox:/home/a/zen (b)"),
        Cookies::Browser("firefox:/home/a/zen (b)".into())
    );
    assert_eq!(
        Cookies::read("/home/a/cookies.txt"),
        Cookies::File("/home/a/cookies.txt".into())
    );
    assert_eq!(Cookies::read("  "), Cookies::None);
}

#[test]
fn a_browser_alone_is_a_browser_and_a_path_that_merely_holds_a_colon_is_a_file() {
    assert_eq!(
        Cookies::read(" firefox "),
        Cookies::Browser("firefox".into())
    );
    assert_eq!(
        Cookies::read("chromium+gnomekeyring:Default"),
        Cookies::Browser("chromium+gnomekeyring:Default".into())
    );
    assert_eq!(
        Cookies::read(r"C:\Users\a\cookies.txt"),
        Cookies::File(r"C:\Users\a\cookies.txt".into())
    );
    assert_eq!(
        Cookies::read("./firefox:cookies.txt"),
        Cookies::File("./firefox:cookies.txt".into())
    );
}

#[test]
fn a_bare_word_goes_to_the_tool_as_a_browser_and_a_separator_or_an_extension_names_a_file() {
    assert_eq!(Cookies::read("zen"), Cookies::Browser("zen".into()));
    assert_eq!(Cookies::read("firefx"), Cookies::Browser("firefx".into()));
    assert_eq!(
        Cookies::read("cookies.txt"),
        Cookies::File("cookies.txt".into())
    );
    assert_eq!(
        Cookies::read("youtube.cookies"),
        Cookies::File("youtube.cookies".into())
    );
    assert_eq!(
        Cookies::read("exported/cookies.txt"),
        Cookies::File("exported/cookies.txt".into())
    );
    assert_eq!(
        Cookies::read(r"exported\cookies.txt"),
        Cookies::File(r"exported\cookies.txt".into())
    );
}

#[test]
fn a_cookie_value_is_withheld_from_debug() {
    let browser = Cookies::read("firefox:/home/a/zen");
    let file = Cookies::read("/home/a/cookies.txt");
    let tool = Tool::new(None, browser.clone());

    for told in [
        format!("{browser:?}"),
        format!("{file:?}"),
        format!("{tool:?}"),
    ] {
        assert!(told.contains("<withheld>"), "{told}");
        assert!(!told.contains("/home/a"), "{told}");
    }
}

#[test]
fn an_id_that_would_leave_the_address_is_never_asked() {
    for refused in [
        "",
        "../admin",
        "a b",
        "a&list=LM",
        "a?b",
        "a/b",
        "é",
        A_WORD_TOO_LONG_FOR_AN_ID,
    ] {
        assert_eq!(VideoId::read(refused), None, "{refused:?}");
    }
    assert_eq!(song().as_str(), SONG);
    assert_eq!(
        song().address(),
        "https://music.youtube.com/watch?v=9RfVp-GhKfs"
    );
}

#[test]
fn only_the_tiers_a_free_account_lacks_are_premium_and_each_lands_as_it_was_served() {
    assert_eq!(
        [Tier::Opus774, Tier::Aac141, Tier::Opus251]
            .map(|tier| (tier.extension(), tier.is_premium())),
        [("mka", true), ("m4a", true), ("mka", false)]
    );
}

#[test]
fn a_tool_not_there_is_the_provider_away() {
    let missing = Tool::new(Some("/nowhere/at/all/yt-dlp".into()), Cookies::None);

    let listing = missing.formats(&song()).expect_err("no tool to run");
    let download = match missing.download(&song(), Tier::Opus774) {
        Ok(_) => panic!("no tool to run"),
        Err(error) => error,
    };

    let playlists = missing.playlists().expect_err("no tool to run");

    assert!(matches!(
        listing,
        Error::ToolNotRun {
            op: ProviderOp::Search,
            ..
        }
    ));
    assert!(matches!(
        download,
        Error::ToolNotRun {
            op: ProviderOp::Download,
            ..
        }
    ));
    assert!(matches!(
        playlists,
        Error::ToolNotRun {
            op: ProviderOp::Browse,
            ..
        }
    ));
    for error in [listing, download, playlists] {
        assert!(error.is_the_provider_away());
        assert_eq!(error.why_away(), resonate_providers::WhyAway::ToolMissing);
    }
}

#[cfg(unix)]
mod support;

#[cfg(unix)]
mod faked {
    use std::{
        fs,
        io::{self, Read},
        process::Command,
        time::{Duration, Instant},
    };

    use resonate_providers::Opened;
    use resonate_ytmusic::Formats;

    use super::{support::Fake, *};

    const IMPATIENT: Duration = Duration::from_millis(300);
    const GIVEN_UP_WELL_BEFORE: Duration = Duration::from_secs(4);
    const NO_ARGUMENT_MAY_CONVERT: [&str; 11] = [
        "-x",
        "--extract-audio",
        "--audio-format",
        "--audio-quality",
        "--remux-video",
        "--recode-video",
        "--embed-metadata",
        "--embed-thumbnail",
        "--embed-chapters",
        "--embed-info-json",
        "--postprocessor-args",
    ];
    const COOKIES_LAPSED: &str = "echo 'WARNING: [youtube] The provided YouTube account cookies are no longer valid. They have likely been rotated in the browser as a security measure.' >&2";

    fn opened(tool: &Tool, tier: Tier) -> Box<dyn Read + Send> {
        match tool.download(&song(), tier).expect("a download") {
            Opened::Reading { reader, .. } => reader,
            Opened::Gone => panic!("the song was gone"),
        }
    }

    fn told_by(read: &io::Error) -> Option<&Error> {
        read.get_ref()?.downcast_ref()
    }

    fn still_running(fake: &Fake) -> bool {
        let pid = fs::read_to_string(fake.folder().join("pid")).expect("the tool's pid");
        Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(std::process::Stdio::null())
            .status()
            .expect("kill")
            .success()
    }

    #[test]
    fn the_best_tier_listed_is_taken_in_the_order_774_141_251() {
        for (listed, best) in [
            (vec!["251", "141", "774"], Tier::Opus774),
            (vec!["251", "141"], Tier::Aac141),
            (vec!["18", "251"], Tier::Opus251),
        ] {
            let listing: Vec<(&str, u64)> = listed.iter().map(|id| (*id, 1)).collect();
            let fake = Fake::listing(&listing);

            let formats = fake
                .tool(Cookies::None)
                .formats(&song())
                .expect("a listing");

            assert_eq!(formats.best, Some(best), "{listed:?}");
        }
    }

    #[test]
    fn a_length_is_what_the_best_tier_stated_and_none_where_it_stated_nothing() {
        let stated = Fake::listing(&[("251", 3), ("774", 7_800_000)]);
        let unstated = Fake::answering(
            r#"echo '{"formats":[{"format_id":"774","filesize":null},{"format_id":"251","filesize":3}]}'"#,
        );

        assert_eq!(
            stated
                .tool(Cookies::None)
                .formats(&song())
                .expect("a listing"),
            Formats {
                best: Some(Tier::Opus774),
                length: Some(7_800_000)
            }
        );
        assert_eq!(
            unstated
                .tool(Cookies::None)
                .formats(&song())
                .expect("a listing"),
            Formats {
                best: Some(Tier::Opus774),
                length: None
            }
        );
    }

    #[test]
    fn a_song_with_none_of_the_tiers_has_no_best() {
        let fake = Fake::listing(&[("140", 1), ("251-drc", 1), ("18", 1)]);

        let formats = fake
            .tool(Cookies::None)
            .formats(&song())
            .expect("a listing");

        assert_eq!(formats.best, None);
    }

    #[test]
    fn one_odd_format_never_fails_the_listing() {
        let fake = Fake::answering(
            r#"echo '{"formats":[{"format_id":7},"odd",{"format_id":"251","filesize":5}]}'"#,
        );

        let formats = fake
            .tool(Cookies::None)
            .formats(&song())
            .expect("a listing");

        assert_eq!(formats.best, Some(Tier::Opus251));
    }

    #[test]
    fn a_listing_that_is_not_the_document_expected_is_unreadable() {
        let fake = Fake::answering("echo 'not a document'");

        assert!(matches!(
            fake.tool(Cookies::None).formats(&song()),
            Err(Error::Unreadable {
                op: ProviderOp::Search,
                ..
            })
        ));
    }

    #[test]
    fn the_bytes_read_are_exactly_what_the_tool_wrote_to_its_stdout() {
        let bytes: Vec<u8> = (0..=u8::MAX).cycle().take(300_000).collect();
        let fake = Fake::answering("cat \"$here/bytes\"");
        fs::write(fake.folder().join("bytes"), &bytes).expect("the bytes the tool sends");
        let mut read = Vec::new();

        opened(&fake.tool(Cookies::None), Tier::Opus774)
            .read_to_end(&mut read)
            .expect("a whole download");

        assert_eq!(read, bytes);
    }

    #[test]
    fn a_download_asks_for_the_one_format_onto_stdout_at_the_songs_address() {
        let fake = Fake::answering("printf bytes");

        opened(&fake.tool(Cookies::None), Tier::Aac141)
            .read_to_end(&mut Vec::new())
            .expect("a whole download");

        let call = fake.calls().remove(0);
        let at = |flag: &str| call.iter().position(|argument| argument == flag);
        assert_eq!(call[at("-f").expect("a format") + 1], "141");
        assert_eq!(call[at("-o").expect("an output") + 1], "-");
        assert_eq!(
            call.last().map(String::as_str),
            Some("https://music.youtube.com/watch?v=9RfVp-GhKfs")
        );
    }

    #[test]
    fn every_call_ignores_the_listeners_own_config_and_asks_for_one_video_unconverted() {
        let fake = Fake::answering(r#"echo '{"formats":[]}'"#);
        let tool = fake.tool(Cookies::None);

        tool.formats(&song()).expect("a listing");
        opened(&tool, Tier::Opus774)
            .read_to_end(&mut Vec::new())
            .expect("a whole download");

        let calls = fake.calls();
        assert_eq!(calls.len(), 2);
        for call in calls {
            assert!(call.iter().any(|argument| argument == "--ignore-config"));
            assert!(call.iter().any(|argument| argument == "--no-playlist"));
            for converting in NO_ARGUMENT_MAY_CONVERT {
                assert!(
                    !call.iter().any(|argument| argument == converting),
                    "{call:?}"
                );
            }
        }
    }

    #[test]
    fn a_cookie_value_with_spaces_reaches_the_tool_as_one_argument_on_every_call() {
        let fake = Fake::answering(r#"echo '{"formats":[]}'"#);
        let tool = fake.tool(Cookies::read("firefox:/home/a/zen (b)"));

        tool.formats(&song()).expect("a listing");
        opened(&tool, Tier::Opus774)
            .read_to_end(&mut Vec::new())
            .expect("a whole download");

        for call in fake.calls() {
            let at = call
                .iter()
                .position(|argument| argument == "--cookies-from-browser")
                .expect("the browser asked");
            assert_eq!(call[at + 1], "firefox:/home/a/zen (b)");
        }
    }

    #[test]
    fn a_cookie_file_is_named_to_the_tool_and_no_cookie_flag_goes_out_without_a_value() {
        let fake = Fake::answering(r#"echo '{"formats":[]}'"#);

        fake.tool(Cookies::read("/home/a/cookies file.txt"))
            .formats(&song())
            .expect("a listing");
        fake.tool(Cookies::None)
            .formats(&song())
            .expect("a listing");

        let calls = fake.calls();
        let at = calls[0]
            .iter()
            .position(|argument| argument == "--cookies")
            .expect("the file named");
        assert_eq!(calls[0][at + 1], "/home/a/cookies file.txt");
        assert!(!calls[1].iter().any(|argument| argument.contains("cookies")));
    }

    #[test]
    fn a_download_that_dies_part_way_is_an_error_and_never_an_end_of_file() {
        let fake = Fake::answering("printf part\nexit 1");
        let mut reader = opened(&fake.tool(Cookies::None), Tier::Opus774);
        let mut read = Vec::new();

        let died = reader.read_to_end(&mut read).expect_err("a cut download");

        assert_eq!(read, b"part");
        assert!(matches!(
            told_by(&died),
            Some(Error::ToolFailed {
                op: ProviderOp::Download,
                code: Some(1),
                ..
            })
        ));
        assert!(reader.read(&mut [0; 8]).is_err());
    }

    #[test]
    fn a_hung_tool_is_killed_past_its_limit() {
        let fake = Fake::answering("echo $$ > \"$here/pid\"\nsleep 6");
        let tool = fake.tool(Cookies::None).waiting_at_most(IMPATIENT);
        let began = Instant::now();

        let refused = tool
            .formats(&song())
            .expect_err("a tool that never answers");

        assert!(
            began.elapsed() < GIVEN_UP_WELL_BEFORE,
            "{:?}",
            began.elapsed()
        );
        assert!(!still_running(&fake));
        assert!(refused.is_the_provider_away());
        assert!(matches!(
            refused,
            Error::Io {
                op: ProviderOp::Search,
                ..
            }
        ));
    }

    #[test]
    fn a_tool_hung_before_its_first_byte_is_killed_past_its_limit() {
        let fake = Fake::answering("echo $$ > \"$here/pid\"\nsleep 6");
        let tool = fake.tool(Cookies::None).waiting_at_most(IMPATIENT);
        let began = Instant::now();

        let refused = tool.download(&song(), Tier::Opus774);

        assert!(
            began.elapsed() < GIVEN_UP_WELL_BEFORE,
            "{:?}",
            began.elapsed()
        );
        assert!(!still_running(&fake));
        assert!(matches!(
            refused,
            Err(Error::Io {
                op: ProviderOp::Download,
                ..
            })
        ));
    }

    #[test]
    fn a_download_gone_quiet_is_killed_and_an_error_though_no_whole_body_limit_binds_it() {
        let fake = Fake::answering("echo $$ > \"$here/pid\"\nprintf part\nsleep 6");
        let tool = fake.tool(Cookies::None).waiting_at_most(IMPATIENT);
        let mut reader = opened(&tool, Tier::Opus774);
        let began = Instant::now();

        let stalled = reader
            .read_to_end(&mut Vec::new())
            .expect_err("a stalled download");

        assert_eq!(stalled.kind(), io::ErrorKind::TimedOut);
        assert!(told_by(&stalled).is_some_and(Error::is_the_provider_away));
        assert!(
            began.elapsed() < GIVEN_UP_WELL_BEFORE,
            "{:?}",
            began.elapsed()
        );
        drop(reader);
        assert!(!still_running(&fake));
    }

    #[test]
    fn a_download_that_keeps_sending_outlasts_the_limit_on_silence() {
        let fake = Fake::answering("for part in 1 2 3 4 5 6; do printf x; sleep 0.1; done");
        let tool = fake.tool(Cookies::None).waiting_at_most(IMPATIENT);
        let mut read = Vec::new();

        opened(&tool, Tier::Opus774)
            .read_to_end(&mut read)
            .expect("a slow and whole download");

        assert_eq!(read, b"xxxxxx");
    }

    #[test]
    fn a_reader_dropped_part_way_kills_the_tool() {
        let fake = Fake::answering("echo $$ > \"$here/pid\"\nexec yes resonate");
        let mut reader = opened(&fake.tool(Cookies::None), Tier::Opus774);
        reader.read_exact(&mut [0; 16]).expect("the first bytes");

        assert!(still_running(&fake));
        drop(reader);

        assert!(!still_running(&fake));
    }

    #[test]
    fn a_video_gone_or_private_opens_as_gone() {
        for said in [
            "ERROR: [youtube] abc: Private video. Sign in if you have been granted access",
            "ERROR: [youtube] abc: Video unavailable",
            "ERROR: [youtube] abc: This video is not available",
        ] {
            let fake = Fake::answering(&format!("echo '{said}' >&2\nexit 1"));
            let tool = fake.tool(Cookies::None);

            assert!(matches!(
                tool.download(&song(), Tier::Opus774),
                Ok(Opened::Gone)
            ));
            assert_eq!(
                tool.formats(&song()).expect("no formats"),
                Formats::default()
            );
        }
    }

    #[test]
    fn a_video_refused_for_its_own_reason_opens_as_gone_and_lists_no_tier() {
        for reason in [
            "This video has been removed by the uploader",
            "This video is DRM protected",
            "Requested format is not available. Use --list-formats for a list of available formats",
        ] {
            let fake = Fake::answering(&format!(
                "echo \"ERROR: [youtube] {SONG}: {reason}\" >&2\nexit 1"
            ));
            let tool = fake.tool(Cookies::None);

            assert!(
                matches!(tool.download(&song(), Tier::Opus774), Ok(Opened::Gone)),
                "{reason}"
            );
            assert_eq!(
                tool.formats(&song()).expect("no formats"),
                Formats::default(),
                "{reason}"
            );
        }
    }

    #[test]
    fn trouble_of_the_account_or_the_network_is_no_verdict_on_the_video() {
        for reason in [
            "Sign in to confirm you're not a bot. Use --cookies-from-browser",
            "Unable to download API page: HTTP Error 503: Service Unavailable",
            "Unable to download webpage: <urlopen error [Errno -3] name resolution>",
            "Video unavailable. This content isn't available, try again later. The current session has been rate-limited by YouTube for up to an hour",
            "Failed to extract any player response; please report this issue on https://github.com/yt-dlp/yt-dlp/issues",
            "All player responses are invalid. Your IP is likely being blocked by Youtube",
            "Sign in to confirm your age. This video may be inappropriate for some users.",
            "This video is only available to Music Premium members",
            "Join this channel to get access to members-only content like this video",
            "The uploader has not made this video available in your country",
            "This video is not available in your location",
        ] {
            let fake = Fake::answering(&format!(
                "cat >&2 <<'SAID'\nERROR: [youtube] {SONG}: {reason}\nSAID\nexit 1"
            ));
            let tool = fake.tool(Cookies::None);

            let listing = tool.formats(&song()).expect_err("no verdict");
            let download = tool.download(&song(), Tier::Opus774);

            assert!(listing.is_the_provider_away(), "{reason}");
            assert!(matches!(listing, Error::ToolFailed { .. }), "{reason}");
            assert!(
                matches!(download, Err(Error::ToolFailed { .. })),
                "{reason}"
            );
        }
    }

    #[test]
    fn a_listing_cut_short_by_a_failed_challenge_or_storyboards_alone_is_no_verdict_on_the_video() {
        for warned in [
            "WARNING: [youtube] {SONG}: n challenge solving failed: Some formats may be missing",
            "WARNING: [youtube] {SONG}: Signature solving failed: Some formats may be missing",
            "WARNING: Only images are available for download. use --list-formats to see them",
        ] {
            let fake = Fake::answering(&format!(
                "cat >&2 <<'SAID'\n{warned}\nERROR: [youtube] {SONG}: Requested format is not available. Use --list-formats for a list of available formats\nSAID\nexit 1"
            ));
            let tool = fake.tool(Cookies::None);

            let listing = tool.formats(&song()).expect_err("no verdict");
            let download = tool.download(&song(), Tier::Opus774);

            assert!(listing.is_the_provider_away(), "{warned}");
            assert!(
                matches!(download, Err(Error::ToolFailed { .. })),
                "{warned}"
            );
        }
    }

    #[test]
    fn a_tool_that_never_reached_the_video_is_the_tool_failed() {
        for said in [
            "yt-dlp: error: unsupported browser specified for cookies: \"zen\"",
            "Traceback (most recent call last):",
            "ERROR: [youtube] another-id: Video withdrawn",
        ] {
            let fake = Fake::answering(&format!("cat >&2 <<'SAID'\n{said}\nSAID\nexit 2"));
            let tool = fake.tool(Cookies::None);

            assert!(
                matches!(tool.formats(&song()), Err(Error::ToolFailed { .. })),
                "{said}"
            );
        }
    }

    #[test]
    fn a_tool_that_exits_well_having_sent_nothing_is_no_download() {
        let fake = Fake::answering("exit 0");

        assert!(matches!(
            fake.tool(Cookies::None).download(&song(), Tier::Opus774),
            Err(Error::ToolFailed {
                op: ProviderOp::Download,
                code: Some(0),
                ..
            })
        ));
    }

    #[test]
    fn a_failure_that_is_no_verdict_on_the_video_is_the_tool_failed_with_its_code() {
        let fake = Fake::answering("echo 'ERROR: something else' >&2\nexit 3");
        let tool = fake.tool(Cookies::None);

        let listing = tool.formats(&song()).expect_err("a failed listing");
        let download = tool.download(&song(), Tier::Opus774);

        assert!(listing.is_the_provider_away());
        assert!(matches!(
            listing,
            Error::ToolFailed {
                op: ProviderOp::Search,
                code: Some(3),
                ..
            }
        ));
        assert!(matches!(
            download,
            Err(Error::ToolFailed {
                op: ProviderOp::Download,
                code: Some(3),
                ..
            })
        ));
    }

    #[test]
    fn lapsed_cookies_are_heard_though_the_tool_says_so_in_a_warning_and_exits_well() {
        let fake = Fake::answering(&format!(
            "{COOKIES_LAPSED}\necho '{{\"formats\":[{{\"format_id\":\"251\"}}]}}'"
        ));

        let refused = fake
            .tool(Cookies::read("firefox"))
            .formats(&song())
            .expect_err("lapsed cookies");

        assert!(refused.is_the_provider_away());
        assert!(matches!(
            refused,
            Error::Unwelcome {
                op: ProviderOp::Search,
                ..
            }
        ));
        assert!(
            !fake.calls()[0]
                .iter()
                .any(|argument| argument == "--no-warnings")
        );
    }

    #[test]
    fn a_lapse_heard_with_nothing_sent_is_unwelcome_though_the_video_reads_as_gone() {
        let fake = Fake::answering(&format!(
            "{COOKIES_LAPSED}\necho 'ERROR: [youtube] abc: Video unavailable' >&2\nexit 1"
        ));

        assert!(matches!(
            fake.tool(Cookies::read("/home/a/cookies.txt"))
                .download(&song(), Tier::Opus774),
            Err(Error::Unwelcome {
                op: ProviderOp::Download,
                ..
            })
        ));
    }

    #[test]
    fn a_lapse_heard_at_the_end_of_a_download_makes_the_next_listing_unwelcome() {
        let fake = Fake::answering(&format!(
            "case \" $* \" in *\" -J \"*) echo '{{\"formats\":[{{\"format_id\":\"251\"}}]}}' ;; *) {COOKIES_LAPSED}; printf bytes ;; esac"
        ));
        let tool = fake.tool(Cookies::read("/home/a/cookies.txt"));
        let mut read = Vec::new();

        tool.formats(&song()).expect("a listing");
        opened(&tool, Tier::Opus251)
            .read_to_end(&mut read)
            .expect("the bytes asked for");
        let refused = tool.formats(&song()).expect_err("cookies heard lapsed");

        assert_eq!(read, b"bytes");
        assert!(matches!(
            refused,
            Error::Unwelcome {
                op: ProviderOp::Search,
                ..
            }
        ));
        assert_eq!(fake.calls().len(), 2);
    }

    #[test]
    fn a_lapse_said_once_holds_for_the_next_listing_though_the_tool_never_says_it_again() {
        let fake = Fake::answering(&format!(
            "[ -e \"$here/warned\" ] || {{ touch \"$here/warned\"; {COOKIES_LAPSED}; }}\necho '{{\"formats\":[{{\"format_id\":\"251\"}}]}}'"
        ));
        let tool = fake.tool(Cookies::read("/home/a/cookies.txt"));

        for _ in 0..2 {
            assert!(matches!(
                tool.formats(&song()),
                Err(Error::Unwelcome {
                    op: ProviderOp::Search,
                    ..
                })
            ));
        }

        assert_eq!(fake.calls().len(), 1);
    }

    #[test]
    fn a_tool_that_writes_a_great_deal_to_its_stderr_is_never_held_up_by_it() {
        let fake = Fake::answering(
            "yes complaint | head -c 1000000 >&2\necho '{\"formats\":[{\"format_id\":\"774\"}]}'",
        );

        let formats = fake
            .tool(Cookies::None)
            .formats(&song())
            .expect("a listing");

        assert_eq!(formats.best, Some(Tier::Opus774));
    }
}
