# Audit: Android, iOS, and the shared mobile stack

Read against `900b8478` on 2026-10-27. Scope is bugs and UI/UX in `android/`, `ios/`, and `crates/resonate-mobile`. The desktop window, security, performance, dependencies, tests, and docs were left alone except where a phone and the shared boundary disagree about a behaviour both already implement.

Evidence is the tree at that commit. Nothing here was exercised on an emulator, an iPhone, or a device. Line numbers are the ones read in that tree.

## Ranked findings

Leverage is impact divided by effort, discounted by confidence and by how easily the fix can go wrong.

| Rank | ID | Finding | Effort | Risk | Confidence |
| --- | --- | --- | --- | --- | --- |
| 1 | MOBILE-01 | A seek while a song is still opening is dropped and reported as success | S | MED | HIGH |
| 2 | MOBILE-02 | A save while a song is still opening writes the place as the start of the song | S | MED | HIGH |
| 3 | MOBILE-03 | A tag undo that cannot finish drops the previous tags it did not write back | S | LOW | HIGH |
| 4 | ANDROID-01 | A failed lyric lookup is asked again at once and drawn as a song with no lyrics | S | LOW | HIGH |
| 5 | ANDROID-04 | A library that failed to open is drawn as an empty library | S | LOW | HIGH |
| 6 | ANDROID-05 | A playlist that failed to load is drawn as an empty playlist, with Add songs | S | LOW | HIGH |
| 7 | IOS-01 | Sleep timer from Now Playing never shows its sheet | S | LOW | HIGH |
| 8 | IOS-02 | A pairing link opened while Now Playing is up never asks to join | S | LOW | HIGH |
| 9 | ANDROID-02 | A finger still on the scrubber seeks the song that is playing when it lifts | S | LOW | HIGH |
| 10 | ANDROID-03 | A queue swipe that the engine refuses leaves the row visually gone | S | LOW | HIGH |
| 11 | ANDROID-06 | A renewed TIDAL token replaces the account draft the listener is typing | S | MED | HIGH |
| 12 | MOBILE-04 | One failed lyric reread is remembered as a song with no words | S | LOW | HIGH |
| 13 | ANDROID-07 | Playback position and the lyric sweep move only on the 250 ms poll, and a cache miss runs on the transport lock | M | MED | HIGH |
| 14 | IOS-03 | Save as playlist and Edit search drop a typed draft on Cancel and on a swipe | S | LOW | HIGH |
| 15 | ANDROID-08 | Clear on the queue removes history and up next on one tap | S | LOW | HIGH |

The first five are the ones worth planning first: the three shared defects, the Android lyric lie, and then either the empty-library welcome or the iOS sleep timer. ANDROID-04 and ANDROID-05 are the same class of lie and can be one plan.

## Shared mobile

### [MOBILE-01] Send a seek that arrives while a song is still opening

- **Evidence**: `crates/resonate-mobile/src/lib.rs:419` — `seek` sends `Command::Seek` only when `player.state().current` is `Some`, and otherwise returns `Ok(())`. `crates/resonate-engine/src/engine.rs:2041` — the engine already keeps that seek: while `self.opening` is set, `seek` writes `opening.at` and returns. `crates/resonate-mobile/src/lib.rs:446` — the snapshot still names the opening row through `row_being_opened` (`lib.rs:115`, buffering or paused with a queue position and no `current`), and `lib.rs:465` fills `duration_ms` from the catalog track when the engine has not published a duration. Android's scrubber is enabled whenever that duration is above zero (`android/app/src/main/java/org/resonate/android/ui/player/NowPlaying.kt:391`). A release goes through `Playback.seek` (`ui/Resonate.kt:85`) into `RustMediaPlayer.handleSeek` (`RustMediaPlayer.kt:285`), which calls `it.seek(position)` on the current item. iOS seeks through the same `MobilePlayer::seek`.
- **Impact**: After a skip, the phones draw the song that is opening and a scrubber or lyric line that can be pressed. The press succeeds and the song opens where it was already going, which is its start. A slow document open (a large file, a cold Storage Access Framework grant) is the window where this is easy to hit.
- **Effort**: S
- **Risk**: MED — the `Frames` have to be built from the opening document's rate, the same way a resumed place is, or the landing frame is wrong.
- **Confidence**: HIGH
- **Fix sketch**: When `current` is absent and `row_being_opened` names a row, build `Frames` from that document's rate and send `Command::Seek`. Extend the queue-edit test that already holds "a row still opening is the song a snapshot names".

