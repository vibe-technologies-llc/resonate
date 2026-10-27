# Audit

Read on 2026-10-27 at `8481edc3` (`master`). Scope: bugs and UI/UX defects in the desktop window, the engine and library paths a play, a search and a handoff use, and the Android and iOS players beyond the shuffle-from-search change in `388dfad3`. Findings below were opened in the tree. A behaviour a rule in `.claude/rules/` chooses is left out. The two open items in `docs/BUGS.md` are left as they stand.

Six findings are ranked. Two more are recorded with no fix written up. One direction is recorded and not ranked.

## Ranked

| ID | Impact | Effort | Risk | Confidence |
|---|---|---|---|---|
| PHONE-01 | A Shuffle press, and a search tap while shuffle is on, end in catalog order with the control off | S | MED | HIGH |
| UI-01 | A song pressed on an album or artist page, while the search box still holds text and shuffle is on, queues the whole visible library | S | MED | HIGH |
| UI-02 | That same path starts the first visible song when the pressed one is hidden | S | LOW | HIGH |
| BUG-01 | A handoff whose playing row has no catalog recording replays the previous library song from the start | S | LOW | HIGH |
| PHONE-02 | Album, lyrics, analysis and the player's own pages open underneath the expanded player | S | LOW | HIGH |
| UI-03 | A search whose read fails keeps the previous songs under the new query, and an empty failure is drawn as no match | S | LOW | HIGH |

## Findings

### [PHONE-01] Keep shuffle on after an Android play that asked for it

- **Evidence**: `crates/resonate-mobile/src/lib.rs:356` — `load` with `autoplay` false, which is what Android always sends, is `Command::Resume` filled with `..Resumption::default()`. `crates/resonate-core/src/resume.rs:25` — that default has `shuffle: false`. `crates/resonate-engine/src/queue.rs:592` — `restore` copies the flag onto the queue. `android/app/src/main/java/org/resonate/android/ui/library/Catalog.kt:110` — `playShuffled` turns shuffle on and then calls `play`. `android/app/src/main/java/org/resonate/android/LibraryRepository.kt:216` — library actions run in send order, and `RustMediaPlayer.handleSetMediaItems` (`RustMediaPlayer.kt:247`) only queues the load. `android/app/src/main/java/org/resonate/android/ui/Resonate.kt:64` — `playFromSearch` calls `play` on the whole catalog and never turns shuffle back on.
- **Impact**: Shuffle, and a library result pressed while shuffle is already on, start on the chosen song and then play the rest in catalog order. The control ends off. `.claude/rules/android.md` says a search tap with shuffle on plays the whole catalog with shuffle left on.
- **Effort**: S
- **Risk**: MED. Shuffle has to be sent after the load. Sending it before is what the resume then clears. `play` itself must keep ending with shuffle off, so an album or artist row stays in list order.
- **Confidence**: HIGH
- **Fix sketch**: In `playShuffled`, call `play` and then `playback.shuffle(true)`. In `playFromSearch`, after the whole-catalog `play`, call `shuffle(true)`. `Queue::set_shuffle(true)` after an unshuffled restore keeps the restored row (`queue.rs:963`).

### [UI-01] Play a searched album's own row while shuffle is on

- **Evidence**: `crates/resonate-ui/src/views/root.rs:3108` — with shuffle on and `is_searching()`, `play_the_listing_from` loads `whole_library()` and returns. `crates/resonate-ui/src/models.rs:2909` — `is_searching` is a non-empty search box. `crates/resonate-ui/src/models.rs:4148` — `select` opens an album or artist and leaves the query. `crates/resonate-ui/src/views/search.rs:495` — the search page is in front only while the selection is the whole library. `crates/resonate-ui/src/views/browser.rs:1003` — every tracks-pane row, including a row on an album or artist page, plays through `play_the_listing_from`.
- **Impact**: Search, open an album from the results, leave shuffle on, press a song. The page shows that album. The press queues every visible song in the library from that song. The album's own Play still plays the album, and turns shuffle off. The rule in `.claude/rules/ui.md` says a row pressed in a search plays the whole library, and the sentence before it says a row press queues what Play would. The search page is the unscoped one. Android and iOS limit the same gesture to the Search tab.
- **Effort**: S
- **Risk**: MED. The unscoped search, its keyboard reach, its top-result songs, and a row pressed while songs are being added to a playlist must keep playing the whole library with shuffle left on.
- **Confidence**: HIGH
- **Fix sketch**: Take the whole-library path only when shuffle is on, the selection is the whole library, and the search box holds text.

### [UI-02] Play the pressed hidden song instead of the first visible one

