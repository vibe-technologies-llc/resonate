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

- **A catalog on disc is checked and kept before it is migrated.** Where `lay_out` finds a stamp a
  prefix names but not this build's, it runs `PRAGMA quick_check` (anything but `ok`:
  `Error::CatalogDamaged`, left unmigrated), then `VACUUM INTO` `<catalog>.before-<stamp>`
  (`schema::kept_before`), staged as `….part` under `writer::Held` and renamed once synced; an
  existing copy is kept. Migrations are irreversible (one deletes `loves_told` rows), so that copy
  is an older build's way back. In-memory catalogs keep nothing.
  (`a_catalog_on_disc_is_kept_as_it_stood_before_it_is_migrated`)
- **Schema = `V1` + `MIGRATIONS`; catalogs migrate, not break.** A change is a new SQL step
  appended; never edit `V1` or a written step (strands stamped catalogs). `PRAGMA user_version` =
  `fingerprint_after`, a compile-time FNV-1a over `V1` then each step; `SCHEMA_FINGERPRINT` is the
  last. `lay_out`: `UNSTAMPED` writes `V1` + all steps; this build's stamp opens; else finds the
  step prefix the stamp names and applies the rest in one `IMMEDIATE` transaction, reading the stamp
  inside it (a second process waits, then reads the stamp left); a failing step is
  `StoreOp::Migrate`, stamp unchanged. Only a stamp no prefix names is `Error::SchemaMismatch`: the
  one case where the catalog is deleted and rescanned. The stamp stays a hash (a counted version
  cannot tell same-count builds apart). **Migrate wherever rows can be carried**; break only for an
  index whose meaning changed beyond SQL recomputation, signalled by the step's absence. History
  begins `0ca1e683` (`V1_AS_FIRST_STAMPED`). (`the_first_schema_is_never_edited_where_it_stands`)
- **A changed probe is carried forward by marking rows.** A migration step sets `tracks.probe_again`
  on rows the new probe bills differently (first: every WavPack, `codec = 11`). An incremental scan
  treats a marked row as changed despite unmoved size/mtime; upsert clears the mark; vault links and
  lookup results stay. Never a whole-catalog rescan.
- **Transport state = three tables, each moving at its own rate.** `resume`: singleton (queue's row,
  frame, shuffled, taken, `next_first`/`next_last`). `resume_rows`: one row per *loaded*-order
  position. `resume_order`: one per *playing*-order position. `Keeping` (`resonate-engine`
  `kept.rs`) picks the write: unchanged queue = one upsert of the place; reorder/reshuffle =
  `resume_order` only; only rows arriving/leaving rewrite `resume_rows`. `keep_place` writes row and
  frame only, so a place written seconds into a track cannot lose the order; `keep_order` writes
  order, place, shuffle, next span together. Rows carry `MediaLocation::to_uri`;
  `span_start`/`span_frames` follow `store::span`, so a cue row resumes as its cut.  A URI no longer
  naming a location drops that row alone (`resumed::Kept` renumbers over the rest; playing row
  dropped = resume at the next row's start; no openable row = `None`). `keep_resumption` replaces
  all three in one transaction; `resumption` reads them in one read transaction.
- **A favourite is when, not whether.** `tracks.favourite`, `albums.favourite`, `artists.favourite`
  are nullable nanosecond stamps, so the marking column also orders by when marked.
  `Library::favour` takes one `Favoured` and answers whether the value moved: favouring a favourite
  keeps its stamp and answers false, as does an unknown id. `SortOrder::Favourited` needs
  `tracks_by_favourite` declared `favourite DESC, title_filed COLLATE NOCASE`, exactly as its `ORDER
  BY` reads; album and artist orders need no index.
- **A genre is the track's and its artist's at once, in one column.** `tracks.genre` is the scan's
  `TagSet::genre`; the fourth `tracks_fts` column is it folded through `folded_letters`, joined with
  the artist's `artist_genres` names. `land_artist` writes `artist_genres` long after the scan, so
  `store::reindex_the_tracks_of` runs beside it; `artist_genres` is in `NAMES_MOVED_TRIGGERS` and
  `NAMED_TABLES`, else a landed genre leaves vocabulary and suggestions stale.
- **A pinned playlist leads every order; `undo.rs` is where that is easy to lose.**
  `playlists.pinned` is a nullable stamp; `order_by` prefixes `PINNED_FIRST` onto every
  `PlaylistOrder`. Undo re-creates the whole row from `Held`, so a `playlists` column missed in
  `COLUMNS`, `read`, `Held`, `held_in`, `rewritten` is silently dropped the first time an edit is
  walked back. A pin is no edit: `rewritten` reads it off the live row via `unedited_in`.
  (`a_pin_made_after_an_edit_survives_walking_the_edit_back`)
- **Listening: reads over `listens`, `passes`, `unheld_listens`.** `listens.heard` = nanoseconds
  actually heard; `passes` (`Library::passed`) = time on visits that never earned a play.
  `statistics` and `listening_by_day` add `passes` time and `unheld_listens` to `listens`; tracks,
  albums, artists heard and the most-heard lists read `listens` alone. All are bounded by
  `listens.at >= ?`. **A day is the listener's, not Greenwich's.** `listening_by_day` groups stamps
  by quarter hour in SQL (every offset and transition is whole quarters) and folds each into the day
  `resonate_core::Calendar::local` says (jiff's system zone, from `TZ` or `/etc/localtime`, which
  jiff reads again after five minutes; UTC if unreadable); `Day::at` = the calendar's midnight (23
  or 25 hour days; `Calendar::midnight_of`). **A phone names its zone, never only its offset**:
  `Calendar::named` reads an IANA identifier from the system database (on Android first the
  `com.android.tzdata` module's `etc/tz/tzdata`, under `ANDROID_TZDATA_ROOT`, since jiff looks only
  at the older `/system` and `/data` copies, which mainline updates no longer touch), else from the
  copy of the database the phones bundle (`tzdb-bundle-always`): iOS's sandbox is not known to let
  the zoneinfo folder be listed, and a stale bundled rule is better than a day cut at one offset all
  year. The phones' `listening_detail` folds quarter hours into days the same way
  (`catalog_extras::by_day`); a name neither database holds falls back to the offset the phone
  passes beside it (`the_listeners_calendar`), so the screen still draws, cut as it was before
  (`a_winter_first_of_the_month_counts_in_its_month_while_the_phone_keeps_summer_time`).
- **History is kept as long as the listener says, never past what a service was told, and a service
  no longer signed in to holds nothing back.** Every open of the catalog by the binary first asks
  `Library::stop_telling` for each service whose settings hold no account: its `submissions` and
  `earlier_submissions` rows go (read first, so a reader-only process writes nothing). `HistoryKept`
  = `Forever` (default) or days. `Library::age_the_history` deletes `listens`, `unheld_listens`,
  `passes`, `playlist_plays` older than it (`forgotten_before`), a listen only at or behind the
  lowest `submissions` mark. `tracks.plays`/`played` are separate counters and stay. The binary ages
  in `open_library_with`; the settings pane arms a shorter span on the first press
  (`RootView::aging_the_history`, lowered by `disarm`) and keeps it on the second; a longer span or
  *Forever* is kept at once.
- **A service's told-ness is a mark in the history.** `submissions`: per `ListeningService`, id of
  the last `listens` row told. `Library::submit_listens` (`scrobble.rs`) reads listens past the mark
  in id order, `SUBMITTED_AT_ONCE` (100) a batch, each at `listens.began`, joined for the names and
  MusicBrainz ids a `Scrobble` carries, hands them to the `Scrobbler` (library-owned seam;
  `resonate-online`'s `ListenBrainz` fills it) in pieces of at most `Scrobbler::at_once` (default
  `SUBMITTED_AT_ONCE`; Last.fm answers 50) and moves the mark past each piece in a `max`, so a
  later piece failing never retells an earlier one
  (`a_batch_a_service_takes_in_pieces_is_marked_a_piece_at_a_time`).
  `listens.id` is `AUTOINCREMENT` seeded at the greater of its newest id and every mark, so an
  aged-away id is never reissued below a mark. No title or artist = `Submitted::unnamed`, passed
  over. A batch refused as malformed (`Refused`, 400) is retold a listen at a time; a listen refused
  alone = `Submitted::refused`, passed over; any other failure moves the mark only past what was
  told and answers the error. A service with no row yet is marked at the last listen held and told
  nothing (`Submitted::started`). **Asked, it does**: `Library::tell_earlier_listens` notes an
  `earlier_submissions` row (`through` 0, `until` the service's first mark) and every
  `submit_listens` after its own pass tells listens in `(through, until]`, counted in
  `Submitted::earlier`; aging holds back every listen past the lower of both marks. Counting a play
  writes nothing here, so whichever run submits next tells it. `Library::billed_as` = the same names
  for one row, what `playing_now` is told. Two runs submitting at once may tell a batch twice;
  ListenBrainz keeps one listen per moment and name. The binary's `submitting.rs` is the asking
  thread (`online.md`).
- **A suggestion is a named saved query, so it costs almost nothing.** `suggest.rs` answers
  `Suggestion { name, reason, query, rows, length, pictured_by }`; the query is written through
  `Display for Search`, so suggestion and search-box parse cannot drift. A genre/artist with no
  letter or digit or containing `"` is not offered. Saving = `Library::save_query`; nothing persists
  until saved; nothing offered below `ENOUGH_TO_OFFER` rows; `names_a_decade` drops a genre that is
  only a decade. `Library::suggestions` answers an `Arc<[Suggestion]>` kept beside the search
  vocabulary under a counter the `update_hook` steps for any write in `tracks`, `albums`, `artists`,
  `artist_genres` (plays and favourites count too); another process's writes drop both via the data
  version. `length` is an aggregate with no `ORDER BY` wherever the query has no limit/offset.
  `pictured_by` = `db::pictured_by`: albums holding a picture the search's rows fall on, most rows
  first, up to `PICTURED_BY_AT_MOST` (4), skipping an album whose picture another already stood for,
  then one that merely *looks like* one standing. `store::the_picture_of!` = `coalesce(cover_key,
  cover_print)`, where `albums.cover_print` (length + hex of the first 256 bytes, kept by the
  `albums_print_their_cover_*` triggers) means a sweep never reads the cover.
  `resonate_codec::Likeness`: cover averaged in linear light onto 8x8 cells; alike within
  `ALIKE_WITHIN_A_ROOT_MEAN_SQUARE_OF` (12 of 255); `likeness::of` keeps it in `likenesses` under
  the picture identity (`ORPHANS` removes rows no album's picture names; `update_hook` ignores the
  table).

(`every_suggestion_reads_back_through_the_grammar_it_was_written_in`)
- **A share is the link alone, one for three callers.** `Library::shareable` reads the track and its
  `release_track_links`, `album_links`; `Shared::written` is a pure function over them, so window,
  `resonate share` and others agree. **Song.link's own short page wherever the service has one**:
  `ShortForm::of` writes `https://song.link/<letter>/<id>` (song) or
  `https://album.link/<letter>/<id>`: `s` Spotify, `d` Deezer, `t` Tidal, `i` Apple Music, `y`
  YouTube and YouTube Music. A service with no short page or a URL with no readable id =
  `https://song.link/` + the service URL percent-encoded as one path segment (raw, the server
  collapses `//` and reads `?` as its own query). Candidates: `RELATIONS_SONG_LINK_TAKES` x
  `SERVICES_SONG_LINK_RESOLVES`; recording's links, then album's, each in declared service order.
  **Nothing song.link opens in the catalog: ask the reference where the track streams**:
  `Shared::streamed_where_asked` hands `Reference::streamed_at` a `StreamAsked`, puts the answer
  first; already-linked asks nothing; a failing reference = warning.  Failing all: the MusicBrainz
  recording; failing that, nothing to copy.
- **Every pane order is read off an index; planner knowledge is written after a scan.**
  `tracks_by_album` carries the trailing `title_filed COLLATE NOCASE`; `tracks_by_title`,
  `_artist_name`, `_added`, `_duration`, `_plays`, `_played`, `_favourite` serve the other
  `SortOrder`s (`SortOrder::HELD_BY_AN_INDEX`; `Relevance` needs none), each declared as its `ORDER
  BY` reads, collation and `DESC` included (SQLite walks an index backwards only where the whole
  order runs one way). `SortOrder::PlaysThisMonth` is outside the list (correlated `count(*)` over
  `listens`, `heard_this_month!`). **A reversed order needs no index**: `order_by` is a `Reading` of
  the natural spelling and its mirror (every term flipped);
  `db::tests::every_order_the_panes_offer_is_read_off_an_index_either_way_round` plans
  `HELD_BY_AN_INDEX` x `Direction::ALL` through `db::listing`, refusing a temporary B-tree.
  `resonate playlist --reverse` turns `--sort` as well as `--order`; without either the grammar
  refuses it. **Every order ends on the row's own id** (`tracks.id`; `a.id`, `r.id`; `p.id` for
  playlists): tied rows keep one place, paging neither repeats nor drops one. The id runs as the
  index's rowid tail runs (`track_ids_rising!`, `track_ids_falling!`), so the tie costs no sort.
  (`every_order_ends_on_the_rows_own_id_so_a_page_never_splits_a_tie`)