### [MOBILE-02] Keep the resumed place while the next song is still opening

- **Evidence**: `crates/resonate-mobile/src/lib.rs:510` — `save` sets `position_ms` with `state.current.map_or(0, …)`. The in-memory `saved` starts empty, so the first save takes the `save_queue` branch (`lib.rs:534`) and writes that zero. A later save of the same queue revision calls `save_place` with the same zero (`lib.rs:533`). Android writes it every fifth second inside the poll (`LibraryRepository.kt:300`, `POLL_EVERY_MS` is 250) and on leaving the foreground (`MainActivity.kt:330`, `PlaybackService.kt:258`). iOS writes it every five seconds from the poll (`Library.swift:153`, `Library.swift:380`) and on every scene phase other than active (`ResonateApp.swift:28`).
- **Impact**: Skipping to a song and backgrounding the app, or waiting out one save interval, while that song is still opening stores the new queue at the start of the song. A process kill before the track publishes resumes there. The place the listener had reached in the previous song is already gone, because the save rewrote the queue.
- **Effort**: S
- **Risk**: MED — skipping the write for the whole opening also leaves the previous song's place on disk until the new row publishes. That is the right trade: a place of zero is a lie, and the previous place is still true until the new song has a position.
- **Confidence**: HIGH
- **Fix sketch**: Persist a place only once `state.current` is present. Extend the save test that already holds "a save after an edit keeps the edited queue".

### [MOBILE-03] Put an unfinished tag undo back on the stack

- **Evidence**: `crates/resonate-mobile/src/tagging.rs:219` — `undo_tags` pops the step before any write. A URI the caller did not pass (`tagging.rs:225`) and a `retag` error (`tagging.rs:232`) increment `failed` and drop that song's `before` tags. Songs already written in the same call stay written. `edit_tags` (`tagging.rs:187`) only stacks the songs that landed, so the popped step is the only copy of what those files held.
- **Impact**: One document the provider refuses, or one URI the front end leaves out of the undo call, permanently loses the previous tags for that song. Library tools then has nothing left to undo. Both phones use this call.
- **Effort**: S
- **Risk**: LOW — the entries that were not written go back onto `steps` before the function returns. A later undo retries them.
- **Confidence**: HIGH
- **Fix sketch**: Collect the `Undone` rows that were skipped or whose `retag` failed and `keep` them again before returning `TagsWritten`. Extend the tagging test that already walks a staged write and one undo.

### [MOBILE-04] Try a failed lyric reread again

- **Evidence**: `crates/resonate-mobile/src/lyrics.rs:222` — `lyric_frame` on a cache miss calls `lyrics_from_the_catalog`. On `Err` it stores `Kept::Wordless` (`lyrics.rs:226`). `kept_frame` (`lyrics.rs:243`) then answers `unlit()` for that entry, and `kept_lyric_frame` returns `Some`, so the caller treats the answer as final. A successful empty read also stores `Wordless` (`remember_lyrics`, `lyrics.rs:271`); that path is a real miss. iOS asks `keptLyricFrame` on the display tick and returns as soon as it gets a frame (`LyricsPane.swift:620`). Android's poll asks `lyricFrame` (`LibraryFeatures.kt:1175`), which hits the same cache.
- **Impact**: One catalog read that fails (a lock, a disk error) leaves the highlight and the active line off for that song until the sixteen-sheet cache drops the entry. The lines can still be on screen from the sheet the pane already holds. A later successful read is never asked.
- **Effort**: S
- **Risk**: LOW — an error returns unlit for that call and leaves the cache unchanged, so the next frame tries the catalog again.
- **Confidence**: HIGH
- **Fix sketch**: On `Err`, log and return `unlit()` without `keep(Wordless)`. Leave the successful empty read as `Wordless`.

## Android

### [ANDROID-01] Stop asking for lyrics again the moment a lookup fails

