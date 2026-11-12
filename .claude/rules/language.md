# Language

Every word a listener reads comes out of a catalog, never a literal at the place it is drawn. English
is the only language shipped; adding another is adding files and one enum variant, not touching
the code that says the words.

## The Rust catalog

- **`resonate-words` is the runtime and reaches no workspace crate.** `Language` (`English`, its
  `tag`, its `endonym`, which is never translated), `LanguageChoice` (`System` or `Chosen`, the
  `language` key's type), `negotiated` (the first tag a listener reads whose primary language is
  spoken here, else English), `speak`/`spoken` (the one process-wide language, an atomic) and
  `languages_the_listener_reads` (on Unix `LANGUAGE`'s list, passed over under a `C`/`POSIX` or
  absent locale, then the first of `LC_ALL`, `LC_MESSAGES`, `LANG`, each made a tag; on Windows and
  macOS `sys-locale`, since a window started from Finder has no such variable and a terminal's
  `LANG` there follows the region, not the languages the listener put in order). The lyrics sidecar
  reads the same list (`lyrics.md`).
- **`Area` holds one `.ftl` file in every language that ships it.** A bundle per language is built
  on first use (`fluent-bundle`'s concurrent bundle, isolation marks off), so English pays nothing
  for languages not spoken. `plain(index)` formats a message with no variables once per language and
  keeps it, so it is `&'static str` (`Choice::label`, a const-like label stay static);
  `formatted(index, args)` formats each call. A message missing from the spoken language is said in
  English and logged; one missing from English says its id.
- **`resonate-words-build` writes the functions a crate says its words through.** A crate with
  words holds `words/<tag>/<area>.ftl` and a `build.rs` calling `resonate_words_build::write()`;
  `mod words { include!(concat!(env!("OUT_DIR"), "/words.rs")); }` takes in a module per area and a
  `pub(crate) fn` per message: `words::format::just_now() -> &'static str`,
  `words::format::days_ago(count) -> String`, a parameter per variable the English message uses
  (through referenced messages too), each `impl Into<Arg>` (numbers, `&str`, `String`, `Cow`). An id
  is kebab-case and becomes snake_case; a keyword is raised (`r#type`).
- **The build refuses what would rot**: a parse error, a comment (the no-comment rule holds in
  `.ftl`; every other source is walked by the test `rust-style.md` names), a message with attributes or no value, an id named twice, an area or language folder
  missing from English or not in it, and a second language's message missing, extra or taking other
  variables than English's. A tag no `Language` carries fails to compile (`Language::by_tag` is a
  `const fn`). **An unused message is `dead_code`**, so clippy's `-D warnings` refuses dead words.
- **A count is a Fluent selector, never `if count == 1`**: `{ $count -> [one] … *[other] … }`,
  English's plural rules from the bundle's locale. A figure whose precision matters (dB, kHz) is
  passed formatted (`format!("{:+.2}")`); numbers are not grouped by locale.
- **One area per subject, kept beside the code that says it**: `crates/resonate-ui/words/en/format.ftl`
  is `format.rs`'s. A literal that is a proper name (a theme's, a service's, a font family) or
  markup is not a word.
- **An area's name is snake_case, so a file of the settings pane speaks through
  `settings_<file>.ftl`** (`settings_pane.ftl` for its `mod.rs`): a hyphen in an area's name is
  refused, and the prefix keeps the pane's areas apart from the window's own (`equaliser.ftl` beside
  `settings_equaliser.ftl`). A message naming the sound system takes it as `$system`
  (`sound_system!()`), and a sentence naming a speaker selects on `$speaker` (`left`, `right`).

## What is not translated

