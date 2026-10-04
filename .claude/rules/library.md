---
paths:
  - "crates/resonate-library/**/*.rs"
  - "crates/resonate-mpris/**/*.rs"
  - "crates/resonate-ui/src/views/playlists.rs"
  - "crates/resonate/src/playlists.rs"
---

# The catalog and its playlists

`resonate-library` is the local source's catalog and nothing else's. It walks directories, so it
stores paths and hands them back as local `MediaLocation`s; nothing in the schema is keyed by source.
A non-filesystem source brings its own catalog, and a queue row from one is read through
`Player::media` like any unscanned row.

## Schema and grouping

- **The schema is `V1` then `MIGRATIONS`, and a catalog is carried forward wherever it can be.** A
  change to what the catalog holds is a new SQL step appended to `MIGRATIONS` — an `ALTER TABLE`, a
  new table or index, a rewrite of the rows the change moves — never an edit to `V1` or a written
  step, either of which strands every catalog stamped before it. The stamp in `PRAGMA user_version`
  is `fingerprint_after`: an FNV-1a over the `V1` text then each step's in turn, taken at compile
  time, so every point in the history has its own stamp and `SCHEMA_FINGERPRINT` is the last.
  `lay_out` writes `V1` and every step where the stamp is `UNSTAMPED`, opens where it is this build's,
  and otherwise finds which prefix of the steps the stamp names and applies the rest in one
  transaction that restamps as it commits — a failing step is `StoreOp::Migrate` and leaves the
  catalog, stamp and all, as it was. The stamp is read *inside* that transaction, begun `IMMEDIATE`,
  so a second process opening an old catalog waits on the first's write lock and then reads the stamp
  it left, rather than reading the old one first and failing on a duplicate column
  (`a_catalog_another_process_is_migrating_is_read_once_it_has_and_not_migrated_twice`). Only a stamp no prefix names is `Error::SchemaMismatch`: a
  catalog from before the history began, or from a build with a history this one does not share —
  the one case left where the catalog is deleted and scanned again. A counted version was weighed and
  refused: it cannot tell a build with the same count and a different schema from ours, so the stamp
  stays a hash. `the_first_schema_is_never_edited_where_it_stands` pins `V1`'s fingerprint, so an edit
  in place fails a test saying to write a step, and `never_unstamped` keeps a schema hashing to zero
  from reading as unstamped. **Migrate wherever the rows can be carried**; break only where they
  cannot — an index whose meaning changed in a way no SQL can recompute — and then say so by the
  step's absence, not by editing `V1`. The history begins at `0ca1e683` (`V1_AS_FIRST_STAMPED`), the
  `V1` standing when the policy changed, so a catalog from any build since opens and is carried
  forward.
- **A change to what a probe reads is carried forward by marking the rows it moves.**
  `tracks.probe_again` is set by a migration step over the rows whose billing the new probe would
  change — the first such step marks every WavPack (`codec = 11`), a hybrid one having been billed
  lossless until the flag was read — and an incremental scan weighs a marked row as changed though
  its size and mtime have not moved, the upsert clearing the mark. Nothing else about the row moves:
  the vault link and what a lookup wrote are kept on the unchanged size and mtime, so a marked row is
  re-read, not re-imported or asked about again
  (`a_row_marked_to_be_probed_again_is_read_again_though_its_file_has_not_moved`). A later probe change
  is another step setting the mark on the rows it concerns, never a rescan of the whole catalog.
- **What the transport was doing is three tables, the rows, their order and the place each moving at
  a rate of their own.** `resume` is a singleton row — the row the queue was on, the frame into it,
  whether it was shuffled, when it was taken, and `next_first`/`next_last`, the span of the queued
  rows in the drawn list. `resume_rows` is the queue, one row per position in the order it was
  *loaded*; `resume_order` one row per position in the order it was *playing*, naming the loaded row
  there. `Keeping` decides which to write, and the three let each be written alone: an unchanged queue
  costs one `UPDATE`, a queue reordered or reshuffled costs `resume_order` and no URI, and only rows
  arriving or leaving rewrite `resume_rows`. `keep_place` names neither `shuffle` nor a row, so a
  place written seconds into a track cannot lose the order the rows were kept under; `keep_order`
  writes order, place and shuffle together, a toggle moving all three. The rows carry
  `MediaLocation::to_uri` rather than a path — the one place the catalog stores something not the
  local source's, a queue row coming from any registered source, while `Library::track_played` keys
  on a path precisely because it means a *scanned* row. `span_start` and `span_frames` sit beside it
  under `tracks`' `store::span` convention, so a cue row resumes as its cut. The order is what lets a
  shuffled queue come back playing as it played and unshuffle into its album, and
  `resonate-core::plays_in` is the whole of how it is trusted: an order naming each row exactly once
  is taken as it stands, and anything else — wrong length, a repeat, a row the queue lacks — gives
  back the load order, so no reading of it refuses a queue. A URI no longer naming a location drops
  that row alone: `resumed::Kept` holds each read row as an `Option<Resumable>` and renumbers over what
  is left — the order walked in play order with the dropped rows taken out, the kept position moved to
  where the same track now stands and the queued-next span closed over the gap — so the place still
  names the track it named; where the playing row itself is the one dropped, the queue resumes at the
  start of the row after it, `at` put back to nothing, and only a queue none of whose rows open is
  `None`. A queue whose every row opens comes back exactly as kept, order and all
  (`a_kept_queue_row_that_will_not_open_is_dropped_and_the_rest_resumed`). `keep_resumption` replaces
  rows, order and place in one transaction, and `resumption` reads the three tables inside one read
  transaction (`unchecked_transaction` on the reader), so another process's keep landing between two
  of the reads cannot hand back one run's rows under another's order or place; a queue is never half of
  one run and half of another either way. Writing no rows discards the queue — `resumption` answers
  `None` for an empty one, as before anything has played.
- **A favourite is when, not whether.** `tracks.favourite`, `albums.favourite` and
  `artists.favourite` are nullable nanosecond stamps, so the column saying a row is a favourite also
  orders the favourites by when they were marked; a boolean would buy nothing and cost a second
  column to sort on. `Library::favour` takes one `Favoured` — `Track`, `Album` or `Artist` — rather
  than three functions, the writes differing only in the table. It answers whether the value moved:
  the write is guarded on the column standing the other way, so favouring a favourite keeps its stamp
  — and its place in *recently favourited* — writes nothing and answers false, as does an id no row
  holds. `is:favourite` is a `Shape` beside `is:hires`; `SortOrder::Favourited` needs
  `tracks_by_favourite` declared `favourite DESC, title COLLATE NOCASE`, exactly as its `ORDER BY`
  reads, while `AlbumOrder::Favourited` and `ArtistOrder::Favourited` need nothing, those orders having
  no indexes by design.
- **A genre is the track's and its artist's at once, folded into one column.** `tracks.genre` is what
  the scan always read into `TagSet::genre` and dropped; the fourth `tracks_fts` column is that,
  folded through `folded_letters` and joined with the names `artist_genres` holds for the track's
  artist, so `genre:` is an ordinary scoped word against the index and reaches a file whose tagger
  named no genre. The cost is that enrichment re-indexes: `land_artist` writes `artist_genres` long
  after the scan wrote the row, so `store::reindex_the_tracks_of` runs beside it, and `NAMED_TABLES`
  learned `artist_genres` or a landed genre would leave the spelling vocabulary stale.
- **A pinned playlist leads every order, and `undo.rs` is where that is easy to lose.**
  `playlists.pinned` is the same nullable stamp a favourite is, and `order_by` prefixes
  `CASE WHEN p.pinned IS NULL THEN 1 ELSE 0 END, p.pinned DESC` onto every `PlaylistOrder`, one rule
  rather than six. It is threaded through `COLUMNS` and `read`, then `undo.rs`'s `Held`, `held_in` and
  `rewritten` — an undo re-creates the whole row from what was held, so a column added to `playlists`
  and missed there is silently dropped the first time an edit is walked back. A pin is no more an edit
  than a play, so `rewritten` reads it off the live row through `unedited_in` beside `played` and
  `plays`, and only a playlist the walk re-creates from nothing takes the one `Held` kept — a pin made
  or taken off after an edit survives walking the edit back. `undoing_an_edit_keeps_a_playlist_pinned`
  and `a_pin_made_after_an_edit_survives_walking_the_edit_back` are the guards. `resonate playlist
  --pin` and `--unpin` make and take off a pin.
- **What was listened to is three reads over `listens` and `passes`.** `listens.heard` is the
  nanoseconds of that visit actually listened to and `listens_by_time` what every window reads off;
  `passes` — the ninth `MIGRATIONS` step, a stamp and a heard time, indexed by `passes_by_time` — is
  the time spent on visits that never earned a play (`Library::passed`). `statistics` adds it to what
  was listened to and `listening_by_day` to each day's listening, while the plays, the tracks heard
  and the three most-heard lists read `listens` alone, a pass being time and not a play.
  `Library::statistics`, `most_listened` and `listening_by_day` are bounded by `listens.at >= ?` so
  the index serves each, and `most_listened` answers the three lists from one `read`, a pane costing
  one connection, not three. **A day is the listener's, not Greenwich's.** `listening_by_day` groups
  the stamps by the quarter hour in SQL and folds each quarter into the day `resonate_core::Calendar`
  says it fell on — `Calendar::local`, the zone `TZ` or `/etc/localtime` names, read through `tz-rs` on
  every call so a zone changed under a running window is followed, and UTC where none can be read. A
  quarter hour because every offset in use and every transition between two is whole quarters, so no
  bucket straddles a local midnight or a clock change, and the rows the read answers are bounded by the
  quarters anything was heard in rather than by the listens. Each `Day::at` is the calendar's own
  midnight, so a day the clocks change on is 23 or 25 hours long, and one whose midnight the clocks
  skip begins at its first hour (`Calendar::midnight_of`); *today* is the calendar's day of now, and a
  listen at 23:30 UTC east of Greenwich lands on the day the listener was living
  (`a_listen_late_in_a_utc_day_is_counted_on_the_day_the_listener_was_living`). A day nothing was played
  on is written as a zero row, so nothing downstream draws around a gap. Nothing is stored that the
  history does not already say. `most_listened` may sort on a `count(*)` because it is its own read,
  not a `SortOrder` — why the Statistics pane can answer what was heard most this month and the tracks
  pane still cannot.