- **Albums and artists panes have orders too, deliberately unindexed.** `AlbumOrder` and
  `ArtistOrder` read those tables, which hold thousands of rows against `tracks`' hundreds of
  thousands, so a temp B-tree beats an index. **An artist files under the name its files sort it
  by**, else MusicBrainz's sort name, else its own: `filed_as!` = `coalesce(tagged_sort, sort_name,
  name)`, tied on by the artists orders and the albums' artist order (`album_owner_filed_as!`).
  `artists.tagged_sort` = the scan's `ARTISTSORT`/`ALBUMARTISTSORT`, taken once a pass per artist,
  never cleared by a file naming none; a billing read as a list's lead takes none. **An album files
  under the title its files sort it by**: `albums.tagged_sort` = `ALBUMSORT`, once a pass per album;
  `album_filed_as!` = `coalesce(a.tagged_sort, a.release_title, a.title)`; `gather` keeps the
  survivor's own, else the absorbed one's. **A track files under the title and artist its file sorts
  it by**: `tracks.title_sort` = `TITLESORT`, `tracks.artist_sort` = `ARTISTSORT` for the whole
  credit; both written by every upsert, followed by a tag write (`files_retagged`). `title_filed` is
  a generated `VIRTUAL` column reading the sort name only while the row is billed as the file names
  it (`title IS tagged_title`). `artist_filed` is a plain column kept by triggers
  (`tracks_file_their_artist_when_added`/`_when_named`, `artists_file_their_tracks_when_sorted`):
  the file's `artist_sort` while billed as the file names it, else the artist's
  `coalesce(tagged_sort, sort_name)` where `artist_id` names an artist whose name is the whole
  credit, else the credit. Pure SQL, so any SQLite keeps it; every track order and order index is
  declared on the two filed columns. The steps adding the sort columns mark scanned rows
  `probe_again`.
- **A saved query's direction is its own column.** `playlist_queries.sort` = order alone
  (`store::sort_code`/`sort_of`); `playlist_queries.reading` = direction (`direction_code`/
  `direction_of`; `OrderedColumn::Reading` names it in a refusal). It once rode in `sort` as order +
  `READ_BACKWARDS`, which an older build misread; the step adding the column carries every such code
  out of the sum.
- **The planner is told, and connections are sized to their job.** `schema::restate_the_statistics`
  (`PRAGMA analysis_limit` + `PRAGMA optimize` on the writer after a scan that changed anything, a
  vault import that kept a row, a poll that kept a delivery, a playlist import): with no
  `sqlite_stat1` the planner picks join order from hardcoded guesses. `configure` gives a connection
  a 256 MiB memory map (`MEMORY_MAPPED_BYTES`) and a page cache by `schema::Role`: `Writing`
  `WRITER_PAGE_CACHE_KIB` (8 MiB), `Reading` `READER_PAGE_CACHE_KIB` (2 MiB).
- **A reader is checked out and handed back; `READER_POOL` (8) is how many may exist, not how many
  are kept.** `Inner::checkout` takes a parked connection, opens one while under the count, else
  waits on `Inner::freed`. `Reader` returns its connection from `Drop`.
- **Every write begins `IMMEDIATE`.** `Inner::write`, `reconcile_artists`,
  `settle_the_credits_if_owed` take the write lock as the transaction opens, so a second process
  waits its `busy_timeout`; a deferred transaction that read before writing fails at once with
  `SQLITE_BUSY_SNAPSHOT`, which no timeout waits out.
- **A cue sheet claims the file it names; the scan reads sheets before audio.** `.cue` is a sidecar,
  not in `AUDIO_EXTENSIONS`: `directory_of` reads every sheet in a directory first (name order),
  resolves each `FILE` against the sheet's folder, then probes audio no sheet claimed, so one FLAC
  is not stored as an album's worth of rows plus a whole-file row. A `FILE` cutting no audio track
  claims nothing. A `FILE` naming a folder below the sheet (`cue::folder_named`, `audio.md`) is
  matched against the audio listed there; the claim is kept in `claimed_from_above`, which that
  folder's own pass takes the file out of; a lower sheet naming a file already claimed from above is
  passed over. **Two sheets in one folder naming one file: first in name order keeps it.** A `FILE`
  matches audio beside the sheet by its last component (split on `/` and `\\`) via `cue::Naming`:
  exact name, else any case, else same stem with another extension; closest reading wins, a tie
  names nothing. `organise` resolves claims through the same `scan::claimed_beside`; a file cut by a
  sheet in a folder above = `Refusal::NamedFromAbove`; a sheet naming files in two folders =
  `SharesASheet`; `cue::renamed` rewrites whichever `FILE` line the moved file answered to.
  Incrementality weighs two mtimes: `tracks.modified` = the audio's, `tracks.sheet_modified` = the
  sidecar's (NULL where none cut the row). Editing a sheet rescans its rows; removing it reprobes
  the file and prunes rows the cut no longer names whichever mtime was later.
- **Embedded sheets are cut in the probe worker, not the walk.** A file's chapters are such a sheet
  (`audio.md`): audiobook = row a chapter. `read_candidate` probes then cuts on `MediaInfo::cue`
  where it names audio tracks, sharing `cut_into_rows` with `read_cut`. A sidecar still wins:
  `claimed` removes its audio file from the audio pass before `whole_file_job`.
  `rows_past_the_first` is what the worker adds beyond the one `discovered`.
- **The walk reads the catalog once, bounded by the roots.** `Known::under` takes `(path, id,
  file_size, modified, sheet_modified, probe_again)` per walked root in one indexed range query, as
  a snapshot before the walker starts; safe as no path is walked twice per scan. `Known::rows` =
  every stored row of a path in `span_start` order; unchanged = at least one row, each matching
  size, mtime and sheet mtime, none `probe_again`; `Candidate::existing` carries them all.
- **One pass walks the tree at a time; a second is refused, not queued.** `Inner::walking` is the
  flag, `Walk` the guard. Taken by every pass walking or rewriting the tree: `Library::scan`,
  `organise`, `retag`, `import` (each in its `start`, before the thread spawns), `prune_the_vault`,
  `release_from_vault`, `delete_tracks`; and by the root edits a walk reads under: `add_root`,
  `remove_root`, `forget_the_gone`, `retire`. The thread owns the guard; `Drop` returns it on a
  panic. Taking never waits: `Error::AlreadyWalking` at once. Why: `Known::under` is a snapshot, so
  an organise committing a path rewrite before the walker reached the directory left it probing the
  file as new and the prune taking the rewritten row; two scans each prune what their own walk
  missed. `enrich` and `poll` stay outside.
  - **Across processes**: a catalog on disc also takes an exclusive `File::try_lock` on
    `<catalog>.walk` (`locked_across_processes`), released as its `File` closes (panic or kill
    alike); held elsewhere = `Error::AlreadyWalking`.
  - **Wind-down**: `Library::wind_down(patience)` cancels the pass through `Walk::cancelled_by`,
    waits for the guard to drop, answers whether it did.
  - **Lookup and poll** likewise, each with its own lock (`<catalog>.enrich`, `<catalog>.poll`,
    `Library::asking_alone`), refused as `Error::AlreadyAsking`: two processes never ask MusicBrainz
    or a provider about the same rows and double the paced rate.
- **A folder-grouped album is re-keyed in place when its folder moves.** Only the sleeve tier embeds
  a path, so only it can name a vanished folder; a re-probed file would key onto the new folder,
  insert a second `albums` row, and `ORPHANS` would sweep the old row's cover, `mbid`,
  `release_group`, `release_tracks` and `wants`. `organise::re_key_the_sleeves` runs in its own
  transaction once every batch has landed: takes the albums the moved files name, keeps those
  `store::is_keyed_by_its_folder` answers for, writes `store::sleeve_key` of the title and the
  folder its tracks now share (`scan::sleeve`; tracks in several folders, or directly in a root,
  keep the key; an album `album_keys` names more than once is left alone). `album_keys.key` is
  primary, so an album moving into a folder another album names keeps its old key.
  - **What the re-key leaves alone, a probe keeps.** A rescan or a probe of a file whose size/mtime
    moved computes the new folder's key afresh; `store::kept_where_it_was` answers first: where the
    row already belongs to an album of the same title, none of its other names finds an album, and
    its folder's key names another album or none, the row stays and `lend_the_free_names` gives the
    album whichever of its keys nobody holds.
- **A scan says what went wrong with a file.** `ScanStats::failed` = `Failures`, four counts;
  `Failure` decides: `Unnamed` (non-UTF-8 path: the catalog keys on text paths, so such a file is
  counted and never catalogued, whether or not a sheet cuts it, and a sheet with such a name still cuts), `Misnamed` (`UnrecognisedContainer`,
  `NoAudioTrack`), `Undecodable` (`NoDecoder`, `DsdCompressed`, unrepresentable properties or
  packets), `Unreadable` for the rest (`Io`, `Symphonia` residual: where a corrupt file lands). The
  match on `resonate_codec::Error` is exhaustive, so a new codec variant fails to compile here.
- **Hidden is not music.** `scan::is_passed_over` drops every dot-name (`.Trash-1000`,
  `.stversions`, AppleDouble `._`, a tag write's staged copy) and `$RECYCLE.BIN`, `System Volume
  Information`, `lost+found` from the walk and `audio_below`. A dot-named root is still walked.
- **A root may not be inside a root; a wider one absorbs what it covers.** `store::register_root` is
  the one writer (`Library::add_root` and the scan's `roots`): inside a registered root =
  `Error::RootInsideRoot`; containing registered roots = re-parent their tracks onto itself and drop
  their `roots` rows (not delete: `tracks.root_id` cascades and widening must not cost a play).
- **A row reached unchanged is not written; only what the walk missed is marked.** An upsert stamps
  `tracks.seen` with the scan's `generation`; an unchanged row (`Outcome::Seen`) or kept one only
  lends its id to `store::Reached`, in the writer's memory. Once the walk is whole,
  `store::mark_the_unreached` writes `seen = -generation` on ids under walked roots that `Reached`
  lacks. Prune and `moves` read that mark; one left by a scan killed before its prune is never read.
- **What the scan could not read, it keeps.** The prune takes every unreached row, so a row counts
  reached wherever a gone file cannot be told from an unreadable one.
  - A file whose size/mtime moved and which then would not probe is counted failed; its
    `Candidate::existing` rows are kept as `Outcome::Kept`; likewise a cue-cut file.
  - A directory `read_dir` refuses or whose listing an error cuts short (entries are gathered whole
    first), an entry whose kind cannot be read, an entry whose `stat` fails: every row
    `Known::at_or_under` names at or below the path is kept.
  - A cue sheet that cannot be read (`scan::heard_sheet` answers `Heard::Unreadable`: an `io` error
    but `NotFound`, longer than `LARGEST_SHEET_ON_DISC`, or zero bytes, as a sheet read mid-rewrite):
    it claims nothing, but every row with a `sheet_modified` that `Known::cuts_in` finds on a file
    beside it or in a folder below within `DEEPEST_FOLDER_A_SHEET_NAMES` is kept and its file is not
    probed whole (a file below goes into `claimed_from_above`, so its own folder's pass leaves it,
    and a sheet of that folder's own does not recut it until the upper one reads again). A sheet that
    parses to nothing (`Heard::Blank`) keeps nothing.
    (`a_sheet_read_as_empty_keeps_the_rows_it_cut_in_folders_below_it`)
  - An *empty* folder is guarded only where seen as a volume: keeping every empty folder's rows kept
    `moves::follow_the_moved` from pairing any move out of a folder it emptied.
- **A volume seen mounted is remembered; its rows are kept while it is not.** The walk notes a
  directory whose `st_dev` differs from its parent's, or that `/proc/self/mounts` lists as a mount
  point (`volumes::MountPoints`, read once a walk and a call to `absent`: a bind mount of the same
  filesystem shares its parent's device), root included and a followed link into another
  filesystem, as a volume. (`a_bind_mount_of_the_same_filesystem_is_mounted_where_the_table_lists_it_and_not_otherwise`) `volumes::settle` writes them to `volumes` after the prune and
  drops a row for one no longer mounted that no track and no playlist row sits under. A noted volume
  whose device is its parent's (empty mount point) or whose directory is gone keeps every row
  `Known::at_or_under` names as `Outcome::Kept` and is not walked; `tidy_the_roots_beside` and
  `forget_the_gone` skip paths on one.
  - **Retiring a volume for good is the listener's call.** `Library::retire` (takes the `Walk`
    guard) takes a folder at or inside a root, forgets every row at or under it whose file is not
    there, then drops each `volumes` row at or under it holding no track
    (`volumes::retire_at_or_under`). `forget_the_gone` is the same walk with unmounted volumes
    guarded. A file that *is* there keeps its row.
- **Every walk hazard but a lost worker is stepped past.** A directory past `MAX_DEPTH` is warned
  over, not read, every row kept. A symlink is weighed only where it names a directory (and
  `follow_symlinks` is on); every directory is walked once by its canonical path
  (`Visited::first_time_through`). A link whose canonical target is inside a registered root, or
  holds one, is never followed (`Walking::reaches_a_root`, over every root `roots` holds): else
  files are stored twice under both paths or filed under the wrong root. Not stepped past: a
  panicked probe worker. `run` joins every one and answers `Error::Stopped` naming the scan's
  `PassKind` before the prune (short counts would prune rows never reached). A failing commit drops
  the result channel's receiver before any join, so blocked workers wake, the job channel closes
  under the walker, and the scan returns the `Walk` guard.
- **A scan tidies every root, prunes only walked ones, and touches a root not there not at all.**
  `tidy_the_roots_beside` handles registered roots not walked: reads their distinct paths
  (`store::paths_under`), keeps those still there, hands the rest to `store::forget_paths`;
  `store::sweep_orphans` runs once at the end where anything went. A bare `resonate scan` walks
  every root. `is_there` guards both: a registered root whose directory is missing is dropped from
  the walk and stepped over by the tidy (an unmounted drive is not an empty one); a root named on
  the command line is `Error::RootNotADirectory` where missing. A window-watch scan
  (`Library::scan_what_is_held`) walks only the named roots `roots` still holds and that are there,
  registers nothing, tidies no root beside them (`scan::Beside::LeftToTheWatch`), answers `None`
  where none is left; the window takes a root off what it owes only once a scan has taken it.
- **A per-row statement is prepared once a connection.** `store::cached` runs literal SQL through
  `prepare_cached`; every per-row write uses it; per-row reads and the 39-column track upsert use
  `store::queried`. A scan cycles through more statements than rusqlite's default cache of 16, so
  `connect` raises it to `STATEMENTS_CACHED` (128). `format!`-built SQL stays on `execute`.
- **A track is keyed by `(path, span_start)`.** N cue rows share a path: `UNIQUE` on the pair,
  `span_start` `NOT NULL DEFAULT 0` (NULLs are distinct in a unique index). Every lookup meaning
  *this row* takes the span beside the path (`Library::track_at`, `Library::track_played`). A
  playlist entry stores `path`, `span_start`, `span_frames` and joins `tracks` on both
  (`ON_THE_SAME_CUT`).
- **Each MusicBrainz id is weighed against the column of its kind.** `tracks.mbid` = *recording* id
  (`TagSet::musicbrainz_track_id`); `release_track_mbid` = `MUSICBRAINZ_RELEASETRACKID`, the track's
  place on one release. `rematch_release_tracks` weighs each against its own (`recording_mbid` vs
  first, `track_mbid` vs second).
- **An artist is keyed by the fold of its name: one spelling, one artist.**
  `resonate_core::folded_letters` (re-exported by `store`) lowercases, decomposes, drops a combining
  mark on a Latin, Greek or Cyrillic letter (`takes_accents` over `ACCENTED_SCRIPTS`), keeps every
  other (kana voicing mark, Indic vowel sign: they change the word), recomposes, and spells out
  letters Unicode does not decompose (`ł`, `ø`, `đ`, `ð`, `þ`, `ß`, `æ`, `œ`, `ı`, …). `tracks_fts`
  is written in the same fold and a typed word folded through it. Not `enriched::folded_title`,
  which keeps marks (against a MusicBrainz title a mark is evidence); `enriched::stripped_title` =
  both, the enrichment's fallback. **A combining mark is part of the word it sits in**
  (`resonate_core::is_lettered`): a typed word is folded whole before it is cut, and a held name is
  cut into highlight runs by the same test; `store::words_of` still cuts at a kept mark. **The
  billed spelling is the one with the marks** (`marks_in`, applied on cache hit as on the row), so
  scan order does not decide the display name. `resonate missing --artist` and the window's
  type-ahead weigh a typed name through the fold.
- **A catalog keyed before the fold is folded back when opened.** `store::reconcile_artists` runs
  once in `Library::build`, after `schema::lay_out`: groups artists by `folded_letters` of the name,
  leaves a group alone where its one row is already keyed by its own fold; otherwise keeps one row
  (with an `mbid`, then lowest id); the rest hand over tracks, albums, genres, links, kept releases,
  credits via `store::take_over_artist` (`UPDATE OR IGNORE` except tracks and albums;
  `dismissed_releases` among them, else a dismissal cascades away; the survivor also takes the
  favourite, portrait and tagged sort name it lacks) and are deleted; the survivor is rekeyed,
  renamed to the most marked spelling, and its tracks indexed again. An invariant kept, not a
  migration (a second run is a no-op).
  - **A change to the fold is a schema step asking for the index to be folded again**: it creates
    `index_refold_wanted`; `store::refold_the_index` (in `Library::build` after the reconcile)
    rewrites every `tracks_fts` row whose title, artist or album no longer reads as its fold, then
    drops the table. A release's `folded` haystack, a kept release's and a dismissal are rewritten
    when the album is next landed.
- **An album is whatever a grouping key names; several may name one.** `album_keys` (key primary, an
  album holding any number) makes a grouping a *name* for an album. `store::album` reads it rather
  than upserting on a column: a computed key an album holds fills that album, a key nothing holds
  makes a new one.
- **One song held more than once is listed once, as the best copy.** `alternatives.rs` runs at the
  end of every scan that owes it: groups rows by fold of album title, album owner else track artist,
  disc, track number, title; within a group gathers rows with lengths within
  `THE_SAME_LENGTH_WITHIN` (2 s) of the run's shortest, or both lengthless. An album is named both
  ways a row can be billed (release title, tag title) and rows sharing either are one song
  (`one_song_apiece`, union-find). A row with no title or artist names nothing. Best = lossless over
  lossy, then wider word, higher rate, higher bitrate, then row held first; every other copy names
  it in `tracks.alternative_of` whatever its format. Copies are kept whole (plays, playlists,
  files); only listings step past them: `scoped` filters every track listing and saved query on
  `+tracks.alternative_of IS NULL` (`THE_BEST_COPY`; the unary plus keeps the term off the index so
  order indexes still lead), album and artist counts count the best copy alone
  (`HOLDS_A_BEST_COPY`), and an album whose every track is another's alternative leaves the albums
  pane. `Track::alternatives` = copies a row stands for (`+N`); `Library::alternatives_of` = what
  the menu offers to play instead. A best copy that goes puts `ON DELETE SET NULL` on rows under it;
  the end-of-scan pass and every other removal (`remove_root`, `forget_delivered`, `delete_tracks`:
  each runs `store::sweep_orphans` then `alternatives::settle_if_owed`) crown the next.
  `remove_root` matches a root whose folder has gone by its lexical spelling (`sheet::settled`).
- **Music filters narrow listings, keep the catalog whole.** `MusicFilters` = supported extension
  selection + `MinimumLength` bounded to ten minutes. `Library::filter_music` sets it for this
  opened catalog (the binary fills it from config before any listing) and drops the suggestion
  cache. Track listings and measurements apply it before ordering and paging; albums, artists, their
  counts and search follow. Extension is matched against the file suffix, case-insensitively; a cue
  row has its backing file's extension and its own duration. Zero = no duration condition (unknown
  lengths kept); positive requires a known length at least as long.
- **A hidden track is kept and only stepped past.** `tracks.hidden`; `Library::hide_track` sets it
  either way, answers whether it moved. Row, file, plays, playlists, favourite stay; a scan never
  touches the column. `scoped` adds `tracks.hidden = 0` beside the best-copy term; album and artist
  counts and `holds_a_best_copy` weigh it alike, and the albums and artists panes' search
  (`narrowed_onto`) matches shown best copies alone, by the same terms. The counts are built per
  query from the music filters (`album_columns`, `artist_columns`, `a_shown_copy`); the count sorts
  order by their `shown_tracks` and `shown_albums` result columns. `is:hidden` is a `Shape`; a search *insisting* on
  it (a clause of that one alternative, not denied) lifts the visibility term
  (`Clause::insists_on`); `-is:hidden` or an alternative beside it lifts nothing.
- **A track names its album every way it can and joins the album the first name finds.**
  `grouping_keys` answers a *run* of keys in precedence order. A MusicBrainz release id stands
  alone. Otherwise: an `ALBUMARTIST` the tagger wrote, on anything not flagged a compilation = one
  name; the track's folder (`TrackRecord::sleeve`) another; `album_key(title, owner)` the fallback.
  A track joins the album the first of its names finds and lends it the rest, so `ALBUMARTIST`
  gathers discs the folders keep apart and a folder gathers an album its files bill to different
  owners. Every key but a release's carries the title, so nothing gathers two albums not called the
  same. Two of a track's names finding *different* albums: first wins, no merge. Tracks of one album
  disagreeing on album artist: the album keeps none; `Album::artist_count` lets a pane draw *Various
  artists*.
- **A sleeve is a folder made to hold an album, not simply the parent.** `scan::sleeve` answers
  `None` for a track directly in a root (a dumping ground); the third tier falls back to the track
  artist. A folder named `CD2`, `Disc 3`, `disk-01` is one disc of a set and answers with its
  parent.
  - **A record filed loose in a root is gathered once the scan has written it.**
    `loose::gather_the_loose` runs after the prune on every walked root where the scan wrote a row
    (weighing only albums titled as one the scan wrote into, `Weighed::TitlesWrittenAt`, unless it
    removed one, `Weighed::EveryAlbum`): albums whose tracks all sit in that root and whose keys are
    all fallback tier, grouped by lowercased title, gather via `enriched::gather` only where
    `one_record` says the numbering makes one (every track numbered, no disc and number taken twice,
    years and declared `TRACKTOTAL`s agreeing, no more tracks than a declared total). Two *Greatest
    Hits* each numbered from one stay apart.
- **Disc words.** `SPELLINGS` (`cd`, `disc`, `disk`, `disque`, `disco`, `dysk`, `platte`, `schijf`,
  `skiva`, `диск`): shortest remainder wins (`disc` prefixes `disco`); `Discovery` stays an album
  (no separator). `scan::disc_in_folder` also reads numbers in words (`Disc One`, `Second Disc`, to
  99; word forms need a separator, digits don't) and, via `numerals.rs`, French, Spanish, Italian,
  German, Dutch and Portuguese cardinals and ordinals composed as that language writes them
  (`ELSEWHERE`, built once; `numerals::plainly` folds through `folded_letters`); languages are not
  told apart. `{disc}` in an organise layout reads such sets via `organise::disc_of`. A set scanned
  as `Second Disc` is keyed by that folder until rescanned.

- **`album_keys.key` format**: changing it means a rescan; `ORPHANS` removes albums nothing points
  at. Every key is NFC-composed (`store::composed`) after lowercasing; the step bringing that in
  leaves `album_keys_composed_wanted` and `store::compose_the_album_keys` (in `Library::build`)
  rewrites every stored key once.
- **Albums proving to share a release are gathered; both keys name the survivor.** A release id the
  *pass* finds arrives after grouping, so a rip split over two folders can hold one release in two
  albums. `gather_under` (in `land_release`, before wants are read) finds other albums with that
  `mbid` or named by `release_key`; `gather` moves their tracks, release rows, *keys*, refused
  releases to the survivor, fills what it lacks (cover, year, declared count, sort name, favourite),
  keeps the owner only where the two agree, deletes the other. Keys move, not rewrite, so the next
  scan finds each folder's old key in `album_keys` naming the survivor. Nothing joins on a key
  (`tracks.album_id` is the only membership).
- **An album takes the cover and year any track carries, not the first's.** `Cache::covered` holds
  albums holding a picture, so `store::owe_a_cover` is asked per track until one lands. A file's
  picture (`cover_source = 0` or `cover_path`) is never replaced by the archive's
  (`CoverSource::Archive`, 1), which is replaced by a file's. **A thumbnail isn't deliberate**:
  where `store::betters` finds the archive picture's shorter side at least twice a file picture's
  whose shorter side is under `A_THUMBNAIL_BELOW` (300 px), the archive's stays. A cache hit whose
  album has no year runs `fill_year`. `store::year` reads separator-less dates (`19750601`).
- **A file's picture is learnt on the open its tags came off; the picture is read after the commit,
  outside the write lock.** `probe_scanned` returns `Scanned { info, carries_a_picture, packets }`.
  Inside the batch's transaction `store::owe_a_cover` reads nothing: it notes `(album, path)` in
  `Cache::owed_covers`; once the batch commits, `OwedCovers::read` opens the owed files with no lock
  held and `store::land_covers` writes them in a second short transaction, weighing the archive's
  picture and the album's standing again.
- **A play is a count, a date and a row.** `Library::track_played` writes in one transaction
  `tracks.plays` stepped, `tracks.played` set, one `listens (track_id, at)` row. `listens` has
  `tracks(id) ON DELETE CASCADE` and `configure` sets `PRAGMA foreign_keys = ON`, so forgetting a
  root takes its plays (else a reused `tracks.id` re-attributes an orphan). History is what
  `plays:@` narrows on; count is what a pane draws. `SortOrder::PlaysThisMonth` (`sort` code 9)
  ranks by correlated `count(*)` of listens in the last thirty days, `tracks.plays` tie-break.
  Playlist twin: `playlist_plays`, a row per play a playlist was loaded for; no cascade from the
  playlist row (`undo.rs` re-creates a playlist); the history span ages it. SQLite reuses a
  discarded playlist's id, so `playlist::created` clears `playlist_plays` under the new id.
- **Moved files are followed, not forgotten and refound.** A hand rename/move (not `organise`) =
  gone-file row + unnamed file. `moves::follow_the_moved` (before the prune) pairs a whole-file row
  (only row at its path, file missing) with a row this scan added alike in `file_size`, `duration`,
  `codec`, `tagged_title`, `tagged_artist`; among several alike, `moves::told_apart` scores paths by
  names shared from the end and pairs only where each is the other's one best, sharing at least the
  file name. **Leftovers: by sound.** `pairs_by_sound` pairs rows sharing a `Sound` (exact decoded
  frames, codec, rate, channels), each the *only* row on its side with it, agreeing on tagged title,
  tagged artist or file name. **No name agrees: the packets decide.** The scan digests the first
  `PACKETS_DIGESTED` (48) coded packets (FNV-1a) into `tracks.packets` (taggers rewrite what
  surrounds the audio, never a packet). **Moved before the scan could read it: the print decides.**
  A gone row whose study kept a Chromaprint pairs with the one new row of its sound where
  `resonate_analysis::print` writes the same print; decoding is only for rows a unique sound leaves
  unnamed, never in a transaction (`moves::to_be_heard` notes the paths on a read connection, the
  scan decodes lock-free, `follow_the_moved` pairs in the write). A pair goes through
  `organise::files_moved` (the old row takes the new path and keeps its id, so plays, listens,
  favourite, enrichment, vault object, playlist rows, kept lyric and queue row follow). **The old
  row is first refreshed from the arrived one** (`moves::refresh_from_the_arrival`, a one-row pair
  only; the upsert's own retag and unchanged rules over the file's columns: size, mtime, tags, sort
  names, the names and ids an answered row keeps unless the file was retagged), and indexed again
  after `settle_the_album` (`store::reindex_the_track`), so a file retagged as it moved is found by
  its new names (`a_file_retagged_past_every_name_as_it_moved_is_followed_by_the_packets_its_scan_digested`);
  `settle_the_album` gathers into the old album via `enriched::gather` when the new folder's album
  holds nothing else and every moved row came from one album. **A cue track whose `INDEX 01` moved
  is the row it was** (`store::follow_a_cut_that_moved`, carrying its dependents, `CUT_FOLLOWERS`);
  **a file a sheet cuts is followed whole** (`cuts_moved`). `ScanStats::moved` counts rows followed;
  `added` leaves them out.
- **The count is the catalog's; a rescan leaves it.** `tracks.plays`, `tracks.played` are absent
  from the upsert's `DO UPDATE SET`. `Library::track_played` answers `None` where a non-local
  location has no row, **but the play is kept against the path.** `unheld_listens` holds path, span
  start, moment, time heard; `history::credit_the_unheld` runs after the prune of every scan a row
  arrived in: a play whose path and start now name a row becomes a `listens` row stamped when heard,
  the row's `plays`, `played` follow. `Counted` carries a `Listen` (`Held` or `Unheld`);
  `Library::listened` spends either; the history span ages unheld plays too. `plays:`, `played:`
  narrow on the two columns; `SortOrder::Plays`, `Played` order on them, so *Top 25 most played* is
  a saved query.

- **What another device sent is written without an undo step and never told onward.**
  `synced.rs` serves the sync seam (`sync.md`): list playlists read as recordings (an unscanned row
  reads as none), written whole with `modified` moved past any undo step and the steps dropped
  (`undo::overtaken`, also for steps whose name the arrival now holds), a taken name given the first
  free `NAME (N)`; favourites and recordings resolved by MBID, else folded artist key, `title_words`
  and folded album with lengths within 2 s, the first best copy; a remote listen is a `listens` row
  with `elsewhere = 1` (counted in plays and statistics, passed over by `THE_LISTENS_AFTER` and by
  `listens_to_sync`, which also stops at a listen not yet settled). `Library::synced_revision`
  counts these writes, which `data_version` cannot see; `catalog_revision` adds every other writer.

## Enrichment

- **A reference's answer lands beside what the scan read, in columns the scan never rewrites.**
  Artist, album and track rows carry `mbid`, the two clocks `asked`/`answered`, `asks`, and their
  profile or release columns; beside them `release_tracks`, `release_media`, `artist_genres`, link
  tables `artist_links`, `album_links`, `release_track_links` (codes via `store::relation_code`,
  `service_code`; a later build's code is `Error::UnknownLinkCode`), `artist_releases`, `wants`,
  `lyrics_kept`. Scan upserts touch only the ids, the two `tagged_` columns and what tags *declare*
  of a release (`barcode`, `catalog_number`, `label`, `albums.tagged_tracks` from `TRACKTOTAL`), all
  `coalesce`d (`fill_artist_mbid`, `store::Declaration`, `fill_declared`) so a file dropping its
  barcode leaves the held one. `tracks.mbid`, `artist_mbid`, `release_track_mbid`, `isrc` come from
  tags where the file names one and are kept where it names none unless retagged, since lookups also
  *find* them. An *answered* row with unchanged file (size, mtime, sheet mtime, span) keeps all four
  plus `track_number`, `disc_number` whatever the file says (the one write replacing a file's value
  is a listener taking the name its audio was heard as, `analysis.md`). A rescan leaves every other
  enrichment column where it stood; the retagging rule below is the only scan write of
  `tracks.answered`. `asking_albums` hands `catalog_number`, `tagged_tracks` and the owner's
  `artists.mbid` to `AlbumToAsk` beside the barcode.
  (`a_rescan_leaves_every_enrichment_column_where_it_stood`)
- **`tagged_title`, `tagged_artist` are what the *file* was read as: how a rescan tells retagging
  from identification.** Enrichment writes `tracks.title`, `tracks.artist` like the scan. The upsert
  keeps an answered row's `title`, `artist`, `artist_id` where both `tagged_` columns return
  unchanged, takes the file's where either moved and sets `answered` to `NULL` so the row is asked
  again; never-answered rows take the file's. They hold what `scan::name_from_stem` left in the
  `TagSet`: `tagged_title IS NULL` means neither tags nor file name said anything. `stem::read`
  takes a leading number as track only where punctuation parts it (`03.`, `03 -`, `7)`, `003_`) or
  it is padded; such a number is a guess, marked `tracks.numbered_by_its_stem`: a lookup's seat
  number replaces it (`land_recording`) and `TRACKS_TO_TAG` reads it as no number; an unpadded
  space-parted number is the title's (*99 Luftballons*). **A name the file's *name* gave is not a
  tag; renaming is not retagging.** `tracks.named_by_its_stem` says the scan took title or artist
  off the stem; `store::RETAGGED` is the one reading of *the file said something else* every upsert
  column weighs. `store::index_row` is shared by scan and `land_recording`, so a corrected title
  reaches `tracks_fts` at once; the upsert answers the names it *left* on the row.
  (`a_rescan_keeps_an_answered_tracks_names_unless_the_file_itself_was_retagged`)
- **The seam is `Reference`; `Library::enrich` walks it.** It speaks `Mbid` (dashed lowercase, else
  `resonate_core::Error::NotAnMbid`) and `Isrc` (twelve characters, upper-cased, dashes stripped,
  else `NotAnIsrc`), both in `resonate-core`. `reference.rs` carries the vocabulary (`Release`,
  `Medium`, `ReleaseTrack`, `Credit`, `Wording` (`Phrase`/`Words`), the `*Asked`/`*Match` pairs,
  `Recording`, `ReleaseGroup`, `ArtistProfile`, `Discography`, `StreamAsked`, `LyricsAsked`,
  `LyricText`) and `LookupOp`, every thing a service can be asked. `Reference` answers `Option`s and
  `Vec`s, nothing about how reached; `resonate-online`'s `Online` is the one implementation and only
  the binary reaches it behind `online`. `enrich.rs` is the pass: a `resonate-enrich` thread behind
  an `EnrichHandle`; `EnrichProgress` counts what landed and answers `is_cancelled` between
  requests; `EnrichSummary` carries stats, whether cancelled, `stopped_by`. `EnrichOptions`:
  `refresh`, `at_most` (caps albums, tracks, artists each), `sought`, `studies`, `lyrics`.
  Rematch-only albums go first; the rest is one queue of `Ask`s walked in order, albums first
  because a landing album retires every track under it. `tests/library.rs` drives it via a `Fake`
  that can fault on a chosen call; the online crate proves its mapping over captured fixtures,
  reaching services only under `RESONATE_ONLINE_TESTS`.
- **A picture is fetched beside the pass, never in it.** `Pictures`: `PICTURE_READERS` (4) threads
  reading a bounded channel (`PICTURES_ASKED`, 64) of `Picture`s (cover, release group's cover,
  unheld release's cover, portrait); `Pass::want` is the whole of asking; `Pictures::rest` joins
  readers before the summary. The archive and Wikimedia Commons are not MusicBrainz and
  `Client::pace` keeps a slot per host, so a picture costs the pass only the handover. An
  unfetchable picture is a warning plus a `refused` count; with no reader thread, `want` fetches on
  the spot.
- **What a listener just reached for is asked next; a seek is spent once.** `Sought` is a
  `Mutex<Vec<Seek>>` shared with whoever started the pass (`Sought::album`, `Sought::artist`);
  `Sought::taken` drains it at the top of every queue turn and `Pass::lift` rotates the named `Ask`
  to the front of what is *left*. A seek naming nothing the queue holds is *read back*
  (`Library::album_if_due`, `artist_is_due`) and inserted at the front where still due. What the
  pass asked *this run* is refused by the `spent` set. `LibraryModel` holds one `Sought` for the
  window's run. `LibraryModel::select` seeks the album and its owner; `ask_about_what_is_drawn`
  seeks every album the drawn tracks belong to, then the artist (last pushed is first asked).
- **An artist's row is read when asked, not when the queue was built.** `Ask::Artist` carries an id,
  `Library::artist_to_ask` reads the row, so the `artists.mbid` `Pass::credits` wrote while a
  *later* album landed is what `profile_of` sees. **An artist the pass brings into the catalog is
  asked before it ends**: `Pass::run` re-reads `artists_to_ask` once the queue is walked until
  nothing new is due; a capped run (`--albums`) does not.
- **`asked`, `answered` are the two clocks, `asks`, `refusals` the columns they are read with,
  `Waits` the three waits in one value.** `due_again` writes the clause per table alias. Due: never
  asked; asked, unanswered, longer ago than the wait its `asks` earned; answered over
  `REFRESH_AFTER` (30 days) ago and not asked in vain since; or anything under `refresh`. A landing
  zeroes `asks`, `refusals`, so a row carrying either since its answer is due only once
  `waited_its_turn`. `Waits` carries `retry_after`, `refused_again_after`, `refresh_after`; `WAITS`
  is this build's. Failures, via `Pass::heard`: `Error::Unreachable` ends the pass and stamps
  nothing (`EnrichSummary::stopped_by` names the `LookupOp`); `Refused`, `Unreadable` count in
  `refusals`, stamp `asked`, carry on **until `REFUSALS_THAT_END_A_PASS` (10) in a row**
  (`Pass::refused_in_a_row`; any answer resets it; the tenth is `Error::RefusedInARow`, ending the
  pass, the row being asked unstamped). `TooLarge` is a refusal not counting toward the streak. An
  album, track or artist gathered or removed while asked about answers
  `UnknownAlbum`/`UnknownTrack`/`UnknownArtist`, which `passed_over_if_gone` logs and passes over.
  Any other error ending the pass cancels its progress and rests the picture, study and lyric
  workers first.
- **Fruitless asks back off; an answer resets.** `asks` counts stampings without answer
  (`Fruitless::Missed`); `due_again` waits `RETRY_AFTER` (24 h) × 2^(asks−1), capped at
  2^`WAITS_DOUBLE_AT_MOST` (5) = 32 days. `refusals` counts consecutive `Fruitless::Refused` asks:
  refused rows wait `REFUSED_AGAIN_AFTER` (1 h) × 2^(refusals−1) (`waited_its_turn` reads whichever
  count the last ask left). Landings write `asks = 0`; `store::apply` zeroes `asks`/`refusals` and
  nulls `answered` where a track's `tagged_title`/`tagged_artist` changed. `resonate enrich
  --refresh` reaches every row.
- **A running pass leaves a mark; next launch carries on.** The `enrichment` table holds one row
  (starting `refresh`) while a pass runs: `note_began` writes it, `note_finished` removes it when
  the queue ran out or the listener stopped. A pass ended by the reference (`stopped_by` `Some`) or
  never ended leaves it, read by `Library::unfinished_enrichment`; `LibraryModel::new` asks at start
  and carries on if the network is reachable. Headless, the binary's `carrying_on` does the same on
  both ways in (`resonate enrich`; scan handing over).
- **Release by tag or strict match, artist by tag or exact match; near miss writes nothing.**
  `Identified` = `Found` | `Group` | `Nothing`. `albums.mbid` asked directly, `None` → search,
  `Refused` ends the ask; a tagged `albums.release_group` held nowhere → `find_group` likewise.
  `top_release` takes `top_of` weighing `weighed_release` = `(agreed_owner, count_fits, year_agrees,
  score)`, lands only on `matches_strictly`: `agreed_owner` (score ≥ `STRICT_SCORE` 95 + the credit
  agrees with the owner by tagged id or name) and `count_fits` (`track_count` = `declared_count` =
  `tagged_tracks`, else rows held). Owner and score agree, count not, `group` present →
  `Identified::Group`; else `Nothing`. **No owner: weighed on performers**: `AlbumToAsk::performers`
  = distinct artists its tracks bill (≤ `PERFORMERS_WEIGHED_AT_MOST` = 32); `owned_by` agrees a hit
  credited to Various Artists or via `same_credit` to one of them; none billed: score and count
  alone. Artist: `Pass::profile_of` asks a tagged `artists.mbid` directly, → `find_artist` where the
  reference holds nothing; a hit must `matches_exactly`: `EXACT_SCORE` (100) + same name. A
  shortfall logs a debug record; the album/artist is stamped `asked` alone.
- **The strict rules are exported, so the phones weigh with them, not with a copy.** `Identified`,
  `Fit`, `top_release`, `top_group`, `closest_release`, `the_strict_recording` (the strict pick
  `Pass::take_match` makes), `best_recording`, `needs_its_releases_told`, `best_release`,
  `recording_asked` (the titled-enough test and the ask `Pass::by_search` sends), `release_asked`,
  `group_asked`, `REFUSALS_THAT_END_A_PASS`, `due_again` with `due_parameters` (the clause over
  `?1`..`?5` and the values it binds), `COVERS_ASKED_AGAIN_AFTER`, `PORTRAITS_ASKED_AGAIN_AFTER`,
  `A_THUMBNAIL_BELOW` and `betters` are public. `Pass` calls the same functions, so a rule changed
  here changes `resonate-mobile`'s pass too (`android.md`).
- **A rejected match is taken away, never landed again.** `Library::forget_the_match` records the
  album's release (group where landed as group alone) in `refused_releases`; clears the landing's
  writes (ids, release title, date, country, kind, disambiguation, `front_cover`, archive cover);
  removes release rows, media, links; `asked` → never. **Tracks revert**: each regains the
  title/artist its file gave, loses release title and any release-track id naming a removed row,
  returns to never asked, is re-indexed. A recording id or ISRC a release row stamped goes too (else
  the next lookup takes it as tagged); the row is marked `probe_again`. Label, catalogue number,
  barcode stay. `take_release`/`take_group` ask `Library::refuses` first and pass a refused id as
  `nothing_landed`; only the release is refused, not its group; gathering carries refusals to the
  survivor. **Choosing the pressing**: `Library::take_pressing` asks for the release, lifts any
  refusal of it for that album (`enriched::forgive`), lands via `enriched::land_release`, re-pairs.
- **Six ways to agree; `Spelling`'s derived `Ord` is the ranking.** `same_name`: `Marked`
  (`folded_title`s agree), `Stripped` (only `stripped_title`s), `Dequalified` (agree once
  `dequalified` strips a version qualifier from each end). `names_it` weighs a MusicBrainz artist's
  aliases alike (`as_an_alias` → `AliasMarked`/`AliasStripped`). `same_credit` answers `ById` above
  all where a credited artist's `mbid` is the tagged one. Rank `Dequalified < AliasStripped <
  AliasMarked < Stripped < Marked < ById`. `top_of` weighs `(Option<Spelling>, score)`: agreement
  beats none, better spelling beats worse whatever either scored, score decides between equals. A
  credit is weighed twice by `same_credit` (names joined as MusicBrainz bills them, and each artist
  singly), better wins. Only artist routes read aliases; `owned_by` and `matches_a_recording` use
  `same_credit`.
- **A version qualifier is a closed list; anything else names a different recording.**
  `VERSION_QUALIFIERS` (`resonate_core::titles`, shared with the providers' name match: *Album
  Version*, *Radio Edit*, *Explicit*, *Clean*, *Remastered*, *Bonus Track*, …) are what
  `dequalified` strips from a bracketed title's end; `a_dated_qualifier` adds a remaster with a
  four-digit year (*(Remastered 2011)* goes; *(Remastered by Ada)* stays). `(Live)`, `(Remix)`,
  `(feat. …)`, `(Acoustic)`, `(Demo)`, anything unlisted stay. Strips end-inwards, `( )` and `[ ]`
  alike, never leaving nothing.
- **An album no pressing matches lands as its release group.** `Pass::album` reads four routes in
  order: `albums.mbid`, `find_release`, `albums.release_group`, `find_group`. `matches_as_a_group`
  weighs score and folded owner, **no track count**; it is also reached from the release search when
  the top hit is strict but for count and names a group. `settle_group` → `closest_release` answers
  a pressing and a `Fit`: `Exact` (`track_count` = `declared_count`), else `Wider` (smallest
  pressing with more tracks than the album), else `Narrower` (widest with no more); earliest dated
  among equals; `land_release` then runs on it. Rows the rip lacks are release rows with no
  `track_id`, so `Album::missing` counts them and the Missing pane lists them. Only where no group
  pressing declares a count does `take_group` run `land_release_group`, deliberately thin
  (`albums.release_group`, `kind`, `date`, `year`, `disambiguation` coalesced; no `albums.mbid`, no
  `release_tracks`). Half an answer, so asking continues: `album_due` = `due_again` or
  (`HOLDS_A_GROUP_ALONE` and `waited_its_turn`). The archive is asked for a cover only in the pass
  that landed the release or its group, only where the album held none or a thumbnail
  (`wants_a_cover`).
- **Phrase first, then words where the phrase *landed* nothing.** `found_either_way` serves
  `find_release`, `find_group`, `Route::Search`: `Wording::Phrase` (fielded query), then
  `Wording::Words` (loose dismax) when weighing the first answer took nothing. A refusal is no
  answer: `found_either_way` returns a `Heard`, so a refused release search ends the album's ladder
  and stamps the refusal; a track's refused search still goes on to its fingerprint (another host).
- **Billed by the release where one landed, by tags until then.** `albums.title` = what the scan
  read, never rewritten by a landing (half of `store::album_key` and `sleeve_key`).
  `albums.release_title` = the reference's answer, written by `land_release` alone; the
  `album_title!` macro in `db.rs`, `coalesce(a.release_title, a.title)`, is the billed title
  listings, sorts, wants and `organise` destinations read.
- **Landing a release is one transaction, pairing its rows a second.** `land_release` writes release
  columns, fills `year` only where the scan left none, stamps `answered`, deletes and reinserts
  `release_tracks`, `release_media`, `album_links`, writes each row's `release_track_links`. Wants
  under the album are read first (`wants_under`) and put back after. **A release row keeps every
  code of its recording** in `release_track_isrcs` (`WANTS_UNHELD` reads them into `Want::isrcs`,
  `providers.md`). `Carried` = what a want is put back *by*, most exact first: `Track`, `Recording`,
  `Seat` (`(disc, position)` **where the title's words still agree**); `want_again` lands exact ones
  first; a want whose track the release no longer holds is dropped. `rematch_release_tracks` pairs
  release rows with catalog rows in five passes, each taking only rows earlier ones left: recording
  mbid vs `tracks.mbid`; track mbid vs `tracks.release_track_mbid`; `folded_title` at its place;
  `folded_title` anywhere; place alone (missing disc = 1 only on a one-medium release). Title
  outranks place (else a pressing in another order stamped each file with its neighbour's ids). It
  writes `release_tracks.track_id` where pairing *moved* and answers how many moved
  (`EnrichStats::matched`); for every paired row it fills `tracks.mbid`, `release_track_mbid`,
  `isrc` by `coalesce`, guarded so re-pairing writes no `tracks` row. `AlbumToAsk::rematch_only`
  makes an album holding release rows rematch whether or not due, but `albums_to_ask` offers it only
  where `HOLDS_AN_UNPAIRED_ROW` or `HOLDS_AN_UNPAIRED_TRACK`; a pass rematches
  `ALBUMS_REMATCHED_PER_TRANSACTION` (64) at a time via `Library::rematch_each`.
- **A track is asked about in its own right.** A file with no `ALBUM` tag has `album_id = NULL` and
  was enriched by nothing. `Ask::Track` joins the queue between albums and artists: `tracks_to_ask`
  = `WHERE NOT IS_PAIRED AND due_again("t")` (index `release_tracks_by_track`);
  `Library::track_to_ask` re-checks `NOT IS_PAIRED` when asked, so a track an earlier album landing
  paired retires silently.
- **Four routes, most exact first; the first answer ends the track.** `Route::ALL` = `Isrc`,
  `Recording`, `Search`, `Fingerprint`; `Pass::track` stamps `asked` alone if none answers.
  **ISRC**: `recordings_of_isrc` answers a *list*; `best_recording` separates takes by length and
  answers with a `Certainty`: a lone answer is `Exactly` where lengths agree or either is
  unmeasured, `Nearly` where length disagrees but title agrees by some `Spelling`; several → those
  within `RECORDING_MAY_DIFFER_BY` (5 s), then closest, `Exactly`; a lengthless file takes none of
  several (nothing separates the takes). **Recording**: a `MUSICBRAINZ_TRACKID` is asked directly. **Search**:
  `find_recording` with title, length, `asked_with` (track's artist + mbid, else album owner's), and
  the album's billed title as `release` only where neither is known; weighed by
  `matches_a_recording`: `STRICT_SCORE`, title agreeing by some `Spelling`, credit agreeing via
  `same_credit`, length within 5 s. Refused before the request where `tagged_title` is missing
  **unless the file vouches for the rest**: `named_enough_by_its_file` lets a plain stem be asked
  when tags name an artist, length is measured and the stem `names_something` (≥
  `LETTERS_A_FILE_NAME_HOLDS_AT_LEAST` = 3 letters, not in `PLACEHOLDER_NAMES`: *Track 07* asks
  nothing); also refused where no artist, owner or album is known. A file naming no artist under an
  album naming one is asked as that artist's; under an unowned album, by the album's name.
- **A search answer says where the recording sits, so nothing is asked twice.** `RecordingMatch`
  carries credit and `RecordingRelease`s beside score, title, length; `into_recording` is what
  `take_match` lands, via `told_where_it_sits`: `/recording` is looked up only where the answer
  named no release, or (ISRC case) several with no kind between them (`/isrc` refuses
  `release-groups`, so `needs_its_releases_told` asks the recording whole wherever `meant_release`
  would otherwise choose by date alone). The search index carries a recording's releases and
  `isrcs`, so `coalesce(isrc, ?8)` fills the column: one request per track, not two.
- **What a lookup may overwrite is `Certainty`, a type, not a rule each caller remembers.**
  `Exactly`: a recording id the *file itself* named, or a named ISRC whose take is as long as the
  file. `Nearly`: text search, fingerprint, an ISRC whose
  only take is another length under the same title. `land_recording` reads it in one
  `CASE` per column: a name is filled wherever the file named none; a name the file *did* carry is
  corrected only under `Exactly`. `artist_id` follows the same `CASE` as the billed name. Everything
  else fills, never replaces (`track_number`, `disc_number`, `mbid`, `isrc`, all `coalesce`d),
  except `release_title` (the route's chosen release) and `artist_id` (repointed via
  `store::artist_named_in`). `asked`/`answered` are stamped in the same statement; its `RETURNING`
  is what `EnrichStats::named` counts and `store::index_row` rewrites `tracks_fts` from.
- **A track naming its release lands the album its own pass never reached.** `best_release` picks
  the release the row is filed under: album's `albums.mbid`, then a release whose `folded_title` is
  the album's, then `elsewhere::meant_release`. Album never answered (`TrackToAsk::album_answered`
  false) → `take_recording` goes on to `land_release`; already answered → the track stops at its own
  row.
- **The listener can place a held track on a release over the rule's choice.**
  `Library::recording_of` reads the recording a track is identified as and asks the reference for it
  whole; `in_the_order_worth_offering` lists its releases as `meant_release` weighs them.
  `Library::place_on` lands the recording with the chosen release under `Certainty::Nearly`; a track
  under an album → the album then gets that release via `take_pressing`. A release the recording is
  not on → `false`, nothing written. The track menu offers *Place on a release…* wherever the
  reference is reachable.
- **`fingerprint.rs` is the seam a recogniser fills.** `Fingerprints` answers a `Printed`
  (`Nothing`, or `Recognised` with `RecordingMatch`es) for a `Sounded` (location, span, length, the
  title and artist the *file* said, the study's `Chromaprint`, so no printer decodes).
  `resonate-online`'s `AcoustId` is registered where an `acoustid-key` is set (`analysis.md`).
  `NoFingerprints` is the stub (`unprinted`); `Fingerprinters::none()` holds it alone, `and`
  registers one per name, `has_a_source` asks whether anything real is behind it; `recognise` asks
  each printer in turn for the first non-empty answer and returns a `Recognition` saying whether any
  refused. **A fingerprint is weighed on its score alone**: `recognised` takes the top match at
  `STRICT_SCORE` and asks nothing of title or artist.
- **Open settles credits/sweeps orphans only if owed.** `settle_owed`: one row, set by triggers on
  track/artist added, renamed or removed and album removed; cleared by every `sweep_orphans`.
  `build` runs `settle_the_credits_if_owed`, so a reader-only process writes nothing and takes no
  write lock. A scan asks the same and **regroups copies only if owed, and only the songs owed**:
  triggers note in `regroup_owed_tracks` a track added, a write to a column `EVERY_COPY`
  groups/ranks by (with the copies held under or beside it), every track of an album retitled,
  rebilled or re-owned, of an artist renamed, and the copies under a deleted track. A song's copies
  share `tracks.title_words` (index `tracks_by_title_words`), so `alternatives::settle_if_owed`
  reads only copies sharing an owed track's, unless the `regroup_owed` flag is set, more than
  `REGROUPED_BY_TITLE_AT_MOST` (4 096) tracks are owed or one is not yet worded. A trigger's insert
  names no conflict clause (`NOT IN` the owed set instead): SQLite overrides a trigger's `OR IGNORE`
  with the outer statement's policy. `restate_the_statistics` and the loose gathering run only if
  the scan wrote/removed a row; `credit_the_unheld` only if one arrived.
- **A collaboration is listed under every artist it credits, never as an artist of its own.**
  `track_credits` holds each member; `credits::credit_the_members` rebuilds it from `tracks.artist`
  on every orphan sweep, finding tracks via `tracks_by_credit` (plain index on `tracks(artist)`; the
  `COLLATE NOCASE` one cannot serve a binary `=`). `members_of` splits on `JOINS` (`&`, `and`,
  comma, semicolon, `/`, `+`, `x`, `×`, `with`, `feat.`, `ft.`, `featuring`, `vs.`,
  space-delimited), **only if every part names a held artist** (*Simon & Garfunkel* stays one). A
  split track's `artist_id` = its first member unless it already names one; text stays the file's
  credit; the whole-credit row is swept once nothing names it. Credits are read beside `artist_id`
  in the artist scope, `artist_albums`, `artist_tracks`, `WHAT_AN_ARTIST_HOLDS`,
  `BY_OR_HOLDING_THE_ARTIST`; `take_over_artist` carries credits across a merge. Members come from a
  landed recording making a row for *every* artist it credits and, where an artist's own lookup
  lands nothing and its name splits, `Pass::bill_the_members` (exact-name rule) and
  `Library::bill_an_artist`. **Names a tag lists apart are artists apart, held or not**: the codec
  joins multi-valued artist tags with `LISTED_APART_BY` (`; `), so such a credit splits whatever the
  catalog holds. `each_value_named` makes a row per value; `Billing::of` bills a listed name to its
  first value, dropping the tag's mbid. `hand_each_list_to_its_lead` runs first in every settle,
  merging an artist row whose name holds `; ` into its lead; a migration step marks a settle owed so
  old catalogs mend on open.
- **A credit names an artist the catalog may hold: identified, not asked.** `Pass::credits` walks
  the `Credit`s of a landed release or group; with an mbid and `Library::artist_named` finding the
  folded name, `write_artist_mbid` fills an empty `artists.mbid`, so the next artist ask goes by id.
  The lookup fold must be the column's (`folded_letters`, not `to_lowercase`).
- **A discography is kept once the profile lands; what the catalog lacks is read off it.**
  `Pass::artist` calls `discography` after `land_artist`: `release_groups_of` asks every release
  group the artist is credited on; `worth_keeping` keeps primary type in `KEPT_KINDS` (`Album`,
  `EP`, `Single`) with secondary types none or `Soundtrack` alone. `land_artist_releases`
  (`Discographed::Afresh`) deletes and reinserts `artist_releases` (`EnrichStats::releases_found`).
  *Unheld* = no album's `release_group` is its mbid (`unheld_by_any_album!`). **A single is held
  wherever its song is**: title kept as `artist_releases.song`, weighed against `tracks.title_words`
  (`store::words_of`, a deterministic SQLite function `schema::configure` registers; temp triggers
  `track_worded`/`track_reworded` keep the column; `title:=` reads it too), looked up by
  `tracks_by_artist_and_words`. **What was not read is said, not logged**:
  `Reference::release_groups_of` answers a `Discography` (releases + how many more the service
  credits than the browse cap read), kept as `artists.releases_unread` and `releases_read_to`. **The
  rest is read on request**: `Library::read_the_rest_of` asks from that offset and lands
  `Discographed::Further`; the artist page's *Read the rest* and `resonate missing --artist <NAME>
  --read-the-rest` read a thousand more; an unknown name is `Error::NoSuchArtist` (exit 1).
  `Library::unheld_releases`, `missing_counted` (a `Missing`: unheld releases plus release rows with
  no `track_id`) and `missing_tracks` (with each `WantId`) feed the artist page and `resonate
  missing`. **A discography that did not arrive is asked again within the hour**: `land_artist`
  already stamped the profile `answered`, so a refused `release_groups_of` stamps
  `Fruitless::Refused`, making the row due after `REFUSED_AGAIN_AFTER`.
- **What the catalog lacks can be dismissed; a dismissal outlives the release it was made against.**
  `Library::dismiss_missing`/`dismiss_release` take a missing row/unheld release off every reader
  (`missing_tracks`, `unheld_releases`, `missing_counted`, `unheld_matching`,
  `ArtistDetail::releases_unheld`) via the shared `held_or_wanted!`/`unheld_by_any_album!`. Own
  tables, because what they name is rewritten by every refresh: `dismissed_missing` (album, disc,
  row position, `folded` title/artist/release title; position keeps one dismissed *Interlude* from
  hiding the disc's other) and `dismissed_releases` (artist + release group MBID); both cascade with
  album/artist. **A want and a dismissal undo each other**: dismissing withdraws the want, `want_in`
  clears a dismissal of its row. **A want asked again is due at once**: `want_in` resets
  `tried`/`misses` of a standing, not-yet-offered want and forgets the offers refused for it
  (`refused_offers`), so every provider is heard again (`providers.md`). `Library::dismissed` counts
  dismissed-and-still-missing; `bring_back_dismissed` empties both tables.
- **What the catalog lacks is narrowed by the words typed; each half answers with what it has.** All
  three readers take `Option<&str>`, so list, counts and sidebar narrow together. A missing track
  belongs to a *held* album: `narrowed_onto("album_id", "a.id")` gives it the whole grammar. An
  unheld release has only its own row: `unheld_holding` writes one `r.folded LIKE ? OR ar.key LIKE
  ?` per lone word, folded via `folded_letters`; only lone words are read.
- **A cover says where it came from, the file's wins, the archive is asked until it has answered.**
  `albums.cover_source`: `CoverSource::File`/`Archive`/`Vault` (`vault.md`). `land_archive_cover`
  writes under `cover_art IS NULL` (and `cover_path IS NULL`): a file's picture is never
  overwritten, except a thumbnail the archive's `betters`. `Pass::album` asks in the pass that
  landed the release, where `has_cover` is false or `Library::covered_by_a_thumbnail` finds the held
  picture under `A_THUMBNAIL_BELOW`; such an archive cover survives a rescan and is what `resonate
  tag` writes over the file's thumbnail. A vault-held cover is not weighed. `albums.cover_asked` is
  stamped once the archive *answered* (picture or none), never where the fetch failed, was refused
  or was cut off; `Pass::look_again_for_covers` asks at every run's end for each album with a
  release/group, no picture, no stamp, skipping those `Pass::covered` says the run asked. An
  answered-lacking cover is not asked within `COVERS_ASKED_AGAIN_AFTER` (30 days) unless the release
  is refreshed or `Library::ask_again_for_covers` clears the stamps (settings *Look for missing
  covers*). **A release MusicBrainz says has no front cover is not asked of the archive**:
  `Release::has_front_cover` (`cover-art-archive.front`) is kept as `albums.front_cover` (`NULL` =
  perhaps); `Pass::cover` asks the group's cover instead where the release names a group, nothing
  where it names none . An album landed as its group asks `group_cover` under the same conditions.
  Portraits, same shape: `land_portrait` writes under `portrait IS NULL`, asked only where the
  profile's links name a picture; a miss is looked for again from the stored `artist_links`
  (`Library::artists_wanting_a_portrait`, `Pass::look_again_for_portraits`) and remembered in
  `artists.portrait_asked` for `PORTRAITS_ASKED_AGAIN_AFTER` (30 days).
- **A link is a relation, a service and a URL, both names read off the reference's own words.**
  `Relation::of_type` matches the exact MusicBrainz type string (`Relation::TYPES`), else
  `Relation::Other`. `Service::of_url` reads only the host of an `http`/`https` URL (other schemes,
  or an authority with a backslash, → `Other`), lowercases it, drops a leading `www.`, matches it or
  a parent domain against `Service::HOSTS`; `x.com` and `twitter.com` both `Twitter`, any host with
  an `amazon` label `AmazonMusic`, else `Service::Other` keeping the URL. All in `resonate-core`'s
  `link.rs`; tables still name the column `provider`; `store::service_code`/`service_of` encode it.
- **A want is a release row the catalog holds no file for; a provider fills it.** `wants`: one row
  per `release_track_id`; `Library::want` refuses an unheld row with `Error::UnknownReleaseTrack`
  and answers the same `WantId` twice for one row; `unwant` drops; `wants` reads all, newest first.
  `Want::identity` makes the `resonate_providers::Identity` a provider gets; `supply.rs` is the
  pass: `Library::poll` walks wants `Want::due_at` says are due (retries and give-up in
  `providers.md`) on a `resonate-poll` thread, asks `Providers::first`, lands the answer.
  `Delivery::File` → `Vault::keep`, `Delivery::Stream` → `Vault::keep_delivered`; either kept writes
  the `vault_objects` row via `note_delivered` (`taken_from` = file's URI or `<provider>:<key>`) and
  `wants.offered` = the vault object's URI. No vault: filed in the music folder where one is set
  (`providers.md`); with neither, a file's own URI is the offer and a stream is dropped. That, a
  vault refusal, a length disagreeing with the release row's, and a landing after another held the
  row each count `unkept` and offer nothing, so `offered` never names what cannot be opened.
  `note_tried` keeps an earlier offer where the new pass found none, and is not called where a
  provider refused, ran late or was away. `forget_delivered` clears the offer and records it in
  `forgotten_deliveries`, which `wants_under` and `want_again` carry to the want's new row.
- **A lyric fetched once is kept, and so is a miss; what is kept only gets better.** `lyrics_kept`
  is keyed `(path, span_start)`; `text` `NULL` = remembered miss; `taken` = last asked. A kept row,
  and a `lyrics_refused` one, goes with its track (`store::ORPHANS`; the
  `lyrics_forget_a_changed_file` trigger when `file_size`, `modified` or `span_frames` moves). A tag
  run touches no audio, so `files_retagged` holds a written file's kept words across the trigger
  (`KeptAcross`); a remembered miss and a refusal are held only where it wrote no title or artist. A
  kept row is a `KeptLyrics` holding `Option<LyricText>`: text, `synced`, `lyricsfile` (kept only
  where it says more than its lines). `LyricDetail`: `Plain` < `Lines` < `Lyricsfile`; `sung::keep`
  never trades a richer set for a plainer one or a miss, stamps `taken` either way, answers whether
  it improved. `KeptLyrics::is_due`: a miss after `MISSED_AGAIN_AFTER` (a week), a set short of a
  Lyricsfile after `BETTERED_AFTER` (a month), a Lyricsfile never.
  `Library::kept_lyrics`/`keep_lyrics` take `MediaLocation` + `Option<FrameSpan>`, refusing a
  non-local location. The search index is written from `text`, so `lyrics:` never reaches a
  Lyricsfile's YAML.
- **The lookup asks for every track's words beside the pass.** `Reference::lyrics` takes
  `LyricsAsked` (read off the row when asked) and answers `Option<LyricText>`.
  `Library::lyrics_to_ask`: every row with no kept row or a due one (all under `refresh`), cut to
  `at_most`; `EnrichOptions::lyrics` (`fetch-lyrics`) turns it off. `Verses` is one thread (LRCLIB
  is paced apart from MusicBrainz). A refusal is counted and the row left unkept, not asked on the
  next pass: `lyrics_refused` holds last-refused time and count (`sung::note_refused`, removed by
  `sung::keep`); `lyrics_to_ask` skips a row refused within `REFUSED_AGAIN_AFTER` doubled per
  refusal; its own table because a `lyrics_kept` row's `NULL` text already means a miss the window
  reads. An unreachable service ends the walk, not the pass. `Lyricists::find` walks every provider
  for the most finely timed answer (`lyrics.md`).

## Writing the catalog back into the files

Write-back is a seam; this build writes only what it reads back. `resonate-codec`: `TagField` (a
writable field), `TagEdit`, `Writing` (edits, `taken` fields, optional front cover, `unpictured`,
`popularity`: one call), `TagSink` (seam over `TagSource`), `FileTags` (lofty writes, symphonia
reads, so a write is weighed against what the rest of the build sees). `FileTags::writes` refuses
formats this build would not read back: it answers for AAC, AIFF, Monkey's Audio, FLAC, MP3, MP4,
Ogg Vorbis, Opus, WAV, WavPack (WAV: `riff.rs` reads the `id3 ` chunk lofty writes); `.caf`, `.mka`,
`.oga` and the DSD containers are passed over. `Library::retag` is the pass behind `resonate tag`
(preview until `--apply`, like `organise`); the settings pane's *Rescan, refresh and retag* group has the same
preview-then-arm shape as *Organising*.

**Every field the `TagSet` names a writer can reach is a `TagField`** (names, numbers, date, label,
identifiers, genre, seven credits, comment, BPM, compilation, grouping, copyright, four ReplayGain
values, four sort names). `TagField::read` spells each as written, so writes compare in the file's
text (compilation `1`, gain `spelled_gain`'s `+x.xx dB`, peak `spelled_peak`'s six places).
`TagField::key_in` is the lofty key per tag kind, `None` where lofty has none (APE BPM, ID3
performer): those are `TagField::unkeyed_in`, written into the concrete tag `saved` converts the
generic one into (APE `BPM` item; ID3v2.4 `TMCL` pairs via `credit_performers`, one per name listed
with `; `, a name already credited keeping its instrument). The reader takes every `TMCL` pair's
name as a performer. Names are append-only: the undo record keeps fields by `TagField::as_str`.

- **A guess is never written: a name goes only where a lookup answered for its row.** `tracks.title`
  falls back to the stem, `albums.title` to whatever grouped the folder. `offered` gates track name
  and artist on `tracks.answered`; album name and its two totals on `albums.answered` (offering
  `albums.release_title`, never `albums.title`); album artist, its id and sort name on the
  *artist's* own `answered` (`land_release` never repoints `albums.artist_id`). Identifiers go
  whatever answered: `tracks.mbid`, `release_track_mbid`, `artist_mbid`, `isrc`, `albums.mbid`,
  `release_group`, what the tags declared about the pressing.
- **A sort name is the lookup's, only where the file names none.** `ArtistSort`/`AlbumArtistSort`
  come from the artist's `sort_name` where it answered (the track artist only if it is the whole of
  `tracks.artist`); `KEPT_AS_THE_FILE_SPELLS_IT` drops either where the file has one.
- **Only the difference is written.** `wanted` reads the file's `TagSet` through `TagSource` and
  keeps fields whose value differs: a written library costs one probe a file, no writes
  (`RetagStats::unchanged`).
- **A picture is the catalog's to give, the file's to keep, in the same write.** `offered_picture`
  mirrors `albums.cover_source`'s rule: offered where the file has none or a thumbnail the album's
  cover `betters`. A replaced picture is noted with the run. The plan reads each file once via
  `TagSource::read` (`Picturing::Copied` where the album holds a cover to weigh against, else
  `Whether`); `Sleeve` holds one album's bytes while `TRACKS_TO_TAG` reads in path order. It is
  written as `PictureType::CoverFront` *replacing* every picture the reader takes as the cover
  (`writing::uncovered` removes front cover, `Other`, untyped; lofty reads every MP4 `covr` as
  `Other`). `written` weighs the picture read back against the bytes sent; a container dropping one
  is `Unwritten::Unconfirmed`. `RetagStats::pictures` counts apart from `fields`.
- **A favourite is this build's rating, plays the count every player reads; anybody else's stay.**
  `TrackToTag::popularity` rides in `Writing::popularity`. Favourite: lofty's popularimeter under
  `resonate_codec::RATED_BY` (`resonate`), five stars; non-favourite: this build's rating removed.
  Lands as ID3v2 `POPM` (counter kept), Vorbis `RATING:resonate`, MP4 `rate`, RIFF `IRTD`; WavPack
  and Monkey's Audio carry FMPS `FMPS_RATING` `1.0`. Another player's rating under its own name
  stays; where a format names nobody that one is ours. **Plays are written favourite or not**, under
  each tag's FMPS name (`FMPS_PLAYCOUNT` in Vorbis/APE, `TXXX` `FMPS_PlayCount` in ID3v2,
  `----:com.apple.iTunes:FMPS_Playcount` in MP4); zero is removed. lofty's generic `Tag` drops a
  name with no `ItemKey`, so `counted.rs` converts it into the format's own tag, sets the count
  there, saves, and reads it back off the same concrete tag. `TagSink::rated` reads what the file
  holds, not the `TagSet`: a count-keeping format is parsed by its own reader (`Counted::read`).
  `Rated::differs_from` weighs the favourite always, plays wherever the tag keeps a count (RIFF
  `INFO` keeps none). Undo keeps both in one integer: favourite's plays as is, an unfavoured row's
  negated less one. `RetagStats::ratings` counts them.
- **A field cleared is cleared from every tag the file holds.** lofty edits only the primary tag
  while the reader takes a name from whichever tag still says it: `writing::cleared_elsewhere`
  copies every other tag holding a field in `Writing::taken`, removes it and saves each into the
  same staged copy. A field *set* needs no pass (`audio.md`).
- **An ID3v2.3 tag stays v2.3.** lofty writes v2.4 unless told; `FileTags::write` asks
  `Counted::holds_id3v2_3` and saves through `WriteOptions::use_id3v23` where it was. No tag: v2.4.
- **A write never touches its file until it is whole.** `FileTags::write` copies to a staged sibling
  `.<stem>.<pid>-<n>.<ext>` (`staged_beside`), writes tags into the copy, `sync_all`s, renames over
  the file, syncs the folder, removing the copy on any failure. A killed writer's copy is swept by
  the next write to that track (`sweep_what_a_dead_writer_staged`, via `staged_by` and
  `writer::writer_of`; the copy is `Held` from creation: `rust-style.md`). **The copy lands only
  over the file it was taken from.** `Taken::of` asks `access(W_OK)` first (an unwritable file is
  refused `PermissionDenied`) and holds device, inode, length, mtime; `landed_through` weighs
  `Taken::still_stands` before the rename: an edit another program made meanwhile answers
  `Error::ChangedWhileWritten`. A folder the listener may not create in does not refuse a file they
  may write: `landed_from_elsewhere` stages under the spool's folders (`/var/tmp`, replaced by
  `TMPDIR`; then the temp dir) and writes back through `written_back`. The copy is a clone
  (`cloned_beside`, `FICLONE`) where the filesystem shares extents. **Where it cannot clone, an edit
  the tag's own room holds lands in the file itself**: `landed_in_place` has lofty write into an
  `Overlay` (4 KiB in-memory pages); `Overlay::land` writes back only differing pages and
  `sync_data`s, only where the file keeps its length and at most `MOST_BYTES_WRITTEN_IN_PLACE` (32
  MiB) was touched; audio that would move or a tag that would grow goes through a whole staged copy.
  For FLAC padding and ID3v2 tags (whose length lofty would change) `Head` has lofty write into an
  in-memory copy of the tag's head and `Head::absorbed` fits the result into the old length or
  answers it will not fit. **A page landed in place can be taken back:** before the first page
  `Undo::kept_beside` writes and syncs a journal (`.<name>.<pid>-<n>.resonate-undo`: each changed
  page's bytes before and after, FNV-1a check), removed once pages sync; a failure rolls back at
  once. A dead writer's journal is mended by the next write to that track and by the scan, which
  looks in each folder before reading a file there: same length and every byte the journal's before
  or after → rolled back, unless all after; other length or bytes is another program's, left alone.
  **What the rename would lose is kept.** A symlinked track is written where the link points. The
  staged copy gets the file's owner, mode and extended attributes (`carries_what_the_file_did`:
  `chown`, `set_permissions`, every `listxattr` name, via `rustix`). Where any is refused, and
  wherever the file has a second name (`nlink` over one), the whole tagged copy is written back into
  the file's own inode instead (`settled_over`). Before that window the synced copy is renamed
  `.<name>.<pid>-<n>.resonate-whole` (`journal::WholeCopy`); a write-back failing part way renames
  it `.resonate-torn` and answers `codec::Error::WrittenBackPartway`. **The whole copy says what the
  track was:** `stood_beside` appends an FNV-1a hash of each 4 KiB page of the track as it stood,
  both lengths and a check, `STOOD_MAGIC` last; the write-back lays the copy over in page-aligned
  chunks (`laid_over`). A dead writer's whole or torn copy is finished by the next write to that
  track or the scan of its folder, never swept as left over, and only over a track still between the
  two (`Stood::standing`): `Mended::WrittenBack`, `Finished`, `ChangedSince` (a page neither:
  another program's edit, copy dropped), `Unreadable` (record failing its check writes nothing).
- **A row cut out of a shared file is never written to.** A path with more than one row, or one row
  carrying a span, is `Unwritten::Cut`, passed over whole.
- **The catalog follows the file, which the next scan reads.** A write changes size and mtime, so
  `files_retagged` writes both back (next walk: row unchanged) and `tagged_title`, `tagged_artist`,
  `title_sort`, `artist_sort`, only for fields it wrote (an unrecorded corrected title would read as
  the tagger moving it and null `answered`). A written file's `track_studies` and `unstudied` rows
  are held across the follow (`analysis.md`).
- **A write is confirmed by reading it back; an unconfirmed one is not followed.** `written`
  re-reads through `TagSource` after `TagSink::write`, weighing every edit, taken field, rating and
  picture; a mismatch leaves `Unwritten::Unconfirmed`, catalog unmoved. **A WAV whose ID3v2 tag
  stands before its `RIFF` header has the tag moved into the RIFF** (lofty does not recognise such a
  file): `FileTags::write` stages a copy that is the RIFF plus the tag's bytes as an `id3 ` chunk
  and settles that copy in the file's place.
- **`Library::retag` takes the `Walk` guard** (the pass rewrites sizes and mtimes a scan's snapshot
  was taken against); a second caller gets `Error::AlreadyWalking`. A vaulted row is
  `Unwritten::Vaulted` (`vault.md`).
- **The preview is the plan.** One `Retagging` is built, handed to the apply or not; a failing write
  moves from `Retagging::writes` to `passed_over`. The tags part of *Rescan, refresh and retag* draws the same plan
  (`ui.md`).
- **The last applied run can be put back.** Every write carries a `Held`: each touched field as read
  before, the rating where changed, a replaced picture. The apply notes it in `retagged` and
  `retagged_fields` with whether the write added the album's cover; a run's first page writing
  anything clears the previous run's notes. **Note before files**: `apply` notes every planned write
  of a page (`Library::retag_to_be_written`) before touching one, then follows the catalog and
  forgets the notes of failed or cancelled writes in one transaction (`files_retagged`); an
  unwritable note stops the run before any file. A write that landed but reads back otherwise keeps
  its note (`Noted::keeps_a_note_of`). The note keeps what the run wrote beside what it replaced
  (`retagged_fields.wrote`), and the walk back leaves a field whose file no longer holds what was
  written. `RetagOptions::undo` plans from that record, not the catalog: each field read before
  written back, one the run added removed (`Writing::taken`), an added cover taken out
  (`Writing::unpictured`), rating put back; through the same `apply`, so putting the walk back
  writes the run again. **A walk back cut short leaves the rest to the next**: `Noted::walking_back`
  holds the record read; a write that fails or reads back otherwise has its old note put back
  (`note_again`); unless every noted file was put back (`retag::walked_back`) the files that were
  lose their notes. `resonate tag --undo` previews, `--undo --apply` writes; the tags part of *Rescan, refresh and retag*
  offers *Put the last run back* behind a second press wherever `Library::retag_walks_back` says a
  run is noted. **A replaced picture is kept once**: `retagged_pictures`, `retagged.picture_id`:
  `KeptPictures` weighs each against the last eight kept (`PICTURES_WEIGHED_AGAINST`), by pointer
  then bytes; `run` drops `Held::picture` from every write once its page is applied; the walk back
  reads each picture by id as a file first asks for it (`PicturesPutBack`).
- **Rows are read a page at a time; a file is never split across two.** `retag::run` walks the
  catalog through `paged::Paging`: `ROWS_A_PAGE` (2 048) rows in path order past the last path
  handed out, the rows of the file a page ended in held back for the next, the page doubled where
  one file's cue rows fill it. Each page is planned and, under `--apply`, written and followed
  before the next is read; release totals are read once. Memory holds only the plan's writes.

## Deleting

- **A deleted track takes its file, every row cut from it, its want and its vault object.**
  `Library::delete_tracks` (`deleted.rs`): `Walk` guard; distinct paths of the tracks
  (`Error::UnknownTrack` for an unheld id); files removed (already gone = removed; refused: logged,
  `Deleted::kept`, rows left). Per path gone, one transaction: drop `wants` naming a release row its
  tracks fill or offering the path (else the next poll refetches it), drop every `tracks` row at it
  (plays, listens, studies cascade), then `store::sweep_orphans` and `alternatives::settle`. A vault
  object no row names leaves the vault; one that will not go waits for `prune_the_vault`. Nothing is
  kept to put back. The window asks first (`ui.md`).

## Organising

`Library::organise` files every scanned track under a layout (`organise-as` key, `--as` per run) via
`resonate organise` and the settings pane's *Organising* group. It takes `Library::scan`'s `Walk`
guard and re-keys a sleeve-keyed album after moves land.

- **`Layout`: segments split before pieces are read.** `Layout::read` splits on `/`, then reads
  `Piece::Literal`s and `Piece::Named(Field)`s, so no `/` reaches a literal or field. Empty segment:
  `LayoutFault::EmptySegment`; `.`/`..`: `Error::LayoutEscapes`; `{{`/`}}` write a brace; unclosed
  `{`: `LayoutFault::Unclosed`; unknown name: `Error::UnknownLayoutField`. Refused when
  `organise-as` is *read*, so a typo is a startup error, never a half-moved library. With
  `as_one_component` writing a value's `/` as `-`, neither a tagger's text nor the template can name
  a path outside the file's root. `DEFAULT_LAYOUT` = `{albumartist}/{album}/{disc}{track} {title}`
  is asserted against `Display for Layout`.
- **A component is what a filesystem takes; a segment resolving to nothing is dropped.**
  `as_one_component`: `/` to `-`, control characters dropped, whitespace and dots trimmed at both
  ends (a scan passes a dot-name over and would prune what was filed there), cut to
  `COMPONENT_BYTES` (255) on a character boundary. The extension is appended only if the layout
  lacks `{ext}`; the last segment's budget is 255 less it (`Segment::write_ahead_of_the_extension`),
  else a cut name loses its extension and the next scan prunes the file with its plays. A middle
  segment resolving to nothing is skipped; the *last*: `Refusal::Unidentified`. **Volume limits are
  per root**: `Naming::of` finds the root's mount in `/proc/self/mounts`; `vfat`, `msdos`, `exfat`,
  `ntfs`, `ntfs3`, `fuseblk`, `fat`, `cifs`, `smb3`, `smbfs` give `Naming::Portable`, also writing
  `\ : * ? " < > |` as `-`. **Derived names obey the 255 bytes**: `noted_staging` cuts the file name
  to fit `.resonate-staging`, `a_place_to_park` the stem to fit `.resonate-parked-<pid>`;
  `in_the_way` treats a destination file name over a component as untakeable.
- **`{albumartist}` falls back to nothing, never `{artist}`** (an album whose tracks disagree has
  none; the track artist would scatter a compilation into a folder per singer). `{disc}`: `N-` only
  if the album has more than one disc.
- **Disc is read as the scan reads it; `max(disc_number)` alone is not enough.**
  `scan::disc_in_folder` has two callers: `scan::sleeve` and `organise::disc_in` (via `disc_of`). A
  set filed as `CD1`, `CD2` with no disc tags has no `disc_number`; `Planner::knows` raises each
  album's count (per album, so every track of a set agrees) to the larger of stored
  `max(disc_number)` and what the folders spell.
- **Library-wide reads are lean and paged.** Only destinations, a root's `Naming` and an album's
  disc count are library-wide: `Planner::knows` is fed `Filing`s (path, root, album, disc) by a
  cursor collecting nothing; rows come via retag's `paged::Paging`. A sheet tying files is followed
  across a page. The plan stays whole.
- **The preview is the plan the apply performs.** `organise::run` builds one `Plan` and hands it to
  `apply` only if `OrganiseOptions::apply`; the pane's `Pass::Preview`/`Pass::Apply` are one call
  with one flag. A preview still reads the filesystem (`symlink_metadata` per destination,
  `read_dir` per source folder). `prune` takes landed moves' source folders and `climb_out_of` walks
  each upward removing while empty, stopping at the first non-empty folder or the root,
  `deepest_first`.
- **Files move before the catalog, in batches; an unfinishable batch is put back.** `apply`:
  `MOVES_PER_BATCH` (256) at a time; cancel heard before each move. Per move: `standing` re-weigh
  (vanished source `SourceGone`, destination now taken `Collided`), rename with sidecars, record in
  `done`. **Every rename refuses an existing name**: `unreplacing::renamed_over_nothing` is
  `renameat2(RENAME_NOREPLACE)`, falling back to `hard_link` then unlink, then a checked `rename`
  (also used by a delivery's filing and the drop-in). **A destination that is another hard link of
  the same file is no rename** (`another_link_of`): it lands as `Landing::Linked`, the source's name
  taken away once the catalog committed. Sidecars are weighed apart: gone since: dropped;
  destination now taken: stays; the track moves without. `Library::files_moved`, one transaction,
  rewrites `tracks.path`, `playlist_entries.path` (touching those playlists' `modified`),
  `lyrics_kept.path`, `lyrics_ahead.path`, `lyrics_refused.path`, `resume_rows.uri`, after first
  deleting any `tracks` or lyric row at the destination (path-keyed). The open queue follows: the
  window sends `Command::Relocate`, `resonate organise --apply` reaches every bus player via
  `org.resonate.Player1`'s `Relocate` (`mpris.md`); `Queue::relocate` follows each row through the
  moves in landing order (`queue::landed_at`). A failing rename puts back only its own move's steps,
  refused as `Refusal::Unmoved` with the volume's `io::ErrorKind`, batch going on. A failing catalog
  write runs `put_back` over all of `done` in reverse; the whole batch counts `failed`. Files before
  catalog: a crash between leaves a moved file the next scan reconciles; the reverse would leave the
  catalog naming absent files.
- **A batch takes back the folders it made and did not fill.** `not_there_yet` records the missing
  levels walking up from the destination's parent; `batch_moved` runs `take_back_the_empty` over
  them whatever the outcome, removing only an empty folder, deepest first, never an already-standing
  empty one.
- **A chain is ordered; only a cycle is refused.** `Planner::in_the_way`: `InTheWay::Stands` for a
  destination another planned move claimed or one the filesystem holds that no scanned row names
  (except a file moved onto itself, same device and inode); the claims (`Claims`) are keyed
  case-folded where the destination's nearest standing folder holds a name and its case-flipped
  twin as one file (`case_blind_beside`), so a second destination differing in case is refused in
  the preview; `InTheWay::MayGo` for a scanned row. **A
  destination that is the track itself is reduced to its own name in its own folder**: on a
  case-insensitive volume a case-only layout difference changes only the file name's case.
  `Planner::order_the_chains`: a move waits on at most one other, so `walked` follows each chain to
  its end, deepest first. **A cycle is broken through a parked name.** `parked_out_of_their_cycles`
  splits one cycle of single-file moves: its file goes first to `<stem>.resonate-parked-<pid>.<ext>`
  (`PARKED`) beside where it stood, then to its destination, so files filed under each other's names
  trade places in one run. The catalog follows each step in the batch's one transaction.
  `Move::parks` names the first half (listed by the preview, counted as moved by nothing). A run
  killed between rename and commit leaves a file the next scan follows by `moves::follow_the_moved`.
  A move leading into a cycle a unit sits in, and every move waiting on a row that stays, is still
  `Collided`; a refused move releases its source and sidecars from `going`. `Refusal`:
  `Unidentified`, `Loose` (destination with no folder between it and the root), `Collided`,
  `SharesASheet`, `NamedFromAbove`, `SourceGone`, `Unmoved`.
- **A rename crossing a filesystem is a copy; the source goes only once the catalog has followed.**
  `landed_onto` reads `io::ErrorKind::CrossesDevices` off `fs::rename` and falls back to `copying`:
  copy, carry the source's mode, owner and extended attributes (`codec::carries_what_the_file_did`,
  refusals logged, never failing the copy) and mtime onto the copy (so the next scan reads the known file), `sync_all`,
  written as destination name plus `STAGED` and renamed into place once whole. Nothing is deleted
  inside the batch: `put_back` undoes a copy by *removing* it; `left_behind` removes sources only
  after `Library::files_moved` committed. A run killed after the rename, before the commit, leaves
  the whole copy beside its source; `already_copied` reads it next run (another device, same size,
  same mtime, same bytes in `COMPARED_AT_ONCE` pieces once the cheap readings agree) and it lands as
  `Landing::Copied`; hence no `Refusal::AcrossDevices`.
- **A staging file is noted before it is written, so a killed run's is swept by the next.** It sits
  beside a destination the next run may never plan again. `noted_staging` writes its path and this
  process's pid into `staged_writes`, committed before a byte is written; `staged_left` removes the
  file where it stands and drops the row. The copy holds its file from creation to rename
  (`Held::taken_over`, `rust-style.md`). A run that applies begins with
  `sweep_what_a_dead_writer_staged`: removes each noted regular file whose name ends `STAGED`,
  skipping one `writer::writer_of` reads as still written (a window, a poll's delivery and `resonate
  organise --apply` may all be copying).
- **The last applied run can be walked back.** `organised` holds what the last apply landed (units,
  companions and sidecars, landing order), each apply replacing it, **noted batch by batch in the
  transaction the catalog follows the files in** (`organise::Noting`: the first batch landing
  anything clears the last run's note, later ones append), so a run killed part way leaves the note
  of what it moved. `OrganiseOptions::walk_back` plans from it, not a layout: units in reverse, each
  `Move::reversed`, through the same `apply`. What the walk back landed is noted in turn. A run cut
  short keeps the units not put back. `resonate organise --undo` previews, `--undo --apply` makes
  it; *Organising* offers *Put the last run back* behind a second press wherever
  `Library::walks_back` says a run is kept. A file moved or gone since is refused at `standing`.
- **A run given files files those alone.** `OrganiseOptions::only` empty is every row; named paths
  read just those rows via `tracks_to_file_at`. Every file is still *known* to the planner; a sheet
  naming a file not given still reads it as a member.
- **A folder's pictures follow its tracks where every track leaves for one folder.**
  `Planner::pictures_follow_their_folders`, after chains are ordered: each picture
  (`names_a_picture`) in a source folder all of whose scanned tracks go, all into one folder
  (`FolderLanding`), is a sidecar of the last move out. Noted with the run.
- **A run files the roots it is given; one the catalog does not hold is refused.**
  `OrganiseOptions::roots` empty is every root; named roots put `roots.path IN (…)` on the
  `TRACKS_TO_FILE` queries. The held-root check (`Error::NotARoot`) runs on the pass thread, not
  `organise::start`, which may not read the catalog before taking the `Walk` guard (the read blocks
  behind a writer the guard waits on). `--as` is the one-run layout, read via `Layout::read`.
- **A file beside a track sharing its name travels with it.** `Planner::sidecars`: files in the
  track's folder that are not scanned rows, named stem plus `.` and more, extension not audio
  (`Meddle.cue`, `Meddle.wav.log` follow `Meddle.wav`); one also carrying a longer-named audio
  file's stem is left to that one. Rows cut from one file are filed by the folder their layouts
  agree on and keep their name; rows of one file naming two folders are `Unidentified`. A sheet
  cutting *one* row from a file renders by the layout like any other, so `sheets_follow_their_audio`
  rewrites the landed sheet after the batch's catalog write via `codec::cue::renamed`: it reads the
  sheet's encoding, checks exactly one `FILE` line names that file and splices the one byte run
  following a `FILE` command (one encoding unit at a time: a UTF-16 sheet is walked in pairs;
  `REM`/`TITLE` lines are left alone), so BOM, line endings and every other byte survive;
  `staged_over` gives the staged file the sheet's mode, owner and extended attributes
  (`codec::carries_what_the_file_did`), renames it over the sheet and syncs the folder. A file two `FILE` lines name leaves the sheet
  alone. A name the sheet's legacy code page cannot hold carries the sheet into UTF-8 with a
  byte-order mark (`carried_into_unicode`).
- **A sheet cutting a file travels with it whatever it is called; one naming several files takes
  them all as one move.** `Planner::sheets_in` reads each source folder's sheets once via the scan's
  `read_sheet` and `claimed_beside`; `sheets_travelling` weighs every sheet naming the moving file:
  one naming only it is a sidecar that must land (a destination it cannot take refuses the move as
  `Collided`). A sheet naming several existing files ties them (`Planner::tied_by_sheets`):
  `file_together` plans them as **one `Move`** (first file `from`/`to`, rest `companions`, sheet a
  sidecar) that a batch never splits; `renamed_onto` puts every file back where one fails,
  `files_moved` follows each file, `sheets_follow_their_audio` renames every `FILE` line. The layout
  must land every file in one folder and every file must be one this pass files, else each is
  `Refusal::SharesASheet`. **A unit is a link in a chain like any move**: `Planned::waits_for` is
  every source standing where the move lands; `walked` is an iterative depth-first topological
  order; meeting a cycle or a doomed move dooms every move on the walk (`Collided`); a unit whose
  member lands where another member stands waits on itself and is doomed.

## Taking files in

`take_in(TakeInOptions { paths, into })` (`take_in.rs`, dropping songs on the window) starts a
`resonate-take-in` thread behind a `TakeInHandle` (`PassKind::TakeIn`) answering a `TakeInSummary`
(`landed`, `passed` as typed `Passing`s). No catalog, so no `Walk` guard; the window follows with a
scan. `Error::DestinationNotADirectory` is the one refusal before the thread starts.

- **Taken: what the scan reads, plus companions; names kept, nothing overwritten.** File:
  `names_audio` or `.cue`. Folder: walked (`.`-names and links skipped, `DEEPEST_FOLDER` levels).
  Companions: anywhere in a dropped folder a picture (`resonate_core::names_a_picture`) and lyric
  sheet (`LYRIC_ENDINGS`); in any folder holding audio, a file named after a track
  (`is_named_after`, organise's sidecar rule). A folder yielding only companions: `NothingInside`; a
  loose file is a companion (`Looks::Companion`) only if the same drop takes audio from its folder;
  else `NotAudio`. Landing is `<into>/<name>`, folders merging: file inside `into`: `AlreadyThere`;
  same bytes: `AlreadyHeld` (neither is a refusal, `Passing::is_a_refusal`); a different file under
  the name: `name (2).ext` and on (to 999, then `Unwritable`).
- **A copy is whole, read back, then named.** Bytes go to hidden `.<name>.<pid>.resonate-part`, held
  under `writer::Held` while written; a run sweeps every folder it lands in once, taking
  `.resonate-part` files whose writer `writer::writer_of` reads as gone. Compared in
  `COMPARED_AT_ONCE` (256 KiB) pieces, synced, `hard_link`ed to the name (refuses an existing name
  atomically), `rename` where no links; the staged file is always removed. Not reading back:
  `Unverified`, not kept.
- **A copy is whole, read back, then named.** Bytes go to hidden `.<name>.<pid>.resonate-part`, held
  under `writer::Held` while written; a run sweeps every folder it lands in once, taking
  `.resonate-part` files whose writer `writer::writer_of` reads as gone. Compared in
  `COMPARED_AT_ONCE` (256 KiB) pieces, synced, `hard_link`ed to the name (refuses an existing name
  atomically), `rename` where no links; the staged file is always removed. Not reading back:
  `Unverified`, not kept.
- **What travels with a track follows its landed name.** `Renames` notes each audio whose name moved
  (`name (2).ext`, or held byte for byte under one: `Stood::Held`) by source folder. Sheets and
  companions from that folder follow: one named after the track takes the new stem
  (`following_its_audio`, longest stem wins; a lyric sheet in a `lyrics`/`lyric`/`lrc` folder also
  follows the folder above's renames); a sheet whose `FILE` line names a renamed track is rewritten
  by `resonate_codec::renamed_cue` (over `LARGEST_SHEET_REWRITTEN`, 1 MiB, copied as is), landed as
  `Content::Rewritten`. The source is never touched.

## The search grammar

- **Words and terms; a term's meaning is typed.** `Search::read` is the whole grammar. A token is a
  word unless it names a field: `title:`, `artist:`, `album:`, `genre:`, `lyrics:` (or `lyric:`,
  `LYRICS_ALIAS`) scope words; `year:`, `added:`, `plays:`, `played:`, `length:`, `rate:`, `depth:`,
  `codec:`, `is:` are `Term`s, with `KEYS` aliases `heard:`, `duration:`, `samplerate:`, `bits:`,
  `format:`. An unknown field or unreadable value reads as the words as written: never a parse
  failure. Numeric readers are overflow-safe. `Display for Term` is canonical and reads back as the
  same term. `is:hires` = lossless above CD (`CD_SAMPLE_RATE` 44 100, `CD_SAMPLE_DEPTH` 16, in
  `db.rs`); `depth:` reads the stored `SampleFormat` (24 valid bits for float).
- **Terms narrow on what the catalog stored; two are ages.** `added:`, `played:` measure from query
  time (month 30 days, year 365), so a saved query answers differently tomorrow. `plays:` is a count
  (`plays:0` never heard); `heard:` = `played:`. `@` windows the count over `listens`, not the
  lifetime column: `plays:>20@30d`; ranges carry the window on both bounds; two `Plays` terms fold
  into a range only with agreeing windows; unreadable window: plain word. It costs the one term no
  index serves, so it narrows, never orders. `year:` is the *album's*. `rate:`, `depth:` are what
  the file is, not the sink request. `is:lossy` = codecs known lossy, so an unnamed codec is in
  neither it nor `is:lossless`. No rating term; nothing sorts on a term.
- **Everything holds, unless `-` denies or `or` alternates.** `Search` = `Clause`s all holding;
  `Clause` = `Asked` alternatives, one must; `Asked` = one token's `Condition`s plus denied. A range
  is a shape: `year:1970-1979` two bounds both holding; `-year:1970-1979` each bound denied in one
  clause (De Morgan at the parse); `Display for Asked` folds an undenied bound pair back. `-` denies
  only at an unquoted token's start (`well-known` untouched); `or` joins only between two unquoted
  undenied tokens, else the word written. No bracketing: a denial reaches one token, an alternation
  one flat run. `and` is no word. **A search asking nothing holds everything**: a word is a
  condition only where `search::pieces_of` finds a letter or digit (`!!!`, `+/-`, `artist:?` drop at
  the parse, an `or` closing the gap); `db::matching` skips a clause-less `Search`; `cuts_matching`
  answers `Narrowed` (`Unasked`, `To`, `Nothing`): a list narrowed by blank text holds every row, by
  text a search reads nothing in (`!!!`) `Nothing`; a drop takes only `To`.
- **A number is read as meant, to the precision typed.** `Term::Length` carries a `Grain` (finest
  `ClockUnit` named, decimals written) and weighs the length cut to it: `length:3:30` and
  `length:3m30s` hold 210 <= s < 211; `length:3m` any 3-minute-and-some track; `length:<=3:30` below
  3:31; `length:>3:30` from 3:31. `Display for Term` writes a length to its grain. A range typed
  high end first is the range named (`bounded`). A bare rate below
  `A_BARE_RATE_IS_IN_KILOHERTZ_BELOW` (1 000) is kilohertz (`rate:44.1` = 44 100 Hz).
- **`=` asks a whole name; a word or phrase a run inside one.** A phrase matches tokens anywhere in
  a column (`artist:"Air"` holds *Air Supply*); `artist:=Air` is `Reach::Whole` (beside `Begins`,
  `Phrase`); `=` inside quotes is phrase text; `lyrics:` takes no whole name. `db::named` weighs it
  through the `words_of` SQL function: artist by id (`tracks.artist_id` or a `track_credits` member
  of that name); album by billed or scanned title; title by the column behind the phrase's
  `INDEX_LOOKUP`; genre (track's or its artist's) by the same lookup, the track's column read as a
  list (`store::lists_words`, the SQL `lists_words`: `genre:=House` holds `House; Disco`, cut on
  `LISTED_APART_BY`, each item through `words_of`). Never ranked: reaches the `WHERE` as a denial
  does. `suggest.rs` artist mixes and genre cards are whole names (`genre:="Rock"` holds no
  *Rockabilly*); a genre list is tallied item by item, so no card is a list.
- **`tracks_fts` holds the fold of a name; the query is folded with it.** Indexed: `folded_letters`
  of title, billed artist (with the album's owner beside it, read by `store::index_row` itself via
  `album_owner_of`, so scan, enrichment and every reindex write the album artist alike), album,
  genres (lyrics apart); `db::indexed` folds every piece of a typed word likewise. Why: `unicode61
  remove_diacritics 2` folds `Ç` to `c`, `İ` to `i`, not dotless `ı`; `remove_diacritics` stays.
  `prefix = '1 2'` answers a one- or two-letter word from a prefix index. No `tracks_fts` column is
  read back. A schema break without a migration: an index from before the fold holds spellings no
  folded query matches, so such a catalog is deleted and rescanned.
- **Lighting reads the name, not the index.** `Search::lit` answers the byte runs of a display
  string a search matched, for one `Column`. FTS5 `highlight`/`snippet` answer the stored (folded)
  column, so the name is split on `char::is_alphanumeric`, each token folded whole keeping its
  original byte range and weighed against the `search::pieces_of` that `db::indexed` builds the
  `MATCH` from. Lit = matched: prefix for a bare word, consecutive tokens for a phrase, nothing for
  a denied word or a term, a column-scoped word nothing in another column. Words are taken before
  the name: `lit` runs per drawn cell per frame.
- **What a track sings is indexed; only a word asking reaches it.** `tracks_fts`' fifth column
  `lyrics` is filled by `store::index_row`: `sung_by` reads `tracks.lyrics` or, where the file had
  none, the fetched `lyrics_kept`; `sung_words` drops `[…]` and `<…>` runs before folding.
  `Library::keep_lyrics` rewrites the column in the keeping transaction. A bare word is `{title
  artist album genre} : "…"*` (`Column::NAMES`), so *love* does not return every song singing it;
  `lyrics:` says the words are meant. `Search::as_sung` turns a search of plain words alone into one
  phrase `lyrics:"…"`, counted by `Library::sung` so the window can offer it. `spelling.rs` has no
  vocabulary for the column. Also a schema break without a migration.
- **A search reaches what the catalog lacks.** `release_tracks.folded` = title, artist, release
  title, through `folded_letters` by `land_release`. `Library::unheld_matching` asks it one `LIKE`
  per piece of every word asked by name (`elsewhere::words_asked`: lone words, bare or scoped to
  title, artist, album); a term, denial, genre or lyric answers nothing on an unheld row. Wanted
  rows first; it and the Missing pane share `SHORT_OF_WHAT_IS_HELD_OR_WANTED` (a release row is
  missing where its album holds a track or the row is wanted).
- **A song is placed on the album it was meant for, not whatever came out first.**
  `elsewhere::meant_release` weighs `Issued`: `Standing` (official, then unstated status, then
  bootleg/promotion/withdrawn), then `Meant` (album, soundtrack counting, then EP, single, unstated
  kind, then compilation/live/any other secondary type), only then date, undated last. It is what
  `best_release` falls back to and what a found song is wanted from.
- **Found elsewhere: `Library::found_elsewhere`.** Sends the words asked by name to
  `Reference::find_songs`, answers `Found`s: recording, title, credit, length, the release meant,
  `performer` (first credited artist MusicBrainz names by id, `Performer::Elsewhere`). Leaves out
  recordings already held (`tracks.mbid`, `release_tracks.recording_mbid`) and recordings on
  bootlegs alone naming no ISRC (`is_worth_offering`). **Of the takes of one folded title and credit
  it offers the one a provider can deliver** (`the_take_worth_offering`: ISRC first, then the best
  standing and kind of release, then the most releases). **Songs are ranked before the cap**:
  `found_among` orders groups by `Fit` (`Titled`, `Answered`, `Partly`), then by the releases the
  group's takes are on, then first place in the answer; to `FOUND_ELSEWHERE_AT_MOST` (12). **A found
  song carries the codes of every take of it as long as it** (`Found::isrcs`, within
  `LENGTHS_AGREE_WITHIN`). Words are weighed by `resonate_core::words_of_a_name` (`Found::answers`,
  `still_answering`, `artists_named_by`, `albums_named_by`, `albums_kept_named_by`: *cant* answers
  *Can't*); the SQL haystacks keep their stored splitting. Halves are public apart (the window keeps
  the reference's answer and reweighs it): `songs_asked` = title, artist, album words only,
  lower-cased, single-spaced, `None` under three letters; `Library::unheld_among` asks the catalog
  about those recordings alone; a link's ISRC, an album's release id and an artist's id are found
  off `tracks_by_isrc`, `albums_by_release`, `artists_by_mbid`; `still_answering` narrows songs
  found for one search to those every word of another begins a word of.
- **Songs of unheld releases are learnt in the lookup pass.** `Pass::learn_the_songs` runs after the
  album, track and artist lookups, before `cover_the_unheld`, **artist by artist**:
  `artists_whose_songs_are_due` (every artist with a group due, most played first); due groups are
  re-read on reaching the artist (`songs_due_for`) and each group again before asking
  (`songs_still_due`), so a group an artist's page landed meanwhile is not asked twice; `at_most`
  caps groups asked. Due: unheld by `unheld_by_any_album!` and never read, refused longer ago than
  `REFUSED_AGAIN_AFTER`, or read longer ago than `REFRESH_AFTER` less `spread_of` its artist (SQL
  function from `schema::configure`: Fibonacci hash of the artist id into `[0, REFRESH_SPREAD)`, ten
  days), so a first pass does not fall due all at once a month later while one artist's groups still
  fall due together and can share a browse.
- **One browse of the artist's pressings where that costs fewer requests.**
  `learning::learn_the_songs_of` is the walk the pass and an artist's page share (`Learner`). With
  an artist mbid and `BROWSED_FROM_GROUPS_DUE` (3) or more groups due,
  `Reference::releases_of_artist` is read page by page keeping only due groups' pressings, while
  pages left < due groups not yet seen. Every group gathered lands through `pressing_of` as if read
  alone; each the browse never reached is asked alone (`Reference::releases_of_group`) after
  `songs_still_due`. A refused or unreadable browse lands nothing and falls back to the groups.
  `songs::pressing_of` takes the pressing whose track count most pressings share (fewer tracks on a
  tie), earliest of those, full date before bare year.
- **Landing songs.** `songs::land` replaces the group's `discography_songs` rows and stamps
  `discography_songs_read`; no pressing: stamped read with nothing; a refusal counts in `refusals`;
  an unreachable reference ends the pass. `EnrichStats::songs` counts rows. **An artist's page does
  not wait for the pass**: `Library::learn_the_songs_of_artist` walks the same due rule over that
  artist's groups (`songs_due_for`, `songs_still_due`, same `learning` walk), telling the `Learning`
  it is handed of each group as it lands and asking nothing more once it says `abandoned`. **Sleeves
  of those releases are kept in the same pass, each as its songs land**: `Pass::cover_as_landed`
  hands a group whose songs just landed to the picture readers where `unheld_cover_is_due` says so
  (`unheld_covered` remembers); `Pass::cover_the_unheld` runs after `learn_the_songs` for the rest
  (`unheld_covers_due`: unheld, no cover held, none asked within `COVERS_ASKED_AGAIN_AFTER`, most
  played artist first, `at_most` caps) as `Picture::OfAnUnheldRelease`: `Reference::cover` for the
  pressing its songs were read off, else `group_cover`. `land_unheld_cover` writes `unheld_covers`
  for either answer, never for a failed or refused fetch; an empty answer never blanks a held
  picture; `Library::unheld_cover` reads one back.
- **Wanting.** `Library::want_album` wants every song of a group not held by recording or title:
  lands the optional named `pressing` first, reads the group's pressings where the pass never
  reached it, then `want_from_landed` (as `want_found` via `want_from_release`): release read and
  landed once, each recording's row wanted, cover asked once. `Library::want_missing_tracks` (album
  already in the catalog) `want_in`s release rows without a held track in disc and position order
  under one write transaction; an existing want is due again.
- **`Library::want_found` is the want.** The release the `Found` names, or where the search gave
  none the one `meant_release` picks from `Reference::recording`'s releases, is read whole through
  `Reference::release`. An album already carrying that mbid is taken as is; else a new one (billed
  to `store::artist_named`, stamped `albums.found_elsewhere`), `land_release` writing rows as
  enrichment does. Wanted artist: track credit where present, else landed release artist. If the
  landed release advertises front art or has a release group and the album has no picture,
  `want_found_uncovered` answers a `Covering` and the caller asks `cover_what_was_wanted` after the
  want is out; a failed fetch does not cancel the want. Errors: `Error::UnknownRelease`,
  `Unreleased`, `NotOnTheRelease`; **a release answering either of the first and last is not the
  end**: the next of the recording's releases in the order worth offering is tried, up to
  `RELEASES_TRIED_AFTER_THE_FIRST` (3). The want's row is given every code of `Found::isrcs`.
  `store::ORPHANS` spares a trackless album only where found elsewhere and still wanted; the albums
  pane never shows one (`HOLDS_A_BEST_COPY` asks for a track). **Which release is the listener's to
  say**: a `Found` carries every release of its recording, ordered by
  `Found::in_the_order_worth_offering`; the found row's *Want* mark opens them as a menu; a press is
  `want_found` with `Found::from` that release.
- **An album or artist the words name is found beside the songs.** `Reference::find_albums` is a
  release-group search (`AlbumMatch`), asked after `find_songs`, failing without failing it.
  `Library::unheld_albums_among` keeps an `AlbumFound` per album or EP (soundtrack counts) every
  folded word of which begins a word of title or credit, once per folded title and credit, to
  `ALBUMS_FOUND_ELSEWHERE_AT_MOST` (12), dropping one the catalog holds by `albums.release_group` or
  folded title under an artist of the same key. Opening or wanting one is `open_album_uncovered` and
  `want_album`. **What the lookup kept answers first, offline**: `Library::albums_kept_for` reads
  albums and EPs of library artists' discographies held by no album, and the window lists them ahead
  of MusicBrainz's answer. `elsewhere::artists_named_by` reads credits of the `find_songs` matches:
  a credit with a MusicBrainz id whose folded name has a word begun by every word typed, once per
  id; `Library::unheld_artists_among` drops those held, up to `ARTISTS_FOUND_ELSEWHERE_AT_MOST` (4);
  `Library::open_artist_found` bills it through `store::artist_named` under the id, stamps
  `artists.found_elsewhere` where it holds no track, album or credit, lands the profile
  (`land_artist`) and release groups (`land_artist_releases`). **`ORPHANS` spares an artist stamped
  `found_elsewhere`.**
- **Song link: followed to its recording by ISRC; by title and artist only under the strict rule.**
  `linked.rs`. `SongLink::read` takes one whitespace-free token: `MusicBrainz` recording URL,
  `Deezer`, or `Elsewhere` (a song page on a streaming service or song.link); nothing for an album,
  artist, playlist, other host or text with a space. `Library::follow_link` answers a `Linked`:
  `Held` (a recording link checked against `tracks.mbid`, a link elsewhere against `tracks.isrc` for
  every code `Reference::song_linked` names), `Found` or `Unnamed`. Each code goes to
  `recordings_of_isrc` in turn until one names a take; `the_take_linked` keeps takes within
  `LENGTHS_AGREE_WITHIN` (5 s) of the link's length, the closest. **No code names a take**:
  `LinkNames` carries the billed title and artist; `the_song_searched_for` asks `find_recording`
  with them and the link's length, phrase then words, accepting only under
  `enrich::the_recording_named` (the track identification's `matches_a_recording` weighing; no rule
  of its own). **Upload titles are read as billed**: `readings_of` reads *Artist - Song* where the
  part before the dash agrees with the channel (`- Topic` stripped); else (a `…VEVO` channel) the
  upload's own billing is asked first, the whole title under the channel second. `VIDEO_QUALIFIERS`
  is a closed list like `VERSION_QUALIFIERS` (*Official Video*, *Lyric Video*, *HD*, *4K*, …)
  stripped by `enrich::without_brackets_that`; *(Live)*, *(Remix)* stay. **A video may run past its
  song**: for a YouTube page (`linked::Footage::of`) where the strict search named nothing, the
  readings are asked again with no length and `enrich::the_recording_a_video_names` takes a take
  agreeing on title and credit that the video could have played whole (no longer than the video by
  more than `RECORDING_MAY_DIFFER_BY`, no shorter by more than
  `A_VIDEO_MAY_RUN_LONGER_THAN_ITS_SONG_BY`, 4 min).
- **Album link: followed to its release group by identifier, never title.** `AlbumLink::read`:
  `Release`/`Group` (`musicbrainz.org/release/<mbid>`, `/release-group/<mbid>`), `Deezer`,
  `Elsewhere` (album pages on Spotify, TIDAL, Apple Music, YouTube Music, Amazon Music, album.link).
  `FollowedLink::read` tries song, album, then artist. `Library::follow_album_link` settles a
  `NamedAlbum` via `linked::album_named`: group link asks `Reference::release_group`, release link
  `Reference::release` and takes its group; else `Reference::album_linked` gives the `Barcode`s the
  album is sold under (8 to 14 digits), each in turn to `Reference::releases_by_barcode`;
  `the_release_barcoded` takes a release only where its barcode is that code (`Barcode::names`:
  UPC-A and EAN-13 are one GTIN) and every release so barcoded is in one group. A held album
  carrying the group or release (`album_held`) gives `Linked::HeldAlbum`; else `Linked::Album`, both
  handed to `want_album` (a named pressing is read whole and its songs landed as the group's; a
  group-only link is wanted from the pressing most of its pressings share). **Playlist link**:
  `PlaylistLink::read` takes Deezer, ListenBrainz, Spotify, Apple Music and YouTube playlists (never
  an `OLAK5uy_` album list or a `watch` link carrying a `list`); `Reference::playlist_linked`
  answers a `LinkedPlaylist` of `ListedSong`s (`Recording`, `Named` (`LinkNames`; Spotify's and
  Apple Music's are followed by the strict search) or `Uploaded` (YouTube));
  `Library::follow_playlist_link` follows each as `follow_link` would;
  `Library::fill_playlist_named` (`playlist::fill`) puts held tracks into the list of that name
  (folded), only rows it lacks, else starts one under a free name (`Filled`). **Artist link**:
  `ArtistLink::read` takes `musicbrainz.org/artist/<mbid>` and `deezer.com/[lang/]artist/<n>`;
  `Library::follow_artist_link` answers `Linked::HeldArtist` or `Linked::Artist` with the
  `ArtistFound` MusicBrainz names. **An artist link elsewhere is followed by its page, never the
  name it shows**: `ArtistLink::Elsewhere` holds the page as MusicBrainz stores it (`artist_page`;
  `SOUNDCLOUD_PAGES_NAMING_NO_ARTIST` are no artist), `ArtistLink::pages` lists the pages to ask
  (Apple Music in another storefront also as `us`), `Reference::artist_at` says which artist
  MusicBrainz files a page under, taken only where exactly one; a Deezer artist is asked that way
  first, by name only where MusicBrainz files its page under nobody.
- **Lone word ranked; denied or alternated word looked up.** Unnegated, unalternated, non-`=` words
  are what `indexed` folds into the single FTS5 `MATCH` the index is joined for, so `rank` and
  `SortOrder::Relevance` mean what they did. Any other word is `INDEX_LOOKUP` in the `WHERE` (a
  subquery on the same index, scoring nothing); a search with every word denied or alternated has no
  join and relevance falls back to album order. Denial is `NOT coalesce(…, 0)`: a row unable to
  answer (no album for `year:`, no duration for `length:`) satisfies it. `Matching::grouped` carries
  the lot into album and artist listings.
- **Words naming a title by an artist are read so, against the artists held.** `meant.rs`.
  `ByArtist::read` takes words with no grammar, split at the last standalone *by* (*Stand by Me by
  Ben E. King* keeps its title) or a spaced dash, artist first, each side holding a word.
  `Library::meant` weighs the artist half through `Spellings::artist_named` (the name the vocabulary
  holds whole, or nearest within `furthest_from`'s budget) and answers a `Meant` (the search
  `ByArtist::searched_as` writes: title words scoped `title:`, artist `artist:=`) only where that
  search holds a track; else words are searched as typed. The window reads every page through it
  (`Asked::meant` beside `Asked::text`) and offers the words as typed (`ui.md`). `words_asked` drops
  the *by* and the dash from the songs asked elsewhere; `songs_asked` carries the reading beside the
  words. **MusicBrainz's answer is weighed here, only where it bears the reading out**: where no
  match is `ByArtist::answered_by` the answer passes unweighed, the reading being wrong (*stand by
  me* is not *stand* by *me*); else `weighed_for` keeps matches whose credit is nearest the typed
  artist, of those the titles matching the typed title likewise.
- **A search matching nothing is answered in the catalog's own spelling; only then is the catalog
  read.** `spelling.rs`. `Spellings` = four `Vocabulary`s (titles, artists, albums, genres), each
  mapping a folded word to its drawn spelling and row count (`store::spellings`, through the
  `search::runs_in` splitting that lights a matched run). `Spellings::did_you_mean` corrects words
  of the parsed `Search` in place; terms, denials, phrases, scopes survive (`title:` weighs its word
  against titles alone); the result is the whole query rewritten via `Display for Search` (what the
  *Reads* chips draw). **Correct only where the catalog cannot already match, and never further than
  affordable**: `Vocabulary::holds` passes over a run the index would find (held outright, or
  beginning a held word: *floy* is not corrected to *Floyd*, *floid* is). `furthest_from` is the
  budget: under four letters never, up to seven (`ONE_LETTER_WRONG_UNTIL`) one letter wrong, longer
  two (`FURTHEST`); `apart_by` is a bounded optimal-string-alignment distance. Nearest wins, then
  the word most rows hold; the marked spelling is drawn. A suggestion is only ever a held word.
  **Run-together and split are corrections too; neither invents a word.** `Spellings::whole` is the
  *exact* reading; `run_together` where two runs joined name one held word and the two apart do not
  both, `split_apart` where one run cuts into held words (*pinkfloyd* is **Pink Floyd**). Order:
  join, nearest word, split. A split cuts into up to `MOST_PIECES` (4): fewest pieces win, then most
  rows. `run_tokens_together` joins across two plain undenied same-scope *tokens*; a scoped split is
  written as a phrase so `artist:` reaches both halves; `worth_asking` weighs an adjacent pair
  joined as well as each run alone. **A phrase, or a run of tokens, is weighed as a whole name
  before a word at a time**: `Vocabulary` holds multi-run names beside words; `instead_of_the_whole`
  weighs a quoted token against them first under the budget its whole length earns (`names` is
  `holds`'s counterpart); unquoted, `name_the_tokens` gathers adjacent same-scope plain tokens
  (longest run to two, `MOST_TOKENS_IN_A_NAME`) for `instead_of_the_name`, but `beyond_a_word`
  weighs a run as a name only where a token is a word the catalog neither holds nor can spell nearer
  (*the great gig in teh sky* is **The Great Gig in the Sky**, *pink floid* is still **pink
  Floyd**). The budget is in letters (`letters_in`), not bytes.
- **The read is the cost: paid once, kept until a name could have moved.** `Library::did_you_mean`
  walks every title, artist and album name, so the window asks only where albums, artists, tracks
  and lyric matches (`sung`) all came back empty, on the background executor, and
  `spelling::worth_asking` refuses before the read where no word is long enough to correct.
  `Inner::vocabulary` stamps the `Spellings` with a counter and returns an `Arc` until the counter
  moves. **SQLite itself moves the counter.** `watch_the_names` lays `NAMES_MOVED_TRIGGERS` on the
  writer: `TEMP` triggers (that connection alone; never the schema, a migration or another program)
  step a row of a `TEMP` table wherever a row of `tracks`, `albums`, `artists` or `artist_genres` is
  inserted or deleted, or a column the vocabulary reads takes another value; an `update_hook` on the
  writer sees that row move and steps the counter. Invalidation is per *column*: a counted play, a
  favourite, a scan writing a title back unchanged drop nothing. Another process's writer is
  invisible to the hook: that half is `PRAGMA data_version`, read off the writer connection beside
  the counter into one `CatalogStamp { named, written_elsewhere }` (`Inner::names_stamp`), under
  `try_lock` (a search must not wait on a scan batch; a busy writer is this process writing, so the
  counter alone decides). Every delete on the four tables carries a `WHERE`, so SQLite's truncate
  optimisation (which skips the hook) cannot arise. `Library::written_elsewhere` hands the same
  reading out as a `WrittenElsewhere`, what the window watches (`ui.md`).
- **Which rows a name moved is journalled, not only that one did.** `renamed.rs`: temp triggers on
  `tracks` (`RENAMED_TRIGGERS`) insert-then-delete a row of `temp.titles_moved` or
  `temp.titles_added` keyed by the track id, so the update hook hears the id; the hook steps the
  names counter itself and notes `(counter after the step, id, added)` in `Inner::renamed`, a
  `Renamed` journal of at most `RENAMED_HELD_AT_MOST` (16 384) notes, oldest let go first.
  `Library::names_moved_since(a stamp)` answers `NamesMoved`: `Nothing` where the stamp still holds;
  `Everything` where another process wrote or a note after the stamp was let go; else `These {
  retitled, added }`.

## Playlists

- **A playlist is a list of cuts, not library rows.** `Cut` = `MediaLocation` + `Option<FrameSpan>`
  (the pair `Track`, `QueueItem`, `Resumable` carry); `playlist_entries` stores `path`,
  `span_start`, `span_frames` (`store::span` convention). Unscanned files are rows; a file leaving
  the library leaves its playlists named but unresolved. Read back = `LEFT JOIN tracks` on `path`
  *and* `span_start`: a `PlaylistEntry` has its `Cut` always, a `Track` only where cataloged. Local
  files only: `add_to_playlist` refuses another source's `MediaLocation`. `Cut::of` = catalog row to
  cut; `Cut::whole` = a sheet's path, command-line file, bus `AddTrack` (no region vocabulary in
  those formats). Folding doubles reads the whole cut: two cuts of one file = two rows, one cut
  twice = one.
- **Edits write only rows they move.** Append at `max(position) + 1`; remove shifts rows after; move
  shifts the span between ends; tidy rewrites from the first gone file. A shift parks rows at `-1 -
  position` and unparks in a second statement (`PRIMARY KEY (playlist_id, position)` refuses
  transient collisions). `position` stays dense, so spans are weighed against the list, never cast
  to `i64`: `move_in_playlist` answers false where either span end or the drop row is past the end,
  or the drop row is inside the span; `remove_from_playlist` takes first-named to list end, refusing
  a span starting past it.
- **A name is one name however written.** `playlists.folded` = `playlist::folded` (`to_lowercase`,
  all alphabets unlike SQLite `NOCASE`; then NFC, not NFKC) with the `UNIQUE`: two spellings cannot
  coexist; one function for stored and asked names (`Library::playlist_named`, `HOLDS_THE_WORD`,
  `PlaylistOrder::Name`). `Library::rename_playlist` refuses a spelling another playlist holds via
  `refuse_duplicate`, which excludes the renamed one.
- **Every row edit refuses inside its transaction.** `only_a_list` checks existence and not-a-query
  against the transaction (`Error::UnknownPlaylist`, `Error::NotAList` never stale). Dropping
  passes: `Going` = the question (`Gone`, `Doubled`, `Unwanted`, `Matching`); `asked_of` asks it of
  rows `numbered` read inside the transaction. Only the filesystem is asked outside: `gone_from`
  stats each distinct path first through a reader (a downed mount holds no write lock). A refused
  edit still pays the restore point. `copy_playlist` reads its source outside its transaction by
  design.
- **A playlist holds a list or fills itself from a query; both answer via `playlist_entries`.** A
  `playlist_queries` row = saved query: text, `SortOrder`, reading, row cap.
  `Library::playlist_entries` runs it instead of reading the table (queue, pane, MPRIS, sheet
  writers see an ordinary playlist); listing counts are the query's, **measured once a minute and a
  catalog move** (`Inner::measured_saved`: a `KeptMeasurements` under the rows stamp the suggestions
  use, the music filters and `MEASUREMENTS_KEPT_FOR`, since `added:`/`played:` windows slide with
  the clock), so listing every smart playlist is not a whole-catalog count apiece
  (`a_saved_query_measured_twice_is_counted_once_until_the_catalog_moves`). A query refuses with
  `Error::NotAList`: `add`, `remove_rows`, `move_rows`, `prune`, `sort_playlist`, `remove_matching`,
  `fold_doubles`, `copy_playlist` into it, keeping in order; rename, play, export, drop work.
  `Library::revise_query` rewrites text, sort, cap, reading in place, renaming in the same step
  (undoable, `Edit::Revised`); a list refuses it with `Error::NotAQuery`. No album/artist id is
  held, so saving with an album selected cannot silently widen: *Save this search* appears only
  where the search box scoped the pane; CLI `--query` saves under an unused name and revises the one
  named. A query is read when asked (can differ twice running); a queue loaded from one is a
  snapshot. `Library::playlist_lists` (the picker for putting rows in a list): grouped pass, queries
  excluded in SQL, no `count(*)`.
- **A list is put in order by the library, not a row at a time.** `Library::sort_playlist(RowOrder,
  Direction)` rewrites in place (one `DELETE`, dense reinsert from the first moved row), answers
  rows moved: an ordered list costs no write. `RowOrder::Album`/`Artist`/`Title`/`Length` read the
  catalog via the tracks pane's order columns, unscanned last either way; `File` reads the path. An
  edit, not a property: a list in hand still appends at the end. **A list is in hand or kept in an
  order**: `Library::keep_playlist_in_order(Option<Kept>)` (`Kept` = `RowOrder` + `Direction`)
  writes `playlists.kept_order`/`kept_reading` and orders rows in the same transaction;
  `playlist::add` re-runs the pass, so hand adds, `--add`, imported sheets land where the order
  says. Kept refuses `move_in_playlist` and `sort_playlist` with `Error::KeptInOrder`; `None` = back
  in hand, rows stay; no change writes nothing (`Change::Nothing`). The window's
  `views/playlists.rs::Rows` is `InHand` (moved, edited), `Kept` (edited), `Narrowed` (edited),
  `Matched` (neither); movers, drag, reach, ✕ each ask one question, not "is it a query". CLI
  `--order <ORDER> [--reverse]`, `--keep`, `--by-hand`.

- **What a search showed is what a drop removes; nothing empties a list in one gesture.**
  `Library::remove_matching` reads matched positions and rewrites from the first that goes
  (`prune_playlist`'s pass, both `dropped_where`). It takes `&str`, not `Option<&str>`, so no call
  empties a list (emptied = discard). *Drop shown* only under `Rows::Narrowed`; CLI `--matching
  <TEXT> --drop` (`--drop` requires `--matching`). A narrowing a search reads nothing in (`???`,
  `!!!`) matches nothing, not no condition (`db::asks_for_nothing_it_can_read`); blank = no
  narrowing.
- **A doubled row is folded away, first of each staying.** `Library::fold_doubles` drops each row
  naming a cut (path+span) an earlier row named, via `dropped_where`; nothing folds on its own (a
  file twice is deliberate in `add_to_playlist`). `Library::tidy_playlist` asks gone and
  earlier-live-row-names-it (`Going::Unwanted`), dropping the union in one `Edit::Tidied` step (the
  pane's *Tidy*, one undo). CLI `--tidy` (missing files, `prune_playlist`), `--fold` (doubles),
  refused together. **A playlist is copied into another, never moved**: `Library::copy_playlist`
  reads rows (all, or search-matched) and lands them via `add_to_playlist`; `Error::IntoItself`
  refuses one playlist both sides; out of a query freezes a search into a list (CLI `--into <OTHER>`
  only, creating OTHER if absent). **Duplicated whole, one undo step**:
  `Library::duplicate_playlist` names the copy the first free of *NAME (copy)*, *NAME (copy 2)*, ...
  in one `undo::started_under_a_name_found` step (name weighed inside the taking write, so
  concurrent duplicators each get a name); a list keeps rows as stored plus kept order, a saved
  query its search, order, cap; pin, plays, when-played stay with the source.
- **A playlist's picture: the covered albums its rows reach first.** `Library::playlist_pictures`
  answers at most the covers asked, one per distinct picture (`pictured_by` identity and likeness).
  List: own order; saved query without cap: `pictured_by` over its search; with cap: the rows the
  cap leaves. `Library::pinned_playlists`: pinned ids and names, most lately pinned first, no join.
- **A search narrows a playlist; shown = played.** `Library::playlists` matches each standalone word
  against the name; a term is passed over; no standalone word = whole index.
  `Library::playlist_entries` runs the whole text against the catalog: an unscanned row falls out; a
  saved query's text and the typed one are read *beside* each other (`db::matching` takes both
  texts, asks their clauses together) under the query's cap; listing count stays the query's.
  `PlaylistEntry` carries `position`, so a narrowed row's number and ✕ are the list's;
  `Rows::Narrowed` is edited, not reached/moved (narrowing broke the adjacency a `Span` needs).
  Whole regardless of search: Rename, Sort, Tidy, Export, Discard. What is shown: Play, Play next,
  Add to queue, Copy, Drop shown, row gestures. Playing a narrowed playlist leaves
  `Library::playing_playlist` unset.
- **A listing is an order and a `Direction`; its drawer picks both.** `Library::playlists` takes
  both, `order_by` writes the sense into SQL (pinned first, ties by id), so a reading is the
  query's, not a `reverse()`: never-played sits last in *Most recent first*, first in *Longest ago
  first*. `PlaylistOrder::reads` = the direction an order *opens* at: ascending `Name`, descending
  the other five (`Created`, `Modified`, `Played`, `Plays`, `PlaysThisMonth`); the pane's *Reading*
  row and `resonate playlists --reverse` flip it. The bus opens ascending always (MPRIS defines
  `CreationDate`, `ModifiedDate`, `LastPlayDate` oldest first) and maps `reverseOrder` to
  `Direction::Descending`.
- **Which playlist is in play is the library's, kept by the queue it was loaded as.**
  `Library::playing_playlist` = a cell beside the catalog holding a `Playing`: `PlaylistId` + the
  `QueueStamp` `engine::stamp_of` reads off the loaded rows (locations and spans, not handed ids);
  it answers only where stamps agree. `Queue::rows_changed` restamps on every row arriving/leaving;
  `stamp_what_is_playing_from` stamps the rows outside the queued-next run, which the engine
  publishes as `PlayerState::queue_stamp`: an `Insert` into the playlist's rows or a `Remove` takes
  the badge off wherever it came from; rows queued next leave it; `Queue::split` restamps;
  reordering the same rows stamps the same (read off `items`, not `order`).  `QueueItem` derives no
  `Hash`: `engine::stamp_of` is the only stamping path. Only held playlists: `set_playing_playlist`
  fills the cell from whether `played_now` counted the play (`counted_a_play`). Not persisted (as
  the queue); the same call writes `playlists.played` and steps `playlists.plays`; loading twice
  counts twice. `Library::playlists_revision` = companion cell bumped by every playlist write,
  watched by a 200 ms bus poll; the engine publishes after applying the load, so the badge trails
  the queue by a poll.

## Undo

- **A playlist edit is one undoable step: the playlist as it stood.** `undo.rs` is all of it.
  `undo::edited` wraps every mutator's transaction: reads name, kept order, saved query and the rows
  its `Reach` names; keeps the restore point only if the change moved something (`Change::Made`, not
  `Change::Nothing`). `undo::started` is the twin for gestures creating a playlist. `Library::undo`
  writes the newest step back; a `Whole` step = one `DELETE` of the playlist row (cascades entries
  and query) then a dense rewrite, so a discarded playlist returns under its id. `played`, `plays`,
  `pinned` are read off the row, not restored; the date last changed is restored.
  `Library::start_playlist` and `Library::revise_query` are single calls (two calls = two steps).
  Stack `Inner::steps`: 32 steps (`STEPS_HELD`), 50 000 rows (`ROWS_HELD`), per run (command line:
  no undo); the newest step is always kept.
- **A step holds only the rows its edit could have moved.** The mutator names a `Reach`;
  `Reach::settled` makes the step's `Reached` inside the transaction: `Unmoved` (rename, revision):
  no rows; `From(len)` (`Reach::Appended`): rows from the old length on; `Window`: the run an edit
  replaced (`Reach::Emptied`, `Reach::Shuffled`), clipped by `clipped` in signed arithmetic;
  `Whole`: discard, sort, keep, row-dropping passes, append to a kept list. Restore: `Unmoved` =
  `written_over` (the playlist row, query rewritten if any); `From` = `written_over` +
  `rewritten_from`; `Window` = `written_over` + `rewritten_within` (delete the `leaves` rows, shift
  later rows via `closed_up`'s park-and-unpark, write `holds` rows into the gap); only `Whole` takes
  the cascading `DELETE`. `undo::walk` reads the standing it overwrites through `Reached::turned`,
  so the inverse holds the same stretch.
- **A step is walked either way; walking writes it back the other way.** `undo::walk` serves
  `Library::undo` and `redo`: pop a step, read the standing it will overwrite, apply, push what was
  read onto the other stack (`Inner::walked`, with its own 32-step / 50 000-row bound); no step
  carries an inverse. No playlist under a step's id = `Standing::Fresh`; walking one that discards
  the playlist in play clears `playing_playlist`. The next edit ends a redo (`undo::note` clears
  `walked`). Ids stay safe: undo is strictly last-first, so a step on `walked` never names a
  playlist SQLite has since given the id to.  **Walked only over the playlist its edit left.**
  `Step::left_at` = the playlist's `modified` stamp as the edit finished (`None` if it discarded the
  playlist); `walk` re-reads it in the restoring transaction; any other value =
  `Error::PlaylistChanged`: step dropped, nothing written. The command line, MCP or a second
  instance adding a row, and `organise::files_moved` rewriting row paths, end the steps under them.

## Sheets

- **A playlist leaves and arrives as M3U, PLS or XSPF; a sheet names files, not tracks.** `sheet.rs`
  is the seam (`Library::import_playlist` / `export_playlist`): reads bytes, settles encoding,
  decides from the *text* which format, hands `m3u.rs`, `pls.rs` or `xspf.rs` the job;
  `sheet::parse` reads a sheet with no file. Shared there: the relative-or-absolute path rule,
  percent escaping both ways, and the staged rename over the target (hidden sibling
  `.<name>.<pid>-<n>.new`, `create_new`, carrying the replaced file's mode, owner and extended
  attributes, `sync_all`ed, the folder synced after the rename, removed on any failure). Writing picks the
  format by the target's extension (M3U where it declares nothing); reading picks by content.
  - **Track facts are never believed**: a row is a path, so `#EXTINF:`, `TitleN`/`LengthN`,
    `<title>`/`<creator>`/`<duration>` are written, never read. XSPF text is unescaped in one pass
    (`&` looks for its `;` only within `LONGEST_ENTITY` bytes).
  - **Non-`file://` rows** are counted, not refused. Scheme and `localhost` authority are read per
    RFC 3986 (`FILE:///a.wav`, `file:/a.wav` local; `file:track.flac` relative). A row empty once
    trimmed names nothing.
  - **Encoding**: byte-order mark (UTF-8; UTF-16 either way, odd length refused), else valid UTF-8,
    else the code page `resonate_core::text` detects (`rust-style.md`), unless the name declares
    UTF-8 (`.m3u8`, `.xspf`) or the bytes hold a NUL; either refusal =
    `Error::UnreadablePlaylistFile`. A line ends at `\n`, `\r\n` or a bare `\r` (`sheet::lines_of`).
  - **Import reconciles by count, not set**: a row the playlist holds counts as already there (a
    sheet read twice is a no-op); a file the *sheet* names twice is two rows. A taken name is
    appended to, not refused.
  - **A row is written as a path only where `sheet::as_a_row` finds it reads back as itself**, else
    as an escaped `file://` URI.
  - **Settling**: the parse settles lexically (`settled`). `playlist::cuts_of` settles each row
    against the catalog: the path as written where a track holds it, else the path under a root a
    folder of it resolves to, else its canonical path (canonical for every row would lose a file a
    `follow_symlinks` scan stored through its link), or lexical where the file is absent. A row
    naming a file nowhere on this machine is reconnected by trailing components: the one catalogued
    path whose last components agree with the most of the row's, at least two; a tie leaves the row
    as written. The candidates are read off `tracks_by_file_name` (the generated `tracks.file_name`,
    matched `COLLATE NOCASE`, so ASCII case only), one probe per distinct lost file name; a name
    beyond ASCII alone scans the catalog once, lowercased by Unicode as the components are
    (`a_sheet_naming_a_file_in_another_case_beyond_ascii_still_reconnects_it`).
  - **Windows rows**: backslash and no forward slash = a Windows path; `forward_separated` turns the
    separators, and `reads_back_as_itself` refuses a row it would turn (`AC\DC.wav` is written as an
    escaped `file://` URI). A literal backslash cannot stand in a `file://` URI or XSPF location
    (writers escape it `%5C`), so `sheet::forward_escaped` turns every literal one into a separator
    *before* unescaping.
  - **Size**: the 64 MiB ceiling (`LARGEST_PLAYLIST_FILE`) is weighed against the declared size, the
    read size and what an export would write (`read_back_within` refuses past it as
    `PlaylistFileTooLarge` before a byte lands).
  - **Short and missing**: `Library::prune_playlist` drops rows whose files have gone (asked for).
    PLS `NumberOfEntries` is weighed against the text, shortfall in `Imported::short`; M3U and XSPF
    declare no count.
  - **A tidy drops only what is surely gone.** `gone_from` weighs a file gone where the stat
    succeeds on a non-file, or answers `NotFound`/`NotADirectory` (other failures keep the row) and
    the path is on no volume `volumes::is_mounted` finds unmounted, under no root whose directory is
    missing, under no desktop mount point nothing stands at
    (`volumes::is_under_a_mount_point_not_there`: `/run/media`, `/media`, `/mnt`), under none
    `/etc/fstab` lists that `/proc/self/mounts` does not (`volumes::listed_and_not_mounted`), and
    either the file's own folder stands or, its folder gone too, the nearest standing folder above
    holds something. **A row notes its volume as added**: `start`, `add`, `import` find the mount
    point above each row's folder (highest folder sharing the nearest standing one's `st_dev`;
    `volumes::under`) and `volumes::note` writes it beside the rows, so a hand-mounted drive no
    table lists is out of reach once unplugged.
- **A cue row leaves as its file plus VLC's times, and returns as the cut.**
  `#EXTVLCOPT:start-time=` / `stop-time=` lines before an M3U row; the same two options as
  `<vlc:option>` in a track's VLC `<extension>` in XSPF. Seconds = the cut's frames at the track's
  rate; reading back takes the rate the catalog holds for the row's path, or probes the file once on
  import (`probed_rate`), and lands on the written frame. A row whose file the catalog lacks and
  will not probe, or a PLS row, is the whole file. `Sheet::locations` holds `Listed` rows (a
  location and its `Timed`): the parse stays I/O-free and the fuzz target reaches it.
- **`xspf.rs` reads its own markup; every leniency is deliberate.** A tag ends at the first `>`
  *outside* a quoted attribute value. `xml:base` resolves down an element stack: a relative
  `<location>` answers to the base in force; a non-`file://` base makes every relative row under it
  count as elsewhere. A track's `<location>`s are alternates: the first naming a local file is the
  row; none = one `elsewhere`.  `<album>`, `<image>`, `<annotation>`, `<meta>` stay unread on
  purpose (a sheet is never a source of tags). Writing is strict where reading is lenient:
  `marked_up` escapes the five entities and drops every character XML 1.0 forbids.

## The MPRIS seam

- **`resonate-mpris` reaches playlists through a seam, never the library.** `Playlists` is the trait
  `Mpris::start` takes beside `Host`, filled by the binary with a `Collection` over the `Library`
  and `Player`. The interface is served only when one is supplied: no catalog = no
  `org.mpris.MediaPlayer2.Playlists` advertised at all.
  - `Orderings` comes from `mpris::PlaylistOrder::ALL` (Alphabetical, CreationDate, ModifiedDate,
    LastPlayDate: the spec's four, distinct from the library's six): an unadvertised ordering is
    refused, not answered under another; `UserDefined` is among them.
  - `PlaylistCount`, `ActivePlaylist`, `PlaylistChanged` are diffed from the player properties' 200
    ms poll, against a listing re-read only when `Library::playlists_revision` moves. `moved` asks
    of every row: a playlist held under another name = `PlaylistChanged`; one that arrived or left =
    `PlaylistCount` announced, even where an addition and a removal in one sample leave the count
    equal (the spec has no signal for a playlist made or gone).
  - Neither seam read is `Library::playlists`: `Playlists::count` = `Library::playlist_count`, one
    `count(*)` over `playlists`; `Playlists::listing` = `Library::playlist_names`, id and name with
    no join, `GetPlaylists`' `index` and `maxCount` as `OFFSET` and `LIMIT`. Both query SQLite on
    the bus thread. The bus cannot ask for a narrowing, so the seam takes none.