- **An error's `Display` is a diagnostic, English, for the log and the terminal's `{error}`**; the
  window never shows one (`ui.md`'s *Toasts speak plainly*). The exception is `resonate-mobile`'s
  `Error`: it is flat across the boundary, so its message is the phone's sentence and comes out of
  the mobile crate's catalog.
- What clap and clap_mangen say with no seam to say it otherwise: clap's errors, its `[default: …]`
  and `[possible values: …]` notes, the `help` subcommand's line. The completions are English, one
  file a shell (`binary.md`).
- Log lines and test names are English.

## Choosing the language

- **`language` in `config.toml`** (`system`, default, or a tag) is read first thing in `run`, which
  calls `speak(config.language_spoken())`, so the terminal, the window, notifications and Discord
  speak it. The help is said before the settings are read, so `run` first speaks
  `config::spoken_before_the_grammar`, the `language` key alone (`binary.md`). The Appearance
  category's *Language* group writes it (`Setting::Language`, `ResonateApp::language`); a change is
  spoken from the next start, and the group says so.
- **The grammar, the man pages, the desktop entry and the metainfo come out of `resonate-cli`'s
  catalog.** Every help and about is `words::grammar::…` (`grammar.ftl`), clap's headings, flags and
  clap_mangen's section headings `words::said::…`, and the binary's `build.rs` writes each
  language's man pages by speaking it in turn (English in the output folder, any other under
  `<tag>/`). The desktop entry and the metainfo are files the packages install as they are, so
  their words are the catalog's `presented.ftl` (`presented`) and
  `the_desktop_entry_and_the_metainfo_say_what_the_catalog_says_in_every_language` holds them to
  it: English as the plain keys and elements, any other language as `GenericName[tag]`,
  `Comment[tag]`, `Keywords[tag]` and `<summary xml:lang="tag">` and `<p xml:lang="tag">`.
- **The phones choose their own**: `speak_languages(tags)` at the boundary answers the tag it
  settled on, so what Rust sends (errors, failures, presets, Wrapped, mix reasons) is in the phone's
  language. Android tells it the configuration's locales as the player opens and again from
  `ResonateApplication.onConfigurationChanged` (`SpokenLanguages`); iOS tells it
  `Locale.preferredLanguages` as the player opens, since iOS ends an app whose language changes.
  Their own words are each platform's resources, the language being the system's per-app setting:
  Android's `res/values/strings.xml` (`<plurals>` for a count; `generateLocaleConfig` and
  `res/resources.properties`' `unqualifiedResLocale=en` offer the per-app choice), iOS's
  `Resonate/en.lproj` (`Localizable.strings`, `Localizable.stringsdict` for a count, `InfoPlist.strings`
  for the prompts and the name; `defaultLocalization` in `Package.swift`, `CFBundleDevelopmentRegion`
  and `CFBundleLocalizations` in `Info.plist`, the folder copied by `xtool.yml`'s `resources`).

## Adding a language

A `Language` variant (tag, endonym, `ALL`, `by_tag`); `words/<tag>/` beside every crate's `en/` with
every area and message; `values-<tag>/strings.xml` on Android; `<tag>.lproj` and
`CFBundleLocalizations` on iOS; the `language` key's man page entry names the tag; the desktop
entry's and the metainfo's keys and elements in the tag, as the catalog says them; the packages
install `<tag>/*.1` from the build's output folder under `man/<tag>/man1`, and a translated copy of
the hand-written `resonate-config.toml(5)` beside the English one.

## The checks

- **A prose literal fails its crate's test**: `resonate_words_build::prose_in(src, PROPER)` walks a
  crate's sources and answers each string literal that reads as words (a capitalised word, or a word
  beside a space once `{…}` placeholders are taken out) outside tests, attributes, panics, asserts,
  `expect` and `tracing` calls. `resonate-ui`'s is `every_word_the_window_shows_is_said_through_its_catalog`
  (`views/audit.rs`): `PROPER` names the literals that are names, `NOT_WORDS` the files that hold
  none (markup, font families; a file mixing markup and words keeps its markup in a module of its
  own, as `wrapped_card/markup.rs`), `ONLY_UNDER_TEST` the test modules kept in files of their own (`models/tests.rs`, `views/root/tests.rs`), and `STILL_TO_MOVE` the files not yet moved, which must shrink
  to nothing; a file it names that holds no prose fails the test too. A string that is a code
  identifier and never shown (a key context, a `Debug` name) is written `stringify!(Search)`, so
  `PROPER` never has to name a word another file shows. `resonate-mobile`'s is
  `every_word_the_phones_show_is_said_through_the_catalog` (`language.rs`), with nothing left to
  move: `PROPER` names the services, platforms and sink names, `ONLY_UNDER_TEST` the test-only
  files, and a literal opening on an SQL keyword is a statement, not words. An `#[error(...)]`
  message is an attribute the walk passes over, so every `Error` sentence there is read by eye:
  each is `words::errors::…` or a `failure.rs` function, never a literal.