- **Evidence**: `crates/resonate-ui/src/views/root.rs:3111` — the whole-library path uses `unwrap_or(0)` when the pressed id is absent. `crates/resonate-ui/src/views/root.rs:3122` — the path used with shuffle off returns without playing in that case. `crates/resonate-library/src/db.rs:100` — a query with no text, which is what `whole_library` builds, keeps `tracks.hidden = 0`. `crates/resonate-library/src/search.rs:597` — a search that insists on `is:hidden` lifts that term, so the drawn row is a hidden song.
- **Impact**: Shuffle on, search `is:hidden`, press the row. Another song starts: the first song of the visible library.
- **Effort**: S
- **Risk**: LOW
- **Confidence**: HIGH
- **Fix sketch**: When the pressed id is missing from the whole-library read, fall through to the drawn listing, the same listing the shuffle-off path already plays. Same change as UI-01.

### [BUG-01] Start a handoff on the next named song when the one playing has no catalog row

- **Evidence**: `crates/resonate/src/sync.rs:578` — while building a handoff, every kept row at or before the playhead moves `at` onto itself. A row `recording_at` cannot name is skipped (`crates/resonate-library/src/synced.rs:679` returns none for a location that is not a catalog path). `crates/resonate/src/sync.rs:986` — the receiver starts at `at` and, when that row is held here, seeks into it. `.claude/rules/sync.md` says a row that is not held continues from the first later song that is.
- **Impact**: Queue of library song A, an unscanned file B that is playing, then library song C. The other device is handed A then C with `at` on A and position 0, and replays A from the start.
- **Effort**: S
- **Risk**: LOW. A playhead the catalog can name stays the row `at` already names, and its position stays.
- **Confidence**: HIGH
- **Fix sketch**: Set `at` to the first kept row whose play-order place is at or after the playhead. When every later row was omitted, set `at` past the end so the receiver plays nothing.

### [PHONE-02] Leave the Android player when its song sheet opens a page

- **Evidence**: `android/app/src/main/java/org/resonate/android/ui/components/Sheets.kt:237` — Go to album, Go to artist, Lyrics, Analysis, Song details, and, from the player, Equaliser, Audio output and Visualizer call `navigator.open` (or `openAlbumOf` / `openArtistOf`). `Sheets.kt:291` — the row dismisses the sheet and then runs that action. `android/app/src/main/java/org/resonate/android/ui/ResonateApp.kt:182` — `NowPlaying` stays composed while `expansion.shown`. `android/app/src/main/java/org/resonate/android/ui/player/NowPlaying.kt:334` — the title and the artist collapse the player before the same navigation. `QueueSheet.kt:131` reads `LocalPlayerExpansion.current` and collapses before opening a page.
- **Impact**: From the expanded player the sheet closes and the destination stays underneath until the player is dragged down.
- **Effort**: S
- **Risk**: LOW
- **Confidence**: HIGH
- **Fix sketch**: At the start of `SongSheet`, read `LocalPlayerExpansion.current`. Collapse before every navigation action. Leave favourite, queue, playlist, share, tags, delete, sleep and remove-from-queue as they are.

### [UI-03] Drop the previous songs when a search cannot be read

- **Evidence**: `crates/resonate-ui/src/models.rs:4135` — `set_query` stores the new text and calls `restate_the_listing` before the read returns. `models.rs:3028` — `listing` is the last loaded `tracks` or `scoped`. `models.rs:4396` — a failed `landed` sets `unreadable`, toasts, and leaves those lists. `crates/resonate-ui/src/views/browser.rs:900` — the tracks empty state says the library could not be read only when the box is empty and nothing is scoped. `browser.rs:304` and `browser.rs:727` — albums and artists say "No albums match." and "No artists match." whenever the box holds text.
- **Impact**: A search whose read fails leaves the previous songs, albums or artists under the new Reads chips, and a press plays one of them. An empty failure is drawn as no match. The toast still fires.
- **Effort**: S
- **Risk**: LOW. A failed refresh of the query already on screen keeps its rows.
- **Confidence**: HIGH
- **Fix sketch**: Remember the query the current browse lists were read for, and clear those lists only when a failure arrives for a different query. When the pane is empty and `unreadable`, draw "The library couldn't be read."

## Recorded, no write-up beyond this note

### [PHONE-03] Name the span the Android listening chart actually draws

- **Evidence**: `android/app/src/main/java/org/resonate/android/ui/settings/StatisticsWords.kt:16` — Year is 365 days and All is the whole history. `StatisticsScreen.kt:99` — `DAYS_DRAWN` is 60. `StatisticsScreen.kt:223` — the chart receives `daysShown`, which is `takeLast` (`StatisticsWords.kt:74`). `StatisticsScreen.kt:419` — the card is titled "Listening by day", and "Most on" is the busiest day of that suffix. The axis labels are the first and last day of the suffix (`StatisticsScreen.kt:452`).
- **Impact**: Week and Month pass through. Year and All draw the last 60 days while the tiles above them are the whole period. The axis dates are the drawn span.
- **Effort**: S
- **Risk**: LOW
- **Confidence**: HIGH
- **Fix sketch**: Title the card with the first and last day it draws, and say "Most on" is inside that span. A year that should show the whole window needs the week and month grouping iOS already uses. The axis already names the days.

