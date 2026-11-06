# YouTube Music Provider Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect a YouTube Premium account to Resonate once, then fetch songs as the stream YouTube served, filling wants as a last resort and keeping songs pressed in search and playlists.

**Architecture:** A provider crate `resonate-ytmusic` fills the `Provider` and `PlaysInPlace` seams, searching signed out over InnerTube and running `yt-dlp` for formats, private playlists and the download. A keep is a row of a new `kept_songs` table walked by its own pass, which asks that provider alone and files through the existing `filed` path. `resonate-codec` learns to write Matroska tags, since Opus lands un-remuxed as `.mka`.

**Tech Stack:** Rust 2024, `ureq` through `resonate-fetch`, `serde_json`, rusqlite, GPUI, external `yt-dlp`.

**Spec:** `docs/superpowers/specs/2026-11-06-youtube-music-provider-design.md`

## Global Constraints

- Never re-encode or remux. What lands is the stream YouTube served, byte for byte, but for tags written into the file.
- Tier order `774`, `141`, `251`.
- Nothing runs until `youtube-music` is true. `online = false` registers nothing.
- Song address is always `https://music.youtube.com/watch?v=<id>`.
- No comments of any kind (`rust-style.md`). `rust-formatter`, never `cargo fmt`. Full version pins in manifests.
- Errors per `errors.md`: typed variants, no prose, `size_of::<Error>() <= 128`.
- `parking_lot` mutexes. Test names are sentences in `snake_case`.
- A schema change is a new step appended to `MIGRATIONS`. `V1` and existing steps are never edited.
- Commit only as each task says, on a feature branch, never on `master`.

## Review Focus

1. A `yt-dlp` that hangs or dies part way through a download: the song must not land truncated, and the poll lane must come back. (Task 4, `a_download_that_dies_part_way_is_an_error_and_never_an_end_of_file`, `a_hung_tool_is_killed_past_its_limit`)
2. Cookies lapsing mid-poll: the library must not fill with 134k files. (Task 5, `lapsed_cookies_stop_the_poll_and_leave_the_want_due`)
3. A slow or refused TIDAL with YouTube enabled: a lossy file must not take a FLAC's place. (Task 3, `a_last_resort_offer_is_not_taken_while_an_earlier_provider_was_late`)
4. Keeping a playlist of mostly held songs: no lossy duplicates, and the held copies fill the list in place. (Task 8, `a_song_held_already_is_not_kept_again_and_its_copy_fills_the_list`)
5. A process killed while a keep downloads: the staged file is swept and the song is still queued. (Task 8, `a_keep_killed_part_way_is_taken_up_again_and_its_staging_swept`)

---

### Task 1: Finish the spike

Throwaway. Nothing here is committed but the spec's *What was measured*.

**Files:**
- Modify: `docs/superpowers/specs/2026-11-06-youtube-music-provider-design.md`

- [ ] **Step 1: Ask the listener to run the two cookie commands, in their own terminal**

No agent reads their cookies. Ask them to send the output.

```bash
yt-dlp --cookies-from-browser "firefox:$HOME/.config/zen/PROFILE.Default (release)" --flat-playlist -J --playlist-end 3 "https://music.youtube.com/playlist?list=LM" | python3 -c 'import json,sys;d=json.load(sys.stdin);print([{k:e.get(k) for k in ("id","title","duration","channel","uploader")} for e in d["entries"]])'
yt-dlp --cookies-from-browser "firefox:$HOME/.config/zen/PROFILE.Default (release)" -f 774 -o "$HOME/Music/spike-774.mka" "ALBUM_SONG_URL"
```

- [ ] **Step 2: Check the file decodes under its new name**

Run: `cargo run --no-default-features -- info "$HOME/Music/spike-774.mka"`
Expected: codec Opus, rate 48000, a length.

If it refuses a `webm` DocType, widen the check in `crates/resonate-codec/src/matroska.rs` in Task 2.

- [ ] **Step 3: Record the private playlist's fields in the spec**

If a flat listing names no duration or artist, Task 7's `mix` makes one `-J` call per song, paced.

---

### Task 2: Matroska tags in `resonate-codec`

