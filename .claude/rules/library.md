---
paths:
  - "crates/resonate-library/**/*.rs"
  - "crates/resonate-mpris/**/*.rs"
  - "crates/resonate-ui/src/views/playlists.rs"
  - "crates/resonate/src/playlists.rs"
---

# The catalog and its playlists

`resonate-library` is the local source's catalog only: walks directories, stores paths, returns
local `MediaLocation`s; nothing keyed by source. Another source brings its own catalog; its queue
rows read via `Player::media` like any unscanned row.

## Schema and grouping

- **Schema = `V1` + `MIGRATIONS`; catalogs migrate, not break.** Change = new SQL step appended
  (`ALTER TABLE`, table/index, row rewrite); never edit `V1` or a written step (strands stamped
  catalogs). `PRAGMA user_version` = `fingerprint_after`: compile-time FNV-1a over `V1` text then
  each step, so each history point has a stamp; `SCHEMA_FINGERPRINT` is the last. `lay_out`:
  `UNSTAMPED` writes `V1` + all steps; this build's stamp opens; else finds the step prefix the
  stamp names, applies the rest in one transaction restamping on commit (failing step =
  `StoreOp::Migrate`, catalog and stamp unchanged). Stamp read inside that `IMMEDIATE` transaction:
  a second process opening an old catalog waits on the write lock, reads the stamp left (else
  duplicate-column failure). Only a stamp no prefix names is `Error::SchemaMismatch` (pre-history
  catalog, or unshared history): the one case where the catalog is deleted and rescanned. Counted
  version refused (cannot tell same-count, different-schema builds), so the stamp stays a hash.
  **Migrate wherever rows can be carried**; break only for an index whose meaning changed beyond SQL
  recomputation, signalled by the step's absence, never a `V1` edit. History begins `0ca1e683`
  (`V1_AS_FIRST_STAMPED`, the `V1` at policy change), so any build since is carried forward.
  (`the_first_schema_is_never_edited_where_it_stands` pins `V1`'s fingerprint; `never_unstamped`
  keeps a zero hash from reading as unstamped;
  `a_catalog_another_process_is_migrating_is_read_once_it_has_and_not_migrated_twice`)
- **A changed probe is carried forward by marking rows.** A migration step sets `tracks.probe_again`
  on rows the new probe bills differently (first: every WavPack, `codec = 11`; hybrid was billed
  lossless until the flag was read). Incremental scan treats a marked row as changed despite unmoved
  size/mtime; upsert clears the mark. Vault link and lookup results stay (kept on unchanged
  size/mtime): re-read, not re-imported or re-asked. Later probe change = another marking step,
  never a whole-catalog rescan (one marks every ALAC, AAC, MP3 row under a root, for the counted MP3
  length and the Sound Check and LAME gains `rg_*` holds).
  (`a_row_marked_to_be_probed_again_is_read_again_though_its_file_has_not_moved`)
- **Transport state = three tables, each moving at its own rate.** `resume`: singleton (queue's row,
  frame, shuffled, taken, `next_first`/`next_last` = queued-rows span in drawn list). `resume_rows`:
  one row per *loaded*-order position. `resume_order`: one per *playing*-order position, naming the
  loaded row. `Keeping` (`resonate-engine` `kept.rs`) picks the write: unchanged queue = one upsert
  of the place; reorder/reshuffle = `resume_order`, no URI; only rows arriving/leaving rewrite
  `resume_rows`. `keep_place` writes row and frame only (never `shuffle`, order, next span), so a
  place written seconds into a track cannot lose the order the rows were kept under; `keep_order`
  writes order, place, shuffle, next span together (a toggle moves all of them). Rows carry
  `MediaLocation::to_uri`, not a path: the one non-local-source thing stored (queue rows come from
  any registered source); `Library::track_played` keys on a path (a *scanned* row).
  `span_start`/`span_frames` follow `tracks`' `store::span` convention, so a cue row resumes as its
  cut. Order lets a shuffled queue return playing as it played and unshuffle into its album;
  `resonate-core::plays_in` is all the trust: order naming each row exactly once stands, else (wrong
  length, repeat, missing row) load order, so no reading refuses a queue. A URI no longer naming a
  location drops that row alone: `resumed::Kept` holds `Option<Resumable>` rows, renumbers over the
  rest (order in play order minus dropped, kept position moved to where the same track stands,
  queued-next span closed over the gap); playing row dropped = resume at next row's start, `at`
  zeroed; only a queue with no openable row is `None`; all rows open = exactly as kept.
  `keep_resumption` replaces rows, order, place in one transaction; `resumption` reads all three
  tables in one read transaction (`unchecked_transaction` on the reader), so another process's keep
  between reads cannot mix runs. No rows written = queue discarded; `resumption` answers `None` for
  an empty one, as before anything has played.
  (`a_kept_queue_row_that_will_not_open_is_dropped_and_the_rest_resumed`)
- **A favourite is when, not whether.** `tracks.favourite`, `albums.favourite`, `artists.favourite`:
  nullable nanosecond stamps, so the marking column also orders by when marked (a boolean costs a
  second sort column). `Library::favour` takes one `Favoured` (`Track`/`Album`/`Artist`; writes
  differ only in table), answers whether the value moved: guarded on the column standing the other
  way, so favouring a favourite keeps its stamp (and place in *recently favourited*), writes
  nothing, answers false, as does an unknown id. `is:favourite` is a `Shape` beside `is:hires`.
  `SortOrder::Favourited` needs `tracks_by_favourite` declared `favourite DESC, title_filed COLLATE
  NOCASE`, exactly as its `ORDER BY` reads; `AlbumOrder::Favourited`, `ArtistOrder::Favourited` need
  nothing (no indexes by design).
- **A genre is the track's and its artist's at once, in one column.** `tracks.genre` = what the scan
  read into `TagSet::genre` and once dropped; fourth `tracks_fts` column = it folded through
  `folded_letters`, joined with the artist's `artist_genres` names, so `genre:` is an ordinary
  scoped word and reaches files whose tagger named none. Enrichment re-indexes: `land_artist` writes
  `artist_genres` long after the scan wrote the row, so `store::reindex_the_tracks_of` runs beside
  it; `artist_genres` is in `NAMES_MOVED_TRIGGERS` (spelling vocabulary) and `NAMED_TABLES`
  (suggestions), else a landed genre leaves them stale.
- **A pinned playlist leads every order; `undo.rs` is where that is easy to lose.**
  `playlists.pinned` = nullable stamp like a favourite; `order_by` prefixes `PINNED_FIRST` (`CASE
  WHEN p.pinned IS NULL THEN 1 ELSE 0 END, p.pinned DESC`) onto every `PlaylistOrder` (one rule, not
  six). Threaded through `COLUMNS`, `read`, `undo.rs`'s `Held`, `held_in`, `rewritten`: undo
  re-creates the whole row from what was held, so a column added to `playlists` and missed there is
  silently dropped the first time an edit is walked back. A pin is no more an edit than a play:
  `rewritten` reads it off the live row via `unedited_in` beside `played`/`plays`; only a playlist
  re-created from nothing takes `Held`'s, so a pin made or removed after an edit survives walking it
  back. `resonate playlist --pin`/`--unpin`. (`undoing_an_edit_keeps_a_playlist_pinned`,
  `a_pin_made_after_an_edit_survives_walking_the_edit_back`)
- **Listening: reads over `listens`, `passes`, `unheld_listens`.** `listens.heard` = nanoseconds of
  the visit actually heard; `listens_by_time` is what every window reads. `passes` (ninth
  `MIGRATIONS` step: stamp + heard time, index `passes_by_time`; `Library::passed`) = time on visits
  that never earned a play. `statistics` and `listening_by_day` add `passes` time and
  `unheld_listens` (plays of files the catalog lacks, with time) to `listens`; tracks/albums/artists
  heard and the three most-heard lists read `listens` alone (a pass is time, not a play).
  `Library::statistics`, `most_listened`, `listening_by_day` are bounded by `listens.at >= ?` (index
  serves each); `most_listened` answers its three lists from one `read` (one connection, not three)
  and may sort on an aggregate as its own read, not a `SortOrder`. **A day is the listener's, not
  Greenwich's.** `listening_by_day` groups stamps by quarter hour in SQL, folds each into the day
  `resonate_core::Calendar` says it fell on: `Calendar::local`, zone from `TZ` or `/etc/localtime`
  via `tz-rs` on every call (zone changed under a running window is followed), UTC if unreadable.
  Quarter hour because every offset in use and every transition between two is whole quarters (no
  bucket straddles a local midnight or clock change), and rows read are bounded by quarters heard
  in, not listens. `Day::at` = the calendar's own midnight (clock-change day: 23 or 25 hours;
  skipped midnight begins the day at its first hour, `Calendar::midnight_of`); *today* = the
  calendar's day of now; no-play day = zero row (nothing draws around a gap). Nothing stored that
  history does not say. (`a_listen_late_in_a_utc_day_is_counted_on_the_day_the_listener_was_living`)
- **History is kept as long as the listener says, never past what a service was told.**
  `HistoryKept` = `Forever` (default) or days. `Library::age_the_history` deletes `listens`,
  `unheld_listens`, `passes`, `playlist_plays` older than it (`forgotten_before`); a listen goes
  only at or behind the lowest `submissions` mark, so history aged while ListenBrainz was
  unreachable is still told in full. History only: `tracks.plays`/`tracks.played` are separate
  counters and stay (track count, *most played* order unchanged); only Statistics-pane reads (all
  over `listens`) stop at the span. The binary ages in `open_library_with` (every command opening
  the catalog); settings pane *Listening history* ages once a span is chosen: a span shorter than
  the one in force is armed by the first press (`RootView::aging_the_history`, lowered by `disarm`
  with the pane's other armed presses), kept by the second, as every setting forgetting for good; a
  longer span or *Forever* is kept at once.
  (`a_shorter_history_is_armed_by_the_first_press_and_kept_by_the_second`,
  `a_history_kept_for_a_span_forgets_what_is_older_once_every_service_was_told`)
- **A service's told-ness is a mark in the history; the history is what is told.** `submissions`
  (sixth `MIGRATIONS` step): per `ListeningService`, id of the last `listens` row told.
  `scrobble.rs`: `Library::submit_listens` reads listens past the mark in id order,
  `SUBMITTED_AT_ONCE` (100) a batch, each at `listens.began` (moment it counted less what
  `track_played` was told had been heard of the visit; a `MIGRATIONS` step gave unheld plays it
  too), joined to track/album/artist for the names and MusicBrainz ids a `Scrobble` carries; hands
  them to the `Scrobbler` (library-owned seam, like `Reference`; `resonate-online`'s `ListenBrainz`
  fills it); moves the mark past the batch in a `max` (never back). `listens.id` is `AUTOINCREMENT`
  (a later step rebuilt the table, seeding the sequence at the greater of its newest id and every
  mark): an aged-away id is never reissued below a mark. Row with no title or artist =
  `Submitted::unnamed`, passed over (services file nothing under a blank name). Batch refused as
  malformed (`Refused`, 400) is retold a listen at a time (one bad row costs itself); a listen
  refused alone = `Submitted::refused`, passed over; any other failure moves the mark only past what
  was told, answers the error, rest told next ask. Service with no row yet is marked at the last
  listen held, told nothing (`Submitted::started`): a token given today does not send ten years of
  history. Counting a play writes nothing here, so any process may have counted it (offline, before
  a restart) and whichever run submits next tells it; a listen whose track leaves the catalog first
  leaves with it (cascade). `Library::billed_as` = same names for one row, what `playing_now` is
  told. Two runs submitting at once may tell a batch twice; ListenBrainz keeps one listen per moment
  and name, so nothing guards it. Binary's `submitting.rs` is the asking thread (`online.md`).
  (`a_play_counted_after_the_newest_listens_were_forgotten_is_still_told`,
  `a_service_is_told_what_was_heard_after_it_was_first_asked_and_each_play_once`,
  `a_play_the_service_refuses_as_malformed_is_passed_over_and_the_rest_are_told`,
  `a_play_a_service_could_not_be_reached_for_is_told_the_next_time`,
  `a_listen_is_told_as_when_it_began_rather_than_when_it_counted`,
  `what_is_playing_is_billed_as_the_catalog_names_it_and_a_file_it_does_not_hold_is_not`)
- **A suggestion is a named saved query, so it costs almost nothing.** `suggest.rs` answers
  `Suggestion { name, reason, query, rows, length, pictured_by }`; query written through `Display
  for Search`, not a literal, so suggestion and search-box parse cannot drift. A genre/artist with
  no letter or digit (`!!!`, `?`) or containing `"` is not offered (`fits_in_a_phrase`: its phrase
  would read as no condition, the playlist as the whole library). Saving = `Library::save_query`, no
  new code (a saved-query playlist fills itself). Nothing persists until saved; nothing offered
  below `ENOUGH_TO_OFFER` rows (thin catalog offers few, not a screen of empty ones). Two rules from
  real data: `names_a_decade` drops a genre that is only a decade (MusicBrainz hands out `2010s`,
  beside the `albums.year` decade saying the same worse); `billed_as` capitalises a lower-cased
  genre after any mark, not only a space (else `contemporary r&b` is *Contemporary R&b*).
  `Library::suggestions` answers an `Arc<[Suggestion]>` kept beside the search vocabulary under a
  counter of its own plus the music filters: the same `update_hook` steps `written` for any row
  written in `tracks`, `albums`, `artists`, `artist_genres` (every table a suggestion reads), so a
  reload that moved none (settled search, scroll, playlist edit) gets the kept answer, not a
  `measured` pass per candidate. Per table, not per name: suggestions count plays and favourites too
  (a counted play is a `tracks` write, dropping them where the vocabulary stands); another process's
  writes drop both via the data version, whatever table. `length` = the same `measured` read's
  total: aggregate with no `ORDER BY` wherever the query has no limit/offset (order matters only to
  which rows a bounded query counts), so a candidate is counted without sorting all it matches.
  `pictured_by` = `db::pictured_by`: albums holding a picture the search's rows fall on, most rows
  first, up to `PICTURED_BY_AT_MOST` (4) (weighs `PICTURES_WEIGHED_PER_TILE`, 4, candidates per
  tile), skipping an album whose picture (vault key, or bytes' length + first 256 bytes) another
  already stood for, then one that merely *looks like* one standing (one sleeve at two resolutions
  is not two tiles). `store::the_picture_of!` = that identity, written once for query and sweep:
  `coalesce(cover_key, cover_print)`, where `albums.cover_print` is the bytes' length + hex of the
  first 256, kept by the triggers `albums_print_their_cover_when_added`/`_when_changed` as the
  cover is written, so a sweep or a suggestion candidate reads a short text, never the cover (the
  text is the expression it replaced: kept `likenesses` still match;
  `an_album_cover_is_printed_once_as_it_is_written_and_reads_as_it_always_did`).
  `resonate_codec::Likeness`: cover averaged in linear light onto 8x8 cells kept as sRGB bytes;
  alike within a root mean square of `ALIKE_WITHIN_A_ROOT_MEAN_SQUARE_OF` (12 of 255): sleeve at
  quarter size ~1, re-encoded as JPEG ~3, mirrored or with a banner across the top ~80. A likeness
  costs a decode, so `likeness::of` keeps it in `likenesses` (fifth `MIGRATIONS` step) under the
  picture identity: unreadable picture = `NULL` (not decoded again); cover that failed to reach the
  reader = nothing kept, retried; `ORPHANS` removes rows no album's picture names. `update_hook`
  ignores that table, so weighing a cover never drops the suggestions it serves. `Reason::kind`
  sorts a suggestion under a `SuggestionKind`, how the pane shelves them.
  (`every_suggestion_reads_back_through_the_grammar_it_was_written_in`,
  `a_name_with_no_letter_or_digit_is_no_phrase_a_search_can_hold`,
  `a_suggestion_says_how_long_it_runs_and_which_covers_picture_it`,
  `one_sleeve_saved_at_two_resolutions_pictures_a_suggestion_once`)
- **A share is the link alone, one for three callers.** `Library::shareable` reads the track and its
  `release_track_links`, `album_links` rows; `Shared::written` = pure function over them (one URL or
  nothing) in `resonate-library`, so window, `resonate share` and others agree. **Song.link's own
  short page wherever the service has one**: `ShortForm::of` reads the service id from its URL,
  writes `https://song.link/<letter>/<id>` (song) or `https://album.link/<letter>/<id>` (album): `s`
  Spotify, `d` Deezer, `t` Tidal, `i` Apple Music (song = the `i=` of an album URL), `y` YouTube and
  YouTube Music; the address song.link redirects the long form to, short, saying nothing of where
  found. Service with no short page (Amazon, SoundCloud, Bandcamp, Qobuz) or URL with no readable id
  = `https://song.link/` + the service URL percent-encoded as one path segment (raw, the server
  collapses the unescaped `//` and answers 308 to `https:/…`, and `?` reads as song.link's own
  query, so the track id never arrives). Candidates: `RELATIONS_SONG_LINK_TAKES` x
  `SERVICES_SONG_LINK_RESOLVES`; recording's links, then album's, each in declared service order, so
  one track shares the same way twice running. **Nothing song.link opens in the catalog: ask the
  reference where the track streams**: `Shared::streamed_where_asked` hands `Reference::streamed_at`
  a `StreamAsked` (title, artist, ISRC, length, carried by `Shared`), puts the answer first, written
  like any held link; already-linked asks nothing; failing reference = warning, share goes on.
  Window *Share* asks only while `online` is on, `resonate share` only where `online::reference`
  answers; the find is not stored (a share is a gesture made once). Failing all: the MusicBrainz
  recording; failing that, nothing to copy.
- **Every pane order is read off an index; planner knowledge is written after a scan.**
  `tracks_by_album` carries the trailing `title_filed COLLATE NOCASE` the album order ends on;
  `tracks_by_title`, `tracks_by_artist_name`, `tracks_by_added`, `tracks_by_duration`,
  `tracks_by_plays`, `tracks_by_played`, `tracks_by_favourite` serve the other `SortOrder`s
  (`SortOrder::HELD_BY_AN_INDEX` lists the nine, `Relevance` needing none), each declared as its
  `ORDER BY` reads: collation included (no ordinary index serves a `COLLATE NOCASE` order), `DESC`
  included (SQLite walks an index backwards only where the whole order runs one way; `plays DESC,
  title` does not). `SortOrder::PlaysThisMonth` is outside that list: correlated `count(*)` over
  `listens` of the last 30 days (`heard_this_month!`). **A reversed order needs no index**:
  `order_by` is a `Reading` of the natural spelling and its mirror (every term flipped, `plays DESC,
  title` becomes `plays, title DESC`);
  `db::tests::every_order_the_panes_offer_is_read_off_an_index_either_way_round` plans
  `SortOrder::HELD_BY_AN_INDEX` x `Direction::ALL` through `db::listing`, refusing a plan with a
  temporary B-tree (a full sort before the `LIMIT`). Cost: eight order indexes on `tracks` written
  per stored row, up from three. `resonate playlist --reverse` turns `--sort` as well as `--order`;
  without either the grammar refuses it (the `ordered` group) rather than take a no-op flag. **Every
  order ends on the row's own id** (`tracks.id`; `a.id`, `r.id` in albums/artists orders): tied rows
  keep one place, `LIMIT`/`OFFSET` paging neither repeats nor drops one. Id runs as the index's
  rowid tail runs: rising where the order walks its index forward (`plays DESC, title, tracks.id`),
  falling backward, so the tie costs no sort (`track_ids_rising!`, `track_ids_falling!`). A playlist
  listing already ended on `p.id`.
  (`every_order_ends_on_the_rows_own_id_so_a_page_never_splits_a_tie` beside the index guard)
- **Albums and artists panes have orders too, deliberately unindexed.** `AlbumOrder`: relevance,
  title, artist, year, track count, when a track was last added, favourited. `ArtistOrder`:
  relevance, name, album count, track count, favourited. `album_order_by`/`artist_order_by` mirror
  `order_by`. **An artist files under the name its files sort it by**, else MusicBrainz's sort name,
  else its own: `filed_as!` = `coalesce(tagged_sort, sort_name, name)`, which the artists orders and
  the albums' artist order (`album_owner_filed_as!`) tie on (*The Beatles* files under *B*).
  `artists.tagged_sort` = scan's: billing's `ARTISTSORT`/`ALBUMARTISTSORT` (`TSOP`, `TSO2`, `soar`,
  `soaa`), taken once a pass per artist, never cleared by a file naming none (another may still name
  it); a billing read as a list's lead takes none (the sort was the list's). **An album files under
  the title its files sort it by**: `albums.tagged_sort` = a track's `ALBUMSORT` (`TSOA`, `soal`),
  once a pass per album (a later pass replaces it); `album_filed_as!` = `coalesce(a.tagged_sort,
  a.release_title, a.title)`, which every `AlbumOrder` ties on; `gather` keeps the survivor's own,
  else the absorbed one's. **A track files under the title and artist its file sorts it by**:
  `tracks.title_sort` = `TITLESORT` (`TSOT`, `sonm`), `tracks.artist_sort` = `ARTISTSORT` for the
  whole credit in `tracks.artist`; both written by every upsert, followed by a tag write
  (`files_retagged`). `title_filed` = a generated `VIRTUAL` column reading the sort name only while
  the row is billed as the file names it (`title IS tagged_title`), so a lookup-corrected title is
  not filed under the file's sort of the old one. `artist_filed` is a plain column the schema's
  triggers keep (`tracks_file_their_artist_when_added`/`_when_named`,
  `artists_file_their_tracks_when_sorted`): the file's own `artist_sort` while billed as the file
  names it (`artist IS tagged_artist`), else the artist's `coalesce(tagged_sort, sort_name)` where
  `artist_id` names an artist whose name is the whole credit (`COLLATE NOCASE`), else the credit:
  an artist sorted by MusicBrainz or by another file's `ARTISTSORT` files its untagged tracks too,
  a credit naming more than the artist keeps its own spelling
  (`a_track_its_file_does_not_sort_is_filed_under_its_artists_sort_name`). Pure SQL, no
  application function, so a catalog written by any SQLite still keeps it. Every track order reads
  the two filed columns and every order index is declared on them (a plain column allows it; an
  expression would want an index per spelling). Steps adding `artists.tagged_sort`,
  `albums.tagged_sort` (rows with an album) and `title_sort`/`artist_sort` (which also rebuilt six
  indexes) each mark scanned rows `probe_again`, the sorts never having been read.
  Albums/artists orders skip the index guard: those tables hold thousands of rows against `tracks`'
  hundreds of thousands, so a temp B-tree beats an index, and `SCHEMA_FINGERPRINT` covers the index
  list (adding one = a `MIGRATIONS` step and a rebuild in every catalog). No `resonate
  albums`/`resonate artists` exists, so neither enum has a CLI argument (a variant nothing
  constructs is left out). (`an_artist_is_listed_under_the_name_its_files_sort_it_by`,
  `an_album_is_listed_under_the_title_its_files_sort_it_by`,
  `a_track_is_listed_under_the_title_and_artist_its_file_sorts_it_by`,
  `a_track_is_filed_by_its_sort_names_only_while_it_is_billed_as_its_file_names_it`)
- **A saved query's direction is its own column.** `playlist_queries.sort` = order alone
  (`store::sort_code`/`sort_of`); `playlist_queries.reading` = direction
  (`direction_code`/`direction_of`, as a playlist's kept order already used;
  `OrderedColumn::Reading` names it in a refusal). It once rode in `sort` as order + a
  `READ_BACKWARDS` of sixteen, which an older build read as whatever order the sum landed on instead
  of refusing; the `MIGRATIONS` step adding the column carries every such code out of the sum.
  (`a_saved_querys_direction_is_carried_out_of_its_sort_code_into_a_column_of_its_own`)
  `schema::restate_the_statistics` (`PRAGMA analysis_limit` + `PRAGMA optimize` on the writer after
  a scan that changed anything) is the other half: with no `sqlite_stat1` the planner picks join
  order from hardcoded guesses. `configure` gives a connection a page cache and a 256 MiB memory map
  (`MEMORY_MAPPED_BYTES`; the 2 MB default had each pooled reader re-reading pages it had just
  read). **Cache size follows the connection's job** (page cache is per connection; `READER_POOL` is
  eight): `schema::Role::Writing` takes `WRITER_PAGE_CACHE_KIB` (8 MiB; batches inserts, maintains
  indexes), `Role::Reading` `READER_PAGE_CACHE_KIB` (2 MiB), so all readers out bound the page cache
  at 24 MiB, not the 72 MiB one size for all allowed. A bound, not a measured saving (SQLite fills
  the cache lazily; a small catalog never reached either figure); the writer has the working set
  worth keeping.
- **A reader is checked out and handed back; `READER_POOL` is how many may exist, not how many are
  kept.** `Inner::checkout` takes a parked connection, opens one while under the count, else waits
  on `Inner::freed`: eight is the SQLite connections a 500k-track scan can have open at once, not
  the callers. `Reader` returns its connection from `Drop`, so a panicking query costs the pool
  nothing. The wait is safe because no reader is taken while another is held (every `Inner::read`
  closure queries and returns; a caller needing two takes them in turn), so a nested checkout is
  never what the pool waits for.
- **Every write begins `IMMEDIATE`.** `Inner::write`, `reconcile_artists`,
  `settle_the_credits_if_owed` take the write lock as the transaction opens, so a second process
  waits its `busy_timeout`. A deferred transaction that read before writing (every undo-wrapped
  playlist edit) held a snapshot another process could commit past; its first write then failed at
  once with `SQLITE_BUSY_SNAPSHOT`, which no timeout waits out. In one process the writer's `Mutex`
  already serialised them; the only cost is a write that writes nothing holding the lock for its
  read. (`an_edit_that_reads_before_it_writes_is_not_torn_by_another_process_committing`)
