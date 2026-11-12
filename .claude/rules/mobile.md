---
paths:
  - "crates/resonate-mobile/**"
  - "crates/resonate-bindgen/**"
  - "android/build-native.py"
  - "ios/build-native.py"
---

# The mobile boundary

`resonate-mobile` is the one native boundary both phone front ends sit on: UniFFI proc-macros
export `MobilePlayer` and its records, and `resonate-bindgen` writes Kotlin (`android.md`) and Swift
(`ios.md`) from the same library metadata. Behaviour that is the boundary's (document catalog, playlists, sync bridge, downloads, online
orchestration, enrichment) is described once in `android.md` and holds for iOS unchanged; this file
holds what is the crate's alone.

The phones have no vault and none is planned: the boundary never opens one, and a song stays the
listener's own file.

## Unsafe

`resonate-mobile` is one of the three exceptions to the workspace's unsafe prohibition, explicitly
approved for the native boundary (the others are `resonate-win32`, the Windows calls no safe crate
wraps, `windows.md`, and `resonate-apple`, the Core Audio and AudioToolbox calls, `dependencies.md`).
UniFFI generates the FFI scaffolding; the handwritten unsafe operation is one. **An owned
descriptor**: the front end hands Rust a descriptor it opened (Android detaches
`ParcelFileDescriptor`, iOS calls `open(2)`), transferred once and closed by Rust's `File`; a
negative descriptor is refused (`documents::owned`, for a document read through `Documents` and one replaced through `DocumentRewriting` alike). The same transfer (`downloads::fresh_destination`) owns a newly
created download or received-song destination, never an existing music document. **The iOS audio
unit is no longer here**: the AudioToolbox calls that create, configure and dispose a RemoteIO unit,
their sequencing, the render callback and its reading of the system's buffer list are
`resonate-apple`'s (`Unit`, shared with the macOS backend), reached through its safe API, so
`output/remote_io*.rs` holds no `unsafe`. Domain, codec, DSP, engine and all other crates remain under `forbid`. New errors
are typed and bounded as in `errors.md`.

## A panic is an error on the phones

The workspace's release profile aborts on a panic, which is right for the desktop binary and wrong
across the boundary: UniFFI's `catch_unwind` turns a panic in an exported call (a listener's
corrupt file, a catalog row nobody foresaw) into an error the app shows, but only if the library
unwinds. Both `ios/build-native.py` and `android/build-native.py` therefore build `resonate-mobile`
with `CARGO_PROFILE_RELEASE_PANIC=unwind`, so a release build and a debug build fail alike. The
render callbacks abort on a panic, as an unwind may not cross them: `AURenderCallback`'s type is
`extern "C-unwind"`, so `remote_io_render::render` catches the panic itself and calls
`process::abort`.

## An error is the sentence the app shows

`Error` is `flat_error`, so a front end sees its `Display` alone: a variant a listener can meet is
written as a capitalised sentence composed from its typed fields (`MusicProvider`'s `Display` names
the service, `DownloadOp`'s says what the download could not be), never a debug print of an op.
`downloads::failure_of` is the one reading of a provider's error into those variants, for a find, an
opening and the TIDAL sign-in alike.

**Every sentence comes out of the crate's catalog** (`words/en/*.ftl`, one area per subject:
`errors` for each variant and the op words its `Display` reads, `failure`, `measurement`,
`downloads`, `equaliser` for the presets' names and lines, `statistics`, `search`, `sync`,
`driving` for `sync_control_offered`): a variant's `#[error]` calls its `words::errors` function
with the typed fields as the variables, a count chooses its form by a Fluent selector, and a
figure whose precision matters (a frequency, a decibel) is passed formatted. A foreign error a
sentence carries (`{ $source }`) and the words another crate hands over (`LookupOp::asked_for`,
`SpotifySignInFailure`, a `resonate_core::Error` passed as `Domain`, `Reason::says`, a Wrapped
slide, a built-in target's label, a service's title) are that crate's own.