- **Evidence**: `android/app/src/main/java/org/resonate/android/LibraryFeatures.kt:1145` — `loadLyrics` clears `lyrics` and `lyricFrame` and sets `lyricsLoading` before the call. On a throw, `work` reports the failure and `finally` clears `lyricsLoading` with `lyrics` still null. `ui/tools/Lyrics.kt:104` — while the pane is composed, `LaunchedEffect` calls `loadLyrics` whenever loading is false, the pane holds no sheet, and the shared sheet is for another song. `crates/resonate-mobile/src/lyrics.rs:209` — a real miss (`Ok(None)`) stores an empty text and `lyrics` then returns a sheet (`lyrics.rs:219`), which stops the effect. Only an `Err` with nothing cached (`lyrics.rs:213`) comes back as a failure. The empty copy is always "No lyrics for this song", and while online "none were found online" (`Lyrics.kt:116`). A sheet already held for this song stays in `held` (`Lyrics.kt:98`), so a failed refresh of a song that already has lines keeps those lines and only clears the frame.
- **Impact**: The first lookup for a song, or a lookup after a track change, that the network or the catalog refuses is asked again as soon as loading falls, for as long as the pane is open. Between those asks the pane says the song has no lyrics and that the online search found none. The error banner says the real failure at the same time. A genuine miss settles after one ask.
- **Effort**: S
- **Risk**: LOW — one failure stays on the banner, the pane says the lyrics could not be read, and Search again is the listener's retry. iOS already does this (`Features.swift` keeps the sheet and sets `lyricsUnread`; the pane says "Lyrics could not be read").
- **Confidence**: HIGH
- **Fix sketch**: Remember a failed id the way the analysis failure is remembered, draw the failure with Try again, and call `loadLyrics` again only from that button. Leave `lyrics` in place when a refresh of the same id throws.

### [ANDROID-02] Drop the scrubber draft when the song changes

- **Evidence**: `android/app/src/main/java/org/resonate/android/ui/player/NowPlaying.kt:384` — `draft` is `remember { mutableStateOf<Float?>(null) }` with no song or queue key. `shown` is the draft while it is set. `ui/components/CommitSlider.kt:37` — `dragging` is its own unkeyed `remember`. While dragging, `SideEffect` does not follow the live value (`CommitSlider.kt:39`). `onValueChangeFinished` commits `state.value` (`CommitSlider.kt:59`). `SliderState` is `remember(range)`, so a duration change recreates the slider, and the parent draft survives. `ui/Resonate.kt:85` — `seek` calls `seekTo` on whatever is current.
- **Impact**: A finger still down when the song changes keeps the previous song's millisecond. Lifting seeks the song that is then playing to that millisecond.
- **Effort**: S
- **Risk**: LOW — key the draft and the drag on the playing queue id, and clear both when it changes. A gesture the system cancels should take the same path as a finished drag that is discarded.
- **Confidence**: HIGH
- **Fix sketch**: Remember the draft and the dragging flag against `playing.track.queueId`. On a change, set the draft back to null and skip the commit.

### [ANDROID-03] Put a refused queue swipe back

- **Evidence**: `android/app/src/main/java/org/resonate/android/ui/player/QueueSheet.kt:223` — `LaunchedEffect(dismiss.currentValue)` calls `model.remove(track)` when the swipe reaches `EndToStart` and never resets the box. `PlayerViewModel.kt:130` — `remove` returns immediately when `queueId` is null, and otherwise sends `removeFromQueue`. `LibraryRepository.kt:177` — a thrown action is reported and the published queue stays as it was. `crates/resonate-mobile/src/queue_edit.rs:95` — `request_if_the_queue_is_still` refuses the edit when the revision moved. Playlist rows do reset: `PlaylistScreen.kt` calls `onRemove()`, waits, then `dismiss.reset()`.
- **Impact**: A swipe the engine refuses, and a row with no queue id, stays drawn off the side. The song is still in the queue and can still play. The listener has no row to swipe again.
- **Effort**: S
- **Risk**: LOW — reset the box on a null id and after the request returns, the way the playlist row already does.
- **Confidence**: HIGH
- **Fix sketch**: Make `remove` a suspending call that reports success, and `dismiss.reset()` unless the published queue no longer contains that `queueId`.

### [ANDROID-04] Say when the library could not be read