**Files:**
- Create: `crates/resonate-codec/src/matroska_tags.rs`
- Modify: `crates/resonate-codec/src/writing.rs:1604-1618`, `crates/resonate-codec/src/lib.rs`
- Test: `crates/resonate-codec/tests/encoded.rs`

**Interfaces:**
- Produces: `FileTags::writes` answers true for `.mka`, and `FileTags::write` writes it. No new public names.

lofty has no Matroska and `FileType::from_path` answers `None` for `.mka`, so `writes` and `write` branch on the extension before reaching lofty.

- [ ] **Step 1: Write the failing tests**

In `encoded.rs`, on an Opus WebM made by ffmpeg and named `.mka`, skipped without it:

```rust
#[test]
fn a_matroska_file_is_tagged_and_every_audio_packet_stays_as_it_was() {
    let before = packets_of(&file);
    FileTags::over(Sources::local()).write(&location, title_and_artist("Creep", "Radiohead"))?;
    let read = tags_of(&location);
    assert_eq!(read.title.as_deref(), Some("Creep"));
    assert_eq!(read.artist.as_deref(), Some("Radiohead"));
    assert_eq!(packets_of(&file), before);
}

#[test]
fn a_matroska_tag_written_twice_is_replaced_and_lands_in_the_room_left_for_it() {}

#[test]
fn a_matroska_seek_head_still_finds_every_element_after_a_write() {}

#[test]
fn a_matroska_segment_of_unknown_size_is_refused_rather_than_guessed_at() {}
```

The second asserts one `Tags` element after two writes, with the second write leaving the file's length unchanged while the `Void` holds it.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p resonate-codec --test encoded a_matroska -- --nocapture`
Expected: FAIL, `.mka` is not written.

- [ ] **Step 3: Implement `pub(crate) fn rewritten(head: &[u8], tags: &[(TagField, String)]) -> Result<Option<Vec<u8>>>` in `matroska_tags.rs`**

Reuses `matroska.rs`'s element reading and `TAGS`, `VOID`, `SEEK_HEAD`. Lays a `Tags` element with `Targets` at `TargetTypeValue` 50 and one `SimpleTag` per field, spelled with the keys `tags.rs` reads, followed by a `Void` of `PADDING_LEFT_FOR_THE_NEXT_EDIT`. An existing `Tags` and any `Void` after it are the room. Where the new element fits, it lands in place through `landed_in_place`. Where it does not, a staged copy holds it ahead of the first `Cluster`, correcting the `Segment` size and every `SeekHead` position. Clusters are copied, never parsed for rewriting. Staging, `Held`, read back and `Unwritten::Unconfirmed` are `writing.rs`'s existing paths.

- [ ] **Step 4: Run the tests, then the codec suite**

Run: `cargo test -p resonate-codec`
Expected: PASS.

- [ ] **Step 5: Fuzz seed and rules**

Add a Matroska case to the existing tag-writing fuzz target (`build.md`). In `library.md`, move `.mka` from *passed over* to what `FileTags::writes` answers for.

- [ ] **Step 6: Commit**

```bash
git add crates/resonate-codec fuzz .claude/rules/library.md
git commit -m "Write tags into Matroska files without touching their audio"
```

---

### Task 3: The seam's last resort and a tool's failure

**Files:**
- Modify: `crates/resonate-providers/src/provider.rs`, `crates/resonate-providers/src/error.rs`

**Interfaces:**
- Produces: `Provider::last_resort(&self) -> bool` (default false). `Error::ToolFailed { provider: SourceId, op: ProviderOp, code: Option<i32> }`, provider-away. `PlaysInPlace::plays_songs(&self) -> bool` (default true) in `in_place.rs`.

- [ ] **Step 1: Write the failing tests in `provider.rs`'s test module**

```rust
#[test]
fn a_last_resort_is_not_asked_until_every_provider_before_it_has_answered() {}

#[test]
fn a_last_resort_offer_is_taken_once_every_earlier_provider_offered_nothing() {}

#[test]
fn a_last_resort_offer_is_not_taken_while_an_earlier_provider_was_late() {
    assert!(answer.delivered.is_none());
    assert!(!answer.heard_from_every_provider());
}

#[test]
fn a_last_resort_is_not_asked_where_an_earlier_provider_refused_or_is_away() {}

#[test]
fn a_provider_named_alone_is_asked_though_it_is_a_last_resort() {}
```