- **A cue sheet claims the file it names; the scan reads sheets before audio.** `.cue` = sidecar,
  not audio, not in `AUDIO_EXTENSIONS`: `directory_of` reads every sheet in a directory first (name
  order), resolves each `FILE` against the sheet's folder, then sends probe jobs for audio no sheet
  claimed, so one FLAC is not stored as an album's worth of rows plus a whole-file row. A `FILE`
  cutting no audio track (data track alone) claims nothing: its file stays a whole-file row rather
  than being claimed then pruned with its plays and favourite. A `FILE` naming a folder below the
  sheet (`cue::folder_named`, `audio.md`) is matched against the audio listed there; claim kept in
  the walk's `claimed_from_above`, which that folder's own pass takes the file out of (always later:
  depth-first, children walked after their parent's pass); a lower sheet naming a file one above
  already claims is passed over (no file cut twice). **Two sheets in one folder naming one file:
  first in name order keeps it** (listing order is arbitrary). Otherwise a `FILE` matches audio
  beside the sheet by its last component (split on `/` and `\\`, so a Windows path names the file in
  the sheet's folder) via `cue::Naming`: exact name, else any case, else same stem with another
  extension (rip converted after its sheet, `FILE "ALBUM.WAV"` beside `album.flac`); closest reading
  wins, a tie at it names nothing. `organise` resolves claims through the same
  `scan::claimed_beside` (a stem naming only audio); file cut by a sheet in a folder above =
  `Refusal::NamedFromAbove`, stays put (moving it would leave the sheet naming nothing); sheet
  naming files in two folders = `SharesASheet` (the move files them into one folder); `cue::renamed`
  rewrites whichever `FILE` line the moved file answered to. Incrementality weighs two mtimes apart:
  `tracks.modified` = audio file's, `tracks.sheet_modified` = sidecar's (NULL where no sidecar cut
  the row). Row unchanged only where both agree with the walk: editing a sheet rescans its rows;
  removing it reprobes the file and prunes rows the cut no longer names *whichever* mtime was later
  (as one stored number, a sheet older than its audio left its rows standing when it went).
  (`a_sheet_that_cuts_no_audio_track_leaves_its_file_to_the_whole_file_pass`,
  `two_sheets_in_one_folder_naming_one_file_cut_it_once_by_the_first_in_name_order`,
  `a_sheet_older_than_the_file_it_cut_is_still_missed_once_it_has_gone` (needs an mtime set by hand:
  a sheet written after its file is the newer))
- **Embedded sheets are cut in the probe worker, not the walk.** A file's chapters are such a sheet
  (`audio.md`): audiobook = row a chapter. A sidecar shows in a listing, an embedded `CUESHEET`
  does not, so `read_candidate` probes then cuts on `MediaInfo::cue` where it names audio tracks,
  sharing `cut_into_rows` with `read_cut`. Sidecar still wins by the walk: `claimed` removes its
  audio file from the audio pass before `whole_file_job`, so that file's embedded sheet is never
  read. Cost: one file = one `discovered` until the probe says otherwise; `rows_past_the_first` is
  what the worker then adds (as the incremental path does, sending one `Job::Known` per stored
  row), so `discovered` ends at the rows either way.
- **The walk reads the catalog once, bounded by the roots.** `Known::under` takes
  `(path, id, file_size, modified, sheet_modified, probe_again)` per walked root: one indexed range
  query each (500k point queries used to interleave with the writer's commits). Snapshot before
  the walker starts; safe as no path is walked twice per scan (a root inside a root is refused, a
  directory reached by a second link is stepped past, a sheet claims its file before the audio
  pass). `Known::rows` = every stored row of a path, `span_start` order (cut or whole); unchanged
  = at least one row, each matching size, mtime and cutting sheet's mtime, none `probe_again`;
  `Candidate::existing` carries them all so a probe answering N rows claims the N there. Cost:
  walked roots' paths in memory.
- **One pass walks the tree at a time; a second is refused, not queued.** `Inner::walking` is the
  flag, `Walk` the guard. Taken by every pass walking or rewriting the tree: `Library::scan`,
  `organise`, `retag`, `import` (each in its `start`, before the thread spawns),
  `prune_the_vault`, `release_from_vault`, `delete_tracks`; and by the root edits a walk reads
  under: `add_root`, `remove_root`, `forget_the_gone`, `retire` (`resonate forget` and *Add folder*
  refuse beside a scan: `a_root_is_neither_added_nor_dropped_under_a_pass_walking_the_tree`).
  `resonate scan <roots>` checks each is a folder, leaves registering to the scan's `roots` inside
  the guard (a refused scan registers nothing). The thread owns the guard; `Drop` returns it on a
  panic as `Reader` does a pooled connection. Taking never waits: `Error::AlreadyWalking` at once
  (a minutes-long block is no pass a window or CLI can start). Why: `Known::under` is a snapshot,
  so an organise committing a path rewrite before the walker reached the directory left it
  probing the file as new and the prune taking the rewritten row (`id`, `added`, counts,
  `listens`); two scans each prune what their own walk missed, deleting every row the other had not
  reached. `enrich` and `poll` stay outside (no walk, no path rewrite).
  - **Across processes**: `resonate scan` beside the window's scan, or `resonate vault --prune`
    beside its import = two `Inner`s, two flags, so a catalog on disc also takes an exclusive
    `File::try_lock` on `<catalog>.walk` (`locked_across_processes`), held by the `Walk`, released
    as its `File` closes (panic or kill alike: the kernel drops an advisory lock with its
    descriptor); held elsewhere = `Error::AlreadyWalking`. The lock file stays (removing it races a
    process that opened and not yet locked it). In-memory: flag alone
    (`a_walk_in_one_process_refuses_a_walk_in_another_on_the_same_catalog`).
  - **Wind-down**: `Walk::cancelled_by` hands the guard the pass's progress;
    `Library::wind_down(patience)` cancels it, waits for the guard to drop, answers whether it did.
    The window calls it with `WIND_DOWN_WITHIN` (ten seconds) after its event loop ends: a tag run,
    organise, vault import or scan stops at its next file, a close no longer kills it mid-write
    (`winding_down_cancels_the_pass_walking_the_tree_and_waits_for_it_to_let_go`).
  - **Lookup and poll** likewise, each with its own lock (`<catalog>.enrich`, `<catalog>.poll`,
    `Library::asking_alone`) held by the pass thread, refused as `Error::AlreadyAsking`: two
    processes never ask MusicBrainz or a provider about the same rows and double the paced rate
    (`a_lookup_or_a_poll_in_one_process_refuses_the_same_in_another_on_the_same_catalog`). No walk
    guard needed.
- **A folder-grouped album is re-keyed in place when its folder moves.** Only the sleeve tier
  embeds a path, so only it can name a vanished folder; one file of such an album re-probed later
  keys onto the new folder, inserts a second `albums` row, takes its tracks, and `ORPHANS` sweeps
  the old row's cover, `mbid`, `release_group`, `release_tracks` and, via cascades, `wants`.
  `organise::re_key_the_sleeves` runs in its own transaction once every batch has landed: takes
  the albums the moved files name, keeps those `store::is_keyed_by_its_folder` answers for, writes
  `store::sleeve_key` of the row's title and the folder its tracks now share. Membership is
  unchanged (`tracks.album_id` references `albums(id)`; nothing joins on a key): nothing orphans.
  Folder = `scan::sleeve` of each track read together (disc folders and all); tracks in several
  folders, or any directly in a root, keep the key (debug record). An album `album_keys` names
  more than once is left alone likewise (a release-gathered album is named by every folder it came
  from). `album_keys.key` is primary, so an album moving into a folder another album names keeps
  its old key: merging albums *by where they landed* is a decision nobody asked for; merging two
  that prove one release is made on evidence.
  - **What the re-key leaves alone, a probe keeps.** A whole rescan, or a probe of a file whose
    size/mtime moved, computes the new folder's key afresh; that key naming another album, or none,
    would file the track there and `ORPHANS` would take the album it left with release, cover,
    wants. `store::kept_where_it_was` answers first: where the row already belongs to an album of
    the same title, none of its other names finds an album, and its folder's key names another
    album or none, the row stays and `lend_the_free_names` gives the album whichever of its keys
    nobody holds
    (`a_whole_rescan_after_two_albums_land_in_one_folder_keeps_each_the_album_it_was`).
- **A row names its billed artist; a listing beside it is narrowed and capped.** `ALBUM_COLUMNS`
  reads the artist's name by id, `(SELECT r.name FROM artists r WHERE r.id = a.artist_id)`
  (`album_owner!`, beside `album_title!` and `album_tracks!`; correlated scalar like the three
  counting an album's tracks, distinct track artists, what its release is missing), so
  `Album::artist` sits by its `artist_id`; `ArtistDetail::name` likewise. Subquery, not `JOIN`:
  `Library::album` and `Library::albums` read `ALBUM_COLUMNS` against `FROM albums a` plus a
  varying `scoped.from`; a join would be written twice and could collide with
  `Matching::grouped`'s aliases. Replaced the window resolving names via
  `LibraryModel::artist_names`, built from the artists *listing*, which `ArtistQuery` narrows by
  typed text and caps at `PAGE` (`resonate-ui` `models.rs`): names went missing exactly where
  search worked (album cell drew year alone, scoped heading lost its artist line, artist heading
  fell back to literal `artist <id>`). `browsed` (`models.rs`) reads the scoped album by id like
  release, tracks and artist detail, so `LibraryModel::album_of` answers whether or not the
  listing holds it; map and getter are gone.
- **A scan says what went wrong with a file.** `ScanStats::failed` = `Failures`, four counts;
  `Failure` decides: `Unnamed` (non-UTF-8 path, counted before anything opens); `Misnamed`
  (`UnrecognisedContainer`, `NoAudioTrack`: bytes are not the promised container); `Undecodable`
  (container read but audio not: `NoDecoder`, `DsdCompressed`, properties or packets it cannot
  represent); `Unreadable` for the rest (`Io`, `Symphonia` residual: where a corrupt file lands).
  The match on `resonate_codec::Error` is exhaustive and the enum has no `#[non_exhaustive]`: a
  new codec variant fails to compile here, not into a catch-all. As `AUDIO_EXTENSIONS` advertises
  nothing undecodable, the tally holds files whose extension promised a container their bytes are
  not (retagging, bad rip). Presenters print the total and append only non-zero counts.
- **Hidden is not music.** `scan::is_passed_over` drops every dot-name (`.Trash-1000`, Syncthing's
  `.stversions`, AppleDouble `._`, a tag write's staged copy) and `$RECYCLE.BIN`,
  `System Volume Information`, `lost+found` from the walk and `audio_below`: never cataloged or
  counted failed. A dot-named root is still walked (the rule reads what is inside it)
  (`hidden_trash_and_system_folders_and_dot_files_are_passed_over`).
- **A root may not be inside a root; a wider one absorbs what it covers.** `store::register_root`
  is the one writer (`Library::add_root` and the scan's `roots` refuse/absorb alike): inside a
  registered root = `Error::RootInsideRoot`; containing registered roots = re-parent their tracks
  onto itself, drop their `roots` rows (re-parent, not delete: `tracks.root_id` cascades and
  widening must not cost a counted play). Nesting walked the overlap twice, `root_id` flipping on
  the second upsert.
- **A row reached unchanged is not written; only what the walk missed is marked.** An upsert
  stamps `tracks.seen` with the scan's `generation`; an unchanged row (`Outcome::Seen`) or kept one
  only lends its id to `store::Reached`, in the writer's memory (a batch with nothing to store
  opens no transaction). Once the walk is whole, `store::mark_the_unreached` reads ids under walked
  roots not stamped this generation off `tracks_by_root`, writes `seen = -generation` on those
  `Reached` lacks. Prune and `moves` read that mark: an incremental scan of 500 000 unchanged
  files writes only rows whose files went
  (`a_rescan_writes_no_row_whose_file_it_found_unchanged_and_still_prunes_the_one_gone`). A mark
  left by a scan killed before its prune is never read (each scan marks with its own generation).
- **What the scan could not read, it keeps.** The prune takes every unreached row, so a row counts
  reached wherever a gone file cannot be told from an unreadable one.
  - A file whose size/mtime moved and which then would not probe (torn write, transient `EIO`) is
    counted failed; its `Candidate::existing` rows are kept as `Outcome::Kept` (reaches the row
    without counting it processed twice); likewise a cue-cut file.
  - A directory `read_dir` refuses (`EACCES`, `EIO`, automount not answering) or whose listing an
    error cuts short (entries are gathered whole first: half-listed = kept whole, not half pruned),
    an entry whose kind cannot be read, an entry whose `stat` fails (link into an unplugged drive
    among them): every row `Known::at_or_under` names at or below the path is kept (range over the
    rows' `BTreeMap`). Before, each was stepped past and the prune deleted rows with their plays,
    listens, favourites
    (`a_changed_file_that_will_not_probe_keeps_its_row_and_what_was_heard_of_it`,
    `a_folder_the_scan_cannot_read_keeps_every_row_under_it`).
  - An *empty* folder is guarded only where seen as a volume: an empty mount point reads as a
    folder whose files moved out, and keeping every empty folder's rows kept
    `moves::follow_the_moved` from pairing any move out of a folder it emptied.
- **A volume seen mounted is remembered; its rows are kept while it is not.** The walk carries each
  directory's parent's `st_dev` and notes a directory whose own differs (root included, against its
  parent) as a volume; a followed link into another filesystem too. `volumes::settle` writes them
  to `volumes` after the prune and drops a row for one no longer mounted that no track and no
  playlist row sits under. A noted volume whose device is its parent's (empty mount point) or whose
  directory is gone keeps every row `Known::at_or_under` names as `Outcome::Kept` and is not
  walked; `tidy_the_roots_beside` and `forget_the_gone` skip paths on one
  (`a_volume_not_mounted_keeps_every_row_on_it_whether_its_mount_point_is_empty_or_gone`,
  `a_folder_on_another_volume_is_noted_and_its_rows_kept_once_it_is_not_mounted`). Nothing moves
  out of an unmounted volume, so no pairing is lost.
  - **Retiring a volume for good is the listener's call.** `Library::retire` (takes the `Walk`
    guard) takes a folder at or inside a root (mount point or any folder), forgets every row at or
    under it whose file is not there (unmounted volume's included), then drops each `volumes` row
    at or under it holding no track, its playlist rows left to a tidy
    (`volumes::retire_at_or_under`). `forget_the_gone` is the same walk (`forget_at_or_under`) with
    unmounted volumes guarded. A file that *is* there keeps its row (next scan finds it anyway):
    retiring a mounted drive forgets nothing
    (`a_volume_retired_for_good_forgets_its_rows_and_is_no_longer_remembered`,
    `a_folder_whose_files_are_still_there_is_not_retired`).
- **Every walk hazard but a lost worker is stepped past.**
  - Directory past `MAX_DEPTH`: warned over, skipped (not failing scan and prune).
  - Symlink: weighed only where it names a directory (and `follow_symlinks` is on); one naming a
    directory this walk has been down is stepped past, so two albums linked to a shared folder walk
    it once, not abort (the set is what a cycle meets on its second pass: skipping terminates).
  - A link whose canonical target is inside a registered root, or holds one, is never followed
    (`Walking::reaches_a_root`, over every root `roots` holds, not only the walked). The set holds
    only link targets, so a link to a folder inside its own root was walked beside it, every file
    stored twice under both paths; a link into another root filed that root's files under this one
    too. Inside a root is walked by its own path from its own root; what holds a root would walk it
    again from above
    (`a_link_to_a_folder_inside_the_same_root_is_not_walked_a_second_time`,
    `a_link_to_a_folder_holding_the_root_is_not_walked_back_into_it`,
    `a_link_into_another_root_leaves_its_files_to_that_root`).
  - Not stepped past: a panicked probe worker. `run` joins every one, answers `Error::Stopped`
    naming the scan's `PassKind` before the prune (short counts would prune rows never reached). A
    failing commit drops the result channel's receiver before any join: workers blocked sending
    wake to a closed channel, their exit closes the job channel under the walker, the scan answers
    the store's error and returns the `Walk` guard instead of waiting on a pipeline nothing drains.
- **A scan tidies every root, prunes only walked ones, and touches a root not there not at all.**
  The prune is the unreached mark over walked roots, so `resonate scan <root>` used to leave other
  roots holding rows for vanished files. `tidy_the_roots_beside` is the other half: per registered
  root not walked, read its distinct paths (`store::paths_under`), keep those no longer there, hand
  them to `store::forget_paths`; `store::sweep_orphans` (the prune's batch, lifted out) runs once at
  the end where anything went. Cost: one `stat` per path of a root nobody asked about; nothing
  opened, read, or newly found. A bare `resonate scan` walks every root: nothing beside. `is_there`
  guards both: a registered root whose directory is missing is dropped from the bare scan's walk
  and stepped over by the tidy (unmounted drive != empty one; pruning would take every counted
  play). A root named on the command line is still `Error::RootNotADirectory` where missing (asked
  for). A window-watch scan is the other kind: `Library::scan_what_is_held` walks only the named
  roots `roots` still holds and that are there, registers nothing, answers `None` where none is
  left, so a drive unplugged after its root was queued costs the roots beside it nothing; the
  window takes a root off what it owes only once a scan has taken it.
- **A per-row statement is prepared once a connection.** `store::cached` runs literal SQL through
  `prepare_cached`; every per-row write uses it (scan store writes: unreached mark, index row,
  album key, year, cover; resumption's and queue order's rows; a playlist's inserts), so an
  incremental scan of 500 000 tracks parses its `UPDATE` once. Per-row reads and the 39-column
  track upsert use `store::queried` (same cache, statement returning a row). A scan cycles through
  more distinct statements a record than rusqlite's default cache of 16, so `connect` raises it to
  `STATEMENTS_CACHED` (128), else they evict each other and are reparsed. `format!`-built SQL stays
  on `execute` (each spelling its own statement).
- **A track is keyed by `(path, span_start)`.** N cue rows share a path: `UNIQUE` on the pair,
  `span_start` `NOT NULL DEFAULT 0` (NULLs are distinct in a SQLite unique index: one file could
  insert twice). Every lookup meaning *this row* takes the span beside the path:
  `Library::track_at`, `Library::track_played` (a play keyed on path alone counts every track of an
  album against one row). A playlist entry stores `path`, `span_start`, `span_frames` and joins
  `tracks` on both (`ON_THE_SAME_CUT`, `playlist.rs`).
- **A `Track` names its artist by id too.** `tracks.artist_id` is what the artists listing always
  counted against; `Track::artist_id` reads it beside `album_id`, so the window opens an artist
  from the playing row without weighing a name against a search-narrowed listing. `TRACK_COLUMNS`
  is append-only: `BESIDE_A_TRACK` counts it, so appending leaves joined reads' own columns put.
- **Each MusicBrainz id is weighed against the column of its kind.** `tracks.mbid` = *recording*
  id, what `TagSet::musicbrainz_track_id` carries (`MUSICBRAINZ_TRACKID` in Vorbis comments, the
  `UFID` frame MusicBrainz owns in ID3); `release_track_mbid` = `MUSICBRAINZ_RELEASETRACKID`, the
  track's place on one release. `rematch_release_tracks` weighs each against its own (release
  row's `recording_mbid` vs first, `track_mbid` vs second); one column against both could pair the
  second only by accident.
- **An artist is keyed by the fold of its name: one spelling, one artist.**
  `resonate_core::folded_letters` (re-exported by `store` and the crate) lowercases, decomposes,
  drops a combining mark on a Latin, Greek or Cyrillic letter (`takes_accents` over
  `ACCENTED_SCRIPTS`: scripts where a mark is an accent), keeps every other (kana voicing mark,
  Indic vowel sign: they change the word), recomposes: ガラス/カラス, バンド/ハンド stay two artists,
  two searches
  (`a_kana_voicing_mark_keeps_two_names_apart_and_an_old_index_is_folded_again`). It spells out
  letters Unicode does not decompose (`ł`, `ø`, `đ`, `ð`, `þ`, `ß`, `æ`, `œ`, dotless `ı`, `ħ`,
  `ŋ`, `ŧ`, `ĸ`, the rest): *Marcin Przybyłowicz* and *Marcin Przybylowicz* are both `artists.key`
  `marcin przybylowicz` (`name.to_lowercase()` made two artists, listings, portraits, half a
  discography each). `tracks_fts` is written in the same fold and a typed word folded through it:
  SQLite's tokenizer folds `İ` to `i` but leaves `ı`, so *Kıskanç*/*KISKANÇ* were two searches
  (an index written before the fold needed a rescan). Not `enriched::folded_title`, which keeps
  marks (vs a MusicBrainz title a mark is evidence; gathering spellings of one name it is noise);
  `enriched::stripped_title` = both (letter fold through title fold), the enrichment's fallback.
  **A combining mark is part of the word it sits in** (`resonate_core::is_lettered`): a typed word
  is folded whole before it is cut into pieces, and a held name is cut into highlight runs by the
  same test, as SQLite's `unicode61` keeps a mark inside its token; `Mo` + U+0308 + `tley` is one
  word, `motley`
  (`a_name_typed_or_tagged_with_combining_accents_is_found_as_the_precomposed_name_is`).
  `store::words_of` still cuts at a kept mark, as the `songs` and single-release rows it wrote
  were cut.
  **The billed spelling is the one with the marks** (`marks_in`, applied on cache hit as on the
  row), so a later scan meeting the stripped spelling does not undo the accented one and the
  display name does not follow scan order. `resonate missing --artist` and the window's
  type-ahead weigh a typed name through the fold: either spelling reaches the artist filed under
  the marked one.
- **A catalog keyed before the fold is folded back when opened.** `store::reconcile_artists` runs
  once in `Library::build`, after `schema::lay_out`: reads every artist, groups by `folded_letters`
  of the *name*, leaves a group alone where its one row is already keyed by its own fold.
  Otherwise keeps one row (with an `mbid`, then lowest id: what enrichment, portrait, genres, links
  hang off); the rest hand over tracks, albums, genres, links, kept releases, credits via
  `store::take_over_artist` (`UPDATE OR IGNORE` except tracks and albums; the survivor also takes
  the favourite, portrait, tagged sort name it lacks) and are deleted; survivor rekeyed and renamed
  to the group's most marked spelling. No sentinel key against an unreached row is needed: every
  key is a fold or `to_lowercase` of the same name and folding is idempotent, so rows whose keys
  could collide fold into one group. An invariant kept, not a migration run (no schema step; a
  second run is a no-op). A rekeyed row has its tracks indexed again.
  - **A change to the fold is a schema step asking for the index to be folded again**: it creates
    `index_refold_wanted`; `store::refold_the_index` (in `Library::build` after the reconcile)
    rewrites every `tracks_fts` row whose title, artist or album no longer reads as its fold, then
    drops the table. A release's `folded` haystack, a kept release's and a dismissal folded before
    the change are rewritten when the album is next landed.
- **An album is whatever a grouping key names; several may name one.** `album_keys` (key primary,
  an album holding any number) makes a grouping a *name* for an album, not a property.
  `store::album` reads it rather than upserting on a column: a computed key an album holds fills
  that album, a key nothing holds makes a new one. `ORPHANS` unchanged (an unreferenced album's keys
  cascade away).
- **One song held more than once is listed once, as the best copy.** `alternatives.rs` runs at the
  end of every scan that owes it: groups rows by fold of album title (not row: two folders of one
  album meet), album owner else track artist, disc, track number, title; within a group gathers
  rows with lengths within `THE_SAME_LENGTH_WITHIN` (2 s) of the run's shortest, or both
  lengthless. An album is named both ways a row can be billed (release title enrichment gave, title
  its tags gave) and rows sharing either are one song: a copy MusicBrainz billed *Meddle* meets an
  untouched copy tagged *Meddle* though its tags said *Meddle (Remastered)*; `one_song_apiece` joins
  names by union-find. A row with no title or artist names nothing (a loose *Intro* is no evidence
  of another). Best = lossless over lossy, then wider word, higher rate, higher bitrate, then row
  held first; every other copy names it in `tracks.alternative_of` *whatever* its format (two
  identical rips in two folders = one row with `+1`; a same-format copy is hidden beside one in
  another). The row's menu tells two copies of one kind apart by folder and file. Copies are kept
  whole (plays, playlists, files); only listings step past them: `scoped` filters every track
  listing and saved query on `+tracks.alternative_of IS NULL` (`THE_BEST_COPY`; the unary plus
  keeps the term off `tracks_by_alternative` so order indexes still lead), album and artist counts
  count the best copy alone (`HOLDS_A_BEST_COPY`), an album whose every track is another's
  alternative leaves the albums pane. `Track::alternatives` = copies a row stands for (`+N` beside
  the title); `Library::alternatives_of` = what the menu offers to play instead. A best copy that
  goes puts `ON DELETE SET NULL` on rows under it; the end-of-scan pass crowns the next.
- **Music filters narrow listings, keep the catalog whole.** `MusicFilters` = supported extension
  selection + `MinimumLength` bounded to ten minutes. `Library::filter_music` sets it for this
  opened catalog (binary fills it from config before any listing), drops the suggestion cache.
  Track listings and measurements apply it before ordering and paging; albums, artists, their
  counts and search follow the same selection. Extension matched against the file suffix,
  case-insensitively, not inferred from the codec; a cue row has its backing file's extension and
  its own duration. Zero = no duration condition (unknown lengths kept); positive minimum requires
  a known length at least as long. Excluded rows keep ids, favourites, plays and still resolve for
  explicit playback; scans read every supported format. Defaults add no SQL condition.
- **A hidden track is kept and only stepped past.** `tracks.hidden` is the second `MIGRATIONS`
  step; `Library::hide_track` sets it either way, answers whether it moved. Row, file, plays,
  playlists, favourite stay; a scan never touches the column, so it stays hidden however often its
  root is read. `scoped` adds `tracks.hidden = 0` beside the best-copy term; album and artist
  counts and `HOLDS_A_BEST_COPY` weigh it alike: a hidden row leaves listings, counts, and an album
  holding nothing else. `is:hidden` is a `Shape`; a search *insisting* on it (a clause of that one
  alternative, not denied) lifts the visibility term, the one way a hidden row is listed again
  (`Clause::insists_on`); `-is:hidden` or an alternative beside it lifts nothing. `Track::hidden`
  is what the menu reads to offer *Hide from library* / *Show in library*.
- **A track names its album every way it can and joins the album the first name finds.**
  `grouping_keys` answers a *run* of keys in precedence order. A MusicBrainz release id stands
  alone (two releases sharing title and artist stay two). Otherwise: an `ALBUMARTIST` the tagger
  wrote, on anything not flagged a compilation = one name; the track's folder
  (`TrackRecord::sleeve`) another; `album_key(title, owner)` the fallback where a track has
  neither. A track joins the album the first of its names finds and lends it the rest, so both
  gatherings hold: `ALBUMARTIST` gathers discs the folders keep apart; a folder gathers an album
  its files bill to different owners (Cyberpunk 2077 soundtrack: three album artists made three
  albums under one sleeve when the owner outranked the folder and the loser had nowhere to go).
  Every key but a release's carries the title, so nothing gathers two albums not called the same.
  Two of a track's names finding *different* albums: first wins, second left, not merged (the pass
  gathers only on a release). Tracks of one album disagreeing on album artist: album keeps none
  (what `COMPILATION` already meant); `Album::artist_count` lets a pane draw *Various artists*
  rather than a bare year.
- **A sleeve is a folder made to hold an album, not simply the parent.** `scan::sleeve` answers
  `None` for a track directly in a root (a root of loose files is a dumping ground; two albums
  sharing a title there stay two); the third tier falls back to the track artist. A folder named
  `CD2`, `Disc 3` or `disk-01` is one disc of a set and answers with its parent: a set filed that
  way is one album without an `ALBUMARTIST` saying so.
  - **A record filed loose in a root is gathered once the scan has written it.** A compilation
    with no `ALBUMARTIST`/`COMPILATION` flag, folder = root, would stand as one album per track
    artist. `loose::gather_the_loose` runs after the prune on every walked root where the scan
    wrote a row: takes albums whose tracks all sit in that root and whose keys are all fallback
    tier (none a folder's or release's), groups by lowercased title, gathers a group via
    `enriched::gather` only where `one_record` says the numbering makes one: every track numbered,
    no disc and number taken twice, years and declared `TRACKTOTAL`s agreeing where stated, no more
    tracks than a declared total. Two *Greatest Hits* each numbered from one, or files with no
    numbers, stay apart (the reading the rule above protects). The loser's keys name the survivor,
    so a file read again joins it rather than splitting off.
- **Disc word: ten spellings, shortest remainder wins.** `SPELLINGS`: `cd`, `disc`, `disk`,
  `disque`, `disco`, `dysk`, `platte`, `schijf`, `skiva`, `диск`; `Disque 2`, `Disco 3` gather as
  `CD2` did. First-hit fails (`disc` prefixes `disco`: leaves `o 2`, no separator). `Discovery`,
  `Disconnected` stay albums (longest match leaves no separator).
- **A disc numbered in words is the same disc** (`ONES`, `TEENS`, `TENS`). `disc_in_folder` reads a
  cardinal after the word (`Disc One`, `CD Two`, `disk_three`) or ordinal before (`Second Disc`,
  `First CD`); tens + ones compose (`Disc Twenty One`, `Twenty-First Disc`) to 99 (past that: filed
  in digits). Word forms need a separator between words (else `Discone` is a disc,
  `Discovery`/`Disconnected` no longer albums; `Twentyfirst Disc` is nothing); digits don't (`CD1`).
  Two callers (`scan::disc_in_folder`): `{disc}` in an organise layout reads such sets as
  `organise::disc_of` reads `CD1`. Cost: key format change; a set scanned as `Second Disc` is keyed
  by that folder, two albums until rescanned
  (`a_word_run_together_with_the_one_beside_it_names_no_disc`).
- **Other-language number words compose as that language writes them, to 99.** `numerals.rs`:
  cardinals and ordinals in French, Spanish, Italian, German, Dutch, Portuguese: `vingt-et-un`,
  `quatre-vingt-onze`, `treinta y uno`, `veintidós`, `ventuno` (vowel elided), `ventitré`,
  `einundzwanzig`, `tweeëntwintig`, `vinte e um`, Portugal's `dezasseis`; `vingt-et-unième`,
  `vigésimo primero` (two words and one), `ventunesimo`, `einundzwanzigste` (each ending),
  `eenentwintigste`, feminine of every Romance ordinal. `ELSEWHERE`: built once, on first use.
  `numerals::plainly` reads both sides: `folded_letters` (mark, `ß`, `ë` cost no second spelling),
  separator runs one space (`Disque Vingt-et-un`, `disque_vingt_et_un`, `DISQUE.VINGT ET UN`: one
  disc). `numbered_after_the_word` looks a cardinal up whole; `ordinal_elsewhere_at_the_front` takes
  the longest ordinal the name begins with (`Vigésimo Primero Disco` is 21, not 20 plus `primero
  disco`). English keeps the composed tables above (its idiom is what they were written for).
  `named_before_the_word` weighs English and composed readings, taking whichever leaves a separator
  and disc word (English `second` begins `Secondo Disco`, leaving `o disco`; `secondo` leaves the
  disc); composed table read at longest too (`primer` begins `primera`). `Zweite CD` keyed by its
  folder like `Second Disc`: one album a disc until scanned again from nothing. Languages aren't
  told apart (a folder is a string): a word numbering in one numbers here whatever language the rest
  is in (`a_spelling_names_one_number_whichever_language_spells_it`: one number per spelling across
  the six; `a_disc_past_twelve_is_composed_in_every_language_it_is_numbered_in`).
- **`album_keys.key` format**: changing it means a rescan (old keys match nothing); `ORPHANS`
  removes albums nothing points at.
- **Albums proving to share a release are gathered; both keys name the survivor.** A release id the
  *pass* finds arrives after grouping: a rip split over two folders, or halves declaring different
  owners, can hold one release in two albums. `gather_under` (in `land_release`, after release
  columns are written, before wants are read) finds other albums with that `mbid` or already named
  by `release_key`; `gather` moves their tracks, release rows, *keys*, refused releases to the
  survivor, fills what it lacks (cover, year, declared count, sort name, favourite), keeps the owner
  only where the two agree (else none), deletes the other (cascade takes links, media). Keys move,
  not rewrite: next scan computes each folder's old key, finds it in `album_keys` naming the
  survivor, fills that album instead of remaking the row (a single `key` column couldn't;
  re-keying was once refused outright). `gather_under` also names the survivor by `release_key`, so
  a file later tagged with the id lands via the first tier. Nothing joins on a key
  (`tracks.album_id` is the only membership): four moving `UPDATE`s, one filling `UPDATE`, one
  `DELETE`.
- **An album takes the cover and year any track carries, not the first's.** `Cache::covered` holds
  albums holding a picture, so `store::cover` is asked per track until one lands. Reads
  `cover_art IS NOT NULL AND cover_source = 0` (or `cover_path`) first: file's own picture costs a
  query, no probe; archive's (`CoverSource::Archive`, code 1) is probed again, file's replaces it
  (deliberate vs stand-in). **A thumbnail isn't deliberate**: where `store::betters` finds the
  archive picture's shorter side at least twice a file picture's whose shorter side is under
  `A_THUMBNAIL_BELOW` (300 px), archive's stays and the album counts as covered. Year: `Grouped`
  carries it; a cache hit whose album has none runs `fill_year` instead of skipping the upsert that
  would have coalesced it. `store::year` reads separator-less dates (`19750601`, `197506` = 1975;
  formerly only `1975-06-01`, `1975`).
- **A file's picture is learnt on the open its tags came off; the picture is read at commit.** Scan
  uses `probe_scanned` (`Scanned { info, carries_a_picture, packets }`);
  `TrackRecord::embeds_a_picture` carries it; `store::cover` reopens only where there's something to
  find. Removed: a `probe_cover_art` per track of an album embedding none (uncovered albums never
  set `Cache::covered`: every track). An album that *does* embed one costs one extra open at its
  first committed row, deliberately: carrying bytes out of the probe was measured and rejected
  (copy per file, claimed once per album key: 1 200-track, 120-album scan 41 MiB to 111 MiB peak
  RSS, one cover per album alive from a worker's probe until its row commits, a `BATCH` of 1 000
  rows keeping nearly all alive, to save 120 opens of 1 320). Bytes across threads to save an open:
  wrong trade; the `bool` is all worth carrying. (`probe_pictured` + `Picturing::Whether` answers
  the same for other readers, e.g. `retag.rs`.)
- **A play is a count, a date and a row.** `Library::track_played` writes in one transaction
  `tracks.plays` stepped, `tracks.played` set, one `listens (track_id, at)` row, same nanos: columns
  = the cheap answer every listing reads. `listens` has `tracks(id) ON DELETE CASCADE` and
  `configure` sets `PRAGMA foreign_keys = ON`, so forgetting a root takes its plays (else a reused
  `tracks.id` re-attributes an orphan). History is what `plays:@` narrows on; count is what a pane
  draws. **One order is read off history**: `SortOrder::PlaysThisMonth` (*Most played this month*,
  `sort` code 9) ranks by correlated `count(*)` of listens since `unixepoch()` less thirty days,
  `tracks.plays` tie-break: a sort, not an index walk, so `SortOrder::HELD_BY_AN_INDEX` is what the
  index guard walks, every other order held to it. Playlist twin: `playlist_plays`, a row per play a
  playlist was loaded for (a `MIGRATIONS` step seeded played playlists with their last play);
  `PlaylistOrder::PlaysThisMonth` counts it likewise (a history, not one date and a total). Apart
  from the playlist row, no cascade (`undo.rs` re-creates a playlist from what it held; a cascade
  would lose history per undo); history span ages it with listens. SQLite reuses a discarded
  playlist's id, so `playlist::created` clears `playlist_plays` under the new id
  (`a_playlist_taking_the_id_of_one_discarded_starts_with_no_plays_this_month`,
  `the_tracks_most_played_this_month_are_ordered_by_what_the_month_heard`,
  `the_playlists_most_played_this_month_are_ordered_by_what_the_month_played`).
- **Moved files are followed, not forgotten and refound.** Hand rename/move (not `organise`, which
  rewrites rows) = gone-file row + unnamed file. `moves::follow_the_moved` (before the prune) pairs
  a whole-file row (only row at its path, file missing) with a row this scan added (`added` at or
  past the generation) alike in `file_size`, `duration`, `codec`, `tagged_title`, `tagged_artist`.
  Several alike per side (identical rips moved together): `moves::told_apart` scores paths by names
  shared from the end (file, then folders); pair only where each is the other's one best, sharing
  at least the file name: `vinyl/echoes.wav` + `tape/echoes.wav` under `filed/` follow to their own
  folders; moved to `c/`, `d/` they share only the file name with both: forgotten and found, not
  guessed. **Leftovers: by sound.** A tagger changes size and names at once: `pairs_by_sound` pairs
  rows sharing a `Sound` (exact decoded frames, codec, rate, channels), each the *only* row on its
  side with it, agreeing on tagged title, tagged artist or file name. Frames make it safe;
  uniqueness stops guessing between two rips of one length; agreement stops a deleted file plus an
  unrelated same-length addition passing for a move. **No name agrees: the packets decide.** Scan
  digests (`probe_scanned`, every whole file) the first `PACKETS_DIGESTED` (48) packets of the audio
  track (coded bytes pre-decode, FNV-1a) into `tracks.packets`. Taggers rewrite what surrounds the
  audio, never a packet: equal digests on the gone row and the one new row of its sound = one file
  however renamed and retagged, differing = not, no decode or study
  (`a_file_retagged_past_every_name_as_it_moved_is_followed_by_the_packets_its_scan_digested`).
  Pre-column rows hold none: the next `MIGRATIONS` step marks every rooted whole file without a
  digest `probe_again`, so the first scan reads each once whatever size and mtime say
  (`a_catalog_carried_forward_reads_again_every_whole_file_it_holds_no_packets_for`). **Moved
  before the scan could read it: the print decides.** A gone row whose study kept a Chromaprint
  pairs with the one new row of its sound where `resonate_analysis::print` (first two minutes,
  decoded as the study does) writes the same print; unstudied or differing: forgotten and found.
  Decode only for rows a unique sound leaves unnamed, never in a transaction: `moves::to_be_heard`
  runs the pairing on a read connection with a `heard` that notes each path asked and answers
  nothing; scan decodes those lock-free; `follow_the_moved` pairs in the write with prints in hand
  (else a bulk move holds writers past their busy timeout while files decode). A pair goes through
  `organise::files_moved`: new row dropped, old takes its path, root, generation, keeping id,
  plays, listens, favourite, enrichment, vault object, playlist rows, kept lyric, queue row.
  `settle_the_album`: if the album the scan made for the new folder holds nothing else and every
  moved row came from one album, gather into that one via `enriched::gather` (new folder's key
  names the old album; release, cover, favourite kept); else the row joins the album the scan filed
  it under. **A file a sheet cuts is followed whole.** `cuts_moved` groups rows sharing a path
  (gone: every row, file missing; new: every row, none of the path's rows older than the scan),
  pairing groups alike in size, codec, every cut's start and length, each the one group of that
  shape per side, where sheet titles or file name agree; `files_moved` moves the whole path.
  `ScanStats::moved` counts rows followed; `added` leaves them out
  (`a_file_retagged_past_every_name_as_it_moved_is_followed_by_the_print_its_study_took`,
  `a_file_retagged_as_it_moved_is_followed_by_what_it_sounds_like`,
  `a_file_taken_away_and_another_of_its_length_added_are_not_one_file`,
  `a_file_a_sheet_cuts_moved_with_its_sheet_keeps_every_rows_plays`,
  `a_file_moved_between_scans_keeps_its_row_its_plays_and_its_place_in_a_playlist`,
  `an_album_moved_into_a_folder_of_its_own_stays_the_album_it_was`,
  `two_files_alike_in_every_way_are_told_apart_by_the_folders_they_moved_with`).
- **The count is the catalog's; a rescan leaves it.** `tracks.plays`, `tracks.played` are absent
  from the upsert's `DO UPDATE SET` (as `added`); forgetting a root drops rows and counts. Kept
  against the row, so a file no scan has seen isn't counted against one (`Library::track_played`
  answers `None` where a non-local location has no row), **but the play is kept against the path.**
  `unheld_listens` holds path, span start, moment, time heard; `history::credit_the_unheld` runs
  after the prune of every scan a row arrived in: a play whose path and start now name a row becomes
  a `listens` row stamped when heard, the row's `plays`, `played` follow, the unheld play goes
  (files played before their folder was scanned count the moment it is;
  `a_file_played_before_any_scan_saw_it_is_credited_with_the_play_and_the_time_heard_once_one_does`).
  `Counted` carries a `Listen` (`Held` names a `listens` row, `Unheld` the rowid of an
  `unheld_listens` one); `Library::listened` spends either, so a settle keeps what it heard:
  statistics count an unheld play and its time beside the held, the credit carries `heard` onto the
  listen it becomes, history span ages unheld plays too. Answers with the counted row where there
  was one, read back in the same transaction. `plays:`, `played:` narrow on the two columns;
  `SortOrder::Plays`, `Played` order on them, so *Top 25 most played* is a saved query, not a
  feature; the count is drawn through `listing::times` wherever a row names a track (tracks pane,
  queue, opened playlist, playlists index: where `plays:`, `played:` started).

## Enrichment

- **A reference's answer lands beside what the scan read, in columns the scan never rewrites.**
  `artists`: `mbid`, `sort_name`, `kind`, `gender`, `country`, `area`, `began_in`, `began`,
  `ended`, `has_ended`, `disambiguation`, `portrait`, `portrait_format`, `asked`, `answered`.
  `albums`: `cover_source`, `mbid`, `release_group`, `release_title`, `date`, `country`, `label`,
  `catalog_number`, `barcode`, `kind`, `disambiguation`, `asked`, `asks`, `answered`. `tracks`:
  `mbid`, `artist_mbid`, `release_track_mbid`, `isrc`, `tagged_title`, `tagged_artist`,
  `release_title`, `asked`, `asks`, `answered`. Beside them: `release_tracks` (row per release
  track; `release_tracks_by_album`, `release_tracks_in_order`); `artist_genres`; link tables
  `artist_links`, `album_links`, `release_track_links` (`relation`, `provider`, `url`; first two
  codes via `store::relation_code`, `service_code`, read via `relation_of`, `service_of`, which
  refuse a later build's code with `Error::UnknownLinkCode`); `artist_releases` (discography,
  below); `wants`; `lyrics_kept`. `V1` held them as first written; later ones (`cover_asked`,
  `track_credits`, `refused_releases`, `lyrics_kept.lyricsfile`, `releases_unread` among them) are
  `MIGRATIONS` steps. Scan upserts touch only the ids, the two `tagged_` columns and what tags
  *declare* of a release (`barcode`, `catalog_number`, `label`, `albums.tagged_tracks` from
  `TRACKTOTAL`); `release_media`, like `release_tracks`, is written by a landing, never a scan.
  Albums: `coalesce(excluded.mbid, albums.mbid)`; artists: `coalesce(excluded.mbid, artists.mbid)`,
  `fill_artist_mbid` for one the cache held; `store::Declaration` coalesced likewise (file's where
  it names one, held otherwise, as `mbid`, `release_group`), `fill_declared` for an album the cache
  held, filling only what `Declared` says it lacks, so a file dropping its barcode leaves the held
  one (`a_scan_stores_what_the_tags_declare_about_the_release`,
  `a_rescan_keeps_a_barcode_the_file_no_longer_carries`). `tracks.mbid`, `artist_mbid`,
  `release_track_mbid`, `isrc`: from tags where the file names one, kept where it names none unless
  retagged (the two `tagged_` columns' weighing), since lookups and pairings also *find* them: a
  whole rescan once wrote the file's silence over a search-identified recording the lookup would
  not ask about for a month
  (`a_whole_rescan_keeps_the_recording_a_lookup_identified_a_file_by_until_it_is_retagged`). An
  *answered* row with unchanged file (size, mtime, sheet mtime, span) keeps all four plus
  `track_number`, `disc_number` whatever the file says: the one write replacing a file's value is a
  listener taking the name its audio was heard as (`analysis.md`), which a rescan of the same bytes
  would undo; a lookup otherwise only fills, so for other answered rows file and held agree. So a
  rescan leaves every other enrichment column where it stood
  (`a_rescan_leaves_every_enrichment_column_where_it_stood` in `tests/library.rs`); the exception
  is the retagging rule below, the only scan write of `tracks.answered`. The four declared columns
  build the ask: `asking_albums` hands `catalog_number`, `tagged_tracks`, the owner's
  `artists.mbid` to `AlbumToAsk` beside the barcode, so a search names what the tagger knew of the
  pressing.
- **`tagged_title`, `tagged_artist` are what the *file* was read as: how a rescan tells retagging
  from identification.** Enrichment writes `tracks.title`, `tracks.artist` like the scan; without a
  record of what the file said, a rescan would restore the tagger's spelling over the reference's.
  The upsert keeps an answered row's `title`, `artist`, `artist_id` where both `tagged_` columns
  return unchanged, takes the file's where either moved and sets `answered` to `NULL` in that
  `CASE` so the row is asked again
  (`a_rescan_keeps_an_answered_tracks_names_unless_the_file_itself_was_retagged`); never-answered
  rows take the file's names. The columns hold what `scan::name_from_stem` left in the `TagSet`: a
  file naming no title is read through `stem.rs` first, so `tagged_title IS NULL` means neither tags
  nor file name said anything, and `title` is the bare stem `store::title` falls back to.
  `stem::read` takes a leading number as track only where punctuation parts it (`03.`, `03 -`,
  `7)`, `003_`) or it is padded (`03 So What`); a number a space alone parts, unpadded, is the
  title's (*99 Luftballons*, *21 Guns*;
  `a_number_only_a_space_parts_from_the_title_is_the_titles_own_unless_padded`). **A name the file's
  *name* gave is not a tag; renaming is not retagging.** `tracks.named_by_its_stem` (a `MIGRATIONS`
  step) says the scan took title or artist off the stem; `store::RETAGGED` is the one reading of
  *the file said something else* every upsert column weighs: tagged names moved, not between two
  readings both off the stem. A stem-named row a lookup answered keeps what it was told when renamed
  by hand, edited or both; a tag written in or taken out is still a retagging
  (`a_row_named_by_its_file_name_keeps_what_a_lookup_answered_when_it_is_renamed`). Pre-column rows
  hold nothing, weighed the old way until next read. `store::index_row` is shared by scan and
  `land_recording`: a corrected title reaches `tracks_fts` at once, not at the next scan; the upsert
  answers the title, artist, artist id it *left* on the row, so a rescan indexes those, not the
  file's (`a_rescan_indexes_and_bills_the_names_the_lookup_kept`).
- **The seam is `Reference`; `Library::enrich` walks it.** Speaks `Mbid` (dashed lowercase text,
  else `resonate_core::Error::NotAnMbid`) and `Isrc` (twelve characters, upper-cased, dashes
  stripped, else `resonate_core::Error::NotAnIsrc`), each refusing other shapes; both in
  `resonate-core` (a provider crate names them, may not see the library), re-exported here.
  `reference.rs` carries the rest of the vocabulary: `Release`, `Medium`, `ReleaseTrack`, `Credit`;
  `Wording` (`Phrase`/`Words`: how a search is put); `ReleaseAsked`, `ReleaseMatch` (hit's `group`,
  whole `credit`), `BarcodeMatch`; `Recording`, `RecordingRelease`, `Issued`, `RecordingAsked`,
  `RecordingMatch`; `ReleaseGroup`, `GroupRelease`, `GroupAsked`, `GroupMatch`, `AlbumMatch`;
  `ArtistProfile`, `LifeSpan`, `Genre`, `ArtistRelease`, `ArtistPressings`, `Discography`,
  `ArtistMatch`; `StreamAsked`, `LyricsAsked`, `LyricDetail`, `LyricText`; core's re-exported
  `Link`, `Relation`, `Service`; from `linked.rs` `SongLink`, `LinkNames`, `AlbumLink`,
  `AlbumNames`, `Barcode` (below); `LookupOp`: twenty-three things a service can be asked, fourteen
  a reference's, the rest the lyric provider's (`Lyrics`), AutoEq catalogue's (`Devices`),
  correction source's (`Correction`), a recogniser's (`Recognise`), a scrobbler's (`Submit`, `Love`,
  `Token`), stream lookup's (`StreamLink`), link follower's (`FollowLink`). `Reference`: twenty-four
  methods (beside `source`) answering `Option`s and `Vec`s in that vocabulary, nothing about how
  reached. `resonate-online`'s `Online` is the one implementation, only the binary reaching it
  behind `online`: `cargo tree -p resonate-library` has no `ureq` or `serde`. `enrich.rs` is the
  pass: a `resonate-enrich` thread behind an `EnrichHandle`; `EnrichProgress` counts albums,
  releases, matched rows, covers, tracks, tracks a lookup renamed (`named`), artists, portraits,
  releases found for an artist, refusals, studies, fakes, recognitions, misnamed tracks, lyrics,
  songs, and answers `is_cancelled` between requests; `EnrichSummary` carries stats, whether
  cancelled, `stopped_by`. `EnrichOptions`: `refresh` (ask again what was answered), `at_most` (caps
  albums, tracks, artists each), `sought` (below), `studies`, `lyrics` (the `study`, `fetch-lyrics`
  keys). Rematch-only albums go first (a capped run still pairs every album's rows); the rest is one
  queue of `Ask`s (due albums, tracks, artists) walked in order, albums first because a landing
  album retires every track under it before the track pass reaches one. `tests/library.rs` drives it
  via a `Fake` answering canned releases, recordings, groups, profiles and faulting on the call it
  is told to (the rules below are proved there); the online crate proves its mapping over captured
  fixtures, reaching services only under `RESONATE_ONLINE_TESTS`.
- **A picture is fetched beside the pass, never in it.** `Pictures`: four threads
  (`PICTURE_READERS`), `resonate-pictures-<n>`, reading a bounded channel (`PICTURES_ASKED`, 64) of
  `Picture`s (cover, release group's cover, unheld release's cover, portrait); `Pass::want` is the
  whole of asking; `Pictures::rest` drops the sender and joins readers before the summary, so stats
  say what landed. The archive and Wikimedia Commons are not MusicBrainz and `Client::pace` keeps a
  slot per host: a picture costs the pass only the handover while the next MusicBrainz request waits
  out its second (that one request a second is what a pass is made of). An unfetchable picture is a
  warning plus a `refused` count, not the end of the pass (which finds out at its next request);
  with no reader thread started, `want` fetches on the spot. A picture call has no place in the
  pass's asking order, so `asked_in_order` leaves it out of asserted sequences
  (`a_picture_is_fetched_beside_the_pass_rather_than_in_it` holds a cover and watches the pass ask
  the next question anyway).
- **What a listener just reached for is asked next; a seek is spent once.** `Sought` is a
  `Mutex<Vec<Seek>>` shared with whoever started the pass (`Sought::album`, `Sought::artist` push,
  newest last, each held once); `Sought::taken` drains it at the top of every queue turn.
  `Pass::lift` reads it: `rotated` moves a named `Ask` to the front of what is *left*, newest
  leading (each seek rotates its match to index zero over the previous). A seek naming nothing the
  queue holds is *read back*, not dropped: `Library::album_if_due`, `Library::artist_is_due` weigh
  the row against the queue's `Waits`; where still due the `Ask` is inserted at the front. So an
  album a scan landed mid-lookup is reached by a click (a slice rotation never could); an answered
  one is not (not due: `None`). What the pass asked *this run* is refused by the `spent` set, not
  the clocks (`refresh` makes every row due; a click on the album being asked would ask it twice).
  `LibraryModel` holds one `Sought` for the window's run, hands it to every `enrich`, so a row
  reached while nothing runs is at the front when a run starts.
- **The window seeks what it has drawn, front-most last.** `LibraryModel::select` seeks the album
  and its owner; `ask_about_what_is_drawn` runs where a listing lands: an artist selection seeks
  every album the drawn tracks belong to, then the artist, in reverse listing order (`lift` rotates
  each seek to index zero in turn, so last pushed is first asked): artist, first album on screen,
  the rest down the pane.
- **An artist's row is read when asked, not when the queue was built.** `artists_to_ask` answers due
  ids, `Ask::Artist` carries one, `Library::artist_to_ask` reads the row, so the `artists.mbid`
  `Pass::credits` wrote while a *later* album landed is what `profile_of` sees. The pass once got
  that free by reading `artists_to_ask` only after the album loop; one queue holding both cannot,
  and reading where used makes ordering irrelevant, not load-bearing
  (`an_artist_named_in_a_release_credit_is_looked_up_by_the_credit_id_and_never_searched`). **An
  artist the pass brings into the catalog is asked before it ends.** A landing can bill somebody no
  row named yet (the Witcher 2 score's tracks credited Adam Skorupa, Krzysztof Wierzynkiewicz and
  Oleksa Lozowchuk by id), due only next pass with an mbid, no profile, no portrait. `Pass::run`
  re-reads `artists_to_ask` once the queue is walked (`artists_born_in_the_pass`), walking what it
  names that the pass has not spent until nothing new is due; a capped run (`--albums`) does not
  (the cap promises how much is asked)
  (`an_artist_a_landing_names_for_the_first_time_is_asked_about_in_the_same_pass`).
- **`asked`, `answered` are the two clocks, `asks`, `refusals` the columns they are read with,
  `Waits` the three waits in one value.** `due_again` writes the clause per table alias; `albums`,
  `tracks`, `artists` are read through it. Due: never asked; asked, unanswered, longer ago than the
  wait its `asks` earned; answered over `REFRESH_AFTER` (thirty days) ago and not asked in vain
  since; or anything under `refresh`. **An answer asked again in vain waits its turn like any ask**:
  a landing zeroes `asks`, `refusals`, so a row carrying either since its answer was stamped by a
  fruitless ask after it and is due only once `waited_its_turn` (the clause once read `answered <
  ?4` alone, so a stale row the reference missed or refused was due every pass;
  `a_stale_answer_asked_again_in_vain_waits_its_turn_like_any_other_ask` in `enriched.rs`). Same
  reading makes an answered row carrying a refusal due once waited, stale or not (an artist whose
  discography was refused is asked again within the hour;
  `an_answer_whose_companion_ask_was_refused_is_asked_again_once_it_has_waited`). `Waits` carries
  `retry_after`, `refused_again_after`, `refresh_after` together, not three swappable `Duration`s;
  `WAITS` is this build's, a test builds its own. Failures, via `Pass::heard`: `Error::Unreachable`
  ends the pass and stamps nothing, `EnrichSummary::stopped_by` names the `LookupOp`, a run with no
  network asks the same albums next time instead of writing a day's silence into each; `Refused`,
  `Unreadable` count in `refusals`, stamp `asked`, carry on (one refused album says nothing of the
  next) **until `REFUSALS_THAT_END_A_PASS` (10) in a row**: `Pass::refused_in_a_row` counts them,
  any answer resets it, the tenth is `Error::RefusedInARow`, ending the pass like `Unreachable`,
  `stopped_by` naming its op, the row being asked unstamped (a service refusing everything has said
  something of the next album after all; `a_reference_refusing_lookup_after_lookup_ends_the_pass`).
  `TooLarge` is heard as a refusal not counting toward that streak. Landing a release or profile
  stamps both, inside the transaction writing it. An album, track or artist gathered into another or
  removed while asked about answers `UnknownAlbum`, `UnknownTrack`, `UnknownArtist`, which
  `passed_over_if_gone` logs and passes over
  (`an_album_gone_while_a_lookup_asks_about_it_is_passed_over_and_the_pass_goes_on`); any other
  error ending the pass cancels its progress and rests the picture, study and lyric workers before
  returning (else they work through the queue).
- **Fruitless asks back off; an answer resets.** `asks` counts stampings without answer
  (`stamp_*_asked` with `Fruitless::Missed`); `due_again` waits `RETRY_AFTER` (24 h) × 2^(asks−1),
  max 2^`WAITS_DOUBLE_AT_MOST` (5) = 32 days (unnameable files: one pass, not one a day). Landings
  write `asks = 0` beside `answered`. `store::apply` zeroes `asks`/`refusals`, nulls `answered`
  where a track's `tagged_title`/`tagged_artist` changed: retagged track waits ≤ `RETRY_AFTER` from
  last ask, not a month. `refresh` reaches every row (`resonate enrich --refresh`).
- **A refusal has its own count.** `refusals` = consecutive asks ending `Fruitless::Refused`;
  stepped by `stamp_*_asked` (leaves `asks`), zeroed by miss or landing. `waited_its_turn` reads
  whichever count the last ask left (`doubled` writes one clause): refused rows wait
  `REFUSED_AGAIN_AFTER` (1 h) × 2^(refusals−1), missed rows `RETRY_AFTER` × 2^(asks−1), same cap.
  Refused for ever → asked every 32 h, not hourly; one bad minute costs an hour. Exclusive: a miss
  after a refusal waits the first-miss day.
- **A running pass leaves a mark; next launch carries on.** `enrichment` table (in `V1`) holds one
  row (starting `refresh`) while a pass runs: `note_began` writes it as `run` starts,
  `note_finished` removes it when the queue ran out or the listener stopped. Pass ended by the
  reference (`stopped_by` `Some`) or never ended (process gone) leaves it, read by
  `Library::unfinished_enrichment`. `LibraryModel::new` asks at start; row standing and network
  reachable → carries on with its refresh. A failed mark write only warns via `tracing`. Unreached
  rows were never stamped, so are due. Headless, binary's `carrying_on` does the same on both ways
  in (`resonate enrich`; scan handing over, else an ordinary pass's mark removal would *discard* an
  interrupted refresh); `--refresh` already does the wider pass, reads nothing.
- **Release by tag or strict match, artist by tag or exact match; near miss writes nothing.**
  `Identified` = `Found` | `Group` | `Nothing`. `albums.mbid` asked directly: `None` → search,
  `Refused` ends the ask (one bad minute says nothing of the tag)
  (`a_tagged_release_id_the_reference_does_not_hold_falls_back_to_a_search_that_lands`); tagged
  `albums.release_group` held nowhere → `find_group` likewise
  (`a_tagged_release_group_id_the_reference_does_not_hold_falls_back_to_a_group_search`).
  `find_release` takes title, owner, owner `mbid`, barcode, catalogue number. `top_release` takes
  `top_of` weighing `weighed_release` = `(agreed_owner, count_fits, year_agrees, score)`, lands only
  on `matches_strictly`: `agreed_owner` (score ≥ `STRICT_SCORE` 95 + credit the owner agrees with,
  by tagged id or name) and `count_fits` (`track_count` = `declared_count` = `tagged_tracks`
  (declared `TRACKTOTAL`), else rows held: a rip short a track is weighed against its pressing, not
  its own hole). Owner and score agree, count not, `group` present → `Identified::Group` (fetched
  directly, no `find_group`); else `Nothing`
  (`a_hit_is_weighed_on_its_owner_its_count_its_year_and_then_its_score`,
  `a_strict_hit_of_the_wrong_count_names_its_group_and_one_without_a_group_names_nothing`, in
  `enrich.rs`). **No owner: weighed on performers**: `AlbumToAsk::performers` = distinct artists its
  tracks bill (`performers_of`, ≤ `PERFORMERS_WEIGHED_AT_MOST` = 32); `owned_by` agrees a hit
  credited to Various Artists (MusicBrainz id or name) or via `same_credit` to one of them (a
  compilation is not another artist's same-titled, same-count release)
  (`an_album_with_no_owner_is_taken_only_where_the_hit_credits_its_performers_or_various_artists`,
  `an_album_naming_no_owner_is_not_taken_for_a_release_by_somebody_none_of_its_tracks_bill`); none
  billed: score and count alone. Artist: `Pass::profile_of` asks a tagged `artists.mbid` directly, →
  `find_artist` where the reference holds nothing
  (`a_tagged_artist_id_the_reference_does_not_hold_falls_back_to_a_search_that_lands`); hit must
  `matches_exactly`: `EXACT_SCORE` (100) + same name. A shortfall logs a debug record (what it was,
  how it fell); album/artist stamped `asked` alone.
- **A rejected match is taken away, never landed again.** `Library::forget_the_match` records the
  album's release (group where landed as group alone) in `refused_releases` (eighth `MIGRATIONS`
  step); clears the landing's writes (ids, release title, date, country, kind, disambiguation,
  `front_cover`, archive cover); removes release rows, media, links; `asked` → never (next lookup
  asks at once). **Tracks revert**: each regains the title/artist its file gave (`tagged_title`,
  `tagged_artist`), loses release title and any release-track id naming a removed row, returns to
  never asked, is re-indexed (else a wrong pressing's rename stays billed and found)
  (`forgetting_a_match_puts_back_the_names_the_files_gave_and_asks_about_the_tracks_again`).
  Recording id or ISRC a release row holds goes too (pairing stamped it; next lookup would take it
  as tagged and rename the track to the refused recording as an exact identification); row marked
  `probe_again` so next scan re-reads the file
  (`forgetting_a_match_takes_away_the_recording_ids_and_codes_it_stamped`). Label, catalogue number,
  barcode stay (tags may have given them). `take_release`/`take_group` (every route lands there) ask
  `Library::refuses` first, pass a refused id as `nothing_landed`: next lookup settles on another
  pressing or nothing. Only the release is refused, not its group (wrong *edition* → another
  pressing). Gathering carries the loser's refusals to the survivor
  (`a_match_the_listener_forgets_is_taken_away_and_never_landed_again`). **Choosing the pressing**:
  `Library::take_pressing` asks for the release, lifts any refusal of it for that album
  (`enriched::forgive`: hand-chosen outranks said-wrong), lands via `enriched::land_release`,
  re-pairs; strict match on a wrong edition fixed without forgetting
  (`a_pressing_the_listener_chooses_is_landed_in_place_of_the_one_the_lookup_took`).
- **Six ways to agree; `Spelling`'s derived `Ord` is the ranking.** `same_name`: `Marked`
  (`folded_title`s agree, marks and all), `Stripped` (only `stripped_title`s), `Dequalified` (agree
  once `dequalified` strips a version qualifier from each end). `names_it` weighs a MusicBrainz
  artist's aliases alike, lowered by `as_an_alias` to `AliasMarked`/`AliasStripped`. `same_credit`
  answers `ById` above all where a credited artist's `mbid` is the tagged one (no spelling outweighs
  a tagger-written id). Rank `Dequalified < AliasStripped < AliasMarked < Stripped < Marked < ById`:
  real name beats alias however well spelled; a title that gave up a qualifier is last. All six
  agree: tagged *Marcin Przybylowicz* matches MusicBrainz's *Marcin Przybyłowicz* (marked fold alone
  left the artist stamped `asked`, re-asked every `RETRY_AFTER` for ever). `top_of` weighs
  `(Option<Spelling>, score)`: agreement beats none, better spelling beats worse whatever either
  scored, score decides between equals (artists differing only by a mark stay two; marked tag takes
  marked row, never reverse). No agreement: `(None, score)` for all; debug record names the highest
  scored. A credit is weighed twice by `same_credit`, names joined as MusicBrainz bills them and
  each artist singly, better wins (*The Weeknd with JENNIE & Lily-Rose Depp* agrees with a file
  tagged *The Weeknd*)
  (`a_credit_agrees_where_any_one_of_its_names_does_and_an_id_beats_every_spelling`,
  `a_release_owned_by_the_tagged_id_agrees_however_the_credit_spells_it`,
  `a_collaboration_credit_agrees_where_the_file_names_one_of_its_artists` in `enrich.rs`;
  `an_album_whose_owner_holds_an_id_is_searched_for_by_that_id_and_agrees_by_it` through the pass).
  Only artist routes read aliases; `owned_by` (release/group credit vs album owner) and
  `matches_a_recording` (recording credit vs what the track was asked with) use `same_credit`;
  recording title and credit agreeing by different spellings → the weaker is the weight.
- **A version qualifier is a closed list; anything else names a different recording.**
  `VERSION_QUALIFIERS` (14: *Album Version*, *Radio Edit*, *Explicit*, *Clean*, *Remastered*, *Bonus
  Track*, *Original Mix*, …) are what `dequalified` strips from a bracketed title's end, each via
  `folded_title`; `a_dated_qualifier` adds a remaster with a four-digit year either side
  (*(Remastered 2011)*, *(2011 Remaster)* go; *(Remastered by Ada)* stays). `(with Justin Bieber)`,
  `(Live)`, `(Remix)`, `(feat. …)`, `(Acoustic)`, `(Demo)`, anything unlisted stay (different
  recording, not pressing; stripping would pair the wrong take). Strips end-inwards, `( )` and `[ ]`
  alike, until nothing more comes off, never leaving nothing: *Know (Album Version) (Radio Edit)* →
  *Know*; *Explicit* stays a title.
- **An album no pressing matches lands as its release group.** `Pass::album` reads four routes in
  order: `albums.mbid` (tags'), `find_release`, `albums.release_group`
  (`MUSICBRAINZ_RELEASEGROUPID`'s), `find_group`. The group answers a release search failing on
  count (rip missing a track, bonus disc, reissue two tracks longer): `matches_as_a_group` weighs
  score and folded owner, **no track count** (a group has none), so it answers where
  `matches_strictly` cannot; also reached from the release search when the top hit is strict but for
  count and names a group. A group names many pressings, the catalog wants one: `settle_group` →
  `closest_release` answers a pressing and a `Fit`: `Exact` (`track_count` = `declared_count`), else
  `Wider` (smallest pressing with more tracks than the album), else `Narrower` (widest with no
  more); earliest dated among equals (`earliest` sorts undated last, taken only where nothing else
  fits); `land_release` then runs on it as if the search had found it. Wider lands because the cost
  is honest: rows the rip lacks are release rows with no `track_id`, so `Album::missing` counts them
  and the Missing pane lists them (a rip weighed against its own hole landed as nothing)
  (`a_group_with_no_exact_pressing_lands_the_smallest_wider_one_or_the_widest_narrower_one` in
  `enrich.rs`, `a_hit_one_track_short_lands_the_pressing_its_group_names_and_lists_the_missing_row`
  through the pass). Narrower is the same reversed: a rip with bonus tracks no pressing has is short
  of nothing, the widest pressing is the most the reference accounts for, and landing it writes
  release columns, links and the rows it *does* name, not just a group id; the fallback weighs the
  rip's own count, not `declared_count`, so a `TRACKTOTAL` over the files held still lands the
  pressing as wide as the rip
  (`an_album_wider_than_every_pressing_of_its_group_lands_the_widest_one`). Only where no group
  pressing declares a count does `take_group` run `land_release_group`, deliberately thin:
  `albums.release_group`, `kind`, `date`, `year`, `disambiguation` coalesced (release columns a
  pressing would fill are never guessed); no `albums.mbid`, no `release_tracks` (a pressing unable
  to hold the rip is not its source). Half an answer, so asking continues: `album_due` = `due_again`
  or (`HOLDS_A_GROUP_ALONE` (`release_group`, no `mbid`) and `waited_its_turn`); the landing leaves
  `asks` as `stamp_album_asked` (`Missed`) set it: retries at a day, two, four … to the
  `WAITS_DOUBLE_AT_MOST` ceiling. Retrying pays as the album moves (rip finished, track retagged,
  count now matching a group pressing); else only the `REFRESH_AFTER` month would look again. A
  landing pressing zeroes `asks`. The archive is asked for a cover only in the pass that landed the
  release or its group, only where the album held none or a thumbnail (`wants_a_cover`).
- **Phrase first, then words where the phrase *landed* nothing.** `found_either_way` serves
  `find_release`, `find_group`, `Route::Search`: `Wording::Phrase` (online crate's fielded query),
  then `Wording::Words` (loose dismax) when weighing the first answer took nothing. Takes the
  caller's `weigh`, returns its result; `Landed` unifies the three (`Identified::Nothing`, two
  `None`s = nothing landed). Empty answer = near miss: queries differ (fielded phrase matches a
  title exactly, dismax loosely), so a pressing the phrase ranked under the wrong one, or missed
  over a dropped subtitle, is reachable only by asking again (a phrase finding the wrong pressing
  once stayed, the strict rule having weighed it). A refusal is not asked again (service's bad day)
  and is no answer: `found_either_way` returns a `Heard`, so a refused release search ends the
  album's ladder and stamps the refusal (it once read as nothing found and asked the group, phrase
  and words, of a service that had just said no)
  (`a_refused_release_search_stamps_the_refusal_rather_than_asking_down_the_ladder`). A track's
  refused search still goes on to its fingerprint (AcoustID is another host). Cost: one more search
  per album the phrase could not settle, bounded by the retry doubling. Tests:
  `a_phrase_that_answers_nothing_is_asked_again_in_words_and_lands_under_the_strict_rule`,
  `a_phrase_that_answers_a_near_miss_is_asked_again_in_words_and_lands_there`,
  `a_track_the_phrase_answered_nothing_for_is_searched_again_in_words`; group searches beside them
  asked once each; a landing phrase is never re-asked. `Looking` = one matchable (id, title) pair
  for all three, so the debug record names an album or track, not a spelled string.
- **Billed by the release where one landed, by tags until then.** `albums.title` = what the scan
  read, never rewritten by a landing (half of `store::album_key` and `sleeve_key`; rewriting would
  move the grouping key). `albums.release_title` = reference's answer, written by `land_release`
  alone; the `album_title!` macro in `db.rs`, `coalesce(a.release_title, a.title)`, is the billed
  title listings, sorts, wants, `organise` destinations read (`alternatives.rs`, `sung.rs`,
  `vaulted.rs`, `linked.rs`, `enriched.rs` spell the `coalesce` by hand). Macro, not `const`: such
  SQL is built with `concat!`, which takes literals. Serves a set's two discs (one release id and
  group, one album whose `title` is whichever disc the walk reached first; pane and layout say the
  release's title). A rescan cannot undo it (scan names no `release_title` column).
- **A medium is a row (a disc is a thing, not a number on a track).** `release_media` = `(album_id,
  position)` + MusicBrainz's `format` and `title`, written by `land_release`, read as
  `ReleaseDetail::media`; `release_tracks.disc` says which medium a row sits on.
  `models::album_rows` (`resonate-ui`) pushes a `ListedRow::Disc` (via `headed_by_disc`) above each
  run of rows where the album spans > 1 disc, in seat order only (`arranging.by_seat()`), read off
  the rows so a set the reference never answered for still heads its discs. `browser::disc_heading`
  names it from the medium where one landed, title over format, drawn with the `row` every track row
  takes (tracks pane is a `uniform_list`; a taller row lays the whole list out wrong).
  `browser::pressings`: `2 × CD` where every medium agrees, `N discs` where not.
- **Landing a release is one transaction, pairing its rows a second.** `land_release` writes release
  columns, fills `year` only where the scan left none, stamps `answered`, deletes and reinserts
  `release_tracks`, `release_media`, `album_links`, writes each row's `release_track_links`. Wants
  under the album are read first (`wants_under`) and put back after, so a want survives a refresh
  that changed row ids. `Carried` = what a want is put back *by*, most exact first: `Track` (release
  track's own mbid), `Recording` (recording's), `Seat` (`(disc, position)` **where the title's
  words, `words_of`, still agree**: a re-edited release moving a track must not carry the want to
  whatever now sits there; a song taking another's seat carries nothing)
  (`a_want_follows_its_seat_only_to_the_song_it_was_for`). `want_again` lands exact ones first;
  insert is `ON CONFLICT DO NOTHING`, so two wants on one row leave it to the better-carried; a want
  whose track the release no longer holds is dropped. `rematch_release_tracks` pairs release rows
  with catalog rows in five passes, each taking only rows earlier ones left and catalog rows not yet
  taken: recording mbid vs `tracks.mbid`; track mbid vs `tracks.release_track_mbid`; `folded_title`
  at its place; `folded_title` anywhere; place alone (disc+position vs `disc_number`/`track_number`;
  missing disc = 1 only on a one-medium release, so a two-disc set never pairs a disc-less row with
  the wrong disc). Title outranks place (else a pressing in another order stamped each file with its
  neighbour's recording id and ISRC, which the tag writer wrote and the scrobbler sent); place still
  pairs what no title answers; title-at-its-place first keeps two songs of one title (two
  *Interlude*s) each at its seat
  (`a_pressing_in_another_order_pairs_each_row_with_the_song_of_its_title`). Writes
  `release_tracks.track_id` where pairing *moved*, answers how many moved: `EnrichStats::matched` =
  what a pass changed, not what was already true. For every paired row, moved or not, fills
  `tracks.mbid`, `release_track_mbid`, `isrc` by `coalesce` from the release row, so an untagged
  file learns its seat's (`a_paired_track_receives_the_identifiers_its_release_row_holds`): fill,
  not correction (a code the file carries stands; a rescan keeps the three on an answered, unchanged
  file, takes the file's where it names one, nulls them where retagged and naming none). Fill is
  guarded on a column it would change, so re-pairing writes no `tracks` row, counts no catalog write
  (`matching_an_album_again_writes_no_track_whose_identifiers_it_already_holds`).
  `AlbumToAsk::rematch_only` makes an album holding release rows rematch whether or not due, so a
  rescan adding a file pairs without the network, but only where pairing could change anything:
  `albums_to_ask` offers it only where `HOLDS_AN_UNPAIRED_ROW` or `HOLDS_AN_UNPAIRED_TRACK` (else a
  read and a write transaction per pass for nothing). A pass rematches those
  `ALBUMS_REMATCHED_PER_TRANSACTION` (64) at a time via `Library::rematch_each`, one write
  transaction per batch; an album a lookup just landed still rematches on its own.
- **A track is asked about in its own right.** A file with no `ALBUM` tag has `album_id = NULL`,
  under no album the pass reaches, was enriched by nothing (singles, loose rips). `Ask::Track` joins
  the queue between albums and artists: `tracks_to_ask` = `WHERE NOT IS_PAIRED AND due_again("t")`
  (paired rows never queued); the `release_tracks_by_track` index keeps `IS_PAIRED` a lookup, not a
  read of every release row per track. Row read when asked via `Library::track_to_ask`, which asks
  `NOT IS_PAIRED` again: an album landing earlier in the pass pairs its tracks, which retire
  silently instead of being asked just after the answer
  (`a_track_the_album_pass_already_paired_is_never_asked_about`). Same discipline as
  `Library::artist_to_ask`; hence the queue order albums, tracks, artists.
- **Four routes, most exact first; the first answer ends the track.** `Route::ALL` = `Isrc`,
  `Recording`, `Search`, `Fingerprint`; `Pass::track` walks them, stamping `asked` alone if none
  answers. **ISRC**: `recordings_of_isrc` answers a *list* (a code names every take released under
  it); `best_recording` separates them by length and answers the take with a `Certainty`: lone
  answer `Exactly` where lengths agree or either is unmeasured, `Nearly` where length disagrees but
  title agrees by some `Spelling` (`the_only_take`; code under a title the file also carries
  evidences the recording, not which take)
  (`an_isrcs_only_take_is_nearly_the_file_where_the_title_agrees_and_the_length_does_not` in
  `enrich.rs`); several → those within `RECORDING_MAY_DIFFER_BY` (5 s) of the file, then closest,
  `Exactly`; lengthless file takes the first as `Nearly` (recording named, no carried title
  renamed). **Recording**: a `MUSICBRAINZ_TRACKID` is asked directly. **Search**: `find_recording`
  with title, length, `asked_with` (track's artist + `artists.mbid`, else album owner's name +
  mbid), and the album's billed title as `release` only where neither is known; weighed by
  `matches_a_recording`: `STRICT_SCORE`, title agreeing by some `Spelling`, credit agreeing via
  `same_credit` with whatever was asked, length within the same 5 s. Refused before the request
  where `tagged_title` is missing (a file-name-only title must not be handed alone to a text search)
  **unless the file vouches for the rest**: `named_enough_by_its_file` lets a plain stem be asked
  when tags name an artist, length is measured and the stem `names_something` (≥ 3 letters,
  `LETTERS_A_FILE_NAME_HOLDS_AT_LEAST`, digits and punctuation off, not in `PLACEHOLDER_NAMES`):
  *Track 07*, *Audio_03*, *1* ask nothing; *One of These Days.wav* by a tagged artist is searched,
  still agreeing on title, credit and length to land `Nearly`
  (`a_file_named_like_a_song_by_an_artist_it_names_is_searched_for_by_its_file_name`). Also refused
  where no artist, owner or album is known (a bare title names nothing)
  (`a_track_that_names_no_artist_is_never_searched_for` claims both halves). Fallbacks: file naming
  no artist under an album naming one → asked as that artist's; under an unowned album → by the
  album's name
  (`a_track_naming_no_artist_under_an_owned_album_is_searched_with_the_owner_and_lands`,
  `a_track_naming_no_artist_under_an_unowned_album_is_searched_with_its_release`).
- **A search answer says where the recording sits, so nothing is asked twice.** `RecordingMatch`
  carries credit and `RecordingRelease`s beside score, title, length; `into_recording` is what
  `take_match` lands, via `told_where_it_sits` (the ISRC route's rule): `/recording` is looked up
  only where the answer named no release, or (ISRC case) several with no kind between them.
  MusicBrainz's `/isrc` lookup refuses `release-groups` among its includes, so releases arrive
  untyped; `needs_its_releases_told` asks the recording whole wherever `meant_release` would
  otherwise choose by date alone. One release costs nothing more. The search index carries a
  recording's releases in full (placing the match on each by the medium's `track-offset` where the
  document gives no `position`) and its `isrcs`, so `RecordingMatch` reaches `land_recording` with
  the code and `coalesce(isrc, ?8)` fills the column. Result: one request per track, not two, on the
  route a loose-file library spends nearly all its pass in; the code an *identifier* route would
  write arrives anyway. A recording registered under no code writes none (differs from not having
  asked).
- **What a lookup may overwrite is `Certainty`, a type, not a rule each caller remembers.**
  `Exactly`: a recording id the *file itself* named, or a named ISRC whose take is as long as the
  file. `Nearly`: text search, fingerprint, an ISRC naming several takes of a lengthless file, an
  ISRC whose only take is another length under the same title. `land_recording` reads it in one
  `CASE` per column: a name is filled wherever the file named none (`tagged_title IS NULL`,
  `tagged_artist IS NULL`) whatever the certainty; a name the file *did* carry is corrected only
  under `Exactly`. So a lookup tidies *one of these days* into *One of These Days* on a
  tagger-written identifier, never renames on a score
  (`an_exact_identification_corrects_a_title_the_file_carried`,
  `a_search_match_leaves_the_title_the_file_carried_and_writes_the_identifiers_alone`). `artist_id`
  follows the same `CASE` as the billed name: a strict-score fingerprint on a file naming another
  artist neither renames nor refiles it
  (`a_near_match_credited_to_another_artist_leaves_the_track_filed_under_its_own`). Everything else
  fills, never replaces (`track_number`, `disc_number` from the recording's seat on the release,
  `mbid`, `isrc`, all `coalesce`d), except `release_title` (the route's chosen release) and
  `artist_id` (repointed via `store::artist_named_in`, billed to the artist the catalog holds under
  that fold). `asked`/`answered` are stamped in the same statement; its `RETURNING` is what
  `EnrichStats::named` counts a moved name off and `store::index_row` rewrites `tracks_fts` from, so
  a corrected title is searchable at once.
- **A track naming its release lands the album its own pass never reached.** `best_release` picks
  the release the row is filed under: album's `albums.mbid`, then a release whose `folded_title` is
  the album's, then `elsewhere::meant_release`. Album never answered (`TrackToAsk::album_answered`
  false) → `take_recording` goes on to `land_release` on that id: one identified track of an
  untagged album lands the whole release and pairs every row in the same turn. Album already
  answered → the track stops at its own row (the album's identification is the more considered).
- **The listener can place a held track on a release over the rule's choice.**
  `Library::recording_of` reads the recording a track is identified as (its `tracks.mbid`, from
  tags, a lookup or a name taken from its audio), asks the reference for it whole so releases arrive
  with kinds; `in_the_order_worth_offering` lists them as `meant_release` weighs them (the order a
  found song's releases are offered in). `Library::place_on` lands the recording with the chosen
  release under `Certainty::Nearly` (release title and a position the file never gave written; no
  carried name touched); a track under an album → the album then gets that release via
  `take_pressing` (the album card's gesture; a folder's tracks are on the release the folder is;
  what the rule seated is replaced, not weighed against). A release the recording is not on →
  `false`, nothing written. The track menu offers *Place on a release…* wherever the reference is
  reachable
  (`a_held_track_is_placed_on_the_release_the_listener_chooses_rather_than_the_one_a_rule_would`).
- **`fingerprint.rs` is the seam a recogniser fills.** `Fingerprints` answers a `Printed`
  (`Nothing`, or `Recognised` with `RecordingMatch`es) for a `Sounded`: location, span, length, the
  title and artist the *file* said (not the catalog's), the study's `Chromaprint`, so no printer
  decodes. `resonate-online`'s `AcoustId` is this build's, registered where an `acoustid-key` is
  set; `analysis.md` has the print's studies and how a recognition is weighed. `NoFingerprints` =
  the stub, registered as `unprinted`; `Fingerprinters::none()` holds it alone, `and` registers one
  per name (as `Providers::and`, `Lyricists::and`), `has_a_source` asks whether anything real is
  behind it. `Library::enrich` takes one beside the `Reference`; `Fingerprinters::recognise` asks
  each printer in turn for the first non-empty answer, returning a `Recognition` saying whether any
  refused (the pass counts it in `refused`, carries on). **A fingerprint is weighed on its score
  alone**: `recognised` takes the top match at `STRICT_SCORE`, asks nothing of title or artist (the
  audio is the evidence; a file worth fingerprinting is one whose name is not).
- **Open settles credits/sweeps orphans only if owed.** `settle_owed`: one row, set by triggers on
  track added/renamed/removed, artist added/renamed/removed, album removed; cleared by every
  `sweep_orphans`. `build` runs `settle_the_credits_if_owed`, so a reader-only process (`resonate
  stats`, a second window) writes nothing, takes no write lock, does not make the first window
  reload its vocabulary (`a_catalog_with_nothing_to_settle_is_opened_again_without_a_write`). A
  scan asks the same (`sweep_orphans_if_owed`) and **regroups copies only if owed**: `regroup_owed`,
  set by triggers on track added/removed, a write to a column `EVERY_COPY` groups/ranks by, an
  album's title/release title/owner, an artist's name; cleared by `alternatives::settle` (via
  `settle_if_owed`). An unchanged scan sweeps and regroups nothing; a lookup billing an album under
  its release still has the next scan meet its copies
  (`only_a_write_to_what_the_grouping_reads_owes_a_regroup`,
  `a_copy_billed_under_its_release_title_still_meets_one_tagged_the_same`).
  `restate_the_statistics` and the loose gathering run only if the scan wrote/removed a row;
  `credit_the_unheld` only if one arrived.
- **A collaboration is listed under every artist it credits, never as an artist of its own.**
  `track_credits` (migration step) holds each member; `credits::credit_the_members` rebuilds it from
  `tracks.artist` on every orphan sweep, finding tracks via `tracks_by_credit` (plain index on
  `tracks(artist)`; the `COLLATE NOCASE` one cannot serve a binary `=`). `members_of` splits on
  `JOINS` (`&`, `and`, comma, semicolon, `/`, `+`, `x`, `×`, `with`, `feat.`, `ft.`, `featuring`,
  `vs.`, dotless `feat`/`ft`/`vs`; space-delimited), taken **only if every part names a held
  artist**: *Adam Skorupa & Krzysztof Wierzynkiewicz* -> two composers, *Simon & Garfunkel* (halves
  name nobody) stays one. A split track's `artist_id` = its first member unless it already names
  one; text stays the file's credit; the whole-credit row is swept once nothing names it. Credits
  are read beside `artist_id` in the artist scope, `artist_albums`, `artist_tracks`,
  `WHAT_AN_ARTIST_HOLDS`, `BY_OR_HOLDING_THE_ARTIST`; the sweep keeps an artist a credit names;
  `take_over_artist` carries credits across a merge. Members come from a landed recording making a
  row for *every* artist it credits (so the split has names), and, where an artist's own lookup
  lands nothing and its name splits, `Pass::bill_the_members` searching each unheld member under the
  exact-name rule and `Library::bill_an_artist` making a row per find, which the pass asks about
  like any artist it brought in (before: Witcher 2 score tracks split between *Adam Skorupa*, where
  a recording was identified, and a whole-credit row, neither page holding the other half). **Names
  a tag lists apart are artists apart, held or not**: the codec joins multi-valued artist tags with
  `LISTED_APART_BY` (`; `, no band is named with it), so such a credit splits whatever the catalog
  holds. `each_value_named` makes a row per value (`artist_named_in`; a value that is itself a
  held-artists collaboration splits further); `Billing::of` bills a listed name to its first value
  (`lead_of_a_list`), dropping the tag's mbid (one id kept from several).
  `hand_each_list_to_its_lead` runs first in every settle, merging an artist row whose name holds `;
  ` (older build) into its lead via `take_over_artist`; a migration step marks a settle owed so old
  catalogs mend on open (`artists_a_tag_lists_apart_are_each_an_artist_though_none_was_held_alone`).
  Claims: `a_collaboration_is_listed_under_each_artist_it_credits_and_not_as_one_of_its_own`,
  `a_name_whose_halves_name_nobody_held_is_one_artist`,
  `a_collaboration_the_reference_cannot_name_is_asked_about_one_member_at_a_time`.
- **A credit names an artist the catalog may hold: identified, not asked.** `Pass::credits` walks
  the `Credit`s of a landed release or (via `take_group`) group; with an mbid and
  `Library::artist_named` finding the folded name, `write_artist_mbid` fills an empty
  `artists.mbid`, so the next artist ask goes by id, no name search. The lookup fold must be the
  column's: `artist_named` once keyed on `name.to_lowercase()` vs `artists.key` =
  `folded_letters(name)`, silently missing every marked spelling (*Marcin Przybyłowicz* searched by
  name right after the release named him by id)
  (`an_artist_is_found_by_the_fold_of_a_credit_name_however_it_is_spelled`).
- **A discography is kept once the profile lands; what the catalog lacks is read off it.**
  `Pass::artist` calls `discography` after `land_artist`: `release_groups_of` asks every release
  group the artist is credited on; `worth_keeping` keeps primary type in `KEPT_KINDS` (`Album`,
  `EP`, `Single`) with secondary types none or `Soundtrack` alone (live, compilation, remix, untyped
  out) (`an_album_an_ep_and_a_single_are_worth_keeping_and_a_soundtrack_is_still_one`,
  `a_live_album_a_compilation_a_remix_and_an_unkinded_group_are_left_out`, in `enrich.rs`).
  `land_artist_releases` (`Discographed::Afresh`) deletes and reinserts `artist_releases`:
  `(artist_id, mbid)`, title, kind, first release date, `folded` haystack from `spelt_out` (the
  three via `folded_letters`); answers rows written, counted by `EnrichStats::releases_found`.
  *Unheld* = no album's `release_group` is its mbid (`unheld_by_any_album!` in `db.rs`, via
  `albums_by_release_group`), so a landed pressing or thin group leaves the list. **A single is held
  wherever its song is**: title kept as `artist_releases.song` (`store::words_of`: letter fold, each
  non-letter-or-digit run one space), weighed by the macro against `words_of` of every title of the
  artist's tracks (deterministic SQLite function registered per connection by `schema::configure`):
  *Fearless* on the album holds the *Fearless* single however punctuated; only a single whose song
  is nowhere is listed
  (`a_single_is_not_held_only_where_its_song_is_not_and_a_discography_says_what_it_did_not_read`,
  which reads the rest too). **What was not read is said, not logged**:
  `Reference::release_groups_of` answers a `Discography` (releases + how many more the service
  credits than the browse cap read); `land_artist_releases` keeps that as `artists.releases_unread`
  and the browse's stopping point as `artists.releases_read_to` (a `MIGRATIONS` step set the
  thousand the cap always was on every artist a read fell short on). **The rest is read on
  request**: `Library::read_the_rest_of` asks for the next groups from that offset, lands them
  `Discographed::Further` (beside, not over, the first read), moves count and offset on; the artist
  page's *Read the rest* and `resonate missing --artist <NAME> --read-the-rest` each read a thousand
  more; a discography read to its end is not asked again. An unknown name is `Error::NoSuchArtist`
  from `resonate missing --artist` (with or without `--read-the-rest`, exit 1; it once read nothing,
  then said nothing of the artist was missing). `ArtistDetail::releases_unread` feeds the artist
  page's *N releases not held* button and `resonate missing --artist`; `Library::unheld_releases`
  lists them under a cap by artist, first release date; `ArtistDetail::releases_unheld` counts one
  artist's; `Library::missing_counted` answers a `Missing` (those plus release rows with no
  `track_id`); `Library::missing_tracks` lists those rows in album order with each `WantId`. **A
  discography that did not arrive is asked again within the hour**: `land_artist` already stamped
  the profile `answered`, so a refused `release_groups_of` stamps `Fruitless::Refused` (as does an
  unreachable or ending one before the pass ends); the refusal count makes the row due after
  `REFUSED_AGAIN_AFTER` (once an empty discography sat for `REFRESH_AFTER`)
  (`a_discography_refused_as_its_artist_landed_is_asked_for_again_within_the_hour`). Readers:
  `an_artists_discography_is_kept_once_its_profile_lands_and_the_catalog_says_what_it_does_not_hold`,
  `the_rows_an_album_is_short_of_are_listed_with_their_wants`.
- **What the catalog lacks can be dismissed; a dismissal outlives the release it was made
  against.** `Library::dismiss_missing`/`dismiss_release` take a missing row/unheld release off
  every reader (`missing_tracks`, `unheld_releases`, `missing_counted`, `unheld_matching`,
  `ArtistDetail::releases_unheld`) via the shared `held_or_wanted!`/`unheld_by_any_album!`. Own
  table, because what it names is rewritten (`land_release` deletes and reinserts an album's
  `release_tracks` each refresh, `land_artist_releases` an artist's `artist_releases`) and a row
  flag died with the next lookup. `dismissed_missing`: album, disc, row position, `folded` (title,
  artist, release title: stable across a refresh of the same release, not across another track);
  position came in a later `MIGRATIONS` step keying an earlier dismissal at every position its
  title stood at (hiding what it hid), so dismissing one *Interlude* leaves the disc's other listed
  (`dismissing_one_of_two_missing_rows_of_a_title_leaves_the_other_listed`). `dismissed_releases`:
  artist + release group MBID (`MIGRATIONS` step); both cascade with album/artist. **A want and a
  dismissal undo each other**: dismissing withdraws the want, `want_in` clears a dismissal of its
  row, so a row is never both asked for and hidden. **A want asked again is due at once**:
  `want_in` resets `tried`/`misses` of a standing, not-yet-offered want (every caller is a gesture:
  Want, found song, whole album), so the next poll sends for it instead of waiting out a retry or
  staying given up (`providers.md`), and forgets the offers refused for it (`refused_offers`), so
  every provider is heard again
  (`a_want_asked_for_again_is_due_at_once_however_lately_it_was_tried`). `Library::dismissed` counts
  dismissed-and-still-missing; `bring_back_dismissed` empties both tables, answers what came back
  (`a_dismissed_missing_row_leaves_the_listing_through_a_refresh_until_wanted_or_brought_back`,
  `a_dismissed_unheld_release_leaves_the_listing_and_the_artists_count`).
- **What the catalog lacks is narrowed by the words typed; each half answers with what it has.**
  All three readers take `Option<&str>`, so list, counts and sidebar figure narrow together. A
  missing track belongs to a *held* album: `narrowed_onto("album_id", "a.id")` (the grouped
  `tracks_fts` join of the albums pane) gives it the whole grammar; typing an album, its owner or a
  held track brings up what it lacks. An unheld release has only its own row: `unheld_holding`
  writes one `r.folded LIKE ? OR ar.key LIKE ?` per lone word, folded via `folded_letters` as
  `artists.key` is (title, kind, year, artist narrow it; *przybylowicz* finds *Przybyłowicz*). Only
  lone words are read (`plays:>20` says nothing of an unheld release); `playlist::words_of` already
  read them (`what_the_catalog_is_short_of_is_narrowed_by_the_words_typed`,
  `an_unheld_release_is_found_by_a_name_spelt_either_way`).
- **A cover says where it came from, the file's wins, the archive is asked until it has
  answered.** `albums.cover_source`: `CoverSource::File`/`Archive`/`Vault` (`vault.md`).
  `land_archive_cover` writes `cover_art`, `cover_format`, `cover_source` under `cover_art IS NULL`
  (and `cover_path IS NULL`): a file's picture is never overwritten, except a thumbnail the
  archive's `betters`. `Pass::album` asks in the pass that landed the release, where `has_cover` is
  false or `Library::covered_by_a_thumbnail` finds the held picture's header under
  `A_THUMBNAIL_BELOW`; such an archive cover survives a rescan and is what `resonate tag` writes
  over the file's thumbnail. A vault-held cover is not weighed (JXL bytes). `albums.cover_asked`
  (first `MIGRATIONS` step) is stamped once the archive *answered* (picture or none), never where
  the fetch failed, was refused or was cut off by the pass ending; `Pass::look_again_for_covers`
  asks at every run's end for each album with a release/group, no picture, no stamp, skipping those
  `Pass::covered` says the run asked (before: one failed fetch left an album sleeveless for good
  while Discord drew the same release's cover off its id). An answered-lacking cover is not asked
  within `COVERS_ASKED_AGAIN_AFTER` (30 days) unless the release is refreshed or
  `Library::ask_again_for_covers` clears the stamps (settings *Look for missing covers*); past it
  `albums_wanting_a_cover` reads the stamp as none (a sleeve may have been uploaded) and the answer
  restamps for another month: one request a month per uncovered album, hundreds of albums a few
  minutes at the archive's pace
  (`a_cover_the_archive_said_it_lacked_a_month_ago_is_asked_for_again`). **A release MusicBrainz
  says has no front cover is not asked of the archive**: `Release::has_front_cover` =
  `cover-art-archive.front`, kept by `land_release` as `albums.front_cover` (`MIGRATIONS` step;
  `NULL` for earlier landings, read as perhaps). `Pass::cover` asks the group's cover instead where
  the release names a group, nothing where it names none; `albums_wanting_a_cover` offers such an
  album as `CoverFrom::Group` or not at all: no monthly ask about an absent sleeve, and the monthly
  release refresh notices a later upload
  (`a_release_said_to_have_no_front_cover_is_pictured_by_its_group_and_never_by_itself`,
  `a_release_said_to_have_no_front_cover_and_no_group_is_never_asked_for_one`). An album landed as
  its group asks `group_cover` under the same two conditions (the group route lacks a release's
  rows, never a sleeve). Portraits, same shape: `land_portrait` writes under `portrait IS NULL`,
  asked only where the profile's links name a picture and none is held.
- **A portrait that missed is looked for again from the links already held.** `land_artist` stamps
  `answered`, zeroes `asks` *before* the picture is asked, and the fetch's thread feeds no column,
  so a bad-day failure waited out the thirty-day `REFRESH_AFTER` unrecorded.
  `Library::artists_wanting_a_portrait` reads every artist with `portrait IS NULL` whose stored
  `artist_links` name a picture; `Pass::look_again_for_portraits` asks each at run's end (no
  MusicBrainz request: links stored at enrichment, never read back); `Pass::pictured` stops an
  artist already asked this pass being asked twice (else threaded fetches race the sweep's read of
  `portrait IS NULL`). **A miss is remembered as a cover's is**: `artists.portrait_asked`
  (`MIGRATIONS` step) stamped once the sources *answered* (picture or none), never where the fetch
  failed; the sweep skips an artist stamped within `PORTRAITS_ASKED_AGAIN_AFTER` (30 days), so an
  artist with no picture anywhere is asked monthly, not every pass
  (`a_portrait_the_reference_answered_it_lacks_is_not_looked_for_again_the_next_pass`), while a
  refused fetch is still retried
  (`a_portrait_that_never_landed_is_looked_for_again_without_asking_the_reference_twice`).
- **A link is a relation, a service and a URL, both names read off the reference's own words.**
  `Relation::of_type` matches the exact MusicBrainz type string (`streaming`, `free streaming`,
  `official homepage`, `image`, rest of `Relation::TYPES`), else `Relation::Other`.
  `Service::of_url` reads only the host of an `http`/`https` URL (other schemes, or an authority
  with a backslash, which a browser reads as the path's start, -> `Other`), lowercases it, drops a
  leading `www.`, matches it or a parent domain against `Service::HOSTS`; `x.com` and `twitter.com`
  both `Twitter`, any host with an `amazon` label `AmazonMusic`, else `Service::Other` keeping the
  URL. `Service::name` is the lowercase figure a pane draws. All in `resonate-core`'s `link.rs`
  (the library's former `Provider` enum, renamed so *provider* means only a plugin that obtains
  media); tables still name the column `provider`; `store::service_code`/`service_of` encode it.
- **A want is a release row the catalog holds no file for; a provider fills it.** `wants`: one row
  per `release_track_id`; `Library::want` refuses an unheld row with `Error::UnknownReleaseTrack`
  and answers the same `WantId` twice for one row; `unwant` drops; `wants` reads all, newest first,
  each a `Want` (album title; row title/artist; recording and track MBIDs; album release MBID; ISRC,
  length, disc, position; own and release links). An unparsable identifier reads as absent via
  `store::mbid_in`/`isrc_in` (the tag readers). `Want::identity` makes the
  `resonate_providers::Identity` a provider gets; `supply.rs` is the pass: `Library::poll` walks
  wants `Want::due_at` says are due (every unheld one under `PollOptions::every_want`; retries,
  give-up in `providers.md`) on a `resonate-poll` thread, asks `Providers::first`, lands the answer.
  `Delivery::File` -> `Vault::keep`, `Delivery::Stream` -> `Vault::keep_delivered`; either kept
  writes the `vault_objects` row via `note_delivered` (`taken_from` = file's URI or
  `<provider>:<key>`) and `wants.offered` = the vault object's URI. No vault: filed in the music
  folder where one is set (`providers.md`); with neither, a file's own URI is the offer and a
  stream is dropped. That, a vault refusal/failure, a length disagreeing with the release row's,
  and a landing after another held the row each count `unkept` and offer nothing, so `offered`
  never names what cannot be opened. `note_tried` keeps an earlier offer where the new pass found
  none, and is not called where a provider refused, ran late or was passed over as away, or a
  registry narrowed to one provider found nothing (want stays due). `forget_delivered` clears the
  offer and records it in `forgotten_deliveries`, which `land_release`'s `wants_under` and
  `want_again` carry to the want's new row. `providers.md` has the rest.
- **A lyric fetched once is kept, and so is a miss; what is kept only gets better.** `lyrics_kept`
  is keyed `(path, span_start)` as `tracks` is (a cue row keeps its words apart from the file's);
  `text` `NULL` = remembered miss; `taken` = last asked. A kept row, and a `lyrics_refused` one,
  goes with its track: `store::ORPHANS` deletes those no `tracks` row names; the
  `lyrics_forget_a_changed_file` trigger deletes a path's when `file_size`, `modified` or
  `span_frames` moves (as `track_studies` is forgotten), so a replaced file asks again instead of
  showing the old song's words
  (`kept_lyrics_go_with_a_file_that_goes_or_is_replaced_and_stay_with_one_left_alone`). A tag run
  touches no audio, so `files_retagged` holds a written file's kept words across the trigger
  (`KeptAcross`); a remembered miss and a refusal are held only where it wrote no title or artist,
  the new name being worth asking again
  (`a_tag_run_keeps_the_words_kept_for_every_file_it_wrote_and_asks_again_after_a_new_name`). A kept row
  is a `KeptLyrics` holding `Option<LyricText>`: text, `synced`, migration-added `lyricsfile`
  (Lyricsfile document, kept only where it says more than its lines: word-timed or two overlapping
  voices). `LyricDetail`: `Plain` < `Lines` < `Lyricsfile`; `sung::keep` never trades a richer set
  for a plainer one or a miss: keeps the richer of held and told, stamps `taken` either way,
  answers whether it improved (the enrichment's `lyrics` count). `KeptLyrics::is_due` is the one
  rule both askers follow: miss after `MISSED_AGAIN_AFTER` (a week), set short of a Lyricsfile
  after `BETTERED_AFTER` (a month; a synced/word-timed set may exist since), Lyricsfile never.
  `Library::kept_lyrics`/`keep_lyrics` take `MediaLocation` + `Option<FrameSpan>`, refusing a
  non-local location via `playlist::local_path` (a row is a path). The search index is written from
  `text`, so a Lyricsfile's YAML is never what `lyrics:` reaches. The catalog holds words it never
  parses; the online crate reads them.
- **The lookup asks for every track's words beside the pass.** `Reference::lyrics` takes
  `LyricsAsked` (title, artist or album owner, album, length; read off the row when asked, so a name
  the pass just corrected is what is sent), answers `Option<LyricText>` (`None`: instrumental or
  absent; `Online` answers via the `lrclib::told` the window's `Lrclib` also asks).
  `Library::lyrics_to_ask`: every row with no kept row or a due one (all under `refresh`), cut to
  `at_most` like the other queues; `EnrichOptions::lyrics` (`fetch-lyrics` key) turns it off.
  `Verses` is one thread, as `Pictures` is four: LRCLIB is paced apart from MusicBrainz, so words
  cost the pass only the wait at its end. A refusal is counted and the row left unkept, but not
  asked on the very next pass: `lyrics_refused` (`MIGRATIONS` step, keyed like `lyrics_kept`) holds
  last-refused time and consecutive count; `sung::note_refused` steps it, `sung::keep` removes it,
  `lyrics_to_ask` skips a row refused within `REFUSED_AGAIN_AFTER` doubled per refusal under the
  same `doubled` cap the lookup's rows wait by. Own table because a `lyrics_kept` row's `NULL` text
  already means a miss the window reads
  (`a_lyric_the_service_refused_is_not_asked_for_again_on_the_next_pass`). An unreachable service
  ends the walk, not the pass. A delivered row has its words fetched like any other.
  `Lyricists::find` walks every provider for the most finely timed answer, so a file's plain words
  give way to a synced set fetched for it (`lyrics.md`).
- **The panes read what landed through seven calls; two counts ride on the listings.**
  `Library::release_of` -> `ReleaseDetail` (release columns, `CoverSource`, the two clocks, album's
  links); `release_tracks` -> `HeldReleaseTrack`s (each row's links, paired `TrackId`);
  `artist_detail` -> `ArtistDetail` (genres, links, `releases_unheld`); `portrait` -> `CoverArt`,
  sniffed where the stored format code is missing; `missing_tracks`, `unheld_releases`,
  `missing_counted` as the discography bullet says, the first two under an `Option<usize>` cap.
  `Album::missing` counts release rows with `track_id` `NULL`; `Album::mbid` and `Artist::mbid`
  come off the same listing rows, so an album grid shows which albums lack a track without a second
  read; `Artist::has_portrait` lets a list draw a placeholder without fetching bytes.
## Writing the catalog back into the files

Write-back is a seam; this build writes only what it reads back. `resonate-codec`: `TagField` (a
writable field), `TagEdit` (field + value), `Writing` (edits, `taken` fields, optional front cover,
`unpictured`, `popularity`: one call), `TagSink` (seam over `TagSource`), `FileTags` (lofty writes,
symphonia reads, so a write is weighed against what the rest of the build sees). Formats this build
would not read back are refused: `FileTags::writes` answers for AAC, AIFF, Monkey's Audio, FLAC,
MP3, MP4, Ogg Vorbis, Opus, WAV, WavPack (WAV: `riff.rs` reads the `id3 ` chunk lofty writes,
symphonia skips it); `.caf`, `.mka`, `.oga`, the two DSD containers (lofty cannot write) are passed
over. `Library::retag` is the pass behind `resonate tag` (preview until `--apply`, like `organise`);
the settings pane's *Tagging* group under Library has the same preview-then-arm shape as
*Organising*.

**Every field the `TagSet` names a writer can reach is a `TagField`** (forty): names, numbers, date,
label, identifiers, genre, seven credits, comment, BPM, compilation, grouping, copyright, four
ReplayGain values, four sort names (artist, album artist, title, album). `TagField::read` spells
each as written, so writes compare in the file's text: compilation `1`, gain `spelled_gain`'s
`+x.xx dB`, peak `spelled_peak`'s six places. `TagField::key_in` is the lofty key per tag kind (BPM:
`Bpm` in Vorbis comments, `IntegerBpm` in ID3 and MP4), `None` where lofty 0.25 has none: APE BPM,
ID3 performer. Those two are `TagField::unkeyed_in`, written into the concrete tag `saved` converts
the generic one into (as the play count is): APE `BPM` item (`beat`); ID3v2.4 `TMCL` musician
credits (`credit_performers`), one pair per name the value lists apart with `; `, role blank as
Picard writes a performer without an instrument, a name the frame already credited keeping its
instrument (a *guitar* another tagger wrote survives an edit keeping the guitarist). The reader
takes every `TMCL` pair's name as a performer whatever its role (`id3_list`). Names are append-only:
the undo record keeps fields by `TagField::as_str`.

- **A guess is never written: a name goes only where a lookup answered for its row.** `tracks.title`
  falls back to the stem, `albums.title` to whatever grouped the folder; writing either passes this
  build's reading off as a reference's. `offered` gates track name and artist on `tracks.answered`;
  album name and its two totals on `albums.answered` (offering `albums.release_title`, never
  `albums.title`: only a landed release names a pressing); album artist, its id and sort name on the
  *artist's* own `answered` (`land_release` never repoints `albums.artist_id`: that credit is the
  scan's attribution). Identifiers go whatever answered: `tracks.mbid`, `release_track_mbid`,
  `artist_mbid`, `isrc`, `albums.mbid`, `release_group`, what the tags declared about the pressing
  (date, label, catalogue number, barcode).
- **A sort name is the lookup's, only where the file names none.** `ArtistSort`: the track artist's
  `sort_name` where that artist answered and is the whole of `tracks.artist`; `AlbumArtistSort`: the
  album artist's. `KEPT_AS_THE_FILE_SPELLS_IT` drops either where the file has one (tagger outranks
  reference, as in the order;
  `a_sort_name_a_lookup_gave_is_written_only_where_the_file_names_none`).
- **Only the difference is written.** `wanted` reads the file's `TagSet` through `TagSource`, keeps
  fields whose value differs: a written library costs one probe a file, no writes
  (`RetagStats::unchanged`). Blank values are dropped before comparing, as `tags::given` does on the
  way in.
- **A picture is the catalog's to give, the file's to keep, in the same write** (one `save_to_path`,
  not two rewrites). `offered_picture` mirrors `albums.cover_source`'s rule: offered where the file
  has none or a thumbnail the album's cover `betters` (archive cover reaches a coverless file; a
  file's own picture stays unless a ripper's thumbnail). A replaced picture is noted with the run so
  putting back restores it (`a_thumbnail_a_ripper_embedded_gives_way_to_a_cover_twice_its_size`).
  The plan reads each file **once** via `TagSource::read` (`Picturing::Copied` where the album holds
  a cover to weigh against, else `Whether`); `Sleeve` holds one album's bytes while `TRACKS_TO_TAG`
  reads in path order (one blob read per folder); `written` reads back once, `Copied` where a
  picture went in. No album, no picture. Written as `PictureType::CoverFront` in the format's media
  type, *replacing* every picture the reader takes as the cover (`writing::uncovered` removes front
  cover, `Other`, untyped: the set `probe::choose` draws from), so a file never collects two. lofty
  reads every MP4 `covr` as `Other` where symphonia reads front cover, so removing front cover alone
  left an m4a's cover with a new one behind it, never drawn
  (`a_cover_taken_away_is_gone_and_a_cover_written_replaces_the_one_there`). `written` weighs the
  picture read back against the bytes sent like each field; a container dropping one is
  `Unwritten::Unconfirmed`, catalog unmoved. `RetagStats::pictures` counts apart from `fields`; a
  picture alone is still a write.
- **A favourite is this build's rating, plays the count every player reads; anybody else's stay.**
  `TrackToTag::popularity` rides in `Writing::popularity`. Favourite: lofty's generic popularimeter
  under `resonate_codec::RATED_BY` (`resonate`: software, nothing about the listener), five stars;
  non-favourite: this build's rating removed. Lands wherever lofty maps one: ID3v2 `POPM` (counter
  kept), Vorbis `RATING:resonate`, MP4 `rate`, RIFF `IRTD`. APE has none: WavPack and Monkey's Audio
  carry FMPS `FMPS_RATING` `1.0`, removed likewise. Another player's rating under its own name
  stays; where a format names nobody (MP4, RIFF, APE hold one rating) that one is ours. **Plays are
  written favourite or not**, under each tag's FMPS name: `FMPS_PLAYCOUNT` (Vorbis, APE), `TXXX`
  `FMPS_PlayCount` (ID3v2), `----:com.apple.iTunes:FMPS_Playcount` (MP4); zero is removed. lofty's
  generic `Tag` drops a name with no `ItemKey`, so `counted.rs` converts it into the format's own
  (`VorbisComments`, `ApeTag`, `Ilst`, `Id3v2Tag`; what lofty's save does), sets the count there,
  saves, reads the count back off the same concrete tag. `TagSink::rated` reads what the file holds,
  not the `TagSet` (symphonia reads `POPM`, ignores a Vorbis rating): lofty once, file type guessed
  first (`guessed_for_its_tags`), primary tag type from the type alone; a count-keeping format is
  parsed by its own reader (`Counted::read`), the generic tag the ratings come from being that
  converted back (`Counted::tag`), others generically; neither parses audio properties or covers (a
  write opens the file whole, saving what it read). `Rated::Unrated`/`Favourite` carry the count the
  tag keeps (an MP3 written before counts has only our `POPM`'s: reads that). `Rated::differs_from`
  weighs the favourite always, plays wherever the tag keeps a count (a play since the last run
  rewrites it; RIFF `INFO` keeps none: favourite alone). Undo keeps both in one integer: favourite's
  plays as is, unfavoured row's negated less one (the `-1` an older build wrote for *unrated* reads
  so). `RetagStats::ratings` counts them
  (`a_favourite_and_its_plays_are_written_into_the_file_and_taken_away_again`,
  `a_play_count_is_written_into_a_file_nobody_marked_a_favourite`,
  `a_play_count_and_a_favourite_read_back_out_of_every_tag_this_build_writes`).
- **A field cleared is cleared from every tag the file holds.** lofty edits only the primary tag (a
  WAV's `id3 ` chunk, an MP3's ID3v2); the reader takes a name from whichever tag still says it (a
  title removed from a WAV's ID3 came back from `INFO`, an MP3's from ID3v1).
  `writing::cleared_elsewhere` copies every other tag holding a field in `Writing::taken`, removes
  it, saves each into the same staged copy after the primary: one settle
  (`a_field_cleared_from_a_wave_file_is_gone_from_its_info_list_too`). A field *set* needs no pass:
  the primary outranks the rest wherever the reader weighs them (`audio.md`).
- **An ID3v2.3 tag stays v2.3.** lofty writes v2.4 unless told; a v2.3-only player loses an upgraded
  tag. `FileTags::write` asks `Counted::holds_id3v2_3` (the file's ID3v2 read whole; four kinds
  carry one) and saves every tag through `WriteOptions::use_id3v23` where it was, lofty folding v2.4
  frames back (`an_id3v2_3_tag_is_written_back_as_the_version_it_was`). No tag: v2.4.
- **A write never touches its file until it is whole.** lofty's `save_to_path` splices a FLAC's
  metadata and shifts audio in place (full disc or killed run: truncated file). `FileTags::write`
  copies to a staged sibling `.<stem>.<pid>-<n>.<ext>` (`staged_beside`; extension kept, lofty reads
  the kind off it), writes tags into the copy, `sync_all`s, renames over the file, syncs the folder,
  removing the copy on any failure. A killed writer's copy is swept by the next write to that track:
  `sweep_what_a_dead_writer_staged` removes a sibling `staged_by` reads as this track's, staged by a
  pid neither ours nor under `/proc`
  (`a_copy_a_dead_writer_staged_beside_the_track_is_swept_and_no_other`); the scan never catalogs it
  meanwhile (dot-names are passed over). **The copy lands only over the file it was taken from.**
  `Taken::of` asks `access(W_OK)` first: an unwritable file is refused `PermissionDenied`, not
  replaced via a writable folder (`a_file_nobody_may_write_is_refused_rather_than_replaced`), and
  holds device, inode, length, mtime; `landed_through` weighs `Taken::still_stands` before the
  rename: an edit another program made meanwhile answers `Error::ChangedWhileWritten`, copy removed
  (`a_file_another_program_changed_meanwhile_no_longer_stands_as_taken`). A folder the listener may
  not create in does not refuse a file they may write: a clone or copy refused there falls to the
  in-place edit, else `landed_from_elsewhere` stages under the spool's folders (`/var/tmp` then the
  temp dir; a set `TMPDIR` replaces `/var/tmp`) and writes back over the file through `written_back`
  (`a_file_in_a_folder_nothing_may_be_created_in_is_written_all_the_same`). The copy is a clone
  (`cloned_beside`, `FICLONE`) where the filesystem shares extents (btrfs, XFS): the tag's bytes
  only. **Where it cannot clone (ext4, tmpfs), an edit the tag's own room holds lands in the file
  itself**, not a gigabyte copy: `landed_in_place` has lofty write into an `Overlay` (file seen
  through 4 KiB in-memory pages); `Overlay::land` writes back only differing pages, then
  `sync_data`s, only where the file keeps its length and at most `MOST_BYTES_WRITTEN_IN_PLACE` (32
  MiB) was touched; audio that would move or a tag at the end that would grow goes through a whole
  staged copy. **A page landed in place can be taken back:** before the first page,
  `Undo::kept_beside` writes and syncs a journal beside the track
  (`.<name>.<pid>-<n>.resonate-undo`: track name and length, each changed page's bytes before and
  after, FNV-1a check over the lot), removed once pages sync; a write failing part way rolls back at
  once. A dead writer's journal is mended by the next write to that track
  (`mend_what_a_dead_writer_left`) and by the scan, which looks in each folder before reading a file
  there (`mend_a_cut_short_write`): same length and every byte the journal's before or after: rolled
  back to before, unless all after (write finished, journal outlived it); other length or bytes is
  another program's, left alone; a journal failing its check was cut short before any page moved:
  only removed (`a_write_cut_short_between_its_pages_is_rolled_back_whole`). No journal possible
  (folder nothing may be created in): pages land without one. lofty 0.25 keeps a FLAC's padding
  block as was and pads an ID3v2 tag with a fresh 1 KiB, so neither keeps its length: for those two
  `Head` has lofty write into an in-memory copy of the tag's head (plus 64 KiB past it, so the kind
  still probes) and `Head::absorbed` fits the result into the head's old length (FLAC: blocks plus
  one padding block sized to the room left; ID3v2: frames zero-padded to old size) or answers it
  will not fit. MP4 `free` atoms already absorb an edit
  (`an_edit_the_padding_holds_lands_in_the_file_itself_rather_than_a_copy`,
  `an_edit_a_leading_id3_tag_has_room_for_lands_in_the_file_itself`,
  `an_edit_that_moves_the_audio_is_left_to_a_whole_copy`). **What the rename would lose is kept.** A
  symlinked track is written where the link points, staged beside the target (link stays a link).
  The staged copy gets the file's owner, mode, extended attributes (`carries_what_the_file_did`:
  `chown`, `set_permissions`, every `listxattr` name, ACLs and SELinux labels included, via
  `rustix`). Where any is refused (another user's file in a shared folder, a label only root may
  set) and wherever the file has a second name (`nlink` over one), the whole tagged copy is written
  back into the file's own inode instead (`settled_over`; in-place edits never need it): keeps
  everything and both names, at the price of a second copy and a window where the file is part
  rewritten. Before that window the synced copy is renamed `.<name>.<pid>-<n>.resonate-whole`
  (`journal::WholeCopy`); a write-back failing part way renames it `.resonate-torn` and answers
  `codec::Error::WrittenBackPartway` naming it, never removing the one whole copy. A dead writer's
  whole copy, and any torn one, is finished by the next write to that track or the scan of its
  folder (`Mended::WrittenBack`), never swept as left over
  (`a_track_a_dead_writer_left_part_written_back_is_finished_from_its_whole_copy`). A copy staged
  in the spool folder (the track's own folder refusing one) is kept there, named by the error
  (`a_track_reached_through_a_link_is_written_where_the_link_points_and_stays_a_link`,
  `a_track_with_two_names_keeps_both_and_both_read_the_write`,
  `a_tracks_extended_attributes_survive_a_write`).
- **A row cut out of a shared file is never written to.** Cue rows are readings of one file with one
  tag set: a path with more than one row, or one row carrying a span, is `Unwritten::Cut`, passed
  over whole (why `Library::track_played` keys a play on the pair: a cut is a row of its own
  everywhere but in the file).
- **The catalog follows the file, which the next scan reads.** A write changes size and mtime, what
  `Known::under` compares, so `files_retagged` writes both back (next walk: row unchanged). It also
  writes `tagged_title`, `tagged_artist`, `title_sort`, `artist_sort`, only for fields it wrote.
  `tagged_title`/`tagged_artist` tell a retagging from an identification: a corrected title written
  but unrecorded reads as the tagger moving it, takes the file's names back over the reference's,
  nulls `answered`, so the whole library is asked again next non-incremental scan
  (`a_rescan_reads_this_builds_own_write_as_the_names_it_already_knew`). A written file's
  `track_studies` and `unstudied` rows are held across the follow (`analysis.md`).
- **A write is confirmed by reading it back; an unconfirmed one is not followed.** `written`
  re-reads through `TagSource` after `TagSink::write`, weighing every edit, taken field, rating and
  picture; a mismatch leaves `Unwritten::Unconfirmed`, catalog unmoved: a format quirk shows as a
  refusal, not a preview offering the same edit for ever (`FileTags::writes` refusing a format is
  the cheap half). **A WAV whose ID3v2 tag stands before its `RIFF` header has the tag moved into
  the RIFF**: lofty does not recognise such a file, so `FileTags::write` first stages a copy that is
  the RIFF plus the leading tag's bytes as an `id3 ` chunk, header size grown (whatever followed the
  RIFF still following), and edits and settles that copy in the file's place: all tag fields kept,
  audio byte for byte
  (`a_wave_file_tagged_ahead_of_its_riff_header_has_the_tag_moved_into_a_chunk_and_written`).
- **`Library::retag` takes the `Walk` guard**: scan and write-back cannot overlap (the pass rewrites
  the sizes and mtimes a scan's snapshot was taken against, `organise`'s hazard); a second caller
  gets `Error::AlreadyWalking`. A vaulted row is `Unwritten::Vaulted` (`vault.md`).
- **The preview is the plan.** One `Retagging` is built, handed to the apply or not: `resonate tag`
  and `--apply` cannot disagree; a failing write moves from `Retagging::writes` to `passed_over`, so
  what prints after an apply is what was done. The *Tagging* group draws the same plan (`ui.md`).
- **The last applied run can be put back.** Every write carries a `Held`: each touched field as read
  before (`None` where none), the rating where changed, a replaced picture. The apply notes it in
  `retagged` and `retagged_fields` (a `MIGRATIONS` step) with whether the write added the album's
  cover; a run's first page writing anything clears the previous run's notes. **Note before files**:
  `apply` notes every planned write of a page (`Library::retag_to_be_written`) before touching one,
  then follows the catalog and forgets the notes of failed or cancelled writes in one transaction
  (`files_retagged`). The reverse order let a catalog error leave files changed with nothing to put
  them back by; now an unwritable note stops the run before any file
  (`a_tag_run_the_catalog_cannot_note_writes_no_file`), an unwritable follow leaves the record whole
  and sizes and mtimes reading as changed next scan. Price: a run whose every write fails still
  clears the previous record (`a_write_that_fails_is_not_noted_as_one_to_put_back`). **A write that
  landed but reads back otherwise keeps its note**: the file changed, so `Noted::keeps_a_note_of`
  holds `Unconfirmed` out of what is forgotten; the catalog does not follow it (next scan reads the
  new size; `a_write_that_lands_but_reads_back_otherwise_can_still_be_put_back`).
  `RetagOptions::undo` plans from that record, not the catalog: each field read before written back,
  one the run added removed (`Writing::taken`), an added cover taken out (`Writing::unpictured`,
  through `uncovered`), rating put back; all through the same `apply`, which reads every file back,
  has the catalog follow `tagged_title`/`tagged_artist` as they now stand and notes what it
  replaced, so putting the walk back writes the run again (except a cover: the walk back has nothing
  to put back but its absence). **A walk back cut short leaves the rest to the next**: clears
  nothing ahead; `Noted::walking_back` holds the record read; a write that fails or reads back
  otherwise has its old note put back (`note_again`); unless every noted file was put back
  (`retag::walked_back`) the files that were lose their notes, so the next walk back finishes
  instead of writing the run into what was already restored; pictures no note names are swept either
  way (`a_tag_walk_back_cut_short_keeps_what_it_did_not_put_back_for_the_next_one`).
  `resonate tag --undo` previews, `--undo --apply` writes; the *Tagging* group offers *Put the last
  run back* behind a second press wherever `Library::retag_walks_back` says a run is noted. A file
  gone since is passed over as unreadable; one that moved is not found by its old path
  (`an_applied_tag_run_is_put_back_field_for_field_and_putting_it_back_again_writes_it_again`). **A
  replaced picture is kept once, held no longer than its page.** A cover giving way to a better one
  is noted in `retagged_pictures`, `retagged.picture_id` naming it: `KeptPictures` weighs each
  against the last eight kept (`PICTURES_WEIGHED_AGAINST`), by pointer then bytes, so the thumbnail
  twelve tracks of one album carried is one row (a `MIGRATIONS` step folds the earlier per-file
  record into the table). `run` drops `Held::picture` from every write once its page is applied: a
  whole library's summary holds none. The walk back reads notes without pictures and each picture by
  id as a file first asks for it, once (`PicturesPutBack`), not every row's copy up front
  (`a_picture_the_last_tag_run_kept_per_file_is_kept_once`).
- **Rows are read a page at a time; a file is never split across two.** `retag::run` walks the
  catalog through `paged::Paging`: `ROWS_A_PAGE` (2 048) rows in path order past the last path
  handed out, the rows of the file a page ended in held back for the next, the page doubled where
  one file's cue rows fill it. Each page is planned and, under `--apply`, written and followed
  before the next is read. Release totals (what every row is weighed against) are read once. Memory
  holds only the plan's writes, which the preview prints; a 500 000-row catalog is not held whole
  (`every_row_is_handed_out_once_and_no_file_is_split_across_two_pages`: every page size from one
  up).
## Deleting

- **A deleted track takes its file, every row cut from it, its want and its vault object.**
  `Library::delete_tracks` (`deleted.rs`): `Walk` guard; distinct paths of the tracks
  (`Error::UnknownTrack` for an unheld id); files removed (already gone = removed; refused: logged,
  `Deleted::kept`, rows left). Per path gone, one transaction: drop `wants` naming a release row its
  tracks fill or offering the path (else the next poll refetches it), drop every `tracks` row at it
  (cue cuts go with the file; plays, listens, studies cascade), then `store::sweep_orphans` and
  `alternatives::settle` as after a prune. A vault object no row names leaves the vault, its row
  forgotten; one that will not go waits for `prune_the_vault`. Nothing is kept to put back. The
  window asks first (`ui.md`).
  (`deleting_a_track_removes_its_file_and_its_row_and_leaves_the_rest`,
  `deleting_a_delivered_track_takes_its_vault_object_and_its_want`)

## Organising

`Library::organise` files every scanned track under a layout (`organise-as` key, `--as` per run) via
`resonate organise` and the settings pane's *Organising* group. Takes `Library::scan`'s `Walk`
guard; re-keys a sleeve-keyed album after moves land (both under *Schema and grouping*).

- **`Layout`: segments split before pieces are read.** `Layout::read` splits on `/`, then reads
  `Piece::Literal`s and `Piece::Named(Field)`s, so no `/` reaches a literal or field. Empty segment:
  `LayoutFault::EmptySegment` (leading `/` refused, not absolute); `.`/`..`: `Error::LayoutEscapes`
  naming its index. With `as_one_component` writing a value's `/` as `-`, that is the whole guard:
  neither a tagger's text nor the template can name a path outside the file's root (hence no
  `Refusal::Escapes`, nothing could construct one). `{{`/`}}` write a brace; unclosed `{`:
  `LayoutFault::Unclosed` at its byte offset; unknown name: `Error::UnknownLayoutField` carrying a
  `FieldName`. Refused when `organise-as` is *read*: a typo is a startup error, never a half-moved
  library. `Display for Layout` writes the template as read (the settings field draws it;
  `DEFAULT_LAYOUT` = `{albumartist}/{album}/{disc}{track} {title}` is asserted against it).
- **A component is what a filesystem takes; a segment resolving to nothing is dropped.**
  `as_one_component`: `/` to `-`, control characters and delete dropped, whitespace and dots
  trimmed at both ends, since a scan passes a dot-name over and would prune what was filed there
  (*...And Justice for All* files as *And Justice for All*, *.38 Special* as *38 Special*; `..` is
  nothing), cut
  to `COMPONENT_BYTES` (255) on a character boundary. Extension appended only if the layout lacks
  `{ext}`; last segment's budget is 255 less it, so a cut name still ends `.flac`. A last segment
  ending `.{ext}` is budgeted alike (`Segment::write_ahead_of_the_extension` writes what precedes
  the dot, cut; extension re-added), else the extension is lost and the next scan prunes the file
  with its plays (`a_long_name_cut_to_fit_keeps_the_extension_the_layout_names`). Middle segment
  nothing: skipped; *last*: `None`, read as `Refusal::Unidentified` (file stays). **Volume limits
  are per root**: `Naming::of` finds the root's mount in `/proc/self/mounts` (deepest mount point
  above it, octal escapes read back); `vfat`, `msdos`, `exfat`, `ntfs`, `ntfs3`, `fuseblk`, `fat`,
  `cifs`, `smb3`, `smbfs` give `Naming::Portable`, also writing `\ : * ? " < > |` as `-` (else a
  title ending `?` is a rename refused every run); other roots keep all but the separator. **Derived
  names obey the 255 bytes**: `noted_staging` cuts the file name to fit `.resonate-staging`,
  `a_place_to_park` the stem to fit `.resonate-parked-<pid>`; `in_the_way` treats a destination file
  name over a component as untakeable (loose sidecar left; sheet travelling with its audio refuses
  the move) (`the_names_derived_beside_a_longest_name_still_fit_a_component`).
- **`{albumartist}` falls back to nothing, never `{artist}`.** Its column is the album's own owner;
  an album whose tracks disagree has none (what the compilation flag meant). The track artist would
  scatter a compilation into a folder per singer (the split the grouping's third tier prevents); the
  album folder sits under the root. `{disc}`: `N-` only if the album has more than one disc.
- **Disc is read as the scan reads it; `max(disc_number)` alone is not enough.**
  `scan::disc_in_folder` has two callers: `scan::sleeve` (`CD1`, `CD2` into one album) and
  `organise::disc_in` (via `disc_of`; no tag names a disc). A set so filed with no disc tags has no
  `disc_number`: `{disc}` empty, both discs' track 1 one file. `Planner::knows` raises each album's
  count (per album, not row, so every track of a set agrees) to the larger of stored
  `max(disc_number)` and what the folders spell.
- **Library-wide reads are lean and paged.** Only destinations (vs every source path), a root's
  `Naming` and an album's disc count are library-wide: `Planner::knows` is fed `Filing`s (path,
  root, album, disc) by a cursor collecting nothing; rows come via retag's `paged::Paging`. A sheet
  tying files is followed across a page (missing members read by path, set filed together, each
  member's own page later skipping it). The plan stays whole (preview lists every move; chains
  ordered across all)
  (`files_one_sheet_names_are_filed_together_even_when_a_page_holds_only_one_of_them`).
- **The preview is the plan the apply performs.** `organise::run` builds one `Plan`, hands it to
  `apply` only if `OrganiseOptions::apply` (no second walk or rules); the settings pane's
  `Pass::Preview`/`Pass::Apply` are one call with one flag. A preview still reads the filesystem
  (`symlink_metadata` per destination, `read_dir` per source folder): collisions and sidecars are
  facts about the disc. `Plan::folders`: folders all of whose files go, deepest first. Apply ignores
  it: `prune` takes landed moves' source folders, `climb_out_of` walks each upward removing while
  empty, stopping at the first non-empty folder or the root; both sort `deepest_first`, so a preview
  names what an apply would take, in its order.
- **Files move before the catalog, in batches; an unfinishable batch is put back.** `apply`:
  `MOVES_PER_BATCH` (256) at a time; cancel heard before each move (landed moves settled and
  followed by the catalog, rest not begun) (`a_cancel_is_heard_between_the_moves_of_a_batch`). Per
  move: `standing` re-weigh (vanished source `SourceGone`, destination now taken `Collided`), rename
  with sidecars, record in `done`. Sidecars weighed apart (`with_the_sidecars_that_can_go`): gone
  since: dropped; destination now taken: stays; track moves without (`rename` once overwrote the
  file there; a deleted sidecar refused its track on every undo)
  (`a_walk_back_leaves_a_sidecar_rather_than_overwrite_a_file_or_refuse_its_track`). `settle`
  fsyncs written folders; `Library::files_moved`, one transaction, rewrites `tracks.path`,
  `playlist_entries.path` (touching those playlists' `modified`), `lyrics_kept.path`,
  `resume_rows.uri`: plays, playlist rows, kept lyrics, kept queue follow the file, not a rescan
  into a new row. It first deletes any `tracks`/`lyrics_kept` row at the destination (path-keyed,
  else the `UPDATE` is refused; `standing` weighs the *file*, so a row whose file had gone, untidied
  by a scan of another root, once failed all 256 moves of its batch). `playlist_entries` and
  `resume_rows` need no delete (not path-unique); indexes `playlist_entries_by_path`,
  `resume_rows_by_uri` avoid a scan per file. The open queue follows: window-run organise:
  `LibraryModel` keeps an applied run's moves, root view sends `Command::Relocate`;
  `resonate organise --apply`: every bus player via `org.resonate.Player1`'s `Relocate`
  (`mpris.md`). `Queue::relocate` rewrites every row naming a moved file (playing track too),
  following each through the moves in landing order (`queue::landed_at`): a cycle broken through a
  parked name lands each row where its file did, not at the parked name, and the next resumption
  written carries the new paths instead of overwriting them
  (`a_queued_row_whose_file_was_moved_is_reached_where_it_went`,
  `two_files_trading_names_through_a_parked_one_are_each_followed_to_where_they_landed`). A failing
  rename puts back only its own move's steps (audio, sidecars already gone), refused as
  `Refusal::Unmoved` with the volume's `io::ErrorKind`, batch going on (whole-batch put-back for one
  refused file failed identically every run). A failing catalog write runs `put_back` over all of
  `done` in reverse; whole batch counts `failed`. Order matters: a crash between the two leaves a
  moved file the catalog has not followed (next scan reconciles); the reverse leaves the catalog
  naming absent files. Batch size bounds what a failed catalog write can undo
  (`a_move_the_volume_refuses_is_refused_alone_and_the_rest_of_its_batch_lands`).
- **A batch takes back the folders it made and did not fill.** `create_dir_all` does not say which
  levels were new: `not_there_yet` first records the missing ones walking up from the destination's
  parent; `batch_moved` runs `take_back_the_empty` over them whatever the outcome. `taken_away`
  removes only an empty folder (one with a landed file survives, one left by a rollback goes),
  deepest first. No climbing: never an already-standing empty folder.
- **A chain is ordered; only a cycle is refused.** `Planner::in_the_way`: `InTheWay::Stands` for a
  destination another planned move claimed or one the filesystem holds that no scanned row names
  (except a file moved onto itself, same device and inode); `InTheWay::MayGo` for a scanned row
  (whether it goes is unknowable until every row is read). **A destination that is the track itself
  is reduced to its own name in its own folder** (`as_the_volume_names_it`): on a case-insensitive
  volume a case-only layout difference resolves to the file itself, so only the file name's case
  changes (a folder spelled otherwise stays as the volume spells it; a right name is unchanged);
  `standing` lets a move onto the same inode land, not refuse as colliding with itself every run
  (`a_destination_that_is_the_track_itself_under_another_case_is_never_a_collision`).
  `Planner::order_the_chains`: a move waits on at most one other, so the graph is functional and
  `walked` follows each chain to its end, deepest first (`B → C` before `A → B`, one run). **A cycle
  is broken through a parked name.** `parked_out_of_their_cycles` finds each cycle of single-file
  moves (each waiting on exactly one other) and splits one: its file goes first to
  `<stem>.resonate-parked-<pid>.<ext>` (`PARKED`) beside where it stood, waiting on nothing, then to
  its destination, waiting on what it waited on; the walk orders the rest of the cycle between, so
  files filed under each other's names trade places in one run
  (`two_files_filed_under_each_others_names_trade_places_and_keep_their_plays`). The catalog follows
  each step in the batch's one transaction (the row sits at the parked name no longer than the
  batch). `Move::parks` names the first half: listed by the preview, counted as moved by nothing. A
  failure between the halves leaves a file under a visible name the catalog follows, filed properly
  next run; a run killed between rename and commit leaves a file the next scan follows by
  `moves::follow_the_moved` like any hand-moved file. A move leading into a cycle a unit sits in,
  and every move waiting on a row that stays, is still `Collided`; a move refused there releases its
  source and sidecars from `going`, so `empties` counts only folders all of whose files really
  leave. `Refusal`: `Unidentified`, `Loose` (destination with no folder between it and the root),
  `Collided`, `SharesASheet`, `NamedFromAbove`, `SourceGone`, `Unmoved`, each printed against the
  path it left standing.
- **A rename crossing a filesystem is a copy; the source goes only once the catalog has followed.**
  `landed_onto` reads `io::ErrorKind::CrossesDevices` off `fs::rename`, falls back to `copying`:
  copy, carry the source's modification time onto the copy (so the next scan reads the known file,
  not one to probe again, which would re-identify a stem-named row against its new name),
  `sync_all`. Written as destination name plus `STAGED`, renamed into place once whole and synced (a
  killed run leaves a staging file, not half a file under the name). Nothing is deleted inside the
  batch: `put_back` undoes a copy by *removing* it, original standing; `left_behind` removes sources
  only after `Library::files_moved` committed. A run killed after the rename, before the commit,
  leaves the whole copy beside its source; `already_copied` reads it next run: another device, same
  size, same modification time (the copy carries the source's), same bytes (`COMPARED_AT_ONCE`
  pieces, only once the three cheap readings agree). Such a destination is not in the way, not
  copied again, lands as `Landing::Copied` (catalog follows, source goes); hence no
  `Refusal::AcrossDevices`
  (`a_copy_a_killed_run_left_whole_on_the_other_filesystem_is_taken_as_landed`,
  `a_file_of_the_same_size_and_time_but_other_bytes_is_still_in_the_way`).
- **A staging file is noted before it is written, so a killed run's is swept by the next.** It sits
  beside a destination the next run may never plan again (layout moved, file retagged), so walking a
  plan cannot find it. `noted_staging` writes its path and this process's pid into `staged_writes`
  (fourth `MIGRATIONS` step), committed before a byte of the copy or rewritten sheet is written;
  `staged_left` removes the file where it stands and drops the row once it is gone, landed or not
  (an unremovable file keeps its row). A run that applies begins with
  `sweep_what_a_killed_run_staged`: removes each noted file (only a regular file whose name ends
  `STAGED`, so a row never costs a file it did not name), skipping a row whose pid is another
  process `/proc` still holds (`Walk` is this process's alone; a window and a
  `resonate organise --apply` may both be copying). A preview writes and sweeps nothing
  (`what_a_run_that_was_killed_staged_is_taken_away_by_the_next_run_that_applies`).
- **The last applied run can be walked back.** `organised` (a `MIGRATIONS` step) holds what the last
  apply landed (units, companions and sidecars, landing order), each moving apply replacing it.
  `OrganiseOptions::walk_back` plans from it, not a layout: units in reverse, each
  `Move::reversed` (chains and parked cycles undo in the order that makes room), through the same
  `apply` (batches, catalog following, a sheet's `FILE` line renamed back, made folders pruned).
  What the walk back landed is noted in turn, so walking back again files the tracks again, every
  unit where it landed. A run cut short (cancel, refusal) keeps the units not put back and drops
  those put back (`still_to_walk_back`); the next walk back finishes
  (`a_walk_back_cut_short_keeps_what_it_did_not_put_back_for_the_next_one`).
  `resonate organise --undo` previews, `--undo --apply` makes it; *Organising* offers *Put the last
  run back* behind a second press wherever `Library::walks_back` says a run is kept
  (`an_applied_run_is_walked_back_file_for_file_and_walking_it_back_again_files_them_again`). A file
  moved or gone since is refused at `standing` like any other.
- **A run given files files those alone.** `OrganiseOptions::only` empty is every row; named paths
  read just those rows via `tracks_to_file_at`, `ROWS_A_PAGE` at a time, not the paging walk (a
  drop's filing costs its own rows). Every file is still *known* to the planner (collisions and
  chains are facts about the whole library); a sheet naming a file not given still reads it as a
  member (`a_run_given_files_files_those_alone_and_leaves_the_rest_where_they_stand`).
- **A folder's pictures follow its tracks where every track leaves for one folder.**
  `Planner::pictures_follow_their_folders`, after chains are ordered: a source folder all of whose
  scanned tracks go, all into one folder (`FolderLanding`): each picture in it (`names_a_picture`)
  is a sidecar of the last move out, landing under its own name unless something stands or is
  claimed there. Scattering tracks, or one kept behind, keep the pictures (the sleeve belongs to
  what stays too). Noted with the run, so a walk back brings them home
  (`a_folders_pictures_follow_its_tracks_only_where_every_track_lands_in_one_folder`).
- **A run files the roots it is given; one the catalog does not hold is refused.**
  `OrganiseOptions::roots` empty is every root (settings pane, bare `resonate organise`); named
  roots put `roots.path IN (…)` on the `TRACKS_TO_FILE` queries, so unwanted rows never leave
  SQLite.
  The held-root check (`Error::NotARoot`) runs on the pass thread, not `organise::start`, which may
  not read the catalog before taking the `Walk` guard (the read blocks behind a writer the guard
  waits on; caught by `a_scan_refuses_to_start_while_an_organise_is_running`). `--as` is the
  one-run layout, read via `Layout::read` like `organise-as`, so a typo is refused before anything
  moves.
- **A file beside a track sharing its name travels with it.** `Planner::sidecars`: files in the
  track's folder that are not scanned rows, named stem plus `.` and more, extension not audio
  (`Meddle.cue`, `Meddle.wav.log` follow `Meddle.wav` onto the destination's stem). A file also
  carrying a longer-named audio file's stem beside it (`Song.live.lrc` beside `Song.flac` and
  `Song.live.flac`) is left to that one
  (`a_sidecar_travels_with_the_track_whose_longer_stem_it_carries`). A cue-cut file is the
  exception proving the rule: rows cut from one file are filed by the folder their layouts agree on
  and keep their name, the sheet naming that file, so the sidecar lands under an unchanged stem and
  still names its audio; rows of one file naming two folders are `Unidentified`, not filed under
  whichever came first. A sheet cutting *one* row from a file is the case that fails: the file
  renders by the layout like any other, so `sheets_follow_their_audio` rewrites the landed sheet
  after the batch's catalog write. `codec::cue::renamed` is the whole of how: reads the sheet's
  encoding, checks exactly one `FILE` line names that file, encodes old and new names in that
  encoding, splices the one byte run following a `FILE` command on its own line
  (`the_run_on_the_file_line`, one encoding unit at a time: a UTF-16 sheet is walked in pairs, a
  `REM` or `TITLE` naming the file is left alone), so BOM, line endings and every other byte
  survive; `staged_over` renames a staged file over the sheet, so a crash cannot truncate it. A
  file two `FILE` lines name leaves the sheet alone. A name the sheet's encoding cannot hold can
  only be a legacy code page's (UTF-8 and UTF-16 hold every name); that sheet is carried into UTF-8
  with a byte-order mark, not left naming a vanished file: `carried_into_unicode` reads the bytes
  around the run in the sheet's code page (line endings kept) and writes the new name between; the
  mark tells a player that reads a markless sheet as the system code page that this one is not
  (`a_sheet_whose_encoding_has_no_letters_for_the_new_name_is_carried_into_unicode`).
- **A sheet cutting a file travels with it whatever it is called; one naming several files takes
  them all as one move.** The stem rule finds only `Meddle.cue` beside `Meddle.wav`; sheet
  `Meddle.cue` with audio `CDImage.wav` was left behind, the next scan read the file whole and
  pruned every row cut from it (plays, listens, favourites, vault links). `Planner::sheets_in` reads
  each source folder's sheets once via the scan's `read_sheet` and `claimed_beside` (a sheet may
  name files in the folder `the_folder_a_cue_names` gives); `sheets_travelling` weighs every sheet
  naming the moving file: one naming only it is a sidecar that must land (renamed stem where it
  shared the audio's, else its own name); a destination it cannot take refuses the whole move as
  `Collided` (a loose sidecar is merely left). A sheet naming several existing files (EAC rip, one
  `FILE` per track) ties them: `Planner::tied_by_sheets` gathers every file such a sheet names plus
  every file any sheet naming one of those names; `file_together` plans them as **one `Move`**
  (first file `from`/`to`, rest `companions`, sheet a sidecar under its own name). One move stays
  together after the plan: a batch never splits it, `renamed_onto` puts every file back where one
  fails, `files_moved` follows each file, `sheets_follow_their_audio` renames every `FILE` line.
  `Move::files` is the one walk all take, `Plan::files_moving` what a preview counts. The layout
  must land every file in one folder and every file must be one this pass files, else each is
  `Refusal::SharesASheet` (the one failing on its own account takes its own refusal); such a sheet
  is likewise kept out of the loose stem pass. **A unit is a link in a chain like any move.** A
  destination held by a file this pass moves on is waited for; a unit may wait on as many as it has
  members: `Planned::waits_for` is every source standing where the move lands, `order_the_chains`
  puts a move behind each move vacating one. `walked` is an iterative (no stack cost) depth-first
  topological order over the waits: a move is ordered once everything it waits on is; meeting one
  still being walked (a cycle) or already doomed dooms every move on the walk, each waiting on it in
  turn. A move left out is `Collided` with the first source it waited on, each unit member alike. A
  unit whose member lands where another member stands waits on itself and is doomed: a rename inside
  one move cannot be ordered. At apply `standing` weighs every file of a move, so a chain the disc
  changed under since the plan is refused, not renamed over
  (`the_files_a_sheet_names_wait_for_a_file_standing_where_one_lands_to_move_on_first`).
## Taking files in

`take_in(TakeInOptions { paths, into })` (`take_in.rs`, dropping songs on the window) starts a
`resonate-take-in` thread behind a `TakeInHandle` (`PassKind::TakeIn`) answering a `TakeInSummary`:
`landed`, `passed` (typed `Passing` each), counts, cancelled. No catalog, so no `Walk` guard; the
window follows with a scan. `Error::DestinationNotADirectory` is the one refusal before the thread
starts.

- **Taken: what the scan reads, plus companions.** File: `names_audio` or `.cue`. Folder: walked
  (`.`-names and links skipped, `DEEPEST_FOLDER` levels) for audio and cue sheets. Companions:
  anywhere in a dropped folder a picture (`resonate_core::names_a_picture`: `cover.jpg`, `Scans/`)
  and lyric sheet (`LYRIC_ENDINGS`: `.lrc`, Lyricsfile); in any folder holding audio, a file named
  after a track (stem, `.`, more: `is_named_after`, organise's sidecar rule): `01.txt`,
  `Album.flac.log` yes, `notes.txt` no. A folder yielding only companions: `NothingInside`; a loose
  file is a companion (`Looks::Companion`) only if the same drop takes audio from its folder. Else
  `NotAudio`. Same file twice (canonical path) is one. `weigh` is the cheap read-only look drawn
  during a drag (`Looks`); `gather` reads the drop through it.
- **Names kept, nothing overwritten.** `<into>/<name>`; folders `<into>/<folder>/<relative path>`,
  merging. File inside `into`: `AlreadyThere`; same bytes at destination: `AlreadyHeld`; neither a
  refusal (`Passing::is_a_refusal`). Different file under the name: `name (2).ext` and on (to 999,
  then `Unwritable`).
- **A copy is whole, read back, then named.** Bytes to hidden `.<name>.<pid>.resonate-part`,
  compared in `COMPARED_AT_ONCE` (256 KiB) pieces, synced, `hard_link`ed to the name (refuses an
  existing name atomically: a file made meanwhile is never overwritten), `rename` where no links;
  staged file always removed. Not reading back: `Unverified`, not kept (`nothing_is_left_staged_…`;
  `a_byte_for_byte_copy_already_there_is_held_and_a_different_one_is_kept_beside_it`).
- **What travels with a track follows its landed name.** Audio lands first; `Renames` notes each
  whose name moved (`name (2).ext`, or held byte for byte under one: `Stood::Held`) by source
  folder. Sheets and companions from that folder follow: one named after the track takes the new
  stem (`following_its_audio`, longest stem wins; a lyric sheet in a `lyrics`/`lyric`/`lrc` folder
  also follows the folder above's renames, where the sidecar reader looks); a sheet whose `FILE`
  line names a renamed track is rewritten by `resonate_codec::renamed_cue` (organise's rule;
  encoding and bytes kept; over `LARGEST_SHEET_REWRITTEN`, 1 MiB, copied as is), landed as
  `Content::Rewritten`, read back and weighed against a standing file likewise. Source never touched
  (`what_travels_with_audio_that_landed_under_a_new_name_follows_that_name`,
  `a_sheet_naming_audio_already_held_under_a_new_name_is_written_naming_that_name`).
- **The window files what landed once scanned.** With `file-dropped` on (`binary.md`),
  `RootView::took_in` hands landed audio and the `organise-as` layout to
  `LibraryModel::file_once_scanned`; `take_up_what_waited` (as every pass ends) starts an applied
  `organise` with `OrganiseOptions::only` naming them once no root waits to be scanned. Ordinary
  run: `Walk` guard, sidecars and sheets carried, emptied folders pruned, toast, and the run *Put
  the last run back* walks back. A file the layout cannot name (no title, or nothing between it and
  the root) stays where it landed.

## The search grammar

- **Words and terms; a term's meaning is typed.** `Search::read` is the whole grammar. A token is
  a word unless it names a field: `title:`, `artist:`, `album:`, `genre:`, `lyrics:` (or `lyric:`,
  `LYRICS_ALIAS`) scope words; `year:`, `added:`, `plays:`, `played:`, `length:`, `rate:`, `depth:`,
  `codec:`, `is:` are `Term`s, with `KEYS` aliases `heard:`, `duration:`, `samplerate:`, `bits:`,
  `format:`. Unknown field or unreadable value: the words as written (`lrc.rs`'s rule for a bracket
  neither moment nor id tag): never a parse failure, box stays live, a mistyped term goes quietly.
  Numeric readers are overflow-safe: `length:400000000000000000:30` is three words. `Display for
  Term` is canonical and reads back as the same term (the *Reads* row, `resonate playlist --query`,
  tests share it). `is:hires` = lossless above CD: `CD_SAMPLE_RATE` (44 100), `CD_SAMPLE_DEPTH`
  (16) in `db.rs`; `depth:` reads the stored `SampleFormat` (24 valid bits for float).
- **Terms narrow on what the catalog stored; two are ages.** `added:`, `played:` measure from query
  time (month 30 days, year 365): "added this year" is `added:<1y`; a saved query answers
  differently tomorrow. `plays:` is a count (`plays:0` never heard); `heard:` = `played:`. `@`
  windows the count over `listens`, not the lifetime column: `plays:>20@30d`, age read as `added:`
  reads it; ranges carry the window on both bounds (`plays:5-10@30d`); two `Plays` terms fold into
  a range only with agreeing windows; unreadable window: plain word. Costs the one term no index
  serves (correlated `count(*)` over `listens` per row), so it narrows, never orders. `year:` is the
  *album's* (no album year, or ungrouped single: no answer). `rate:`, `depth:` are what the file is,
  not the sink request (`depth:32` = 32-bit integer files alone). `is:lossy` = codecs known lossy,
  so an unnamed codec is in neither it nor `is:lossless`. No rating term (none stored); nothing
  sorts on a term (`SortOrder` orders). Parsed every keystroke and saved-query read, uncached.
- **Everything holds, unless `-` denies or `or` alternates.** `Search` = `Clause`s all holding;
  `Clause` = `Asked` alternatives, one must; `Asked` = one token's `Condition`s plus denied. A range
  is a shape: `year:1970-1979` two bounds both holding; `-year:1970-1979` each bound denied in one
  clause (De Morgan at the parse; nothing downstream brackets). `Display for Asked` folds an
  undenied bound pair back to its range (`year:1970-1979 or is:hires` keeps the alternation against
  the whole range); a denied range (two denials joined by `or`) reads back as itself. `-` denies
  only at an unquoted token's start (`well-known`, `1970-1979` untouched); `or` joins only between
  two unquoted undenied tokens, else the word written (quote to search either literally). No
  bracketing: a denial reaches one token, an alternation one flat run; `-a or b` = "not a, or b",
  `-(a b)` cannot be written. `and` is no word (all is already joined).
- **A search asking nothing holds everything.** A word is a condition only where
  `search::pieces_of` finds a letter or digit (all that reaches the index): `!!!`, `+/-`, `-!!!`,
  `artist:?` drop at the parse, an `or` closing the gap (`moon !!! or sun` reads `moon or sun`).
  `db::matching` skips a clause-less `Search`: punctuation, blank saved query, no text = the catalog
  (each once became an FTS `MATCH` of nothing, no rows). `cuts_matching` answers `Narrowed`:
  `Unasked` (narrows nothing), `To`, `Nothing` (no row can match). A list narrowed by blank text
  holds every row, as a saved query does; by text a search reads nothing in (`!!!`, `???`):
  `Nothing`, no row (`asks_for_nothing_it_can_read`; else it played and copied the whole list). The
  search box still reads punctuation as no condition (mid-word caret does not empty the pane). A
  drop takes only `To`: nothing empties a list in one gesture
  (`a_search_made_only_of_punctuation_asks_nothing_and_so_holds_everything`,
  `a_saved_query_with_no_search_fills_itself_with_the_whole_catalog`,
  `a_list_narrowed_by_a_text_a_search_reads_nothing_in_holds_no_row_and_drops_none`, in
  `tests/search.rs`).
- **A number is read as meant, to the precision typed.** `Term::Length` carries a `Grain` (finest
  `ClockUnit` named, decimals written) and weighs the length cut to it: `length:3:30`,
  `length:3m30s` hold 210 <= s < 211; `length:3m` any 3-minute-and-some track; `length:3.5m` the six
  seconds from 3:30; `length:<=3:30` below 3:31; `length:>3:30` from 3:31 (as a pane draws a
  length). Was a float equality (only exactly 210.000 s) where `added:`/`played:` already read an
  exact value as a span. `Display for Term` writes a length to its grain, zero components and
  decimals kept (`length:180` reads back `length:3m0s`, `length:0.50s` itself). A range typed high
  end first is the range named (`bounded` orders): `year:2000-1990` = `year:1990-2000`. A bare rate
  below `A_BARE_RATE_IS_IN_KILOHERTZ_BELOW` (1 000) is kilohertz (no file samples that slowly):
  `rate:44.1` 44 100 Hz, `rate:96` 96 000, `rate:500hz` 500, written back with its unit. *Long
  players* is `length:>=10m`.
- **`=` asks a whole name; a word or phrase a run inside one.** A phrase matches tokens anywhere in
  a column (`artist:"Air"` holds *Air Supply*); `artist:=Air`, `artist:="Air Supply"` are
  `Reach::Whole` (beside `Begins`, `Phrase`; `Display for Word` writes them quoted); `=` inside
  quotes is phrase text; `lyrics:` takes no whole name. `db::named` weighs it through the
  `words_of` SQL function (`store::words_of`; both sides folded and split alike): artist by id
  (`tracks.artist_id` or a `track_credits` member of that name, the artist pane's reading); album by
  billed or scanned title; title, and genre (track's or its artist's), by the column behind the
  phrase's `INDEX_LOOKUP`, which narrows the rows the function sees. Never ranked: reaches the
  `WHERE` as a denial does. `suggest.rs` artist mixes are whole names, so a mix for *Air* is *Air*'s
  (`an_artist_mix_holds_the_artist_it_names_and_not_one_whose_name_begins_with_it`).
- **`tracks_fts` holds the fold of a name; the query is folded with it.** Indexed: `folded_letters`
  of title, billed artist, album, genres (lyrics apart). The artist column is the billed artist with
  the album's owner beside it, read by `store::index_row` itself (`album_owner_of`), so scan,
  enrichment and every reindex write the album artist alike; the scan adds a compilation's tagged
  album artist too
  (`an_album_artist_still_finds_its_tracks_once_the_enrichment_indexes_them_again`); `db::indexed` folds every piece of a typed
  word likewise, so the sides differ only by being changed apart. Why: `unicode61
  remove_diacritics 2` folds `Ç` to `c`, `İ` to `i`, not dotless `ı`, so *Kıskanç* matched only
  typed with `ı`, *KISKANÇ* only without; the fold also lets *Przybylowicz* find *Przybyłowicz*.
  `remove_diacritics` stays (free; the index does not rest on a complete fold). `prefix = '1 2'`
  answers a one- or two-letter word ahead of its rest from a prefix index, not by enumerating terms
  (`a_catalog_carried_forward_keeps_its_index_and_gains_the_short_prefixes`: step copies rows out,
  redeclares, puts back). No `tracks_fts` column is read back (only `MATCH`, `rank`): no spelling
  kept beside the fold. Schema break without a migration: an index from before the fold holds
  spellings no folded query matches and an incremental rescan never rewrites unchanged rows, so
  such a catalog is deleted and rescanned.
- **Lighting reads the name, not the index.** `Search::lit` answers the byte runs of a display
  string a search matched, for one `Column`, drawn in the accent. FTS5 `highlight`/`snippet` answer
  the stored (folded) column (row reads `bjork`, pane draws *Björk*), so: split the name on
  `char::is_alphanumeric`, fold each token whole keeping its original byte range, weigh each against
  the `search::pieces_of` that `db::indexed` builds the `MATCH` from. Lit = matched: prefix for a
  bare word, consecutive tokens for a phrase, nothing for a denied word or a term, a column-scoped
  word nothing in another column. A whole token lights (half-lit reads as a typo); meeting runs
  merge. **Words are taken before the name**: `lit` runs per drawn cell per frame; tokenising first
  (a `Vec` plus a lowercasing and an NFD `String` per token) ran even with an empty box: eighteen
  rows of two lit cells was some five hundred `String`s and normalising passes a frame for nothing.
- **What a track sings is indexed; only a word asking reaches it.** `tracks_fts`' fifth column
  `lyrics` is filled by `store::index_row`: `sung_by` reads `tracks.lyrics` or, where the file had
  none, the fetched `lyrics_kept`; `sung_words` drops `[…]` and `<…>` runs before folding (LRC
  timestamps, karaoke word marks). `Library::keep_lyrics` rewrites the column for rows at that path
  and span whose file had none, in the keeping transaction: a lyric fetched during playback is
  searchable at once, a rescan indexes the same words. A bare word is
  `{title artist album genre} : "…"*` (`Column::NAMES`), so *love* does not return every song
  singing it; `lyrics:` says the words are meant. `Search::as_sung` turns a search of plain words
  alone into one phrase `lyrics:"…"`, counted by `Library::sung` so the window can offer it.
  `spelling.rs` has no vocabulary for the column, corrects no lyric word. Also a schema break
  without a migration (`a_track_is_found_by_the_words_it_sings_and_only_when_they_are_asked_for`,
  `a_search_of_plain_words_is_offered_as_the_words_a_track_sings`).
- **A search reaches what the catalog lacks.** `release_tracks.folded` = title, artist (track credit
  or release's), release title, through `folded_letters` by `land_release`.
  `Library::unheld_matching` asks it one `LIKE` per piece of every word asked by name: lone words,
  bare or scoped to title, artist, album (`elsewhere::words_asked`). A term, denial, genre or lyric
  has nothing to answer with on an unheld row, so a search of those alone answers nothing. Wanted
  rows first; it and the Missing pane share `SHORT_OF_WHAT_IS_HELD_OR_WANTED`: a release row is
  missing where its album holds a track or the row is wanted, so a release landed for one song does
  not list the eleven nobody asked for
  (`a_search_reaches_the_rows_the_catalog_knows_it_is_short_of`).
- **A song is placed on the album it was meant for, not whatever came out first** (a single predates
  its album, a compilation often both). `elsewhere::meant_release` weighs `Issued`: `Standing`
  (official, then unstated status, then bootleg/promotion/withdrawn), then `Meant` (album,
  soundtrack counting, then EP, single, unstated kind, then compilation/live/any other secondary
  type), only then date, undated last. It is what `best_release` falls back to and what a found
  song is wanted from
  (`a_song_is_placed_on_its_album_before_a_single_or_a_compilation_that_came_out_first`).
- **Found elsewhere: `Library::found_elsewhere`.** Sends the words asked by name to
  `Reference::find_songs`, answers `Found`s: recording, title, credit, length, the release meant
  (`meant_release`), `performer` (first credited artist MusicBrainz names by id, as
  `Performer::Elsewhere`, no request of its own). Leaves out recordings in `tracks.mbid` or
  `release_tracks.recording_mbid`; drops a second recording of the same folded title and credit; to
  `FOUND_ELSEWHERE_AT_MOST` (12). Halves are public apart (the window keeps the reference's answer
  and reweighs it): `songs_asked` = words as sent (title, artist, album words only, lower-cased,
  single-spaced: *Pink  Floyd* and *pink floyd year:1971* are one ask), `None` under three letters
  (not sent; `asks_elsewhere` is its `is_some`); `Library::unheld_among` asks the catalog about
  those recordings alone (`WHERE mbid IN (…)` under `tracks_by_recording`,
  `release_tracks_by_recording`: a `MIGRATIONS` step) instead of reading every recording id.
  `still_answering` narrows songs found for one search to those every folded word of another begins
  a word of (title, credit, a release's title): what the window shows while asking
  (`songs_found_for_fewer_words_are_narrowed_to_those_still_answering_more`).
- **Songs of unheld releases are learnt in the lookup pass.** `Pass::learn_the_songs` runs after
  the album, track and artist lookups, before `cover_the_unheld`, **artist by artist**:
  `artists_whose_songs_are_due` (every artist with a group due, most played first); due groups
  re-read on reaching the artist (`songs_due_for`) and each group again before asking
  (`songs_still_due`), so a group an artist's page landed meanwhile is not asked twice
  (`a_group_an_artists_page_read_while_the_lookup_ran_is_not_asked_for_again`); `at_most` caps
  groups asked. Due: unheld by `unheld_by_any_album!` and never read, refused longer ago than
  `REFUSED_AGAIN_AFTER`, or read longer ago than `REFRESH_AFTER` less `spread_of` its artist (SQL
  function from `schema::configure`: Fibonacci hash of the artist id into `[0, REFRESH_SPREAD)`,
  ten days), so a first pass reading every group in an hour does not fall due all at once a month
  later, while one artist's groups still fall due together and can share a browse.
- **One browse of the artist's pressings where that costs fewer requests.**
  `learning::learn_the_songs_of` is the walk the pass and an artist's page share (`Learner`: may it
  take another turn, how a refusal is weighed, what a landing does). With an artist mbid and
  `BROWSED_FROM_GROUPS_DUE` (3) or more groups due, `Reference::releases_of_artist` is read page by
  page keeping only due groups' pressings, while pages left < due groups not yet seen. Every group
  gathered lands through `pressing_of` as if read alone; each the browse never reached (other
  credited spelling, no official pressing, past the cost rule) is asked alone after
  `songs_still_due`. A refused or unreadable browse lands nothing, falls back to the groups. Worst
  case one request more per artist; usual one page for twenty groups
  (`an_artists_songs_are_read_off_one_browse_of_their_releases_rather_than_a_request_a_group`,
  `a_group_the_browse_never_reached_is_read_on_its_own`,
  `a_browse_longer_than_the_groups_it_would_save_is_left_for_the_groups_themselves`,
  `a_browse_refused_falls_back_to_reading_each_group`). A group read alone is asked
  `Reference::releases_of_group`; `songs::pressing_of` takes the pressing whose track count most
  pressings share (fewer tracks on a tie: original over deluxe), earliest of those, full date
  before bare year.
- **Landing songs.** `songs::land` replaces the group's `discography_songs` rows (group,
  recording, pressing id, title, date, kind, disc, position, title, credit, length, `words_of`
  title, a `folded` haystack of every word of title, credit, release title) and stamps
  `discography_songs_read` (both a `MIGRATIONS` step). No pressing: stamped read with nothing;
  refusal counts in `refusals`; unreachable reference ends the pass as in every lookup.
  `EnrichStats::songs` counts rows
  (`the_songs_of_releases_not_held_are_learnt_in_the_lookup_and_found_without_asking`,
  `a_release_group_refused_waits_before_its_songs_are_asked_for_again`). **An artist's page does not
  wait for the pass**: `Library::learn_the_songs_of_artist` walks the same due rule over that
  artist's groups (`songs_due_for`, earliest first, `songs_still_due` each, same `learning` walk),
  telling the `Learning` it is handed of each group as it lands, asking nothing more once it says
  `abandoned`; landing and stamping as the pass (refusal or unreadable answer stamped, unreachable
  reference answered as the error) (`an_artists_page_is_told_of_each_batch_of_songs_as_it_lands`,
  `an_artists_page_left_asks_nothing_more`,
  `an_artists_page_reads_the_songs_of_its_releases_not_held_before_the_lookup_reaches_them`).
- **Sleeves of those releases are kept in the same pass, each as its songs land.**
  `Pass::cover_as_landed` hands a group whose songs just landed to the picture readers where
  `unheld_cover_is_due` (one-row `UNHELD_COVERS_DUE`) says so (Cover Art Archive: another host, own
  pace and threads), so sleeves arrive beside songs, not an hour later; `unheld_covered` remembers
  what was handed over (`the_sleeve_of_a_release_not_held_is_asked_for_as_soon_as_its_songs_land`).
  `Pass::cover_the_unheld` runs after `learn_the_songs` for the rest, skipping `unheld_covered`:
  every group `unheld_covers_due` answers (unheld by the same predicate, no cover held, none asked
  within `COVERS_ASKED_AGAIN_AFTER`, most played artist first, `at_most` caps) goes to the picture
  readers as `Picture::OfAnUnheldRelease`: `Reference::cover` for the pressing its songs were read
  off (group as fallback), `group_cover` where none was read. `land_unheld_cover` writes
  `unheld_covers` (group, picture or none, when asked; a `MIGRATIONS` step) for either answer, never
  for a failed or refused fetch; a held picture is not asked again and an empty answer never blanks
  it (`coalesce`). `EnrichStats::covers` counts pictures. `Library::unheld_cover` reads one back,
  format sniffed: what the artist's page draws before asking the archive itself
  (`the_covers_of_releases_not_held_are_kept_in_the_lookup_and_not_asked_for_again`).
- **Three readers.** `songs_kept_for` (a search's): every folded word a word start in `folded`,
  held nowhere by recording, group still unheld, no track of its artist the same `words_of`; albums
  first, one per folded title and credit, to `FOUND_ELSEWHERE_AT_MOST`, each a `Performer::Held` by
  the lowest `artist_releases.artist_id` holding its group (under `artist_releases_by_group`).
  `songs_not_held_by` (an artist page's): rows the artist's own albums lack (not dismissed, no held
  track of that title), then songs of its unheld groups, one per title and credit.
  `albums_not_held_by`: an `AlbumNotHeld` per release group of the artist with no album holding a
  track of it (one landed and still downloading stays), carrying the pressing its songs were read
  off.
- **Wanting.** `Library::want_album` wants every song of a group not held by recording or title:
  lands the optional named `pressing` first, reads the group's pressings where the pass never
  reached it, then `want_from_landed` (as `want_found` via `want_from_release`): release read and
  landed once, each recording's row wanted, cover asked once
  (`an_album_not_held_is_wanted_whole_from_the_pressing_its_songs_were_read_off`,
  `an_album_whose_songs_were_never_read_has_them_asked_for_when_it_is_wanted`).
  `Library::want_missing_tracks` (album already in the catalog) selects release rows without a held
  track and `want_in`s them in disc and position order under one write transaction and timestamp;
  an existing want is due again as when asked singly. The UI starts one provider poll after the
  batch lands.
- **`Library::want_found` is the want.** The release the `Found` names, or where the search gave
  none the one `meant_release` picks from `Reference::recording`'s releases, is read whole through
  `Reference::release`. Album already carrying that mbid: taken as is; else a new one (billed to
  `store::artist_named`, stamped `albums.found_elsewhere`), `land_release` writing rows as
  enrichment does: an ordinary `wants` row a provider is asked for and a delivery lands on. Wanted
  artist: track credit where present, else landed release artist (providers and player keep it with
  no track-level MusicBrainz credit). If the landed release advertises front art or has a release
  group and the album has no picture, `want_found_uncovered` answers a `Covering` and the caller
  asks `cover_what_was_wanted` after the want is out (`want_found` does both), so a poll starts
  before the sleeve arrives; answer saved on the album; a failed fetch or store does not cancel the
  want. Errors: `Error::UnknownRelease` (reference lacks the release), `Error::Unreleased`
  (recording has none), `Error::NotOnTheRelease`. `store::ORPHANS` spares a trackless album only
  where found elsewhere and still wanted (a scan keeps it while the want stands, removes it once
  gone; an album scanned from a root still leaves with the root); the albums pane never shows one
  (`HOLDS_A_BEST_COPY` asks for a track)
  (`a_song_found_elsewhere_is_wanted_by_landing_the_release_it_first_came_out_on`,
  `a_song_with_no_release_named_is_wanted_from_the_one_its_recording_first_came_out_on`).
- **Which release is the listener's to say.** A `Found` carries every release of its recording;
  `Found::in_the_order_worth_offering` orders them as `meant_release` weighs; the found row's *Want*
  mark opens them as a right-button menu (title, year, kind); a press is `want_found` with
  `Found::from` that release, so a song wanted for its single or compilation lands there
  (`a_found_song_offers_every_release_it_is_on_the_one_it_would_be_placed_on_first`).
- **An album the words name is found beside the songs.** `Reference::find_albums` is a
  release-group search (`AlbumMatch`: group, title, credit, kind, secondary types, first release
  date), asked after `find_songs`, failing without failing it. `Library::unheld_albums_among` keeps
  an `AlbumFound` per album or EP (soundtrack counts, no other secondary type) every folded word of
  which begins a word of title or credit, once per folded title and credit, to
  `ALBUMS_FOUND_ELSEWHERE_AT_MOST` (12), dropping one the catalog holds by `albums.release_group` or
  folded title under an artist of the same key (a found-elsewhere album landed is not held)
  (`an_album_the_words_name_is_offered_unless_the_catalog_holds_it`). Opening or wanting one is
  `open_album_uncovered` and `want_album`, which read the group's songs themselves. **What the
  lookup kept answers first, offline**: `Library::albums_kept_for` reads albums and EPs of library
  artists' discographies held by no album (`artist_releases` under `unheld_by_any_album!`, an
  `instr` per folded word over the release's `folded` and the artist's key, in SQL);
  `elsewhere::albums_kept_named_by` keeps those every folded word of which begins a word of title
  or artist name, once per folded title and name, earliest first, to
  `ALBUMS_FOUND_ELSEWHERE_AT_MOST`; the window lists them ahead of MusicBrainz's answer, dropping an
  answer of the same group or folded title and name
  (`the_albums_of_a_library_artist_not_held_answer_a_search_without_asking`).
- **An artist the words name is found off the answer in hand.** `elsewhere::artists_named_by` reads
  credits of the `find_songs` matches: a credit with a MusicBrainz id whose folded name has a word
  begun by every word typed (`words_asked`), once per id (*twenty one pilots* names the band,
  *twenty one pilots stressed out* nobody). `Library::unheld_artists_among` drops those held
  (`artists.mbid` or `artists.key`, `found_elsewhere` null), keeps up to
  `ARTISTS_FOUND_ELSEWHERE_AT_MOST` (4). `Library::open_artist_found` is the press: billed through
  `store::artist_named` under the id, stamped `artists.found_elsewhere` (a `MIGRATIONS` step) where
  it holds no track, album or credit, profile landed by `land_artist`, release groups by
  `land_artist_releases` as the lookup pass does, so its page lists releases at once and a press on
  one is `open_album_uncovered`. **`ORPHANS` spares an artist stamped `found_elsewhere`**: a scan
  keeps the row; it stays offered, not held, until something of its own is scanned
  (`an_artist_named_by_what_was_typed_is_offered_when_unheld_and_landed_with_its_releases`).
- **Song link: followed to its recording by ISRC; by title and artist only under the strict
  rule.** `linked.rs`. `SongLink::read` takes one whitespace-free token: `MusicBrainz`
  (`musicbrainz.org/recording/<mbid>`), `Deezer` (`deezer.com/[lang/]track/<n>`), `Elsewhere` (song
  page on Spotify or `spotify:track:`, TIDAL, Apple Music (`i=` of an album URL, or a `song` path),
  YouTube, YouTube Music, SoundCloud (account + track, not a set), Amazon Music, Anghami, Boomplay,
  Audiomack, Yandex Music, song.link); nothing for an album, artist, playlist, other host or text
  with a space (typed words are never a link). `Library::follow_link` answers a `Linked`: `Held`
  (title, artist of a track a file already is: recording link checked against `tracks.mbid` before
  anything is asked; link elsewhere against `tracks.isrc` for every code `Reference::song_linked`
  names, and `tracks.mbid` again once a recording is settled), `Found` (the `Found` to want) or
  `Unnamed`. Each code goes to `recordings_of_isrc` in turn until one names a take;
  `the_take_linked` keeps takes within `LENGTHS_AGREE_WITHIN` (5 s) of the link's length, the
  closest, and the first where the link gave none (else a video edit under the same code passes
  for the song).
  **No code names a take** (YouTube upload: no ISRC, often no Deezer twin): `LinkNames` carries the
  billed title and artist; `the_song_searched_for` asks `find_recording` with them and the link's
  length, as phrase then words, accepting only under `enrich::the_recording_named` = the track
  identification's `matches_a_recording` weighing (`STRICT_SCORE`, title and credit agreeing by some
  `Spelling`, lengths within `RECORDING_MAY_DIFFER_BY`) over a `NamedAs` not a catalog row; no rule
  of its own. A title no artist can be read for is never searched. **Upload titles are read as
  billed**: `readings_of` reads *Artist - Song* as that song by that artist where the part before
  the dash agrees with the channel (`- Topic` stripped); else (a `…VEVO` channel) the upload's own
  billing is asked first, the whole title under the channel second, each its own search.
  `VIDEO_QUALIFIERS` is a closed list like `VERSION_QUALIFIERS` (*Official Video*, *Official Music
  Video*, *Official Audio*, *Lyric Video*, *Visualizer*, *HD*, *4K*, …) stripped from a title's
  bracketed end by `enrich::without_brackets_that`, the stripper `dequalified` runs; *(Live)*,
  *(Remix)*, *(4K Remaster)* stay and stop the stripping (another recording, or nothing on the
  list). A strict answer is fetched whole through `Reference::recording` as an ISRC's take is;
  nothing strict names nothing. **A video may run past its song**: for a YouTube page
  (`linked::Footage::of`, `Service::Youtube`; YouTube Music is the song's own) where the strict
  search named nothing, the readings are asked again with no length and
  `enrich::the_recording_a_video_names` takes a take agreeing on title and credit that the video
  could have played whole (take no longer than the video by more than `RECORDING_MAY_DIFFER_BY`,
  no shorter by more than `A_VIDEO_MAY_RUN_LONGER_THAN_ITS_SONG_BY`, 4 min): better spelling, then
  length nearest the video's, then score. The take is placed by `elsewhere::found_among`,
  `meant_release` choosing the album as for a search. A recording only a release row names (wanted,
  or missing from a held album) is `Found`, `want_found` landing it as anything else.
  `RecordingMatch: From<Recording>` scores it whole (shared with `resonate-online`'s `by_ear.rs`).
  (`a_link_to_a_song_nothing_holds_is_followed_by_its_isrc_to_the_recording_to_want`,
  `a_link_to_a_song_the_library_holds_answers_the_track_and_asks_musicbrainz_nothing`,
  `a_link_no_service_can_name_names_nothing`,
  `a_song_link_naming_no_isrc_is_followed_by_its_title_and_artist_under_the_strict_rule`,
  `a_song_link_whose_title_finds_only_a_near_miss_still_names_nothing`,
  `a_music_video_running_past_its_song_is_followed_to_the_take_it_plays`)
- **Album link: followed to its release group by identifier, never title.** `AlbumLink::read`:
  `Release`/`Group` (`musicbrainz.org/release/<mbid>`, `/release-group/<mbid>`), `Deezer`
  (`deezer.com/[lang/]album/<n>`), `Elsewhere` (album page on Spotify or `spotify:album:`, TIDAL,
  Apple Music (`album` path, no `i=`), YouTube Music (`playlist?list=OLAK5uy_…`), Amazon Music
  (`albums/<asin>`, no `trackAsin`), album.link). `FollowedLink::read` tries song, album, then
  artist; `is_a_followed_link` is the window's test. `Library::follow_album_link` settles a
  `NamedAlbum` (group, release if named, title, credit) via `linked::album_named`: group link asks
  `Reference::release_group`, release link `Reference::release` and takes its group; else
  `Reference::album_linked` gives the `Barcode`s the album is sold under (service's UPC, then its
  Deezer twin's), each in turn to `Reference::releases_by_barcode`; `the_release_barcoded` takes a
  release only where its barcode is that code (`Barcode::names`: digits alike once leading zeros
  are off, UPC-A and EAN-13 being one GTIN) and every release so barcoded is in one group (a code
  two groups share names neither). A `Barcode` is 8 to 14 digits, nothing else. A held album
  carrying the group
  or release (`album_held`) gives `Linked::HeldAlbum` (album, billed title and owner, `missing` =
  release rows without a file); else `Linked::Album` (group, named release), both handed to
  `want_album`: a named pressing is read whole and its songs landed as the group's in place of the
  usual pressing's (linked deluxe edition = wanted as the deluxe track list); a group-only link is
  wanted from the pressing most of its pressings share
  (`an_album_linked_by_its_reissue_is_wanted_from_the_reissue_and_not_the_usual_pressing`).
  **Artist link: followed to the artist it names.** `ArtistLink::read` takes
  `musicbrainz.org/artist/<mbid>` and `deezer.com/[lang/]artist/<n>`; `Library::follow_artist_link`
  answers `Linked::HeldArtist` for a held artist (`artists.mbid` for the first; for the second the
  fold of the name `Reference::artist_linked` reads off Deezer), else `Linked::Artist` with the
  `ArtistFound` MusicBrainz names (profile for an id; `enrich::top_artist`'s exact rule over
  `find_artist` for a name). **An artist link elsewhere is followed by its page, never the name it
  shows.** `ArtistLink::Elsewhere` holds the page as MusicBrainz stores it (`artist_page`: Spotify
  `open.spotify.com/artist/<id>` from any locale or `spotify:artist:`; Apple Music
  `music.apple.com/<storefront>/artist/<id>`, slug dropped; TIDAL `tidal.com/artist/<id>` from
  `listen.` or `browse/`; YouTube or YouTube Music channel; SoundCloud account with nothing under
  it and none of `SOUNDCLOUD_PAGES_NAMING_NO_ARTIST`; Bandcamp site; Amazon Music artist);
  `ArtistLink::pages` lists the pages to ask (Apple Music in another storefront also as `us`,
  second: MusicBrainz files most under it). `Reference::artist_at` asks which artist MusicBrainz
  files a page under, taken only where exactly one, then followed as a MusicBrainz link. A Deezer
  artist is asked that way first, by name only where MusicBrainz files its page under nobody.
  Nothing filed names nothing. A playlist link is still words to search.
  (`a_link_to_an_album_is_followed_by_its_barcode_to_the_release_group_to_want`,
  `an_album_link_whose_codes_name_no_release_or_several_groups_names_nothing`,
  `a_link_to_an_album_the_library_holds_answers_the_album_and_how_many_songs_it_lacks`,
  `a_link_to_an_artist_opens_the_artist_held_or_the_one_musicbrainz_names_exactly`,
  `an_artist_link_on_spotify_is_followed_through_musicbrainz_to_the_artist_it_names`,
  `an_artist_page_is_written_as_musicbrainz_stores_it`)
- **Lone word ranked; denied or alternated word looked up.** Unnegated, unalternated, non-`=`
  words are what `indexed` folds into the single FTS5 `MATCH` the index is joined for, so `rank`
  and `SortOrder::Relevance` mean what they did. Any other word is `INDEX_LOOKUP` in the `WHERE`
  (a subquery on the same index, scoring nothing); a search with every word denied or alternated
  has no join and relevance falls back to album order. Denial is `NOT coalesce(…, 0)`: a row unable
  to answer (no album for `year:`, no duration for `length:`) satisfies it, not drops out.
  `Matching::grouped` carries the lot into album and artist listings, so one text narrows every
  browse pane.
- **Words naming a title by an artist are read so, against the artists held.** `meant.rs`.
  `ByArtist::read` takes words with no grammar (no field, quote, denial), split at the last
  standalone *by* (*You F O by stela cole*; last, so *Stand by Me by Ben E. King* keeps its title)
  or a spaced dash (hyphen, en, em), artist first (*Stela Cole - You F O*), each side holding a
  word. `Library::meant` weighs the artist half through `Spellings::artist_named` (the name the
  artists vocabulary holds whole, or nearest within `furthest_from`'s budget: *stella cole* is
  *Stela Cole*) and answers a `Meant` (the search `ByArtist::searched_as` writes: title words
  scoped `title:`, artist `artist:=` whole) only where that search holds a track; else words are
  searched as typed. The window reads every page through it (`Asked::meant` beside `Asked::text`,
  the text still keying what keys on what was typed), lights and reads what it meant, offers the
  words as typed (`ui.md`). `words_asked` drops the *by* and the dash from the songs asked
  elsewhere (title and artist only), so `songs_kept_for`, `still_answering`, `unheld_matching`
  never look for a song called *by*; `songs_asked` answers a `SongsAsked` carrying the reading
  beside the words (`ui.md`). **MusicBrainz's answer is weighed here**: `weighed_for` keeps matches
  whose credit is nearest the typed artist (same letters, then one name holding the other; folded,
  all but letters and digits dropped), of those the titles matching the typed title likewise, same
  letters first; no title matching = every song of the artist kept (title perhaps misremembered).
  The window weighs an answer before keeping it, `Library::found_elsewhere` before weighing it
  against the catalog.
  (`songs_asked_for_by_an_artist_keep_the_nearest_artist_and_the_titles_that_match`,
  `a_title_by_an_artist_is_read_as_that_title_by_the_artist_the_catalog_holds`)
- **A search matching nothing is answered in the catalog's own spelling; only then is the catalog
  read.** `spelling.rs`. `Spellings` = four `Vocabulary`s (titles, artists, albums, genres: the
  searchable `tracks_fts` columns bar lyrics), each mapping a folded word to its drawn spelling and
  row count, filled by `store::spellings` from `tracks.title`, `tracks.artist`, `artists.name`,
  `albums.title`, `tracks.genre`, `artist_genres.name` through the `search::runs_in` splitting that
  lights a matched run (so a vocabulary word is a word a search could match).
  `Spellings::did_you_mean` corrects words of the parsed `Search` in place; terms, denials,
  phrases, scopes survive: `year:1973` passes, `-floid` is left (a denial is not a mistyping),
  `title:` weighs its word against titles alone. Result = whole query rewritten via
  `Display for Search` (what the *Reads* chips draw).
- **Correct only where the catalog cannot already match, and never further than affordable.**
  `Vocabulary::holds` passes over a run the index would find (held outright, or beginning a held
  word: a bare word is a prefix match): *floy* is not corrected to *Floyd*, *floid* is.
  `furthest_from` is the budget: under four letters never, up to seven (`ONE_LETTER_WRONG_UNTIL`)
  one letter wrong, longer two (`FURTHEST`); `apart_by` is a bounded optimal-string-alignment
  distance (swapped letters cost one: most mistypings). Nearest wins, then the word most rows hold,
  then the spelling itself (same answer twice running); among spellings of one word the marked one
  is drawn, as `folded_letters` does for an artist. A suggestion is only ever a held word (never a
  search matching nothing).
- **Run-together and split are corrections too; neither invents a word.** `Spellings::whole` is
  the *exact* reading (entry a folded run names, not nearest); both rest on it: `run_together`
  where two runs joined name one held word and the two apart do not both, `split_apart` where one
  run cuts into held words. *pinkfloyd* is **Pink Floyd**, *pink floy d* is **pink Floyd** (a word
  at a time reads neither: *floy* is a prefix `holds` passes, *pinkfloyd* is four letters from
  anything). Order: join (two runs explained where others explain one), nearest word (a mistyped
  letter is commoner), split. A split cuts into up to `MOST_PIECES` (4) as a walk, not a scan:
  `reached[to][pieces]` carries the most rows a segmentation of the first `to` letters into that
  many held words can hold, so fewest pieces win, then most rows (*thegreatgig* is **the Great
  Gig**; neither *thegreat* nor *greatgig* held). Refused below the length `furthest_from` refuses
  (one rule bounds both). `run_tokens_together` joins across two *tokens* (*pink floy d* = three
  clauses): only a clause that is one plain undenied word of one run scoped as its neighbour;
  phrases, denials, alternations left. A split writes two words reading back as two; a scoped word
  is written as a phrase so `artist:` reaches both halves. `worth_asking` weighs an adjacent pair
  joined as well as each run alone (*flo yd* is worth the read *flo* and *yd* are not).
- **Vocabulary is indexed for catalog scale; the cost is measured.** A `Vocabulary` holds its
  words and names each as a `Held`: entries keyed by `Arc<str>`, the keys bucketed by letter count,
  and (built when a prefix is first asked, dropped when `take` adds a key) the keys in order.
  `nearest_in` weighs only buckets within `furthest` letters of the run, and within them only keys
  whose `Signature` (letter set folded into 32 bits) differs from the run's by at most two bits an
  edit (no edit exceeds that: a substitution takes one letter out, puts one in), so the bounded
  distance runs on few keys; `edits_between` works on bytes where both are ASCII and on the stack
  below 64 letters, allocating nothing. `holds`, `names` binary-search the ordered keys.
  `cargo bench -p resonate-library --bench spelling` builds the vocabulary of 500 000 tracks,
  50 000 artists, 60 000 albums from synthetic words and times listener asks: build ~700 ms; held
  word or name microseconds; word a letter pair away 4 ms; query like nothing held under 1 ms (the
  walk over every entry with an allocating distance took 20 and 127 ms); completion ~1 ms.
- **A phrase is weighed against a whole name before a word at a time.** `Vocabulary` holds names
  beside words (every multi-run name, keyed by its runs folded and space-joined, spelt as
  `better_spelt` picks); `instead_of_the_whole` weighs a quoted token against them first, under the
  budget its whole length earns from `furthest_from`; only where nothing is near does the
  run-by-run walk take over. Reaches what a word at a time cannot: *"the great gig in teh sky"* is
  **The Great Gig in the Sky** though `teh` is three letters (nothing that short is corrected
  alone). `names` is `holds`'s counterpart, guarding likewise: a phrase *beginning* a held name is
  left alone.
- **A run of tokens is weighed as a name too, only where a word in it is beyond correcting.**
  Unquoted, a title is several clauses: `name_the_tokens` gathers
  adjacent clauses each one plain undenied word of one run scoped alike (`run_tokens_together`'s
  reading, via `one_run_of`), joins them with a space and hands the result to
  `instead_of_the_name` (`instead_of_the_whole`'s second half, the one place a name is weighed).
  Shrinks from the longest run to two (most specific wins; a neighbouring token is left); a term,
  denial or scope change ends the run; `MOST_TOKENS_IN_A_NAME` bounds length.
  `beyond_a_word` prevents overreach: a run is weighed as a name only where a token is a word the
  catalog neither holds nor can spell nearer, exactly what a word at a time cannot reach. So *the
  great gig in teh sky* is **The Great Gig in the Sky** but *pink floid* is still **pink Floyd**,
  not *Pink Floyd* (the narrower fix stands wherever enough; casing is not written over a word
  typed right). Written back unquoted (offered means what was typed: words the search still ANDs,
  spelling fixed) unless scoped, then as a phrase like a split (else `artist:Pink Floyd` reads
  back as `artist:Pink` plus a loose `Floyd` over every field). The budget is in letters via
  `letters_in`, not bytes: folded Cyrillic or CJK earns what Latin of the same length does (*мир*
  is not offered as *мор*).
- **The read is the cost: paid once, kept until a name could have moved.**
  `Library::did_you_mean` walks every title, artist and album name, so the window asks only where
  albums, artists, tracks and lyric matches (`sung`) all came back empty (`browsed` reads them
  first, asks after, on the background executor with the rest of the load), and
  `spelling::worth_asking` refuses before the read where no word is long enough to correct.
  `Inner::vocabulary` stamps the `Spellings` with a counter and returns an `Arc` until the counter
  moves (typing past what is held pays one read, not one per settled keystroke).
  **SQLite itself moves the counter.** `watch_the_names` lays `NAMES_MOVED_TRIGGERS` on the
  writer: `TEMP` triggers (that connection alone; never the schema, a migration or another
  program) step a row of a `TEMP` table wherever a row of `tracks`, `albums`, `artists` or
  `artist_genres` is inserted or deleted, or a column the vocabulary reads (track title, artist,
  genre; album title; artist name; any update of an `artist_genres` row) takes another value; an
  `update_hook` on the writer sees that row move and steps the counter. Invalidation is derived
  from what was written, not a list of write paths to keep in step (a future pass is covered by
  using the writer), and per *column*: a counted play, a favourite, a scan
  writing a title back unchanged drop nothing. The hook answers per table and row (per column only
  via `sqlite3_preupdate_hook`, a compile-time flag on the bundled library), hence triggers weigh,
  hook counts; ~0.35 µs a row of a scan's inserts. Another process's writer is invisible to the
  hook: that half is `PRAGMA data_version`, read off the writer connection beside the counter into
  one `CatalogStamp { named, written_elsewhere }` (`Inner::names_stamp`), moving only for a commit
  some *other* connection made (a `resonate scan` beside a window drops the window's vocabulary at
  the next ask; this process's own writes stay the hook's). Read under `try_lock` (writer possibly
  held through a whole scan batch; a search must not wait); a busy writer is this process writing,
  so the stamp is then weighed by the counter alone. Every delete on the four tables carries a
  `WHERE`, so the truncate optimisation (the one case SQLite skips the hook for) cannot arise.
  `Library::written_elsewhere` hands the same reading out as a `WrittenElsewhere`, what the window
  watches for another process's edits (`ui.md`).
## Playlists

- **A playlist is a list of cuts, not library rows.** `Cut` = `MediaLocation` + `Option<FrameSpan>`
  (the pair `Track`, `QueueItem`, `Resumable` carry); `playlist_entries` stores `path`,
  `span_start`, `span_frames` (`store::span` convention, as `tracks`/`resume_rows`). Unscanned files
  are rows; a file leaving the library leaves its playlists named but unresolved. Read back =
  `LEFT JOIN tracks` on `path` *and* `span_start`: a `PlaylistEntry` has its `Cut` always, a `Track`
  only where cataloged (queue pane's unscanned fallback). Local files only: `add_to_playlist`
  refuses another source's `MediaLocation`.
- **A cut row is the cut, not the file, to the graph.** Once the join took the path's lowest
  `span_start` (three `.cue` rows of one FLAC drew as the first, counted its length thrice, played
  the whole file); `Held`/`Reaching` carried bare locations (a drag lost the span first); both
  `queue_items` built `span: None`. `Cut::of` = catalog row to cut; `Cut::whole` = a sheet's path,
  command-line file, bus `AddTrack` (no region vocabulary in those formats). Folding doubles reads
  the whole cut: two cuts of one file = two rows, one cut twice = one
  (`a_cue_row_put_in_a_playlist_is_the_cut_it_was_rather_than_the_file_it_came_out_of`,
  `the_rows_a_sheet_cut_are_a_playlist_of_their_own_lengths`,
  `two_rows_one_sheet_cut_out_of_a_file_are_not_doubles_of_each_other`).
- **Edits write only rows they move.** Append at `max(position) + 1`; remove shifts rows after; move
  shifts the span between ends; tidy rewrites from the first gone file. A shift parks rows at
  `-1 - position`, unparks in a second statement (`PRIMARY KEY (playlist_id, position)` refuses
  transient collisions; `UPDATE` visit order unpromised). Any span = one row's two writes (why
  `Span` reaches SQL). `position` stays dense (index = position), so spans are weighed against the
  list, never cast to `i64`: `move_in_playlist` answers false where either span end or the drop row
  is past the end, or the drop row is inside the span; `remove_from_playlist` takes first-named to
  list end, refusing a span starting past it. `undo::edited`'s restore point reads only reachable
  rows (`Undo` below): append to a list in hand none (kept: all), remove the span, move the rows
  crossed.
- **A name is one name however written.** `playlists.folded` = `playlist::folded` (`to_lowercase`,
  all alphabets unlike SQLite `NOCASE`; then NFC, as `to_lowercase` may not compose) with the
  `UNIQUE`: two spellings cannot coexist whatever `refuse_duplicate` does; one function for stored
  and asked names. `Library::playlist_named`, `HOLDS_THE_WORD`, `PlaylistOrder::Name` read it, so
  addressing, narrowing, ordering fold alike (`resonate playlist <NAME>`, the pane). NFC not NFKC:
  "Café" combining = precomposed, ﬁ and fi stay two (ligature folding is another claim). Written
  with the row: a database from before a fold change keeps its folds (the delete-and-rescan the
  schema note assumes). `Library::rename_playlist` refuses a spelling another playlist holds via
  `refuse_duplicate`, which excludes the renamed one (re-casing is not self-duplication).
  Rename/discard are not row edits, so a saved query takes both; CLI `--rename <NEW>` and
  `--discard` (`resonate playlist <NAME>`) have no undo.
- **Every row edit refuses inside its transaction.** `only_a_list` checks existence and
  not-a-query against the transaction, not a reader (`Error::UnknownPlaylist`, `Error::NotAList`
  never stale). Dropping passes: `Going` = the question (`Gone`, `Doubled`, `Unwanted` = gone or
  doubled for a tidy, `Matching`); `asked_of` asks it of rows `numbered` read inside the
  transaction (no closure names an unseen row). Only the filesystem is asked outside: `gone_from`
  stats each distinct path first through a reader (a downed mount holds no write lock);
  `Gone`/`Unwanted` carry the paths found gone; a row landing between reads stays. A refused edit
  still pays the restore point. `copy_playlist` reads its source outside its transaction by design
  (a copy is the source as it stood).
- **A playlist holds a list or fills itself from a query; both answer via `playlist_entries`.** A
  `playlist_queries` row = saved query: text, `SortOrder`, reading, row cap (`TrackQuery` minus
  album/artist). `Library::playlist_entries` runs it instead of reading the table (queue, pane,
  MPRIS, sheet writers see an ordinary playlist); listing counts are the query's (one `count(*)`
  per saved query atop the grouped pass). A query refuses with `Error::NotAList`: `add`,
  `remove_rows`, `move_rows`, `prune`, `sort_playlist`, `remove_matching`, `fold_doubles`,
  `copy_playlist` into it, keeping in order; rename, play, export, drop work.
  `Library::revise_query` rewrites text, sort, cap, reading in place, renaming in the same step
  (undoable, `Edit::Revised`); a list refuses it with `Error::NotAQuery` (each kind refuses exactly
  the other's edit).
  - No album/artist id held, so saving with an album selected cannot silently widen: *Save this
    search* appears only where the search box scoped the pane; CLI `--query` saves under an unused
    name and revises the one named.
  - Read when asked (can differ twice running); a queue loaded from one is a snapshot. `Plays`/
    `Played` + cap = *Top 25 most played*; `plays:0` = never heard.
  - `Library::playlist_lists` (the picker for putting rows in a list): grouped pass, queries
    excluded in SQL, no `count(*)`.
- **A list is put in order by the library, not a row at a time.** `Library::sort_playlist(RowOrder,
  Direction)` rewrites in place (one `DELETE`, dense reinsert from the first moved row; no
  park/unpark), answers rows moved: an ordered list costs no write, moves no revision.
  `RowOrder::Album`/`Artist`/`Title`/`Length` read the catalog via the tracks pane's order columns,
  unscanned last either way; `File` reads the path every row has (the one order placing unscanned
  rows). `Length` is seconds (rates differ). An edit, not a property: a list in hand still appends
  at the end. Pane: opened playlist's *Sort* (*Order*, *Reading* chip rows); CLI
  `--order <ORDER> [--reverse]`.
- **A list is in hand or kept in an order; kept refuses hand placement.**
  `Library::keep_playlist_in_order(Option<Kept>)` (`Kept` = `RowOrder` + `Direction`) writes
  `playlists.kept_order`/`kept_reading` and orders rows in the same transaction (sort once, then
  hold). `playlist::add` re-runs the pass, so hand adds, `--add`, imported sheets land where the
  order says. Kept refuses `move_in_playlist` and `sort_playlist` with `Error::KeptInOrder` (mirror
  of `NotAList`: its order is the sort's); remove, tidy, rename, export, add, drop work. `None` =
  back in hand, rows stay. Same kept order, or hand to hand, writes nothing (`Change::Nothing`, as
  every no-write edit).
  - Window `views/playlists.rs::Rows`: `InHand` (moved, edited), `Kept` (edited), `Narrowed`
    (edited), `Matched` (neither); movers, drag, reach, ✕ each ask one question, not "is it a
    query". *Sort* adds a third chip row *Keeps* (*Just once*/*From now on*); the index draws a
    kept list under the sort mark. CLI `--order <ORDER> --keep`, `--by-hand`.
  - Cost: whole list read per append (order re-read, rows rewritten from the first moved): a row
    sorting to the end of a kept thousand = one write, to its top = a thousand. Re-read only on list
    writes: a scan renaming a track or filling its album leaves a kept list as is.
- **What a search showed is what a drop removes; nothing empties a list in one gesture.**
  `Library::remove_matching` reads matched positions, rewrites from the first that goes:
  `prune_playlist`'s pass, both `dropped_where` (narrowing breaks the adjacency a `Span` needs; one
  question per row). Takes `&str`, not `Option<&str>` like `copy_playlist`, so no call empties a
  list (emptied = discard). *Drop shown* only under `Rows::Narrowed`; CLI `--matching <TEXT>
  --drop` (`--drop` requires `--matching`). A narrowing a search reads nothing in (`???`, `!!!`,
  fold-dropped punctuation) matches nothing, not no condition (`db::asks_for_nothing_it_can_read`):
  `--matching '???'` plays, copies, drops no row (it once took the whole list); blank = still no
  narrowing
  (`a_narrowing_with_nothing_a_search_reads_matches_nothing_rather_than_everything`).
- **A doubled row is folded away, first of each staying.** `Library::fold_doubles` drops each row
  naming a cut (path+span) an earlier row named, via `dropped_where`. Companion of `copy_playlist`
  (reconciles nothing, doubles what it lands) and `add_to_playlist` (a file twice is deliberate);
  nothing folds on its own. Reconciles on path like `import_playlist`: two names for one file stay
  two rows. `Library::tidy_playlist` asks gone and earlier-live-row-names-it (`Going::Unwanted`),
  dropping the union in one `Edit::Tidied` step (missing rows do not make later live rows look
  doubled); the pane draws it as *Tidy*, one undo restoring both. CLI `--tidy` (missing files,
  `prune_playlist`), `--fold` (doubles), refused together. One press, whole list: no fold out of a
  span, no preview of rows taken.
- **A playlist is copied into another, never moved.** `Library::copy_playlist` reads rows (all, or
  search-matched), lands them via `add_to_playlist`: source keeps them, a kept target orders them;
  `Error::IntoItself` refuses one playlist both sides (would only double it). Out of a query
  freezes a search into a list; only the CLI does it: `--into <OTHER>` (creates OTHER if absent).
  Cost: whole source in memory as locations, a write per row, plus order re-read for a kept target.
  Index and opened playlist use + to enter a track-browsing mode for the target; row-level +
  actions already holding tracks open `hold_for_a_playlist`.
- **A playlist is duplicated whole; one undo step.** `Library::duplicate_playlist` names the copy
  the first free of *NAME (copy)*, *NAME (copy 2)*, ... in one `undo::started_under_a_name_found`
  step: name read and free name weighed inside the taking write, so concurrent duplicators each get
  a name rather than one failing `DuplicatePlaylist`
  (`two_catalogs_duplicating_one_playlist_at_once_each_find_a_free_name`). List: rows as stored
  (spans too) plus kept order; saved query: same search, order, cap (not frozen into rows). Pin,
  plays, when-played stay with the source. *Duplicate* is in the menu of a playlist card, a playlist
  row, the opened playlist's more mark
  (`a_duplicated_playlist_holds_what_the_source_holds_under_a_free_name`,
  `a_duplicated_playlist_keeps_the_order_or_the_search_the_source_had`).
- **A playlist's picture: the covered albums its rows reach first.** `Library::playlist_pictures`
  answers at most the covers asked, one per distinct picture (`pictured_by` identity and
  likeness). List: own order (mosaic = opening); saved query without cap: `pictured_by` over its
  search; with cap: the rows the cap leaves (*Top 25* = those 25). `Library::pinned_playlists`: the
  other sidebar read, pinned ids and names, most lately pinned first, no join, no narrowing.
- **A search narrows a playlist; shown = played.** `Library::playlists` matches each standalone
  word against the name (all a playlist has); a term is passed over; no standalone word = whole
  index. `Library::playlist_entries` runs the whole text against the catalog: an unscanned row
  falls out; a saved query's text and the typed one must both hold.
  - `PlaylistEntry` carries `position`, so a narrowed row's number and ✕ are the list's.
    `Rows::Narrowed`: edited, not reached/moved (a reach is adjacent rows = a `Span`, the SQL's
    unit; narrowing broke adjacency).
  - Whole regardless of search: Rename, Sort, Tidy, Export, Discard. What is shown: Play, Play
    next, Add to queue, Copy, Drop shown, row gestures. Playing a narrowed playlist leaves
    `Library::playing_playlist` unset (the queue is part of the playlist, not it). CLI
    `resonate playlists --named`, `resonate playlist <NAME> --matching`.
  - A saved query's text and the typed one are read *beside* each other: `db::matching` takes the
    texts, reads each through `Search::read`, asks their clauses together (an unbalanced quote
    closes at its own text's end; a trailing `or` stays a word). Both under the query's cap (falls
    on what the two match); listing count stays the query's. No telling which a row answered; the
    same box *revises* a query via *Edit search* (one field read two ways).
- **A listing is an order and a `Direction`; its drawer picks both.** `Library::playlists` takes
  both, `order_by` writes the sense into SQL (pinned first, ties by id), so a reading is the
  query's, not a `reverse()`: never-played sits last in *Most recent first*, first in *Longest ago
  first* (SQLite NULL placement). `PlaylistOrder::reads` = the direction an order *opens* at:
  ascending `Name`, descending the other five (`Created`, `Modified`, `Played`, `Plays`,
  `PlaysThisMonth`; newest/most played on top). A default: the pane's *Reading* row and
  `resonate playlists --reverse` flip it; choosing an order resets it. The bus opens ascending
  always (MPRIS defines `CreationDate`, `ModifiedDate`, `LastPlayDate` oldest first) and maps
  `reverseOrder` to `Direction::Descending`, not reversing a read list.
- **Which playlist is in play is the library's, kept by the queue it was loaded as.**
  `Library::playing_playlist` = a cell beside the catalog (no column) holding a `Playing`:
  `PlaylistId` + the `QueueStamp` `engine::stamp_of` reads off the loaded rows (locations and spans,
  not handed ids). `playing_playlist(queue)` answers only where stamps agree: the cell *is* the
  claim, nothing for callers to tear up.
  - `Queue::rows_changed` restamps on every row arriving/leaving; `stamp_what_is_playing_from`
    stamps the rows outside the queued-next run, which the engine publishes as
    `PlayerState::queue_stamp`. So an `Insert` into the playlist's rows or a `Remove` takes the
    badge off wherever it came from (`RootView::queue`, a row's ✕, bus `AddTrack`/`RemoveTrack` with
    no window); rows queued next leave it. `Queue::split` restamps (a drag across the queued-next
    boundary changes which rows the queue plays from; a drag back restores the stamp). Reordering
    the same rows stamps the same (read off `items`, not `order`): queue move and shuffle leave the
    playlist in play and `PlayerState::loaded_position` still names the row heard.
  - Three setters, each stamping the items it sends: `RootView::play_playlist`,
    `Collection::activate`, the binary's `play_queue`. MPRIS `ActivePlaylist` reads the cell by
    the same stamp. `QueueItem` derives no `Hash`: `engine::stamp_of` is the only stamping path
    (`Collection::activate` once hashed whole items, ids included, which no published queue matched,
    so `ActivePlaylist` never named a bus-activated playlist).
  - Only held playlists: `set_playing_playlist` fills the cell from whether `played_now` counted the
    play (`counted_a_play`); an unheld id leaves cell and `ActivePlaylist` empty. Not persisted (as
    the queue); *when*/how often are: the same call writes `playlists.played`, steps
    `playlists.plays` (what `PlaylistOrder::Played`/`Plays` order on, bus `LastPlayDate` reads);
    loading twice counts twice.
  - `Library::playlists_revision` = companion cell bumped by every playlist write; a 200 ms bus poll
    watches it, re-reading a listing only when an edit moved it. The stamp costs a poll: the engine
    publishes after applying the load, so the badge trails the queue by a poll. 64-bit hash of rows
    in loaded order: colliding queues are one to it; a queue edited back is the playlist again.
## Undo

- **A playlist edit is one undoable step: the playlist as it stood.** `undo.rs` is all of it.
  `undo::edited` wraps every mutator's transaction: reads name, kept order, saved query and the rows
  its `Reach` names; keeps the restore point only if the change moved something (`Change::Made`, not
  `Change::Nothing`). `undo::started` is the twin for gestures creating a playlist (restore point =
  not existing). `Library::undo` writes the newest step back; a `Whole` step = one `DELETE` of the
  playlist row (cascades entries and query) then a dense rewrite, so a discarded playlist returns
  under its id. `played`, `plays`, `pinned` are read off the row, not restored (a play is no edit);
  the date last changed is restored (else a walked-back edit leaves the playlist climbing a *Last
  changed* listing). `Library::start_playlist` and `Library::revise_query` are single calls (the
  window's gesture is name-and-rows / name-and-search; two calls = two steps). Stack `Inner::steps`:
  32 steps (`STEPS_HELD`), 50 000 rows (`ROWS_HELD`), per run (command line: no undo); the newest
  step is always kept, so a long run drops the oldest, not the largest.
- **A step holds only the rows its edit could have moved.** The mutator names a `Reach`;
  `Reach::settled` makes the step's `Reached` inside the transaction:
  - `Unmoved` (rename, revision): no rows, none counted against the 50 000.
  - `From(len)` (`Reach::Appended`): rows from the old length on (one row added to 100 000: none).
  - `Window`: the run an edit replaced. `Reach::Emptied` (removed span, leaves nothing) or
    `Reach::Shuffled` (move: span and landing row bound the permuted rows, as many after as
    before). Clipped by `clipped` in signed arithmetic (a span may run to `usize::MAX`).
  - `Whole`: discard, sort, keep, row-dropping passes, append to a kept list (re-sort may move
    any row).

  Restore: `Unmoved` = `written_over` (`UPDATE` of the playlist row, query rewritten if any);
  `From` = `written_over` + `rewritten_from` (delete rows from its first on, write the held tail
  from where the list now ends); `Window` = `written_over` + `rewritten_within` (delete the
  `leaves` rows in the window, shift later rows by the difference via `closed_up`'s
  park-and-unpark, write `holds` rows into the gap; positions stay dense); only `Whole` takes the
  cascading `DELETE`, so only it reads `played`/`plays` off the row. `undo::walk` reads the
  standing it overwrites through `Reached::turned` (a window's two lengths traded), so the inverse
  holds the same stretch; a created playlist's step is `Whole`, its inverse the playlist whole. A
  move from the top of a long list to its end still crosses all of it.
  (`an_edit_to_a_long_playlist_holds_only_the_rows_from_where_it_reached`,
  `an_edit_near_the_top_of_a_long_playlist_holds_only_the_rows_it_crossed`)
- **A step is walked either way; walking writes it back the other way.** `undo::walk` serves
  `Library::undo` and `redo`: pop a step, read the standing it will overwrite, apply, push what was
  read onto the other stack. `Inner::walked` thus holds the playlist as each edit *left* it; no step
  carries an inverse, and a redone step walks back again. No playlist under a step's id =
  `Standing::Fresh` (discard and create are one shape from opposite ends); walking one that discards
  the playlist in play clears `playing_playlist`. The next edit ends a redo: `undo::note` clears
  `walked` before keeping a step (an edit writing nothing never reaches `note`). Ids stay safe: undo
  is strictly last-first, every id-freeing gesture leaves a step under every id-taking one, and
  every id-taking gesture is an edit, so a step on `walked` never names a playlist SQLite has since
  given the id to. `RootView::undo_edit` / `redo_edit` = the playlists headings' *Undo* / *Redo*,
  and `ctrl-z`, `ctrl-shift-z`, `ctrl-y` away from the search field (which binds its own three
  inside it). **Walked only over the playlist its edit left.** `Step::left_at` = the playlist's
  `modified` stamp as the edit finished (`None` if it discarded the playlist), read in the edit's
  transaction; `walk` re-reads it in the restoring transaction; any other value =
  `Error::PlaylistChanged`: step dropped (else refused for ever), nothing written. The command line,
  MCP or a second instance adding a row, and `organise::files_moved` rewriting row paths (touches
  `modified` per playlist changed), end the steps under them: an undo never destroys a row it did
  not hold or restores a path since moved. The inverse carries the stamp the restore wrote (the held
  one). (`an_undo_refuses_a_playlist_another_catalog_changed_since_the_edit`,
  `a_playlist_row_followed_to_its_new_path_ends_what_an_undo_could_put_back`) `Inner::walked` has
  its own 32-step / 50 000-row bound (not shared with `steps`): a long run of undos over long
  playlists can hold both ends. Nothing collapses a step walked back and forth into its origin: each
  press costs the edit's read and rewrite.

## Sheets

- **A playlist leaves and arrives as M3U, PLS or XSPF; a sheet names files, not tracks.**
  `sheet.rs` is the seam (`Library::import_playlist` / `export_playlist` the way in): reads bytes,
  settles encoding, decides from the *text* which format, hands `m3u.rs`, `pls.rs` or `xspf.rs`
  the job; `sheet::parse` (split from `sheet::read`) reads a sheet with no file. Shared there:
  `Described` (a row's seconds/artist/title), the relative-or-absolute path rule, percent
  escaping both ways, the staged rename over the target (a crash mid-write never truncates). The
  stage is a hidden sibling `.<name>.<pid>-<n>.new` (`staging_name_for`), `create_new` (never a
  listener's or another export's file), written and `sync_all`ed before the rename, removed on
  any failure: a refused export leaves nothing beside its target
  (`an_export_that_fails_leaves_nothing_staged_beside_its_target`). Writing picks the format by
  the target's extension (M3U where it declares nothing); reading picks by content, so a wrong
  extension still reads.
  - **XSPF text is unescaped in one pass**: an `&` looks for its `;` only within
    `LONGEST_ENTITY` bytes (`a_value_of_ampersands_never_closed_is_read_in_one_pass`).
  - **Track facts are never believed**: a `playlist_entries` row is a path, so `#EXTINF:`,
    `TitleN`/`LengthN`, `<title>`/`<creator>`/`<duration>` are written, never read.
  - **Non-`file://` rows** are counted, not refused (one stream must not cost the other fifty
    rows). Scheme and `localhost` authority read per RFC 3986: `FILE:///a.wav` and
    `file://LocalHost/a.wav` are local, an `xml:base` either way still resolves; `file:/a.wav`
    reads as `file:///a.wav` (query and fragment cut as after an authority); `file:track.flac`
    (no slash) stays relative. A row empty once trimmed (PLS `File1=`) names nothing, counted as
    elsewhere (once resolved to the sheet's folder and stored).
  - **Encoding**: byte-order mark (UTF-8; UTF-16 either way, odd length refused), else valid
    UTF-8, else the code page `resonate_core::text` detects (`rust-style.md`), unless the name
    declares UTF-8 (`.m3u8`, `.xspf`) or the bytes hold a NUL (no text sheet does); either refusal
    = `Error::UnreadablePlaylistFile`. `Imported::encoding` reports it. A line ends at `\n`, `\r\n`
    or a bare `\r` (an old Mac sheet; `sheet::lines_of`, and the cue reader likewise), never read
    as one comment line
    (`a_sheet_whose_lines_end_in_a_carriage_return_alone_reads_every_row`).
  - **Import reconciles by count, not set**: a row the playlist holds counts as already there (a
    sheet read twice is a no-op); a file the *sheet* names twice is two rows (`add_to_playlist`
    deliberately allows a file twice). A taken name is appended to, not refused: `import` and
    `resonate playlist <NAME> --add` are one gesture. A `.cue` given to `--add` becomes the rows
    it cuts, via the `sheet_cuts` that `resonate play` / `resonate queue` use, not a row naming
    the sheet.
  - **A row is written as a path only where `sheet::as_a_row` finds it reads back as itself**,
    else as an escaped `file://` URI (carries a `#`, line break or scheme of its own through M3U
    and PLS).
  - **Settling**: the parse settles lexically (`settled` in `sheet.rs`: absolute, `.`/`..` removed,
    no link followed; no filesystem access). `playlist::cuts_of` settles each row against the
    catalog: the path as written where a track holds it, else the path under a root a folder of it
    resolves to (scan stores a root canonical, what is beneath as walked, a followed link's own
    name included), else its canonical path; none of the three catalogued: canonical, or lexical
    where the file is absent (a sheet imported while a mount is down still names what a later scan
    will store). Canonicalising every row once lost a file a `follow_symlinks` scan stored through
    its link
    (`a_sheet_naming_a_file_through_a_link_the_scan_followed_lands_on_the_catalog_row`).
  - **A row naming a file nowhere on this machine is reconnected by trailing components**
    (`reconnected_by_trailing_components`): none of the three readings catalogued and nothing at
    the path: the catalog's paths are read once for the file names asked about; the row takes the
    one catalogued path whose last components (lowercased) agree with the most of the row's, at
    least two (file name + its folder); a tie at that depth leaves the row as written
    (`a_sheet_from_another_drive_layout_is_reconnected_by_the_one_file_its_trailing_folders_name`).
  - **Windows rows**: backslash and no forward slash = a Windows player's path; `forward_separated`
    turns the separators (`..\Music\01.mp3` resolves beside the sheet, not as one oddly named
    file). Reverse held: `reads_back_as_itself` refuses a row `forward_separated` would turn, so
    `AC\DC.wav` is written as an escaped `file://` URI, not a row reading back `AC/DC.wav`.
    Escaped forms are unambiguous: a literal backslash cannot stand in a `file://` URI or XSPF
    location (writers escape it `%5C`), so `sheet::forward_escaped` turns every literal one into a
    separator *before* unescaping (`file:///music\a.wav`, XSPF `album\a.wav`, `xml:base="discs\"`
    name folders); `AC%5CDC.wav` still reads as its one file.
  - **A reference ends at its first raw `?` or `#`**: `sheet::path_of_reference` cuts a `file:` URI
    and a relative XSPF location there before unescaping (as `MediaLocation::from_uri`):
    `file:///music/Echoes.flac#t=10` and this build's own `#frames=` URI are the file named;
    `%23`/`%3F` still decode into it. A plain M3U/PLS row is no URI: a mid-row `#` stays in the
    name.
  - **Size**: the 64 MiB ceiling (`LARGEST_PLAYLIST_FILE`, some 400 000 rows) is weighed against
    the declared size and again against what the read took (a FIFO reporting zero is refused, not
    read unbounded), and against what an export would write: `read_back_within` refuses a sheet
    past it as `PlaylistFileTooLarge` before a byte lands, so what this build writes it reads back
    (`a_sheet_too_large_to_import_is_refused_rather_than_exported`).
  - **Short and missing**: `Library::prune_playlist` drops rows whose files have gone (asked for,
    not automatic). PLS `NumberOfEntries` is weighed against the text, shortfall in
    `Imported::short` (a truncated sheet says so); M3U and XSPF declare no count (only PLS can say
    it lost rows); more than promised is taken whole silently. Import resolves and stats every
    row: a sheet naming a downed network mount reports each row missing rather than waiting;
    nothing tells a file grown between the two reads from one that lied about its size.
  - **A tidy drops only what is surely gone.** `gone_from` weighs a file gone where the stat
    succeeds on a non-file, or answers `NotFound`/`NotADirectory` and the path is as below (other
    failures, permission or `EIO`, keep the row):
    - on no volume `volumes::is_mounted` finds unmounted; under no root whose directory is
      missing; under no desktop mount point nothing stands at
      (`volumes::is_under_a_mount_point_not_there`: `/run/media/<user>/<label>`,
      `/media/<user>/<label>`, `/mnt/<label>`; the first missing component below the base, not
      itself a mount point); under none `/etc/fstab` lists that `/proc/self/mounts` does not
      (`volumes::listed_and_not_mounted`; `/` and swap aside);
    - and either the file's own folder stands (gone from a folder that is there, though emptied),
      or, its folder gone too, the nearest standing folder above holds something (an unmounted
      mount point reads as empty or absent, so a stick's album folders are kept whether or not
      the catalog ever noted its volume).

    **A row notes its volume as added**: `start`, `add`, `import` find, before their transaction,
    the mount point above each row's folder (highest folder sharing the nearest standing one's
    `st_dev`, `/` never one; `volumes::under`, one walk per folder); `volumes::note` writes them
    beside the rows, so a hand-mounted drive no table lists is out of reach once unplugged though
    no scan walked it and its songs sat at its root. A row added while its drive is already gone
    notes nothing; rows added before this build were noted by nothing
    (`a_folder_is_on_the_volume_whose_mount_point_is_the_highest_folder_sharing_its_device`,
    `a_volume_holding_only_a_listed_row_stays_noted_through_a_scan_and_its_row_through_a_tidy`,
    `a_row_on_a_drive_that_is_not_mounted_is_kept_by_a_tidy_and_a_deleted_one_is_not`,
    `a_prune_keeps_the_rows_under_a_root_that_is_not_there`,
    `a_mount_point_the_filesystem_table_lists_and_nothing_is_mounted_at_is_out_of_reach`).
- **A cue row leaves as its file plus VLC's times, and returns as the cut.** Where the format can
  say it: `#EXTVLCOPT:start-time=` / `stop-time=` lines before an M3U row; the same two options as
  `<vlc:option>` in a track's VLC `<extension>` in XSPF (playlist element declares the `vlc`
  namespace). Seconds = the cut's frames at the track's rate, rounded to the nanosecond. Reading
  back takes the rate the catalog holds for the row's path, or probes the file once on import
  (`probed_rate` keeps each file's answer: a sheet cutting one image into twenty rows opens it
  once), and rounds again, landing on the written frame at every rate the workspace holds (a
  frame is never shorter than 1.3 µs)
  (`a_timed_row_takes_its_rate_from_the_catalog_rather_than_probing_the_file`). A row whose file
  the catalog lacks and will not probe, or a PLS row (no word for a region), is the whole file.
  `Sheet::locations` holds `Listed` rows (a location and its `Timed`): the parse stays I/O-free
  and the fuzz target reaches it
  (`a_playlist_of_cue_rows_exports_and_imports_as_the_rows_it_holds`).
- **`xspf.rs` reads its own markup; every leniency is deliberate.**
  - A tag ends at the first `>` *outside* a quoted attribute value.
  - `xml:base` resolves down an element stack: a relative `<location>` answers to the base in
    force, not always the sheet's folder; a non-`file://` base makes every relative row under it
    count as elsewhere.
  - A track's `<location>`s are alternates: the first naming a local file is the row; none = one
    `elsewhere`.
  - `<!DOCTYPE …>` and `<?…?>` are skipped, not pushed (a pushed, never-popped element would carry
    its base to the rest of the sheet).
  - `<album>`, `<image>`, `<annotation>`, `<meta>` stay unread on purpose: a sheet is never a
    source of tags.
  - Trusted: elements match by local name (two namespaces both calling something `track` are one
    element); only the five XML entities and numeric references (decimal, or hex after `x`/`X`)
    are known; nesting is trusted (a never-closed element carries its base to everything after).
  - Writing is strict where reading is lenient: `marked_up` escapes the five entities and drops
    every character XML 1.0 forbids (`is_an_xml_character`: C0 controls but tab/LF/CR, U+FFFE,
    U+FFFF; not even a numeric reference may carry them), so a title holding U+0001 writes a
    sheet VLC and Kodi read rather than refuse.

## The MPRIS seam

- **`resonate-mpris` reaches playlists through a seam, never the library.** `Playlists` is the
  trait `Mpris::start` takes beside `Host`, filled by the binary with a `Collection` over the
  `Library` and `Player`. The interface is served only when one is supplied: no catalog = no
  `org.mpris.MediaPlayer2.Playlists` advertised at all, not an empty one.
  - `Orderings` comes from `mpris::PlaylistOrder::ALL` (Alphabetical, CreationDate, ModifiedDate,
    LastPlayDate: the spec's four, distinct from the library's six): an unadvertised ordering is
    refused, not answered under another; `UserDefined` is among them (a playlist's own order
    cannot be asked for by name on the bus).
  - `PlaylistCount`, `ActivePlaylist`, `PlaylistChanged` are diffed from the player properties'
    200 ms poll, against a listing re-read only when `Library::playlists_revision` moves. `moved`
    asks of every row: a playlist held under another name = `PlaylistChanged` (the spec keeps it
    for a name or icon change); one that arrived or left = `PlaylistCount` announced, even where
    an addition and a removal in one sample leave the count equal (the spec has no signal for a
    playlist made or gone; a client re-reads `GetPlaylists` when the count is announced).
  - Neither seam read is `Library::playlists` (a grouped pass over every row of every playlist
    plus a `count(*)` per saved query; the bus keeps only id and name): `Playlists::count` =
    `Library::playlist_count`, one `count(*)` over `playlists`; `Playlists::listing` =
    `Library::playlist_names`, id and name with no join, `GetPlaylists`' `index` and `maxCount`
    as `OFFSET` and `LIMIT` (asking for ten reads ten). `PlaylistCount` is read by every client
    and re-read on every announced change, hence it had to stop measuring a listing. Both query
    SQLite on the bus thread, so a client asking for a listing pays for it there. The bus cannot
    ask for a narrowing, so the seam takes none.