- **Evidence**: `android/app/src/main/java/org/resonate/android/LibraryRepository.kt:236` — a `refresh()` failure at open is reported, and `poll` still starts, so snapshots arrive with `allTracks` still empty. `ui/library/Catalog.kt:45` — `catalogReady()` is true once any snapshot exists or the track list is non-empty. `HomeViewModel.kt:32` uses that flag. `HomeScreen.kt:112` and `LibraryScreen.kt:114` then draw `Welcome()` when the song list is empty. `Parts.kt:414` — Welcome says "Make room for your music" and offers Add a music folder and Use Downloads. `Loaded.Failed` and `Unreadable` (`Parts.kt:389`) already exist for other catalog reads and are unused on this path.
- **Impact**: A catalog that fails to open (a full disk, a migration refusal, a native error) looks like a new install. The listener is offered the gestures that add a folder. The failure is also on the banner, under a screen that says the library is empty.
- **Effort**: S
- **Risk**: LOW — keep Welcome for a refresh that succeeded with no songs. A failed refresh stays on `Loaded.Failed` with Try again until a later read succeeds.
- **Confidence**: HIGH
- **Fix sketch**: Record the opening `refresh` failure apart from an empty catalog. Home and Library draw `Unreadable` on that flag, and Welcome only once a read has succeeded with no songs.

### [ANDROID-05] Say when a playlist could not be read

- **Evidence**: `LibraryFeatures.kt:1069` — `selectPlaylist` sets `playlistRows` to empty and `playlistLoading` to true before `reloadPlaylist`. `finally` only clears loading. A throw leaves the rows empty. `PlaylistViewModel.kt:32` — loading false with any list, including an empty one, is `Loaded.Ready`. `CollectionPage.kt:39` draws that ready value. `PlaylistScreen.kt:189` — an empty list of a real playlist says "Add songs to shape this playlist." and shows Add songs (`PlaylistScreen.kt:160`).
- **Impact**: A failed read looks like a playlist the listener has not filled. Add songs appends onto the songs still stored. The banner has the error, and the page offers the gesture that changes the playlist.
- **Effort**: S
- **Risk**: LOW — a failure is `Loaded.Failed` with the page's existing Try again. An empty list after a successful read keeps the current copy.
- **Confidence**: HIGH
- **Fix sketch**: Keep a failed-id beside `playlistRows`. The view model emits `Loaded.Failed` for that id, and `selectPlaylist` clears it only after `reloadPlaylist` returns.

### [ANDROID-06] Copy a renewed TIDAL token into the open draft

- **Evidence**: `ui/settings/ProviderWords.kt:46` — `accountIdentity` is the provider name plus `refreshToken`. `ProviderAccountScreen.kt:91` — any stored account whose identity changed replaces the whole `draft`. `LibraryFeatures.kt:164` — a download's renewed token calls `ProviderAccountBook.renewed` (`ProviderAccountBook.kt:66`), which rewrites the TIDAL account's refresh token. `LibraryFeatures.kt:1378` — finishing sign-in saves `account.copy(refreshToken = token)` from the account captured when sign-in started.
- **Impact**: Typing a client id, a secret, or a password while a download rotates the refresh token, or while sign-in completes, replaces those fields with the stored account. The new token is kept. A later Save can write the rest of the form back as it was before the listener's edits.
- **Effort**: S
- **Risk**: MED — the screen still has to show the new token, and a draft the listener has not touched should still follow the stored account.
- **Confidence**: HIGH
- **Fix sketch**: When the identity changes and `draft` is non-null, copy only `refreshToken` into the current draft. Replace the whole draft when it is still null or still equal to the previous stored account.

### [ANDROID-07] Move the lyric frame off the transport poll

- **Evidence**: `LibraryRepository.kt:276` — the poll holds `access` (`LibraryRepository.kt:283`), the same lock as every `request` (`LibraryRepository.kt:180`), then calls `features.tick` (`LibraryFeatures.kt:1170`). With the lyric pane open and in the foreground, `tick` calls `native.lyricFrame` (`LibraryFeatures.kt:1175`) at the snapshot's `positionMs`. That is the catalog-capable function (`lyrics.rs:222`). The delay is `POLL_EVERY_MS` (`LibraryRepository.kt:647`, 250). `NowPlaying.kt:381` draws the scrubber from that snapshot position, with no clock between polls. The mini player uses the same stepped fraction. iOS asks `keptLyricFrame` inside the display tick (`LyricsPane.swift:620`) and only falls through to the catalog on a miss. `mobile.md` says `kept_lyric_frame` exists so a front end may ask it inside a display tick.
- **Impact**: The scrubber, the mini-player progress, and the word sweep jump in quarter-second steps. A cache miss reads the catalog on the lock that play, pause, and seek wait behind.
- **Effort**: M
- **Risk**: MED — a display-rate call has to be `keptLyricFrame`, off `access`. Extrapolating the position has to stop at a seek, a skip, and a pause, or the bar runs past the real place.
- **Confidence**: HIGH
- **Fix sketch**: Publish position from a clock seeded by the snapshot, in the same way iOS's scrubber flows between readings. Ask `keptLyricFrame` from the lyric pane on that clock. Leave `lyricFrame` for a miss, on a worker, outside the transport lock.