In `error.rs`'s test, `ToolFailed` `is_the_provider_away`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p resonate-providers last_resort`
Expected: FAIL to compile.

- [ ] **Step 3: Implement in `Providers::first`**

A last resort's `Turn::Waiting` is skipped unless every turn before it is `Turn::Over` and `answer.refused`, `answer.late` and `answer.passed_over` are all zero. Where they are not, its turn goes `Over` unasked and is counted in `passed_over`, so the want stays due. `apart` and `grace` do not apply to it. A narrowed registry has nothing before it. Add `ToolFailed` to the `true` arm of `is_the_provider_away`.

- [ ] **Step 4: Run the seam's tests**

Run: `cargo test -p resonate-providers`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/resonate-providers
git commit -m "Let a provider answer only once every earlier one said no"
```

---

### Task 4: The crate, its tool, and a song's bytes

**Files:**
- Create: `crates/providers/ytmusic/Cargo.toml`, `src/lib.rs`, `src/tool.rs`, `tests/tool.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- Consumes: `Error::ToolFailed` (Task 3).
- Produces:
  - `pub enum Cookies { None, Browser(String), File(PathBuf) }` and `Cookies::read(&str) -> Self`
  - `pub struct Tool` with `Tool::new(program: Option<PathBuf>, cookies: Cookies) -> Self`
  - `Tool::formats(&self, video: &VideoId) -> Result<Formats>`
  - `Tool::download(&self, video: &VideoId, format: Tier) -> Result<Opened>`
  - `pub struct VideoId` with `VideoId::read(&str) -> Option<Self>` (letters, digits, `-`, `_`, at most 16)
  - `pub enum Tier { Opus774, Aac141, Opus251 }` with `Tier::extension(self) -> &'static str` (`mka`, `m4a`, `mka`) and `Tier::is_premium(self) -> bool`
  - `pub struct Formats { pub best: Option<Tier>, pub length: Option<u64> }`

Depends on `resonate-core`, `resonate-fetch`, `resonate-providers`, `serde`, `serde_json`, `tracing`, `ureq`. `Cookies`' `Debug` prints `<withheld>`.

- [ ] **Step 1: Write the failing tests against a fake `yt-dlp` script in `tests/tool.rs`**

```rust
#[test]
fn a_browser_with_a_profile_is_a_browser_and_any_other_path_is_a_file() {
    assert_eq!(Cookies::read("firefox:/home/a/zen (b)"), Cookies::Browser("firefox:/home/a/zen (b)".into()));
    assert_eq!(Cookies::read("/home/a/cookies.txt"), Cookies::File("/home/a/cookies.txt".into()));
    assert_eq!(Cookies::read("  "), Cookies::None);
}

#[test]
fn the_best_tier_listed_is_taken_in_the_order_774_141_251() {}

#[test]
fn a_song_with_none_of_the_tiers_has_no_best() {}

#[test]
fn the_bytes_read_are_exactly_what_the_tool_wrote_to_its_stdout() {}

#[test]
fn a_download_that_dies_part_way_is_an_error_and_never_an_end_of_file() {
    assert!(matches!(read_to_end(reader), Err(_)));
}

#[test]
fn a_hung_tool_is_killed_past_its_limit() {}

#[test]
fn a_reader_dropped_part_way_kills_the_tool() {}

#[test]
fn a_tool_not_there_is_the_provider_away() {}

#[test]
fn a_video_gone_or_private_opens_as_gone() {}

#[test]
fn an_id_that_would_leave_the_address_is_never_asked() {}

#[test]
fn a_cookie_value_is_withheld_from_debug() {}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p resonate-ytmusic --test tool`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `tool.rs`**

