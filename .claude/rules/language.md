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
  absent locale, then the first of `LC_ALL`, `LC_MESSAGES`, `LANG`, each made a tag; on Windows
  `sys-locale`). The lyrics sidecar reads the same list (`lyrics.md`).
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
  `.ftl`), a message with attributes or no value, an id named twice, an area or language folder
  missing from English or not in it, and a second language's message missing, extra or taking other
  variables than English's. A tag no `Language` carries fails to compile (`Language::by_tag` is a
  `const fn`). **An unused message is `dead_code`**, so clippy's `-D warnings` refuses dead words.
- **A count is a Fluent selector, never `if count == 1`**: `{ $count -> [one] … *[other] … }`,
  English's plural rules from the bundle's locale. A figure whose precision matters (dB, kHz) is
  passed formatted (`format!("{:+.2}")`); numbers are not grouped by locale.
- **One area per subject, kept beside the code that says it**: `crates/resonate-ui/words/en/format.ftl`
  is `format.rs`'s. A literal that is a proper name (a theme's, a service's, a font family) or
  markup is not a word.

## What is not translated

- **An error's `Display` is a diagnostic, English, for the log and the terminal's `{error}`**; the
  window never shows one (`ui.md`'s *Toasts speak plainly*). The exception is `resonate-mobile`'s
  `Error`: it is flat across the boundary, so its message is the phone's sentence and comes out of
  the mobile crate's catalog.
- The CLI grammar (`cli.rs`'s help, the man pages and completions `build.rs` writes) is English.
- Log lines, test names and the desktop entry and metainfo are English.

## Choosing the language

- **`language` in `config.toml`** (`system`, default, or a tag) is read first thing in `run`, which
  calls `speak(config.language_spoken())`, so the terminal, the window, notifications and Discord
  speak it. The Appearance category's *Language* group writes it (`Setting::Language`,
  `ResonateApp::language`); a change is spoken from the next start, and the group says so.
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
`CFBundleLocalizations` on iOS; the `language` key's man page entry names the tag.

## The checks

- **A prose literal fails its crate's test**: `resonate_words_build::prose_in(src, PROPER)` walks a
  crate's sources and answers each string literal that reads as words (a capitalised word, or a word
  beside a space once `{…}` placeholders are taken out) outside tests, attributes, panics, asserts,
  `expect` and `tracing` calls. `resonate-ui`'s is `every_word_the_window_shows_is_said_through_its_catalog`
  (`views/audit.rs`): `PROPER` names the literals that are names, `NOT_WORDS` the files that hold
  none (markup, font families), and `STILL_TO_MOVE` the files not yet moved, which must shrink to
  nothing; a file it names that holds no prose fails the test too.
- **The phones hold the same rule.** Android's `WordsTest` walks `src/main/java` with a Kotlin reader
  of the same heuristic, quiet inside `Log.`, `require`, `check`, `error`, an exception and
  `testTag`, with its own `PROPER` and `STILL_TO_MOVE`. iOS's `test_strings.py` (`words.py`, under
  `python3 -m unittest discover -s ios`) walks `Sources`, quiet inside logging, `print` and the
  preconditions, with its own two lists; it also holds every dotted key a view names
  (`Text("queue.clear")`, `String(localized:)`) to `Localizable.strings` or the stringsdict, and
  `InfoPlist.strings` to the `Info.plist` prompts it speaks.
- `cargo test -p resonate-words -p resonate-words-build` holds negotiation, the plural forms, the
  static plain words and every refusal of the build.