### [ANDROID-08] Confirm Clear on the queue

- **Evidence**: `QueueSheet.kt:115` — Clear is a `TextButton` whose click is `model::clear`, and `PlayerViewModel.clear` sends `clearQueue` (`PlayerViewModel.kt:135`). iOS asks first: `PlayerViews.swift:213` confirms "Clear the queue?" and says history and up next are removed and the playing song keeps playing. Android has no undo for the removal (see Direction).
- **Impact**: One tap clears history and everything queued after the playing song. The label says Clear, and the songs are gone for the session.
- **Effort**: S
- **Risk**: LOW — a confirmation dialog, the same copy iOS already uses.
- **Confidence**: HIGH
- **Fix sketch**: Confirm, then call `clear`. Leave the engine command as it is.

## iOS

### [IOS-01] Present the sleep timer from inside Now Playing

- **Evidence**: `ResonateApp.swift:288` — Now Playing is a `fullScreenCover` on the root. `ResonateApp.swift:295` — the sleep-timer sheet is a sibling of that cover. `PlayerViews.swift:792` — the countdown chip sets `navigation.sleepTimer` while the cover is up, with the accessibility hint "Opens the sleep timer". `PlayerViews.swift:1075` — More › Sleep timer does the same. Tags, share, delete, and the playlist picker are already attached so they can present while the cover is up: `Navigation.editTags` dismisses the cover first (`ResonateApp.swift:109`), and `NowPlayingView` hosts the share sheet, the clear confirmation, and the playlist picker (`PlayerViews.swift:209`). Settings still sets `navigation.sleepTimer` with the cover down (`SettingsViews.swift`).
- **Impact**: With the player open, Sleep timer and the countdown chip do nothing visible. A timer cannot be started, changed, or cancelled from Now Playing. VoiceOver announces a control that opens the sheet.
- **Effort**: S
- **Risk**: LOW — the sheet already works from Settings. This adds the same sheet inside the cover while the cover is up, and leaves the root sheet for Settings.
- **Confidence**: HIGH
- **Fix sketch**: Present `SleepTimerSheet` from `NowPlayingView` while `nowPlaying` is true, the same way `SharingSheet(presenting: true)` is attached there. Gate the root sheet with `!nowPlaying`, as the root share sheet already is.

### [IOS-02] Ask to join when a pairing link arrives over Now Playing

- **Evidence**: `Library.swift:1477` — a `resonate://` open only calls `offerPairing`. `Features.swift:368` — that sets `pairingOffered` and leaves `nowPlaying` as it is. `ResonateApp.swift:305` — the "Join these devices?" alert is on the same root as the full-screen cover. The share sheet was moved inside the cover, and the root share sheet is gated with `!navigation.nowPlaying` (`ResonateApp.swift:304`), because a root presentation does not appear over this cover. The tag editor uses the other pattern: dismiss the cover, then present from `playerClosed` (`ResonateApp.swift:115`).
- **Impact**: Opening a pairing link or a QR code while the player is up never asks to join. The code stays in `pairingOffered` with no prompt. Confirm still calls `joinSync` only after the listener agrees (`ResonateApp.swift:313`).
- **Effort**: S
- **Risk**: LOW — the alert's actions stay. Only the presentation moves above the cover, or waits until the cover has dismissed.
- **Confidence**: HIGH
- **Fix sketch**: Attach the alert inside `NowPlayingView` when the cover is up, or clear `nowPlaying` and present from `playerClosed`, as `editTags` does. Do not call `joinSync` until the listener confirms.

### [IOS-03] Keep a saved-search draft until Save or an explicit discard