`formats` runs `yt-dlp --no-warnings -J --skip-download <cookies> <url>` and reads `formats[].format_id` and `filesize`. `download` runs `yt-dlp -f <id> -o - <url>`. `--js-runtimes` is passed only where `deno` is not on `PATH`, naming the first of `node`, `bun`. The reader wraps the `Child`: at end of stdout it waits on the child and a non-zero exit is an `io::Error`. `Drop` kills and reaps. Stderr is drained on a thread into a bounded buffer and to `tracing` at debug. `ASKED_WITHIN` (60 s) bounds `formats`. A download has no whole-body limit: `Silence` already bounds it. Private, deleted and unavailable videos are told apart from other failures by stderr's `Private video`, `Video unavailable`, `This video is not available`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p resonate-ytmusic`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/providers/ytmusic
git commit -m "Add resonate-ytmusic and run yt-dlp as its tool"
```

---

### Task 5: Search and answering a want

**Files:**
- Create: `crates/providers/ytmusic/src/search.rs`, `tests/provider.rs`
- Modify: `crates/providers/ytmusic/src/lib.rs`

**Interfaces:**
- Consumes: `Tool`, `Tier`, `VideoId` (Task 4). `Provider::last_resort` (Task 3).
- Produces:
  - `pub struct YtMusic` with `YtMusic::new(tool: Tool) -> Self` and `YtMusic::at(tool: Tool, music: &str) -> Self` for a fake server
  - `impl Provider for YtMusic`: `source()` is `ytmusic`, `last_resort()` is true
  - `pub(crate) struct Row { video, title, artist, album, length, explicit, thumbnail }`
  - `pub(crate) fn songs(&self, words: &str) -> Result<Vec<Row>>`

- [ ] **Step 1: Save a captured answer as a fixture and write the failing tests**

Fixture: `tests/fixtures/songs.json`, trimmed from a real `search` answer. Tests serve it on loopback.

```rust
#[test]
fn a_song_row_names_its_video_title_artist_album_length_and_badge() {
    assert_eq!(rows[0].artist.as_deref(), Some("Radiohead"));
    assert_eq!(rows[0].length, Some(Duration::from_secs(239)));
    assert!(rows[0].explicit);
}

#[test]
fn a_row_that_does_not_read_is_passed_over() {}

#[test]
fn a_want_named_alike_is_offered_and_opened_only_when_taken() {}

#[test]
fn a_want_with_a_youtube_music_link_takes_that_video_with_no_search() {}

#[test]
fn a_want_with_a_plain_youtube_link_is_searched_for() {}

#[test]
fn a_song_with_no_wanted_tier_is_nothing() {}

#[test]
fn a_clean_row_is_never_taken() {}

#[test]
fn lapsed_cookies_stop_the_poll_and_leave_the_want_due() {
    assert!(error.is_the_provider_away());
}

#[test]
fn a_song_short_of_premium_is_taken_where_the_account_still_has_it() {}

#[test]
fn a_song_short_of_premium_is_refused_where_the_account_has_lost_it() {
    assert!(error.is_the_provider_away());
}

#[test]
fn the_account_is_asked_about_once_a_poll_however_many_songs_lack_premium() {}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p resonate-ytmusic --test provider`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`songs` POSTs to `<music>/youtubei/v1/search?prettyPrint=false` with `{"context":{"client":{"clientName":"WEB_REMIX","clientVersion":<read>,"hl":"en"}},"query":<words>,"params":"EgWKAQIIAWoKEAkQBRAKEAMQBA=="}` and header `Origin: https://music.youtube.com`, through `resonate_fetch::asking_agent`, paced by a `Pacing` at `ASKED_APART` (2 s), at most `LISTED_AT_MOST` (25). The client version is read once from `INNERTUBE_CONTEXT_CLIENT_VERSION` on the home page and kept, as `resonate-online`'s `youtube.rs` reads it. Each `musicResponsiveListItemRenderer` gives `playlistItemData.videoId`, the first flex column as title, the second split on ` • ` as artist, album, `m:ss`, and `badges[].musicInlineBadgeRenderer.icon.iconType == "MUSIC_EXPLICIT_BADGE"` as explicit, read as `Rating::flagged`. A row's artist split on ` & ` and `, ` fills `Listed::artists`.