### [UI-04] Start an iOS shuffled search on the song that was pressed

- **Evidence**: `ios/Sources/Resonate/SearchViews.swift:125` — with shuffle on and a non-empty catalog, a Search-tab row plays `library.allTracks` from `firstIndex { $0.id == track.id } ?? 0`. Android's `playFromSearch` (`Resonate.kt:63`) plays the results when that place is missing.
- **Impact**: A Search-tab row that is no longer in `allTracks` starts the catalog at its first song. The usual trigger is a stale row after the song left the catalog. Phones do not list hidden songs, so `is:hidden` is not this path.
- **Effort**: S
- **Risk**: LOW
- **Confidence**: MED
- **Fix sketch**: When the index is missing, play the results from the pressed row, as Android does.

## Direction

Android still has no undo after one queue row is removed. iOS keeps `RemovedQueueRow` and offers Undo on the notice (`Library.swift`). `.claude/rules/android.md` describes the swipe and does not mention undo. The previous mobile audit left this the same way. It is not ranked.

## Considered and rejected

- An iOS lyric tap seeks a song the pane is not showing. `LyricStage` puts a moment on a row only while `playingHere`, and `LyricHeard.of` is nil unless the sheet's song is the snapshot's current song. `seek(to:)` is unguarded and has no moment to receive for the other song. A paused sheet of the current song still has a moment, which matches the lyric rule.
- The minimum-length field applies on Enter, as its note says, and on a press outside the field, as every keyed field does. Escape writes the stored seconds back. Tab leaving the field matches the note.
- Android already sends the whole catalog from the Songs tab. A shuffled search that does the same is the rule, and `handleSetMediaItems` reads `mediaId` alone. No confirmed transaction-size failure was opened.
- An iOS album or artist row calls `Library.play`, which turns shuffle off. The iOS rule does not say a row press leaves shuffle on. After PHONE-01, an Android album row still ends with shuffle off, on purpose.
- The minimised iOS mini player shows the artist under the title and no next button. That is the rule updated in `388dfad3`.
- Both iOS Statistics entries say "Your listening in numbers" (`8481edc3`).
- `Queue::load` and `set_shuffle(true)` keep the start row when shuffle was already on. PHONE-01 is the resume default, and this path is sound.
- `search_in_front` is false on an album page because the selection is no longer the whole library. That is the right signal. UI-01 is `play_the_listing_from` reading `is_searching` instead.
- `Player::cuts_to_fit` uses the first cut when nothing is playing, then rotates. That is the fit's start rule.
- The two `docs/BUGS.md` items (a huge embedded picture, a stereo Opus packet into a stale decoder) stay blocked on upstream. `docs/FEATURES.md` gaps (Android DoP, an iOS widget, CarPlay) are not defects. Playlist work was not proposed.
- Engine and library paths that were opened and match their rules: a save while a song is opening writes nothing; a seek in that window is kept; queue moves agree with an independent rebuild; a play-next jump matches the rule; `restore` writes a playhead that sits in the next span from `playing_next()`; `remove_all` dedupes; phone `Placement::At` against the desktop's `Placement::Queued`; search `added:` and `-is:hidden`; history aging while signed out; a counted listen whose URI is no longer in the phone catalog. The fixes in `9bbdf693` were left as they landed.
- Desktop paths that match their rules: a timed lyric click and its cue shift; a word wipe that turns one space into one newline; one queue undo step; settings fields that commit on blur as their figures say; an empty unscoped library already says it could not be read, and an empty library with no query already says nothing is there yet.
- Phone paths that match their rules: a shuffled search queues the whole catalog (PHONE-01 is the flag that then clears); the collapsed iOS mini player; an iOS year bar labelled "Week of" (the rule asks for the year on the end labels); organise confirms; the tag editor confirms a discard; an Android lyric tap already requires the pane to be following. The statistics chips keep the previous figures until `listeningDetail` returns, and that disagreement ends when the read lands.

## What was not audited

Security, performance, dependencies, tests as a category, docs, and generated UniFFI and `app/build` output. Nothing was exercised in the player, on an emulator, or on a phone. Line numbers are from `8481edc3`.

The desktop pass stopped after the play, search, queue and settings paths named above. The engine and library pass stopped after transport open, queue edit, handoff, scan keep, and the phone load path. The phone pass stopped after shuffle, lyrics, the player sheet, statistics, search and the sheets that confirm a destructive press. Codec, Listen, downloads and tag write-back did not get a second full pass.
