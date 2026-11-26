use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const RULES: &str = ".claude/rules";
const HUMPS_OF_A_TYPE_AT_LEAST: usize = 2;
const SOURCE_ROOTS: &[&str] = &[
    "crates",
    "fuzz",
    "dsp-lab",
    "android",
    "ios",
    ".github",
    "packaging",
];
const SOURCE_EXTENSIONS: &[&str] = &[
    "rs", "kt", "kts", "swift", "py", "toml", "ftl", "xml", "gradle", "yml", "plist", "service",
    "env", "iss",
];
const NOT_SOURCE: &[&str] = &[
    "target",
    ".git",
    ".gradle",
    "build",
    ".build",
    "node_modules",
];
const OTHER_PROGRAMS_AND_PROTOCOLS: &[&str] = &[
    "ASWebAuthenticationSession",
    "AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY",
    "AUDCLNT_BUFFERFLAGS_SILENT",
    "AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED",
    "AUDCLNT_E_DEVICE_INVALIDATED",
    "AUDCLNT_E_DEVICE_IN_USE",
    "AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED",
    "AUDCLNT_STREAMFLAGS_LOOPBACK",
    "AURenderCallback",
    "AfterFirstUnlockThisDeviceOnly",
    "AfterTrack",
    "AllowReplacement",
    "AnyShape",
    "ApproachingDataLimit",
    "AttachedFile",
    "AudioObject",
    "CAP_LEASE",
    "CARGO_CFG_TARGET_ARCH",
    "CFRunLoopRunInMode",
    "CLOCK_MONOTONIC",
    "CTRL_CLOSE_EVENT",
    "CoInitializeEx",
    "CodecDelay",
    "CompanyName",
    "CopyFileExW",
    "CreateObject",
    "CueClusterPosition",
    "DataException",
    "DeleteAll",
    "DeletionDate",
    "DestroyObject",
    "DiscardPadding",
    "ERROR_LOCK_VIOLATION",
    "ERROR_NO_DATA",
    "E_NOTFOUND",
    "EditionEntry",
    "EditionUID",
    "FALLOC_FL_INSERT_RANGE",
    "FILE_FLAG_BACKUP_SEMANTICS",
    "FILE_FLAG_DELETE_ON_CLOSE",
    "FILE_FLAG_FIRST_PIPE_INSTANCE",
    "FileUID",
    "FileVersion",
    "FlushFileBuffers",
    "GetBuffer",
    "GetBufferSize",
    "GetForCurrentView",
    "GetForWindow",
    "GetFrequency",
    "GetPosition",
    "IAudioClient",
    "IAudioClock",
    "IAudioMeterInformation",
    "IFileOperation",
    "IPV6_V6ONLY",
    "IS_MUSIC",
    "ImportResource",
    "InterfacesAdded",
    "IsFormatSupported",
    "KEYCODE_MEDIA_PLAY",
    "LegalCopyright",
    "LengthN",
    "LineWrapper",
    "LinkMetadataDT",
    "LiveRegion",
    "LockFileEx",
    "MAX_PATH",
    "MTKView",
    "MethodError",
    "MouseExited",
    "MultiSpeakerBRIR",
    "MusicTrackMediaMetadata",
    "NSFileManager",
    "NameLost",
    "OPUS_GET_PITCH",
    "O_NOFOLLOW",
    "O_PATH",
    "OriginalFilename",
    "OverDataLimit",
    "OwnedHandle",
    "PIPE_NOWAIT",
    "PW_KEY_NODE_RATE",
    "PW_KEY_TARGET_OBJECT",
    "PathBuilder",
    "ProductName",
    "ProductVersion",
    "RPC_E_CHANGED_MODE",
    "ReadFileEx",
    "ReleaseBuffer",
    "ReservedRegion",
    "RoInitialize",
    "SCROLL_LINES",
    "SO_PEERCRED",
    "SO_REUSEADDR",
    "SQLITE_BUSY_SNAPSHOT",
    "STATUS_DLL_NOT_FOUND",
    "SamplingFrequency",
    "SeCreateSymbolicLinkPrivilege",
    "SeekHead",
    "SeekId",
    "SetThreadExecutionState",
    "ShaderLibrary",
    "SimpleFreeFieldHRIR",
    "StdExternalCrate",
    "SupportedMimeTypes",
    "TimelineView",
    "TitleN",
    "TrackEntry",
    "UIDevice",
    "UIMenu",
    "UIPanGestureRecognizer",
    "UnsatisfiedLinkError",
    "WindowVisibility",
    "XF86Back",
    "accesskit_unix",
    "content_size",
    "destination_id",
    "hoverable_tooltip",
    "in_focus",
    "load_icon",
    "matrix_mixdown_idx",
    "nw_path_monitor",
    "payload_binary",
    "payload_type",
    "payload_utf8",
    "pitch_downsample",
    "pitch_search",
    "pre_exec",
    "pseudo_surround",
    "pw_stream_destroy",
    "pw_stream_update_properties",
    "remove_doubling",
    "replaces_id",
    "spa_hook_remove",
    "sqlite_stat1",
    "sun_path",
    "thread_loop",
    "web_music",
    "with_guessed_format",
    "workflow_dispatch",
    "xkb_keysym_get_name",
    "zh_yue",
];
const CRATES_AND_THEIR_FEATURES: &[&str] = &[
    "CoreAudioBaseTypes",
    "accesskit_macos",
    "coinit_apartmentthreaded",
    "const_generics",
    "gpui_apple",
    "gpui_macos",
    "id3v2_compression_support",
    "lock_api",
    "macos_kqueue",
    "miniz_oxide",
    "parking_lot_core",
    "rust_backend",
    "rwh_06",
    "safe_arch",
    "v0_3_50",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

fn source_files(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();

        if NOT_SOURCE.iter().any(|skipped| name == *skipped) {
            continue;
        }
        if path.is_dir() {
            source_files(&path, found);
        } else if path
            .extension()
            .is_some_and(|extension| SOURCE_EXTENSIONS.iter().any(|known| extension == *known))
        {
            found.push(path);
        }
    }
}