- **Evidence**: `ios/Sources/Resonate/SavedSearchSheet.swift:55` — Cancel calls `dismiss()` with no check. `SavedSearchSheet.swift:68` — the sheet sets a detent and never disables the swipe. `ToolViews.swift:95` — `keepingEditsUnlessDiscarded` disables the swipe and asks before closing. The tag editor, Add songs, and the filter sheet use it (`TagEditor.swift`, `PlaylistViews.swift`, `LibraryToolViews.swift`). `ios.md` names those three as the sheets that hold edits until their confirm button.
- **Impact**: A name, query, order, or limit typed into Save as playlist or Edit search is dropped by a swipe or by Cancel, with no chance to keep editing.
- **Effort**: S
- **Risk**: LOW — Save still writes the draft. Only a dirty dismiss starts asking first.
- **Confidence**: HIGH
- **Fix sketch**: Compare the draft with the sheet's initial value and apply `keepingEditsUnlessDiscarded` the way the filter sheet does. Point Cancel through that same ask.

## Direction

These are gaps, separate from the ranked bugs. Each phone's own rules describe the current behaviour.

### [DIR-01] Offer undo when a queue row is removed on Android

iOS remembers the last removed row and offers Undo (`Library.swift` `removeFromQueue`, the notice offer `restoreQueueRow`). `insert_in_queue` already puts a song back at the row it left. Android's swipe (`QueueSheet.kt:225`), the song sheet, and a Media3 removal (`RustMediaPlayer.kt:281`) call `removeFromQueue` and say nothing afterwards. `android.md` describes the swipe and does not mention undo. A listener who uses both phones loses the row on Android with no way back. The in-app swipe and the song sheet are the surfaces worth matching; a car or headset removal can stay immediate.

## Considered and rejected

Recorded so a later pass does not reopen them.

- Queue move indexes. Android `queueLanding` adds one when the destination is below the source. The engine's `move_in_queue` uses the SwiftUI convention and subtracts one in that same case. The tests pin `(1, 3) → 4`. The visual landing matches.
- Saving the queue keeps repeats. Both phones pass every queue id into `createPlaylist` and `addToPlaylist`, and the insert does not collapse a repeated song.
- Android statistics cut days in the phone's zone. `StatisticsScreen` calls `listeningDetail` with `java.util.TimeZone`. An older note that Android never asked for local days is stale.
- Repeated songs in History. `Track` carries `queue_id`, and `indexOf` on the generated data class tells two plays of one song apart.
- Android `handleSeek` jumps and then seeks inside one `library.request`. That is the same-item seek fix, and it is the path MOBILE-01 then drops while `current` is absent.
- One Up next section on Android. `android.md` specifies History, the playing row, and Up next. iOS also splits Playing next from Continue playing. The snapshot fields `next_first` and `next_count` are there for a phone that wants the split. Android's single section matches its rule.
- Analysis cleared before a pass. Both phones null the report before `analyse`. The Android pane returns to "Analyse original audio", and the banner reports the error. It does not claim the song has no analysis. The lyric pane does claim that, which is ANDROID-01.
- Documented gaps, left as written: DoP on Android, an iOS widget, CarPlay, `arm64e`, changing an Oboe stream's rate or word in place, phones sending lyrics and studies and neither releases nor covers nor Lyricsfiles, Fit the preamp leaving room correction out, match-song-rate defaulting off on Android and on on iOS, unasked notices defaulting off.
- Cross-phone differences that match each phone's rule: the equaliser switching on when a preset is chosen, no minimum letter count before an online search, statistics showing the period the loaded data was read for, cancelling one download, the sleep timer's three choices, the lyric follow pause, the favourite snackbar, settings search, saved-search draft rules other than IOS-03, search settle delays, Listen's retries, clearing a Mixed tag field, and putting organised files back.

## What was not audited

The GPUI window (`crates/resonate-ui`) and the desktop catalog, except where a phone bug is a misuse of a shared call. Generated UniFFI (`ios/Generated`, the Android bindings under `app/build`). A device or emulator pass, an xtool build, and the host Swift and Gradle test suites. Security, performance, dependency, test, and documentation passes. Playlist behaviour that already matches the finished-playlist rule. The open items in `docs/BUGS.md` are desktop and format bugs (a huge embedded picture, stereo Opus mono packets) and were left there.