`find`: `may_be_listed` first; `track_on(Service::YoutubeMusic)` next; then `wordings` through a `Choosing`. Then `Tool::formats`. A stderr holding `The provided YouTube account cookies are no longer valid` is `Error::Unwelcome` under `ProviderOp::Search`. Where cookies are set and `best` is not premium, `account_still_premium()` probes `CHECKED_WITH` (an album song's id, a `const`), once a poll, cached for `A_POLL_LASTS` (10 min): lost is `Error::Unwelcome`, kept is a warn and the song is taken. The offer is `Delivery::Stream { key: "track/<id>", extension: tier.extension(), opening }`. Downloads hold one `parking_lot::Mutex` inside the crate through the reader's life, and `downloads_at_once` is not declared. The tier taken is logged at info.

- [ ] **Step 4: Run the crate's tests and clippy**

Run: `cargo test -p resonate-ytmusic && cargo clippy -p resonate-ytmusic --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/providers/ytmusic
git commit -m "Search YouTube Music and answer a want as a last resort"
```

---

### Task 6: Settings, registration and the connection check

**Files:**
- Modify: `crates/resonate/src/config.rs`, `crates/resonate/src/error.rs`, `crates/resonate/src/providers.rs`, `crates/resonate/Cargo.toml`, `crates/resonate-ui/src/settings.rs`, `crates/resonate-ui/src/views/settings/find.rs`
- Create: `crates/resonate-ui/src/views/settings/youtube_music.rs`

**Interfaces:**
- Consumes: `YtMusic`, `Tool`, `Cookies`.
- Produces:
  - Config keys `youtube-music: bool` (false), `youtube-music-cookies: String`, `yt-dlp: String`, `youtube-music-keep-order: bool` (true), each on `Config` and `Online`
  - `Accounts.ytmusic: Option<YtMusicAccount>`, `Made.ytmusic`
  - `YtMusic::check(&self) -> Connection`, `pub enum Connection { Premium, SignedInWithoutPremium, NotSignedIn, ToolMissing }`

- [ ] **Step 1: Write the failing tests in `providers.rs`'s and `config.rs`'s test modules**

```rust
#[test]
fn youtube_music_is_registered_only_where_it_is_turned_on_and_online_is_on() {}

#[test]
fn youtube_music_is_registered_last() {
    assert_eq!(providers.names().last().map(SourceId::as_str), Some("ytmusic"));
}

#[test]
fn a_cookie_setting_is_withheld_from_the_configs_debug() {}

#[test]
fn a_setting_read_from_the_file_is_the_setting_the_pane_writes() {}
```

In `resonate-ytmusic`: `a_connection_is_told_premium_from_signed_in_from_signed_out`, against the fake tool.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p resonate --no-default-features --features online youtube_music`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Follow *Writing one* in `providers.md`: `ConfigKey`, `Config`, `Online`, `Accounts::of`, `Accounts::given`, `Kept` cell, `.and(..)` after Monochrome. `in_place` waits for Task 7. `Group::YoutubeMusic` in the Library category holds the switch, the cookies field (masked as `HifiSecret`), the program field, the order switch and *Check connection*. Copy, exactly: *Saving songs this way breaks YouTube's terms, even on a paid account, and uses this account's cookies.*

- [ ] **Step 4: Run the three feature sets**

Run: `cargo test -p resonate --no-default-features --features online && cargo clippy -p resonate --no-default-features --features online --all-targets -- -D warnings && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: See it in the window**

Use the `run-ui` skill. Open Settings, find the group, press *Check connection* with the Zen profile value. Expected: *Premium*.

- [ ] **Step 6: Commit**

```bash
git add crates/resonate crates/resonate-ui crates/providers/ytmusic
git commit -m "Connect a YouTube Music account from settings"
```

---

### Task 7: Search and playlists in the window

**Files:**
- Create: `crates/providers/ytmusic/src/in_place.rs`
- Modify: `crates/resonate/src/providers.rs`, `crates/resonate/src/served.rs:32`, `crates/resonate-ui/src/served.rs:51`, `crates/resonate-ui/src/views/served.rs`

**Interfaces:**
- Consumes: `YtMusic::songs`, `Tool`, `PlaysInPlace::plays_songs` (Task 3).
- Produces:
  - `impl PlaysInPlace for YtMusic`
  - `Serves::plays_songs(&self) -> bool` (default true), passed through from the seam
  - `Tool::playlists(&self) -> Result<Vec<Mix>>`, `Tool::playlist(&self, id: &Id) -> Result<Vec<Song>>`

- [ ] **Step 1: Write the failing tests in `tests/in_place.rs`**

```rust
#[test]
fn a_search_lists_songs_alone_explicit_first_and_offers_no_clean_take() {}

#[test]
fn the_first_page_is_the_accounts_playlists_and_liked_songs() {
    assert_eq!(found.mixes[0].name, "Liked songs");
}

#[test]
fn with_no_cookies_the_first_page_is_empty() {}

#[test]
fn a_playlist_lists_its_songs_in_youtubes_order() {}

#[test]
fn a_song_is_not_offered_for_playing_in_place() {
    assert!(!music.plays_songs());
    assert!(matches!(music.streamed(&id), Err(Error::NotOffered { .. })));
}

#[test]
fn a_cover_is_the_rows_own_thumbnail_and_comes_only_from_youtubes_image_hosts() {}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p resonate-ytmusic --test in_place`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Register in `providers::in_place` after Monochrome and append `ytmusic` to `PLAYED_IN_PLACE` (now seven). `first_page` is `https://music.youtube.com/library/playlists` flat with cookies, plus `LM` as *Liked songs*, as `Found::mixes`. `mix` is `https://music.youtube.com/playlist?list=<id>` flat, reading the fields Task 1 recorded. A playlist id is letters, digits, `-`, `_` alone. `artists` empty, `artist` and `album` `NotOffered` under `ProviderOp::Browse`. `cover` accepts only `lh3.googleusercontent.com` and `i.ytimg.com` over `https`. In `views/served.rs`, *Play* and *Next* are not drawn where `plays_songs` is false. `Row::Mix` opens as `Place::Mix` already does.

- [ ] **Step 4: Run the tests, then see it**

Run: `cargo test -p resonate-ytmusic && cargo test -p resonate --no-default-features --features online served`
Expected: PASS. Then `run-ui`: search a song, open *Liked songs*, see rows with no *Play*.

- [ ] **Step 5: Commit**

```bash
git add crates/providers/ytmusic crates/resonate crates/resonate-ui
git commit -m "Browse YouTube Music search and playlists in the window"
```

---

### Task 8: The keep queue

**Files:**
- Create: `crates/resonate-library/src/keeps.rs`
- Modify: `crates/resonate-library/src/schema.rs:29`, `crates/resonate-library/src/filed.rs:67`, `crates/resonate-library/src/supply.rs`, `crates/resonate-library/src/lib.rs`
- Test: `crates/resonate-library/tests/library.rs`

**Interfaces:**
- Consumes: `Providers::only`, `Provider::find`, `Unfiled`, `DeliveryFolder`, `Library::fill_playlist_named`.
- Produces:
  - `pub struct SongToKeep { pub source: SourceId, pub video: String, pub title: String, pub artist: Option<String>, pub album: Option<String>, pub length: Option<Duration> }`
  - `pub struct PlaylistToKeep { pub name: String, pub ordered: bool, pub songs: Vec<SongToKeep> }`
  - `Library::keep_song(&self, song: SongToKeep) -> Result<Kept>`, `Library::keep_playlist(&self, list: PlaylistToKeep) -> Result<KeptList>`
  - `pub enum Kept { Queued, AlreadyHeld(TrackId) }`, `pub struct KeptList { pub queued: usize, pub held: usize }`
  - `Library::keep_pending(&self, providers: &Providers, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<KeepSummary>`
  - `pub struct KeepSummary { pub landed: usize, pub gone: usize, pub unkept: usize, pub stopped_by: Option<SourceId> }`
  - `Library::keeps_pending(&self) -> Result<u64>`
  - `pub(crate) struct ToFile<'a>`: what `filed` takes in place of `&Want`

Schema step `SONGS_KEPT_FROM_A_SERVICE`:

```sql
CREATE TABLE kept_songs (
    id       INTEGER PRIMARY KEY,
    source   TEXT NOT NULL,
    video    TEXT NOT NULL,
    title    TEXT NOT NULL,
    artist   TEXT,
    album    TEXT,
    length   INTEGER,
    playlist TEXT,
    place    INTEGER,
    asked    INTEGER NOT NULL,
    landed   TEXT,
    gone     INTEGER NOT NULL DEFAULT 0,
    UNIQUE (source, video, playlist)
) STRICT;
```

A row stays once `landed`, so a second press of a playlist knows what it placed and what the listener later removed.

- [ ] **Step 1: Write the failing tests in `tests/library.rs`, over a fake provider**

```rust
#[test]
fn a_song_kept_lands_in_the_music_folder_tagged_as_the_service_named_it() {}

#[test]
fn a_keep_is_asked_of_its_own_provider_alone() {}

#[test]
fn a_song_held_already_is_not_kept_again_and_its_copy_fills_the_list() {
    assert_eq!(kept, KeptList { queued: 1, held: 1 });
}

#[test]
fn a_song_kept_twice_is_queued_once() {}

#[test]
fn a_playlist_keeps_youtubes_order_however_its_songs_land() {
    assert_eq!(titles_of_playlist(), ["One", "Two", "Three"]);
}

#[test]
fn a_playlist_told_not_to_keep_order_is_in_the_order_songs_landed() {}

#[test]
fn a_song_that_never_lands_leaves_its_place_and_the_rest_keep_theirs() {}

#[test]
fn a_second_press_adds_new_songs_in_their_places_and_leaves_a_removed_one_removed() {}

#[test]
fn a_keep_longer_than_the_song_was_listed_is_not_kept() {}

#[test]
fn a_keep_with_no_delivery_folder_is_refused() {}

#[test]
fn a_keep_killed_part_way_is_taken_up_again_and_its_staging_swept() {}

#[test]
fn a_provider_away_stops_the_pass_and_keeps_the_queue() {
    assert_eq!(summary.stopped_by, Some(source));
}

#[test]
fn a_keep_waits_while_the_connection_is_metered_and_goes_on_when_it_is_not() {}

#[test]
fn a_catalog_from_before_keeps_is_carried_forward() {}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p resonate-library --test library keep`
Expected: FAIL to compile.

- [ ] **Step 3: Generalise `filed`**

`filed` and `weighed` take `ToFile<'a> { title, artist, album: Option<&AlbumToFile>, disc, position, length, tags: Vec<TagEdit> }`. The want's path builds it from `Want` and `album_to_file`, so its behaviour and its tests do not move. Run `cargo test -p resonate-library` before going on: PASS.

- [ ] **Step 4: Implement `keeps.rs`**

The held check is `store::held_named_alike`: artist key, `title_words` and length within `A_LISTING_NAMED_ALIKE_MAY_DIFFER_BY`, as `synced.rs` resolves a recording, best copy first. `keep_pending` takes `Library::asking_alone` (the poll's lock) and walks rows with `landed IS NULL AND gone = 0`, oldest first. Per row: build `Identity` with a `Link` `https://music.youtube.com/watch?v=<video>`, call `find` on `providers.only(source)`, open the `Delivery::Stream`, `filed`, scan the roots landed under, set `landed`. `Opened::Gone` sets `gone`. An away error ends the pass with `stopped_by`. After each landing, a row with a `playlist` is placed: ordered rows are written through `playlist::fill` and then set in `place` order among the rows of that list already landed; unordered rows append. A `playlist_entries` row the listener removed is known by a landed `kept_songs` row with no entry, and is not put back.

`organise::sweep_what_a_dead_writer_staged` runs at the head of a pass. Before opening each offer the pass waits on the gate `Library::stall_downloads_by` set, as a poll's lane does. A registry that holds no such provider (`online` off, `youtube-music` off) leaves the queue as it stands and answers an empty summary.

- [ ] **Step 5: Run the catalog's tests**

Run: `cargo test -p resonate-library`
Expected: PASS, with `every_step_of_the_chain_stamps_a_catalog_differently` among them.

- [ ] **Step 6: Commit**

```bash
git add crates/resonate-library
git commit -m "Keep a song or playlist from a service in a queue of its own"
```

---

### Task 9: Keep in the window and the terminal

**Files:**
- Modify: `crates/resonate/src/cli.rs`, `crates/resonate/src/main.rs`, `crates/resonate/src/served.rs`, `crates/resonate-ui/src/served.rs`, `crates/resonate-ui/src/views/served.rs`, `crates/resonate-ui/src/models.rs`
- Create: `crates/resonate/src/ytmusic.rs`
- Test: `crates/resonate/tests/cli.rs`

**Interfaces:**
- Consumes: `Library::keep_song`, `keep_playlist`, `keep_pending`, `keeps_pending`.
- Produces:
  - `Sub::Ytmusic(YtmusicArgs)`: `resonate ytmusic keep <URL>`, `resonate ytmusic check`
  - `Serves::keeps_songs(&self) -> bool` (default false), true for `ytmusic`
  - `LibraryModel::keep(&mut self, row: &Row, cx)`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_keep_is_read_from_a_song_link_and_a_playlist_link() {
    assert_eq!(keep_of("https://music.youtube.com/watch?v=abc123"), Keeping::Song("abc123".into()));
    assert_eq!(keep_of("https://music.youtube.com/playlist?list=PLx"), Keeping::Playlist("PLx".into()));
}

#[test]
fn a_keep_with_youtube_music_off_says_so_and_asks_nothing() {}

#[test]
fn a_link_to_anywhere_else_is_refused() {}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p resonate --no-default-features --features online keep`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`cli.rs` stays clap alone (`build.rs` writes man pages from it). `ytmusic.rs` queues, then runs `keep_pending`, printing landed, gone and unkept, the first interrupt as cancel. `check` prints `YtMusic::check`. In the window, *Keep* is drawn on a song row and a mix's header where `keeps_songs`, and pressing it queues and starts the pass on a background thread, reported through the existing *Downloading…* shelf. The window also runs `keep_pending` at open where `keeps_pending` is above zero and `youtube-music` is on.

- [ ] **Step 4: Run the tests and regenerate what `build.rs` writes**

Run: `cargo test -p resonate --no-default-features --features online && cargo build -p resonate`
Expected: PASS.

- [ ] **Step 5: Use it for real**

`cargo run -- ytmusic keep "ALBUM_SONG_URL"`. Expected: a `.mka` under the music folder, `ffprobe` showing Opus near 260k, Resonate naming it after a rescan. Then `run-ui`: press *Keep* on a small playlist, see it land in YouTube's order.

- [ ] **Step 6: Commit**

```bash
git add crates/resonate crates/resonate-ui
git commit -m "Keep YouTube Music songs and playlists from the window and terminal"
```

---

### Task 10: What the repo records

**Files:**
- Modify: `.claude/rules/providers.md`, `.claude/rules/binary.md`, `.claude/rules/ui.md`, `CLAUDE.md`, `.github/workflows/ci.yml`, `packaging/` (Arch `optdepends`, DEB `Suggests`, Fedora `Suggests`)

- [ ] **Step 1: Write the rules**

`providers.md`: a section *A YouTube Music account*, what the last resort is in *Registration order decides*, a keep's queue. `binary.md`: the four keys, `ytmusic`. `ui.md`: the group and *Keep*. `CLAUDE.md`: thirty-five crates, `providers/ytmusic` in the tree, the `ytmusic` CLI line. `AGENTS.md` does not change, since no rules file was added.

- [ ] **Step 2: Hold the layering**

Add `resonate-ytmusic` to the `layering` job as the other provider crates are held.

- [ ] **Step 3: Run the whole list**

```bash
rust-formatter --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --exclude resonate-ui --no-default-features --all-targets -- -D warnings
cargo test --workspace --exclude resonate-ui --no-default-features
RUSTFLAGS= cargo xwin check --target x86_64-pc-windows-msvc --workspace --exclude resonate-mobile --exclude resonate-bindgen --exclude resonate-pipewire --all-targets
```

Expected: all clean.

- [ ] **Step 4: Commit**

```bash
git add .claude CLAUDE.md .github packaging
git commit -m "Record the YouTube Music provider in the rules and the layering"
```