fn written_names(root: &Path) -> BTreeSet<String> {
    let mut files = Vec::new();

    for folder in SOURCE_ROOTS {
        source_files(&root.join(folder), &mut files);
    }

    let stems = files
        .iter()
        .filter_map(|file| file.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned());
    let words: Vec<String> = files
        .iter()
        .filter_map(|file| fs::read_to_string(file).ok())
        .flat_map(|text| {
            text.split(|letter: char| !letter.is_ascii_alphanumeric() && letter != '_')
                .filter(|word| !word.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect();

    stems.chain(words).collect()
}

fn is_snake_shaped(word: &str, cased: fn(&char) -> bool) -> bool {
    word.contains('_')
        && !word.starts_with('_')
        && !word.ends_with('_')
        && !word.contains("__")
        && word.starts_with(|first: char| cased(&first))
        && word
            .chars()
            .all(|letter| cased(&letter) || letter.is_ascii_digit() || letter == '_')
}

fn is_camel_case_name(word: &str) -> bool {
    let humps = word.chars().filter(char::is_ascii_uppercase).count();

    word.starts_with(|first: char| first.is_ascii_uppercase())
        && word.chars().all(|letter| letter.is_ascii_alphanumeric())
        && word.chars().any(|letter| letter.is_ascii_lowercase())
        && humps >= HUMPS_OF_A_TYPE_AT_LEAST
}

fn is_a_name(word: &str) -> bool {
    is_snake_shaped(word, char::is_ascii_lowercase)
        || is_snake_shaped(word, char::is_ascii_uppercase)
        || is_camel_case_name(word)
}

fn name_of(span: &str) -> Option<&str> {
    let called = span.strip_suffix("()").unwrap_or(span);

    if !called
        .chars()
        .all(|letter| letter.is_ascii_alphanumeric() || letter == '_' || letter == ':')
    {
        return None;
    }

    let last = called.rsplit("::").next()?;
    is_a_name(last).then_some(last)
}

fn written_by_others(name: &str) -> bool {
    OTHER_PROGRAMS_AND_PROTOCOLS.contains(&name) || CRATES_AND_THEIR_FEATURES.contains(&name)
}

fn names_in(rule: &str) -> BTreeSet<String> {
    rule.split('`')
        .skip(1)
        .step_by(2)
        .filter_map(name_of)
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_function_a_rule_names_is_written_somewhere_in_the_source() {
    let root = workspace();
    let written = written_names(&root);

    let mut missing = Vec::new();
    let mut named = BTreeSet::new();
    let rules = fs::read_dir(root.join(RULES)).expect("the rules folder");

    for entry in rules.flatten() {
        let path = entry.path();

        if path.extension().is_none_or(|extension| extension != "md") {
            continue;
        }

        let text = fs::read_to_string(&path).expect("a readable rule");
        for name in names_in(&text) {
            if !written_by_others(&name) && !written.contains(&name) {
                missing.push(format!("{} names {name}", entry.file_name().display()));
            }
            named.insert(name);
        }
    }

    let unnamed: Vec<&str> = OTHER_PROGRAMS_AND_PROTOCOLS
        .iter()
        .chain(CRATES_AND_THEIR_FEATURES)
        .copied()
        .filter(|name| !named.contains(*name))
        .collect();

    assert!(
        unnamed.is_empty(),
        "names set aside as other programs' that no rule names:\n{}",
        unnamed.join("\n")
    );

    assert!(
        missing.is_empty(),
        "rules name code that is gone:\n{}",
        missing.join("\n")
    );
}

#[test]
fn a_name_is_told_from_a_word() {
    let spans = [
        ("foo_bar", Some("foo_bar")),
        ("foo_bar()", Some("foo_bar")),
        ("Shelf::lists_every_item", Some("lists_every_item")),
        ("foo", None),
        ("Foo_bar", None),
        ("foo_bar.rs", None),
        ("--foo_bar", None),
        ("foo_bar(x)", None),
        ("THE_BEST_COPY", Some("THE_BEST_COPY")),
        ("Error::LedgerVersion", Some("LedgerVersion")),
        ("UpNextList", Some("UpNextList")),
        ("Mirror", None),
        ("UTF", None),
        ("_HIDDEN", None),
        ("B_S(span)", None),
    ];

    for (span, expected) in spans {
        assert_eq!(name_of(span), expected, "{span}");
    }
}