- **The binary's is `every_word_the_terminal_says_is_said_through_its_catalog`** (`catalog.rs`):
  `PROPER` names proper names, diagnostics and a stored spelling, `NOT_WORDS` is `error.rs` and
  `awake.rs` (a power request only `powercfg` shows). Its table headings share one `columns` area.
  Its `mod words` allows dead code unless every feature (`ui`, `online`, `sync`, `mcp`, `discord`)
  is on, so the full-feature clippy is the one that refuses a dead word.
- **`resonate-cli`'s are two**: `every_word_the_grammar_says_is_said_through_its_catalog` walks its
  sources through `prose_in`, and since that walk passes over attributes,
  `no_help_the_grammar_gives_is_a_literal_in_an_attribute` refuses a `help = "…"` or `about = "…"`
  in `grammar.rs`. `every_heading_a_man_page_carries_is_said_in_the_language_spoken` renders every
  page and fails on a section heading `man_heading` does not say.
- **Every other crate a listener hears from has one, each a `said_through_the_catalog` module.** A
  crate whose sources hold little else walks them all through `prose_in` with its `PROPER`
  (`resonate-core`'s `every_word_core_hands_a_listener_is_said_through_its_catalog`: theme and
  service names, MusicBrainz's relation keys; `resonate-analysis`'s
  `every_word_a_study_tells_a_listener_is_said_through_its_catalog`; `resonate-discord`'s
  `every_word_discord_shows_for_the_player_is_said_through_its_catalog`). A crate full of protocol,
  SQL or format strings reads only the files that say words through `prose_of` (`SAYS_WORDS`):
  `resonate-library`'s `every_word_the_library_says_is_said_through_its_catalog` (`said.rs`, which
  also passes over a literal reading as SQL: `sql_is_told_apart_from_words`),
  `resonate-mpris`'s `every_word_the_desktop_shows_for_the_player_is_said_through_its_catalog`
  (`idle.rs`, `notify.rs`), `resonate-sync`'s
  `every_word_sync_names_for_a_listener_is_said_through_its_catalog` (`change.rs`,
  `shared/service.rs`), `resonate-eq`'s
  `every_word_the_equaliser_names_for_a_listener_is_said_through_its_catalog`, `resonate-vault`'s
  `every_reason_the_vault_gives_a_listener_is_said_through_its_catalog`, `resonate-upnp`'s
  `every_word_a_renderer_is_shown_by_is_said_through_its_catalog`, `resonate-cast`'s
  `every_word_a_receiver_is_shown_by_is_said_through_its_catalog`, `resonate-coreaudio`'s
  `every_word_a_device_is_shown_by_is_said_through_its_catalog` (`survey.rs`, a Mac's output names),
  `resonate-listen`'s
  `every_word_a_recording_is_named_by_is_said_through_its_catalog` (the PipeWire stream names),
  `resonate-online`'s `every_word_a_spotify_sign_in_shows_a_listener_is_said_through_its_catalog`,
  `resonate-ytmusic`'s `every_word_a_youtube_music_account_names_is_said_through_its_catalog`,
  `resonate-signin`'s `every_word_the_sign_in_window_shows_is_said_through_its_catalog` (`window.rs`,
  whose title the window speaks in the listener's language) and
  `resonate-server`'s `every_word_an_invitation_page_says_is_said_through_its_catalog` (the `/s/` and
  `/p/` pages, whose `lang` is the language spoken). Not words, and left in the code: an error's
  `Display` and what feeds one (`resonate-engine`'s `CommandKind::as_str`), a codec's or
  container's name (`resonate-codec`'s `Codec::as_str`), a stored key (`Tell::as_str`,
  `TagField::as_str`), MCP's tool and prompt descriptions (read by a model, not a listener) and the
  words a reader matches in another program's output (`yt-dlp`'s, AutoEq's index).
- **The phones hold the same rule.** Android's `WordsTest` walks `src/main/java` with a Kotlin reader
  of the same heuristic, quiet inside `Log.`, `require`, `check`, `error`, an exception and
  `testTag`, with its own `PROPER` and `STILL_TO_MOVE`. iOS's `test_strings.py` (`words.py`, under
  `python3 -m unittest discover -s ios`) walks `Sources`, quiet inside logging, `print` and the
  preconditions, with its own two lists; it also holds every dotted key a view names
  (`Text("queue.clear")`, `String(localized:)`) to `Localizable.strings` or the stringsdict, and
  `InfoPlist.strings` to the `Info.plist` prompts it speaks.
- `cargo test -p resonate-words -p resonate-words-build` holds negotiation, the plural forms, the
  static plain words and every refusal of the build.