- **The history is kept as long as the listener says, never past what a service was told.**
  `HistoryKept` is `Forever` (default) or a span of days, and `Library::age_the_history` deletes the
  `listens`, `unheld_listens`, `passes` and `playlist_plays` older than it (`forgotten_before`); a
  listen is forgotten only where its id is at or behind the lowest mark `submissions` holds, so a
  history aged while ListenBrainz was unreachable is still told in full when it can be. It takes the
  history alone: `tracks.plays` and `tracks.played` are counters of their own and stay, so a track's
  count and the *most played* order are as they were, and only the Statistics pane's reads — every one
  over `listens` — reach back no further than the span. The binary ages the catalog in
  `open_library_with`, so every command opening it does, and the settings pane's *Listening history*
  ages it once a span is chosen: a span shorter than the one in force is armed by the first press
  (`RootView::aging_the_history`, lowered by `disarm` with the pane's other armed presses) and kept
  by the second, as every other setting forgetting something for good is, while a longer one or
  *Forever* forgets nothing and is kept at once
  (`a_shorter_history_is_armed_by_the_first_press_and_kept_by_the_second`).
  `a_history_kept_for_a_span_forgets_what_is_older_once_every_service_was_told` is the claim.
- **What a service has been told is a mark in the history, and the history is what is told.**
  `submissions` — the sixth `MIGRATIONS` step — holds one row per `ListeningService`, the id of the
  last `listens` row that service was told of, and `scrobble.rs` is the pass: `Library::submit_listens`
  reads the listens past the mark in id order, `SUBMITTED_AT_ONCE` (100) at a time, each at
  `listens.began` — the moment it counted less what `track_played` was told had been heard of the
  visit, a `MIGRATIONS` step the unheld plays carry too — joined to track, album and artist for the
  names and MusicBrainz ids a `Scrobble` carries, hands them to the `Scrobbler` (the seam the library
  owns, as it owns `Reference`; `resonate-online`'s `ListenBrainz` fills it) and moves the mark past
  the batch in a `max` so it never goes back. `listens.id` is `AUTOINCREMENT` — a later step rebuilt
  the table, seeding the sequence at the greater of its newest id and every mark — so an id the history
  aged away is never handed out again below a mark
  (`a_play_counted_after_the_newest_listens_were_forgotten_is_still_told`). A listen of a row naming no title or artist is a
  `Submitted::unnamed` and passed over, a service filing nothing under a blank name. A batch refused as
  malformed — `Refused` with a 400 — is told again a listen at a time, so one bad row costs itself,
  and a listen refused alone is `Submitted::refused` and passed over; any other failure moves the mark
  only past what was told and answers the error, the rest told next ask. A service with no row yet is
  marked at the last listen held and told nothing — `Submitted::started` — which keeps a token given
  today from sending ten years of history. Nothing is written when a play is counted, so any process
  may have counted it, offline or before a restart, and whichever run submits next tells it; a listen
  whose track leaves the catalog before it is told leaves with it on the cascade. The claims are
  `a_service_is_told_what_was_heard_after_it_was_first_asked_and_each_play_once`,
  `a_play_the_service_refuses_as_malformed_is_passed_over_and_the_rest_are_told`,
  `a_play_a_service_could_not_be_reached_for_is_told_the_next_time` and
  `a_listen_is_told_as_when_it_began_rather_than_when_it_counted`. `Library::billed_as` is the same
  names for one row, what a `playing_now` is told
  (`what_is_playing_is_billed_as_the_catalog_names_it_and_a_file_it_does_not_hold_is_not`). Two runs
  submitting at once may tell a batch twice; ListenBrainz keeps one listen per moment and name, so
  nothing guards against it. The binary's `submitting.rs` is the thread that asks (`online.md`).
- **A suggestion is a saved query with a name on it, which is why it costs almost nothing.**
  `suggest.rs` answers `Suggestion { name, reason, query, rows, length, pictured_by }`, the query
  written through `Display for Search` rather than as a literal, so a suggestion and what the search
  box would parse cannot drift (`every_suggestion_reads_back_through_the_grammar_it_was_written_in`). A genre or artist with no letter or digit in its name — `!!!`, `?` — is not offered, its phrase
  reading as no condition and the playlist as the whole library (`fits_in_a_phrase`,
  `a_name_with_no_letter_or_digit_is_no_phrase_a_search_can_hold`).
  Saving one is `Library::save_query` and no new code, a saved-query playlist already filling itself.
  Nothing is persisted until saved, and nothing is offered whose count does not clear
  `ENOUGH_TO_OFFER`, so a thin catalog offers few rather than a screen of empty ones. Two rules came
  from real data: `names_a_decade` drops a genre that is only a decade, MusicBrainz handing out `2010s`
  as one, which stood beside the decade built from `albums.year` saying the same worse; and
  `billed_as` capitalises a lower-cased genre after any mark, not only a space, or `contemporary r&b`
  is billed *Contemporary R&b*. `Library::suggestions` answers an `Arc<[Suggestion]>` kept beside the
  search vocabulary under a counter of its own: the same `update_hook` steps `written` for any row
  written in `tracks`, `albums`, `artists` or `artist_genres` — every table a suggestion reads — so a
  reload that moved none of them (a settled search, a scroll, a playlist edited) gets the kept answer
  rather than a `measured` pass per candidate. Per table rather than per name because a suggestion
  counts plays and favourites as well as names: a counted play is a `tracks` write and drops it where
  the vocabulary stands, and a catalog written by another process drops both through the data
  version, whatever table it wrote. `length` is the same `measured` read's total — an aggregate over
  the matching rows with no `ORDER BY` wherever the query has no limit or offset, the order mattering
  only to which rows a bounded query counts, so a candidate is counted without sorting everything it
  matches — and `pictured_by` is
  `db::pictured_by`: the albums holding a picture that the search's rows fall on, most rows first, up
  to `PICTURED_BY_AT_MOST` (4), passing over an album whose picture — its vault key, or its bytes'
  length and first 256 bytes — another already stood for, then one whose picture merely *looks like*
  one standing, so one sleeve at two resolutions is not two tiles. `store::the_picture_of!` is that
  identity, written once for the query and the sweep. What a picture looks like is
  `resonate_codec::Likeness`: the cover averaged in linear light onto eight-by-eight cells kept as sRGB
  bytes; two are alike within a root mean square of `ALIKE_WITHIN_A_ROOT_MEAN_SQUARE_OF` (12 of 255),
  where one sleeve at a quarter its size measures ~1 and re-encoded as JPEG ~3, and a mirrored sleeve
  or one with a banner across its top ~80. A likeness costs a decode, so `likeness::of` keeps it in
  `likenesses`, the fifth `MIGRATIONS` step, under the picture's identity — an unreadable picture kept
  as `NULL` so it is not decoded again, a cover that failed to reach the reader kept as nothing and
  tried again — and `ORPHANS` removes every row no album's picture names. The table is not one the
  `update_hook` counts, so weighing a cover never drops the suggestions it is weighed for.
  `Reason::kind` sorts a suggestion under a `SuggestionKind`, how the pane shelves them.
  `a_suggestion_says_how_long_it_runs_and_which_covers_picture_it` and
  `one_sleeve_saved_at_two_resolutions_pictures_a_suggestion_once` are the claims.
- **A share is the link alone, three callers wanting the same one.** `Library::shareable` reads the
  track and its `release_track_links` and `album_links` rows, and `Shared::written` is a pure function
  over them — one URL or nothing — in `resonate-library` so the window, `resonate share` and anything
  else say the same thing. **It is song.link's own short page wherever the service has one**:
  `ShortForm::of` reads the service's id from its URL and writes `https://song.link/<letter>/<id>` for
  a song and `https://album.link/<letter>/<id>` for an album — `s` Spotify, `d` Deezer, `t` Tidal, `i`
  Apple Music (whose song is the `i=` of an album URL), `y` YouTube and YouTube Music — the address
  song.link itself redirects the long form to, short and saying nothing of where it was found. A
  service with no short page — Amazon, SoundCloud, Bandcamp, Qobuz — or a URL naming no readable id is
  `https://song.link/` with the service URL percent-encoded as one path segment; appended raw, the
  server collapses the unescaped `//` and answers 308 to `https:/…`, and a `?` is read as song.link's
  own query, so the track id never arrives. The candidates are the `Relation`s
  `RELATIONS_SONG_LINK_TAKES` names crossed with `SERVICES_SONG_LINK_RESOLVES`, a recording's own
  links ahead of its release's and the providers weighed in declared order, so one track shares the
  same way twice running. **Where the catalog holds nothing song.link opens, the reference is asked
  where the track streams**: `Shared::streamed_where_asked` hands `Reference::streamed_at` a
  `StreamAsked` — title, artist, ISRC and length, which `Shared` carries — and puts what it answers
  first, written like any held link; a track already linked asks nothing, and a failing reference is a
  warning and the share goes on. The window's *Share* asks only while `online` is on and `resonate
  share` only where `online::reference` answers, and what is found is not stored, a share being a
  gesture made once. Failing all that it is the MusicBrainz recording, and failing that nothing to
  copy.
- **Every order a pane offers is read off an index, and what the planner knows is written after a
  scan.** `tracks_by_album` carries the trailing `title COLLATE NOCASE` the album order ends on, and
  `tracks_by_title`, `tracks_by_artist_name`, `tracks_by_added`, `tracks_by_duration`,
  `tracks_by_plays`, `tracks_by_played` and `tracks_by_favourite` serve the other `SortOrder`s
  (`SortOrder::HELD_BY_AN_INDEX` lists the nine, `Relevance` needing none), each declared as its
  `ORDER BY` reads — collation included, no ordinary index serving a `COLLATE NOCASE` order unless
  declared so, and `DESC` included, SQLite walking an index backwards only where the whole order runs
  one way and `plays DESC, title` does not. **A reversed order needs no index of its own**, SQLite
  scanning one backwards: `order_by` is a `Reading` of the natural spelling and its mirror — every term
  flipped, `plays DESC, title` becoming `plays, title DESC` — and
  `db::tests::every_order_the_panes_offer_is_read_off_an_index_either_way_round` is the claim: it plans
  `SortOrder::ALL` crossed with `Direction::ALL` through `db::listing` and refuses a plan holding a
  temporary B-tree, what a full sort before the `LIMIT` reads as. The cost is eight order indexes on
  `tracks` written per stored row where there were three. `resonate playlist --reverse` turns `--sort`
  round as well as `--order`, and without either the grammar refuses it (the `ordered` group) rather
  than taking a flag that would do nothing. **Every order ends on the row's own id** — `tracks.id`,
  and `a.id` and `r.id` in the albums and artists orders below — so rows tied on every column still
  have one place, and paging with `LIMIT` and `OFFSET` neither repeats nor drops one among them. The
  id runs as an index's rowid tail runs, rising where the order walks its index forward (`plays DESC,
  title, tracks.id`) and falling where it walks it back, so the tie costs no sort; the
  `track_ids_rising!` and `track_ids_falling!` macros spell it, and
  `every_order_ends_on_the_rows_own_id_so_a_page_never_splits_a_tie` is the claim beside the index
  guard. A playlist listing already ended on `p.id`.
- **The albums and artists panes have orders too, deliberately with no index behind them.**
  `AlbumOrder` is relevance, title, artist, year, track count, when a track of it was last added and
  favourited; `ArtistOrder` is relevance, name, album count, track count and favourited;
  `album_order_by` and `artist_order_by` are `order_by`'s counterparts. Neither is held to the index
  guard: those tables hold thousands of rows where `tracks` holds hundreds of thousands, so a temp
  B-tree over one is cheaper than an index — and `SCHEMA_FINGERPRINT` covers the index list, so adding
  one is a `MIGRATIONS` step and a rebuild in every catalog. There is no `resonate albums` or `resonate
  artists`, so neither enum has a CLI argument: a variant nothing constructs is one to leave out.
- **A saved query's direction is a column of its own.** `playlist_queries.sort` holds the order alone,
  through `store::sort_code` and `sort_of`, and `playlist_queries.reading` the direction, through the
  `direction_code` and `direction_of` a playlist's kept order already used — `OrderedColumn::Reading`
  naming it in a refusal. It once rode in the sort column as the order plus a `READ_BACKWARDS` of
  sixteen, which an older build read as whatever order the sum landed on rather than refusing; the
  `MIGRATIONS` step adding the column carries every such code out of the sum
  (`a_saved_querys_direction_is_carried_out_of_its_sort_code_into_a_column_of_its_own`).
  `schema::restate_the_statistics` is the other half — `PRAGMA analysis_limit` and `PRAGMA optimize`
  on the writer once a scan has pruned — since with no `sqlite_stat1` the planner picks join order from
  hardcoded guesses; and `configure` hands a connection a page cache and a 256 MiB memory map
  (`MEMORY_MAPPED_BYTES`), where the 2 MB default had each pooled reader re-reading pages it had just
  read off the disk. **The cache is sized by what the connection is for**, a page cache being per
  connection and `READER_POOL` eight: `schema::Role::Writing` takes `WRITER_PAGE_CACHE_KIB` (8 MiB), the
  one batching inserts and maintaining indexes, and `Role::Reading` `READER_PAGE_CACHE_KIB` (2 MiB), so
  a library with every reader checked out bounds its page cache at 24 MiB, not the 72 MiB one size for
  all allowed. A bound, not a measured saving — SQLite fills a page cache lazily, so a small catalog
  never reached either figure — and the writer has the working set worth keeping.
- **A reader is checked out and handed back, `READER_POOL` being how many there may be, not how many
  are kept.** `Inner::checkout` takes a free parked connection, opens one where the pool is under the
  count, and otherwise waits on `Inner::freed` until a caller is done, so eight is how many SQLite
  connections a 500k-track scan can have open at once, not how many callers. `Reader` hands one back
  from its `Drop` rather than on the normal path alone, so a panicking query costs the pool nothing.
  The wait is safe because no reader is taken while another is held — every `Inner::read` closure
  queries and returns, and a caller needing two reads takes them one after the other — so a nested
  checkout can never be what the pool waits for.
- **Every write begins `IMMEDIATE`.** `Inner::write`, `reconcile_artists` and `settle_the_credits`
  take the write lock as the transaction opens, so a second process waits its `busy_timeout` for it.
  A deferred transaction reading before it wrote — every undo-wrapped playlist edit — held a snapshot
  another process could commit past, and its first write then failed at once with
  `SQLITE_BUSY_SNAPSHOT`, which no timeout waits out
  (`an_edit_that_reads_before_it_writes_is_not_torn_by_another_process_committing`). Within one
  process the writer's `Mutex` already serialised them, so the only cost is a write that ends up
  writing nothing holding the lock for its read.
- **A cue sheet claims the file it names, and the scan reads sheets before audio.** A `.cue` is not
  audio and not in `AUDIO_EXTENSIONS`; it is a sidecar, so `directory_of` reads every sheet in a
  directory first, resolves each `FILE` against the sheet's own folder, and only then sends a probe job
  for audio no sheet claimed — which stops one FLAC being stored as an album's worth of rows and as one
  whole-file row. A `FILE` cutting no audio track — a data track alone — claims nothing, so its file
  stays a whole-file row rather than being claimed and then pruned with its plays and favourite
  (`a_sheet_that_cuts_no_audio_track_leaves_its_file_to_the_whole_file_pass`). A `FILE` naming a folder below the sheet (`cue::folder_named`, `audio.md`) is
  matched against the audio listed there, and the claim is kept in the walk's `claimed_from_above`,
  which that folder's own pass — always later, the walk being depth first with a folder's children
  pushed before it is read — takes the file out of; a sheet in the lower folder naming a file one
  above already claims is passed over, so no file is cut twice. Otherwise a `FILE` is matched against
  the audio beside the sheet by its last component — split on `/` and `\\` alike, so a Windows path
  names the file in the sheet's own folder — through `cue::Naming`: the name exactly, else the name
  in any case, else the same stem with another extension (a rip converted after its sheet was written, `FILE "ALBUM.WAV"` beside
  `album.flac`), the closest reading winning and a tie at it naming nothing. `organise` resolves a
  sheet's claims through the same `scan::claimed_beside`, a stem naming only audio, and a file a
  sheet in a folder above cuts is `Refusal::NamedFromAbove` and stays where it is — moving it would
  leave the sheet naming nothing, and a sheet naming files in two folders is refused as
  `SharesASheet` for the same reason, the move filing its files into one folder; and
  `cue::renamed` rewrites whichever `FILE` line the moved file answered to. Incrementality weighs the two mtimes
  apart: `tracks.modified` is the audio file's and `tracks.sheet_modified` the sidecar's, NULL where no
  sidecar cut the row. A row is unchanged only where both agree with the walk, so editing a sheet
  rescans its rows, and taking the sheet away reprobes the file and prunes the rows the cut no longer
  names *whichever* of the two was later — when the later was one stored number, a sheet older than
  its audio left its rows standing when it went, the audio's mtime alone still matching.
  `a_sheet_older_than_the_file_it_cut_is_still_missed_once_it_has_gone` is that claim, needing an
  mtime set by hand, a sheet written after the file it names being the newer.
- **A sheet the file itself carries is the probe worker's to see, so the cut is decided there, not in
  the walk.** A file's chapters are such a sheet (`audio.md`), an audiobook scanning as a row a
  chapter. A sidecar shows in a directory listing and an embedded `CUESHEET` does not, so
  `read_candidate` probes then cuts on `MediaInfo::cue` where it names audio tracks, sharing
  `cut_into_rows` with `read_cut` so the arithmetic is written once. The sidecar still wins, by the
  walk: `claimed` takes the audio file out of the audio pass before `whole_file_job` sees it, so the
  embedded sheet is never read for a file a `.cue` beside it cuts. The cost is a count the walker
  cannot take: one file is one `discovered` until the probe says otherwise, and `rows_past_the_first`
  is what the worker adds once it knows — the number the incremental path adds when it sends one
  `Job::Known` per stored row, so `discovered` ends at the rows either way.
- **The walk reads what the catalog already holds once, and the roots bound the read.**
  `Known::under` takes `(path, id, file_size, modified, sheet_modified, probe_again)` for the rows
  under each root walked — one indexed range query per root, where a 500k-file tree used to interleave
  500k point queries with the writer's own commits. It is a snapshot taken before the walker starts,
  safe because no path is walked twice in one scan: a root inside a root is refused, a directory
  reached through a second link is stepped past, and a sheet claims its file before the audio pass sees
  it. `Known::rows` is the whole read-back — every stored row for a path, in `span_start` order — a path
  being a cut file as readily as a whole one: unchanged means at least one row and every one matching
  the size, the mtime and the sheet that cut it (and none marked `probe_again`), and
  `Candidate::existing` carries them all so a probe answering N rows can claim the N there. The cost is
  the paths under the walked roots held in memory for the walk.
- **One pass walks the tree at a time, and a second is refused rather than queued.**
  `Inner::walking` is the flag and `Walk` the guard holding it: every pass walking or rewriting the
  tree — `Library::scan`, `organise`, `retag`, `import`, `prune_the_vault`, `release_from_vault` and
  `delete_tracks` — takes one (the first two in `start`, before the thread is spawned), and so do the
  root edits a walk reads under — `add_root`, `remove_root`, `forget_the_gone` and `retire` — so
  `resonate forget` and the window's *Add folder* refuse beside a scan rather than landing under it
  (`a_root_is_neither_added_nor_dropped_under_a_pass_walking_the_tree`). `resonate scan <roots>`
  checks each is a folder and leaves the registering to the scan's own `roots`, inside the guard, so a
  scan refused for a walk already running registers nothing. The thread owns it for its
  life, so it is handed back from `Drop` on a panic as `Reader` hands back a pooled connection. Taking
  it never waits — a caller finding the tree walked gets `Error::AlreadyWalking` at once, a pass
  blocking for minutes being no pass a window or command line can start. What it protects is
  `Known::under`: the snapshot is taken before the walker starts, so an organise committing a path
  rewrite after it and before the walker reaches that directory left the walker probing the file as new
  and the prune taking the rewritten row — its `id`, `added`, counts and `listens` — away. Two scans at
  once are worse: each stamps its own `generation` and prunes `WHERE seen != ?`, so the first to finish
  deletes every row the second wrote. `enrich` and `poll` stay outside it, neither walking the tree nor
  rewriting a path. **The guard holds across processes.** `resonate scan` beside the window's scan,
  or `resonate vault --prune` beside its import, are two `Inner`s and two flags, so a catalog on disc
  also takes an exclusive `File::try_lock` on `<catalog>.walk` (`locked_across_processes`), held by
  the `Walk` and released as its `File` closes — on a panic or a killed process alike, the kernel
  letting an advisory lock go with its descriptor. A lock another process holds is the same
  `Error::AlreadyWalking`. The lock file stays where it is: removing it would race a process that has
  opened it and not yet locked it. An in-memory catalog has no file and no second process, so takes
  the flag alone (`a_walk_in_one_process_refuses_a_walk_in_another_on_the_same_catalog`).
- **An album grouped by its folder is re-keyed to the folder it moved into, in place on its row.** Only
  the third tier embeds a path, so only it can be left naming a vanished folder; one file of such an
  album re-probed later would be keyed onto the new folder, insert a second `albums` row and take its
  tracks, and once nothing pointed at the old row `ORPHANS` would sweep its cover, `mbid`,
  `release_group`, `release_tracks` and, through the cascades, `wants`. `organise::re_key_the_sleeves`
  runs in a transaction of its own once every batch has landed: it takes the albums the moved files
  name, keeps those `store::is_keyed_by_its_folder` answers for, and writes `store::sleeve_key` of the
  row's title and the folder its tracks now share. It changes no membership: `tracks.album_id`
  references `albums(id)` and nothing joins on a key, so rewriting the key on its row orphans nothing.
  The folder is `scan::sleeve` of each track read together — the scan's reading, disc folders and all —
  so an album whose tracks landed in several folders, or with any track directly in a root, keeps its
  key and says so in a debug record. An album `album_keys` names more than once is left alone for the
  same reason — an album gathered onto a release is named by the key of every folder it was gathered
  from, so its tracks do not share one folder either. `album_keys.key` is a primary key, so an album
  moving into a folder another album names keeps its old key too: merging two albums *by where they
  landed* is a decision nobody asked for, where merging two that prove one release is one the pass
  makes on evidence. **What the re-key leaves alone, a probe keeps.** A whole rescan, or any probe of a
  file whose size or mtime moved, computes the new folder's key afresh, and that key naming another
  album — or nothing — filed the track there and let `ORPHANS` take the album it left, release, cover
  and wants with it. `store::kept_where_it_was` answers first: where the row already belongs to an
  album of the same title, none of its other names finds an album, and its folder's key names another
  album or none, the row stays and `lend_the_free_names` gives the album whichever of its keys nobody
  holds. `a_whole_rescan_after_two_albums_land_in_one_folder_keeps_each_the_album_it_was` is the claim.
- **A row names what it is billed to, a listing beside it being narrowed and capped.** `ALBUM_COLUMNS`
  reads the artist's name by id — `(SELECT r.name FROM artists r WHERE r.id = a.artist_id)`, written
  through the `album_owner!` macro beside `album_title!` and `album_tracks!`, a correlated scalar beside
  the three counting an album's tracks, distinct track artists and what its release is missing — so
  `Album::artist` sits by the `artist_id` naming it, and `ArtistDetail::name` likewise for the artist
  pane. A subquery, not a `JOIN`, because `ALBUM_COLUMNS` is read by `Library::album` and
  `Library::albums` against `FROM albums a` plus a varying `scoped.from`, and a join would be written
  twice and could collide with the aliases `Matching::grouped` brings. It replaced the window resolving
  a name through `LibraryModel::artist_names`, a map built from the artists *listing* — which
  `ArtistQuery` narrows by the typed text and caps at `PAGE` — so the name went missing exactly where
  the search did its job: an album cell drew its year alone, the scoped heading's artist line vanished
  and the artist heading fell back to the literal `artist <id>`. `browsed` reads the scoped album by id
  as it reads the release, tracks and artist's detail, so `LibraryModel::album_of` answers for the
  scoped album whether or not the listing holds it; the map and its getter are gone, having lost every
  caller.
- **A scan says what went wrong with a file, not merely that something did.** `ScanStats::failed` is a
  `Failures` of four counts, and `Failure` decides which: `Unnamed` for a non-UTF-8 path, counted before
  anything opens; `Misnamed` for `UnrecognisedContainer` and `NoAudioTrack`, the bytes not being the
  container the extension promised; `Undecodable` for a container this build reads holding audio it
  cannot — `NoDecoder`, `DsdCompressed` and properties it cannot represent; `Unreadable` for all else,
  `Io` and the `Symphonia` residual being where a corrupt file lands. The match on
  `resonate_codec::Error` is exhaustive and the enum has no `#[non_exhaustive]`, so a variant added to
  the codec fails to compile here rather than falling into a catch-all's count. Worth telling apart now
  that `AUDIO_EXTENSIONS` advertises nothing undecodable: what reaches the tally is a file whose
  extension promised a container its bytes are not — a retagging or bad rip, not a declined format.
  Both presenters print the total and append only the non-zero counts, so a clean scan reads as always.
- **What is hidden is not music.** `scan::is_passed_over` takes every name beginning with a dot —
  `.Trash-1000`, Syncthing's `.stversions`, an AppleDouble `._` file, a tag write's staged copy —
  and the system folders `$RECYCLE.BIN`, `System Volume Information` and `lost+found` out of the walk
  and out of `audio_below`, so none is cataloged as a track or counted as a failure on every scan; a
  root named with a dot is still walked, the rule reading what is inside it
  (`hidden_trash_and_system_folders_and_dot_files_are_passed_over`).
- **A root may not be inside a root, and a wider one takes in what it covers.**
  `store::register_root` is the one way a root is written, so `Library::add_root` and the scan's own
  `roots` refuse and absorb alike: a path inside a registered root is `Error::RootInsideRoot`, and a
  path containing registered roots re-parents their tracks onto itself and drops those rows from
  `roots` — re-parented, not deleted, because `tracks.root_id` cascades and widening a root must not
  cost a play counted under it. Nesting was never only untidy: the overlap was walked twice with
  `root_id` flipping on the second upsert.
- **What the scan could not read, it keeps.** The prune takes every row the pass did not stamp, so a
  row is stamped wherever the pass could not tell a file gone from a file it failed to read. A file
  whose size or mtime moved and which then would not probe — a torn write, a transient `EIO` — is
  counted failed and its `Candidate::existing` rows stamped as `Outcome::Kept`, which touches the row
  without counting it processed a second time; so is a cue-cut file. A directory `read_dir` refuses —
  `EACCES`, `EIO`, an automount not answering — or whose listing an error cuts short (the entries are
  gathered whole before any is read, so a folder half listed is kept whole rather than half pruned),
  an entry whose kind cannot be read, and an entry whose `stat` fails, a link into an
  unplugged drive among them, stamp every row `Known::at_or_under` names at or below the path, a range
  over the `BTreeMap` the rows are held in. Before, each was stepped past and the prune deleted the rows,
  and their plays, listens and favourites with them
  (`a_changed_file_that_will_not_probe_keeps_its_row_and_what_was_heard_of_it`,
  `a_folder_the_scan_cannot_read_keeps_every_row_under_it`). An *empty* folder is guarded only where
  it was seen as a volume: a mount point with nothing mounted reads exactly as a folder whose files
  were moved out, and keeping every empty folder's rows kept `moves::follow_the_moved` from pairing
  any move out of a folder emptied by it.
- **A volume seen mounted is remembered, and its rows kept while it is not.** The walk carries each
  directory's parent's `st_dev` and notes a directory whose own differs — the root included, weighed
  against its parent — as a volume; a followed link into another filesystem is one too.
  `volumes::settle` writes them to `volumes` after the prune and drops a row for one no longer
  mounted that no track sits under. A noted volume whose device is its parent's — the empty mount
  point — or whose directory has gone stamps every row `Known::at_or_under` names as `Outcome::Kept`
  and is not walked, and `tidy_the_roots_beside` and `forget_the_gone` skip a path on one
  (`a_volume_not_mounted_keeps_every_row_on_it_whether_its_mount_point_is_empty_or_gone`,
  `a_folder_on_another_volume_is_noted_and_its_rows_kept_once_it_is_not_mounted`). Nothing moves
  out of an unmounted volume, so no pairing is lost. **A volume retired for good is the listener's to
  say.** `Library::retire` takes a folder at or inside a root — a volume's mount point or any folder —
  and forgets every row at or under it whose file is not there, an unmounted volume's included, then
  drops each `volumes` row at or under it that no longer holds a row (`volumes::retire_at_or_under`);
  `forget_the_gone` is the same walk (`forget_at_or_under`) with the unmounted volumes guarded. A file
  that *is* there keeps its row, the next scan finding it again anyway, so retiring a mounted drive
  forgets nothing. It takes the `Walk` guard, as `forget_the_gone` does
  (`a_volume_retired_for_good_forgets_its_rows_and_is_no_longer_remembered`,
  `a_folder_whose_files_are_still_there_is_not_retired`).
- **Every walk hazard but a lost worker is stepped past.** A directory past `MAX_DEPTH` is warned over
  and skipped rather than failing the scan and the prune with it. A symlink is
  weighed only where it names a directory, and one naming a directory this walk has been down is
  stepped past rather than read as a cycle, so two albums linked to one shared folder walk it once
  instead of aborting — the set is what a cycle runs into on its second pass through the link, so
  skipping still terminates. A link whose canonical target is inside a registered root, or holds one,
  is never followed (`Walking::reaches_a_root`, over every root `roots` holds, not only the walked):
  the set holds only link targets, so a link to a folder inside its own root was walked beside the
  folder itself and every file behind it stored twice, once under each path, and a link into another
  root filed that root's files under this one as well. What is inside a root is walked by its own path
  from its own root, and what holds a root would walk the root again from above
  (`a_link_to_a_folder_inside_the_same_root_is_not_walked_a_second_time`,
  `a_link_to_a_folder_holding_the_root_is_not_walked_back_into_it`,
  `a_link_into_another_root_leaves_its_files_to_that_root`). Not stepped past is a probe worker that panicked: `run` joins every one
  and answers `Error::Stopped` naming the scan's `PassKind` before the prune, a scan with short counts
  pruning rows it never reached. A failing commit drops the result channel's receiver before anything
  is joined, so probe workers blocked sending wake to a closed channel, their exit closes the job
  channel under the walker, and the scan answers the store's error and hands the `Walk` guard back
  rather than waiting on a pipeline nothing drains.
- **Every root is tidied by a scan and only the walked ones pruned, and a root not there is neither.**
  The prune is `WHERE seen != ?` over the roots the walk stamped, so `resonate scan <root>` used to
  leave every other root holding rows for vanished files. `tidy_the_roots_beside` is the other half: for
  each registered root this scan did not walk it reads the distinct paths under it through
  `store::paths_under`, keeps those no longer there and hands them to `store::forget_paths`, and
  `store::sweep_orphans` — the prune's batch, lifted out — runs once at the end where anything went. It
  costs one `stat` per path of a root nobody asked about — a tidy, not a walk: no file opened, no tag
  read, nothing new found. A bare `resonate scan` walks every root, so has nothing beside to tidy.
  `is_there` guards both: a registered root whose directory is missing is dropped from the bare scan's
  walk and stepped over by the tidy, an unmounted drive not being an empty one and pruning it taking
  every play counted under it. A root named on the command line is still `Error::RootNotADirectory`
  where missing, being something asked for. A scan the window's watch asks for is the other kind:
  `Library::scan_what_is_held` walks only the named roots `roots` still holds and that are there,
  registers nothing, and settles them before the thread starts — answering `None` where none is left —
  so a drive unplugged after its root was queued costs the roots queued beside it nothing, and the
  window takes a root off what it owes only once a scan has taken it.
- **A statement run once a row is prepared once a connection.** `store::cached` runs literal SQL
  through `prepare_cached`, and every per-row write goes through it — the scan's store writes
  (`touch`, the index row, the album's key, year and cover among them), the resumption's and
  queue order's rows and a playlist's inserts — so an incremental scan of 500 000 tracks parses its
  `UPDATE` once rather than 500 000 times. A scan cycles through more distinct statements a record
  than rusqlite's default cache of 16 holds, so `connect` raises it to `STATEMENTS_CACHED` (64),
  or the statements would evict each other and every one be parsed again. SQL built by `format!`
  stays on `execute`, each spelling being its own statement.
- **A track is keyed by `(path, span_start)`, not path.** N cue rows share one path, so the `UNIQUE` is
  on the pair and `span_start` is `NOT NULL DEFAULT 0`, not nullable — SQLite treats NULLs as distinct
  in a unique index, letting one file insert twice. Every lookup meaning *this row* takes the span
  beside the path: `Library::track_at` and `Library::track_played`, the latter because keying a play on
  the path alone counts every track of an album against one row. A playlist entry stores a path alone,
  so the three joins reaching `tracks` from `playlist_entries` take the row with the lowest
  `span_start` — without it the join multiplies an entry into one row per cue track and inflates a
  playlist's count and length.
- **A `Track` names its artist by id as well as name.** `tracks.artist_id` is the column the artists
  listing always counted against, and `Track::artist_id` reads it back beside `album_id`, so a row says
  who made it, not only what they are called — letting the window open an artist from the playing row
  without weighing a name against a search-narrowed listing. `TRACK_COLUMNS` is append-only (it was
  appended as the last name then): `BESIDE_A_TRACK` counts that list, so appending leaves every joined
  read's own columns where they were.
- **Each MusicBrainz id is weighed against the column holding one of its kind.** `tracks.mbid` is the
  *recording* id, what `TagSet::musicbrainz_track_id` carries — a tagger writes it in
  `MUSICBRAINZ_TRACKID` for a Vorbis comment and in the `UFID` frame MusicBrainz owns for ID3 — and
  `release_track_mbid` is `MUSICBRAINZ_RELEASETRACKID` beside it, naming the track's place on one
  release, not the recording. Different identifiers for different things, so `rematch_release_tracks`
  weighs each against its own — the release row's `recording_mbid` against the first and `track_mbid`
  against the second — where one column weighed against both could pair the second only by accident.
- **An artist is keyed by the fold of its name, so one spelling is one artist however it is
  spelled.** `resonate_core::folded_letters` (re-exported by `store` and the crate) lowercases,
  decomposes, drops a combining mark where it sits on a Latin, Greek or Cyrillic letter (`takes_accents`
  over `ACCENTED_SCRIPTS`, the scripts where a mark is an accent), keeps every other — a kana voicing
  mark, an Indic vowel sign, which change the word — and composes what it kept again, so ガラス and
  カラス, バンド and ハンド stay two artists and two searches
  (`a_kana_voicing_mark_keeps_two_names_apart_and_an_old_index_is_folded_again`); and it spells out the letters Unicode does not decompose — `ł`,
  `ø`, `đ`, `ð`, `þ`, `ß`, `æ`, `œ`, the dotless `ı`, `ħ`, `ŋ`, `ŧ`, `ĸ` and the rest — so *Marcin
  Przybyłowicz* and *Marcin Przybylowicz* are both `artists.key` `marcin przybylowicz`, where
  `name.to_lowercase()` made them two artists with two listings, two portraits and half a discography
  each. The same fold is what `tracks_fts` is written in and a typed word is folded through, because
  SQLite's tokenizer folds `İ` to `i` and leaves the dotless `ı` alone, so *Kıskanç* and *KISKANÇ* were
  two different searches (an index written before the fold needed the catalog scanned again). It is not
  `enriched::folded_title`, which keeps marks: that weighs a MusicBrainz title against a tag, where a
  mark is evidence, and this gathers spellings of one name, where a mark is noise.
  `enriched::stripped_title` is the two together — the letter fold put through the title fold — which
  the enrichment falls back to below. **The billed spelling is the one carrying the marks**, counted by
  `marks_in` and applied on the cache hit as on the row, so a scan meeting the stripped spelling later
  does not undo the accented one and the display name settles rather than following scan order.
  `resonate missing --artist` and the window's type-ahead weigh a typed name through the fold, so a
  name spelt either way reaches the artist filed under the marked spelling.
- **A catalog keyed before the fold is folded back when opened, not when next scanned.**
  `store::reconcile_artists` runs once in `Library::build`, after `schema::lay_out`: it reads every
  artist, groups them by `folded_letters` of the *name*, and leaves a group alone where its one row is
  already keyed by its own fold. Otherwise one row is kept — the one with an `mbid`, then the lowest id,
  the row enrichment, portrait, genres and links hang off — the rest hand over their tracks, albums,
  genres, links and kept releases through `store::take_over_artist`'s `UPDATE OR IGNORE` and are
  deleted, and the survivor is rekeyed and renamed to the group's most marked spelling. It needs no
  sentinel key to avoid colliding with an unreached row: every key is a fold or a `to_lowercase` of the
  same name, and folding is idempotent, so two rows whose keys could collide always fold into one
  group. An invariant the catalog keeps rather than a migration it ran — hence no schema step, and
  running it twice is a no-op. A row it rekeys has its tracks indexed again, the fold having moved
  under them. **A change to the fold is a schema step that asks for the index to be folded again**:
  the step creates `index_refold_wanted`, and `store::refold_the_index`, run in `Library::build` after
  the reconcile, writes again every `tracks_fts` row whose title, artist or album no longer reads as its
  fold, then drops the table. A release's `folded` haystack, a kept release's and a dismissal folded
  before the change are written again when the album is next landed.
- **An album is whatever a grouping key names, and several may name one.** `album_keys` is the table —
  a key its primary key, an album holding any number — making a grouping a *name* for an album rather
  than a property of it. `store::album` reads it rather than upserting on a column, so a scan computing
  a key an album holds fills that album and a key nothing holds makes a new one. `ORPHANS` is
  unchanged, an unreferenced album's keys going with it on the cascade.
- **One song held more than once is listed once, as the best copy.** `alternatives.rs` runs at the end
  of every scan: it groups rows by the fold of the album title, the album's owner or else the track's
  artist, the disc, the track number and the title — the album's *title* rather than its row, so two
  folders of one album meet — and within a group gathers rows whose lengths are within
  `THE_SAME_LENGTH_WITHIN` (2 s) of one another, or both lengthless. An album is named both ways a row
  can be billed — the release title the enrichment gave and the title its tags gave — and two rows
  sharing either are one song, so a copy MusicBrainz billed *Meddle* meets an untouched copy tagged
  *Meddle* though its own tags said *Meddle (Remastered)*; `one_song_apiece` joins the names through a
  union-find. A row with no title or artist names nothing, a loose *Intro* being no evidence of any
  other *Intro*. The best of a gathering is lossless over lossy, then the wider word, the higher rate,
  the higher bitrate, then the row held first; every other copy names it in `tracks.alternative_of` —
  *whatever* its format, so two identical rips in two folders are one row with a `+1` rather than a
  duplicate, and a copy in the best copy's own format is hidden beside one in another. The row's menu
  tells two copies of one kind apart by folder and file. The copies are kept whole — plays, playlists,
  files — and only the listings step past them: `scoped` filters every track listing and saved query on
  `+tracks.alternative_of IS NULL` (the unary plus keeping that term off `tracks_by_alternative` so the
  order indexes still lead), the album and artist counts count the best copy alone (`HOLDS_A_BEST_COPY`),
  and an album whose every track is another's alternative leaves the albums pane. `Track::alternatives`
  is how many copies a row stands for, drawn as `+N` beside the title, and `Library::alternatives_of`
  what the menu offers to play instead. A best copy that goes puts `ON DELETE SET NULL` on the rows
  under it, and the end-of-scan pass crowns the next.
- **A hidden track is kept and only stepped past.** `tracks.hidden` is the second `MIGRATIONS` step,
  and `Library::hide_track` sets it either way and answers whether it moved. The row, file, plays,
  playlists and favourite are kept, and a scan never touches the column, so a hidden track stays hidden
  however often its root is read. `scoped` adds `tracks.hidden = 0` beside the best-copy term, and the
  album and artist counts and `HOLDS_A_BEST_COPY` weigh it the same, so a hidden row leaves the
  listings, the counts and an album holding nothing else. `is:hidden` is a `Shape`, and a search
  *insisting* on it — a clause of that one alternative, not denied — lifts the visibility term, the one
  way a hidden row is listed again (`Clause::insists_on`); `-is:hidden` or an alternative beside it lifts
  nothing. `Track::hidden` is what the menu reads to offer *Hide from library* or *Show in library*.
- **A track names its album every way it can, and joins the album the first of those names finds.**
  `grouping_keys` answers a *run* of keys in precedence order. A MusicBrainz release id stands alone
  with nothing beside it, so two releases sharing a title and artist stay two. Otherwise an
  `ALBUMARTIST` the tagger wrote, on anything not flagged a compilation, is one name, the folder the
  track sits in — `TrackRecord::sleeve` — another, and `album_key(title, owner)` the fallback where a
  track has neither. A track joins the album the first of its names finds and lends it the rest, so both
  gatherings hold at once: an `ALBUMARTIST` gathers discs the folders keep apart, and a folder gathers an
  album its files bill to different owners — the Cyberpunk 2077 soundtrack, whose three album artists
  made three albums under one sleeve when the owner outranked the folder and the loser had nowhere to
  go. Every key carries the title, so nothing gathers two albums not called the same thing. Where two of
  a track's names find *different* albums the first wins and the second is left, not merged — the pass
  gathers on a release, and only on a release. Where the tracks under one album disagree about the album
  artist, the album keeps none (what `COMPILATION` already meant), and `Album::artist_count` lets a pane
  draw that as *Various artists* rather than a bare year.
- **A sleeve is a folder made to hold an album, which is why it is not simply the parent.**
  `scan::sleeve` answers `None` for a track directly in a root, a root of loose files being a dumping
  ground and two albums sharing a title there still two — the third tier then falls back to the track
  artist. **A record filed loose in a root is gathered back once the scan has written it.** A
  compilation with no `ALBUMARTIST` or `COMPILATION` flag, filed with the root as its folder, would
  otherwise stand as one album per track artist. `loose::gather_the_loose` runs after the prune on every
  walked root: it takes the albums whose tracks all sit in that root and whose keys are all the fallback
  tier — none a folder's or release's — groups them by lowercased title, and gathers a group through
  `enriched::gather` only where `one_record` says the numbering makes one: every track numbered, no disc
  and number taken twice, the years and declared `TRACKTOTAL`s agreeing where stated, and no more tracks
  than a declared total. Two *Greatest Hits* each numbered from one, or files with no numbers, stay
  apart — the reading the rule above protects. The loser's keys name the survivor, so a file read again
  joins it rather than splitting off. A folder named `CD2`, `Disc 3` or `disk-01` is read as one disc of
  a set and answers with its parent, so a set filed that way is one album without an `ALBUMARTIST`
  saying so.
- **The word a disc is filed under is read in ten spellings, the longest match taken.** `SPELLINGS`
  carries `cd`, `disc`, `disk`, `disque`, `disco`, `dysk`, `platte`, `schijf`, `skiva` and `диск`, so a
  set filed `Disque 2`, `Disco 3` or `Платте`-style in digits gathers as `CD2` always did. The match is
  by the *shortest remainder*, not the first hit, which the longer spellings require: `disc` prefixes
  `disco`, so a first-hit walk would strip `disc` from `disco 2`, find `o 2` with no separator before
  it and read no disc. `Discovery` and `Disconnected` stay albums, the longest spelling they match
  leaving no separator either.
- **A disc numbered in words is the same disc, and `ONES`, `TEENS` and `TENS` say so.**
  `disc_in_folder` reads the number on either side of the word it numbers: a cardinal after it — `Disc
  One`, `CD Two`, `disk_three` — and an ordinal before it — `Second Disc`, `First CD`. The tables
  compose rather than run on, so a tens word joined to a ones word is read as the number it spells —
  `Disc Twenty One` after, `Twenty-First Disc` before — up to ninety-nine, a set past that being filed
  in digits by anyone patient enough to file it. A word form wants a separator between the two words,
  which keeps `Discovery` and `Disconnected` albums where a bare prefix match would make `Discone` a
  disc, and makes `Twentyfirst Disc` nothing; the digit form does not, `CD1` being how half are written.
  `a_word_run_together_with_the_one_beside_it_names_no_disc` is that claim, and `scan::disc_in_folder`
  having two callers means `{disc}` in an organise layout reads such a set as `organise::disc_of`
  always read `CD1`. The cost was a key format change: a set scanned under `Second Disc` is named by a
  key naming that folder, so it stays two albums until the catalog is scanned again.
- **A number word in another language is composed as that language writes it, up to ninety-nine.**
  `numerals.rs` spells every number from one to ninety-nine in French, Spanish, Italian, German, Dutch
  and Portuguese — cardinals and ordinals, by each language's rules: `vingt-et-un` and
  `quatre-vingt-onze`, `treinta y uno` and `veintidós`, `ventuno` with the vowel elided and
  `ventitré`, `einundzwanzig`, `tweeëntwintig`, `vinte e um` and Portugal's `dezasseis`;
  `vingt-et-unième`, `vigésimo primero` in two words and one, `ventunesimo`, `einundzwanzigste` with
  each ending, `eenentwintigste`, and the feminine of every Romance ordinal — and `ELSEWHERE` is the
  table built from them once, on first use. `numerals::plainly` is what both sides are read through:
  folded by `folded_letters`, so a mark, an `ß` and a `ë` cost no second spelling, and every run of
  separators one space, so `Disque Vingt-et-un`, `disque_vingt_et_un` and `DISQUE.VINGT ET UN` are one
  disc. `numbered_after_the_word` looks a cardinal up whole and `ordinal_elsewhere_at_the_front` takes
  the longest ordinal the name begins with, so `Vigésimo Primero Disco` is the twenty-first, not the
  twentieth with `primero disco` left over. English keeps the composed tables above, its idiom being
  the one they were written for. `a_spelling_names_one_number_whichever_language_spells_it` holds every
  spelling to one number across the six, and
  `a_disc_past_twelve_is_composed_in_every_language_it_is_numbered_in` is the claim.
  `named_before_the_word` weighs the English and composed readings side by side and takes whichever
  leaves a separator and a disc word behind, an English ordinal prefixing some of theirs: `second`
  begins `Secondo Disco` and leaves `o disco`, naming nothing, where `secondo` leaves the disc. The
  composed table is read at its longest match likewise — `primer` begins `primera`. A set scanned under
  `Zweite CD` is keyed by that folder, as one under `Second Disc` was, so it stays one album a disc until
  scanned again from nothing. What it cannot do is tell one language from another: a folder is a
  string, so a word that numbers in one language numbers here whatever the rest of the name is in.
- **The key format is what `album_keys.key` holds**, so changing it means an existing library reads
  back under keys nothing matches and wants a rescan; `ORPHANS` takes away the albums nothing points at.
- **Two albums proving to share a release are gathered, and both their keys name what is left.** A
  release id the *pass* finds arrives after the grouping was made, so until it lands two albums
  scanned under different keys — a rip split across two folders, a set whose halves declare different
  owners — each held the same release and rows. `gather_under` runs inside `land_release`, after the
  release columns are written and before the wants are read: it finds every other album holding that
  `mbid` or already named by `release_key`, hands each one's tracks, release rows and *keys* to the
  album being landed, fills what the survivor holds nothing of — the cover, the year, the declared
  count, and the owner where the two agree — and deletes it. What makes it stick is that the keys move
  rather than being rewritten: the next scan computes the same key each folder was grouped under, finds
  it in `album_keys` naming the gathered album, and fills that album rather than making the row again
  — which a single `key` column could not do, why re-keying was once refused outright. `gather_under`
  then names the survivor by `release_key` too, so a file later tagged with the id lands on it through
  the first tier. Nothing joins on a key — `tracks.album_id` is the only membership — so the move is
  three `UPDATE`s and a `DELETE` whose cascade takes the loser's links and media.
- **An album takes the cover and year any of its tracks carries, not the first one's.**
  `Cache::covered` holds the albums holding a picture rather than the albums asked, so `store::cover`
  answers whether one landed and is asked again for the next track until one does; it reads
  `cover_art IS NOT NULL AND cover_source = 0` first, so an album holding the file's own picture costs
  a query and no probe, while one holding the archive's — `CoverSource::Archive`, code 1 — is probed
  again and the file's replaces it, the file's being deliberate and the archive's only a stand-in. **A
  thumbnail is not deliberate**: where `store::betters` says the archive's picture's shorter side is at
  least twice that of a file's whose shorter side is under `A_THUMBNAIL_BELOW` (300 px), the archive's
  stays and the album counts as covered. The year is the same shape: `Grouped` carries it, and a cache
  hit whose album has none runs `fill_year` rather than skipping the upsert that would have coalesced
  it. `store::year` reads a date with no separators too, so `19750601` and `197506` name 1975 where
  only `1975-06-01` and `1975` used to.
- **A file is asked whether it has a picture on the open its tags came off; the picture itself is
  still read at commit.** `probe_pictured` under `Picturing::Whether` answers the `MediaInfo` and
  whether there is a picture off one open, `TrackRecord::embeds_a_picture` carrying the answer, so
  `store::cover` reopens a file only where there is something to find. That removes the old price —
  one extra `probe_cover_art` per track of an album embedding none, and really every track, since an
  uncovered album never sets `Cache::covered` and asks again at the next row. An album that *does*
  embed one still costs one extra open, at its first committed row, deliberately: carrying the bytes
  out of the probe instead was measured and rejected. Copying a picture per file and claiming one per
  album key took a 1 200-track scan of 120 albums from 41 MiB of peak RSS to 111 MiB — a shelf of one
  cover per album alive from a worker's probe until that album's row commits, and a `BATCH` of a
  thousand rows keeping nearly all alive at once — to save 120 opens of 1 320. Bytes held across
  threads to save an open is the wrong trade; the `bool` is all that was worth carrying.
- **A play is a count, a date and a row of its own.** `Library::track_played` writes all three in one
  transaction: `tracks.plays` stepped, `tracks.played` set, and one `listens (track_id, at)` row
  stamped with the same nanos, so the two columns stay the cheap answer every listing reads while the
  history answers what the columns cannot. `listens` hangs off `tracks(id) ON DELETE CASCADE` and
  `configure` has `PRAGMA foreign_keys = ON`, so forgetting a root takes its plays with the rows —
  necessary because SQLite reuses a deleted `tracks.id` and an orphan would be re-attributed to
  whatever was rescanned into its place. The count is what a pane draws, the history what `plays:@`
  narrows on. **One order is read off the history on purpose**: `SortOrder::PlaysThisMonth` — *Most
  played this month* — ranks by a correlated `count(*)` of the listens since `unixepoch()` less thirty
  days, the whole count as tie-break, so it is a sort rather than an index walk, and
  `SortOrder::HELD_BY_AN_INDEX` is what the index guard walks, every other order still held to it.
  It is saved in a query's `sort` column as code 9, and a playlist has its twin: `playlist_plays`
  holds a row per play a playlist was loaded for — a `MIGRATIONS` step seeding each played playlist
  with its last play — and `PlaylistOrder::PlaysThisMonth` counts it the same way, so a playlist's
  plays are a history, not one date and a total. It is apart from the playlist's row, with no cascade,
  since `undo.rs` re-creates a playlist from what it held and a cascade would lose the history on every
  undo; the history's span ages it with the listens. SQLite hands a discarded playlist's id to the
  next one made, so `playlist::created` clears whatever `playlist_plays` still holds under the id it
  was given, and a new playlist starts with no plays this month
  (`a_playlist_taking_the_id_of_one_discarded_starts_with_no_plays_this_month`).
  `the_tracks_most_played_this_month_are_ordered_by_what_the_month_heard` and
  `the_playlists_most_played_this_month_are_ordered_by_what_the_month_played` are the claims.
- **A file that moved is followed, not forgotten and found again.** A rename or move by hand —
  anything but `organise`, which rewrites the rows itself — reads to a scan as a row whose file has
  gone and a file no row names. `moves::follow_the_moved` runs before the prune and pairs the two: a
  whole-file row, the only row at its path, whose file is missing, with a row this scan added —
  `added` at or past the generation — alike in `file_size`, `duration`, `codec`, `tagged_title` and
  `tagged_artist`. Where more than one on either side is alike — two identical rips moved at once —
  `moves::told_apart` weighs each gone path against each new one by how many names they share from
  the end, the file's own then its folders', and pairs two only where each is the other's one best and
  that best shares at least the file name: `vinyl/echoes.wav` and `tape/echoes.wav` filed under
  `filed/` are each followed to their own folder, while the same two moved to `c/` and `d/` share only
  the file name with both and are left to be forgotten and found rather than guessed between. **What
  the first pass leaves is weighed again by how it sounds.** A tagger filing as it tags changes the
  size and tagged names in one stroke, so `pairs_by_sound` takes what is left on both sides and pairs a
  gone row with a new one only where they share a `Sound` — the exact decoded length in frames, the
  codec, the rate and the channels — each being the *only* row on its side with that sound, and where
  they still agree on the tagged title, the tagged artist or the file's own name. The frame count makes
  the pairing safe and the uniqueness keeps two rips of one length from being guessed between; the
  agreement keeps a file deleted and an unrelated one of the same length added from being taken for one
  moved file. **Where no name agrees, the packets decide.** The scan reads every whole file through
  `probe_scanned`, which digests the first `PACKETS_DIGESTED` (48) packets of its audio track as
  symphonia's reader hands them out — the coded bytes, before any decode, with FNV-1a — into
  `tracks.packets`. A tagger rewrites what sits around the audio, never a packet, so a gone row and the
  one new row of its sound whose digests agree are one file however renamed and retagged, and two whose
  digests differ are not, with no decode or study behind either
  (`a_file_retagged_past_every_name_as_it_moved_is_followed_by_the_packets_its_scan_digested`). A row
  scanned before the column held none until read again, so the `MIGRATIONS` step after the column's
  marks every rooted whole file with no digest `probe_again`, and the first scan after it reads each
  once whatever its size and mtime say
  (`a_catalog_carried_forward_reads_again_every_whole_file_it_holds_no_packets_for`). For a row whose
  file moved before that scan could read it, **the print decides.** A gone row whose study kept a
  Chromaprint is paired with the one new row of its sound where `resonate_analysis::print` — the first
  two minutes decoded exactly as the study decodes them, nothing past — writes the same print, so a file
  retagged past every name and renamed as it moved is still its row; an unstudied row, or a differing
  print, is forgotten and found as before. The decode is asked only of the few rows a unique sound
  leaves unnamed, and never inside a transaction: `moves::to_be_heard` runs the same pairing on a read
  connection with a `heard` that notes each path it is asked about and answers nothing, the scan
  decodes those with no lock held, and `follow_the_moved` then pairs inside the write with the prints
  already in hand — so a bulk move no longer holds every other writer past its busy timeout while
  files decode.
  `a_file_retagged_past_every_name_as_it_moved_is_followed_by_the_print_its_study_took`,
  `a_file_retagged_as_it_moved_is_followed_by_what_it_sounds_like` and
  `a_file_taken_away_and_another_of_its_length_added_are_not_one_file` are the claims. A pair is
  followed through `organise::files_moved`, so the new row is dropped and the old one takes its path,
  root and generation, keeping its id, plays, listens, favourite, enrichment, vault object, playlist
  rows, kept lyric and queue row. The album is `settle_the_album`'s: where the album the scan made for
  the new folder holds nothing else and every row moved into it came out of one album, it is gathered
  into that album through `enriched::gather`, so the new folder's key names the album the rows always
  had and a release, cover and favourite are not left behind; otherwise the moved row joins the album
  the scan filed it under. **A file a sheet cuts is followed whole.** `cuts_moved` groups the rows
  sharing a path on either side — every row gone where the file is missing, every row new where none of
  the path's rows is older than the scan — and pairs two groups alike in size, codec and every cut's
  start and length, each the one group of that shape on its side, where the titles the sheet gave or
  the file's own name agree; `files_moved` moves every row of the path at once
  (`a_file_a_sheet_cuts_moved_with_its_sheet_keeps_every_rows_plays`). `ScanStats::moved` counts the
  rows followed and `added` leaves them out.
  `a_file_moved_between_scans_keeps_its_row_its_plays_and_its_place_in_a_playlist`,
  `an_album_moved_into_a_folder_of_its_own_stays_the_album_it_was` and
  `two_files_alike_in_every_way_are_told_apart_by_the_folders_they_moved_with` are the claims.
- **A track's count is the catalog's, and a rescan leaves it where it stands.** `tracks.plays` and
  `tracks.played` are absent from the upsert's `DO UPDATE SET`, as `added` is, so a rescan keeps both;
  forgetting a root drops the rows and counts. It is kept against the row, so a file no scan has seen is
  not counted against one — `Library::track_played` answers `None` rather than refusing where a
  non-local location has no row — **but the play is kept against the path.** `unheld_listens` holds
  the path, the span's start, the moment and how long it was heard, and `history::credit_the_unheld`
  runs after every scan's prune: a play whose path and start now name a row becomes a `listens` row
  stamped when heard, the row's `plays` and `played` follow, and the unheld play goes, so files played
  before their folder was ever scanned are counted the moment it is
  (`a_file_played_before_any_scan_saw_it_is_credited_with_the_play_and_the_time_heard_once_one_does`).
  A `Counted` carries a `Listen` — `Held` naming a `listens` row, `Unheld` the rowid of an
  `unheld_listens` one — and `Library::listened` spends either, so a settle keeps what it heard: the
  statistics count an unheld play and its time beside the held, and the credit carries `heard` onto the
  listen it becomes. The history's span ages the unheld plays too. It answers with the row it counted
  where there was one, read back inside the same transaction so the caller has the new count without a
  read of its own. `plays:` and `played:` narrow on the two columns and `SortOrder::Plays` and `Played`
  order on them, so *Top 25 most played* is a saved query rather than a feature, and the count is drawn
  through `listing::times` wherever a row names a track — the tracks pane, the queue, an opened playlist
  and the playlists index, where those words started.

## Enrichment

- **What a reference says lands beside what the scan read, in columns the scan never rewrites.**
  `artists` carries `mbid`, `sort_name`, `kind`, `gender`, `country`, `area`, `began_in`, `began`,
  `ended`, `has_ended`, `disambiguation`, `portrait`, `portrait_format`, `asked` and `answered`;
  `albums` carries `cover_source`, `mbid`, `release_group`, `release_title`, `date`, `country`, `label`,
  `catalog_number`, `barcode`, `kind`, `disambiguation`, `asked`, `asks` and `answered`; `tracks`
  carries `mbid`, `artist_mbid`, `release_track_mbid`, `isrc`, `tagged_title`, `tagged_artist`,
  `release_title`, `asked`, `asks` and `answered`. Beside them: `release_tracks`, a row per release
  track with `release_tracks_by_album` and `release_tracks_in_order` over it; `artist_genres`; the three
  link tables `artist_links`, `album_links` and `release_track_links` — a `relation`, a `provider` and a
  `url`, the first two as codes through `store::relation_code` and `service_code`, read back through
  `relation_of` and `service_of`, which refuse a later build's code with `Error::UnknownLinkCode`;
  `artist_releases` (the discography, below); `wants`; `lyrics_kept`. Those were laid into `V1` as first
  written; what came later — `cover_asked`, `track_credits`, `refused_releases`,
  `lyrics_kept.lyricsfile`, `releases_unread` among them — arrived as `MIGRATIONS` steps. Of all of it
  the scan's upserts touch only the ids, the two `tagged_` columns and what the tags *declare* about a
  release — `barcode`, `catalog_number`, `label` and `albums.tagged_tracks` (`TRACKTOTAL`) —
  `release_media` being the one table beside `release_tracks` a landing writes and a scan never sees:
  `coalesce(excluded.mbid, albums.mbid)` on an album, `coalesce(excluded.mbid, artists.mbid)` on an
  artist with `fill_artist_mbid` for one the cache held, `store::Declaration` coalesced the same way on
  an album — the file's where it names one, the held otherwise, as `mbid` and `release_group` go — with
  `fill_declared` for one the cache held, filling only what `Declared` says the row still lacks, so a
  file that stopped carrying its barcode leaves the one held
  (`a_scan_stores_what_the_tags_declare_about_the_release`,
  `a_rescan_keeps_a_barcode_the_file_no_longer_carries`); and `tracks.mbid`, `artist_mbid`,
  `release_track_mbid` and `isrc` taken from the tags wherever the file names one and kept where it
  names none unless the file was retagged — the weighing of the two `tagged_` columns the names take —
  since those four are also what a lookup and a pairing *find*, and a whole rescan once wrote the file's
  silence over a recording a search had identified, which the lookup would not ask about for a month
  (`a_whole_rescan_keeps_the_recording_a_lookup_identified_a_file_by_until_it_is_retagged`). An
  *answered* row whose file is unchanged — same size, mtime, sheet mtime and span — keeps all four and
  its `track_number` and `disc_number` whatever the file says, the one write replacing what a file
  carried being a listener taking the name its audio was heard as (`analysis.md`), which a rescan of the
  same bytes putting the file's word back would undo; a lookup otherwise only fills, so for every other
  answered row the file's value and the held one agree. So a rescan leaves every other enrichment column
  where it stood (`a_rescan_leaves_every_enrichment_column_where_it_stood` in `tests/library.rs`), the
  one exception being the retagging rule below, the only place a scan writes `tracks.answered`. The four
  declared columns build the ask: `asking_albums` hands `catalog_number`, `tagged_tracks` and the
  owner's `artists.mbid` to `AlbumToAsk` beside the barcode, so a search names what the tagger knew
  about the pressing.
- **`tagged_title` and `tagged_artist` are what the *file* was read as — the whole of how a rescan
  tells a retagging from an identification.** An enrichment writes `tracks.title` and `tracks.artist`,
  as the scan does, so without a record of what the file said the next rescan would put the tagger's
  spelling back over the reference's. The upsert keeps an answered row's `title`, `artist` and
  `artist_id` where both `tagged_` columns come back unchanged, takes the file's where either moved,
  and sets `answered` to `NULL` in that `CASE` so the row is asked again
  (`a_rescan_keeps_an_answered_tracks_names_unless_the_file_itself_was_retagged`). A never-answered row
  takes the file's names as always. The two columns hold what `scan::name_from_stem` left in the
  `TagSet`, not the tag alone: a file naming no title is read through `stem.rs` first, so
  `tagged_title IS NULL` means neither tags nor file name said anything, and `title` is then the bare
  stem `store::title` falls back to. `stem::read` takes a leading number for the track only where
  punctuation parts it from the rest (`03.`, `03 -`, `7)`, `003_`) or it is padded (`03 So What`): a
  number a space alone parts, unpadded, is the title's own, so *99 Luftballons* and *21 Guns* keep
  their names (`a_number_only_a_space_parts_from_the_title_is_the_titles_own_unless_padded`).
  **A name the file's *name* gave is not a tag, so renaming the file
  is not retagging it.** `tracks.named_by_its_stem` — a `MIGRATIONS` step, nothing before having kept
  the fact — says the scan's reading took its title or artist off the stem, and `store::RETAGGED` is the
  one reading of *the file said something else* every upsert column weighs: the tagged names moved, and
  not between two readings both off the stem. A stem-named row a lookup answered keeps what it was told
  when renamed by hand, edited or both, while a tag written into it or taken out is still a retagging
  (`a_row_named_by_its_file_name_keeps_what_a_lookup_answered_when_it_is_renamed`). A row scanned before
  the column holds nothing and is weighed the old way until next read. `store::index_row` is shared by
  the scan and `land_recording` so a corrected title is in `tracks_fts` a moment later, not at the next
  scan, and the upsert answers the title, artist and artist id it *left* on the row, so a rescan indexes
  those rather than the file's (`a_rescan_indexes_and_bills_the_names_the_lookup_kept`).
- **The seam is `Reference`, and `Library::enrich` is the pass that walks it.** It speaks in `Mbid` —
  the dashed lowercase text or `resonate_core::Error::NotAnMbid` — and `Isrc` — twelve characters,
  upper-cased and stripped of dashes, or `resonate_core::Error::NotAnIsrc` — each a shape the type
  refuses anything else in, both in `resonate-core` because a provider crate names them and may not see
  the library, and re-exported here. `reference.rs` carries the rest of the vocabulary — `Release`,
  `Medium`, `ReleaseTrack` and `Credit`; `Wording` (`Phrase` or `Words`, how a search is put);
  `ReleaseAsked` and `ReleaseMatch` (the latter with the hit's `group` and whole `credit`);
  `Recording`, `RecordingRelease`, `RecordingAsked` and `RecordingMatch`; `ReleaseGroup`,
  `GroupRelease`, `GroupAsked` and `GroupMatch`; `ArtistProfile`, `LifeSpan`, `Genre`, `ArtistRelease`
  and `ArtistMatch`; core's re-exported `Link`, `Relation` and `Service`; `SongLink`, `LinkNames`,
  `AlbumLink`, `AlbumNames`, `Barcode` and `BarcodeMatch` (below); and `LookupOp`, the twenty-two things a service can be asked for — thirteen a reference's,
  the rest the lyric provider's (`Lyrics`), the AutoEq catalogue's (`Devices`), the correction
  source's (`Correction`), a recogniser's (`Recognise`), a scrobbler's (`Submit`, `Love`, `Token`),
  the stream lookup's (`StreamLink`) and the link follower's (`FollowLink`) — and the `Reference`
  trait, whose twenty methods (beside `source`) answer `Option`s and `Vec`s in that vocabulary and
  nothing about how they were reached. `resonate-online`'s `Online` is the one implementation, only the
  binary reaching it behind `online`, so `cargo tree -p resonate-library` stays free of `ureq` and
  `serde`. `enrich.rs` is the pass: a `resonate-enrich` thread behind an `EnrichHandle`, with
  `EnrichProgress` counting albums, releases, matched rows, covers, tracks, the tracks a lookup renamed
  (`named`), artists, portraits, releases found for an artist, refusals, studies, fakes, recognitions,
  misnamed tracks and lyrics, answering `is_cancelled` between requests, and `EnrichSummary` carrying
  the stats, whether it was cancelled and `stopped_by`. `EnrichOptions` is `refresh` (ask again about
  what was answered), `at_most` (capping albums, tracks and artists each to that many), `sought` (the
  cell below), `studies` and `lyrics` (the `study` and `fetch-lyrics` keys). The rematch-only albums are
  walked first, so a capped run still pairs every album's rows; what is left is one queue of `Ask`s —
  due albums, then due tracks, then due artists — walked in order, albums first because a landing
  album retires every track under it before the track pass reaches one.
  `crates/resonate-library/tests/library.rs` drives it all through a `Fake` answering canned releases,
  recordings, groups and profiles and faulting on the call it is told to — where the rules below are
  proved; the online crate proves its mapping over captured fixtures and reaches the services only
  under `RESONATE_ONLINE_TESTS`.
- **A picture is fetched beside the pass, never in it.** `Pictures` is two threads (`PICTURE_READERS`)
  named `resonate-pictures-<n>` reading a bounded channel of `Picture`s — a cover, a release group's
  cover or a portrait — and `Pass::want` is the whole of how one is asked for; `Pictures::rest` drops
  the sender and joins them before the summary is taken, so the stats say what landed. The archive and
  Wikimedia Commons are not MusicBrainz, and `Client::pace` keeps a slot per host, so a picture costs
  the pass only the handover while the next MusicBrainz request waits out its second — that service's
  one request a second being what a pass is really made of. A picture that cannot be fetched is a
  warning and a `refused` count, not the end of a pass (which finds out for itself at its next
  request), and where no reader thread started `want` fetches on the spot. A picture call therefore has
  no place in the order the pass asks in, so `asked_in_order` leaves it out of the sequences the tests
  assert, and `a_picture_is_fetched_beside_the_pass_rather_than_in_it` holds a cover and watches the pass
  ask the next question anyway.
- **What a listener has just reached for is asked next, and a seek is spent once.** `Sought` is a
  `Mutex<Vec<Seek>>` shared with whoever started the pass — `Sought::album` and `Sought::artist` put
  something on it, newest last, each held once — and `Sought::taken` drains it at the top of every turn
  of the queue. `Pass::lift` reads it: `rotated` moves a named `Ask` to the front of what is *left*, so
  the newest leads, each seek rotating its match to index zero over the one before. A seek naming
  nothing the queue still holds is not dropped but *read back* — `Library::album_if_due` and
  `Library::artist_is_due` weigh the row against the queue's `Waits` — and where still due the `Ask` is
  inserted at the front. So an album a scan landed while the lookup ran is reached by a click, which a
  rotation over a slice never could, and an album already answered is not: not due, the read answers
  `None`. What a pass already asked *this run* is refused by the `spent` set rather than the clocks,
  since `refresh` makes every row due and a click on the album being asked would ask it twice.
  `LibraryModel` holds one `Sought` for the window's run and hands it to every `enrich`, so a row
  reached while nothing runs is still at the front when a run starts.
- **The window seeks what it has drawn, front-most last.** `LibraryModel::select` seeks the album and
  its owner, and `ask_about_what_is_drawn` runs where a listing lands: an artist selection seeks every
  album the drawn tracks belong to then the artist, in reverse of listing order, since `lift` rotates
  each seek to index zero in turn and the last pushed is the first asked. The artist leads, then the
  first album on screen, then the rest down the pane.
- **An artist's row is read when it is asked, not when the queue was built.** `artists_to_ask` answers
  the due ids, `Ask::Artist` carries one, and `Library::artist_to_ask` reads the row — so the
  `artists.mbid` `Pass::credits` wrote while a *later* album was landing is the one `profile_of` sees.
  The pass once got that for nothing by reading `artists_to_ask` only after the album loop; one queue
  holding both cannot, and reading the row where it is used makes the ordering irrelevant rather than
  load-bearing (`an_artist_named_in_a_release_credit_is_looked_up_by_the_credit_id_and_never_searched`).
  **An artist the pass itself brings into the catalog is asked before the pass ends.** A landing can
  bill somebody no row named yet — the Witcher 2 score's tracks landed crediting Adam Skorupa, Krzysztof
  Wierzynkiewicz and Oleksa Lozowchuk by id — and that row was due only next pass, sitting with an mbid,
  no profile and no portrait. `Pass::run` reads `artists_to_ask` again once the queue is walked and
  walks whatever it names that the pass has not spent, until nothing new is due; a capped run
  (`--albums`) does not, the cap being a promise about how much is asked
  (`an_artist_a_landing_names_for_the_first_time_is_asked_about_in_the_same_pass`).
- **`asked` and `answered` are the two clocks, `asks` and `refusals` the columns they are read with,
  and `Waits` the three waits in one value.** `due_again` writes the clause for a table's alias, and
  `albums`, `tracks` and `artists` are each read through it: never asked, asked and unanswered longer
  ago than the wait its `asks` earned, answered more than `REFRESH_AFTER` (thirty days) ago and not
  asked in vain since, or anything under `refresh`. **An answer asked again in vain waits its turn like
  any other ask**: a landing zeroes `asks` and `refusals`, so a row carrying either since its answer was
  stamped by a fruitless ask after it, and is due only once `waited_its_turn` — where the clause once
  read `answered < ?4` alone, and a stale row the reference missed or refused was due on every pass
  (`a_stale_answer_asked_again_in_vain_waits_its_turn_like_any_other_ask` in `enriched.rs`). The same
  reading makes an answered row carrying a refusal due once it has waited, stale or not — how an
  artist whose discography was refused is asked again within the hour
  (`an_answer_whose_companion_ask_was_refused_is_asked_again_once_it_has_waited`). `Waits` carries `retry_after`, `refused_again_after` and `refresh_after` together
  rather than as three `Duration`s a caller could swap; `WAITS` is this build's, and a test builds its
  own. What a failure does depends on which it is, through `Pass::heard`: `Error::Unreachable` ends the
  pass and stamps nothing, so `EnrichSummary::stopped_by` names the `LookupOp` and a run with no network
  asks the same albums next time rather than writing a day's silence into every one; `Refused` and
  `Unreadable` count in `refusals`, stamp `asked` and carry on, one refused album saying nothing about
  the next — **until `REFUSALS_THAT_END_A_PASS` (10) come in a row**: `Pass::refused_in_a_row` counts
  them, any answer puts it back to nothing, and the tenth is `Error::RefusedInARow`, which ends the
  pass as `Unreachable` does, `stopped_by` naming its op and the row being asked left unstamped — a
  service refusing everything having said something about the next album after all
  (`a_reference_refusing_lookup_after_lookup_ends_the_pass`). Landing a release or profile stamps
  both, inside the transaction writing it. An album, track or artist gathered into another or
  removed while it is asked about answers `UnknownAlbum`, `UnknownTrack` or `UnknownArtist`, which
  `passed_over_if_gone` logs and passes over, the pass going on to the rest
  (`an_album_gone_while_a_lookup_asks_about_it_is_passed_over_and_the_pass_goes_on`); any other
  error that ends the pass cancels its progress and rests the picture, study and lyric workers before
  it is returned, rather than leaving them working through the rest of the queue.
- **A row answering nothing is asked half as often each time, and an answer puts the wait back.**
  `asks` counts the stampings a row took without an answer — every `stamp_*_asked` carrying
  `Fruitless::Missed` steps it — and `due_again` waits `RETRY_AFTER` (24 h) doubled that many times,
  `WAITS_DOUBLE_AT_MOST` (5) capping it at thirty-two days, so a library of files the reference cannot
  name costs one pass, not one a day for ever. Every landing writes `asks = 0` beside its `answered`,
  and a retagging is the scan's own reset: `store::apply` puts `asks` back to nothing wherever it nulls
  `answered`, so a file whose tags moved is asked at once, not a month later. `refresh` still reaches
  every row (`resonate enrich --refresh`).
- **A refusal is bounded like a miss, by a count of its own.** `refusals` is the asks in a row that
  ended `Fruitless::Refused`, stepped by the same `stamp_*_asked` and put back to nothing by a miss or a
  landing, and `waited_its_turn` reads whichever count the last ask left standing: `doubled` writes one
  clause for both, so a refused row waits `REFUSED_AGAIN_AFTER` (1 h) doubled per refusal under the same
  cap and a missed one `RETRY_AFTER` doubled per ask. A service refusing one row for ever is asked about
  it every thirty-two hours, not hourly for ever, and a service with one bad minute comes back to the row
  an hour later. The counts are exclusive, not added: the last answer says which wait the row serves, so
  a miss after a refusal waits the day a first miss earns, not the hour the refusal had reached.
- **A pass says it is running, so the next launch carries it on.** The `enrichment` table holds one row
  while a pass runs, carrying the `refresh` it started with: `note_began` writes it as `run` starts and
  `note_finished` removes it where the pass reached the end of its queue or the listener stopped it. A
  pass the reference ended (`stopped_by` `Some`) and one never ended, the process having gone, both
  leave the row standing, read back by `Library::unfinished_enrichment`. `LibraryModel::new` asks at
  start and, where a row stands and the build can reach the network, carries the pass on with its
  refresh; the mark cannot fail a pass, so a library written before the table warns through `tracing`
  and enriches as always. Nothing else needs picking up: a row the interrupted pass never reached was
  never stamped, so it is due. Headless, the binary's `carrying_on` is the same reading, and both ways
  in take it — `resonate enrich` and a scan handing over — because an ordinary pass running to the end
  removes the mark, so a scan's handover would otherwise *discard* an interrupted refresh. A run asking
  `--refresh` itself already does the wider pass and reads nothing.
- **A release is taken by tag or strict match, an artist by tag or exact one, and a near miss writes
  nothing.** `Identified` is `Found`, `Group` or `Nothing`. An album with `albums.mbid` set is asked for
  directly, and where the reference answers `None` under it the album goes on to the search — a
  `Refused` under a tagged id stops it, one bad minute saying nothing of the tag
  (`a_tagged_release_id_the_reference_does_not_hold_falls_back_to_a_search_that_lands`); a tagged
  `albums.release_group` the reference holds nothing under falls through to `find_group` likewise
  (`a_tagged_release_group_id_the_reference_does_not_hold_falls_back_to_a_group_search`). `find_release`
  is asked with the title, the owner and owner's `mbid`, and the tags' barcode and catalogue number, and
  `top_release` takes the top of `top_of` weighing `weighed_release` — `(agreed_owner, count_fits,
  year_agrees, score)`, in that order — landing it only where `matches_strictly`: `agreed_owner` (a
  score of `STRICT_SCORE`, 95, or over, with a credit the owner agrees with — by the id the tags gave or
  a name that agrees) and `count_fits`, a `track_count` equal to `declared_count` — `tagged_tracks`, the
  `TRACKTOTAL` the files declared, or the rows held where none did, so a rip short a track is weighed
  against the pressing it was ripped from, not its own hole. A top hit agreeing on owner and score but
  not count, carrying a `group`, is `Identified::Group` — the group fetched with no `find_group` between,
  landed as the bullet below says — and anything short is `Nothing`
  (`a_hit_is_weighed_on_its_owner_its_count_its_year_and_then_its_score`,
  `a_strict_hit_of_the_wrong_count_names_its_group_and_one_without_a_group_names_nothing` in
  `enrich.rs`). An album with no owner is weighed on score and count alone — what a compilation offers.
  An artist is `profile_of`: a tagged `artists.mbid` is asked for directly and falls through to
  `find_artist` where the reference holds nothing
  (`a_tagged_artist_id_the_reference_does_not_hold_falls_back_to_a_search_that_lands`), and a search hit
  must `matches_exactly`: `EXACT_SCORE` (100) and the same name, a name having nothing but itself to be
  checked against. A hit falling short is a debug record naming what it was and how it fell, and the
  album or artist is stamped `asked` alone, so nothing a person did not tag is written on a guess.
- **A match the listener says is wrong is taken away and never landed again.**
  `Library::forget_the_match` records the album's release — or its release group, where it was landed
  as a group alone — in `refused_releases`, the eighth `MIGRATIONS` step, clears what the landing wrote
  (ids, release title, date, country, kind, disambiguation, an archive cover), removes the release's
  rows, media and links, and puts `asked` back to nothing so the next lookup asks at once. **What the
  match wrote on the tracks goes with it**: each track takes back the title and artist its file gave —
  `tagged_title` and `tagged_artist` — loses the release's title and a release-track id naming a removed
  row, is put back to never asked so the next lookup identifies it afresh, and is re-indexed under its
  name, so a track renamed by a wrong pressing is not left billed and found as that pressing had it
  (`forgetting_a_match_puts_back_the_names_the_files_gave_and_asks_about_the_tracks_again`). A recording
  id or ISRC one of the release's rows holds goes too, since pairing stamped it from the row and the
  next lookup would take it as tagged and rename the track to the refused recording as an exact
  identification; such a row is marked `probe_again`, so the next scan reads back whatever the file
  itself carries (`forgetting_a_match_takes_away_the_recording_ids_and_codes_it_stamped`). Label,
  catalogue number and barcode stay, the tags perhaps having given them. `take_release` and `take_group`
  are where every route lands, tagged id and search alike, and both ask `Library::refuses` first and pass
  a refused id over as `nothing_landed`, so the next lookup settles on another pressing or nothing. Only
  the release is refused, not its group, so a wrong *edition* is put right by another pressing. A
  gathering carries the loser's refusals onto the survivor
  (`a_match_the_listener_forgets_is_taken_away_and_never_landed_again`). **The listener can choose the
  pressing too.** `Library::take_pressing` asks the reference for the named release, lifts any refusal
  of it for that album — `enriched::forgive`, a hand-chosen pressing outranking one said to be wrong —
  lands it through the lookup's `land_release` and pairs the rows again, so a strict match on the wrong
  edition is put right without forgetting anything
  (`a_pressing_the_listener_chooses_is_landed_in_place_of_the_one_the_lookup_took`).
- **A name agrees in one of six ways, and `Spelling`'s derived `Ord` is the whole ranking.** `same_name`
  answers `Marked` where the two `folded_title`s agree, marks and all, `Stripped` where only the
  `stripped_title`s do, and `Dequalified` where they agree only once `dequalified` took a version
  qualifier off the end of each; `names_it` weighs a MusicBrainz artist's aliases the same way and
  lowers each answer through `as_an_alias` to `AliasMarked` or `AliasStripped`; `same_credit` answers
  `ById` above them all where a credited artist's `mbid` is the one the tags gave, an id the tagger
  wrote being the one thing no spelling outweighs. Declaration order is rank — `Dequalified <
  AliasStripped < AliasMarked < Stripped < Marked < ById` — so a real name beats an alias however well
  spelled, and a title that gave up a qualifier is taken last. All six are an agreement, so a tagger who
  wrote *Marcin Przybylowicz* is identified against the *Marcin Przybyłowicz* MusicBrainz answers with,
  where the marked fold alone left that artist stamped `asked` and asked again every `RETRY_AFTER` for
  ever. Which answer is taken is `top_of` weighing `(Option<Spelling>, score)`, so an agreement beats
  none, a better spelling beats a worse whatever either scored, and the score decides between two of a
  kind — two artists differing only by a mark stay two, the marked tag taking the marked row and never
  the reverse. Where no answer agrees, the weight is `(None, score)` for all and the debug record names
  the highest-scored. A credit is weighed by `same_credit` twice — the names joined as MusicBrainz bills
  them, and each credited artist singly — and the better is the answer, so *The Weeknd with JENNIE &
  Lily-Rose Depp* agrees with a file tagged *The Weeknd* alone
  (`a_credit_agrees_where_any_one_of_its_names_does_and_an_id_beats_every_spelling`,
  `a_release_owned_by_the_tagged_id_agrees_however_the_credit_spells_it`,
  `a_collaboration_credit_agrees_where_the_file_names_one_of_its_artists` in `enrich.rs`;
  `an_album_whose_owner_holds_an_id_is_searched_for_by_that_id_and_agrees_by_it` through the pass). Only
  the artist routes read aliases: `owned_by` weighs a release's or group's credit against the album's
  owner and `matches_a_recording` a recording's against what the track was asked with, both through
  `same_credit`, and where a recording's title and credit agree by different spellings the weaker is
  what the match is weighed as.
- **A version qualifier is a closed list, everything outside it naming a different recording.**
  `VERSION_QUALIFIERS` is the fourteen spellings `dequalified` takes off a bracketed title's end —
  *Album Version*, *Radio Edit*, *Explicit*, *Clean*, *Remastered*, *Bonus Track*, *Original Mix* and
  the rest — each weighed through `folded_title`, so case and punctuation inside the bracket do not
  matter, and `a_dated_qualifier` adds a remaster with a four-digit year either side, so *(Remastered
  2011)* and *(2011 Remaster)* both go while *(Remastered by Ada)* stays. `(with Justin Bieber)`,
  `(Live)`, `(Remix)`, `(feat. …)`, `(Acoustic)`, `(Demo)` and anything unlisted stay, being a different
  recording rather than a different pressing, and taking them off would pair the wrong take. It strips
  from the end inwards, in `(` `)` and `[` `]` alike, repeats until nothing more comes off and refuses to
  leave nothing, so *Know (Album Version) (Radio Edit)* folds to *Know* while *Explicit* stays a title.
- **An album no pressing matches is still an album, landed as its release group.** `Pass::album` reads
  four routes in order: `albums.mbid` (the tags'), `find_release`, `albums.release_group`
  (`MUSICBRAINZ_RELEASEGROUPID`'s) and `find_group`. The group answers a release search that keeps
  failing on the count — a rip missing a track, a bonus disc, a reissue with two more — because
  `matches_as_a_group` weighs the score and the folded owner and **no track count**, a group having
  none, which is why it can answer where `matches_strictly` cannot; it is also reached from the release
  search where the top hit is strict in all but the count and names its group. The cost is that a group
  names many pressings and the catalog wants one, decided in `settle_group`: `closest_release` answers a
  pressing and a `Fit` — `Fit::Exact` whose `track_count` *is* the `declared_count`, then `Fit::Wider`
  for the smallest pressing holding more tracks than the album, and where there is none `Fit::Narrower`
  for the widest holding no more, the earliest dated among equals every way (`earliest` sorting an
  undated release last, taken only where nothing else fits) — and `land_release` runs on that id as
  though the search had found it. A wider pressing is landed rather than refused because the cost is
  honest: the rows the rip lacks are release rows with no `track_id`, so `Album::missing` counts them
  and the Missing pane lists them, where a rip weighed against its own hole was landed as nothing
  (`a_group_with_no_exact_pressing_lands_the_smallest_wider_one_or_the_widest_narrower_one` in
  `enrich.rs`, `a_hit_one_track_short_lands_the_pressing_its_group_names_and_lists_the_missing_row`
  through the pass). A narrower one is landed for the same reason read the other way: a rip carrying
  bonus tracks no pressing has is short of nothing, so the widest pressing is the most the reference can
  account for, and landing it writes the release columns, links and the rows it *does* name rather than
  leaving the album with a group id and nothing else — the fallback weighing the rip's own count, not
  `declared_count`, so a `TRACKTOTAL` naming more tracks than the files hold still lands the pressing as
  wide as the rip (`an_album_wider_than_every_pressing_of_its_group_lands_the_widest_one`). Only where no
  pressing in the group declares a count does `take_group` run `land_release_group`, deliberately thin:
  `albums.release_group`, with `kind`, `date`, `year` and `disambiguation` coalesced so the release
  columns a pressing would fill are never guessed. It writes no `albums.mbid` and no `release_tracks`, a
  pressing unable to hold the rip not being its source. Half an answer, so the pass goes on asking:
  `album_due` is `due_again` with `HOLDS_A_GROUP_ALONE` — a `release_group` and no `mbid` — under the
  same `waited_its_turn` a fruitless ask waits, and the landing leaves `asks` where `stamp_album_asked`
  put it rather than zeroing it, so the retries spread a day, two, four and on to the
  `WAITS_DOUBLE_AT_MOST` ceiling. A retry is worth taking because the album moves under it: a rip
  finished, a track retagged, a count now matching a pressing the group held — where the month
  `REFRESH_AFTER` names was the only thing that would look again. A pressing that lands puts `asks` back
  to nothing, taking the album out of the retry. The archive is asked for a cover only in the pass that
  landed the release or its group, and only where the album held none or held a thumbnail.
- **A search is asked as a phrase, and as words wherever the phrase *landed* nothing.**
  `found_either_way` is how `find_release`, `find_group` and `Route::Search` all ask: once with
  `Wording::Phrase`, the online crate's fielded query, and again with `Wording::Words`, the loose dismax
  one, wherever weighing the first answer took nothing. It serves three by weighing as well as asking —
  taking the caller's `weigh` and answering what it answered — and `Landed` is what the three results
  share, `Identified::Nothing` and two `None`s being three spellings of nothing landing. An empty answer
  and a near miss are therefore one case — the correction: a phrase finding the wrong pressing was once
  left there, since the strict rule had weighed it, but the queries differ — the fielded phrase matches
  a title exactly and the dismax words loosely — so a pressing the phrase ranked under the wrong one, or
  missed over a subtitle the tagger dropped, is reachable only by asking again. A refusal is still not
  asked again, being the service's bad day, and it is no answer: `found_either_way` hands back a
  `Heard`, so a refused release search ends the album's ladder and stamps the refusal where it once
  read as nothing found and went on to ask the release group, its phrase and its words of a service
  that had just said no
  (`a_refused_release_search_stamps_the_refusal_rather_than_asking_down_the_ladder`). A track's
  refused search still goes on to its fingerprint, AcoustID being another host. The cost is one more search per album the phrase could not
  settle, bounded by the doubling retry.
  `a_phrase_that_answers_nothing_is_asked_again_in_words_and_lands_under_the_strict_rule`,
  `a_phrase_that_answers_a_near_miss_is_asked_again_in_words_and_lands_there` and
  `a_track_the_phrase_answered_nothing_for_is_searched_again_in_words` are the three claims, and the
  group searches beside them are each asked once — the fourth: a phrase that lands is never asked again.
  `Looking` keeps one `found_either_way` for all three — a matchable pair of an id and a title, so the
  record saying the words are asked names an album or track rather than a spelled string.
- **An album is billed by the release where one landed, and by the tags until then.** `albums.title`
  is what the scan read and no landing rewrites it, being half of `store::album_key` and `sleeve_key`,
  whose rewriting would move an album's grouping key under it. `albums.release_title` is the
  reference's answer, written by `land_release` alone, and the `album_title!` macro in `db.rs` —
  `coalesce(a.release_title, a.title)` — is the billed title listings, sorts, wants and `organise`
  destinations read (some sites in `alternatives.rs`, `sung.rs`, `vaulted.rs` and `enriched.rs` spell the
  same `coalesce` by hand). A macro rather than a `const` because such SQL is `const` built with
  `concat!`, which takes literals. It buys the two discs of a set: they share a release id and group as
  one album whose `title` is whichever disc the walk reached first, and the release's own title is what
  the pane and layout say. A rescan cannot undo it, `release_title` being a column the scan does not name.
- **A medium is a row, a disc being a thing rather than a number on a track.** `release_media` is
  `(album_id, position)` with the `format` and `title` MusicBrainz answers, written by `land_release` and
  read back as `ReleaseDetail::media`; `release_tracks.disc` says which medium a row sits on.
  `models::album_rows` pushes a `ListedRow::Disc` (through `headed_by_disc`) above each run of rows
  where the album spans more than one disc — read off the rows, so a set the reference never answered
  for still heads its discs — and `browser::disc_heading` names it from the medium where one landed,
  title over format. The heading is drawn with the `row` every track row takes, the tracks pane being a
  `uniform_list` where a taller row lays the whole list out wrong. `browser::pressings` is the other
  reader: the release line says `2 × CD` where every medium agrees and `N discs` where they do not.
- **Landing a release is one transaction, and pairing its rows a second.** `land_release` writes the
  release columns on the album, fills `year` only where the scan left none, stamps `answered`, deletes
  and reinserts `release_tracks`, `release_media` and `album_links` and writes each row's
  `release_track_links`; the wants under the album are read first through `wants_under` and put back
  once the rows exist again, so a want survives a refresh that changed row ids. `Carried` is what a
  want is put back *by*, most exact first: `Carried::Track` the release track's own mbid, `Recording`
  the recording's, `Seat` the `(disc, position)` it sat at — a re-edited release moving a track to
  another seat, where the seat alone would carry the want to whatever sits there now. `want_again` lands
  the exact ones first and the insert is `ON CONFLICT DO NOTHING`, so two wants landing on one row leave
  it to the better-carried, and a want whose track the release no longer holds is dropped as always.
  `rematch_release_tracks` then pairs release rows with catalog rows in five passes, each taking only
  rows the earlier left and catalog rows not yet taken: the recording mbid against `tracks.mbid`, the
  track mbid against `tracks.release_track_mbid`, the `folded_title` at its place, the `folded_title`
  anywhere, and last the place alone — disc and position against `disc_number` and `track_number`, a
  missing disc read as 1 only on a one-medium release, so a two-disc set never pairs a disc-less row
  with the wrong disc. The title outranks the place because a pressing in another order otherwise
  stamped each file with its neighbour's recording id and ISRC, which the tag writer then wrote and the
  scrobbler sent; the place still pairs what no title answers, and the title at its place first keeps
  two songs of one title — two *Interlude*s — each at its own seat
  (`a_pressing_in_another_order_pairs_each_row_with_the_song_of_its_title`). It writes `release_tracks.track_id`
  where the pairing *moved* and answers how many moved, so `EnrichStats::matched` says what a pass
  changed, not what was already true; and for every row it paired, moved or not, it fills
  `tracks.mbid`, `release_track_mbid` and `isrc` by `coalesce` from the release row, so a file tagged
  with no identifier learns its seat's (`a_paired_track_receives_the_identifiers_its_release_row_holds`)
  — a fill, not a correction: a code the file carries stands, and a rescan overwrites the three only
  where the file names one. `AlbumToAsk::rematch_only` makes an album holding release rows rematch
  whether or not it is due, so a rescan adding a file pairs it without asking the network — but only
  where a pairing could change anything: `albums_to_ask` offers such an album only where
  `HOLDS_AN_UNPAIRED_ROW` or `HOLDS_AN_UNPAIRED_TRACK`, an album fully paired having nothing for the four
  passes to find at the cost of a read and a write transaction each pass.
- **An album is not the only thing a file belongs to, so a track is asked about in its own right.** A
  file carrying no `ALBUM` tag has `album_id = NULL`, under no album the pass could reach, and was
  enriched by nothing — a whole shape of library, the singles and loose rips a tagger never filed.
  `Ask::Track` is the answer, joining the queue between albums and artists: `tracks_to_ask` is
  `WHERE NOT IS_PAIRED AND due_again("t")`, so a row `rematch_release_tracks` already paired is never
  queued, and `release_tracks_by_track` keeps `IS_PAIRED` a lookup rather than a read of every release
  row per track. The row is read when asked, through `Library::track_to_ask`, which asks `NOT IS_PAIRED`
  again — an album landing earlier in the pass pairs its tracks, so those rows retire silently rather
  than being asked over the network a moment after the answer arrived
  (`a_track_the_album_pass_already_paired_is_never_asked_about`) — the discipline
  `Library::artist_to_ask` follows for a credit, and why the queue is albums, tracks, then artists.
- **Four routes, most exact first, and the first answering ends the track.** `Route::ALL` is `Isrc`,
  `Recording`, `Search`, `Fingerprint`, and `Pass::track` walks them in order until one answers,
  stamping `asked` alone where none does. An ISRC the file carries is asked through
  `recordings_of_isrc`, which answers a *list*, a code naming every take released under it — so
  `best_recording` tells them apart by length and answers the take with a `Certainty`: a lone answer is
  `Exactly` where the lengths agree or either is unmeasured, and `Nearly` where the length disagrees but
  the title agrees by some `Spelling` (`the_only_take`), a code the file carries under a title it also
  carries being evidence of the recording, not of which take
  (`an_isrcs_only_take_is_nearly_the_file_where_the_title_agrees_and_the_length_does_not` in
  `enrich.rs`); several are narrowed to those within `RECORDING_MAY_DIFFER_BY` (5 s) of the file, then
  the closest, `Exactly`; a file with no measured length cannot tell them apart, so it takes the first
  as `Nearly`, the recording named and no title the file carried renamed. A `MUSICBRAINZ_TRACKID` is the recording id and asked for directly. A search
  is `find_recording` with the title, the length and what `asked_with` answers — the track's artist and
  its `artists.mbid`, or where the file names none the album owner's name and mbid — and the album's
  billed title as `release` only where neither is known; it is weighed by `matches_a_recording`:
  `STRICT_SCORE`, a title agreeing by some `Spelling`, a credit agreeing through `same_credit` with
  whatever was asked, and a length inside the same five seconds. It is refused before the request where
  `tagged_title` is missing, a title that is only the file's name being the one thing a text search must
  not be handed alone — **unless the file vouches for the rest**: `named_enough_by_its_file` lets a
  plain stem be asked where the file's tags name an artist, its length is measured and the stem
  `names_something` — three letters or more (`LETTERS_A_FILE_NAME_HOLDS_AT_LEAST`) once digits and
  punctuation are off, and not one of `PLACEHOLDER_NAMES` — so *Track 07*, *Audio_03* and *1* ask
  nothing while *One of These Days.wav* by a tagged artist is searched, still having to agree on title,
  credit and length to land, `Nearly`
  (`a_file_named_like_a_song_by_an_artist_it_names_is_searched_for_by_its_file_name`). It is refused too
  where no artist, owner or album is known, a bare title naming nothing
  (`a_track_that_names_no_artist_is_never_searched_for` claims both halves), and
  `a_track_naming_no_artist_under_an_owned_album_is_searched_with_the_owner_and_lands` and
  `a_track_naming_no_artist_under_an_unowned_album_is_searched_with_its_release` are what the two
  fallbacks buy: a file naming no artist under an album naming one is asked as that artist's, and one
  under an unowned album by the album's name.
- **A search answer says where the recording sits, so nothing is asked twice.** `RecordingMatch`
  carries the credit and the `RecordingRelease`s the search named beside score, title and length, and
  `RecordingMatch::into_recording` is what `take_match` lands — through `told_where_it_sits`, the ISRC
  route's rule, so the `/recording` lookup is made only where the answer named no release — or, the ISRC
  route's case, named several and no kind between them: MusicBrainz's `/isrc` lookup refuses
  `release-groups` among its includes, so its releases arrive untyped, and `needs_its_releases_told`
  asks the recording whole wherever `meant_release` would otherwise choose between them by date alone. A
  recording on one release costs nothing more. MusicBrainz's search index carries a recording's releases
  in full, placing the match on each by the medium's `track-offset` where the document gives no
  `position`; it carries the `isrcs` too, so `RecordingMatch` reaches `land_recording` with the code and
  `coalesce(isrc, ?8)` fills the column. The trade come good: one request a track rather than two, on
  the route a library of loose files spends nearly all its pass in, and the code an *identifier* route
  would write arrives anyway. A recording registered under no code writes none — a different answer from
  not having asked.
- **What a lookup may overwrite is `Certainty`, a type rather than a rule each caller remembers.**
  `Exactly` is a recording id the *file itself* named, or a named ISRC whose take is as long as the file;
  `Nearly` is a text search, a fingerprint, an ISRC naming several takes of a file with no length, or an ISRC whose only take is another length under the same
  title. `land_recording` reads it in one `CASE` per column: a name is filled wherever the file named
  none — `tagged_title IS NULL`, `tagged_artist IS NULL` — whatever the certainty, and a name the file
  *did* carry is corrected only under `Exactly`. So a lookup tidies *one of these days* into *One of
  These Days* on an identifier a tagger wrote, and never renames a track on a score
  (`an_exact_identification_corrects_a_title_the_file_carried`,
  `a_search_match_leaves_the_title_the_file_carried_and_writes_the_identifiers_alone`). The
  `artist_id` the track is listed under follows the same `CASE` as the name it is billed under, so a
  fingerprint at the strict score on a file naming another artist neither renames nor refiles it
  (`a_near_match_credited_to_another_artist_leaves_the_track_filed_under_its_own`). Everything else
  it writes fills and never replaces — `track_number` and `disc_number` from the recording's seat on the
  release, `mbid` and `isrc`, all `coalesce`d — except `release_title`, the release the route chose, and
  `artist_id`, repointed through `store::artist_named_in` so the row is billed to the artist the catalog
  holds under that fold. `asked` and `answered` are stamped in the same statement, whose `RETURNING` is
  what `EnrichStats::named` counts a moved name off and `store::index_row` rewrites `tracks_fts` from, so
  a corrected title is searchable at once.
- **A track naming its release lands the album its own pass never reached.** `best_release` is which
  of a recording's releases the row is filed under: the album's `albums.mbid` where it has one, then a
  release whose `folded_title` is the album's, then `elsewhere::meant_release`. Where the album was never
  answered (`TrackToAsk::album_answered` false), `take_recording` goes on to `land_release` on that id,
  so identifying one track of an untagged album lands the whole release and pairs every row in the same
  turn. Where the album *has* answered, the track stops at its own row, the album's identification being
  the more considered and a recording's idea of its pressing not.
- **Which release a held track is on is the listener's to say, over whatever the rule chose.**
  `Library::recording_of` reads the recording a track is identified as — its `tracks.mbid`, from tags,
  a lookup or a name taken from its audio — and asks the reference for it whole, so its releases arrive
  with their kinds; `in_the_order_worth_offering` lists them as `meant_release` weighs them, the order a
  found song's releases are offered in. `Library::place_on` lands the recording with the chosen release
  under `Certainty::Nearly`, so the release title and a position the file never gave are written and no
  name the file carried is touched; where the track is under an album, the album is then given that
  release through `take_pressing` (the album card's gesture), a folder's tracks being on the release the
  folder is, and what the rule seated is replaced rather than weighed against. A release the recording is
  not on answers `false` and writes nothing. The track's menu offers it as *Place on a release…* wherever
  the reference can be reached
  (`a_held_track_is_placed_on_the_release_the_listener_chooses_rather_than_the_one_a_rule_would`).
- **`fingerprint.rs` is the seam a recogniser fills.** `Fingerprints` answers a `Printed` — `Nothing`,
  or `Recognised` with `RecordingMatch`es — for a `Sounded`: the location, the span, the length, what
  the *file* said its title and artist were (not what the catalog settled on) and the `Chromaprint` the
  study took, so no printer decodes. `resonate-online`'s `AcoustId` is the one this build registers,
  where an `acoustid-key` is set; `analysis.md` has the studies the print comes from and how a
  recognition is weighed. `NoFingerprints` is the stub, registered under `unprinted`;
  `Fingerprinters::none()` holds it alone, `and` registers one per name as `Providers::and` and
  `Lyricists::and` do, and `has_a_source` asks whether anything real is behind it. `Library::enrich`
  takes one beside the `Reference`, and `Fingerprinters::recognise` asks each printer in turn for the
  first non-empty answer, answering a `Recognition` saying whether any refused, which the pass counts in
  `refused` and carries on past. **A fingerprint is weighed on its score alone**: `recognised` takes the
  top match at `STRICT_SCORE` and asks nothing of title or artist, the audio being the evidence and a
  file worth fingerprinting exactly one whose name is not.
- **A collaboration is listed under every artist it credits, never as an artist of its own.**
  `track_credits` — a migration step — holds each member of a track's credit, and
  `credits::credit_the_members` rebuilds it from `tracks.artist` whenever the orphans are swept:
  `members_of` splits on the joins a credit is written with (`JOINS`: `&`, `and`, a comma, a semicolon,
  `/`, `+`, `x`, `×`, `with`, `feat.`, `ft.`, `featuring`, `vs.` and the dotless `feat`, `ft`, `vs`,
  each space-delimited) and takes the split **only where every part names an artist the catalog
  holds**, so *Adam Skorupa & Krzysztof Wierzynkiewicz* becomes the two composers while *Simon &
  Garfunkel*, whose halves name nobody, stays one artist. A split track's `artist_id` is its first member
  unless it already names one of them, the text stays the file's credit, and the row the whole credit was
  filed under is swept once nothing names it. The artist scope, `artist_albums`, `artist_tracks`,
  `WHAT_AN_ARTIST_HOLDS` and `BY_OR_HOLDING_THE_ARTIST` read a credit beside `artist_id`, the sweep keeps
  an artist a credit names, and `take_over_artist` carries its credits across a merge. The members come
  from two places: a landed recording makes a row for *every* artist it credits, not only the first, so
  the split has names to find; and where an artist's own lookup lands nothing and its name splits,
  `Pass::bill_the_members` searches each unheld member under the same exact-name rule and
  `Library::bill_an_artist` makes a row for each found, which the pass then asks about like any artist it
  brought in. Before, the Witcher 2 score's tracks were split between *Adam Skorupa*, where a recording
  had been identified, and a row for the whole credit, where none had, neither composer's page holding
  the other half. `a_collaboration_is_listed_under_each_artist_it_credits_and_not_as_one_of_its_own`,
  `a_name_whose_halves_name_nobody_held_is_one_artist` and
  `a_collaboration_the_reference_cannot_name_is_asked_about_one_member_at_a_time` are the claims.
- **A credit names an artist the catalog may hold, and it is identified rather than asked.**
  `Pass::credits` walks the `Credit`s of a landed release or, through `take_group`, a landed release
  group: where one carries an mbid and `Library::artist_named` finds the folded name,
  `write_artist_mbid` fills an empty `artists.mbid`, so the following artist ask reads the row afresh,
  asks for the profile by id and spends no search on a name the release settled. The lookup fold must be
  the column's: `artist_named` keyed on `name.to_lowercase()` where `artists.key` is
  `folded_letters(name)`, so every marked spelling missed silently and *Marcin Przybyłowicz* was searched
  by name right after the release named him by id.
  `an_artist_is_found_by_the_fold_of_a_credit_name_however_it_is_spelled` holds the two together.
- **An artist's discography is kept once its profile lands, and what the catalog is short of is read off
  it.** `Pass::artist` calls `discography` after `land_artist`: `release_groups_of` is asked for every
  release group the artist is credited on, `worth_keeping` keeps those whose primary type is one of
  `KEPT_KINDS` — `Album`, `EP`, `Single` — and whose secondary types are none or `Soundtrack` alone, so
  a live album, compilation, remix or untyped group is left out
  (`an_album_an_ep_and_a_single_are_worth_keeping_and_a_soundtrack_is_still_one`,
  `a_live_album_a_compilation_a_remix_and_an_unkinded_group_are_left_out` in `enrich.rs`); and
  `land_artist_releases` deletes and reinserts `artist_releases` — `(artist_id, mbid)` with title, kind,
  first release date and the `folded` haystack `spelt_out` writes (those three through `folded_letters`)
  — answering how many rows it wrote, which `EnrichStats::releases_found` counts. A release is *unheld*
  where no album's `release_group` is its mbid — `unheld_by_any_album!` in `db.rs`, read off
  `albums_by_release_group` — so a landed pressing, or a group landed thin, takes its group out of the
  list. **A single is held wherever its song is**: its title is kept as `artist_releases.song`, the
  words of it — `store::words_of`, the letter fold with every run of non-letters-or-digits one space —
  and the macro weighs it against the words of every title the artist's tracks carry through `words_of`,
  registered by `schema::configure` on each connection as a deterministic SQLite function, so *Fearless*
  on the album holds the *Fearless* single however either is punctuated, and only a single whose song
  the catalog has nowhere is listed as not held
  (`a_single_is_not_held_only_where_its_song_is_not_and_a_discography_says_what_it_did_not_read`, which
  reads the rest as well). **What was not read is said rather than logged.**
  `Reference::release_groups_of` answers a `Discography` — the releases and how many more the service
  credits than the browse's cap read — and `land_artist_releases` keeps that as
  `artists.releases_unread`, and where the browse stopped as `artists.releases_read_to` (a `MIGRATIONS`
  step setting the thousand the cap always was on every artist a read fell short on). **The rest is read
  when the listener asks**: `Library::read_the_rest_of` asks for the next groups from that offset and
  lands them `Discographed::Further` — beside, not over, what the first read kept — moving the unread
  count and offset on, so the artist page's *Read the rest* and `resonate missing --artist <NAME>
  --read-the-rest` each read a thousand more and a discography read to its end is not asked again.
  A name the catalog holds no artist under is `Error::NoSuchArtist` from `resonate missing --artist`,
  with or without `--read-the-rest`, exiting 1, where it read nothing and then said nothing of that
  artist was missing.
  `ArtistDetail::releases_unread` carries it to the artist page's *N releases not held* button and
  `resonate missing --artist` prints it; `Library::unheld_releases` lists them under a cap by artist and
  first release date, `ArtistDetail::releases_unheld` counts one artist's, and
  `Library::missing_counted` answers a `Missing` — those beside the release rows with no `track_id`,
  which `Library::missing_tracks` lists in album order with each row's `WantId`. **A discography that
  did not arrive is asked for again within the hour**: `land_artist` has already stamped the profile
  `answered`, so a refused `release_groups_of` stamps the artist `Fruitless::Refused` beside it, and
  an unreachable or ending one does the same before the pass ends — the refusal count making the
  answered row due after `REFUSED_AGAIN_AFTER`, where it once sat with an empty discography for
  `REFRESH_AFTER` (`a_discography_refused_as_its_artist_landed_is_asked_for_again_within_the_hour`).
  `an_artists_discography_is_kept_once_its_profile_lands_and_the_catalog_says_what_it_does_not_hold` and
  `the_rows_an_album_is_short_of_are_listed_with_their_wants` are the readers' claims.
- **What the catalog is short of can be dismissed, and a dismissal outlives the release it was made
  against.** `Library::dismiss_missing` and `dismiss_release` take a missing row or an unheld release
  off every reader of what is missing — `missing_tracks`, `unheld_releases`, `missing_counted`,
  `unheld_matching` and `ArtistDetail::releases_unheld` — through the `held_or_wanted!` and
  `unheld_by_any_album!` predicates they already share. A dismissal is its own table, because what it
  names is rewritten under it: `land_release` deletes and reinserts an album's `release_tracks` on
  every refresh and `land_artist_releases` an artist's `artist_releases`, so a flag on either row died
  with the next lookup. `dismissed_missing` holds the album, the disc, the row's position on it and
  its `folded` — title, artist and release title, stable across a refresh of the same release and not
  across another track — the position since a later `MIGRATIONS` step, so dismissing one *Interlude*
  leaves the disc's other *Interlude* listed
  (`dismissing_one_of_two_missing_rows_of_a_title_leaves_the_other_listed`); the step keys a dismissal
  held before at every position its title stood at, hiding what it hid — and `dismissed_releases` the artist and the release group's MBID, both cascading with their album or
  artist (a `MIGRATIONS` step). **A want and a dismissal undo each other**: dismissing a row withdraws
  its want, and `want_in` clears a dismissal of the row it wants, so a row is never both asked for and
  hidden. **A want asked for again is due at once**: `want_in` puts back to nothing the `tried` and
  `misses` of a want already standing and not yet offered, so pressing Want, a found song or a whole
  album again — every caller is a gesture — sends the next poll for it rather than waiting out a
  retry or staying given up (`providers.md`)
  (`a_want_asked_for_again_is_due_at_once_however_lately_it_was_tried`).
  `Library::dismissed` counts what is dismissed and still missing, and `bring_back_dismissed`
  empties both tables, answering what it brought back.
  `a_dismissed_missing_row_leaves_the_listing_through_a_refresh_until_wanted_or_brought_back` and
  `a_dismissed_unheld_release_leaves_the_listing_and_the_artists_count` are the claims.
- **What the catalog is short of is narrowed by the words typed, each half answering with what it
  has.** All three readers take an `Option<&str>`, so the pane's list, counts and sidebar figure narrow
  together. A missing track belongs to a *held* album, so it is narrowed through
  `narrowed_onto("album_id", "a.id")` — the grouped `tracks_fts` join the albums pane takes — so the
  whole grammar reaches it and typing an album, its owner or a track it holds brings up what it lacks.
  An unheld release is in no catalog and has only its own row, so `unheld_holding` writes one
  `r.folded LIKE ? OR ar.key LIKE ?` per lone search word, each folded through `folded_letters` as
  `artists.key` is — how a title, kind, year and artist's name all narrow it, and how *przybylowicz*
  finds *Przybyłowicz*. Only lone words are read: a `plays:>20` term says nothing about a release nobody
  holds, and `playlist::words_of` is where that reading already lived.
  `what_the_catalog_is_short_of_is_narrowed_by_the_words_typed` and
  `an_unheld_release_is_found_by_a_name_spelt_either_way` are the claims.
- **A cover says where it came from, the file's wins, and the archive is asked until it has
  answered.** `albums.cover_source` is `CoverSource::File` or `Archive` (and `Vault`, `vault.md`).
  `land_archive_cover` writes `cover_art`, `cover_format` and `cover_source` under
  `WHERE cover_art IS NULL`, so a file's picture is never overwritten — except a thumbnail the archive's
  `betters` — and `Pass::album` asks for a cover in the pass that landed the release, where `has_cover`
  is false or `Library::covered_by_a_thumbnail` reads the held picture's header and finds it under
  `A_THUMBNAIL_BELOW`; such an archive cover is kept through a rescan and is what `resonate tag` writes
  over the thumbnail in the file. A vault-held cover is not weighed, its bytes being JXL.
  `albums.cover_asked` — the first `MIGRATIONS` step — is stamped once the archive has *answered*, with
  a picture or with none, never where the fetch failed, was refused or was cut off by a pass ending
  under it; `Pass::look_again_for_covers` asks at the end of every run for each album holding a release
  or group, no picture and no stamp, passing over those `Pass::covered` says the run asked. Before, one
  failed fetch left an album with its release and no sleeve for good while Discord drew the same
  release's cover off its id. A cover the archive answered it lacks is not asked again within
  `COVERS_ASKED_AGAIN_AFTER` (30 days) unless the release is refreshed or
  `Library::ask_again_for_covers` clears the stamps (the settings pane's *Look for missing covers*);
  past it `albums_wanting_a_cover` reads the stamp as none, somebody perhaps having uploaded the sleeve,
  and the answer stamps it for another month either way — at the archive's pace one request a month per
  uncovered album, a library of hundreds spending a few minutes
  (`a_cover_the_archive_said_it_lacked_a_month_ago_is_asked_for_again`). **A release MusicBrainz says
  has no front cover is not asked of the archive for one.** `Release::has_front_cover` is the
  release's `cover-art-archive.front`, and `land_release` keeps it as `albums.front_cover` (a
  `MIGRATIONS` step; `NULL` for a release landed before it, read as perhaps); `Pass::cover` asks the
  group's cover instead where the release names a group, and nothing where it names none, and
  `albums_wanting_a_cover` offers such an album as `CoverFrom::Group` or not at all — so the archive
  is not asked every month about a sleeve MusicBrainz already says is not there, and the release's
  monthly refresh is what notices one uploaded since
  (`a_release_said_to_have_no_front_cover_is_pictured_by_its_group_and_never_by_itself`,
  `a_release_said_to_have_no_front_cover_and_no_group_is_never_asked_for_one`). An album landed as its group
  asks `group_cover` under the same two conditions, so what the group route lacks is a release's rows,
  never a sleeve. A portrait is the same shape: `land_portrait` writes under `portrait IS NULL`, asked
  only where the profile's links name a picture and none is held.
- **A portrait that missed is looked for again from the links already held.** `land_artist` stamps
  `answered` and zeroes `asks` *before* the picture is asked, and the picture is fetched on a thread
  whose outcome feeds no column — so a fetch failing on a bad day waited out the thirty-day
  `REFRESH_AFTER` with nothing recording it. `Library::artists_wanting_a_portrait` reads every artist
  with `portrait IS NULL` whose stored `artist_links` name a picture, and
  `Pass::look_again_for_portraits` asks for each at run's end — no MusicBrainz request, the links
  having been stored since enrichment landed and never read back — and `Pass::pictured` keeps it from
  asking twice, an artist the pass already asked about not asked again by the sweep. Without that set
  the pictures, fetched on their own threads, would race the sweep's read of `portrait IS NULL`.
  **A miss is remembered as a cover's is**: `artists.portrait_asked` (a `MIGRATIONS` step) is stamped
  once the picture sources *answered*, with a picture or with none, never where the fetch failed, and
  the sweep passes over an artist stamped within `PORTRAITS_ASKED_AGAIN_AFTER` (30 days) — so an
  artist no source holds a picture of is asked about once a month rather than on every pass
  (`a_portrait_the_reference_answered_it_lacks_is_not_looked_for_again_the_next_pass`), while one
  whose fetch was refused is still looked for again
  (`a_portrait_that_never_landed_is_looked_for_again_without_asking_the_reference_twice`).
- **A link is a relation, a service and a URL, both names read off the reference's own words.**
  `Relation::of_type` matches the exact type string MusicBrainz writes — `streaming`, `free
  streaming`, `official homepage`, `image` and the rest of `Relation::TYPES` — else `Relation::Other`;
  `Service::of_url` reads the host of an `http` or `https` URL alone — any other scheme, and an
  authority carrying a backslash, which a browser reads as the path's start, is `Other` — lowercases it, drops a leading `www.`, and matches it or a parent
  domain against `Service::HOSTS`, `x.com` and `twitter.com` both `Twitter` and any host with an
  `amazon` label `AmazonMusic`, else `Service::Other`, keeping the URL. `Service::name` is the lowercase
  figure a pane draws. All three live in `resonate-core`'s `link.rs` (what was the library's `Provider`
  enum, renamed so *provider* means only a plugin that obtains media); the tables still name the column
  `provider`, and `store::service_code` and `store::service_of` are its encoding.
- **A want is a release row the catalog holds no file for, and a provider fills one.** `wants` is one
  row per `release_track_id`, so `Library::want` refuses an unheld row with
  `Error::UnknownReleaseTrack` and answers the same `WantId` twice for the same row; `unwant` drops it and
  `wants` reads them all, newest first, each a `Want` carrying the album's title, the row's title and
  artist, recording and track MBIDs, the album's release MBID, ISRC, length, disc and position, its own
  links and the release's. An unparsable identifier reads as absent through `store::mbid_in` and
  `store::isrc_in`, the readers a tag goes through. `Want::identity` turns a want into the
  `resonate_providers::Identity` a provider is handed, and `supply.rs` is the pass: `Library::poll`
  walks the wants `Want::due_at` says are due (every unheld one under `PollOptions::every_want`,
  the retries and the give-up in `providers.md`) on a
  `resonate-poll` thread, asks `Providers::first`, and lands what it answers. A `Delivery::File` goes
  through `Vault::keep` and a `Delivery::Stream` through `Vault::keep_delivered`; either kept writes the
  `vault_objects` row through `note_delivered` with `taken_from` the file's URI or `<provider>:<key>`,
  and `wants.offered` is the vault object's URI. With no vault a file's own URI is the offer and a stream
  is dropped, having nowhere to be kept; that, a vault refusal, a vault failure, a delivery whose
  length disagrees with the release row's and one landing after the row was held by another each count
  `unkept` and offer nothing, so `offered` never names what cannot be opened. `note_tried` keeps an
  earlier offer where the new pass found none, and is not called where a provider refused, ran late or
  was passed over as away, or where a registry narrowed to one provider found nothing, the want
  staying due.
  `forget_delivered` clears the offer and remembers what was forgotten in `forgotten_deliveries`, which
  `land_release`'s `wants_under` and `want_again` carry onto the want's new row with the want itself.
  `providers.md` has the rest.
- **A lyric fetched once is kept, and so is a miss; what is kept only gets better.** `lyrics_kept` is
  keyed by `(path, span_start)` as `tracks` is, so a cue row keeps its words apart from the file's;
  `text` is `NULL` for a remembered miss and `taken` says when last asked. A kept row, and a
  `lyrics_refused` one, goes with its track: `store::ORPHANS` deletes those no `tracks` row names, and
  the `lyrics_forget_a_changed_file` trigger deletes a path's when its `file_size`, `modified` or
  `span_frames` moves — as `track_studies` is forgotten — so a file replaced at the same path asks again
  rather than showing the old song's words
  (`kept_lyrics_go_with_a_file_that_goes_or_is_replaced_and_stay_with_one_left_alone`). A kept row is a `KeptLyrics`
  holding an `Option<LyricText>` — the text, `synced`, and the `lyricsfile` a migration step added, the
  Lyricsfile document kept only where it says more than its lines (a set timed word by word or sung by
  two overlapping voices). Its `LyricDetail` is `Plain`, `Lines` or `Lyricsfile` in that order, and
  `sung::keep` never trades a richer set for a plainer one or a miss: it keeps the richer of held and
  told, stamps `taken` either way and answers whether what it holds improved (the enrichment's `lyrics`
  count). `KeptLyrics::is_due` is the one rule both askers follow: a miss is due after
  `MISSED_AGAIN_AFTER` (a week), a set short of a Lyricsfile after `BETTERED_AFTER` (a month) — a synced
  or word-timed set perhaps written since — and a Lyricsfile never. `Library::kept_lyrics` and
  `keep_lyrics` take the `MediaLocation` and `Option<FrameSpan>` and refuse a non-local location through
  `playlist::local_path`, a row here being a path; the search index is written from `text`, so a
  Lyricsfile's YAML is never what `lyrics:` reaches. The catalog holds words it never parses: the
  reading is the online crate's.
- **The lookup asks for every track's words beside the pass.** `Reference::lyrics` takes a
  `LyricsAsked` — title, artist or the album's owner, album and length, read off the row when asked so
  a name the pass just corrected is the one sent — and answers an `Option<LyricText>`, `None` for an
  instrumental or a song the service lacks (`Online` answers it through the same `lrclib::told` the
  window's `Lrclib` asks). `Library::lyrics_to_ask` is every row with no kept row or a due one — all
  under `refresh` — cut to `at_most` like the other queues, and `EnrichOptions::lyrics` turns the walk
  off (the `fetch-lyrics` key). `Verses` is one thread, as `Pictures` is two: LRCLIB is paced apart from
  MusicBrainz, so the words cost the pass nothing but the wait at its end. A refusal is counted and the
  row left unkept to be asked again, but not on the very next pass: `lyrics_refused` (a `MIGRATIONS`
  step, keyed as `lyrics_kept` is) holds when the row was last refused and how many times in a row,
  `sung::note_refused` steps it, `sung::keep` removes it, and `lyrics_to_ask` passes over a row
  refused within `REFUSED_AGAIN_AFTER` doubled per refusal under the same `doubled` cap the lookup's
  rows wait by — a table of its own rather than a `lyrics_kept` row, whose `NULL` text already means a
  miss the window reads (`a_lyric_the_service_refused_is_not_asked_for_again_on_the_next_pass`). An
  unreachable service ends the walk, not the pass. A delivered row
  is a row like any other here and has its words fetched too. `Lyricists::find` walking every provider
  for the most finely timed answer is why a file's plain words give way to a synced set fetched for it
  (`lyrics.md`).
- **The panes read what landed through seven calls, and two counts ride on the listings.**
  `Library::release_of` answers a `ReleaseDetail` — release columns, `CoverSource`, the two clocks and
  the album's links; `release_tracks` the `HeldReleaseTrack`s with each row's links and paired
  `TrackId`; `artist_detail` an `ArtistDetail` with genres, links and `releases_unheld`; `portrait` the
  `CoverArt`, sniffed where the stored format code is missing; and `missing_tracks`, `unheld_releases`
  and `missing_counted` as the discography bullet says, the first two under an `Option<usize>` cap.
  `Album::missing` counts release rows whose `track_id` is `NULL`, and `Album::mbid` and `Artist::mbid`
  are read off the same listing rows, so an album grid says which albums are short a track without a
  second read; `Artist::has_portrait` lets a list draw a placeholder without asking for bytes.

## Writing the catalog back into the files

What the catalog was told is written back through a seam of its own, and this build writes only
what it will read back. `resonate-codec` carries the writing beside the reading: `TagField` is the
vocabulary of a writable field, `TagEdit` one of them with its value, `Writing` the run of edits and
the optional front cover riding into one call, `TagSink` the seam over `TagSource`, and `FileTags`
the whole of what is behind it — lofty writes and symphonia still reads, because what a write is
weighed against must be what the rest of this build sees. A format whose tags this build would not
read back is refused rather than written: AAC, AIFF, Monkey's Audio, FLAC, MP3, MP4, Ogg Vorbis,
Opus, WAV and WavPack are what `FileTags::writes` answers for — a WAV because `riff.rs` reads the
`id3 ` chunk lofty writes into, which symphonia's reader skips — and `.caf`, `.mka`, `.oga` and the
two DSD containers, which lofty cannot write, are passed over. `Library::retag` is the pass behind
`resonate tag`, a preview until `--apply`, as `organise` is; the settings pane's *Tagging* group
under Library is the window's way in, with the same preview-then-arm shape *Organising* has.

**Every field the `TagSet` names a writer can reach is a `TagField`**, thirty-six of them: the names,
numbers, date, label and identifiers, and genre, the seven credits, comment, BPM, compilation,
grouping, copyright and the four ReplayGain values. `TagField::read` spells each as it is written, so
a write is weighed against the file in the same text: a compilation is `1`, a gain
`spelled_gain`'s `+x.xx dB` and a peak `spelled_peak`'s six places. `TagField::key_in` is the lofty
key a field takes in a tag kind — BPM is `Bpm` in a Vorbis comment, `IntegerBpm` in ID3 and MP4 —
and answers `None` where lofty 0.25 has no key for it there: an APE tag's BPM and an ID3 performer.
Those two are `TagField::unkeyed_in` and written into the concrete tag `saved` converts the generic
one into, as the play count is: an APE `BPM` item (`beat`), and an ID3v2.4 `TMCL` musician credits
list (`credit_performers`), one pair a name the value lists apart with `; `, the role blank as
Picard writes a performer without an instrument — and a name the frame already credited keeping its
instrument, so a *guitar* another tagger wrote survives an edit that keeps the guitarist. The
reader takes every `TMCL` pair's name as a performer whatever its role (`id3_list`). A name is
append-only once shipped: the undo record keeps fields by `TagField::as_str`.

- **A guess is never written, so a name is written only where a lookup answered for the row holding
  it.** `tracks.title` falls back to the stem where the tags named nothing and `albums.title` to
  whatever grouped the folder, so writing either back would put this build's own reading into the
  file as though a reference said it. `offered` therefore gates the track name and artist on
  `tracks.answered`, the album name and its two totals on `albums.answered` — offering
  `albums.release_title`, not `albums.title`, since only a landed release names a pressing and an
  album settled as its group alone names none — and the album artist and its id on the *artist's* own
  `answered`, `land_release` never repointing `albums.artist_id`, the credit there being the scan's
  attribution. An identifier is never a guess: `tracks.mbid`, `release_track_mbid`, `artist_mbid`,
  `isrc`, `albums.mbid`, `release_group` and what the tags declared about the pressing are offered
  whatever answered, each being the file's own or a strict match's.
- **What is written is the difference, so a file already saying it is left alone.** `wanted` reads
  the file's own `TagSet` through the build's `TagSource` and keeps only the fields whose value
  differs, so a run over an already-written library costs one probe a file and no writes, and
  `RetagStats::unchanged` counts them. A blank value names nothing and is dropped before comparing, as
  `tags::given` does on the way in.
- **A picture is the catalog's to give and the file's to keep, riding into the same write.**
  `Writing` carries the edits and an optional front cover, so a file wanting both costs one
  `save_to_path`, not two rewrites of its whole tag; `offered_picture` fills the second half, the
  mirror of `albums.cover_source`'s rule — the album's cover is offered where the file's own read says
  it carries none, or carries a thumbnail the album's cover `betters` — so an archive cover reaches a
  coverless file and a file with its own picture keeps it unless it is a ripper's thumbnail. What a
  write replaced is noted with the run, so putting the run back writes the thumbnail back
  (`a_thumbnail_a_ripper_embedded_gives_way_to_a_cover_twice_its_size`). The plan reads each file
  **once**, through `TagSource::read` — under `Picturing::Copied` where the album holds a cover to
  weigh against, `Whether` where it holds none — so fields and picture come off one open. `Sleeve`
  holds one album's bytes at a time while `TRACKS_TO_TAG` reads in path order, so tracks from one
  folder share one read of the blob. `written` reads the file back once too, under
  `Picturing::Copied` where a picture went in. A file under no album is offered nothing. What is
  written is a `PictureType::CoverFront` under the format's own media type, *replacing* every picture
  the reader would take as the cover — `writing::uncovered` takes out the front cover, `Other` and an
  untyped one, the set `probe::choose` draws from — so a file cannot collect two. lofty reads every
  MP4 `covr` image as `Other` where symphonia reads it as the front cover, so removing the front
  cover alone left an m4a's cover in place and a new one behind it, never drawn
  (`a_cover_taken_away_is_gone_and_a_cover_written_replaces_the_one_there`). `written` weighs the picture read back
  against the bytes sent, as each field, so a container quietly dropping one is
  `Unwritten::Unconfirmed` and the catalog is not moved. `RetagStats::pictures` counts them apart from
  `fields`, a picture not being a field and a write of one alone still a write.
- **A favourite is written as this build's own rating, and the plays as the count every player
  reads, beside anybody else's.** `TrackToTag::popularity` is the row's favourite and play count, and
  `Writing::popularity` rides them into the same write. The favourite is lofty's generic popularimeter
  under the name `resonate_codec::RATED_BY` — `resonate`, the software, nothing about the listener —
  five stars, and for a non-favourite this build's rating taken away. It lands wherever lofty maps one:
  an ID3v2 `POPM` (keeping the counter beside it), a Vorbis `RATING:resonate`, MP4's `rate` and RIFF's
  `IRTD`. An APE tag has no popularimeter, so a WavPack or Monkey's Audio carries its favourite as
  FMPS's `FMPS_RATING` of `1.0`, taken away with it. A rating another player wrote under its own name
  stays, and where a format names nobody — MP4, RIFF and APE hold one rating — that one is ours.
  **The plays are written whether or not the row is a favourite**, under the FMPS name each tag uses —
  `FMPS_PLAYCOUNT` in a Vorbis comment and APE tag, a `TXXX` of `FMPS_PlayCount` in ID3v2 and
  `----:com.apple.iTunes:FMPS_Playcount` in MP4 — and a count of nothing is taken away, not written.
  lofty's generic `Tag` drops a name with no `ItemKey`, so `counted.rs` converts the generic tag into
  the format's own — `VorbisComments`, `ApeTag`, `Ilst` or `Id3v2Tag`, the conversion lofty's save
  makes — sets the count there, saves that, and reads the count back off the same concrete tag. What
  the file holds is read through `TagSink::rated`, not the `TagSet`, symphonia reading `POPM` and
  ignoring a Vorbis rating: `Rated::Unrated` and `Rated::Favourite` each carry the count the tag keeps
  — an MP3 written before counts were, whose only count is our `POPM`'s, reads that — and
  `Rated::differs_from` weighs the favourite always and the plays wherever the tag keeps a count, so a
  play counted since the last run rewrites the count and a RIFF `INFO` list, keeping none, is weighed
  on the favourite alone. The undo record keeps both in one integer: a favourite's plays as they are,
  an unfavoured row's as their negation less one — which the `-1` an older build wrote for *unrated*
  already reads as. `RetagStats::ratings` counts them;
  `a_favourite_and_its_plays_are_written_into_the_file_and_taken_away_again`,
  `a_play_count_is_written_into_a_file_nobody_marked_a_favourite` and
  `a_play_count_and_a_favourite_read_back_out_of_every_tag_this_build_writes` are the claims.
- **A field cleared is cleared from every tag the file holds.** lofty edits one tag, the file's
  primary — a WAV's `id3 ` chunk, an MP3's ID3v2 — while the reader takes a name from whichever tag
  still says it, so a title taken out of a WAV's ID3 chunk came back from its `INFO` list on the next
  read, and an MP3's from its ID3v1. `writing::cleared_elsewhere` copies every other tag holding a
  field in `Writing::taken`, takes the field out and saves each into the same staged copy after the
  primary, one settle for the lot (`a_field_cleared_from_a_wave_file_is_gone_from_its_info_list_too`).
  A field *set* needs no such pass: the primary tag outranks the rest wherever the reader weighs them
  (`audio.md`).
- **An ID3v2.3 tag stays v2.3.** lofty writes v2.4 unless told, and a player reading only v2.3
  loses a tag upgraded under it, so `FileTags::write` asks `Counted::holds_id3v2_3` — the file's own
  ID3v2 tag read whole, for the four kinds that carry one — and saves every tag through
  `WriteOptions::use_id3v23` where it was, lofty folding the v2.4 frames back
  (`an_id3v2_3_tag_is_written_back_as_the_version_it_was`). A file with no tag gets v2.4.
- **A write never touches its file until it is whole.** lofty's `save_to_path` splices a FLAC's
  metadata and shifts the audio behind it in place, so a full disc or a killed run left a truncated
  file. `FileTags::write` copies the file to a staged sibling — `.<stem>.<pid>-<n>.<ext>`
  (`staged_beside`), the extension kept since lofty reads the kind off it — writes the tags into the
  copy, `sync_all`s it, renames it over the file and syncs the folder, removing the copy wherever any
  of that failed. A copy a killed writer left is swept by the next write to that track:
  `sweep_what_a_dead_writer_staged` removes a sibling `staged_by` reads as this track's, staged by a
  pid that is neither ours nor under `/proc`
  (`a_copy_a_dead_writer_staged_beside_the_track_is_swept_and_no_other`); the scan never catalogs it
  meanwhile, a dot-name being passed over (below). **The copy lands only over the file it was taken
  from.** `Taken::of` asks `access(W_OK)` first, so a file the listener may not write is refused
  `PermissionDenied` where the copy and rename would have replaced it in a folder they can write
  (`a_file_nobody_may_write_is_refused_rather_than_replaced`), and holds the device, inode, length
  and modification time; `landed_through` weighs `Taken::still_stands` before the rename, so an edit
  another program made while the copy was written answers `Error::ChangedWhileWritten` and the copy
  is removed rather than renamed over that edit
  (`a_file_another_program_changed_meanwhile_no_longer_stands_as_taken`). A folder the listener may
  not create a file in does not refuse a file they may write: a clone or copy refused there falls to
  the edit in place, and where that will not hold it, `landed_from_elsewhere` stages the copy under
  the spool's folders (`/var/tmp`, or a `TMPDIR` set) and writes it back over the file through
  `written_back` (`a_file_in_a_folder_nothing_may_be_created_in_is_written_all_the_same`). The copy is a clone (`cloned_beside`, `FICLONE`) wherever the filesystem shares
  extents — btrfs, XFS — so there it costs the tag's bytes. **Where it cannot clone — ext4, tmpfs —
  an edit the tag's own room holds lands in the file itself** rather than copying gigabytes:
  `landed_in_place` has lofty write into an `Overlay`, the file seen through 4 KiB pages held in
  memory, and `Overlay::land` writes back only the pages that differ, then `sync_data`s — and only
  where the file keeps its length and under `MOST_BYTES_WRITTEN_IN_PLACE` (32 MiB) was touched, so
  audio that would move, or a tag at the end that would grow, is never rewritten where it stands and
  goes through a whole staged copy instead, the promise above kept. The window this opens is a crash
  during a write of a few pages of tag, never of audio. lofty 0.25 keeps a FLAC's padding block as it
  was and pads an ID3v2 tag with a fresh 1 KiB, so neither would ever keep its length: for those two,
  `Head` has lofty write into an in-memory copy of the tag's head (and 64 KiB past it, so the kind
  still probes), and `Head::absorbed` fits the result back into the head's old length — the FLAC's
  blocks with one padding block sized to the room left, the ID3v2 tag's frames zero-padded to its
  old size — or answers that it will not fit. An MP4's `free` atoms already absorb an edit
  (`an_edit_the_padding_holds_lands_in_the_file_itself_rather_than_a_copy`,
  `an_edit_a_leading_id3_tag_has_room_for_lands_in_the_file_itself`,
  `an_edit_that_moves_the_audio_is_left_to_a_whole_copy`). **What the rename would lose is
  kept.** A track reached through a symlink is written where the link points, staged beside the
  target, so the link stays a link. The staged copy is given the file's owner, mode and extended
  attributes (`carries_what_the_file_did` — `chown`, `set_permissions`, and every `listxattr` name,
  ACLs and SELinux labels among them, through `rustix`); where any of that is refused — a file owned
  by another user in a shared folder, a label only root may set — and wherever the file has a second
  name (`nlink` over one), the whole tagged copy is instead written back into the file's own inode
  (`written_back`) — an edit landed in place never needing it — which keeps all of it and both names
  at the price of a second copy and a window
  where the file is part rewritten, the synced staged copy standing beside it until the write-back is
  synced (`a_track_reached_through_a_link_is_written_where_the_link_points_and_stays_a_link`,
  `a_track_with_two_names_keeps_both_and_both_read_the_write`,
  `a_tracks_extended_attributes_survive_a_write`).
- **A row cut out of a shared file is never written to.** Twelve cue rows are twelve readings of one
  file with one set of tags between them, so a path holding more than one row — or one row carrying a
  span — is `Unwritten::Cut` and passed over whole: the reason `Library::track_played` keys a play on
  the pair, a cut being a row of its own everywhere but in the file.
- **The catalog follows the file, the file being what the next scan reads.** A write changes size and
  mtime, exactly what `Known::under` compares, so `files_retagged` writes both back and the next walk
  reads the row as unchanged. It writes `tagged_title` and `tagged_artist` too, only for names it
  actually wrote: those columns tell a retagging from an identification, so a corrected title written
  into the file and not recorded here reads as the tagger having moved it, takes the file's names back
  over the reference's and nulls `answered` — the whole library asked again on the next non-incremental
  scan (`a_rescan_reads_this_builds_own_write_as_the_names_it_already_knew`).
- **A write is confirmed by reading it back, and an unconfirmed one is not followed.** `written`
  re-reads the file through the `TagSource` after `TagSink::write` and weighs every edit against what
  comes back; a field not reading back leaves the row `Unwritten::Unconfirmed` and the catalog unmoved,
  so a format quirk shows as a refusal rather than a preview offering the same edit for ever —
  `FileTags::writes` refusing a format being the cheap half. **A WAV whose ID3v2 tag stands before its
  `RIFF` header has the tag moved into the RIFF on the way**: lofty does not recognise such a file, so
  `FileTags::write` first stages a copy that is the RIFF with the leading tag's bytes appended as an
  `id3 ` chunk and the header's size grown to hold it — whatever followed the RIFF still following — and
  edits and settles that copy in the file's place, so every field the tag carried is kept and the audio
  copied byte for byte
  (`a_wave_file_tagged_ahead_of_its_riff_header_has_the_tag_moved_into_a_chunk_and_written`).
- **`Library::retag` takes the `Walk` guard**, so a scan and a write-back cannot run at once: the pass
  rewrites the sizes and mtimes a scan's snapshot was taken against — `organise`'s hazard — and a second
  caller gets `Error::AlreadyWalking`. A vaulted row is passed over as `Unwritten::Vaulted` (`vault.md`).
- **The preview is the plan.** One `Retagging` is built then handed to the apply or not, so
  `resonate tag` and `resonate tag --apply` cannot disagree; a failing write moves out of
  `Retagging::writes` into `passed_over`, so what prints after an apply is what was done, not intended.
  The settings pane's *Tagging* group draws the same plan (`ui.md`), so command line and window are two
  presenters of one pass.
- **The last applied run can be put back.** Every write carries a `Held` — what each touched field read
  before, `None` where the file carried none, and the rating where it changes one — and the apply notes
  it in `retagged` and `retagged_fields` (a `MIGRATIONS` step) beside whether the write added the
  album's cover; the first page of a run writing anything clears what the previous run noted, so the
  record is the last run's alone. **The note is written ahead of the files**: `apply` notes every
  planned write of a page (`Library::retag_to_be_written`) before it touches one, then follows the
  catalog and forgets the notes of the writes that failed or were cancelled in one transaction
  (`files_retagged`). Written the other way round, a catalog error between the two left files changed
  with nothing to put them back by; now a note that cannot be written stops the run before any file is
  (`a_tag_run_the_catalog_cannot_note_writes_no_file`), and a follow that cannot be written leaves the
  record whole and sizes and mtimes the next scan reads as changed. The price is that a run whose every
  write fails still clears the previous run's record
  (`a_write_that_fails_is_not_noted_as_one_to_put_back`). **A write that landed but reads back
  otherwise keeps its note**: the file changed, so `Noted::keeps_a_note_of` holds `Unconfirmed` out of
  what is forgotten, and the catalog does not follow it, the next scan reading its new size
  (`a_write_that_lands_but_reads_back_otherwise_can_still_be_put_back`). `RetagOptions::undo` plans from that record instead of the catalog:
  each field read before is written back, one the run added is removed (`Writing::taken`, removing the
  key), an added cover is taken out (`Writing::unpictured`, through the same `uncovered`) and the rating put
  back, all handed to the same `apply`, which reads every file back, has the catalog follow
  `tagged_title` and `tagged_artist` as they now stand and notes what it replaced in turn, so putting
  the walk back writes the run again — only a cover is not rewritten, the walk back having nothing to
  put back but its absence. **A walk back cut short leaves the rest to the next one**: it clears
  nothing ahead, `Noted::walking_back` holds the record it read, a write that fails or reads back
  otherwise has its old note put back (`note_again`), and unless every noted file was put back
  (`retag::walked_back`) the files that were lose their notes, so the next walk back finishes the job
  rather than writing the run into what was already restored; the pictures no note names are swept
  either way (`a_tag_walk_back_cut_short_keeps_what_it_did_not_put_back_for_the_next_one`). `resonate tag --undo` previews it and `--undo --apply` writes it, and the
  *Tagging* group offers *Put the last run back* behind a second press wherever
  `Library::retag_walks_back` says a run is noted. A file gone since is passed over as unreadable, and
  one that moved is not found by its old path.
  `an_applied_tag_run_is_put_back_field_for_field_and_putting_it_back_again_writes_it_again` is the
  claim. **A replaced picture is kept once and held no longer than its page.** A cover that gives way
  to a better one is noted in `retagged_pictures`, `retagged.picture_id` naming it: `KeptPictures`
  weighs each against the last eight it kept, by pointer and then byte for byte, so the thumbnail
  twelve tracks of one album carried is one row, not twelve (a `MIGRATIONS` step folds a record kept
  per file before into the same table). `run` drops `Held::picture` from every write once its page is
  applied, so the summary a whole library's run returns holds none of the pictures it replaced
  (`a_thumbnail_a_ripper_embedded_gives_way_to_a_cover_twice_its_size`). The walk back reads the notes
  without their pictures and each picture by id as a file first asks for it, each read once
  (`PicturesPutBack`), rather than every row's copy up front
  (`a_picture_the_last_tag_run_kept_per_file_is_kept_once`).
- **The rows are read a page at a time, and a file is never split across two.** Planning a file needs
  its rows alone, so `retag::run` walks the catalog through `paged::Paging` — `ROWS_A_PAGE` (2 048) rows
  in path order past the last path handed out, the rows of the file a page ended in held back for the
  next, and the page doubled where one file's cue rows fill it — and plans, and under `--apply` writes
  and follows, each page before reading the next. The release totals every row is weighed against are
  read once. What stays in memory is the plan's writes, which the preview prints; a 500 000-row catalog
  is no longer held whole to decide them.
  `every_row_is_handed_out_once_and_no_file_is_split_across_two_pages` holds the paging to every page
  size from one row up.

## Deleting

- **A deleted track takes its file, every row cut from it, its want and its vault object.**
  `Library::delete_tracks` (`deleted.rs`) takes the `Walk` guard, reads the distinct paths the named
  tracks sit at — `Error::UnknownTrack` for an id no row holds — and removes each file from disk; a
  file already gone counts as removed, and one the filesystem refuses is logged, counted in
  `Deleted::kept` and its rows left standing. For every path that went, in one transaction, the
  `wants` naming a release row the path's tracks fill or offering that path are dropped — or the next
  poll would fetch the song straight back — and every `tracks` row at the path goes, so a cue sheet's
  cuts go with the file they are cut from, plays, listens and studies with them on the cascades;
  `store::sweep_orphans` and `alternatives::settle` then run as after a prune. A vault object no row
  names any more is taken out of the vault and its row forgotten, one that will not go waiting for the
  next `prune_the_vault`. Nothing is kept to put back. The window asks first (`ui.md`).
  `deleting_a_track_removes_its_file_and_its_row_and_leaves_the_rest` and
  `deleting_a_delivered_track_takes_its_vault_object_and_its_want` are the claims.

## Organising

`Library::organise` files every scanned track under a layout — the `organise-as` key, or `--as` for
one run — and `resonate organise` and the settings pane's *Organising* group under Library are the two
ways in. It takes `Library::scan`'s `Walk` guard and re-keys a sleeve-keyed album after the moves land
(both under *Schema and grouping*, being facts about the catalog, not the pass).

- **A `Layout` is segments of pieces, the segments split before the pieces are read.** `Layout::read`
  splits the template on `/` first and only then reads each part into `Piece::Literal`s and
  `Piece::Named(Field)`s, so a `/` can never reach a literal or field: by the time pieces exist no
  separator is left to put in one. An empty segment is `LayoutFault::EmptySegment`, so a leading `/` is
  refused rather than making the path absolute, and a `.` or `..` segment is `Error::LayoutEscapes`
  naming its index. That, with `as_one_component` writing every `/` a *value* holds as `-`, is the whole
  guard: neither a tagger's text nor the template can name a path outside the file's root — why
  `Refusal` has no `Escapes` variant, nothing able to construct one. `{{` and `}}` write a brace, an
  unclosed `{` is `LayoutFault::Unclosed` at its byte offset, and a name `Field::read` does not know is
  `Error::UnknownLayoutField` carrying it as a `FieldName`, not prose. All of it is refused when
  `organise-as` is *read*, so a mistyped template is a startup error, never a half-moved library.
  `Display for Layout` writes the template back as read — what the settings pane's field draws and
  `DEFAULT_LAYOUT` (`{albumartist}/{album}/{disc}{track} {title}`) is asserted against.
- **A component is what a filesystem will take, and a segment resolving to nothing is dropped.**
  `as_one_component` maps `/` to `-`, drops control characters and delete, trims leading whitespace and
  trailing whitespace and dots — so *...And Justice for All* keeps its dots while `..` resolves to
  nothing — and cuts what is left to `COMPONENT_BYTES` (255) on a character boundary. The extension is
  appended only where the layout does not name `{ext}`, and the last segment's budget is 255 less that
  extension, so a name cut to the limit still ends in `.flac`. A last segment that itself ends in
  `.{ext}` is budgeted the same way — `Segment::write_ahead_of_the_extension` writes what comes before
  the dot, which is cut, and the extension goes back on after — so naming `{ext}` cannot lose it to the
  cut and leave a file the next scan prunes with its plays
  (`a_long_name_cut_to_fit_keeps_the_extension_the_layout_names`). A middle segment resolving to nothing is
  skipped and the path closes up; the *last* answers `None`, read by the planner as
  `Refusal::Unidentified`, a file with no name to give being left where it stands. **What a volume
  takes is read per root**: `Naming::of` finds the root's mount in `/proc/self/mounts` — the deepest
  mount point above it, the table's octal escapes read back — and a root on vfat, exFAT or NTFS is
  `Naming::Portable`, which writes `\ : * ? " < > |` as `-` too, so a title ending in a question mark
  becomes a file such a drive takes rather than a rename refused every run. Any other root keeps every
  character but the separator.
- **`{albumartist}` falls back to nothing, never `{artist}`.** Its column is the album's own owner, and
  an album whose tracks disagree about one has none (what `COMPILATION` meant). Falling back to the
  track artist would scatter a compilation into a folder per singer — the split the grouping's third
  tier prevents — so the segment resolves to nothing and the album folder sits directly under the
  root. `{disc}` is the same judgement in miniature: `N-` only where the album has more than one disc.
- **Which disc a set is on is read as the scan reads it; `max(disc_number)` alone is not enough.**
  `scan::disc_in_folder` has two callers — `scan::sleeve`, gathering `CD1` and `CD2` into one album, and
  `organise::disc_of`, naming a row's disc where no tag does. A set so filed with no disc tags has no
  `disc_number`, so `{disc}` would write nothing and both discs' track 1 would name one file;
  `Planner::knows` therefore raises each album's count to the larger of its stored `max(disc_number)`
  and what the folders spell, per album rather than per row, so every track of a set agrees how many
  discs there are.
- **What filing weighs across the library is read lean, and the rows are paged.** A destination is
  checked against every source path, a layout needs its root's `Naming` and an album's disc count, and
  nothing else is library-wide, so `Planner::knows` is fed `Filing`s — path, root, album, disc — by a
  cursor collecting nothing, and the rows naming a file are read through retag's `paged::Paging`. A
  sheet tying files together is followed across a page: members the page lacks are read by their paths,
  the set filed together, and each member's own page later passes it by. The plan itself is still
  whole, the preview listing every move and the chains ordered across all of them.
  `files_one_sheet_names_are_filed_together_even_when_a_page_holds_only_one_of_them` is the claim.
- **The preview is the plan the apply performs, not a description of it.** `organise::run` builds one
  `Plan` and hands that `Plan` to `apply` only where `OrganiseOptions::apply` says so — no second walk,
  no second rule set — and the settings pane's `Pass::Preview` and `Pass::Apply` are one call with one
  flag. A preview still reads the filesystem — `symlink_metadata` on every destination, `read_dir` on
  every source folder — collisions and sidecars being facts about the disc, not the catalog.
  `Plan::folders` is what those listings answer: the folders all of whose files are going, deepest
  first. An apply does not read that list — `prune` takes the source folders of the moves that landed
  and `climb_out_of` walks each upward, removing a folder while empty and stopping at the first holding
  something or at the root — but both sort `deepest_first`, so a preview names the folders an apply
  would take, in its order.
- **A file moves before the catalog does, in batches, and a batch that cannot finish is put back.**
  `apply` walks the moves `MOVES_PER_BATCH` (256) at a time. In a batch each move is weighed against the
  disc again (`standing`: a vanished source is `SourceGone`, a destination now taken `Collided`), renamed
  with its sidecars and recorded in `done`. A sidecar is weighed apart from its track
  (`with_the_sidecars_that_can_go`): one gone since is dropped and one whose destination is now taken
  stays where it stands, the track moving without either, where `rename` overwrote the file there and
  a deleted sidecar refused its track on every undo
  (`a_walk_back_leaves_a_sidecar_rather_than_overwrite_a_file_or_refuse_its_track`); then `settle` fsyncs every folder written and
  `Library::files_moved` rewrites `tracks.path`, `playlist_entries.path`, `lyrics_kept.path` and
  `resume_rows.uri` in one transaction, so a play count, playlist row, kept lyric and kept queue follow
  the file rather than being rescanned into a new row. The queue open in a player follows too: where
  the window ran the organise, `LibraryModel` keeps an applied run's moves and the root view hands them
  on as `Command::Relocate`; where `resonate organise --apply` ran it, every player on the bus is handed
  the landed moves through `org.resonate.Player1`'s `Relocate` (`mpris.md`). `Queue::relocate` rewrites
  every row naming a moved file (the playing track's location with it), following each row through the
  moves in landing order — `queue::landed_at` — so a cycle broken through a parked name lands each row
  where its file did rather than at the parked name, and the next resumption written carries the new
  paths rather than overwriting the rewritten ones
  (`a_queued_row_whose_file_was_moved_is_reached_where_it_went`,
  `two_files_trading_names_through_a_parked_one_are_each_followed_to_where_they_landed`). It first deletes any `tracks` and `lyrics_kept`
  row standing at the destination, both keyed by path, else the `UPDATE` is refused: `standing` weighs
  the *file* there, so a row whose file had gone — not yet tidied by a scan of another root — once
  failed all 256 moves of its batch. `playlist_entries` and `resume_rows` need no such delete, neither
  being unique on path. A failing rename puts back only its own move's steps — the audio and whichever
  sidecars had gone — and is refused as `Refusal::Unmoved` with the volume's `io::ErrorKind`, the rest
  of the batch going on; a batch was once put back whole for one refused file, and with the same plan
  next time it failed the same way every run. A failing catalog write still runs `put_back` over all of
  `done` in reverse and the whole batch counts `failed`. The order is the point: a crash between the
  two leaves a moved file the catalog has not followed, which the next scan reconciles, where the
  reverse would leave the catalog naming absent files.
  `a_move_the_volume_refuses_is_refused_alone_and_the_rest_of_its_batch_lands` is the claim, and the
  batch size bounds how much of a run a failed catalog write can undo.
- **A batch takes back the folders it made and did not fill.** `renamed_onto` calls `create_dir_all`,
  which says nothing of which levels were new, so `not_there_yet` walks up from the destination's
  parent first recording those missing; `batch_moved` runs `take_back_the_empty` over that list whatever
  the outcome. `taken_away` only removes an empty folder, so one holding a landed file survives and one
  left by a rollback goes — one path for both. Deepest first, a tree coming away a level at a time. It
  does *not* climb: only what this batch made goes, never an already-standing empty folder.
- **A chain is ordered and only a cycle refused.** `Planner::in_the_way` answers `InTheWay::Stands` for
  a destination another planned move claimed and for one the filesystem holds that no scanned row names
  — except a file moved onto itself (same device and inode) — and `InTheWay::MayGo` for one that *is* a
  scanned row, whether it is itself going being unknowable until every row is read.
  `Planner::order_the_chains` decides: a planned move waits on at most one other, so the graph is
  functional and `walked` follows each chain to its end and emits the deepest first, putting `B → C`
  before `A → B` and landing both in one run. **A chain closing on itself is broken through a parked
  name.** `parked_out_of_their_cycles` finds each cycle of single-file moves — each waiting on exactly
  one other — and splits one move in two: its file goes first to `<stem>.resonate-parked-<pid>.<ext>`
  (`PARKED`) beside where it stood, waiting on nothing, and from there to its destination, waiting on
  what it waited on; the walk orders the rest of the cycle between, so two files filed under each other's
  names trade places in one run
  (`two_files_filed_under_each_others_names_trade_places_and_keep_their_plays`). The catalog follows each
  step in the batch's one transaction, so the row is at the parked name no longer than the batch.
  `Move::parks` names the first half, which the preview lists and no count counts as moved. A failure
  between the halves leaves a file under a visible name the catalog follows, filed properly next run; a
  run killed between a rename and its commit leaves a file the next scan follows by
  `moves::follow_the_moved` like any hand-moved file. A move leading into a cycle a unit sits in, and
  every move waiting on a row that turned out to stay, is still `Collided`. A move refused there gives
  its source and sidecars back out of `going`, so `empties` counts only folders all of whose files
  really leave. `Refusal` is `Unidentified`, `Loose` (a destination with no folder between it and the
  root), `Collided`, `SharesASheet`, `SourceGone` and `Unmoved`, each printed against the path it left
  standing.
- **A rename crossing a filesystem is a copy, and the source goes only once the catalog has
  followed.** `landed_onto` reads `io::ErrorKind::CrossesDevices` off `fs::rename` and falls back to
  `copying`, which copies the bytes, carries the source's modification time onto the copy — so the next
  scan reads it as the known file rather than one to probe again, which would re-identify a stem-named
  row against its new name — and `sync_all`s it before anything else. It is written under the
  destination's name with `STAGED` appended and renamed into place only once whole and synced, so a run
  killed mid-copy leaves a staging file beside the destination, not half a file under its name. The order
  makes it safe: nothing is deleted inside the batch, so `put_back` undoes a copy by *removing* it while
  the original stands, and `left_behind` removes the sources only after `Library::files_moved`
  committed. A run killed after the rename and before the commit leaves the whole copy on the other
  filesystem beside its source, and `already_copied` is how the next run reads it: another device, the
  same size, the same modification time (the copy carries the source's) and the same bytes, read in
  `COMPARED_AT_ONCE` pieces only once the three cheap readings agree. Such a destination is not in the
  way, not copied again, and the move lands as `Landing::Copied`, the catalog following and the source
  going as it would have. `a_copy_a_killed_run_left_whole_on_the_other_filesystem_is_taken_as_landed`
  and `a_file_of_the_same_size_and_time_but_other_bytes_is_still_in_the_way` are the claims — why
  `Refusal` has no `AcrossDevices` variant any more, nothing constructing one.
- **A staging file is written down before it is written, so a killed run's is swept by the next.** A
  staging file stands beside a destination the next run may never plan again — the layout moved, the
  file retagged — so it cannot be found by walking what a plan names. `noted_staging` writes its path
  and this process's pid into `staged_writes`, the fourth `MIGRATIONS` step, and commits that before a
  byte of the copy or rewritten sheet is written; `staged_left` removes the file where it still stands
  and lets the row go once it is gone, landed or not, and a file it could not remove keeps its row. A
  run that applies begins with `sweep_what_a_killed_run_staged`, removing each noted file — only a
  regular file whose name ends in `STAGED`, so a row can never cost a file it did not name — and passing
  over a row whose pid is another process `/proc` still holds, the `Walk` guard being this process's
  alone and a window and a `resonate organise --apply` beside it each possibly copying. A preview writes
  and sweeps nothing. `what_a_run_that_was_killed_staged_is_taken_away_by_the_next_run_that_applies` is
  the claim.
- **The last applied run can be walked back.** `organised`, a `MIGRATIONS` step, holds what the last
  apply landed — every unit, its companions and sidecars, in landing order — each moving apply replacing
  it. `OrganiseOptions::walk_back` builds its plan from that rather than a layout: the units in reverse,
  each `Move::reversed`, so a chain and a parked cycle undo in the order that makes room, handed to the
  same `apply` — batches, the catalog following, a sheet's `FILE` line renamed back, the folders the run
  made pruned. What the walk back landed is noted in turn, so walking it back again files the tracks
  again — where it landed every unit. One cut short, by a cancel or a refusal, keeps the units it did
  not put back and drops those it did (`still_to_walk_back`), so the next walk back finishes the job
  (`a_walk_back_cut_short_keeps_what_it_did_not_put_back_for_the_next_one`). `resonate organise --undo` previews it and `--undo --apply` makes it, and *Organising* offers
  *Put the last run back* behind a second press wherever `Library::walks_back` says a run is kept
  (`an_applied_run_is_walked_back_file_for_file_and_walking_it_back_again_files_them_again`). A file
  moved or gone since is refused at `standing` like any other.
- **A run given files files those alone.** `OrganiseOptions::only` empty is every row; naming paths
  reads just those rows through `tracks_to_file_at`, `ROWS_A_PAGE` paths at a time, in place of the
  paging walk — so a drop's filing costs its own rows, not the catalog's. Every file is still *known*
  to the planner, collisions and chains being facts about the whole library, and a sheet naming a
  file not given still reads it as a member
  (`a_run_given_files_files_those_alone_and_leaves_the_rest_where_they_stand`).
- **A folder's pictures follow its tracks where every track leaves for one folder.**
  `Planner::pictures_follow_their_folders` runs once the chains are ordered: for each source folder
  every one of whose scanned tracks is going, and all of them into one folder (`FolderLanding`), each
  picture standing in it (`names_a_picture`) is a sidecar of the last move out, landing under its own
  name unless something stands or is claimed there. A folder whose tracks scatter, or keep one behind,
  keeps its pictures, the sleeve belonging to what stays as much as what goes. They are noted with the
  run, so a walk back brings them home
  (`a_folders_pictures_follow_its_tracks_only_where_every_track_lands_in_one_folder`).
- **A run files the roots it is given, and one the catalog does not hold is refused.**
  `OrganiseOptions::roots` empty is every root — what the settings pane and a bare `resonate organise`
  ask; naming one puts a `roots.path IN (…)` on `TRACKS_TO_FILE`, so unwanted rows never leave SQLite.
  The check that each named root is held runs on the pass thread, not in `organise::start`, because
  `start` may not read the catalog before it has the `Walk` guard — a read there blocks behind a writer
  the guard waits on, which `a_scan_refuses_to_start_while_an_organise_is_running` caught. `--as` is the
  layout for one run, read through `organise-as`'s `Layout::read`, so a mistyped template is refused
  before anything moves.
- **A file beside a track sharing its name travels with it.** `Planner::sidecars` takes the files in
  the track's folder that are not scanned rows, whose name is the track's stem followed by `.` and more,
  and whose extension is not audio — so `Meddle.cue` and `Meddle.wav.log` follow `Meddle.wav` onto the
  destination's stem. A cue-cut file is the exception proving the rule: the rows cut from one file are
  filed by the folder their layouts agree on and keep their name, the sheet beside them naming that
  file, so the sidecar lands under an unchanged stem and still names its audio. Rows of one file naming
  two folders are `Unidentified`, not filed under whichever came first. A sheet cutting *one* row from a
  file is the case that does not hold: the file is rendered by the layout like any other, so
  `sheets_follow_their_audio` rewrites the landed sheet after the batch's catalog write. `cue::renamed`
  is the whole of how — it reads the sheet to learn its encoding and check that exactly one `FILE` line
  names that file, encodes old and new names in that encoding and splices the one byte run following a
  `FILE` command on its own line (`the_run_on_the_file_line`, read a unit of the encoding at a time, so a
  UTF-16 sheet is walked in pairs and a `REM` or `TITLE` naming the same file is left alone) — so a BOM,
  a line ending and every other byte survive; and `staged_over` renames a staged file over the sheet, so
  a crash cannot truncate it. A file two `FILE` lines name leaves the sheet alone. A name the sheet's
  encoding cannot hold can only be a legacy code page's, UTF-8 and UTF-16 holding every name, and that
  sheet is carried into UTF-8 with a byte-order mark rather than left naming a vanished file:
  `carried_into_unicode` reads the bytes around the run in the code page the sheet was read in —
  every line ending kept — and writes the new name between. The mark
  tells a player that reads a markless sheet as the system code page that this one is not.
  `a_sheet_whose_encoding_has_no_letters_for_the_new_name_is_carried_into_unicode` is the claim.
- **A sheet cutting a file travels with it whatever it is called, and one naming several files takes
  them all as one move.** The stem rule finds only `Meddle.cue` beside `Meddle.wav`; a rip whose sheet
  is `Meddle.cue` and audio `CDImage.wav` left the sheet behind, and the next scan read the file whole
  and pruned every row it had cut — plays, listens, favourites and vault links with them.
  `Planner::sheets_in` reads each source folder's sheets once, through the scan's `read_sheet` and
  `beside`, and `sheets_travelling` weighs every one naming the file being moved: one naming only that
  file is a sidecar that must land — under the renamed stem where it shared the audio's, under its own
  name otherwise — and a destination it cannot take refuses the whole move as `Collided`, where a loose
  sidecar is merely left. A sheet naming more than one existing file — an EAC rip with one `FILE` per
  track — ties them: `Planner::tied_by_sheets` gathers every file such a sheet names, and every file
  any sheet naming one of those names, and `file_together` plans them as **one `Move`**, the first file
  its `from` and `to` and the rest its `companions`, the sheet a sidecar under its own name. One move
  keeps them together through everything after the plan: a batch never splits it, `renamed_onto` puts
  every file back where one fails, `files_moved` has the catalog follow each file, and
  `sheets_follow_their_audio` renames every `FILE` line in the landed sheet. `Move::files` is the one
  walk all of them take, and `Plan::files_moving` what a preview counts. The layout must land every file
  in one folder and every file must be one this pass files, or each is `Refusal::SharesASheet`, the one
  failing on its own account taking its own refusal, and such a sheet is kept out of the loose stem pass
  likewise. **A unit is a link in a chain like any move.** A destination held by a file this pass moves
  on is something to wait for, and a unit may wait on as many as it has members:
  `Planned::waits_for` is every source standing where the move lands, and `order_the_chains` puts a move
  behind each move vacating one. `walked` is a depth-first topological order over those waits,
  iterative so a long chain costs no stack: a move is ordered once everything it waits on is, and a move
  meeting one still being walked — a cycle — or one already doomed dooms every move on the walk, each
  waiting on it in turn. A move left out is refused as `Collided` with the first source it waited on,
  each member of a unit alike. A unit whose member lands where another member stands waits on itself and
  is doomed with it, as always: a rename inside one move cannot be ordered. At the apply `standing`
  weighs every file of a move, so a chain the disc changed under since the plan is still refused rather
  than renamed over. `the_files_a_sheet_names_wait_for_a_file_standing_where_one_lands_to_move_on_first`
  is the claim.

## Taking files in

`take_in.rs` is the copy behind dropping songs on the window: `take_in(TakeInOptions { paths, into })`
starts a `resonate-take-in` thread behind a `TakeInHandle` (`PassKind::TakeIn`) and answers a
`TakeInSummary` — `landed`, `passed` with a typed `Passing` each, the counts and whether it was
cancelled. It touches no catalog, so it takes no `Walk` guard; the window has a scan follow it.
`Error::DestinationNotADirectory` is the one refusal before the thread starts.

- **What is taken is what the scan reads, and what an album keeps beside it.** A dropped file is
  taken where `names_audio` or it is a `.cue`; a dropped folder is walked, skipping names starting
  with `.` and links, to `DEEPEST_FOLDER` levels, and its audio and cue sheets are taken. Beside
  them come the *companions*: anywhere in a dropped folder, a picture (`resonate_core::names_a_picture`,
  so `cover.jpg` and a `Scans/` folder alike) and a lyric sheet (`LYRIC_ENDINGS`: `.lrc` and a
  Lyricsfile), and in any folder holding audio a file named after one of its tracks — the stem, a
  `.` and more (`is_named_after`, organise's sidecar rule), so `01.txt` and `Album.flac.log` come and
  `notes.txt` does not. A folder whose walk finds companions and nothing else is `NothingInside`,
  and a loose file is a companion (`Looks::Companion`) only where the same drop takes audio from its
  folder — a picture or lyric sheet dropped beside a song, never alone. Anything else is `NotAudio`.
  The same file named twice, by its canonical path, is one. `weigh` is the cheap, read-only look
  the window draws while a drag is over it (`Looks`), and `gather` reads the drop through it.
- **Names are kept, and nothing is overwritten.** A file lands at `<into>/<its name>` and a folder at
  `<into>/<folder>/<relative path>`, merging into a folder already there. A file inside `into` is
  `AlreadyThere`; one whose destination holds the same bytes is `AlreadyHeld` — neither counts as a
  refusal (`Passing::is_a_refusal`); a different file under the name lands as `name (2).ext` and on.
- **A copy is whole, read back and then named.** The bytes go to a hidden
  `.<name>.<pid>.resonate-part` beside the destination, are compared with the original in
  `COMPARED_AT_ONCE` pieces, synced, and taken to their name by `hard_link` — which refuses an
  existing name atomically, so a file made in the meantime is never overwritten — falling back to
  `rename` where the filesystem has no links; the staged file is always removed. A copy that does not
  read back is `Unverified` and is not kept (`nothing_is_left_staged_…`).
  `a_byte_for_byte_copy_already_there_is_held_and_a_different_one_is_kept_beside_it` is the claim.
- **What travels with a track follows the name the track landed under.** The audio lands first, and
  `Renames` notes each whose name moved — copied as `name (2).ext`, or found already held byte for
  byte under such a name (`Stood::Held` carries where) — by the folder it came from. A sheet or
  companion from that folder is then aimed after it: one named after the track takes the new stem
  (`following_its_audio`, the longest stem winning), and a sheet whose `FILE` line names a renamed
  track is rewritten through `resonate_codec::renamed_cue` — the rule organise follows a sheet by,
  encoding and bytes kept — and landed as `Content::Rewritten` bytes rather than a copy, read back
  and weighed against a standing file the same way. The source is never touched.
  `what_travels_with_audio_that_landed_under_a_new_name_follows_that_name` and
  `a_sheet_naming_audio_already_held_under_a_new_name_is_written_naming_that_name` are the claims.
- **The window files what landed once the scan has it.** With `file-dropped` on (`binary.md`),
  `RootView::took_in` hands the landed audio and the `organise-as` layout to
  `LibraryModel::file_once_scanned`, and `take_up_what_waited` — run as every pass ends — starts an
  applied `organise` with `OrganiseOptions::only` naming those files once no root waits to be
  scanned. It is an ordinary run: it takes the `Walk` guard, carries sidecars and sheets, prunes the
  emptied folders, toasts what it filed and is the run *Put the last run back* walks back. A file the
  layout cannot name — no title, or nothing between it and the root — stays where it landed.

## The search grammar

- **A search is words and terms, and what a term means is typed.** `Search::read` is the whole
  grammar: a token is a word unless it names a field, where `title:`, `artist:`, `album:`, `genre:` and
  `lyrics:` (or `lyric:`, `LYRICS_ALIAS`) scope the words beside them and `year:`, `added:`, `plays:`,
  `played:`, `length:`, `rate:`, `depth:`, `codec:` and `is:` are `Term`s — each with the aliases `KEYS`
  lists: `heard:`, `duration:`, `samplerate:`, `bits:`, `format:`. A token naming no field, or whose
  value the grammar cannot read, is the words it was written as — `lrc.rs`'s rule for a bracket neither
  moment nor id tag — so a search never fails to parse and the box stays live as typed, a mistyped term
  going quietly. Every numeric reader is overflow-safe, so a value past its type is unreadable rather
  than a panic: `length:400000000000000000:30` is three words. `Display for Term` is the canonical text
  and reads back as the same term, so the window's *Reads* row, `resonate playlist --query` and the
  grammar's own tests say the same thing rather than each writing prose. `is:hires` is lossless above
  CD, so `CD_SAMPLE_RATE` (44 100) and `CD_SAMPLE_DEPTH` (16) in `db.rs` are where that claim lives, and
  `depth:` reads the stored `SampleFormat`, 24 valid bits for a float file.
- **What a term narrows on is what the catalog stored, and two of them are ages, not dates.** `added:`
  and `played:` are measured from the moment the query runs — a month 30 days whatever the month, a year
  365 — which makes "added this year" `added:<1y` and a saved query answer differently tomorrow.
  `plays:` is a count beside them, so `plays:0` has never been heard and `plays:>5` has, and `heard:` is
  `played:`'s other spelling. A `@` after the count is a window over `listens` rather than the lifetime
  column: `plays:>20@30d` is more than twenty plays in the last thirty days, the age after it read as
  `added:` and `played:` read theirs, a range carrying the window on both bounds (`plays:5-10@30d`), and
  two `Plays` terms folding back into a range only where their windows agree. An unreadable window makes
  the whole token a plain word. It costs the one term no index serves — a correlated `count(*)` over the
  listens per candidate row — which is why it narrows and never orders. `year:` is the *album's*, so a
  track whose album declares no year answers no year term and an ungrouped single has nothing to
  answer with. `rate:` and `depth:` are what the file is, not what the sink is asked for (the
  inspector's report), so `depth:32` names the 32-bit integer files alone. `is:lossy` names the codecs
  known lossy rather than everything not lossless, so a file whose codec the scan could not name is in
  neither it nor `is:lossless`. Nothing narrows on a rating, the catalog storing none, and nothing sorts
  on a term: a term narrows and `SortOrder` orders. A search is parsed every keystroke and a saved query
  parses its text every read, nothing caching either, a handful of tokens being no cost worth holding.
- **Everything a search says holds at once, unless a `-` denies it or an `or` offers an
  alternative.** `Search` is `Clause`s that all hold, a `Clause` is `Asked` alternatives one of which
  must, and an `Asked` is the `Condition`s one token read together with whether it was denied — so a
  range is a shape, not two searches: `year:1970-1979` is two bounds that both hold, and
  `-year:1970-1979` each bound denied in one clause, De Morgan done at the parse so nothing downstream
  brackets. `Display for Asked` folds an undenied pair of bounds back into its typed range, so
  `year:1970-1979 or is:hires` reads back with the alternation against the whole range, and a denied
  range, already its two denials joined by `or`, reads back as itself. `-` denies only at the start of an
  unquoted token, so `well-known` and `1970-1979` are untouched, and `or` joins only between two tokens,
  unquoted and undenied, so a dangling one is the word written — a mistyped term's rule, quoting being
  the way to search either literally. No bracketing: a denial reaches one token and an alternation one
  flat run, so `-a or b` is "not a, or b" and `-(a b)` cannot be written — De Morgan by hand says it.
  `and` is no word the grammar knows, everything being joined by it already.
- **A search asking nothing holds everything, however it was typed.** A word is a condition only
  where `search::pieces_of` finds a letter or digit in it, those pieces being all that ever reaches the
  index, so `!!!`, `+/-`, `-!!!` and `artist:?` are dropped at the parse and an `or` closes over the gap
  (`moon !!! or sun` reads `moon or sun`). `db::matching` passes over a `Search` holding no clause, so a
  box of punctuation, a blank saved query and no text at all are one answer: the catalog. Each once
  became an FTS `MATCH` of nothing and answered no rows. `cuts_matching` answers a `Narrowed` of
  three: `Unasked` where the matching `narrows` nothing, `To` a narrowing, and `Nothing` where no
  row can match. A list narrowed by a blank text holds every row, as a saved query does; one narrowed
  by a text that is there but that a search reads nothing in — `!!!`, `???` — is `Nothing` and holds
  no row (`asks_for_nothing_it_can_read`), since taking it as no condition played and copied the whole
  list. The search box keeps reading punctuation as no condition, a caret mid-word not emptying the
  pane. A drop is the one reader kept refusing, taking only `To`, so nothing empties a list in one
  gesture. `a_search_made_only_of_punctuation_asks_nothing_and_so_holds_everything`,
  `a_saved_query_with_no_search_fills_itself_with_the_whole_catalog` and
  `a_list_narrowed_by_a_text_a_search_reads_nothing_in_holds_no_row_and_drops_none` in
  `tests/search.rs` are the claims.
- **A number is read as meant, to the precision it was typed in.** A `Term::Length` carries a `Grain`
  — the finest `ClockUnit` a component names and how many decimals its count was written to — and is
  weighed as the track's length cut down to that grain: `length:3:30` and `length:3m30s` hold
  210 ≤ s < 211, `length:3m` every track of three minutes and some seconds, `length:3.5m` the six
  seconds from 3:30, `length:<=3:30` everything below 3:31 and `length:>3:30` everything from it, as
  a pane draws a length. It was a float equality, holding only a track exactly 210.000 s long, where
  `added:` and `played:` already read an exact value as a span. `Display for Term` writes a length
  down to its grain, zero components and decimals kept (`length:180` reads back `length:3m0s`,
  `length:0.50s` as itself), so it reads back as the same term. A range typed high end first is the
  range it names — `bounded` puts the two bounds in order, so `year:2000-1990` is `year:1990-2000` —
  and a bare rate below `A_BARE_RATE_IS_IN_KILOHERTZ_BELOW` (1 000) is kilohertz, no file being
  sampled that slowly: `rate:44.1` is 44 100 Hz, `rate:96` 96 000, and `rate:500hz` still 500, written
  back with its unit. *Long players* is `length:>=10m`, ten minutes and over.
- **`=` asks for a whole name, where a word or phrase asks for a run inside one.** A phrase matches
  its tokens anywhere in a column, so `artist:"Air"` holds *Air Supply*. `artist:=Air` and
  `artist:="Air Supply"` are `Reach::Whole` (beside `Begins` and `Phrase`), `Display for Word`
  writing them quoted; an `=` inside the quotes is part of the phrase, and `lyrics:` takes no whole
  name, the words sung being no name. `db::named` weighs a whole name through the `words_of` SQL
  function `store::words_of` registers, both sides folded and split alike: an artist is by id —
  `tracks.artist_id` or a `track_credits` member whose name is it, the artist pane's own reading —
  an album its billed or scanned title, a title and a genre (the track's or its artist's) the column
  itself behind the phrase's `INDEX_LOOKUP`, which narrows the rows the function is asked of. A whole
  name is never ranked, reaching the `WHERE` as a denial does. The artist mixes `suggest.rs` offers
  are whole names, so a mix for *Air* is *Air*'s
  (`an_artist_mix_holds_the_artist_it_names_and_not_one_whose_name_begins_with_it`).
- **`tracks_fts` holds the fold of a name, not the name, and the query is folded with it.** What is
  indexed is `folded_letters` of the title, the billed artist, the album and the genres (and, apart, the
  lyrics), and `db::indexed` folds every piece of a typed word the same way before it becomes a `MATCH`,
  so the two sides can disagree only by being changed apart. That buys a letter with two spellings: the
  tokenizer's `unicode61 remove_diacritics 2` folds `Ç` to `c` and even `İ` to `i` but leaves `ı` alone,
  a dotless i being a letter of its own — so *Kıskanç* was reachable only by typing the dotless ı and
  *KISKANÇ* only by not. The same fold lets *Przybylowicz* find *Przybyłowicz*. `remove_diacritics`
  stays on the tokenizer although the fold did the work, costing nothing, the index not resting on the
  fold being complete. Nothing reads a `tracks_fts` column back — only `MATCH` and `rank` — so no
  spelling is kept beside the fold. It is a schema break without a migration: an index written before
  the fold holds spellings no folded query matches, and an incremental rescan never rewrites an
  unchanged row, so such a catalog is deleted and scanned again.
- **What a search lights up is read off the name, not the index.** `Search::lit` answers the byte runs
  of a display string a search matched, for one `Column`, which the panes draw in the accent. It cannot
  come from FTS5: `highlight` and `snippet` answer the *stored* column, the fold, so a row would read
  `bjork` where the pane draws *Björk*. So it folds the other way — the name split into tokens on
  `char::is_alphanumeric`, each folded whole, keeping every run's byte range in the original — and
  weighs each token against the `search::pieces_of` that `db::indexed` builds the `MATCH` from, so what
  is lit is what matched: a prefix for a bare word, consecutive tokens for a phrase, nothing for a
  denied word or a term, and a word scoped to a column lights nothing in another. A whole token lights
  rather than the matched prefix, a half-lit word reading as a typo. Meeting runs merge, so a word typed
  three ways lights its token once. **The words are taken before the name**, since `lit` runs once per
  drawn cell per frame and the tokenising it did first — a `Vec` and a `folded_letters` per token, each
  lowercasing into one `String` and NFD-normalising into a second — ran even with an empty search box
  and no word able to reach that column: a pane of eighteen rows with two lit cells each was some five
  hundred `String`s and as many normalising passes a frame for nothing.
- **What a track sings is indexed, and only a word asking for it reaches it.** `tracks_fts`' fifth
  column, `lyrics`, is filled by `store::index_row` itself — `sung_by` reads the row's own
  `tracks.lyrics` or, where the file carried none, the `lyrics_kept` a provider fetched, and
  `sung_words` drops every `[…]` and `<…>` run before folding, so an LRC's timestamps and a karaoke
  line's word marks are no words to find. `Library::keep_lyrics` rewrites the column for the rows at
  that path and span whose file carried none, in the transaction keeping them, so a lyric fetched while
  a track plays is searchable at once and a rescan indexes the same words. A bare word is written
  `{title artist album genre} : "…"*` (`Column::NAMES`), so typing *love* does not bring back every
  song singing it; `lyrics:` is how a listener says they mean the words, and `Search::as_sung` turns a
  search of plain words alone into the one phrase `lyrics:"…"`, which `Library::sung` counts so the
  window can offer it. `spelling.rs` holds no vocabulary for the column and corrects no lyric word.
  `a_track_is_found_by_the_words_it_sings_and_only_when_they_are_asked_for` and
  `a_search_of_plain_words_is_offered_as_the_words_a_track_sings` are the claims. It too is a schema
  break without a migration, a catalog written before it deleted and scanned again.
- **A search reaches what the catalog lacks as well as what it holds.** `release_tracks.folded` is the
  row's title, its artist — the track's own credit or the release's — and the release title, through
  `folded_letters` by `land_release`, and `Library::unheld_matching` asks it one `LIKE` per piece of
  every word a search asks by name: the lone words, bare or scoped to a title, artist or album
  (`elsewhere::words_asked`). A term, a denial, a genre and a lyric have nothing to answer with on a row
  nobody holds, so a search of those alone answers nothing. Wanted rows come first, and it and the
  Missing pane read the same `SHORT_OF_WHAT_IS_HELD_OR_WANTED` rule: a release row is missing where its
  album holds a track or the row itself is wanted, so a release landed for one song does not list the
  eleven nobody asked for. `a_search_reaches_the_rows_the_catalog_knows_it_is_short_of` is the claim.
- **A song is placed on the album it was meant for, not whatever came out first.** A hit single is
  dated before the album it was cut from, and a compilation often before both, so the earliest release
  was wrong for most songs. `elsewhere::meant_release` weighs each release's `Issued` first — `Standing`
  puts an official release before one of unstated status and both before a bootleg, promotion or
  withdrawn one, and `Meant` puts an album (a soundtrack counts) before an EP, an EP before a single,
  and any of them before a release of unstated kind and before a compilation, live album or anything
  else with secondary types — and only then the date, undated last. It is what `best_release` falls
  back to and what a found song is wanted from.
  `a_song_is_placed_on_its_album_before_a_single_or_a_compilation_that_came_out_first` is the claim.
- **A song the catalog has never heard of is found elsewhere and wanted by landing its release.**
  `Library::found_elsewhere` sends the words a search asks by name to `Reference::find_songs` and
  answers `Found`s: a recording, its title, credit, length and the release it was meant for
  (`meant_release`), leaving out every recording the catalog names in `tracks.mbid` or
  `release_tracks.recording_mbid` and dropping a second recording of the same folded title by the same
  folded credit, up to `FOUND_ELSEWHERE_AT_MOST` (12). The two halves are public apart, because the
  window keeps the reference's answer and weighs it again: `songs_asked` is the words as sent —
  only the title, artist and album words, lower-cased and single-spaced, so *Pink  Floyd* and
  *pink floyd year:1971* are one ask — or `None` for a search whose words hold fewer than three letters,
  which is not sent (`asks_elsewhere` is its `is_some`); `Library::unheld_among` takes the matches and
  asks the catalog about those recordings alone, `WHERE mbid IN (…)` under `tracks_by_recording` and
  `release_tracks_by_recording` — a `MIGRATIONS` step — rather than reading every recording id the
  catalog names. **The songs of a library artist's releases not held are learnt in the lookup pass.**
  `Pass::learn_the_songs` runs last in every pass, after the covers and portraits: every release group
  `groups_whose_songs_are_due` answers — unheld by the `unheld_by_any_album!` predicate, never read or
  read longer ago than `REFRESH_AFTER`, refused longer ago than `REFUSED_AGAIN_AFTER`, the most played
  artist's first, `at_most` capping it — is asked `Reference::releases_of_group`, and
  `songs::pressing_of` takes the pressing whose track count most pressings share (fewer tracks on a
  tie: the original over a deluxe), the earliest of those, a full date before a bare year. `songs::land`
  replaces the group's rows in `discography_songs` — group, recording, the pressing's id, title, date and
  kind, disc, position, title, credit, length, `words_of` the title and a `folded` haystack of every word
  of title, credit and release title — and stamps `discography_songs_read`, both a `MIGRATIONS` step; an
  answer with no pressing stamps it read with nothing, a refusal counts in `refusals`, and an
  unreachable reference ends the pass as every other lookup does. `EnrichStats::songs` counts the rows
  (`the_songs_of_releases_not_held_are_learnt_in_the_lookup_and_found_without_asking`,
  `a_release_group_refused_waits_before_its_songs_are_asked_for_again`). **The sleeves of those releases
  are kept in the same pass.** `Pass::cover_the_unheld` runs after `learn_the_songs`: every release
  group `unheld_covers_due` answers — unheld by the same predicate, with no cover held and none asked
  within `COVERS_ASKED_AGAIN_AFTER`, the most played artist's first, `at_most` capping it — goes to the
  picture readers as `Picture::OfAnUnheldRelease`, asked as `Reference::cover` for the pressing its
  songs were read off with the group as the fallback, or `group_cover` where no pressing was read.
  `land_unheld_cover` writes `unheld_covers` (group, the picture or none, when asked — a `MIGRATIONS`
  step) for an answer of either kind, never for a fetch that failed or was refused, and a picture once
  held is never replaced; `EnrichStats::covers` counts the pictures. `Library::unheld_cover` reads one
  back, its format sniffed, which is what the artist's page draws before it asks the archive itself
  (`the_covers_of_releases_not_held_are_kept_in_the_lookup_and_not_asked_for_again`). Three readers:
  `songs_kept_for` is a search's — every folded word a word start in `folded`, held nowhere by recording
  and the group still unheld and no track of its artist the same `words_of`, albums first, one per
  folded title and credit, up to `FOUND_ELSEWHERE_AT_MOST`; `songs_not_held_by` is an artist page's —
  the rows the artist's own albums are short of (not dismissed, no held track of that title) and then
  the songs of the artist's unheld groups, one per title and credit; `albums_not_held_by` answers an
  `AlbumNotHeld` per release group of the artist's with no album holding a track of it — so one landed
  and still downloading stays — carrying the pressing its songs were read off. `Library::want_album`
  wants every song of a group not already held by recording or title, reading the group's pressings
  first where the pass never reached it, through the one `want_from_release` `want_found` uses too: the
  release read and landed once, each recording's row wanted, the cover asked once
  (`an_album_not_held_is_wanted_whole_from_the_pressing_its_songs_were_read_off`,
  `an_album_whose_songs_were_never_read_has_them_asked_for_when_it_is_wanted`). For an album already
  in the catalog, `Library::want_missing_tracks` selects release rows without a held track and calls
  `want_in` for them in disc and position order under one write transaction and one timestamp; an
  existing want is due again just as when asked individually. The UI starts one provider poll after
  the batch lands. `still_answering`
  narrows songs found for one search to those every folded word of
  another begins a word of — title, credit or a release's title — what the window shows while it asks
  (`songs_found_for_fewer_words_are_narrowed_to_those_still_answering_more`). `Library::want_found` is the want: the
  release the `Found` names, or the one its recording first came out on where the search answered none,
  is read whole through `Reference::release`; an album already carrying that mbid is taken as it stands,
  otherwise a new one is made — billed to `store::artist_named`, stamped `albums.found_elsewhere` — and
  `land_release` writes its rows as the enrichment does, so the want is an ordinary `wants` row a
  provider is asked for and a delivery lands on. Its wanted artist is the track credit where present,
  otherwise the landed release artist, so providers and the player retain the artist even when
  MusicBrainz has no track-level credit. When the landed release advertises front art or belongs to a
  release group and the album has no picture, `want_found` asks `Reference::cover` and saves the answer
  on the album; failure to fetch or store art does not cancel the want. A release the reference lacks is
  `Error::UnknownRelease`, a recording with no release `Error::Unreleased`, and a release not carrying
  the recording `Error::NotOnTheRelease`. `store::ORPHANS` spares a trackless album only where it was
  found elsewhere and still wanted, so a scan keeps it while the want stands and removes it once it
  goes, and an album scanned from a root still leaves with the root. The albums pane never shows one,
  `HOLDS_A_BEST_COPY` asking for a track.
  `a_song_found_elsewhere_is_wanted_by_landing_the_release_it_first_came_out_on` and
  `a_song_with_no_release_named_is_wanted_from_the_one_its_recording_first_came_out_on` are the claims.
  **Which release is the listener's to say as well.** A `Found` carries every release its recording is
  on, and `Found::in_the_order_worth_offering` lists them as `meant_release` weighs them; the found row's
  *Want* mark opens them as a menu under the right button — title, year and kind — and a press is
  `want_found` with `Found::from` that release, so a song wanted for its single or compilation lands
  there rather than on the album the rule would choose.
  `a_found_song_offers_every_release_it_is_on_the_one_it_would_be_placed_on_first` is the claim.
- **A link to a song is followed to the recording it names, by its ISRC, and by its title and
  artist only under the strict rule.** `linked.rs` is the whole of it. `SongLink::read` takes one
  whitespace-free token and answers `MusicBrainz` for a `musicbrainz.org/recording/<mbid>` link,
  `Deezer` for a `deezer.com/[lang/]track/<n>` one, and `Elsewhere` for a song page on Spotify (or a
  `spotify:track:` URI), TIDAL, Apple Music (an album URL's `i=`, or a `song` path), YouTube and
  YouTube Music, SoundCloud (an account and a track, not a set), Amazon Music, Anghami, Boomplay,
  Audiomack, Yandex Music or song.link itself — and nothing for an album, an artist, a playlist, a
  page of any other host or text with a space in it, so words typed are never taken for a link.
  `Library::follow_link` answers a `Linked`: `Held` with the title and artist of a track a file
  already is — a recording link weighed against `tracks.mbid` before anything is asked, a link
  elsewhere against `tracks.isrc` for every code `Reference::song_linked` names and, once a recording
  is settled, against `tracks.mbid` again — `Found` with the `Found` to want, or `Unnamed`. Each code
  is asked of `recordings_of_isrc` in turn until one names a take; `the_take_linked` keeps the takes
  whose length is within `LENGTHS_AGREE_WITHIN` (5 s) of what the link said, the closest, and the
  first where the link gave no length, so a video edit filed under the same code is not the song.
  **Where no code names a take** — a YouTube upload, whose page names no ISRC and often no Deezer
  twin — `LinkNames` carries the title and artist the page bills it under, and
  `the_song_searched_for` asks `find_recording` with them and the link's length, as a phrase and then
  in words, taking an answer only under `enrich::the_recording_named`: the track identification's
  own `matches_a_recording` weighing (`STRICT_SCORE`, a title and a credit agreeing by some
  `Spelling`, lengths within `RECORDING_MAY_DIFFER_BY`) over a `NamedAs` rather than a catalog row,
  no rule of its own. A title with no artist is never searched. **An upload's title is read the way
  uploads are billed**: `readings_of` reads *Artist - Song* as that song by that artist where the
  part before the dash agrees with the channel, the channel's `- Topic` taken off; where it does not
  — a `…VEVO` channel — the upload's own billing is asked first and the title under the channel
  second, each a search of its own. `VIDEO_QUALIFIERS` is a closed list as `VERSION_QUALIFIERS` is —
  *Official Video*, *Official Music Video*, *Official Audio*, *Lyric Video*, *Visualizer*, *HD*, *4K*
  and the rest — taken off a title's end in brackets through `enrich::without_brackets_that`, the
  same stripper `dequalified` runs, so *(Live)*, *(Remix)* or *(4K Remaster)* stay and stop the
  stripping, naming another recording or nothing on the list. A strict answer is asked for whole
  through `Reference::recording`, as an ISRC's take is; nothing strict names nothing. The take is
  placed by `elsewhere::found_among`, `meant_release` choosing the album as for a search. A recording
  only a release row names — wanted, or missing from a held album — is `Found`, `want_found` landing
  it as it would anything else. `RecordingMatch: From<Recording>` scores such a match whole, the
  reading `by_ear.rs` shares.
  `a_link_to_a_song_nothing_holds_is_followed_by_its_isrc_to_the_recording_to_want`,
  `a_link_to_a_song_the_library_holds_answers_the_track_and_asks_musicbrainz_nothing`,
  `a_link_no_service_can_name_names_nothing`,
  `a_song_link_naming_no_isrc_is_followed_by_its_title_and_artist_under_the_strict_rule` and
  `a_song_link_whose_title_finds_only_a_near_miss_still_names_nothing` are the claims.
- **A link to an album is followed to the release group it names, by an identifier, never by its
  title.** `AlbumLink::read` answers `Release` and `Group` for a `musicbrainz.org/release/<mbid>` or
  `/release-group/<mbid>` link, `Deezer` for `deezer.com/[lang/]album/<n>`, and `Elsewhere` for an
  album page on Spotify (or `spotify:album:`), TIDAL, Apple Music (an `album` path with no `i=`),
  YouTube Music (a `playlist?list=OLAK5uy_…`, its album playlists), Amazon Music (`albums/<asin>` with
  no `trackAsin`) or album.link itself; `FollowedLink::read` tries a song first and an album second,
  and `is_a_followed_link` is the window's test. `Library::follow_album_link` settles a `NamedAlbum`
  — the group, the release where one was named, title and credit — through `linked::album_named`: a
  group link asks `Reference::release_group`, a release link `Reference::release` and takes its group;
  anything else asks `Reference::album_linked` for the `Barcode`s the album is sold under — the
  service's own UPC, then its Deezer twin's — and each in turn of `Reference::releases_by_barcode`,
  `the_release_barcoded` taking a release only where its barcode is that code (`Barcode::names`:
  digits alike once leading zeros are off, a UPC-A and its EAN-13 being one GTIN) and only where every
  release so barcoded sits in one group — a code two groups share names neither. A `Barcode` is eight
  to fourteen digits and nothing else. Where an album the catalog holds a track of carries that group
  or release (`album_held`), the answer is `Linked::HeldAlbum` with the album, its billed title and
  owner, and how many of its release rows have no file (`missing`); otherwise `Linked::Album` with
  the group, which the window hands to `want_album` (above) — the songs read off the pressing most of
  the group's pressings share, not necessarily the one the barcode named. An artist or a playlist
  link is still words to search.
  `a_link_to_an_album_is_followed_by_its_barcode_to_the_release_group_to_want`,
  `an_album_link_whose_codes_name_no_release_or_several_groups_names_nothing` and
  `a_link_to_an_album_the_library_holds_answers_the_album_and_how_many_songs_it_lacks` are the claims.
- **A lone word is ranked; a denied or alternated one is looked up.** The unnegated, unalternated words
  are what `indexed` folds into the single FTS5 `MATCH` the index is joined for, so `rank` and
  `SortOrder::Relevance` mean what they always did. Any other word reaches the `WHERE` as
  `INDEX_LOOKUP`, a subquery against the same index scoring nothing, so a search whose every word is
  denied or alternated has no join and relevance falls back to album order. A denial is written
  `NOT coalesce(…, 0)`, so a row unable to answer — no album for `year:`, no duration for `length:` —
  satisfies the denial rather than dropping out. `Matching::grouped` carries the lot into the album and
  artist listings, so one text narrows every browse pane.
- **Words naming a title by an artist are read as that, against the artists the catalog holds.**
  `meant.rs` is the reading. `ByArtist::read` takes words with nothing of the grammar in them — no
  field, quote or denial — split at their last standalone *by* (*You F O by stela cole*; the last,
  so *Stand by Me by Ben E. King* keeps its title) or at a dash between spaces, artist first (*Stela
  Cole - You F O*), each side holding a word. `Library::meant` weighs the artist half through
  `Spellings::artist_named` — the name the artists vocabulary holds whole, or the nearest within
  `furthest_from`'s budget, so *stella cole* is *Stela Cole* — and answers a `Meant`, the search
  `ByArtist::searched_as` writes (each title word scoped `title:`, the artist `artist:=` whole), only
  where that search holds a track; otherwise the words are searched as typed. The window reads every
  page through it (`Asked::meant` beside `Asked::text`, the text still keying what keys on what was
  typed), lights and reads what it meant, and offers the words as typed (`ui.md`). The words the
  reading leaves out of the songs asked elsewhere are the *by* and the dash: `words_asked` takes the
  title and the artist, so `songs_kept_for`, `still_answering` and `unheld_matching` never look for
  a song called *by*, and `songs_asked` answers a `SongsAsked` carrying the reading beside the words
  (`online.md`). **What MusicBrainz answers for it is weighed here**: `weighed_for` keeps the matches
  whose credit is nearest the typed artist — the same letters, then one name holding the other, the
  letters folded and everything but letters and digits dropped — and of those the titles matching
  the typed title the same way, the same letters first; where no title matches, every song of the
  artist is kept, the title perhaps misremembered. The window weighs an answer before keeping it and
  `Library::found_elsewhere` before weighing it against the catalog
  (`songs_asked_for_by_an_artist_keep_the_nearest_artist_and_the_titles_that_match`).
  `a_title_by_an_artist_is_read_as_that_title_by_the_artist_the_catalog_holds` is the claim.
- **A search that matched nothing is answered in the catalog's own spelling, and only then is the
  catalog read for one.** `spelling.rs` is the whole of it. `Spellings` is four `Vocabulary`s —
  titles, artists, albums and genres, the searchable `tracks_fts` columns bar lyrics — each mapping a
  folded word to the spelling it is drawn in and how many rows hold it, filled by `store::spellings`
  from `tracks.title`, `tracks.artist`, `artists.name`, `albums.title`, `tracks.genre` and
  `artist_genres.name` through the `search::runs_in` splitting that lights a matched run, so a
  vocabulary word is exactly a word a search could match. `Spellings::did_you_mean` walks the parsed
  `Search` and corrects words in place, so terms, denials, phrases and scopes all survive: `year:1973`
  passes untouched, `-floid` is left alone (a denial is not what a listener mistyped), and `title:`
  weighs its word against the titles alone. What comes back is the whole query written again through
  `Display for Search`, the rendering the *Reads* chips draw, so what a press puts in the box reads as
  what the pane already said.
- **A word is corrected only where the catalog cannot already match it, and never further than it can
  afford.** `Vocabulary::holds` passes over a run the index would find — one held outright *or* one
  beginning a held word, a bare word matching as a prefix — so *floy* is not corrected to *Floyd* while
  *floid* is. `furthest_from` is the budget: nothing under four letters is corrected, a word up to seven
  (`ONE_LETTER_WRONG_UNTIL`) may be one letter wrong and a longer one two (`FURTHEST`), and `apart_by` is
  a bounded optimal-string-alignment distance, so two letters typed the wrong way round cost one, not
  two — most mistypings. The nearest wins, then the word most rows hold, then the spelling itself, so the
  answer is the same twice running; among spellings of one word the marked one is drawn, as
  `folded_letters` does for an artist. A suggestion is only ever a held word, so it cannot send a
  listener to a search matching nothing in turn.
- **A run-together and a split are corrections too, and neither invents a word.** `Spellings::whole` is
  the *exact* reading of a vocabulary — the entry a folded run names, not the nearest — and both rest on
  it: `run_together` answers where two runs joined name one held word and the two apart do not both name
  one, and `split_apart` where one run cuts into two that each do. So *pinkfloyd* is **Pink Floyd** and
  *pink floy d* is **pink Floyd**, where a word at a time could read neither — `floy` being a prefix
  `Vocabulary::holds` passes over, and `pinkfloyd` four letters from anything. The three readings are
  weighed in order: the join first, explaining two runs where the others explain one; the ordinary
  nearest word second, a mistyped letter being commoner; the split last. A split cuts into as many
  pieces as needed, up to `MOST_PIECES` (4), as a walk rather than a scan of cuts: `reached[to][pieces]`
  carries the most rows a segmentation of the first `to` letters into that many held words can hold, so
  what is written is the fewest pieces the whole run cuts into and, among those, the most rows —
  *thegreatgig* is **the Great Gig**, beyond a single cut, neither *thegreat* nor *greatgig* being held.
  It is refused below the length `furthest_from` refuses to correct at, one rule bounding both.
  `run_tokens_together` is the join spanning two *tokens* rather than two runs of one word, *pink floy
  d* being three clauses: it takes only a clause that is a single plain undenied word of one run scoped
  as its neighbour, leaving phrases, denials and alternations alone. A split writes two words where
  there was one, reading back as two words — unless the word was scoped, where it is written as a phrase
  so `artist:` reaches both halves. `worth_asking` weighs an adjacent pair joined as well as each run
  alone, so *flo yd* is worth the read *flo* and *yd* are not.
- **A vocabulary is indexed for the catalog the scan is written for, and the cost is measured.** A
  `Vocabulary` holds its words and names each as a `Held`: the entries keyed by an `Arc<str>`, the same
  keys bucketed by letter count, and — built when a prefix is first asked and dropped by the next
  `take` — the keys in order. `nearest_in` weighs only the buckets within `furthest` letters of the run,
  and within them only a key whose `Signature` — its set of letters folded into 32 bits — differs from
  the run's by at most two bits an edit (which no edit can exceed, a substitution taking one letter out
  and putting one in), so the bounded distance runs on the few keys that could be near;
  `edits_between` works on bytes where both are ASCII and on the stack below 64 letters, allocating
  nothing either way. `holds` and `names` read the ordered keys by binary search. `cargo bench -p
  resonate-library --bench spelling` builds the vocabulary of 500 000 tracks, 50 000 artists and 60 000
  albums from synthetic words and times what a listener asks: the build takes ~700 ms, a held word or
  name answers in microseconds, a word a letter pair away in 4 ms, a query like nothing held in under
  1 ms — where the walk over every entry with an allocating distance took 20 and 127 ms — and a
  completion about 1 ms.
- **A phrase is weighed against a whole name before it is corrected a word at a time.** `Vocabulary`
  holds the names beside the words — every title, artist and album of more than one run, keyed by its
  runs folded and joined by a space, spelt as `better_spelt` picks for a word — and
  `instead_of_the_whole` is the reading: a quoted token is weighed against them first, under the budget
  its whole length earns from `furthest_from`, and only where nothing is near does the run-by-run walk
  take over. That reaches what a word at a time cannot: *"the great gig in teh sky"* is **The Great Gig
  in the Sky** though `teh` is three letters and nothing that short is corrected alone. `names` is
  `holds`'s counterpart and guards it likewise, so a phrase *beginning* a held name is left alone.
- **A run of tokens is weighed as a name too, only where a word in it is beyond correcting.** Quoting
  is how a listener says *this is one name*, and most do not: the same title unquoted is several
  clauses, so `name_the_tokens` gathers a run of adjacent clauses each a single plain undenied word of
  one run scoped alike — `run_tokens_together`'s reading, through the same `one_run_of` — joins them
  with a space and hands the result to `instead_of_the_name`, `instead_of_the_whole`'s second half and
  the one place a name is weighed. It shrinks from the longest run down to two, so the most specific
  reading wins and a token beside the name is left rather than drawn in; a term, a denial or a change of
  scope ends the run, and `MOST_TOKENS_IN_A_NAME` bounds its length. What keeps it from overreaching is
  `beyond_a_word`: a run is weighed as a name only where one of its tokens is a word the catalog
  neither holds nor can spell nearer — exactly what a word at a time cannot reach. So *the great gig in
  teh sky* is **The Great Gig in the Sky** and *pink floid* is still **pink Floyd**, not *Pink Floyd* —
  the narrower fix stands wherever enough, and the catalog's casing is not written over a word typed
  right. The name is written back unquoted, so what is offered means what was typed — a run of words
  the search still ANDs, spelling put right — unless the run was scoped, where it is written as a phrase
  as a split is, `artist:Pink Floyd` reading back as `artist:Pink` and a loose `Floyd` over every field.
  The budget `furthest_from` answers is weighed in letters through `letters_in`, not bytes, so a folded
  Cyrillic or CJK word earns what a Latin one of the same length does and *мир* is not offered as *мор*.
- **The read is the cost, so it is paid once and kept until a name could have moved.**
  `Library::did_you_mean` walks every title, artist and album name, which is why the window asks only
  where albums, artists and tracks all came back empty — `browsed` reads them first and asks after, on
  the background executor with the rest of the load — and why `spelling::worth_asking` refuses before
  the read wherever the query holds no word long enough to correct. What the read built is kept:
  `Inner::vocabulary` stamps the `Spellings` with a counter and hands back an `Arc` of it until the
  counter moves, so typing past the end of what is held pays one read, not one per settled keystroke.
  **SQLite itself moves the counter.** `watch_the_names` lays `NAMES_MOVED_TRIGGERS` on the writer —
  `TEMP` triggers, living on that connection alone and never reaching the schema, a migration or another
  program — stepping a row of a `TEMP` table wherever a row of `tracks`, `albums`, `artists` or
  `artist_genres` is inserted or deleted, or a column the vocabulary reads — a track's title, artist
  and genre, an album's title, an artist's name — takes another value; an `update_hook` on the writer
  sees that temporary row move and steps the counter. The invalidation is thus derived from what was
  written rather than from a list of write paths to keep in step — a pass written tomorrow is covered by
  using the writer at all — and is per *column*: a counted play, a favourite, a scan writing a title back
  unchanged drop nothing. The update hook answers per table and row, SQLite offering per column only
  through `sqlite3_preupdate_hook`, a compile-time flag on the bundled library — why the triggers weigh
  and the hook only counts; they cost ~0.35 µs a row of a scan's inserts. Another process's writer this
  one cannot see; that half is `PRAGMA data_version`, read off the writer connection beside the counter
  into one `CatalogStamp { named, written_elsewhere }` (`Inner::names_stamp`): it moves only for a commit
  some *other* connection made, so a `resonate scan` beside a window drops the window's vocabulary the
  next time it is asked for, this process's own writes staying the hook's alone. It is read under
  `try_lock`, the writer possibly held through a whole scan batch and a search not to wait behind one; a
  busy writer is this process writing, and the stamp is then weighed by the counter alone. Every delete
  on the three tables carries a `WHERE`, so the truncate optimisation — the one case SQLite skips the hook
  for — cannot arise. `Library::written_elsewhere` hands the same reading out as a `WrittenElsewhere`,
  what the window watches to see another process's edits (`ui.md`).

## Playlists

- **A playlist is a list of cuts, not library rows.** `Cut` is a `MediaLocation` and its
  `Option<FrameSpan>`, what a playable item is everywhere else — `Track`, `QueueItem` and `Resumable`
  all carry the pair — and `playlist_entries` stores it as `path`, `span_start` and `span_frames` under
  `tracks`' and `resume_rows`' `store::span` convention. A file no scan has seen is still a row, and a
  file leaving the library leaves its playlists named but unresolved. Reading one back is a `LEFT JOIN`
  onto `tracks` on `path` *and* `span_start`: a `PlaylistEntry` carries its `Cut` always and its
  `Track` only where the catalog has one — the queue pane's fallback for an unscanned row.
- **A row cut from a file is the cut, not the file, all the way to the graph.** The join once took the
  path's lowest `span_start`, so the three rows a `.cue` cuts from one FLAC drew as the first, counted
  its length thrice and played the whole file; `Held` and `Reaching` carried bare locations, so a drag
  lost the span before the edit did, and both `queue_items` built `span: None` even where the joined
  `Track` had one. `Cut::of` is the one turn from a catalog row into a queueable cut and `Cut::whole` is
  what a sheet's path, a command-line file and a bus `AddTrack` all are, none of the three formats having
  a vocabulary for a region. Folding doubles reads the whole cut, not the path, so two cuts of one file
  are two rows while one cut twice is one.
  `a_cue_row_put_in_a_playlist_is_the_cut_it_was_rather_than_the_file_it_came_out_of`,
  `the_rows_a_sheet_cut_are_a_playlist_of_their_own_lengths` and
  `two_rows_one_sheet_cut_out_of_a_file_are_not_doubles_of_each_other` are the claims. An edit writes
  only the rows it moved: appending goes at `max(position) + 1`, removing shifts the rows after it,
  moving shifts the span between the two ends, and tidying rewrites from the first row whose file has
  gone. A shift parks rows at `-1 - position` and unparks them in a second statement, since
  `PRIMARY KEY (playlist_id, position)` refuses even a transient collision and SQLite promises no order
  an `UPDATE` visits rows in. `position` stays dense, so a row's index and position are one number —
  why every span is weighed against the list before being written rather than cast to an `i64` and
  trusted: `move_in_playlist` answers false where either end of the span or the row dropped on is past
  the playlist's end, and `remove_from_playlist` takes rows from the first named to the list's end and
  refuses a span starting past it. A span of any length costs one row's two writes — the whole reason
  `Span` reaches the SQL rather than the pane sending an edit per row. The restore point `undo::edited`
  takes before each reads only the rows the edit can reach (`Undo` below), so an append to a list in
  hand reads none, removing a span reads that span and moving one the rows it crosses. Only the local
  source can be in one — `add_to_playlist` refuses a `MediaLocation` naming another.
- **A name is one name however written, and the column says so.** `playlists.folded` holds
  `playlist::folded` — Rust's `to_lowercase`, folding every alphabet rather than SQLite's `NOCASE`
  ASCII, then NFC, folding one letter's spellings into each other — and carries the `UNIQUE`, so two
  spellings of one name cannot both be in the table whatever `refuse_duplicate` does.
  `Library::playlist_named`, `HOLDS_THE_WORD` and `PlaylistOrder::Name` read that column, so addressing,
  narrowing and ordering fold alike — what lets `resonate playlist <NAME>` and the pane address a
  playlist by name. Lowercase then compose, `to_lowercase` not promising composed output; one function
  applied to what is stored and what is asked, so the two cannot disagree. NFC, not NFKC: "Café" typed
  with a combining acute is the playlist typed with a precomposed one, while ﬁ and fi stay two names,
  folding a ligature being a different claim. The column is written with the row, so a database from a
  build before the fold changed keeps its folds — the delete-and-rescan the schema note assumes.
  `Library::rename_playlist` refuses a spelling another playlist holds through `refuse_duplicate`, which
  excludes the playlist renamed, so a re-cased name is not a duplicate of itself. Renaming and
  discarding are not row edits, so a saved query takes both as a list does; `resonate playlist <NAME>
  --rename <NEW>` and `--discard` are the command line's gestures, with no undo behind them.
- **Every row edit refuses inside the transaction writing it.** `only_a_list` asks whether the
  playlist is there and whether it fills itself from a query against the transaction, not a reader
  connection, so `Error::UnknownPlaylist` and `Error::NotAList` cannot be answered from a state the write
  no longer sees. The passes dropping rows share the discipline: `Going` is the question — `Gone`,
  `Doubled`, `Unwanted` (gone or doubled, for a tidy) or `Matching` — and `asked_of` asks it of the rows
  `numbered` read inside that transaction, so no closure names a row it never looked at. The filesystem
  is the one thing not asked inside it: `gone_from` stats each distinct path first, through a reader, so
  a slow or downed mount holds no write lock, and `Gone` and `Unwanted` carry the paths it found gone — a
  row landing between the two reads was never weighed and stays. A refused edit still pays the restore
  point `undo::edited` takes first. `copy_playlist` is the exception by design: it reads the source
  outside the transaction it lands in, a copy being the source as it stood.
- **A playlist holds a list or fills itself from a query, and both answer through
  `playlist_entries`.** A row in `playlist_queries` makes one a saved query: the search text, the
  `SortOrder` and the row cap — a `TrackQuery` without the album and artist. `Library::playlist_entries`
  runs it rather than reading `playlist_entries` where one exists, so the queue, the pane, MPRIS and
  every sheet writer take a query playlist for an ordinary one. A listing's counts are the query's too,
  one `count(*)` per saved query on top of the grouped pass. Editing rows is refused — `Error::NotAList`
  from `add`, `remove_rows`, `move_rows` and `prune` — there being no row to move; renaming, playing,
  exporting and dropping work as for a list. A query has `Library::revise_query` instead, rewriting those
  three columns in place and refusing a list with `Error::NotAQuery`, the mirror of `NotAList`, so each
  kind refuses exactly the edit the other takes. A query holds no album or artist id, so saving one while
  an album is selected cannot quietly widen to the library: the window offers *Save this search* only
  where the search box scoped the pane, and `resonate playlist <NAME> --query` saves one under an unused
  name and revises the one named. A revision is the whole edit: nothing keeps what a query held before.
  What a query holds is read when asked, so it can answer differently twice running, and a queue loaded
  from one is a snapshot — nothing re-reads a query into a queue playing it. `SortOrder::Plays` or
  `Played` beside a cap writes a *Top 25 most played*, and `plays:0` is what was never heard.
  `Library::playlist_lists` is the read for the one gesture reaching only the other kind — the window's
  picker, putting rows in a list — the listing's grouped pass with queries taken out in SQL and no
  `count(*)`.
- **A list is put in order by the library, not a row at a time by the pane.** `Library::sort_playlist`
  takes a `RowOrder` and a `Direction` and rewrites positions in place — one `DELETE` and a dense
  reinsert from the first row the order moves, not a move's park-and-unpark pair — answering how many
  rows moved, so a list already in that order costs no write and moves no revision. `RowOrder::Album`,
  `Artist`, `Title` and `Length` read the catalog through the tracks pane's order columns and put an
  unscanned row at the end whichever way the order reads; `RowOrder::File` reads the path every row
  carries, the one order placing unscanned rows too. `Length` is seconds, not frames, files at different
  rates counting time differently. A saved query refuses it with `Error::NotAList`, its order being the
  query's. It is an edit, not a property, so a list in hand still puts the next appended row at the end;
  `Kept` below makes an order a property. The pane reaches it through the opened playlist's *Sort*
  control, which opens the index's *In order* and *Reading* chips, and `resonate playlist <NAME> --order
  <ORDER> [--reverse]` is the command line's gesture.
- **A list is in hand or kept in an order, and a kept one refuses edits placing a row by hand.**
  `Library::keep_playlist_in_order` takes an `Option<Kept>` — a `RowOrder` and a `Direction` — writes it
  to `playlists.kept_order` and `kept_reading` and puts the rows in it in the same transaction, so
  keeping sorts once and then holds. `playlist::add` runs the same pass after appending, landing a row
  added by hand, by `resonate playlist --add` or by an imported sheet where the order says. A kept list
  refuses `move_in_playlist` and `sort_playlist` with `Error::KeptInOrder` — the mirror of `NotAList`,
  its order being the sort's — while removing, tidying, renaming, exporting and adding work as for a list
  in hand. `keep_playlist_in_order(id, None)` puts one back in hand, leaving rows where they stand. A
  saved query refuses to be kept at all, with `NotAList`. The window reads the states through
  `views/playlists.rs::Rows`: `InHand` is moved and edited, `Kept` edited not moved, `Matched` neither, so
  the movers, the drag, the reach and the ✕ each ask their one question rather than testing for a query.
  The *Sort* control carries the choice as a third chip row, *Keeps*, and the index draws a kept list
  under the sort mark; `resonate playlist <NAME> --order <ORDER> --keep` and `--by-hand` are the command
  line's gestures. Keeping a list in its kept order, or putting one in hand back in hand, moves nothing
  and writes nothing (`Change::Nothing`, as every edit writing nothing answers). Keeping costs the whole
  list read on every append — the order re-read and rows rewritten from the first it moves — so a row
  sorting to the end of a kept list of a thousand costs one write and one sorting to its top a thousand.
  The order is re-read only when the list is written, so a scan renaming a track or filling its album
  leaves a kept list in its order — nothing re-sorts one on its own.
- **What a search showed is what a drop removes, and nothing empties a list in one gesture.**
  `Library::remove_matching` reads the positions a search matched and rewrites the list from the first
  row that goes — `prune_playlist`'s pass; they share `dropped_where`, a narrowing breaking the adjacency
  a `Span` needs and both being one question asked of every row. A saved query refuses it with
  `NotAList`; a kept list takes it, dropping placing none. It asks for the text, not an `Option<&str>` as
  `copy_playlist` does, so no call empties a list: an emptied one is one to discard. The window draws
  *Drop shown* beside *Copy* only under `Rows::Narrowed`, and `resonate playlist <NAME> --matching <TEXT>
  --drop` is the command line's gesture, hence `--drop` requires `--matching`. A narrowing a search
  reads nothing in — `???`, `!!!`, punctuation the fold drops — matches nothing rather than standing
  for no condition (`db::asks_for_nothing_it_can_read`), so `--matching '???'` plays, copies and drops
  no row where it once took the whole list; a blank one is still no narrowing
  (`a_narrowing_with_nothing_a_search_reads_matches_nothing_rather_than_everything`).
- **A doubled row is folded away, the first of each staying.** `Library::fold_doubles` reads the cuts
  a list holds and drops every row naming a cut (path and span) an earlier row already named, through
  the same `dropped_where` pass. It is the companion of `copy_playlist`, which reconciles nothing and so
  doubles what it lands, and of `add_to_playlist`, which lets a file be put in twice deliberately:
  nothing folds on its own, a track twice in a playlist being a thing to be able to do. It reconciles on
  the path as `import_playlist` does, so two names for one file are two rows and stay two. A saved query
  refuses it with `NotAList`; a kept list takes it. `Library::tidy_playlist` asks both whether a file is
  gone and whether an earlier live row already names it (`Going::Unwanted`), dropping the union in one
  `Edit::Tidied` step; missing rows do not make later live rows look doubled. The opened playlist draws
  that combined action as *Tidy*, so one undo restores both kinds. The command line keeps `resonate
  playlist <NAME> --tidy` for missing files and `--fold` for doubles, refusing them together as they ask
  different questions. One press for the whole list — nothing folds out of a span, and nothing previews
  which rows a press would take.
- **A playlist is copied into another, never moved into it.** `Library::copy_playlist` reads one
  playlist's rows — all, or what a search matched — and lands them through `add_to_playlist`, so the
  source keeps them, a kept target puts them in its order, and a saved query refuses them with
  `NotAList`. `Error::IntoItself` refuses both sides being one playlist, which would only double it.
  Copying out of a query freezes a search into a list; the command line remains the way to copy one. The
  playlist index and opened playlist use + to enter a track-browsing mode for the chosen target, while
  row-level + actions already holding tracks open `hold_for_a_playlist`. `resonate playlist <NAME>
  --into <OTHER>` copies, creating OTHER where nothing is named so. It costs the whole source read into
  memory as locations and a write per row, plus the order re-read where the target is kept.
- **A playlist is duplicated whole, and a duplicate is one step to walk back.**
  `Library::duplicate_playlist` makes a playlist under the first free of *NAME (copy)*, *NAME (copy 2)*
  and on, in one `undo::started_under_a_name_found` step — the source's name read and the free one
  weighed inside the write that takes it, so two processes duplicating one playlist at once each land
  under a name of their own rather than one refused as `DuplicatePlaylist`
  (`two_catalogs_duplicating_one_playlist_at_once_each_find_a_free_name`): a list's rows are copied as
  stored, spans and all, with its kept order, and a saved query gets the same search, order and cap
  rather than being frozen into its current rows (`copy_playlist` out of a query does that). The pin,
  the plays and when played stay with the source. The window offers *Duplicate* in the menu of a
  playlist card, a playlist row and the opened playlist's more mark.
  `a_duplicated_playlist_holds_what_the_source_holds_under_a_free_name` and
  `a_duplicated_playlist_keeps_the_order_or_the_search_the_source_had` are the claims.
- **What pictures a playlist is the covered albums its rows reach first.**
  `Library::playlist_pictures` answers at most the covers asked for, one per distinct picture through
  `pictured_by`'s identity and likeness. A list is read in its own order, so the mosaic is the playlist's
  opening; a saved query with no cap is `pictured_by` over its search, and one with a cap reads the rows
  the cap leaves, so *Top 25* is pictured by those twenty-five. `Library::pinned_playlists` is the other
  sidebar read: the pinned playlists' ids and names, most lately pinned first, no join, no narrowing.
- **A search narrows a playlist, and what is shown is what plays.** `Library::playlists` takes a
  search's words and matches each against the name, a playlist having only a name to answer with: a term
  is passed over, and a search with no standalone word leaves the index whole.
  `Library::playlist_entries` runs the whole text against the catalog, so an unscanned row answers
  nothing and falls out, and a saved query's own text and the typed one must both hold. A
  `PlaylistEntry` carries its `position` for that reason — a narrowed row knows where it stands, so the
  number it draws and the row a ✕ drops are the list's, not the view's. `Rows::Narrowed` sits beside
  `InHand`, `Kept` and `Matched`: edited, not reached and not moved, a reach being a run of adjacent rows
  and a `Span` what reaches the SQL, adjacency being what the narrowing broke. What acts on the playlist
  whole — Rename, Sort, Tidy, Export, Discard — acts on the whole whatever is shown; what acts on rows —
  Play, Play next, Add to queue, Copy, Drop shown and every row gesture — acts on what is shown, so
  playing a narrowed playlist leaves `Library::playing_playlist` unset, the queue being part of it rather
  than it. `resonate playlists --named` and `resonate playlist <NAME> --matching` are the command line's
  gestures. A saved query's text and the typed one are read *beside* each other, not joined into one
  string: `db::matching` takes the texts, reads each through `Search::read` and asks their clauses
  together, so an unbalanced quote closes at the end of the text it was written in and a trailing `or`
  stays a word. Both run under the query's own cap, so the cap falls on what the two match, and the
  listing's count is still the query's. Nothing says which of the two a row answered, and the same box
  *revises* a query through *Edit search*, so the text narrowing one and the text defining it are one
  field read two ways.
- **A playlist listing is an order and a `Direction`, and whoever draws it picks both.**
  `Library::playlists` takes the two and `order_by` writes the sense into the SQL, so a reading is the
  query's, not a `reverse()` over what came back — leaving a never-played playlist at the bottom of *Most
  recent first* and the top of *Longest ago first*, where SQLite puts a NULL. `PlaylistOrder::reads` is
  the direction an order *opens* at: ascending for `Name`, descending for the other five, a person
  expecting the newest or most played on top. A default, not a rule — the pane's *Reading* row and
  `resonate playlists --reverse` each turn one around, and choosing an order resets the reading to what
  it opens at. The bus opens ascending whatever the order, the MPRIS Playlists spec defining
  `CreationDate`, `ModifiedDate` and `LastPlayDate` as oldest first, and maps its own `reverseOrder` onto
  `Direction::Descending` rather than reversing a list already read.
- **Which playlist is in play is the library's, kept by the queue it was loaded as.**
  `Library::playing_playlist` is a cell beside the catalog, not a column, holding a `Playing` — the
  `PlaylistId` and the `QueueStamp` `engine::stamp_of` reads off the rows the queue was loaded with,
  their locations and spans rather than the ids handed. It answers `Library::playing_playlist(queue)`
  only where the two stamps agree, so the cell *is* the claim that the queue is that playlist, not a note
  every caller must remember to tear up. `Queue::rows_changed` restamps whenever a row arrives or leaves
  and the engine publishes it as `PlayerState::queue_stamp`, so an `Insert` or `Remove` takes the badge
  off from wherever it came — `RootView::queue`, a row's ✕, or `AddTrack` and `RemoveTrack` over the bus
  with no window between. `Queue::split` restamps too (through `stamp_what_is_playing_from`), a drag
  crossing into or out of the rows queued next changing which rows the queue plays from, and a drag back
  restores its stamp. A reorder keeping the same rows in `order` stamps the same, the stamp being read off
  `items`, not `order`, so a queue move and a shuffle leave the playlist in play and
  `PlayerState::loaded_position` still names the playlist row heard. Three callers set it, each stamping
  the items it is about to send: `RootView::play_playlist`, `Collection::activate` and the binary's
  `play_queue`. MPRIS's `ActivePlaylist` reads the same cell through the same stamp, so bus and pane
  cannot disagree. `QueueItem` derives no `Hash`, so `engine::stamp_of` is the only way a queue's rows are
  stamped: `Collection::activate` once hashed whole items, ids and all, which no published queue could
  match, and `ActivePlaylist` never named the playlist the bus had just activated. It holds only a
  playlist the catalog holds, `set_playing_playlist` filling it from whether `played_now` counted the play
  (`counted_a_play`): an id nothing holds leaves the cell and `ActivePlaylist` empty rather than naming an
  absent playlist. Not persisted, as the queue is not. *When* one was last played is, and how often: the
  same call writes `playlists.played` and steps `playlists.plays`, what `PlaylistOrder::Played` and
  `Plays` order on and the bus answers `LastPlayDate` from. Loading one counts, so loading it twice
  counts twice. `Library::playlists_revision` is the companion cell, bumped by every playlist write, which
  a 200 ms bus poll watches so a listing is re-read only when an edit moved it. The stamp costs a poll:
  the engine publishes it only once it has applied the load, so the badge arrives a poll behind the
  queue. It is a 64-bit hash of the rows in loaded order, so two colliding queues are one queue to it and
  a queue edited back to what it was is the playlist again.

## Undo

- **A playlist edit is one step to walk back, and a step is the playlist as it stood.** `undo.rs` is
  the whole of it. `undo::edited` wraps every mutator's transaction: it reads the playlist's name, kept
  order, saved query and the rows its `Reach` names before the change and keeps that restore point only
  where the change moved something (`Change::Made` and `Change::Nothing`) — an edit writing nothing
  leaves nothing to put back.
  `undo::started` is the other half, for gestures creating a playlist, whose restore point is its not
  existing. `Library::undo` takes the newest step and writes it back — a whole one as one `DELETE` of the
  playlist row, cascading its entries and query away, then a dense rewrite — so a discarded playlist
  returns under its id, while `played` and `plays` are read off the row rather than restored, a play
  not being an edit.
  Why `Library::start_playlist` and `Library::revise_query` are single calls: the window's gesture is
  name-and-rows and name-and-search, and two library calls would be two steps. The stack is
  `Inner::steps`, bounded at 32 steps (`STEPS_HELD`) and 50 000 rows (`ROWS_HELD`) and living only as
  long as the run, so the command line has no undo. The newest step is always kept, so a long run drops
  the oldest, not the largest. A play counted between an edit and its undo is not taken back — `played`
  and `plays` are read off the row — but the date last changed is, so an edit walked back does not leave
  it climbing a *Last changed* listing.
- **A step holds only the rows its edit could have moved.** The mutator names a `Reach` and
  `Reach::settled` turns it into the step's `Reached` inside the transaction: `Unmoved` for a rename and
  a revision, which read no path under the playlist and count none against the stack's 50 000; `From`
  the list's length for an append (`Reach::Appended`), so one row added to a list of 100 000 holds
  none; `Window` the run of rows an edit replaced — `Reach::Emptied` for a removed span, which leaves
  nothing where it stood, and `Reach::Shuffled` for a move, the span and the row it lands on bounding
  the rows it permutes, as many after as before — clipped to the list by `clipped` in signed
  arithmetic, a span may run to `usize::MAX`; and `Whole` for a discard, a sort, a keep, the passes
  dropping rows and an append to a kept list, whose re-sort may move any row. An `Unmoved` step is put
  back by `written_over`, an `UPDATE` of the playlist row and a rewrite of its query where it has one;
  a `From` step by `written_over` and then `rewritten_from`, deleting every row from its first on and
  writing the held tail back from wherever the list now ends; a `Window` step by `written_over` and
  then `rewritten_within`, deleting the `leaves` rows standing in the window, moving the rows after it
  by the difference through `closed_up`'s park-and-unpark, and writing the held `holds` rows back into
  the gap, so the positions stay dense; only a `Whole` step takes the `DELETE` that cascades the
  entries away — which is why that path is the one reading `played` and `plays` back off the row it
  is about to delete. `undo::walk` reads the standing it overwrites through `Reached::turned` — a
  window's two lengths traded, what the step leaves being what its inverse holds — so the inverse
  holds the same stretch the step did, and a created playlist's step is `Whole`, its inverse being the
  playlist whole. A move from the top of a long list to its end still crosses the whole of it.
  `an_edit_to_a_long_playlist_holds_only_the_rows_from_where_it_reached` and
  `an_edit_near_the_top_of_a_long_playlist_holds_only_the_rows_it_crossed` are the claims.
- **A step is walked either way, and walking it writes the step back the other way.** `undo::walk` is
  `Library::undo` and `Library::redo` alike: it pops a step off one stack, reads the standing it is
  about to overwrite, applies the step and pushes what it read onto the other — so `Inner::walked` holds
  the playlist as each edit *left* it, no step carries an inverse, and a redone step is one to walk back
  again. A step finding no playlist under its id is `Standing::Fresh`, how a discard and a create are
  one shape read from opposite ends; walking back one that discards the playlist in play clears
  `playing_playlist` with it. The next edit ends a redo: `undo::note` clears `walked` before keeping a
  step, so an edit after a walk back forgets what was walked, while one writing nothing clears nothing,
  never reaching `note`. That also keeps the id safe: undo is strictly last-first, so every gesture
  freeing an id leaves a step under every gesture taking one, and every id-taking gesture is an edit, so
  a step on `walked` can never name a playlist SQLite has since handed the id to. `RootView::undo_edit`
  and `RootView::redo_edit` are the playlists headings' *Undo* and *Redo*, and `ctrl-z`, `ctrl-shift-z`
  and `ctrl-y` away from the search field, where the field's own three are bound inside it.
  `Inner::walked` is bounded like `steps` — 32 steps or 50 000 rows — but as a second bound, not a
  shared one, so a long run of undos over long playlists can hold both ends of each, and nothing
  collapses a step walked back and forth into the one it came from: a press either way costs the edit's
  read and rewrite.

## Sheets

- **A playlist leaves and arrives as M3U, PLS or XSPF, and the sheet names files, not tracks.**
  `sheet.rs` is the seam and `Library::import_playlist` / `export_playlist` the way in: it reads the
  bytes, settles the encoding, decides which of the three the *text* is and hands `m3u.rs`, `pls.rs` or
  `xspf.rs` the job; `sheet::parse`, split from `sheet::read`, reads a sheet with no file. What the
  three share lives there once — a row's seconds, artist and title as a `Described`, the
  relative-or-absolute path rule, percent escaping both ways, and the staged rename over the target that
  stops a crash mid-write truncating a sheet. The stage is a hidden sibling of its own —
  `.<name>.<pid>-<n>.new` (`staging_name_for`), made `create_new` so it can never be a file of the
  listener's or another export's — written and `sync_all`ed before the rename, and removed wherever the
  write or the rename failed, so a refused export leaves nothing beside its target
  (`an_export_that_fails_leaves_nothing_staged_beside_its_target`). Writing picks the format from the
  target's extension, M3U
  where the name declares nothing; reading picks it from the content, so a sheet under the wrong
  extension still reads. Everything a sheet says about a *track* is read past — a `playlist_entries` row
  is a path, so `#EXTINF:`, `TitleN`/`LengthN` and `<title>`/`<creator>`/`<duration>` are written and
  never believed. A row naming a scheme not `file://` is counted rather than refused, one stream in a
  sheet not being allowed to cost the other fifty rows. The scheme and a `localhost` authority are read
  in any case, as RFC 3986 has them, so `FILE:///a.wav` and `file://LocalHost/a.wav` are local rows and
  an `xml:base` written either way still resolves what sits under it; `file:/a.wav`, with no authority,
  reads as `file:///a.wav` — its query and fragment cut off as they are after an authority — while `file:track.flac`, with no slash, stays a relative path. A row empty
  once trimmed — a PLS `File1=` with nothing after — names nothing and is counted as elsewhere, where it
  once resolved to the sheet's own folder and was stored. A sheet is read as its byte-order mark says —
  UTF-8 or UTF-16 either way round, one of odd length refused — else as UTF-8 where it is, else in the
  code page `resonate_core::text` detects (`rust-style.md`'s text note), unless its name declares UTF-8
  (`.m3u8` and `.xspf` do) or its bytes hold a NUL, which no text sheet does; either refusal is
  `Error::UnreadablePlaylistFile`, and `Imported::encoding` says what it was read as. Importing reconciles by count, not set: a row the playlist holds counts as already there, so a
  sheet read twice is a no-op, while a file the *sheet itself* names twice is two rows,
  `add_to_playlist` letting one file be put in twice deliberately. A taken name is appended to rather
  than refused, making `import` and `resonate playlist <NAME> --add` one gesture. A `.cue` handed to
  `--add` is added as the rows it cuts, through the `sheet_cuts` `resonate play` and `resonate queue`
  read one through, not as a row naming the sheet. A row is written as a path only where
  `sheet::as_a_row` finds the path reads back as itself, else as an escaped `file://` URI, which carries
  a `#`, a line break or a scheme of its own through M3U and PLS. The parse settles a path lexically
  alone (`settled` in `sheet.rs`) — absolute, `.` and `..` taken out, no link followed — so it asks the
  filesystem nothing, and `playlist::cuts_of` settles each row against the catalog: the path as written
  where a track holds it, else the path under a root that a folder of it resolves to — the scan storing
  a root canonical and whatever is beneath it as walked, a followed link's own name included — else its
  canonical path; where the catalog holds none of the three the row is the canonical path, or the
  lexical one where the file is not there to resolve, so a sheet imported while a mount is down still
  names what a later scan will store. Canonicalising every row once lost a file a scan under
  `follow_symlinks` stored through its link
  (`a_sheet_naming_a_file_through_a_link_the_scan_followed_lands_on_the_catalog_row`). A row holding a
  backslash and no forward
  slash is a path a Windows player wrote, and `forward_separated` reads it with its separators turned,
  so `..\Music\01.mp3` resolves beside the sheet rather than as one oddly named file. The reverse is
  held too: `reads_back_as_itself` refuses a row `forward_separated` would turn, so our file named
  `AC\DC.wav` is written as an escaped `file://` URI rather than a row reading back as `AC/DC.wav`. What
  is escaped is not ambiguous that way: a literal backslash cannot stand in a `file://` URI or an XSPF
  location, whose writers escape one as `%5C`, so `sheet::forward_escaped` turns every literal one into
  a separator *before* unescaping — `file:///music\a.wav` and an XSPF `album\a.wav` or
  `xml:base="discs\"` name folders — while `AC%5CDC.wav` still reads back as its one file. A reference
  ends at its first raw `?` or `#`: `sheet::path_of_reference` cuts a `file:` URI and a relative XSPF
  location there before unescaping, as `MediaLocation::from_uri` reads one, so
  `file:///music/Echoes.flac#t=10` and this build's own `#frames=` URI are the file they name while
  `%23` and `%3F` still decode into it. A plain M3U or PLS row is no URI, so a `#` mid-row stays part of
  the name. The 64 MiB ceiling (`LARGEST_PLAYLIST_FILE`, some 400 000 rows) is weighed against what
  the name declares and again against what the read took, so a FIFO reporting zero is refused rather
  than read unbounded — and against what an export would write, `read_back_within` refusing a sheet
  past it as `PlaylistFileTooLarge` before a byte lands, so what this build writes it reads back
  (`a_sheet_too_large_to_import_is_refused_rather_than_exported`).
  `Library::prune_playlist` is the companion dropping rows whose files have gone, asked for rather than
  automatic. A sheet saying how many rows it holds is taken at its word then weighed — PLS's
  `NumberOfEntries` against what the text held, the shortfall carried as `Imported::short` — so a
  truncated sheet says so rather than importing quietly short. M3U and XSPF declare no count, so only a
  PLS can say it lost rows, and a sheet holding more than promised is taken whole silently. Import
  resolves and stats every row, so a sheet naming a downed network mount reports each row missing rather
  than waiting; nothing tells a file grown between the two reads from one that lied about its size.
  **A tidy drops only what is surely gone.** `gone_from` weighs a row's file gone only where the stat
  answers `NotFound` or `NotADirectory` — any other failure, a permission or an `EIO`, keeping the row —
  and the path is on no volume `volumes::is_mounted` finds unmounted and under no root whose directory
  is missing, and the nearest folder above it that stands holds something: an unmounted mount point
  reads as an empty folder or none, so a stick's rows are kept whether the catalog ever noted its volume
  or not. The price is a file deleted with the last of its folder's contents staying until the folder
  goes (`a_row_on_a_drive_that_is_not_mounted_is_kept_by_a_tidy_and_a_deleted_one_is_not`,
  `a_prune_keeps_the_rows_under_a_root_that_is_not_there`).
- **A cue row leaves as its file and the times VLC reads, and comes back as the cut.** M3U, PLS and
  XSPF name files, so a cut row is written as its file and, where the format can say it, the start and
  stop VLC honours: `#EXTVLCOPT:start-time=` and `stop-time=` lines before an M3U row, and the same two
  options as `<vlc:option>` inside a track's VLC `<extension>` in XSPF, the playlist element declaring the
  `vlc` namespace. The seconds are the cut's frames at the track's rate, rounded to the nanosecond, and
  reading back takes the rate the catalog holds for the row's path, or probes the file once an import —
  `probed_rate` keeping each file's answer, so a sheet cutting one image into twenty rows opens it once —
  and rounds again, landing on the written frame at every rate the workspace holds — a frame is never
  shorter than 1.3 µs
  (`a_timed_row_takes_its_rate_from_the_catalog_rather_than_probing_the_file`). A row whose file the
  catalog lacks and will not probe, or a PLS row (no word for a region), is the whole file, and
  `Sheet::locations` holds `Listed` rows — a location
  and its `Timed` — so the parse stays free of I/O and the fuzz target reaches it as before.
  `a_playlist_of_cue_rows_exports_and_imports_as_the_rows_it_holds` is the claim.
- **`xspf.rs` reads its own markup, and every leniency in it is deliberate.** A tag ends at the first
  `>` *outside* a quoted attribute value, so an attribute holding one does not halve the tag. `xml:base`
  is resolved down an element stack, so a relative `<location>` answers to the base in force, not always
  the sheet's folder, and a base naming a non-`file://` scheme makes every relative row under it count as
  elsewhere rather than resolve to nonsense. A track's `<location>`s are alternates, so the first naming a
  local file is the row and a track offering none costs one `elsewhere`. `<!DOCTYPE …>` and `<?…?>` are
  skipped, not pushed, an element pushed and never popped carrying its base to the rest of the sheet.
  `<album>`, `<image>`, `<annotation>` and `<meta>` stay unread on purpose: a row is a path, so a sheet
  is never a source of tags. The rest is taken on trust: an element is matched by local name, so two
  namespaces both calling something `track` are one element; it knows the five entities XML defines and a
  numeric reference — decimal, or hexadecimal after `x` or `X`, both allowed — and nothing else; and it
  trusts the nesting, so a sheet never closing an opened element carries its base to everything after.
  Writing is strict where reading is lenient: `marked_up` escapes the five entities and leaves out every
  character XML 1.0 forbids (`is_an_xml_character` — the C0 controls but tab, line feed and carriage
  return, and U+FFFE and U+FFFF), which not even a numeric reference may carry, so a title holding
  U+0001 writes a sheet VLC and Kodi read rather than one they refuse.

## The MPRIS seam

- **`resonate-mpris` reaches playlists through a seam, never the library.** `Playlists` is the trait
  `Mpris::start` takes beside `Host`, filled by the binary with a `Collection` over the `Library` and the
  `Player`. The interface is served only when one is supplied, so a run with no catalog advertises no
  `org.mpris.MediaPlayer2.Playlists` at all rather than an empty one. `Orderings` is built from
  `mpris::PlaylistOrder::ALL` — Alphabetical, CreationDate, ModifiedDate and LastPlayDate, the spec's
  four, distinct from the library's six — so an ordering the player never advertised is refused rather
  than quietly answered under another, `UserDefined` among them, a playlist's own order not being one the
  bus can ask by name. `PlaylistCount`, `ActivePlaylist` and `PlaylistChanged` are diffed from the
  player properties' 200 ms poll, against a listing re-read only when `Library::playlists_revision` moves.
  `moved` is the question the diff asks of every row: a playlist the listing held under another name
  is `PlaylistChanged`, which the spec keeps for a name or icon changed, and a playlist that arrived or
  left is `PlaylistCount` announced — even where a sample holding an addition and a removal at once
  leaves the count the same, since the spec names no signal for a playlist made or gone and a client
  learns of either by re-reading `GetPlaylists` when the count is announced. Neither of the seam's two reads is `Library::playlists` — a grouped pass
  over every row of every playlist and a `count(*)` per saved query, where the bus keeps only each row's
  id and name: `Playlists::count` is `Library::playlist_count`, one `count(*)` over `playlists`, and
  `Playlists::listing` is `Library::playlist_names`, selecting id and name with no join and taking the
  `GetPlaylists` call's `index` and `maxCount` as `OFFSET` and `LIMIT`, so a client asking for ten reads
  ten. `PlaylistCount` is read by every client and re-read on every announced change, which is why it had
  to stop measuring a listing. Both query SQLite on the bus thread, so a client asking for a listing pays
  for it there. The bus cannot ask for a narrowing, so the seam takes none.