**The sentence is said in the phone's language** (`language.md`): `speak_languages(tags)`
(`language.rs`) takes the platform's preferred languages, settles on the first spoken here
(`resonate_words::negotiated`, English otherwise), makes it the process's and answers its tag, so
every sentence the boundary words afterwards (an error, a failure, a preset's name) is in it. It is
a free function, called before the player opens and whenever the phone's languages move.

**A measurement that will not fit says which file** (`measurement.rs`): `Error::Measurement { file,
source }` carries `MeasuredFile::{Measurement, Target}` and the boxed `resonate_eq::Error`, and
`failure::of_a_measurement` words each refusal (too large, no point, too few points, a span that
misses 500 Hz to 2 kHz, nothing to correct) as a sentence naming the file; a built-in target no one
is called is `UnknownTarget { name }`. `fit_measurement` takes the measurement's bytes and a
`MeasurementTarget` (a built-in's name or a document's bytes), is synchronous CPU work for the
caller to run off its main thread, and `measurement_offers()` hands over the built-ins and the byte
limit (`eq.md`'s *A measurement of your own*). Its result field is `imported`, not `import`, a Swift
keyword. **A live measurement is two calls and a rate**: `measurement_rate_for(output_rate)`,
`measurement_sweep(rate)` (a `MeasurementSweep` of `rate`, `samples` and `recorded_frames`) and
`fit_recorded_sweep(recorded, rate, calibration, target, name)`, which fit what a phone's microphone
heard of the sweep played beside the engine (`eq.md`). A recording that cannot be measured is
`Error::Sweep { source }`, the `resonate_dsp::Error` worded by `failure::of_a_sweep`, and a
calibration file that will not read is `Error::Measurement` naming `MeasuredFile::Calibration`.

## A failure is told by its cause

`failure.rs` reads `resonate_engine::Cause` (the desktop's reading of an engine error, with
`Cause::of_a_read` for a codec error) into the sentences the desktop's toast says, worded for songs:
`Snapshot.failure` is `would_not_play` (naming the song's title where the queue row and catalog still
know it) or `would_not_do` for a refused command, and `Error::Player` and `Error::Decode` display
`of_a_command` and `of_a_read`, so a missing file, an unsupported format, a damaged file and a refused
device read differently. `Error::Reference` displays `of_a_lookup` (what was asked, through
`LookupOp::asked_for`, and how it failed), and `StoreOp`, `SyncOp` and `ReceivedOp` implement `Display`,
so no variant prints an op with `{:?}`. The report text the phones show is `Finding::told` and
`Container`'s `Display`, never a debug print.

**A wait for a device is a state, not a failure.** `Event::Waiting` (the output went, the row is held
at its frame and plays on once a device is there) becomes `Snapshot.waiting`, the desktop's toast
sentence (`waits_for_a_device`: no speakers or headphones, or the device stopped playing, each
*playing on once one is there*). It stands in every snapshot, a pause included, until an output is
bound again or the transport stops (`waiting_is_over`), so a front end shows it while it is true and
takes it down when playback goes on
(`a_snapshot_says_there_is_no_device_to_play_through_until_the_transport_stops`).

**A snapshot carries the open stream's figures** (`OutputFigures::of`): `output_rate`, `underruns`
and `overs`, the samples the chain clamped at full scale (the desktop inspector's *clamped
samples*, `audio.md`), all 0 with no stream open
(`a_snapshot_carries_the_clamped_samples_of_the_open_stream_and_none_without_one`). Android's
*Audio output* page shows `overs` as a *Clamped samples* row only once it is above 0
(`clampedSamplesWords`); iOS's output page shows the same in *Current stream*, read from `PlayClock`.

`snapshot` consumes (it drains engine events and takes the failure), so a caller that
only needs the transport asks `transport`, which reads the engine's state and nothing else.

## The engine's events are heard on a thread of the boundary's

`watching.rs` keeps one thread per player (`resonate-watch`, holding a `Weak` so a dropped player
ends it, and ending when the engine's event channel closes), which owns two jobs a front end's poll
used to do. **It counts listening**: on each engine event and otherwise every second while playing
(every two seconds otherwise, so a resumed song's first step stays inside `A_SEEK`), it reads the
state and queue under the player lock and hands them to `Listening::heard`; a failure to store a
listen is warned once until counting recovers. `snapshot` does not count, so no front end's poll
rate decides what a listen is. **It tells the front end**: `watch_player(PlayerWatch)` registers a
callback whose `moved()` is called, off every lock, when an event that changes what is shown arrives
(`TrackStarted`, `TrackFinished`, `QueueFinished`, `OutputChanged`, `Failed`, `Waiting`,
`CommandFailed`, however many arrive at once: one call), so a front end need not poll fast to see a
song change the engine already knows. The thread and `snapshot` both drain the events (whichever
reads one stores its failure or wait through `take_in`); a poll that finds none was just told
(`engine_events_that_change_what_is_shown_tell_the_watch_once_however_many_arrive`,
`an_underrun_alone_does_not_wake_the_watch`).

**A song being opened is the song a snapshot names.** The engine opens every row off its thread and
publishes no `current` meanwhile (`audio.md`'s *Every track opens off the engine thread*), so a
poll landing in a skip read no song and the phones drew an empty player, then the next song with
nothing to animate from. Where the engine names no track while buffering or paused, `current` is the
queue row at `queue_position` (`row_being_opened`), at position 0 with the catalog's length, until
the opening lands (`a_row_still_opening_is_the_song_a_snapshot_names`). **A seek in that window is kept**:
`seek` sends `Command::Seek` at the opening row's catalog rate (`rate_to_seek_in`), which the engine holds as the
place the opening lands at (`a_seek_in_a_row_still_opening_is_sent_at_that_rows_rate`). **A save in that window
writes nothing** (`place_to_save`): a place of 0 would rewrite the queue over the place the previous song
reached, and the previous place stays true until the new row publishes
(`a_save_waits_for_the_row_opening_to_publish_its_place`).

**A paused load waits to land, and its refusal is still told by the snapshot.** `load` and
`load_served` without autoplay send `Command::Resume` through `command_landed` (`Player::settle`),
which waits for the engine to publish the queue and leaves a refusal an announced event, so the next
queue edit reads the revision the load made and never meets `QueueChanged`
(`a_queue_holding_served_songs_is_kept_with_their_names_and_resumed_from_their_uris`,
`a_failure_the_engine_reports_is_told_by_one_snapshot_and_by_no_other`). An autoplaying load is not waited for.

## The catalog does not hold the transport

Nothing the transport needs waits on the catalog's one connection. `snapshot` copies the engine's
events, state and queue under the player lock and reads the catalog after releasing it (and
`store_counting` takes the connection only with something to store), so a long catalog hold delays
the poll alone, never play, pause or seek (`a_snapshot_waiting_on_the_catalog_does_not_hold_up_the_transport`).
A steady tick takes no catalog lock at all: the playing row's `Track` is kept with the URI and the
catalog revision it was read at (`Catalog::kept_track_uri`), read again only when either moves
(`a_snapshot_keeps_the_playing_song_until_the_catalog_moves`). `save` copies the place and rows
under the player lock and turns URIs into ids after releasing it. A run of rows by URI or id
(`tracks_at`, `ids_held_at`, `tracks_by_id`: the queue read, a save, `load`, the resumption at open; a save passes over a row whose document the catalog no longer holds, the place moved past it, and keeps the queue whole again on the next tick) is
read `ROWS_READ_PER_HOLD` (256) to a hold, so a whole-library queue lets a snapshot or a favourite
in between (`rows_read_across_several_holds_keep_the_order_asked`).
The long holds themselves are short: the search index is read in pages of 2 000 rows with the lock
taken per page (stamped with the revision read before the first, so a write between pages makes the
next ask rebuild). **A walk of the library's pages is read from the index it began on**:
`filtered_page(filter, offset, limit, pinned)` answers a `TrackPage` (the songs and the revision of
the index they came from); a walk asks its first page with no pin and every later one with the
revision the first answered, and `Searching::walked` keeps up to `WALKS_PINNED_AT_MOST` (2) indexes a
walk asked within `A_WALK_PINS_ITS_INDEX_FOR` (10 s), so enrichment writing once a second rebuilds
the index once a walk, not once a page; a page whose pin was let go answers the current revision and
the walk starts again (`a_walk_of_pages_reads_every_page_from_the_index_it_began_on_while_the_catalog_moves`,
`a_walk_whose_index_is_no_longer_pinned_is_answered_from_the_catalog_as_it_stands`). A saved search's
pages are walked the same way (`saved_search_walk(search, offset, limit, pinned)` over
`Searching::saved_walked`, which sorts the matches from the pinned index and numbers each by its place
in them as `saved_search_page` does:
`a_walk_of_a_saved_searchs_pages_reads_every_page_from_the_index_it_began_on`,
`a_walk_of_a_saved_searchs_pages_whose_index_is_no_longer_pinned_starts_from_the_catalog_as_it_stands`).
Android's `pagedPinned` and iOS's `Paging.pinned` are those walks. A playlist file or a link's matches take the lock per song over the
`folded(name)`, `folded(title)` and `recording` indexes
(`a_playlist_import_looks_each_song_up_by_index_not_by_scanning_the_catalog`).

## A playlist kept in step with its link

`playlist_links.rs` is the phones' half of `library.md`'s *A playlist remembers the link it came
from*, over the document catalog. Step 24 (`PLAYLISTS_REMEMBER_THEIR_LINK`) made `playlist_links`
(`playlist` primary, the link as `PlaylistLink::page`, `kept`, `refreshed` in Unix seconds) with no
foreign key and the `playlists_born_again_hold_no_link` trigger, so an id SQLite hands out again
holds no stranger's link (an undone delete comes back without its link).
`import_linked_playlist(link, name, songs, kept_in_step)` matches each listed song by `local_match`
(per song, as a link's matches always were) and makes the playlist under a free name; kept in step
it is made even with nothing held, mirrors the playlist already kept from that link, and saves the
songs not held as wants (`save_wanted`, `ON CONFLICT DO NOTHING`, so a given-up want is never armed
again). `refresh_linked_playlist(id)` reads the link through `songs_of_a_playlist` (the reading
`open_online_link` shares): kept in step, the rows become the held songs in the service's order
through `edit_playlist` (one undo step, the revision stepped) and the unheld are wanted; imported
once, it only appends what the playlist lacks. `playlists_due_a_refresh` names the kept ones read
`KEPT_IN_STEP_EVERY` ago; `keep_playlist_in_step` and `forget_playlist_link` change or drop the
link; `MusicPlaylist.link` carries `PlaylistLinkInfo` (service, page, kept, refreshed) in the
listing (`a_playlist_kept_in_step_takes_the_services_songs_and_order_and_wants_what_is_not_held`,
`a_playlist_imported_once_only_gains_what_it_lacks_and_wants_nothing`,
`a_link_is_kept_in_step_due_once_its_hours_pass_and_forgotten_on_asking`).

**A Deezer profile is followed whole, as on the desktop** (`profile_links.rs`, `library.md`'s *A
Deezer profile is followed whole*). Step 25 (`PROFILES_ARE_FOLLOWED_WHOLE`) made `profile_links`
(`link` primary, the profile's `name` as Deezer last billed it, `refreshed` in Unix seconds) and `profile_pages` (the playlist pages it listed,
cascading with it). `follow_profile(link, kept_in_step)` asks `Reference::profile_linked`, reads each
playlist through `songs_of_a_playlist` and lands it through `import_linked_playlist` under its own
title: in step, a page known before with no playlist still kept from it is let go, a page gone from
the profile has its playlist's link forgotten, and the profile and its pages are kept; once, each
link is forgotten as it lands and the profile is followed no longer. A playlist nothing of which is
held is `unread` once (`NoLocalMatches`) and made kept in step.
`profiles_due_a_refresh`, `profiles_followed` (a `ProfileFollowed` of link, name and service, so a
phone names a profile, never shows its link) and `unfollow_profile` serve the timers and the list;
`profile_link_service` and `playlist_link_service` say which service a pasted text names, so neither
phone keeps a host list of its own
(`a_profile_followed_is_due_once_its_hours_pass_and_unfollowed_on_asking`,
`a_profile_link_names_its_service_and_anything_else_names_none`,
`a_playlist_link_names_its_service_and_any_other_text_names_none`).

## A Spotify account on a phone

`spotify.rs` holds what a phone adds to `online.md`'s *A Spotify account*. The front end keeps the
client ID and the refresh token in its own secret store (Android Keystore, iOS's keychain) and hands
them over at start (`configure_spotify`, either blank being no account), so `Services::online` carries
one `SpotifyAccount` over `KeptByThePhone`. A rotation is taken up at once and handed back through
`refreshed_spotify_token`, taken once, for the phone to write. `authorize_spotify(client_id)` binds
the same `SPOTIFY_REDIRECT` the desktop does (`spotify_redirect`, one address the app registers) and
answers the address to open; `finish_spotify_authorization` waits for the browser on the caller's
thread and `cancel_spotify_authorization` ends it. The boundary's listener answers in-process, so the
phone must stay running while the browser is up: Android has a browser in front and the process
alive, iOS an `ASWebAuthenticationSession` over the app (neither seen on a device, `docs/TESTING.md`).
`spotify_account_link` is what `follow_profile` takes (`profile_link_service` offers it to no search);
signed out it is `NotSignedInToSpotify`, a refused token `SpotifySignInLapsed`.
`sign_out_of_spotify` forgets the grant, the account's links and the following, keeping the rows.
**A song is held by its ISRC first**: step 31 (`DOCUMENTS_ARE_FOUND_BY_THEIR_ISRC`) indexes
`documents(isrc)`, and `local_match` takes a visible document of a song's code within 5 s of its length
before the name match, so a song tagged with a code is found whatever its name reads; a song neither
answers for stays wanted. A list read through an account takes `ACCOUNT_SONGS_READ_AT_MOST` (50 000), not
a pasted page's `PLAYLIST_SONGS_READ_AT_MOST`.

## The songs an album lacks

`album_songs.rs` is the phones' half of `ui.md`'s *Album rows = catalog rows merged with the release's
missing ones*, and a phone's catalog keeps nothing else of a release. Step 26
(`ALBUMS_KEEP_THE_SONGS_OF_THEIR_RELEASE`) made `release_songs` (`album_key`, `disc`, `position` primary,
beside what a `SearchSong` carries; no foreign key, since an album is no row of its own), written whole
whenever a release lands on an album: by the enrichment pass (`land_album_release`, in its transaction,
so a new album's list is there offline) and by `learn_album_songs(key)`, for an album matched before
this step: where the album names a release (`album_art.release`, else a song's tagged `release`), keeps
no song of it and online reaches, it asks `Reference::release` once and answers whether songs landed,
a refusal being `Error::Reference`. `album_songs_not_held(key)` reads the list, no network, and leaves
out what the album holds, by the desktop's passes each taking only what the earlier left and one file
answering for one song: recording, title at its place, title anywhere, place alone (a file with no disc
is on the first of a one-medium release), then `local_match`, so a song held under another album is not
offered again. `SearchSong` carries disc and position, so a row is a download as any online row is, and
`release_songs` (the export) and the list share `songs_of`
(`a_kept_release_lists_only_the_songs_the_album_does_not_hold_in_their_order`,
`a_song_is_held_by_its_recording_its_title_or_its_place`,
`one_file_answers_for_one_song_of_a_name_a_release_holds_twice`,
`a_song_landed_under_another_album_is_held_by_its_recording`,
`an_album_with_a_release_is_asked_once_and_one_without_is_never_asked`,
`a_catalog_of_before_albums_kept_their_songs_keeps_its_rows_and_owes_each_matched_album_a_read`). Both phones draw them (`ios.md`'s
*Collection header*, `android.md`'s *Collection pages*).

## The releases and songs an artist lacks

`artist_discography.rs` is the phones' half of `ui.md`'s *What an artist's discography holds that the
library does not is on the artist's page, greyed* and `library.md`'s *A discography is kept once the
profile lands*, over the document catalog, which kept no discography. Step 29
(`ARTISTS_KEEP_THEIR_DISCOGRAPHY`) made `artist_releases` (the artist's MusicBrainz id and the release
group, with its title, kind and first release date), `artist_discographies` (when an artist's was read),
`group_songs` (the songs of the pressing read for a release group, the shape of `release_songs`) and
`groups_read` (a group read, though it held nothing). An artist page is keyed by name, so the artist's id
is the one most of the songs under that folded name carry (`documents.artist_mbid`); a name with no id
asks and shows nothing. Everything reuses the desktop's rules: `worth_keeping` (Album, EP and Single with
no secondary type but Soundtrack) keeps a release group, `pressing_of` picks the pressing whose songs are
read, and a song is offered with the disc and position `songs_of` gives it, so a row is a download as
any online row is.

`learn_artist_discography(name)` asks `Reference::release_groups_of` (the first page and at most three
further, `FURTHER_PAGES_AT_MOST`) when none was kept within thirty days
(`DISCOGRAPHY_KEPT_FOR_SECONDS`) and answers whether releases were kept; `learn_artist_songs(name)` reads
the pressing (`releases_of_group`) of up to eight unheld groups not yet read (`GROUPS_READ_AT_MOST`)
and answers how many it read, so a front end asks again until it answers 0, reading its lists between
the batches and stopping when the page leaves; `learn_release_songs(group)` reads one group for the sheet
of a release. Each is `Ok(false)` or `Ok(0)` while online is off, and a metered connection's shut gate is
the client's own `Error::Metered` (`online.md`), so a lookup the listener's settings forbid is never sent.
The reads below make no request.

`artist_releases_not_held(name)` lists the kept release groups the library does not hold, Albums before
EPs before Singles and each newest first, with the songs of each still lacking (`ReleaseNotHeld`,
`songs_read` false until its pressing was read). **A release is held by** any of the artist's songs
carrying its group (`documents.release_group`, or the group of an album matched through `album_art`), a
song of its pressing the library holds (`documents.release`, which a download sets), an album of the
same folded title, or, for a single, a song of the same folded title. `artist_songs_not_held(name)` lists
the songs of those releases the library lacks, a song on two releases once (by recording, else by folded
title) and left out when the artist holds a song of its title or `local_match` finds it, and
`release_songs_not_held(artist, group)` the same for one release
(`a_release_the_albums_group_or_title_names_is_held_and_the_rest_are_listed_newest_first`,
`a_single_is_held_wherever_its_song_is_and_a_live_album_is_never_kept`,
`the_songs_of_an_unheld_release_are_read_once_and_listed_without_what_is_held`,
`a_song_on_two_releases_is_offered_once_and_a_release_with_no_pressing_is_read_empty`,
`a_song_downloaded_from_a_release_makes_the_release_held`,
`a_discography_is_asked_once_a_month_and_an_artist_without_an_id_is_never_asked`,
`a_discography_with_more_to_read_is_followed_a_few_pages_and_kept_whole`,
`a_catalog_of_before_discographies_were_kept_keeps_its_songs_and_gains_empty_tables`). Both phones draw
them (`ios.md`'s *Collection header*, `android.md`'s *Collection pages*).

**A release not held wears its sleeve** as on the desktop (`library.md`'s *Sleeves of those releases*).
Step 30 (`UNHELD_RELEASES_KEEP_THEIR_SLEEVE`) made `unheld_covers` (`release_group`, `cover`, `asked` in
Unix seconds), the desktop's table. `learn_artist_covers(name)` asks, for up to eight
(`COVERS_ASKED_AT_MOST`) of the artist's unheld releases with no sleeve held and none asked within
`COVERS_ASKED_AGAIN_AFTER`, `Reference::cover` for the pressing its songs were read off
(`group_songs.release`), else `Reference::group_cover`, and answers how many it asked, so a front end
asks again until it answers 0 after the songs are read; it is `Ok(0)` while online is off and the
client's own `Error::Metered` stops it as it stops every lookup. A miss is kept as an ask with no
bytes (the next try waits the month), an answer never blanks a sleeve held, and a failed or refused ask
writes nothing. `ReleaseNotHeld.covered` says a sleeve is held and `unheld_release_cover(group)` reads
its bytes (only what `ImageFormat::sniff` knows), no network
(`a_release_not_held_is_asked_for_its_sleeve_by_the_pressing_read_else_by_its_group`,
`a_sleeve_is_kept_and_not_asked_again_and_a_miss_waits_a_month`,
`a_failed_ask_for_a_sleeve_is_not_remembered_and_a_later_miss_never_blanks_one_held`,
`a_release_that_becomes_held_is_no_longer_asked_for_and_an_artist_without_an_id_is_never_asked`,
`a_catalog_of_before_unheld_sleeves_were_kept_keeps_its_discography_and_gains_an_empty_table`).

## Logging

`logging.rs` is the phones' `tracing` subscriber, safe code only: `keep_log(logbook)` installs once
per process (`false` when a subscriber already stands) a `fmt` subscriber at `WARN` whose writer
hands each finished line, with its `LogLevel`, to the front end's `Logbook` callback, so there is no
`liblog` binding and no unsafe. Android installs it before the player opens (`Logcat.install`, tag
`ResonateNative`, mapped to `Log.e`/`w`/`i`/`d`), so an engine, library or sync warning reaches
logcat; iOS installs none yet
(`a_warning_reaches_the_logbook_with_its_level_and_a_debug_line_does_not`).

## The surface is the same on every target

Every export exists on every target, so bindings generated from the host library match the device
library (`ios/build-native.py` falls back to the host build where library mode cannot read the iOS
archive). What only one platform uses is a no-op elsewhere: `select_output`/`output_device`,
`select_zone_output`'s device,
`set_follow_song_rate`, `output_offers_changed`, `output_stream` and `watch_output_mixer` act on
Android's Oboe routing (iOS asks its session for the song's rate in Swift, so its `Routing` follows
nothing and offers nothing); `output_route_changed(rate, latency_ms, rebind)` feeds iOS's route.
`audio_extensions` is core's `AUDIO_EXTENSIONS`, so a front end never keeps its own list. No
exported name is a Swift or Kotlin keyword: UniFFI backtick-escapes one in Swift and the backticks
leak into the generated C header, which then fails to compile (hence `Documents::open_document`).

## Granted documents

A document is a URI the front end resolves to a descriptor through the `Documents` callback.
`GRANTED_SCHEMES` lists the schemes a `GrantedFiles` provider is registered for, one `SourceId` each:
`content` (Android's SAF URIs, kept as they are so saved locations still resolve) and
`resonate-folder` (iOS: `resonate-folder://<root>/<percent-encoded relative path>`, the root a
security-scoped bookmark the app keeps, or `local` for its own Documents folder). The key is the URI
after `scheme:` with its escaping kept exactly. Anything else, a bare path or a `file:` URI
included, is `InvalidUri`. A negative descriptor says why: `document_not_found()` (no such
document: Android's `FileNotFoundException`, iOS's unresolvable bookmark or `ENOENT`) reads as
`NotFound`, `document_refused()` (any other failure) as `PermissionDenied`, and
`hide_missing_songs` hides only a `NotFound`, so a provider's transient error leaves a song visible
(`only_a_document_that_is_not_found_reads_as_not_found`), and the sync catalog's `holds` reads only a
`NotFound` as not held, the same rule (`android.md`'s *Device sync*). `hide_unlisted_songs(prefixes, listed)` is
the same pass given the URIs the front end saw listed: those are taken to exist and never opened
(`a_document_the_listing_shows_is_not_opened_and_the_rest_are`); `hide_missing_songs` is it with
none listed.

## A day is the phone's zone's

`listening_detail` takes the phone's IANA zone by name beside its current offset, and cuts each day
at that zone's own midnight through `resonate_core::Calendar::named`, so a winter day read in summer
still begins at its winter midnight; the offset is only the fallback for a name no zone database
holds. iOS passes `TimeZone.current.identifier`; Android's statistics read `statistics(days)` alone
and call no day-cutting export yet. Which database each target reads, and why the phones bundle one
behind the system's, is `library.md`'s *A day is the listener's*.

## The listening test

`abx.rs` hands the phones core's ABX session (`eq.md`'s *The listening test*): `ListeningTest` is a
UniFFI object holding the `Session`, the levelled `Arms` and the arm heard; `MobilePlayer` sends the
arms (`hear_in_listening_test`) and puts the preferences' equaliser back (`end_listening_test`), so
the object never touches the engine itself and a front end holds one per run. Refusals are
`Error::ListeningTest*` sentences; the core errors it cannot name pass as `Domain`. The object
compares either the equaliser's `Arms` or a setting (`Compared`): `begin_setting_test` takes a
`TestedSetting` to the engine's audition (`eq.md`), measuring restoration's levels on the caller's
thread, and the engine's refusals pass as `Error::Player`, worded by `failure::of_a_player`
(`NothingToAudition`, `ArmsAlike`, `LevelsUnmeasured` and `LevelsTooFarApart` each their sentence, the
rest by `Cause`). `TestStanding.levels` says how the arms were levelled (`TestLevels`), and
`setting_test_by_default(SettingKind, PlaybackPreferences)` is the `TestedSetting` a phone begins
from: A the preferences' restoration, or quality and phase, or the dither the phones play
(triangular under `Threshold`, `TestedDither::Shaped`), B the opposite (extend or off, low latency or very
high with the phase kept, no dither). Both phones draw the choice (`eq.md`'s *The phones*).

## Lyrics

`lyrics.rs` hands a phone what the desktop pane reads, so neither phone keeps lyric arithmetic of
its own. `LyricSheet` is the sheet once: `detail`, `two_voices`, `end` (the *END OF LYRICS* row,
`lines.len()`), `written_end`, and per row `blank`, `written` (written lines before it, the
falloff's ordinal) and `breathes` (`Lyrics::breathes_before`: room for the dots). `lyric_frame` is
asked at display rate, so it is a lock, a lookup among at most `SHEETS_KEPT` (16) and the
arithmetic: `line` (`line_at`) and `active` stay for Android; `voice_one`/`voice_two`
(`voices_in_play`); `read_at` (`waiting.next`, else `end` once `has_ended`, else `line_at`); `reads`
chosen exactly as `LyricsModel::follow_the_track` chooses (`Evenly` unsynced; `At` the latest lit
line, else the end, else, with no wait, `read_at`; else `Spent`); `waiting`; `ended`; and a
`LyricWipe` per lit line with words, `Sweep::wiped` turned from UTF-8 bytes into UTF-16 code units,
how Swift and Kotlin index a string (`a_wipe_counts_both_halves_of_a_surrogate_pair`).

`kept_lyric_frame` is the same frame answered only from a kept sheet (`None` where none is, unlit
for a `Wordless` song), never the catalog, so a front end may ask it inside a display tick
(`a_kept_frame_answers_only_from_a_sheet_already_held`).

The kept sheets are least recently *read* first out (`KeptSheets`; a frame reads, so the playing
sheet is never the one dropped: `a_sheet_read_lately_outlasts_the_sheets_remembered_after_it`). A
frame for a sheet not kept reads it from the catalog again rather than answering nothing; a song
the catalog holds no words for is kept `Wordless`, so its frames ask SQLite once, while
`cached_lyrics` (behind `lyrics` and the keeping calls) reads the catalog again past `Wordless`. A catalog read that
fails is not kept: the frame is unlit for that call and the next frame asks again
(`a_failed_reread_is_asked_again_by_the_next_frame`).

## MusicBrainz asked of another device

**The phones hold no copy of MusicBrainz; they ask a device that does, and keep what they learn**
(`musicbrainz.md`). The boundary is unchanged:

- `Asking::layered_by` wraps the enrichment pass's `Online` in a `resonate_musicbrainz::Layered` with
  the kept answers (`kept.sqlite` under the sync folder's `musicbrainz/`, opened on first use) and
  the online devices (`sync/consulted.rs`, `Consulting` over the running `Sync`).
- Every `start` hands the `Sync` a `Consultable` over the kept answers alone, so a phone answers
  another device from what it kept, and the answers both kept travel when they meet.

## Answers from other devices

`DocumentCatalog` takes answers (`takes_answers` true) by the rules the desktop's `answers.rs`
keeps (`sync.md`), over the document catalog, in `sync/answers.rs`. A release (`Answer::Release`)
is settled and written nowhere, since the document catalog keeps no release a strict landing could
weigh; the ledger still passes it on in its snapshot.

- **Words.** `lyrics_to_share` lists every `lyrics` row with text of a visible document, in `asked`
  order (`at` is `asked` in nanoseconds), timed or not by `is_synced`, and only those `Answer::travels`
  admits. `take_lyrics` lands only on a document the recording is held strictly by (the in-memory
  `Indexed` resolves it, else `songs_received` does, through `resolved_in`, then the document's recording id is the recording's, or both lengths are
  known and within 2 s), parsed first, as a row with no Lyricsfile and origin `SHARED_LYRICS`, and
  never where the row keeps words as rich already (`LyricText::detail`). `SHARED_LYRICS` is due for
  the pass's Lyricsfile retry as `FETCHED_LYRICS` is. The kept sheet is dropped from `KeptSheets`
  (shared with the player as an `Arc`), so a pane never draws what landing replaced.
- **A study is the file it measured.** The catalog never knew a file by its packets, so
  `documents.packets` and `documents.frames` (the first-packets digest and frame count
  `probe_scanned` reads, the desktop's `tracks.packets` and `tracks.duration`) are kept by the
  study pass right after it studies (`Hearer::keep_identity`) and by landing, which probes a
  document only when its length (`documents_by_length`: rate and `duration_ms` within a
  millisecond) could be the study's and its identity is not yet kept; the identity goes with the
  file (`documents_forget_which_file_they_were` clears it when `size` or `modified` moves).
  `studies` gained `studied`, `studied_under` (`JUDGED_UNDER`) and the figures the catalog lacked
  (`cutoff_hz`, `cutoff_drop`, `lossy_guess`, `upsampled_from`, `declared_bits`,
  `loudness_range`, `mono_as_stereo`), all written by `keep_studied`; `keep_study` (the analysis
  pane, which has no such figures) leaves `studied_under` null. `studies_to_share` lists a row
  judged under this build with whole band levels (the file-change trigger clears them) on a
  document with a kept identity; a study from before the migration is listed once its song is
  studied again. `take_study` lands on every visible document whose rate, frame count and
  packets digest are the study's, never over a row `studied_under` this build (a row with no
  `studied_under` is a study from before and stays too), over one of another judgement it replaces and
  clears its `agreement`. A verdict, lossy guess, judgement or band layout this build cannot
  name is settled and written nowhere. **The band peaks travel as an answer of their own**
  (step 16's `band_peaks`, whole at `BAND_PEAKS_BYTES` since step 17 cleared the one-moment
  layout, `sync.md`): `band_peaks_to_share` lists a row judged under this build with whole peaks on a document with a kept identity, the same `WholeFile` as the study, and
  `take_band_peaks` writes them on every visible copy of that file studied under this judgement
  whose peaks are not already whole, never over peaks held; a judgement or length this build cannot
  read is settled and written nowhere, and a copy not yet studied takes nothing (the study is
  listed first, so it lands first; a phone that missed it measures its own peaks alone). A study
  landed without peaks reads as unmeasured to `bands_to_measure` until they arrive or the pass
  measures them (`the_peaks_of_a_study_land_on_every_studied_copy_of_the_same_file_and_never_over_peaks_held`,
  `peaks_of_another_encode_judgement_or_layout_are_written_nowhere`,
  `the_peaks_a_phone_kept_are_listed_beside_its_study_with_the_file_it_measured`).
  (`a_study_lands_on_every_copy_that_is_the_same_file_and_on_no_other_encode`,
  `a_study_of_another_encode_another_length_or_another_rate_is_never_written`,
  `a_study_never_lands_over_one_at_this_judgement_but_replaces_one_of_another`,
  `words_land_only_on_a_song_held_strictly_and_never_over_as_rich_words`,
  `landed_studies_are_listed_for_sending_exactly_as_they_arrived_beside_kept_words`,
  `a_study_the_phone_kept_itself_is_listed_with_the_file_it_measured`).
- **A print and its recognition are kept beside the study and asked of a device, not told.** The
  catalog's step 15 (`STUDIES_KEEP_THEIR_PRINT_AND_WHAT_IT_WAS_HEARD_AS`) gave `studies` the
  desktop's `print`, `print_length`, `recognised` (nanoseconds) and `heard_as`, `heard_score`,
  `heard_title`, `heard_artist`, all cleared by the `documents_forget_what_was_printed` trigger when a
  document's `size` or `modified` moves. The pass's `keep_studied` keeps the print its study made
  (the analysis pane's `keep_study` none, its row being judged under no build); `land_study`
  clears them where it replaces a study of another judgement. `recognised_through` (the pass and
  `recognise_track`) keeps the top match heard (`HeardAs::top_of`) and stamps `recognised` through
  `keep_recognition`; a study that made no print is `Unheard` with `recognised` left null, so a print
  landed later is still heard. The seam is the desktop's (`sync.md`), its shared halves
  `resonate-library`'s `KeptPrint::shared` and `weighed_again`: `prints_wanted` lists the
  `WholeFile`s (`documents.packets`, `frames`, `rate`) of visible documents studied under this
  `JUDGED_UNDER` whose `print` or `recognised` is null, one entry a file, the most lately studied
  first; `print` answers the print of such a copy, a recognised one first; `take_print` lands the
  print on each copy of that file studied under this judgement whose `print` is null, never over one,
  and the recognition on each whose `recognised` is null, weighed again against the copy's own name
  (`named_on_the_phone`). The service asks connected devices on the desktop's pacing. **The pass skips
  the second decode once a print is kept**: `Hearer::hear` recognises from `kept_print` without
  studying, and `tracks_to_hear` takes a row whose print is kept and recognition owed even where its
  own study left it `Unheard`
  (`a_print_and_its_recognition_from_another_device_land_on_every_copy_and_are_weighed_against_the_phones_own_names`,
  `a_print_and_recognition_the_phone_made_answer_another_device_lacking_them`,
  `a_print_alone_leaves_the_recognition_owed_and_one_that_heard_nothing_lands_as_unheard`,
  `a_print_kept_beside_a_study_is_forgotten_when_its_file_changes`,
  `a_print_landed_from_another_device_is_heard_without_decoding_the_song_again`).

## The equaliser

The room correction reads a SOFA set at the speaker pair the boundary holds (`room.rs`:
`speaker_angle`, `speaker_elevation`, `head_yaw` and their setters, `speaker_limits()` for the
bounds; `eq.md`).

The phones' curve, its bounds, the drawn response, imports and stored form are `eq.md`'s *The
phones* (`equaliser.rs`); `playback.rs` sends it with the other sound preferences. Both phones draw it
as the one page `eq.md`'s *The phones* lays out (the graph with the chosen band's card docked under
it, corrections on pages of their own), every call that page needs already on this boundary
(`equaliser_limits`, `response_curve`, `parse_eq_profile`, `measurement_offers`, `fit_measurement`,
`fit_recorded_sweep`, `fitted_preamp`, AutoEq's search and profile, room correction and the listening
test).

## Output

`output/mod.rs` picks one backend per target behind `output::player` and one `Routing`:

- **Android**: Oboe, music-typed, opened in whichever of three ways the plan's spec is offered by
  (`android.md`'s *The route's formats*). The boundary's vocabulary is `output/formats.rs`, on every
  target: `OutputFormat` (rate, `OutputEncoding` `Pcm16`/`Pcm32`/`Float`, one or two channels),
  `OutputPath` (`Mixed`, `Exclusive`, `BitPerfect`), `OutputStream` (what opened and how) and the
  `OutputMixer` callback (`prefer_bit_perfect(device, format)`, `mix_freely(device)`). What decides
  is compiled on the host as well, so its tests run there: `output/oboe_offers.rs` (`Offers`: the
  shared mixer's `MIXED`, 48 kHz stereo float, always, and while following the song the route's
  bit-perfect formats and its probed exclusive one, each its own `SinkFormats` entry so no rate and
  word are crossed; `Way` for a spec), `output/oboe_state.rs` (`Routing`: the requested and actual
  device, the switch, the offers, the probe made once per chosen device and route, the mixer
  preference held and given back, the stream told by a number so a late close never clears the
  next) and `output/oboe_frames.rs` (the callback's fill, realtime). The output thread's waits and
  the bounded open are `output/waits.rs`. **The stream is kept across a rebind** (`audio.md`):
  `output/oboe_reshape.rs` holds `Order` (what the output thread is sent: a `StreamCommand`, or
  `Reshaped`, which calls off a drain standing from before), `Opened` (the spec, the effective `Way`
  after the open's fallbacks and the device asked, sent back with the open) and `reshaper`, which
  keeps the stream only where the request is `Opened`'s spec, `Routing::requested` is its device and
  `Routing::way_for` its `Way`, handing the callback the new source through `output/handover.rs`;
  anything else is refused (`AnotherFormat`, `AnotherTarget`, `AnotherPath`) and the engine reopens
  (`a_stream_asked_for_the_shape_it_holds_is_given_the_new_source_and_kept`,
  `a_device_chosen_since_the_stream_opened_is_refused_so_the_stream_opens_on_it`,
  `a_route_that_now_offers_the_song_its_own_way_is_refused_so_the_stream_opens_that_way`).
- **iOS**: RemoteIO at the session's hardware rate, interleaved Float32 stereo; the engine resamples
  into it. **The unit is `resonate_apple::Unit` and the fill `resonate_coreaudio::Rendering`, both
  shared with the macOS backend**; what is the phone's alone stays here and is compiled on the host
  too, so its tests run there. `resonate-apple` holds the sequence (`Sequenced<T: Toolbox>` over
  the AudioToolbox calls a unit takes: find the component, make an instance, set the format, set the
  callback, initialize, start, stop, uninitialize, dispose; every refused step an `Error::Refused`
  naming its `Op` and the `OsStatus`, a unit made taken down by `Drop` in the order stop,
  uninitialize, dispose and what the callback was handed freed exactly once however far the open
  got), the callback's `extern "C-unwind"` shell and its reading of the system's buffer list
  (`buffers::rendered`: the buffer filled up to the bytes it declares and no further, a null list, a
  buffer with no memory, one declaring no bytes, the list's size and alignment; `guarded` is the
  panic catch, taking what to do on a panic, `process::abort` in `render`), all tested on the host
  against a fake `Toolbox`. `resonate-coreaudio` holds `Rendering::fill` (a short fill zeroed, the
  `empty` flag set exactly when the source gave nothing, the stamp) and `Drain` (when a drain is
  over: the latency after the ring empties, never under 20 ms, restarted by a source that fills
  again). `output/remote_io_state.rs` holds `Routing` (the rate, latency and sink-change atomics and
  channels); `remote_io_stream.rs` holds `Hardware` (start, stop and `reformat(rate)`), `launch` (the
  format check, the open thread, the `UNIT_WITHIN`-bounded wait for the unit, the close that waits
  for disposal) and `serve` (the commands, a refused start or stop told as `StreamEvent::Refused`
  with the status, the drain, the moved latency), tested against a fake `Hardware`; `remote_io.rs`
  is `cfg(target_os = "ios")` and holds `IosAudio`, the `Hardware` over `resonate_apple::Unit`
  (`Component::RemoteIo`, no device, Float32 stereo at the route's rate) and the reading of a
  `resonate_apple::Error` into `Error::Backend` naming `Operation::Open` (a missing component
  `NotFound`). **The unit is kept across a rebind and a turn of the route's rate**: `reformat` is
  `Unit::reformatted` (stop, uninitialize, set the format, initialize, stopping at the
  first step the system refuses and returning its status), and `launch` gives the stream a
  `Reshaper` that sends the output thread `Order::Reshape { rate }` (the route's rate now), waits at
  most `UNIT_WITHIN` for its status and only then hands the source to the render callback through
  `handover.rs`, so a refused reformat hands the source back and the engine closes the stream and
  opens another; the thread calls off a drain standing from before, reformats only where the rate
  moved and then announces `FormatChanged` (and the pause the reformat made)
  (`a_stream_asked_for_the_shape_it_holds_takes_the_new_source_and_the_unit_is_left_alone`,
  `a_drain_asked_before_a_reshape_is_called_off_so_the_new_source_is_not_drained`,
  `a_stream_that_drained_is_reshaped_and_started_again_rather_than_reopened`,
  `a_rate_the_route_took_since_the_unit_opened_reformats_the_unit_and_says_so`,
  `a_reformat_the_system_refuses_hands_the_source_back_and_announces_nothing`; in `resonate-apple`,
  `a_unit_reformatted_is_stopped_uninitialised_set_and_initialised_again_at_the_new_rate`; a unit
  reformatted on a device is not proved, below). The shape is still the route's alone
  (`a_shape_other_than_the_routes_is_refused_before_the_unit_is_touched`).
  Their other tests (`a_step_refused_after_the_instance_took_the_unit_down_and_frees_the_render_once`,
  `a_refused_start_is_told_with_its_status_and_leaves_the_stream_failed`,
  `an_open_that_never_answers_times_out_and_the_late_unit_is_disposed_by_its_own_thread`,
  `closing_returns_only_once_the_unit_is_disposed`,
  `a_callback_that_panics_is_caught_and_answered_by_the_abort_alone`,
  `a_short_fill_zeroes_the_rest_and_leaves_the_flag_down`,
  `a_drain_waits_the_latency_after_the_ring_empties_and_then_settles_once`,
  `a_moved_rate_is_announced_once_and_an_unmoved_one_is_not`). Swift reports the rate and the output latency (`outputLatency + ioBufferDuration`)
  through `output_route_changed` (after asking the session for the playing song's rate, `ios.md`'s
  `NowPlaying`); a moved rate announces `SinkChange::Reformatted` and sends
  `Command::FollowTheFormats`, so the engine reopens at the new rate once the survey has read it
  (`audio.md`), and `rebind` sends `SetSink(None)` (a media-services reset).
  A refused `AudioOutputUnitStart` or stop is a `StreamEvent::Refused` carrying the `OSStatus`, so the
  engine's failure names the operation and the status (an inactive session, an interruption not yet
  ended). The output thread sleeps until a command or a moved latency (`Routing::route_changed`) wakes it, and polls every 10 ms only while draining. The render callback writes the source's bytes straight into the system's buffer and zeroes a short
  fill; the stream clock is the reported latency, and a fill is stamped at the buffer's
  `AudioTimeStamp::mHostTime` (where `HostTimeValid`, read in nanoseconds through the
  `mach_timebase_info` read as the unit opens, `HostClock`, against `mach_absolute_time` in the callback) plus that latency
  less the buffer's own frames (the `ioBufferDuration` the host time already counts; `realtime.md`).
  Drain waits that latency after the ring empties.
  `close` returns once the output thread has disposed the unit (waiting at most 3 s), so a rebind
  never opens the next unit beside a living one, and an open waits at most as long for the unit
  (`UNIT_WITHIN`): a media-services stall in `AudioComponentInstanceNew` is a timed-out open that
  the engine parks, not a blocked engine thread, and the late unit is disposed by its own thread.
- **Elsewhere** (host tests): PipeWire through `Player::with_sources`.

On both phones `output::beside` wraps the platform's backend in `resonate_upnp::Beside` over the one
`Network` and the one `Casting` a `MobilePlayer` holds (`rendering::Kinds`), so every zone's engine
also lists the renderers and Google Cast receivers found; the host build leaves PipeWire unwrapped
and its tests wrap their fake backends instead (`upnp.md`'s *On the phones*, `cast.md`'s *On the
phones*).

## Zones

**The boundary owns a `Zones`, never one player** (`zoning.rs`, `audio.md`'s zones). `Playing` holds
the `Zones`, the zone shown (`shown`, the first until another is chosen) and a `Zoned` per zone: its
own output `Routing`, failure, waiting, saved place and `Listening`, and the route string it was bound
for. Every call the phones made before addresses the zone shown (`Playing::player`, `send` through
`Zones::send`, so a setting, the equaliser and room correction reach every zone and a transport
command the shown one); the snapshot names it (`Snapshot.zone`) with `Zones::revision`
(`zones_revision`), so a front end reads a zone's queue again when another is shown, though its
revision may equal the last. `zones()` lists each zone as a `PlayerZone` (id, name, route, requested
device, shown, volume, transport and the title it plays), `show_zone`, `add_zone(name, device, route)` (a
blank name is *Zone N*), `rename_zone` (blank is `ZoneUnnamed`), `remove_zone` (the first is
`FirstZoneStays`; its listen is settled and its catalog rows go), `select_zone_output(zone, device,
route)` and `hand_over_to(zone)`, which hands the shown zone's song to another through
`Zones::hand_over` (`lockstep.rs`; the boundary does not wait on the handover thread) and shows that
zone. `select_output` and `output_device`/`output_stream` read the shown zone's `Routing`; follow-song-rate,
the route's bit-perfect offers, the mixer and iOS's route reports reach every zone's (`Outputs` keeps
the last of each for a zone opened later) and `FollowTheFormats` goes to the zones whose route moved.

**A zone opens over a `Routing` of its own** (`Opener`, the `Opens` the boundary hands `Zones`): the
first zone's sink is `OUTPUT`, a later one `Routing::serving(SinkId::new(zone))`, so the zones are
never one sink to `Zones::holder_of` (the phones' system mixes, and one zone's bit-perfect stream
refused every other zone a `SetSink`); its device is the id it was added or bound with (Oboe's
`setDeviceId`, 0 the system's choice), its engine the first zone's `OutputSettings::applied_to`, and
the room correction applied is sent to it once open. iOS has one route: every zone's RemoteIO unit
plays into it (`FEATURES.md`), so a renderer is the only other room an iPhone's zone reaches.

**A zone may play on a network renderer** (`upnp.md`'s *On the phones*): `Zoned.renderer` is the one it
was bound to, and every `SetSink` the boundary sends (`select_output`, `select_zone_output`, the
`rebind` of `output_route_changed`) sends the zone's wanted sink (`Zoned::wanted_sink`: the
renderer's node, else none), so choosing an Android device or a route reset never takes a zone off
its renderer; `select_zone_renderer(zone, None)` does. The catalog's step 22
(`ZONES_KEEP_THEIR_RENDERER`) gave `zones` a `renderer` (the node) and `renderer_name`, kept by
`keep_zone`; a zone reopens wanting it (`Bound`, and `first_zones_sink` for the first) and
`MobilePlayer::new` searches where any kept zone names one. A renderer the search cannot hear is
added by the address of its description (`add_renderer_address`, `remove_renderer_address`,
`renderer_addresses`; `upnp.md`'s *On the phones*): the `Network` holds the addresses and reaches
them with no search, the host keeps the list (the zones sheet's *Add a renderer by address*) and adds
it again at start; *Find renderers* off only stops the search. **A Google Cast receiver is a renderer
to the boundary** (`cast.md`'s *On the phones*): `renderers()` lists both kinds (`NetworkRenderer.kind`),
`select_zone_renderer` takes either (a node answering as an alias of a listed receiver is kept as
its current name) and a receiver the search cannot hear is added by `add_receiver_address`,
`remove_receiver_address` and `receiver_addresses`, the host keeping that list apart
(`receiver-addresses`) beside the renderers' and adding it again at start.

**A device another zone holds bit-perfect is refused, as the desktop refuses it** (`audio.md`'s
`Zones::holding`): each zone's sink being its own, `Zones::send` never sees two zones on one sink, so
the boundary compares the Android device ids. `Playing::holder_of(asking, device)` takes the zones
`Zones::holding` names (a zone whose settings prefer bit-perfect and whose output mode is
`BitPerfect` or `Repacked`) and finds one whose `Routing` requested `device`, or (for a device other
than 0, the system's choice) opened on it (`reaches`); `add_zone`, `select_zone_output`,
`select_output` and `hand_over_to` refuse it with `resonate_engine::Error::DeviceHeld { by }`
(`Cause::HeldByAnotherZone`'s sentence), a handover also where the holder is the zone handing over
and means to sound (`handover_clashes`, the engine's own rule). Two zones on a mixed device share it,
and a zone that turns bit-perfect after another chose the device is not undone, as on the desktop;
iOS holds no device per zone (one route) and refuses nothing (`OUTPUTS_ARE_HELD_PER_DEVICE`). The host's
`Routing` remembers the device selected so the rule is proved there
(`a_device_another_zone_holds_bit_perfect_is_refused_to_every_way_of_binding_a_zone_to_it`,
`a_device_is_reached_by_the_choice_made_or_the_one_the_system_gave`,
`a_handover_onto_a_held_device_is_refused_unless_the_holder_is_the_zone_handing_over_and_silent`).

**Each zone's queue is kept under its `ZoneId`**: the catalog's step 18
(`ZONES_KEEP_THEIR_OWN_QUEUES`) made `zones` (id, name, route), `zone_queue` and `zone_resume`
(keyed to `zones(id)` with `ON DELETE CASCADE`; `zone_queue.track` references `documents`, so
forgetting and handing over songs reach it as they reach `queue`). The first zone keeps `queue` and
`resume`, its `zones` row holding only a name, route or volume given;
`save_rows_of`, `save_place_of`, `resume_rows_of` and `place_among_rows` read either through `QueueKept`.
`save` saves every zone; `MobilePlayer::new` resumes the first zone, then opens each kept zone at the
system's device with its kept queue paused and at its kept volume (`restore_zones`); the front end
binds a kept route to the device it now names (`select_zone_output`). A song gone or deleted leaves every zone's queue
(`drop_from_queue`, every run of its rows in one `Command::RemoveAll`, asked again where the queue moved meanwhile), and `documents_moved` saves and resumes each zone holding a moved song. One
`resonate-watch` thread follows each zone's events, counting that zone's listening and telling the
watch (`watching::keep(player, zone)`; it ends when the zone's engine goes)
(`a_zone_added_keeps_a_queue_of_its_own_and_every_call_follows_the_zone_shown`,
`each_zones_queue_is_kept_under_its_id_and_goes_with_the_zone`,
`a_zone_renamed_keeps_its_new_name_and_a_blank_name_or_the_first_zones_removal_is_refused`,
`the_song_handed_to_another_zone_plays_there_and_leaves_the_zone_it_came_from`,
`a_route_serving_another_zone_announces_its_own_sink`).

**A queue comes back as it was arranged, as the desktop's `Resumption` does** (`arranged.rs`): the
catalog's step 33 (`QUEUES_KEEP_THEIR_SHUFFLE_LIST_ORDER_AND_ROWS_PLAYED_NEXT`) gave `queue`,
`zone_queue` and `served_rows` a `loaded` (the row's place in the list order, the engine's
`QueueRows::in_load_order`) and `resume` and `zone_resume` a `shuffle`, `next_first` and `next_last`
(the rows played next, as play positions). `save_zone` keeps the rows in play order with each one's
`loaded`, the shuffle and `Queued::next` as one `Arranged`, and saves them again where the queue's
revision or the shuffle moved (`SavedPlace::arranged_as`); `resume_saved_in` reads it back
(`resume_rows_of`; a row read before the step lists in play order unshuffled through
`coalesce(loaded, position)`) and sends `Arranged::resumption`: the rows in list order, each
row's place in it as `order`, the shuffle and the rows played next. A row passed over on either
side (`Arranged::retained`) moves the place and the rows played next down with it, and a kept gap
(a forgotten song's row) is read as the rows that are left (`KeptPlace::next_among`)
(`a_shuffled_queue_comes_back_shuffled_with_its_list_order_and_the_rows_played_next`,
`a_catalog_of_before_queues_kept_their_order_resumes_them_in_play_order_unshuffled`,
`rows_played_next_follow_the_rows_left_when_a_kept_row_is_gone`). The phones change nothing: the
snapshot's `shuffle`, `next_first` and `next_count` already follow the engine.

**Each zone's volume is kept beside its device**, as the desktop's: the catalog's step 19
(`ZONES_KEEP_THEIR_VOLUME`) gave `zones` a `volume` (0 to 1, full by default). `save` keeps a zone's
volume (`keep_volume_of`) when the engine's has moved more than `VOLUME_KEPT_WITHIN` from the one
last kept, upserting the zone's row without touching its name or route (so the first zone, which has
no row until something is given, gets one only when its volume leaves full); `MobilePlayer::new`
starts the first zone's engine at it (`first_zones_volume`) and `zone_opened` opens each later zone
at its row's, so `set_volume`, which addresses the zone shown, is each zone's own and survives the
next start. `PlayerZone.volume` hands it to a front end; iOS reads its slider's first position from it and keeps no
volume preference of its own
(`each_zones_volume_is_kept_where_it_moved_and_a_zone_opens_at_the_volume_it_was_kept_at`,
`the_first_zone_reopens_at_the_volume_kept_beside_it_and_full_without_one`,
`a_catalog_of_before_zones_kept_their_volume_gains_a_full_one_for_each`).

**Another device drives every zone, not the zone shown** (`MobilePlayer::driven`, `sync.md`'s *A phone is driven too*): it reaches each through `Zones::send`, and the watch, the save and the front end's poll follow as for a press here.

**A handover to another device is sync's**: the phones already continue another device's queue
(`sync_continue_from`, into the zone shown) and offer the zone shown's queue and place
(`DocumentCatalog::resumption` reads the shown zone's `QueueKept` through the `shown` cell `Playing`
shares with it, the first zone's `queue` and `resume` while it is shown;
`a_handoff_offers_the_queue_and_place_of_the_zone_shown_and_not_the_first_zones`).

## TIDAL sign-in

`authorize_tidal` asks TIDAL for a device code and `finish_tidal_authorization` waits for it on the
caller's thread, as the seam's `SignsIn::authorized` does. Each `authorize_tidal` is an attempt
numbered before anything is asked (`Downloads::sign_ins`); `cancel_tidal_authorization` moves the
number on, so the wait's cancel (`Ok(None)`, heard within the seam's `LOOKED_AT_EVERY`) ends it, and
an attempt cancelled while its code was still being asked never stores the code a later attempt
would wait on (`a_sign_in_cancelled_and_asked_again_waits_for_the_new_code_alone`,
`a_cancel_ends_a_sign_in_waiting_for_its_code`).

## Last.fm on a phone

`authorize_lastfm(key, secret)` answers the Last.fm page to allow Resonate on (`online.md`'s
*Last.fm*), keeping the wait in `Services::lastfm_signing`; `finish_lastfm_authorization` waits on the
caller's thread, ending on a cancel, the client no longer reaching or the lapse, and answers a
`LastfmSignedIn` (`name`, `session`) or none; `cancel_lastfm_authorization` ends it. The password
sign-in (`lastfm_sign_in`) is gone. Both phones open the page in the system browser and keep the
session as before.

## Jellyfin Quick Connect

`begin_jellyfin_quick_connect(server)` answers the code (`JellyfinQuickConnect`; `QuickConnectOff`
where the server has it off or is Emby), `finish_jellyfin_quick_connect` waits on the caller's thread
for a `JellyfinSession` (token, user id, device, name) or a cancel, and `cancel_jellyfin_quick_connect`
moves the attempt number on, as TIDAL's sign-in does. `DownloadAccount` carries `token`, `user_id` and
`device` (`#[uniffi(default = "")]`, so a front end's construction stands); `is_ready` takes a server
and either a login or the whole session, and `reached()` hands it to the provider as `KeptSession`.
Both phones keep the three sealed beside the other fields (Android's records stay version 2, the keys
optional; iOS's keychain item decodes them as optional), save the account on approval with the
server's user name and no password, and let the session go when a password is typed.

## Downloads keep what they received

`downloads.rs` writes a resumable offer into `partials.rs`' folder (`download-partials/` under the
player's folder) before copying it to the descriptor, and a later job for the same want resumes it
with `Range`; `android.md`'s *A download that breaks off resumes* has the whole design. The sweep at
`MobilePlayer::new` is `Downloads::under`, and `Downloads::default()` (no folder) keeps nothing.
`Held::resumed` (core) is the lock on an existing partial that does not empty it.

## Metered connections on the phones

The boundary holds the same two gates as the desktop's `metered.rs`, over one `Metering` in
`Services`: `report_connection(ConnectionMeter)` is how a phone says what its network is (`Unknown`
where it cannot tell, which is not metered), `configure_enrich_on_metered(allowed)` (default off,
so lookups are skipped while metered) and `configure_stall_downloads_on_metered(stalls)` (default
off) are the two switches, and `downloads_wait_on_the_meter()` says whether a download is
waiting. The lookup gate is on the one `Client` (`online.md`) and ends an enrichment pass
`Offline`; `Downloads::waits_out_the_meter` holds `find` before it searches and `download` before
each chunk, ends on a cancel, and leaves the partial for the next job to resume. A song played in
place is not a download.

Each phone reports its network and keeps the two switches (`enrich-on-metered` and
`stall-downloads-on-metered`, both off, in the phone's own settings store, handed to the boundary at
start and on a change). Android's default-network callback reports a network without
`NET_CAPABILITY_NOT_METERED` as `Metered`, one with it as `Unmetered` and `onLost` as `Unknown`
(`ConnectionMeters.of`); iOS's `NWPathMonitor` reports an expensive or constrained satisfied path as
`Metered`, another satisfied path as `Unmetered` and an unsatisfied one as `Unknown`
(`ConnectionMeters.of(satisfied:expensive:constrained:)`). Each phone reports every path change, the
latest meter read when the call runs, and polls `downloads_wait_on_the_meter()` beside its download
progress to show the waiting line *Waiting for a network that is not metered*; the switches sit beside
*Only on Wi-Fi* (library tools) and the hosted-services switch (download sources), which are the
phone's own gates and stay.

## Reading document rows

`catalog::read` takes a document row by position, the first columns of `documents` in `V1`'s
order; a later step only appends columns, so `SELECT *`/`d.*` stays readable. A query adding its own
columns after `d.*` reads them by name (`position`, `listen_id`), never by index, since a later
step would move them.

## Checks

`cargo test -p resonate-mobile` runs on the host.
`every_word_the_phones_show_is_said_through_the_catalog` refuses a prose literal anywhere in the
crate's sources (`language.md`'s *The checks*). The queue edit tests drive a real engine over
a backend with no device (a paused queue never binds a stream; `Harness::sounding` lists one sink
whose stream nothing pulls, for a queue that plays) and WAVE documents through the
`content` provider; that `Harness` (`harness.rs`, test-only) is shared by the organise, inspector,
analysis, snapshot and listening tests. A listen counted for a song the catalog no longer holds
stores nothing and settles nothing: `store_counting` keeps no visit unless the insert changed a row
(`a_listen_counted_for_a_song_the_catalog_lost_does_not_settle_over_another_listen`). The enrichment tests drive the pass over a fake `Reference` through `Asking`
(the reference and the client whose switch it reads): one cover per album, a miss left alone until
its wait is over, a prompt stop, the mark kept on closing, the album key and the artwork fallback.
Its documents tests cover both
schemes and
`the_ios_document_types_admit_what_a_scan_admits` holds `ios/Resonate/Info.plist`'s declared
extensions to `AUDIO_EXTENSIONS`. `a_two_word_search_over_twenty_thousand_songs_answers_within_its_budget`
holds the search index (`android.md`) to 30 ms, the best of five, over 20 000 rows (150 ms with
debug assertions, where the workspace's optimised debug profile still runs it). Target clippy runs through `android/check-native.py` and, for
iOS, `cargo clippy -p resonate-mobile --target aarch64-apple-ios` with the environment
`ios/build-native.py` sets. The layering job refuses PipeWire, GPUI, MPRIS, Discord and Oboe in
`resonate-mobile`'s iOS tree.

## Services played in place

`served.rs` is the phones' half of `providers.md`'s *A Subsonic server played in place*, *A Jellyfin or Emby server
played in place* and *Other services played in place*: Subsonic, Jellyfin or Emby, TIDAL, the keyed Resonate server, hifi-api and Monochrome
browsed and played where they are, never copied into the document catalog. The front end hands the
download accounts that are on and ready (`serve_from(Vec<DownloadAccount>)`, the list a download is
given; empty while online is off). The boundary keeps a ready one (`is_ready`: Subsonic's or Jellyfin's server,
user and password, TIDAL's client id and refresh token) that `Downloads::in_place_of` builds as a
download would (`Reached`, one construction for both: a hosted Monochrome or hifi-api is left out
while hosted services are declined, a hifi-api account with a key is the `resonate` service, and a
TIDAL account hands a rotated token to `refreshed_tidal_token`), the first account a scheme wins. `MusicProvider::Jellyfin` is in-place only: `Downloads::fetches_for_wants` leaves it out of `find`, so a want is never asked of it (`a_jellyfin_server_is_browsed_and_its_song_is_queued_and_played_and_is_never_asked_for_a_want`).
The list is compared with the one held, so a call with the same accounts builds nothing and every
read may bind first; a service whose account stands keeps its `Arc` (one TIDAL session). One slot
(`Arc<RwLock<Vec<Service>>>`) is read on each open by a `ServedSongs` per scheme of
`PLAYED_IN_PLACE`, each a `MediaProvider` registered in every `Sources` the boundary makes
(`Served::sources`), so a stream of known length is probed and seeked by range and one read onward
is spooled (the binary's `ServedSongs` is the same thing over its `Sources`; no provider crate may
see `resonate-codec`). `served_services` lists them (`ServedService`: scheme and title).
`served_first_page(service)` (a Subsonic server's artists, a TIDAL account's favourites with its
favourite mixes among the albums as records of the same shape, a mix's uri being `<scheme>:mix/<id>`,
else nothing) and `served_search(service, words)` take the scheme; `served_albums(artist uri)`,
`served_songs(album or mix uri)`, `served_song_uris(any uri)` and `served_cover(uri, known name, side)`
reach the service the uri's scheme names. Each needs online (`OnlineOff`), the service
(`NoServedServer`) and a uri it names (`InvalidUri`), and a provider failure is
`downloads::failure_of` under the account's `MusicProvider`. The records carry URIs
(`<scheme>:track/<id>`), the cover name as an opaque string, and a song's length in milliseconds.
`load_served`, `play_served_next` and `add_served_to_queue` put songs by uri in the queue as
`load`, `play_next` and `add_to_queue` put catalog ids, with ids minted beside the queue's own
(`every_service_set_is_offered_and_each_is_searched_and_played_from_its_own_uris`,
`a_service_declined_as_hosted_is_not_served_and_a_service_no_longer_given_goes`).

**The queue is read whole or as a window.** `queue()` answers every row; `queue_window(heard_at_most,
ahead_at_most)` answers a `QueueWindow` (`first`, `total` and the rows from `first`), the rows
around the one playing (`Shown::around`), so a front end keeping a long queue in view reads a few
hundred rows rather than every row's catalog lookup
(`the_queue_is_read_as_a_window_around_the_row_playing`). **A list of library and served songs loads
as one queue**: `load_songs(songs, start_at, autoplay)` takes `QueueSong::Library { id }` and
`QueueSong::Served { uri }` in the order given, the served rows' ids minted beside the library's, as
`load` and `load_served` each do for their own kind
(`library_and_served_songs_are_loaded_together_in_the_order_asked`).

**A row whose catalog row is gone stays in the queue, and a saved row that does not read is passed
over.** `queue()` reads through `tracks_held_at`, so a row whose document was deleted is listed as a
`Track::gone` (its uri, its file name as the title, id 0) and every later row keeps its index;
`resume_saved_in` skips a kept row whose uri does not read (a `resonate-displaced:` row organise left)
and moves the kept place down by the rows it skipped before it
(`a_queue_row_whose_catalog_row_is_gone_stays_listed_beside_the_rows_that_are_held`,
`a_saved_row_whose_uri_does_not_read_is_passed_over_and_the_place_follows_the_song`). **`jump_to_at(index,
ms)` jumps and seeks in one call**, converting at the target row's catalog rate
(`a_jump_with_a_place_seeks_at_the_rate_of_the_row_it_lands_on`). **`shutdown` waits up to three
seconds for a zone's last other holder** (`sole_holder`, the listening thread holds one a moment)
before stopping the engine, so a service made at once never opens a second engine beside a running one.

**A served row is not a document, but it is kept.** `Served::names` is the scheme alone; `queue()`
and the snapshot's `current` give such a row a `Track` with id 0 made from what the browse returned
(`served_first_page`, `served_songs`, `served_search` and `served_song_uris` remember the songs they
listed, cleared when the accounts change; a song not remembered is titled by its uri), carrying its
`queue_id` like any row. **A queue holding one is saved**: the catalog's step 20
(`QUEUES_KEEP_SONGS_PLAYED_IN_PLACE`) made `served_rows` (zone, position, uri, the names the browse
gave, and `rate`, the playing row's sample rate where it was the one playing), beside `queue` and
`zone_queue` at the same positions; `save_rows_of` writes both in one transaction, `resume_rows_of`
reads them back in order (`KeptRow`), `place_among_rows` counts both, and `resume_saved_in` resumes
a served row from its uri with its names remembered, at its kept place where its rate was kept, else
from its start. `forget_zone` drops its rows (no foreign key: the first zone has no `zones` row). The
handoff to another device names catalog rows alone (`place_among_documents`)
(`a_queue_holding_served_songs_is_kept_with_their_names_and_resumed_from_their_uris`). **A listen of
one is not counted** (`store_counting`): the phones' `listens` name a document, and a favourite, play
count or playlist belongs to a song the library holds (`docs/FEATURES.md`). Both phones draw it
(`android.md`'s and `ios.md`'s *Services played in place*), each recognising a served row as id 0 with a served
scheme and keying its cover by the uri
(`a_server_is_browsed_from_its_artists_to_its_songs_and_each_has_its_cover`,
`served_songs_are_queued_named_and_opened_by_the_players_sources`,
`a_blank_account_serves_nothing_and_a_uri_of_another_kind_is_invalid`).

**A song played in place is a counted listen, and the service's own mark is its star.** `store_counting`
writes a listen of a served row (`Served::names`) with `track` NULL and `listens.served` its URI (the
catalog's step 21, `LISTENS_NAME_THE_SERVED_SONG_THEY_HEARD`), counted like a document's (the same
`Listening` thresholds) and settled on the same row as it grows. `statistics(days)`'s `plays` and
`listened_ms`, `listening_detail`'s days and `songs_heard` read `(d.missing = 0 OR l.served IS NOT NULL)`
over a `LEFT JOIN documents`, so a served listen is in the totals and the days; the top songs, albums
and artists name documents and leave it out, and the scrobbler and the sync ledger join `documents` and
never see it, as on the desktop (`library.md`'s *The count is the catalog's*)
(`a_song_played_in_place_is_a_counted_listen_in_the_totals_and_the_days_and_told_to_no_one`).
`ServedSong.favourite` is `Some` only where the service keeps a mark (`PlaysInPlace::keeps_favourites`;
a song the service did not say is starred reads `Some(false)`) and `ServedService.favourites` says the
same of the service. `favour_served_song(uri, favoured)` needs online, a served song uri and the
service, writes the mark through `PlaysInPlace::favour` (a refusal is `failure_of`'s sentence; a service
with none is `ProviderUnreadable`), and only then records it in `Served::marked`, which every later
browse and the playing row's `Track.favourite` read over what the service said, until the accounts
change. Nothing is rated into a file or synced (`providers.md`'s *A song played in place is a
favourite in the service's own mark*)
(`a_song_is_starred_and_unstarred_by_the_services_own_call_and_drawn_so_until_the_accounts_move`,
`a_service_with_an_account_marks_its_songs_and_one_without_draws_no_star`,
`a_star_is_refused_for_a_uri_that_is_no_song_and_while_online_is_off`).

## Reaching a private server

`SyncConfig.stun` (a `host` or `host:port`, blank asks none) and `SyncConfig.certificate` (the name of the
certificate kept, blank trusting none) are the desktop's `sync-stun` and `sync-server-certificate`
(`sync.md`). The certificate's PEM is the boundary's: `sync_keep_certificate(pem)` stages it beside the
ledger, requires `resonate_sync::check_trusted_certificate` to find at least one certificate every root
store accepts and only then renames it over `sync-server-certificate.pem`, so a refused file
(`Error::SyncCertificate`) leaves the one kept as it was; `sync_forget_certificate` removes it. `sync_start`,
called with sync on or off, records the pair as `Syncing::reaching` (`Reaching::of`: the kept file's path when
`certificate` is named, the trimmed STUN or none), which `Sync` and shared playlists receive as `trusted` and
`stun` and a party as `trusted`, so a `resonate-server` behind a private authority pairs a phone and a phone on
a `ws` or `wss` server offers its public address. A certificate named but gone fails the start with the sync
crate's `TrustedCertificate`
(`a_certificate_kept_is_trusted_by_sync_and_by_parties_and_one_forgotten_is_not`,
`a_file_that_is_no_certificate_is_refused_and_leaves_the_kept_one_alone`,
`a_certificate_named_but_gone_stops_sync_from_starting`). Both phones keep the name and the STUN in their
sync preferences and choose the PEM as a document (`android.md`, `ios.md`).

## Hosted services on the phones

**`configure_hosted_services(enabled)` is the desktop's `hosted-services` on a phone** (default on, kept
by each phone beside its online switch and applied at start): `Downloads::find` drops a Monochrome or
hifi-api account whose server is blank while it is off, so a named server is still asked and nothing
else is, and an empty remainder is `NoProvider`
(`declined_hosted_services_leave_a_hosted_account_unasked_and_a_named_server_asked`). The switch is
*Ask the hosted services* on each phone's provider screen.

## Shared playlists and parties

`together.rs` is the surface, and `sync/shared.rs` the catalog's half of the seam. Step 28, `playlist_shares`
keyed by `playlist` and `created`, leaves a flagged list out of `Catalog::playlists`; a list written for a
friend is flagged in the transaction that writes it, a followed one refuses `edit_playlist`
(`Error::PlaylistFollowed`), and `shared.sqlite` lives beside the ledger, started and stopped by `sync_start`
and `sync_stop`, so sync must be on. `together_named` keeps the typed name, empty until asked. `together_server(server)` keeps the server the sync settings name whether sync runs or not (each phone tells it whenever it applies those settings), so a party reaches the listener's own server with sync off: an invitation's server, else the running sync's, else this, else the hosted one (`a_party_reaches_the_server_chosen_even_while_sync_has_never_run`). `playlists()` marks a followed one (`MusicPlaylist.followed`, the flag `edit_playlist` refuses by), so a front end hides its edit controls with sync off too (`a_followed_list_is_told_from_the_others_with_sync_off`). A party
follows the zone shown when it began, and `save_zone` leaves that zone's kept queue alone while following
(`shared-playlists.md`, `party.md`).

## Made for you

`mixes.rs` offers the desktop's suggested lists over the document catalog, the code of `library.md`'s
*A suggestion is a named saved query* and not a copy of it. `suggested_lists(time_zone, utc_offset_seconds)`
reads the search index (`Index::artists_heard`, `genres_of_artists`, `genres_played`, `years_held`,
`genres_held`, `genres_of_years`), the artists heard in the last ninety days from `listens` (counted
listens, by heard time), hands them to `resonate_library::lists_to_offer` through a `Filing`, weighs every candidate query with
`Searching::measured` (the grammar the phones already run: `plays:>=3@1mo`, `artist:="X" plays:<=1`,
`or` over kindred artists) and keeps what `worth_offering` keeps. The day is the phone's zone's
(`the_listeners_calendar`), so the turn of *Discover*, *More like*, *deep cuts* and *Back to* moves at the
listener's midnight. The answer is kept under the index it was read from and the day (`KeptOffer`), so
a catalog that has not moved and a day that has not turned cost one comparison; any write that moves
the index, a counted listen included, offers again. Lists come in the desktop's shelf order
(`SuggestionKind::ALL`, `SuggestedList.shelf` and `shelf_name`), each with its `SuggestionReason` (a
front end draws its mark), `detail` (`Reason::says`), `accents`, song count, length, up to four covers
and the `SavedSearch` it is, so opening one is `saved_search_page(search, offset, limit)` and its header
`measure_saved_search(search)`; nothing is stored, and a list the catalog stops offering still opens as
the query it was. The phones offer every shelf the desktop does (*Made for you*, *Rediscover*, *From
your listening*, *Eras*, *Genres*, *Artists*, *Sound*). Where the phones' reading is poorer than the
desktop's: the artists are the index's groups (an album artist's songs count for the artist, as
`artist:` reads them), not the desktop's `artists` rows; a year is the song's own `date` (its first
four digits), not the album's release year; and the ranked genres and the genres each artist is filed
under are the songs' own `genre` tags alone, since the phones keep no enriched `artist_genres`, so a
genre only an artist's profile names is neither offered nor a kinship. **A list opened is kept as a
playlist that fills itself** by `keep_search_as_playlist(name, search)`: it answers the playlist
already filling itself from that same search (`playlist_filling_from`, so keeping twice makes one),
else makes the saved search under the first free `Name`, `Name (2)`, ... (`KeptSearch`: its id and the
name taken). Held by `the_eras_genres_artists_and_sounds_of_the_catalog_are_offered_as_the_desktop_offers_them`,
`a_list_kept_fills_itself_under_a_free_name_and_is_kept_once`,
`what_the_phone_heard_decides_the_lists_it_is_offered`, `a_list_holds_as_many_songs_when_it_is_opened_as_it_said`,
`what_is_offered_to_discover_turns_a_day_at_a_time_and_reads_the_same_all_day` and the rest of `mixes.rs`'s tests.

## Resonate Wrapped

`MobilePlayer::wrapped(year, time_zone, utc_offset_seconds)` answers a `WrappedYear` (the year, every year heard, `WrappedSlide`s, the summary) or nothing where no listen was ever kept; `resonate-mobile`'s `wrapped.rs` fills `resonate_core::wrapped::YearReview` from the document catalog (`library.md`), and the screens draw the slides as they come. The catalog's twenty-seventh step indexes `listens(began)`, since a year is read by time alone. Its thirty-second step, `A_YEAR_OVER_IS_KEPT_AS_IT_WAS_HEARD`, made `wrapped_years` (`library.md`'s *A year that is over is frozen*): `Catalog::wrapped` answers a kept year's review, keeps a finished year that holds something the first time it is shown, and lists kept years among those heard, the same rules as the desktop's (`a_year_over_is_frozen_when_first_shown_and_a_later_listen_or_deleted_song_leaves_it_as_it_was`, `a_year_still_running_is_read_afresh_and_a_year_with_nothing_in_it_is_never_kept`, `a_year_kept_by_a_later_build_is_shown_afresh_and_left_and_a_damaged_one_is_kept_again`, `a_catalog_of_before_years_were_kept_keeps_its_listens_and_gains_an_empty_table`). Neither phone changes: a front end still asks for a year and draws the slides.

A share lookup propagates service failures to the caller instead of returning an empty share after logging them.

A failed enrichment band measurement in an opted-in recognition pass is marked in `unstudied` under `JUDGED_UNDER`, as a failed whole study is. The bands query excludes that mark until refresh, a new judgement, or a changed file clears it. An out-of-reach document and cancellation leave no mark, so a drive or File Provider returning still gets its bands. This prevents a damaged document being fully decoded on every enrichment pass.
