---
paths:
  - "crates/resonate-library/**/*.rs"
  - "crates/resonate-mpris/**/*.rs"
  - "crates/resonate-ui/src/views/playlists.rs"
  - "crates/resonate/src/playlists.rs"
---

# The catalog and its playlists

`resonate-library` is the local source's catalog and nothing else's. It walks directories, so it
stores paths and hands them back as local `MediaLocation`s; nothing in the schema is keyed by
source. A source that is not the filesystem brings its own catalog, and a queue row from one is read
through `Player::media` like any other unscanned row.

## Schema and grouping

- **The schema is `V1` and then `MIGRATIONS`, and a catalog is carried forward wherever it can
  be rather than thrown away.** A change to what the catalog holds is a new SQL step appended to
  `MIGRATIONS` — an `ALTER TABLE`, a new table or index, a rewrite of the rows the change moves —
  and never an edit to `V1` or to a step already written, because either strands every catalog
  stamped before it. The stamp in `PRAGMA user_version` is `fingerprint_after`: an FNV-1a over the
  `V1` text and then each step's in turn, taken at compile time, so every point in the history has
  a stamp of its own and `SCHEMA_FINGERPRINT` is the last. `lay_out` writes `V1` and every step
  where the stamp is `UNSTAMPED`, opens where it is this build's, and otherwise finds which prefix
  of the steps the stamp names and applies the rest in one transaction that restamps as it
  commits — a step that fails is `StoreOp::Migrate` and leaves the catalog, stamp and all, exactly
  as it was. Only a stamp no prefix names is `Error::SchemaMismatch`: a catalog written before the
  history began, or by a build with a history this one does not share, which is the one case left
  where the catalog is deleted and scanned again. A counted version was weighed and refused before:
  it cannot tell a build with the same count and a different schema from ours, so the stamp stays a
  hash. `the_first_schema_is_never_edited_where_it_stands` pins `V1`'s own fingerprint, so an edit
  in place fails a test that says to write a step instead, and `never_unstamped` keeps a schema
  that hashed to zero from reading as a catalog nothing has stamped. **Migrate wherever the rows
  can be carried**; break only where they cannot be — an index whose meaning changed in a way no
  SQL can recompute, say — and then say so in the step's absence rather than by editing `V1`. The
  history begins at `0ca1e683`, the `V1` that stood when the policy changed, so a catalog written
  by any build since opens and is carried forward.
- **What the transport was doing is three tables, because the rows, their order and the place each
  move at a rate of their own.** `resume` is a singleton row — the row the queue was on, the frame
  into it, whether it was shuffled and when it was taken. `resume_rows` is the queue itself, one
  row per position in the order it was *loaded*. `resume_order` is one row per position in the
  order it was *playing*, naming which loaded row sits there. `Keeping` decides which of the three
  to write, and the three are what let each be written alone: a queue that has not changed costs
  one `UPDATE`, a queue dragged into another order or reshuffled costs `resume_order` and not a
  URI, and only rows arriving or leaving rewrite `resume_rows`. `keep_place` names neither
  `shuffle` nor a row, so a place written five seconds into a track cannot lose the order the rows
  were kept under; `keep_order` writes the order, the place and the shuffle together, because a
  toggle moves all three.
  The rows carry `MediaLocation::to_uri` rather than a path, which is the
  one place the catalog stores something that is not the local source's: a queue row may come from
  any source the build registers, and `Library::track_played` keys on a path precisely because it
  means a *scanned* row while this does not. `span_start` and `span_frames` sit beside it under the
  same `store::span` convention `tracks` uses, so a cue row resumes as the cut it was rather than as
  the file it came out of. The order is what lets a shuffled queue come back playing as it played
  and unshuffle into the album it came from, and `resonate-core::plays_in` is the whole of how it
  is trusted: an order naming each row exactly once is taken as it stands and anything else — the
  wrong length, a repeat, a row the queue does not hold — gives back the order the rows were
  loaded in, so no reading of it can refuse a queue. A URI that no longer names a location does
  refuse the whole resumption rather than dropping a row, because a queue one row short is a queue
  whose kept position now names the wrong track. `keep_resumption` replaces the rows, the order and
  the place together in one transaction, so a queue is never half of one run and half of another,
  and writing no rows is how the queue is discarded — `resumption` answers `None` for an empty one,
  which is the same answer it gives before anything has played.

- **A favourite is when, not whether.** `tracks.favourite`, `albums.favourite` and
  `artists.favourite` are nullable nanosecond stamps, so the column that says a row is a
  favourite is also the column that orders the favourites by when they were marked, and a
  boolean would have bought nothing and cost a second column to sort on. `Library::favour` takes
  one `Favoured` — `Track`, `Album` or `Artist` — rather than three functions, because the three
  writes differ only in the table and a caller choosing between three names would have to know
  which. It answers whether the value moved: the write is guarded on the column standing the
  other way, so favouring a favourite keeps the stamp it was marked with — and with it its place
  in *recently favourited* — writes nothing and answers false, and so does an id no row holds. `is:favourite` is a `Shape` beside `is:hires`;
  `SortOrder::Favourited` needs `tracks_by_favourite` declared `favourite DESC, title COLLATE
  NOCASE`, exactly as its `ORDER BY` reads, while `AlbumOrder` and `ArtistOrder` need nothing,
  having no indexes by design.
- **A genre is the track's and its artist's at once, folded into one column.** `tracks.genre` is
  what the scan always read into `TagSet::genre` and dropped on the floor; the fourth
  `tracks_fts` column is that, folded through `folded_letters` and joined with the names
  `artist_genres` holds for the track's artist, so `genre:` is an ordinary scoped word against
  the index and reaches a file whose tagger named no genre at all. What it costs is that
  enrichment has to re-index: `land_artist` writes `artist_genres` long after the scan wrote the
  row, so `store::reindex_the_tracks_of` runs beside it, and `NAMED_TABLES` learned
  `artist_genres` or a landed genre would leave the spelling vocabulary stale.
- **A pinned playlist leads every order, and `undo.rs` is where that is easy to lose.**
  `playlists.pinned` is the same nullable stamp a favourite is, and `order_by` prefixes
  `CASE WHEN p.pinned IS NULL THEN 1 ELSE 0 END, p.pinned DESC` onto every `PlaylistOrder`, so
  pinning is one rule rather than five. It has to be threaded through `COLUMNS` and `read`, and
  then through `undo.rs`'s `Held`, `held_in` and `rewritten` — because an undo re-creates the
  whole row from what was held, so a column added to `playlists` and missed there is silently
  dropped the first time somebody walks an edit back. A pin is not an edit any more than a play
  is, so `rewritten` reads it off the live row through `unedited_in` beside `played` and `plays`,
  and only a playlist the walk re-creates from nothing takes the one `Held` kept — a pin made or
  taken off after an edit survives walking that edit back.
  `undoing_an_edit_keeps_a_playlist_pinned` and
  `a_pin_made_after_an_edit_survives_walking_the_edit_back` are the guards.
- **A ninth sort order did not fit.** `store::sort_code` packs a saved query's `SortOrder` and its
  `Direction` into the one `playlist_queries.sort` column as `order + READ_BACKWARDS`, and
  `READ_BACKWARDS` was 8 against exactly eight orders — so `SortOrder::Favourited` at code 8 would
  have read back as *Relevance, descending*. It is 16 now, which re-encodes every descending saved
  query and is therefore only safe because the fingerprint was breaking in the same pass.
- **What was listened to is three reads over `listens`, and nothing else is stored.**
  `listens.heard` is the nanoseconds of that visit actually listened to and `listens_by_time` is
  what every window reads off; `Library::statistics`, `most_listened` and `listening_by_day` are
  bounded by `listens.at >= ?` so the index serves each, and `most_listened` answers the three
  lists from one `read` so a pane costs one connection rather than three. A day is bucketed by
  dividing the stamp rather than by a calendar, because there is no date crate in the tree and
  one civil-from-days is cheaper than one; a day nothing was played on is written as a zero row,
  so nothing downstream has to draw around a gap. `most_listened` is free to sort on a
  `count(*)` precisely because it is its own read and not a `SortOrder` — which is why the
  Statistics pane can answer what was heard most this month and the tracks pane still cannot.
- **What a service has been told is a mark in the history, and the history is what is told.**
  `submissions` — the sixth step in `MIGRATIONS` — holds one row per `ListeningService`, the id of
  the last `listens` row that service has been told of, and `scrobble.rs` is the pass:
  `Library::submit_listens` reads the listens past the mark in id order, `SUBMITTED_AT_ONCE` — a
  hundred — at a time, joined to the track, its album and its artist for the names and the
  MusicBrainz ids a `Scrobble` carries, hands them to the `Scrobbler` and moves the mark past the
  batch in a `max` so it never goes back. A listen of a row naming no title or no artist is a
  `Submitted::unnamed` and passed over, because a service can file nothing under a blank name. A
  batch the service refuses as malformed — `Refused` with a 400 — is told again a listen at a
  time, so one bad row costs itself rather than every play after it, and a listen refused alone is
  `Submitted::refused` and passed over; any other failure moves the mark only past what was told
  and answers the error, so the rest is told on the next ask. A service with no row yet is marked
  at the last listen the history holds and told nothing — `Submitted::started` — which is what
  keeps a token given today from sending ten years of history. Nothing is written when a play is
  counted, so it does not matter which process counted it, and a listen whose track leaves the
  catalog before it is told leaves with it on the cascade. The claims are
  `a_service_is_told_what_was_heard_after_it_was_first_asked_and_each_play_once`,
  `a_play_the_service_refuses_as_malformed_is_passed_over_and_the_rest_are_told` and
  `a_play_a_service_could_not_be_reached_for_is_told_the_next_time`. Two runs submitting at once
  may tell the same batch twice; ListenBrainz keeps one listen per moment and name, so nothing
  guards against it.
- **A suggestion is a saved query with a name on it.** `suggest.rs` answers
  `Suggestion { name, reason, query, rows, length, pictured_by }`, and the query is written through
  `Display for Search` rather than as a literal, so a suggestion and what the search box would
  have parsed cannot drift — `every_suggestion_reads_back_through_the_grammar_it_was_written_in`
  is the claim. Saving one is `Library::save_query` and no new code at all, because a saved query
  playlist is already a thing that fills itself. Nothing is persisted until it is saved, and
  nothing is offered whose count does not clear `ENOUGH_TO_OFFER`, so a thin catalog offers few
  suggestions rather than a screen of empty ones. Two rules came from the real data rather than
  from the design: `names_a_decade` drops a genre that is only a decade, because MusicBrainz
  hands out `2010s` as one and it stood beside the decade built from `albums.year` saying the
  same thing worse; and `billed_as` capitalises a lower-cased genre after any mark and not only
  after a space, or `contemporary r&b` is billed *Contemporary R&b*.
  `Library::suggestions` answers an `Arc<[Suggestion]>` kept beside the search vocabulary and
  under a counter of its own: the same `update_hook` steps `written` for any row written in
  `tracks`, `albums`, `artists` or `artist_genres`, which are every table a suggestion is read
  from, so a reload that moved none of them — a settled search, a scroll, a playlist edited — is
  handed the kept answer rather than a pass of `measured` per candidate. It is per table rather
  than per name because a suggestion counts plays and favourites as well as names: a counted play
  is a `tracks` write and drops it where it leaves the vocabulary standing, and a catalog written
  by another process drops both through the data version, whatever table that process wrote. `length` is the same `measured` read's total, and
  `pictured_by` is `db::pictured_by`: the albums holding a picture that the search's rows fall on,
  most rows first, up to `PICTURED_BY_AT_MOST`, with an album whose picture — its vault key, or
  its bytes' length and first 256 bytes — another already stood for passed over, and then one
  whose picture merely *looks like* one already standing, so one sleeve saved at two resolutions
  is not two tiles. `store::the_picture_of!` is that identity, written once for the query and for
  the sweep. What a picture looks like is `resonate_codec::Likeness`: the cover averaged in linear
  light onto eight by eight cells and kept as their sRGB bytes, and two are alike where they
  differ by a root mean square of `ALIKE_WITHIN_A_ROOT_MEAN_SQUARE_OF` — 12 of 255 — where one
  sleeve at a quarter of its size measures about 1 and 3 re-encoded as JPEG, and a sleeve mirrored
  or a banner across its top about 80. A likeness costs a decode, so `likeness::of` keeps it in
  `likenesses`, the fifth step in `MIGRATIONS`, under the picture's identity — a picture that
  cannot be read is kept as `NULL` so it is not decoded again, and a cover that failed to reach
  the reader is kept as nothing at all and tried again — and `ORPHANS` takes away every row no
  album's picture names any more. The table is not one the `update_hook` counts, so weighing a
  cover never drops the suggestions it is weighed for. `Reason::kind` sorts a suggestion under a
  `SuggestionKind`, which is how the pane shelves them.
  `a_suggestion_says_how_long_it_runs_and_which_covers_picture_it` and
  `one_sleeve_saved_at_two_resolutions_pictures_a_suggestion_once` are the claims.
- **A share is built here because three callers want the same words.** `Library::shareable` reads
  the track, its album and the `release_track_links` and `album_links` rows, and
  `Shared::written` is a pure function over them — the identity, then one link. The link is
  `https://song.link/` with the service URL appended whole, which is the form the service
  answers 308 to and resolves to its own shortcode; the candidates are the `Relation`s
  `RELATIONS_SONG_LINK_TAKES` names crossed with `SERVICES_SONG_LINK_RESOLVES`, a recording's
  own links ahead of its release's and the providers weighed in the order they are declared, so
  one track shares identically twice running. Failing that it is the MusicBrainz recording, and
  failing that the text alone. It is not in the window because `resonate share` and anything
  else that shares must say the same thing.
- **Every order a pane offers is read off an index, and what the planner knows about the table is
  written after a scan.** `tracks_by_album` carries the trailing `title COLLATE NOCASE` the album
  order ends on, and `tracks_by_title`, `tracks_by_artist_name`, `tracks_by_added`,
  `tracks_by_duration`, `tracks_by_plays` and `tracks_by_played` are the other six `SortOrder`s,
  each declared the way its `ORDER BY` reads — the collation included, because no ordinary index
  serves a `COLLATE NOCASE` order unless it is declared that way, and the `DESC` included, because
  SQLite walks an index backwards only where the whole order runs one way and
  `plays DESC, title` does not. **A reversed order needs no index of its own**, because SQLite
  scans one backwards: `order_by` is a `Reading` of the natural spelling and its mirror — every
  term flipped, so `plays DESC, title` becomes `plays, title DESC` — and
  `db::tests::every_order_the_panes_offer_is_read_off_an_index_either_way_round` is the claim
  rather than the prose: it plans `SortOrder::ALL` crossed with `Direction::ALL` through
  `db::listing` and refuses a plan holding a temporary B-tree, which is what a full sort before
  the `LIMIT` lands reads as. What it costs is nine indexes on `tracks` written per row a scan
  stores where there were three.
- **The albums and artists panes have orders too, and deliberately no index behind them.**
  `AlbumOrder` is relevance, title, artist, year, track count and when a track of it was last
  added; `ArtistOrder` is relevance, name, album count and track count, and `album_order_by` and
  `artist_order_by` are the two counterparts to `order_by`. Neither is held to the index guard:
  those tables hold thousands of rows where `tracks` holds hundreds of thousands, so a temp
  B-tree over one is cheaper than an index would be — `SCHEMA_FINGERPRINT` covers the index list,
  so adding one is a step in `MIGRATIONS` and a rebuild of that index in every catalog it opens.
- **A saved query's direction rides in the column it already had.** `store::sort_code` writes the
  eight orders as before and adds `READ_BACKWARDS`, eight, for a descending reading, and
  `sort_of` reads the pair back out; `playlist_queries.sort` is untouched and a catalog written
  before this reads exactly as it did.
  `schema::restate_the_statistics` is the other half — `PRAGMA analysis_limit` and
  `PRAGMA optimize` on the writer once a scan has pruned — because with no `sqlite_stat1` the
  planner picks its join order from hardcoded guesses; and `configure` hands a connection a page
  cache and a 256 MiB memory map, where the 2 MB default had each pooled reader reading the pages
  it had just read back off the disk. **The cache is sized by what the connection is for**, because
  a page cache is per connection and `READER_POOL` is eight: `schema::Role::Writing` takes
  `WRITER_PAGE_CACHE_KIB` of 8 MiB, which is the one that batches inserts and maintains indexes,
  and `Role::Reading` takes `READER_PAGE_CACHE_KIB` of 2 MiB, so a library with every reader
  checked out bounds its page cache at 24 MiB rather than the 72 MiB one size for all of them
  allowed. It is a bound rather than a measured saving — SQLite fills a page cache lazily, so a
  small catalog never reached either figure — and it is the writer that has the working set worth
  keeping.
- **A reader is checked out and handed back, and `READER_POOL` is how many there may be rather
  than how many are kept.** `Inner::checkout` parks a connection where one is free, opens one where
  the pool is under the count, and otherwise waits on `Inner::freed` until a caller is done, so
  eight is the number of SQLite connections a 500k-track scan can have open at once rather than the
  number of callers at once. `Reader` is what hands one back: the connection goes home from its
  `Drop` rather than on the normal path alone, so a query that panics costs the pool nothing. The
  discipline that makes the wait safe is that no reader is taken while another is held — every one
  of `Inner::read`'s closures queries and returns, and a caller that needs two reads takes them one
  after the other — so a nested checkout can never be the thing the pool is waiting for.
- **A cue sheet claims the file it names, and the scan reads sheets before audio.** A `.cue` is
  not audio and is not in `AUDIO_EXTENSIONS`; it is a sidecar, so `directory_of` reads every sheet
  in a directory first, resolves each `FILE` against that sheet's own folder, and only then sends
  a probe job for the audio files no sheet claimed. That is what stops one FLAC being stored both
  as an album's worth of rows and as one whole-file row. A `FILE` naming anything outside the
  sheet's own folder is refused and logged, because that is what the format means and no ripper
  writes otherwise. Incrementality weighs the two mtimes apart rather than
  folding them: `tracks.modified` is the audio file's own and `tracks.sheet_modified` is the
  sidecar's, NULL where no sidecar cut the row. A row is unchanged only where both agree with what
  the walk found, so editing a sheet rescans the rows it cuts, and taking the sheet away reprobes
  the file and prunes the rows the cut no longer names *whichever* of the two was the later —
  where the later of the two was one stored number, a sheet older than the audio it cut left its
  rows standing when it went, the audio's mtime alone still matching what had been stored.
  `a_sheet_older_than_the_file_it_cut_is_still_missed_once_it_has_gone` is that claim, and it
  needs an mtime set by hand, a sheet written after the file it names being the newer of the two.
- **A sheet the file itself carries is the probe worker's to see, so the cut is decided there
  rather than in the walk.** A sidecar is visible from a directory listing and an embedded
  `CUESHEET` is not, so `read_candidate` probes and then cuts on `MediaInfo::cue` where it names
  audio tracks, sharing `cut_into_rows` with `read_cut` so the arithmetic is written once. The
  sidecar still wins, and it wins by the walk: `claimed` takes the audio file out of the audio pass
  before `whole_file_job` ever sees it, so the embedded sheet is never read for a file a `.cue`
  beside it already cuts. What that costs is a count the walker cannot take: one file is one
  `discovered` until the probe says otherwise, and `rows_past_the_first` is what the worker adds
  once it knows — the same number the incremental path adds when it sends one `Job::Known` per
  stored row, so `discovered` ends at the rows either way.
- **The walk reads what the catalog already holds once, and the roots are what bound the read.**
  `Known::under` takes `(path, id, file_size, modified, sheet_modified)` for the rows under each
  root being
  walked — one indexed range query per root, where a 500k-file tree used to interleave 500k point
  queries with the writer's own commits. It is a snapshot taken before the walker starts, and
  what makes a snapshot safe is that no path is walked twice in one scan: a root inside a root is
  refused, a directory reached through a second link is stepped past, and a sheet claims its file
  before the audio pass sees it. `Known::rows` is the whole of how it is read back — every stored
  row for a path, in `span_start` order — because a path is a cut file as readily as a whole one:
  unchanged means there is at least one row and every one of them matches the size, the mtime and
  the sheet that cut it,
  and `Candidate::existing` carries the lot so a probe that answers with N rows can claim the N
  that were there. What it costs is the paths under the roots being walked held in memory for the
  length of the walk.
- **One pass walks the tree at a time, and a second is refused rather than queued.**
  `Inner::walking` is the flag and `Walk` is the guard that holds it: `Library::scan` and
  `Library::organise` each take one in `start`, before the thread is spawned, and the thread owns it
  for its whole life, so it is handed back from `Drop` on a panic the way `Reader` hands back a
  pooled connection. Taking it never waits — a caller that finds the tree being walked gets
  `Error::AlreadyWalking` at once, because a pass that blocks for minutes is not a pass a window or
  a command line can start. What it protects is `Known::under`: the snapshot is taken before the
  walker starts, so an organise that commits a path rewrite after it and before the walker reaches
  that directory leaves the walker probing the file as new and the prune taking the rewritten row —
  its `id`, its `added`, its counts and its `listens` — away. Two scans at once are the same
  hazard and worse: each stamps its own `generation` and each prunes `WHERE seen != ?`, so the
  first to finish deletes every row the second wrote. `enrich` and `poll` are deliberately outside
  it, because neither walks the tree nor rewrites a path.

- **An album grouped by its folder is re-keyed to the folder it moved into, in place on the row it
  already had.** Only the third tier embeds a path, so only it can be left naming a folder that has
  gone; one file of such an album re-probed later would be keyed onto the new folder, insert a
  second `albums` row and take its tracks with it, and once nothing pointed at the old row `ORPHANS`
  would sweep its cover, its `mbid`, its `release_group`, its `release_tracks` and, through the
  cascades, its `wants`. `organise::re_key_the_sleeves` runs in a transaction of its own once every
  batch has landed: it takes the albums the moved files name, keeps the ones
  `store::is_keyed_by_its_folder` answers for, and writes `store::sleeve_key` of the row's title and
  the folder its tracks now share. It changes no membership: `tracks.album_id` references
  `albums(id)` and nothing joins on a key, so rewriting the key on the row that already holds it
  orphans nothing. The folder is `scan::sleeve` of each track read together — the same reading the
  scan would take, disc folders and all — so an album whose tracks landed in more than one folder,
  or any of whose tracks sits directly in a root, keeps the key it had and says so in a debug
  record rather than guessing. An album `album_keys` names more than once is left alone for the
  same reason, and it is the same case: an album gathered onto a release is named by the key of
  every folder it was gathered from, so its tracks do not share one folder either.
  `album_keys.key` is a primary key, so an album moving into a folder another album already names
  keeps its old key too: merging two albums *by where they landed* is a decision nobody asked for,
  where merging two that turn out to be one release is one the pass can make on evidence.
  **What the re-key leaves alone, a probe keeps.** A whole rescan, or any probe of a file whose
  size or mtime moved, computes the new folder's key afresh, and that key naming another album —
  or nothing — used to file the track there and let `ORPHANS` take the album it left, its release,
  cover and wants with it. `store::kept_where_it_was` answers first: where the row already belongs
  to an album of the same title, none of its other names finds an album, and its folder's key
  names another album or none, the row stays where it is and `lend_the_free_names` gives the
  album whichever of its keys nobody holds. `a_whole_rescan_after_two_albums_land_in_one_folder_keeps_each_the_album_it_was`
  is the claim.

- **A row names what it is billed to, because a listing beside it is narrowed and capped.**
  `ALBUM_COLUMNS` reads the artist's name by id — `(SELECT r.name FROM artists r WHERE r.id =
  a.artist_id)`, a correlated scalar beside the three that already count an album's tracks, its
  distinct track artists and what its release is missing — so `Album::artist` sits on the row next
  to the `artist_id` that names it, and `ArtistDetail::name` does the same for the artist pane. It
  is a subquery rather than a `JOIN` because `ALBUM_COLUMNS` is read by `Library::album` and
  `Library::albums` against `FROM albums a` plus a `scoped.from` that varies, and a join would be
  written twice and could collide with the aliases `Matching::grouped` brings. What it replaces is
  the window resolving a name through `LibraryModel::artist_names`, a map built from the artists
  *listing* — which `ArtistQuery` narrows by the typed text and caps at `PAGE` — so the name went
  missing exactly where the search was doing its job, and an album cell drew its year alone while
  the scoped heading's artist line vanished and the artist heading fell back to the literal
  `artist <id>`. `browsed` reads the scoped album by id the way it already reads the release, its
  tracks and the artist's detail, so `LibraryModel::album_of` answers for the album being scoped to
  whether or not the listing holds it; the map and its getter are gone, having lost every caller.

- **A scan says what went wrong with a file, not merely that something did.** `ScanStats::failed`
  is a `Failures` of four counts rather than one number, and `Failure` is what decides which:
  `Unnamed` for a path that is not UTF-8, which is counted before anything is opened; `Misnamed`
  for `UnrecognisedContainer` and `NoAudioTrack`, which is the bytes not being the container the
  extension promised; `Undecodable` for a container this build reads holding audio it cannot —
  `NoDecoder`, `DsdCompressed` and the properties it cannot represent; and `Unreadable` for
  everything else, `Io` and the `Symphonia` residual being where a corrupt file lands. The match
  on `resonate_codec::Error` is exhaustive and the enum carries no `#[non_exhaustive]`, so a
  variant added to the codec fails to compile here rather than falling quietly into whichever
  count a catch-all named. It is worth telling apart now that `AUDIO_EXTENSIONS` advertises
  nothing undecodable: what reaches the tally is a file whose extension promised a container its
  bytes are not, which is a retagging or a bad rip rather than a format this build declined. Both
  presenters print the total and append only the counts that are not zero, so a clean scan reads
  as it always did.

- **A root may not be inside a root, and a wider one takes in what it covers.**
  `store::register_root` is the one way a root is written, so `Library::add_root` and the scan's own
  `roots` refuse and absorb alike: a path inside a registered root is `Error::RootInsideRoot`, and a
  path containing registered roots re-parents their tracks onto itself and drops those rows from
  `roots`. Re-parented rather than deleted, because `tracks.root_id` cascades and widening a root
  must not cost a play counted against a track under it. Nesting was never only untidy: the overlap
  was walked twice with `root_id` flipping on the second upsert.
- **Every walk hazard but a lost worker is stepped past.** A directory past `MAX_DEPTH` is warned
  over and skipped the way an unreadable one is, rather than failing the scan and taking the prune
  with it. A symlink is weighed only where it names a directory, and one naming a directory this
  walk has already been down is stepped past rather than read as a cycle, so two albums linked to
  one shared folder walk it once instead of aborting the scan — the set is what a cycle runs into
  on its second pass through the same link, so skipping still terminates. What is not stepped past
  is a probe worker that panicked: `run` joins every one of them and answers `Error::ScanStopped`
  before the prune, because a scan whose counts are short would prune the rows it never reached.
  A commit that fails drops the result channel's receiver before anything is joined, so the probe
  workers blocked sending into it wake to a closed channel, their exit closes the job channel under
  the walker, and the scan answers the store's error and hands the `Walk` guard back rather than
  waiting on a pipeline nothing is draining.
- **Every root is tidied by a scan and only the walked ones are pruned, and a root that is not
  there is neither.** The prune is `WHERE seen != ?` over the roots the walk stamped, so
  `resonate scan <root>` used to leave every other root holding rows for files that had gone —
  the catalog was honest only about what had just been walked. `tidy_the_roots_beside` is the
  other half: for each registered root this scan did not walk it reads the distinct paths under it
  through `store::paths_under`, keeps the ones that are no longer there and hands them to
  `store::forget_paths`, and `store::sweep_orphans` — the batch the prune already ran, lifted out
  of it — runs once at the end where anything went. It costs one `stat` per path of a root nobody
  asked about, which is why it is a tidy rather than a walk: no file is opened, no tag is read and
  nothing new is found. A bare `resonate scan` walks every root, so it has nothing beside to tidy.
  `is_there` is the guard over both: a registered root whose directory is not there is dropped
  from the bare scan's walk and stepped over by the tidy, because an unmounted drive is not an
  empty one and pruning it would take every play counted under it. A root named on the command
  line is still `Error::RootNotADirectory` where it is missing, that being a thing somebody asked
  for rather than a thing found in the table. A scan the window's watch asks for is the other
  kind: `Library::scan_what_is_held` walks only the roots it names that `roots` still holds and
  that are there, registers nothing, and settles them before the thread starts — answering `None`
  where none is left — so a drive unplugged after its root was queued costs the roots queued
  beside it nothing, and the window takes a root off what it owes only once a scan has taken it.
- **A track is keyed by `(path, span_start)`, not by path.** N cue rows share one path, so the
  `UNIQUE` is on the pair and `span_start` is `NOT NULL DEFAULT 0` rather than nullable — SQLite
  treats NULLs as distinct in a unique index, which would let one file insert twice. Every lookup
  that means *this row* takes the span beside the path: `Library::track_at` and
  `Library::track_played`, the latter because keying a play on the path alone counts every track
  of an album against one row. A playlist entry stores a path alone, so the three joins reaching
  `tracks` from `playlist_entries` take the row with the lowest `span_start` — without that the
  join multiplies one entry into one row per cue track and inflates a playlist's count and length.
- **A `Track` names its artist by id as well as by name.** `tracks.artist_id` is the column the
  artists listing has always counted against, and `Track::artist_id` is that column read back
  beside `album_id`, so a row says who made it rather than only what they are called — which is
  what lets the window open an artist from the row playing without weighing a name against a
  listing that a search may have narrowed. It is the last name in `TRACK_COLUMNS` on purpose:
  `BESIDE_A_TRACK` counts that list, so appending leaves every joined read's own columns where
  they were.
- **Each MusicBrainz id is weighed against the column that holds one of its kind.** `tracks.mbid`
  is the *recording* id, because that is what `TagSet::musicbrainz_track_id` carries: a tagger
  writes it in `MUSICBRAINZ_TRACKID` for a Vorbis comment and in the `UFID` frame MusicBrainz owns
  for ID3, and `release_track_mbid` is `MUSICBRAINZ_RELEASETRACKID` beside it, which names the
  track's place on one release rather than the recording it is of. The two are different
  identifiers for different things, so `rematch_release_tracks` weighs each against its own — the
  release row's `recording_mbid` against the first column and its `track_mbid` against the second —
  where one column weighed against both could only ever have paired the second by accident.
- **An artist is keyed by the fold of its name, so one spelling is one artist.**
  `store::folded_letters` lowercases, decomposes and drops the combining marks, and spells out the
  letters Unicode does not decompose — `ł`, `ø`, `đ`, `ð`, `þ`, `ß`, `æ`, `œ`, the dotless `ı` and
  the rest — so
  *Marcin Przybyłowicz* and *Marcin Przybylowicz* are `artists.key` `marcin przybylowicz` either
  way, where `name.to_lowercase()` made them two artists with two listings, two portraits and two
  halves of a discography. It is not `enriched::folded_title`, which keeps its marks: that one
  weighs a MusicBrainz title against a tag, where a mark is evidence, and this one gathers
  spellings of one name, where a mark is noise. `enriched::stripped_title` is the two read
  together — the letter fold put through the title fold — and it is what the enrichment falls back
  to below. **Which spelling is billed is the one that carries
  the marks**, counted by `marks_in` and applied on the cache hit as well as the row, so a scan
  that meets the stripped spelling later does not undo the accented one, and the display name
  settles rather than following the scan order. `folded_letters` is exported from the crate for
  that reason: `resonate missing --artist` and the window's type-ahead weigh a typed name through
  it, so a name spelt either way reaches the artist the catalog files under the marked spelling.
- **A catalog keyed before the fold is folded back when it is opened, not when it is next
  scanned.** `store::reconcile_artists` runs once in `Library::build`, after `schema::lay_out`:
  it reads every artist, groups them by `folded_letters` of the *name*, and leaves a group alone
  where its one row is already keyed by its own fold. Where it is not, one row is kept — the one
  carrying an `mbid`, then the lowest id, because that is the row the enrichment, the portrait,
  the genres and the links hang off — the rest hand over their tracks, albums, genres, links and
  the releases kept for them through `store::take_over_artist`'s `UPDATE OR IGNORE` and are
  deleted, and the survivor is rekeyed and renamed to the most
  marked spelling in the group. It needs no sentinel key to avoid colliding with a row it has not
  reached yet: every key in the table is either a fold or a `to_lowercase` of the same name, and
  folding is idempotent, so two rows whose keys could collide always fold into the same group.
  It is an invariant the catalog keeps rather than a migration it ran, which is why there is no
  schema step for it and why running it twice is a no-op.
- **An album is whatever a grouping key names, and one album may be named by several.** `album_keys`
  is the table — a key is its primary key and an album may hold any number of rows — which is what
  makes a grouping a *name* for an album rather than a property of it. `store::album` reads it
  rather than upserting on a column, so a scan that computes a key an album already holds fills
  that album and a key nothing holds makes a new one. `ORPHANS` is unchanged, and the keys of an
  album nothing points at go with it on the cascade.
- **One song held more than once is listed once, as the best copy.** `alternatives.rs`
  runs at the end of every scan: it groups the rows by the fold of the album title, the
  album's owner or else the track's artist, the disc, the track number and the title — the album's
  *title* rather than its row, so two folders of one album still meet — and inside a group
  gathers the rows whose lengths are within `THE_SAME_LENGTH_WITHIN`, two seconds, of one
  another, or which both have no length at all. An album is named both ways a row can be billed
  under — the release title the enrichment gave it and the title its tags gave — and two rows
  sharing either are one song, so a copy MusicBrainz billed as *Meddle* meets an untouched copy
  tagged *Meddle* although its own tags said *Meddle (Remastered)*; `one_song_apiece` joins the
  names through a union-find. A row with no title or no artist names nothing, because a loose
  *Intro* with nothing else on it is not evidence of being any other *Intro*. The best of a
  gathering is lossless over lossy, then the wider word, then the higher rate, then the higher
  bitrate, then whichever row the catalog held first; every other copy names it in
  `tracks.alternative_of` — *whatever* its format, so two identical rips in two folders are one
  row with a `+1` rather than a duplicate, and a copy in the best copy's own format is hidden
  beside one in another. The row's menu tells two copies of one kind apart by the folder and file
  each is in. The copies are kept whole — their plays, their playlists, their
  files — and only the listings step past them: `scoped` filters every track listing and saved
  query on `+tracks.alternative_of IS NULL`, the unary plus keeping that term off
  `tracks_by_alternative` so the order indexes still lead, the album and artist counts count the
  best copy alone, and an album whose every track is another copy's alternative leaves the albums
  pane. `Track::alternatives` is how many copies a row stands for, which the tracks pane draws as
  `+N` beside the title, and `Library::alternatives_of` is what the row's menu offers to play
  instead. A best copy that goes puts `ON DELETE SET NULL` on the rows under it and the pass at the
  end of the scan crowns the next.
- **A hidden track is kept and only stepped past.** `tracks.hidden` is the second step in
  `MIGRATIONS`, and `Library::hide_track` sets it either way and answers whether it moved. The row,
  its file, its plays, its playlists and its favourite are all kept, and a scan never touches the
  column, so a hidden track stays hidden however often its root is read again. `scoped` adds
  `tracks.hidden = 0` beside the best-copy term, and the album and artist counts and
  `HOLDS_A_BEST_COPY` weigh it the same way, so a hidden row leaves the listings, the counts and an
  album holding nothing else. `is:hidden` is a `Shape`, and a search that *insists* on it — a
  clause of that one alternative, not denied — lifts the visibility term, which is the one way a
  hidden row is listed again; `Clause::insists_on` is that reading, and `-is:hidden` or an
  alternative beside it lifts nothing. `Track::hidden` is what the row's menu reads to offer *Hide
  from library* or *Show in library*.
- **A track names its album every way it can, and joins the album the first of those names finds.**
  `grouping_keys` answers a *run* of keys in precedence order rather than one. A MusicBrainz
  release id is on its own and nothing else is written beside it, so two releases sharing a title
  and an artist stay two. Otherwise an `ALBUMARTIST` the tagger wrote, on anything not flagged a
  compilation, is one name, and the folder the track sits in — `TrackRecord::sleeve` — is another,
  with `album_key(title, owner)` as the fallback where a track has neither. A track joins the album
  the first of its names already finds and then lends the album the rest, which is what makes both
  gatherings hold at once: an `ALBUMARTIST` gathers discs the folders keep apart, and a folder
  gathers an album its files bill to different owners — the Cyberpunk 2077 soundtrack, whose three
  album artists used to make three albums under one sleeve, because the owner outranked the folder
  and the loser had nowhere to go. Every key carries the title, so nothing gathers two albums that
  are not called the same thing. Where two of a track's names find *different* albums the first
  wins and the second is left where it is, which is the reading it has always had rather than a
  merge nobody asked for — the pass gathers on a release, and only on a release. Where the tracks
  under one album disagree about the album artist the album keeps none, which is what the
  `COMPILATION` flag already meant, and `Album::artist_count` is what lets a pane draw that as
  *Various artists* rather than as a bare year.
- **A sleeve is a folder that was made to hold an album, which is why it is not simply the parent.**
  `scan::sleeve` answers `None` for a track sitting directly in a root, because a root holding loose
  files is a dumping ground rather than an album and two albums sharing a title in one are still two
  — the third tier then falls back to the track artist, as it always did. A folder named `CD2`,
  `Disc 3` or `disk-01` is read as one disc of a set and answers with its parent, so a set filed
  that way is one album without an `ALBUMARTIST` to say so.
- **The word a disc is filed under is read in ten spellings, and the longest match is the one
  taken.** `SPELLINGS` carries `cd`, `disc`, `disk`, `disque`, `disco`, `dysk`, `platte`, `schijf`,
  `skiva` and `диск`, so a set filed `Disque 2`, `Disco 3` or `Платте`-style in digits gathers the
  way `CD2` always did. The match is by the *shortest remainder* rather than the first hit, which
  is what the longer spellings cost: `disc` is a prefix of `disco`, so a first-hit walk would strip
  `disc` from `disco 2`, find `o 2` with no separator in front of it and read no disc at all.
  `Discovery` and `Disconnected` are still albums of their own, because the longest spelling they
  match leaves no separator either.
- **A disc numbered in words is the same disc, and `ONES`, `TEENS` and `TENS` are the tables that
  say so.** `disc_in_folder` reads the number on either side of the word it numbers: a cardinal
  after it — `Disc One`, `CD Two`, `disk_three` — and an ordinal before it — `Second Disc`,
  `First CD`. The three tables compose rather than run on, so a tens word joined to a ones word is
  read as the number it spells — `Disc Twenty One` after the word and `Twenty-First Disc` before
  it — and ninety-nine is where it stops, a set past that being filed in digits by anyone who has
  the patience to file it at all. A word form wants a separator between the two
  words, which is the whole of what keeps `Discovery` and `Disconnected` albums of their own where
  a bare prefix match would have made `Discone` a disc, and is what makes `Twentyfirst Disc`
  nothing; the digit form does not, because `CD1` is how half of them are written. `a_word_run_together_with_the_one_beside_it_names_no_disc` is that
  claim, and `scan::disc_in_folder` having two callers means `{disc}` in an organise layout reads
  a set filed this way the same as `organise::disc_of` always read `CD1`. What it costs is a key
  format change: a set already scanned under `Second Disc` is named by a key naming that folder,
  so it stays two albums until the catalog is deleted and scanned again.
- **A number word in another language is a flat table and goes no further than twelve.**
  `ELSEWHERE` is `numbered_after_the_word`'s last resort — sixty-eight spellings of one to twelve
  in French, Spanish, Italian, German, Dutch and Portuguese, each marked and unmarked where the
  two differ, so `Disc Un`, `CD Dos` and `Disco Zwölf` read as the discs they name — and
  `ORDINALS_ELSEWHERE` is its twin before the word, the same six languages' ordinals of one to
  twelve in each gender a folder is likely to be named in, so `Zweite CD`, `Deuxième disque`,
  `Primera-CD` and `Tweede Schijf` do too. Both are flat rather than composed, because
  composition — `Disc Twenty One`, `Twenty-First Disc` — is an English idiom with tables of its
  own, and a set past twelve is filed in digits, which every language already reads.
  `named_before_the_word` weighs the English reading and the flat one side by side and takes
  whichever leaves a separator and a disc word behind it, because an English ordinal is a prefix
  of some of theirs: `second` begins `Secondo Disco` and leaves `o disco`, which names nothing,
  where `secondo` leaves the disc. The flat table is read at its longest match for the same
  reason — `primer` begins `primera`. A set already scanned under `Zweite CD` is keyed by that
  folder, the way one under `Second Disc` was, so it stays one album a disc until it is scanned
  again from nothing. What it
  cannot do is tell one language from another: a folder is a string, so a word that numbers in one
  language numbers here whatever the rest of the name is in.
- **The key format is what `album_keys.key` holds**, so changing it means an existing library reads
  back under keys nothing matches any more and wants a rescan; `ORPHANS` is what takes the albums
  nothing points at any more away.
- **Two albums that turn out to share a release are gathered, and both their keys name what is
  left.** A release id the *pass* finds arrives after the grouping was made, so until it lands two
  albums scanned under different keys — a rip split across two folders, a set whose halves declare
  different owners — each held the same release and the same rows. `gather_under` runs inside
  `land_release`, after the release columns are written and before the wants are read: it finds
  every other album holding that `mbid` or already named by `release_key`, hands each one's tracks,
  release rows and *keys* to the album being landed, fills what the survivor holds nothing of — the
  cover, the year, the declared count, and the owner where the two agree — and deletes it.
  What makes it stick is that the keys move rather than being rewritten: the next scan computes the
  same key each folder was grouped under, finds it in `album_keys` naming the gathered album, and
  fills that album rather than making the row again. A single `key` column could not do that, which
  is why re-keying used to be refused outright.
  `gather_under` then names the survivor by `release_key` as well, so a file later tagged with the
  id lands on it through the first tier. Nothing joins on a key — `tracks.album_id` is the only
  membership there is — so the move is three `UPDATE`s and a `DELETE` whose cascade takes the
  loser's links and media.
- **An album takes the cover and the year any of its tracks carries, not the first one's.**
  `Cache::covered` holds the albums that hold a picture rather than the albums that were asked, so
  `store::cover` answers whether one landed and is asked again for the next track until one does;
  it reads `cover_art IS NOT NULL AND cover_source = 0` first, so an album already holding the
  file's own picture costs a query and no probe, while one holding a picture the archive gave —
  `CoverSource::Archive`, code 1 — is probed again and the file's replaces it, because the file's is
  the deliberate one and the archive's was only ever a stand-in. The year is the same shape: `Grouped` carries it, and a cache hit
  whose album has none runs `fill_year` rather than skipping the upsert that would have coalesced
  it. `store::year` reads a date written with no separators too, so `19750601` and `197506`
  name 1975 where only `1975-06-01` and `1975` used to.
- **A file is asked whether it has a picture on the open its tags came off; the picture itself is
  still read at commit.** `probe_pictured` under `Picturing::Whether` answers the `MediaInfo` and
  whether there is a picture off one open, and `TrackRecord::embeds_a_picture` is what the answer rides on, so `store::cover` opens a
  file again only where there is something to find. What that takes away is the old price — one
  extra `probe_cover_art` per track of an album that embeds none, and it really was every track of
  it, because an album with no cover never sets `Cache::covered` and so asks again at the next
  row. An album that *does* embed one still costs one extra open, at its first
  committed row, and that is deliberate: carrying the bytes out of the probe instead was measured
  and rejected. Copying a picture per file and claiming one per album key took a 1 200-track scan
  of 120 albums from 41 MiB of peak RSS to 111 MiB — a shelf of one cover per album stays alive
  from the moment a worker probes it until that album's row commits, and a `BATCH` of a thousand
  rows means nearly all of them are alive at once — to save 120 opens out of 1 320. Bytes held
  across threads to save an open is the wrong trade; the `bool` is the whole of what was worth
  carrying.
- **A play is a count, a date and a row of its own.** `Library::track_played` writes all three in
  one transaction: `tracks.plays` stepped, `tracks.played` set, and one `listens (track_id, at)`
  row stamped with the same nanos, so the two columns stay the cheap answer every listing already
  reads while the history is there to be asked a question the columns cannot answer. `listens`
  hangs off `tracks(id) ON DELETE CASCADE` and `configure` has `PRAGMA foreign_keys = ON`, so
  forgetting a root takes the plays counted under it away with the rows — which matters because
  SQLite reuses a deleted `tracks.id` and an orphan would be re-attributed to whatever was
  rescanned into its place. The count is what a pane draws and the history is what `plays:@`
  narrows on; nothing orders on the history, because an order is an index and a correlated
  `count(*)` is not one.
- **A file that moved is followed, not forgotten and found again.** A rename or a move by hand —
  anything but `organise`, which rewrites the rows itself — reads to a scan as a row whose file
  has gone and a file no row names. `moves::follow_the_moved` runs before the prune and pairs the
  two: a whole-file row, the only row at its path, whose file is not there, with a row this scan
  added — `added` at or past the generation — that is alike in `file_size`, `duration`, `codec`,
  `tagged_title` and `tagged_artist`. Where more than one on either side is alike — two identical
  rips moved at once — `moves::told_apart` weighs each gone path against each new one by how many
  names they share from the end, the file's own and then its folders', and pairs two only where
  each is the other's one best and that best shares at least the file name: `vinyl/echoes.wav`
  and `tape/echoes.wav` filed under `filed/` are each followed to their own folder, while the
  same two moved to `c/` and `d/` share the file name alone with both and are left to be
  forgotten and found rather than guessed between. A pair is
  followed through `organise::files_moved`, so the new row is dropped and the old one takes its
  path, its root and the generation, keeping its id, plays, listens, favourite, enrichment, vault
  object, playlist rows, kept lyric and queue row. The album is `settle_the_album`'s: where the
  album the scan made for the new folder holds nothing else and every row that moved into it came
  out of one album, it is gathered into that album through `enriched::gather`, so the new folder's
  key names the album the rows always had and a release, a cover and a favourite are not left
  behind; otherwise the moved row joins the album the scan filed it under. A cue-cut file is not
  followed, its rows sharing a path. `ScanStats::moved` counts the pairs and `added` leaves them
  out. `a_file_moved_between_scans_keeps_its_row_its_plays_and_its_place_in_a_playlist`,
  `an_album_moved_into_a_folder_of_its_own_stays_the_album_it_was` and
  `two_files_alike_in_every_way_are_told_apart_by_the_folders_they_moved_with` are the claims.
- **A track's count is the catalog's and a rescan leaves it where it stands.** `tracks.plays` and
  `tracks.played` are absent from the upsert's `DO UPDATE SET` the way `added` is, so a rescan keeps
  both; forgetting a root drops the rows and the counts with them. It is kept against the path, so a
  file no scan has seen counts nothing — `Library::track_played` answers `None` rather than
  refusing, because a location that is not local has no row here, and answers with the row it
  counted where there was one, read back inside the same transaction so the caller has the count it
  now stands at without a read of its own. `plays:` and `played:` narrow on
  the two columns and `SortOrder::Plays` and `Played` order on them, so *Top 25 most played* is a
  saved query rather than a feature, and the count is drawn through `listing::times` wherever a row
  names a track — the tracks pane, the queue, an opened playlist and the playlists index, which is
  where those words started.

## Enrichment

- **What a reference says lands beside what the scan read, in columns the scan never rewrites.**
  `artists` carries `mbid`, `sort_name`, `kind`, `gender`, `country`, `area`, `began_in`, `began`,
  `ended`, `has_ended`, `disambiguation`, `portrait`, `portrait_format`, `asked` and `answered`;
  `albums` carries `cover_source`, `mbid`, `release_group`, `release_title`, `date`, `country`,
  `label`, `catalog_number`, `barcode`, `kind`, `disambiguation`, `asked`, `asks` and `answered`;
  `tracks` carries
  `mbid`, `artist_mbid`, `release_track_mbid`, `isrc`, `tagged_title`, `tagged_artist`,
  `release_title`, `asked`, `asks` and `answered`. Beside them are `release_tracks`, one row per
  track of the release with `release_tracks_by_album` and `release_tracks_in_order` over it,
  `artist_genres`, the three link tables `artist_links`, `album_links` and `release_track_links`
  — a `relation`, a `provider` and a `url`, the first two as codes through `store::relation_code`
  and `service_code` and read back through `relation_of` and `service_of`, which refuse a code
  from a later build with `Error::UnknownLinkCode` — `artist_releases`, the discography bullet
  below, and `wants` and `lyrics_kept`. Of all of that the scan's upserts touch the ids, the two
  `tagged_` columns and what the tags *declare* about a release — `barcode`, `catalog_number`,
  `label` and `albums.tagged_tracks`, which is `TRACKTOTAL` — `release_media` being the one table
  beside `release_tracks` that a landing writes and a scan never sees: `coalesce(excluded.mbid,
  albums.mbid)` on an album, `coalesce(excluded.mbid, artists.mbid)` on an artist with
  `fill_artist_mbid` for one the cache already held, `store::Declaration` coalesced the same way
  on an album — the file's where it names one, what is held otherwise, which is the rule `mbid`
  and `release_group` follow — with `fill_declared` for one the cache already held, filling only
  what `Declared` says the row is still without, so a file that stopped carrying its barcode
  leaves the one held, which `a_scan_stores_what_the_tags_declare_about_the_release` and
  `a_rescan_keeps_a_barcode_the_file_no_longer_carries` are the claims of — and `tracks.mbid`,
  `artist_mbid`, `release_track_mbid` and `isrc` taken from the tags wherever the file names one,
  and kept where it names none unless the file was retagged — the same weighing of the two
  `tagged_` columns the names take — because those four are also what a lookup and a pairing
  *find*, and a whole rescan used to write the file's silence over a recording a search had
  identified, which the lookup would not ask about again for a month;
  `a_whole_rescan_keeps_the_recording_a_lookup_identified_a_file_by_until_it_is_retagged` is the
  claim. An *answered* row whose file is unchanged — the same size, mtime, sheet mtime and span —
  keeps all four and its `track_number` and `disc_number` as they stand whatever the file says,
  because the one write that replaces what a file carried is a listener taking the name its audio
  was heard as, which `analysis.md` has, and a rescan of the same bytes putting the file's word
  back would undo the gesture; a lookup otherwise only fills, so for every other answered row the
  file's value and the held one are the same. So a rescan leaves every other enrichment column where it stood, which
  `a_rescan_leaves_every_enrichment_column_where_it_stood` in `tests/library.rs` is the claim of —
  the one exception being the retagging rule below, which is the only place a scan writes
  `tracks.answered`. The four declared columns are what the ask is built from: `asking_albums`
  hands `catalog_number`, `tagged_tracks` and the owner's `artists.mbid` to `AlbumToAsk` beside
  the barcode, so a search names what the tagger knew about the pressing. It is still one `V1`,
  edited where it stands.
- **`tagged_title` and `tagged_artist` are what the *file* was read as, and they are the whole of
  how a rescan tells a retagging from an identification.** An enrichment writes `tracks.title` and
  `tracks.artist`; the scan writes those two columns as well, so without a record of what the file
  said the next rescan would put the tagger's spelling back over the reference's. The upsert
  therefore keeps an answered row's `title`, `artist` and `artist_id` where both `tagged_`
  columns come back unchanged, takes the file's where either has moved, and sets `answered` to `NULL` in that same
  `CASE` so the row is asked about again — which
  `a_rescan_keeps_an_answered_tracks_names_unless_the_file_itself_was_retagged` is the claim of.
  A row that has never been answered takes the file's names as it always did. What the two columns
  hold is what `scan::name_from_stem` left in the `TagSet`, not the tag alone: a file naming no
  title in its tags is read through `stem.rs` first, so `tagged_title IS NULL` means neither the
  tags nor the file name said anything, and `title` is then the bare stem `store::title` falls back
  to. `store::index_row` is shared between the scan and `land_recording` for that reason — a
  corrected title is what `tracks_fts` holds a moment later rather than at the next scan — and
  the upsert answers the title, artist and artist id it *left* on the row, so a rescan indexes
  those rather than the file's; `a_rescan_indexes_and_bills_the_names_the_lookup_kept` is the
  claim.
- **The seam is `Reference`, and `Library::enrich` is the pass that walks it.** It speaks in
  `Mbid`, which is the dashed lowercase text or `resonate_core::Error::NotAnMbid`, and `Isrc`,
  which is the twelve characters shouted and stripped of their dashes or
  `resonate_core::Error::NotAnIsrc`, each a shape the type refuses to hold anything else in — both
  in `resonate-core` now, because a provider crate names them and may not see the library, and
  re-exported here. `reference.rs` carries the rest of the vocabulary — `Release`, `Medium`, `ReleaseTrack` and
  `Credit`; `Wording`, which is `Phrase` or `Words` and says how a search is put; `ReleaseAsked`
  and `ReleaseMatch`, the second carrying the hit's `group` and its whole `credit`; `Recording`,
  `RecordingRelease`, `RecordingAsked` and `RecordingMatch`; `ReleaseGroup`, `GroupRelease`,
  `GroupAsked` and `GroupMatch`; `ArtistProfile`, `LifeSpan`, `Genre`, `ArtistRelease` and
  `ArtistMatch`; `Link`, `Relation` and `Service`, which are core's and re-exported; and `LookupOp`, the seventeen things a service
  can be asked for, twelve of them a reference's and the rest the lyric provider's, the correction
  source's, a recogniser's and a scrobbler's — and the `Reference` trait itself, whose thirteen methods answer `Option`s
  and `Vec`s in that vocabulary and nothing about how they were reached. `enrich.rs` is the pass:
  a thread named `resonate-enrich` behind an `EnrichHandle`, with `EnrichProgress` counting
  albums, releases, matched rows, covers, tracks, the tracks a lookup renamed, artists,
  portraits, the releases found for an artist and refusals and answering `is_cancelled` between
  requests,
  and `EnrichSummary` carrying the stats, whether it was cancelled and `stopped_by`.
  `EnrichOptions` is `refresh`, which asks again about
  what was answered, `at_most`, which caps the albums, the tracks and the artists each to that
  many, and `sought`, the cell below. The rematch-only albums are walked first, so a run with a
  cap set still pairs every album's rows; what is left is one queue of `Ask`s — the due albums,
  then the due tracks, then the due artists — walked in order, albums first because an album that
  lands retires every track under it before the track pass reaches one.
  `crates/resonate-library/tests/library.rs` drives the
  whole of it through a `Fake` that answers canned releases, recordings, groups and profiles and
  faults on the call it is told to, which is where the rules below are proved.
- **A picture is fetched beside the pass, never in it.** `Pictures` is two threads named
  `resonate-pictures-<n>` reading a bounded channel of `Picture`s — a cover, a release group's
  cover or a portrait — and `Pass::want` is the whole of how one is asked for; `Pictures::rest`
  drops the sender and joins them before the summary is taken, so what the stats say is what
  landed. The archive and Wikimedia Commons are not MusicBrainz, and `Client::pace` keeps a slot
  per host, so a picture costs the pass nothing but the handover while the next MusicBrainz
  request waits out its second. A picture that cannot be fetched is a warning and a `refused`
  count rather than the end of a pass — the pass finds out for itself at its next request — and
  where no reader thread started, `want` fetches on the spot. A call to a picture therefore has no
  place in the order the pass asks in, which is why `asked_in_order` leaves it out of the
  sequences the tests assert, and
  `a_picture_is_fetched_beside_the_pass_rather_than_in_it` holds a cover and watches the pass ask
  the next question anyway.
- **What a listener has just reached for is asked about next, and a seek is spent once.** `Sought`
  is a `Mutex<Vec<Seek>>` shared with whoever started the pass — `Sought::album` and
  `Sought::artist` are the whole of how something is put on it, newest last and each held once —
  and `Sought::taken` drains it at the top of every turn of the queue. `Pass::lift` is what reads
  it: `rotated` moves a named `Ask` to the front of what is *left*, and the newest therefore
  leads, because each seek rotates its match to index zero over the one before it. A seek naming
  nothing the queue still holds is not dropped but *read back* — `Library::album_if_due` and
  `Library::artist_is_due` weigh the row against the same `Waits` the queue was built from — and where it is still due the `Ask` is inserted at the front instead. So
  an album a scan landed while the lookup was running is reached by a click, which a rotation over
  a slice could never do, and an album the reference has already answered still is not: it is not
  due, and the read answers `None`. What a pass has already asked *this run* is refused by the
  `spent` set rather than by the clocks, because `refresh` makes every row due and a click on the
  album being asked about would otherwise ask for it twice. `LibraryModel` holds one `Sought` for
  the whole run of the window and hands it to every `enrich`, so a row reached while nothing is
  running is still at the front when a run starts.
- **The window seeks what it has drawn, front-most last.** `LibraryModel::select` seeks the album
  and its owner, and `ask_about_what_is_drawn` runs where a listing lands: an artist selection
  seeks every album the drawn tracks belong to and then the artist, in reverse of the order they
  are listed in, because `lift` rotates each seek to index zero in turn and the last one pushed is
  therefore the first one asked. The artist leads, then the first album on the screen, then the
  rest down the pane.
- **An artist's row is read at the moment it is asked, not when the queue was built.**
  `artists_to_ask` answers the ids that are due, `Ask::Artist` carries one, and
  `Library::artist_to_ask` is what reads the row — so the `artists.mbid` that `Pass::credits` wrote
  while a *later* album was landing is the one `profile_of` sees. The pass used to get that for nothing, by reading `artists_to_ask` only
  after the album loop had finished; one queue holding both cannot, and reading the row where it is
  used is what makes that ordering irrelevant rather than load-bearing — which
  `an_artist_named_in_a_release_credit_is_looked_up_by_the_credit_id_and_never_searched` is the
  claim of. **An artist the pass itself brings into the catalog is asked before the pass ends.** A
  recording or a release that lands can bill somebody no row named yet — the Witcher 2 score's
  tracks landed crediting Adam Skorupa, Krzysztof Wierzynkiewicz and Oleksa Lozowchuk by id — and
  that row was due only on the *next* pass, so it sat with an mbid, no profile and no portrait.
  `Pass::run` reads `artists_to_ask` again once the queue is walked and walks whatever it names
  that the pass has not spent, until nothing new is due; a capped run (`--albums`) does not, the
  cap being a promise about how much is asked.
  `an_artist_a_landing_names_for_the_first_time_is_asked_about_in_the_same_pass` is the claim.
- **`asked` and `answered` are the two clocks, `asks` and `refusals` are the columns they are read
  with, and `Waits` is the three waits in one value.** `due_again` writes the clause for a
  table's alias and `albums`, `tracks` and `artists` are each read through it: never asked, asked
  and unanswered longer ago than the wait its `asks` has earned, answered more than
  `REFRESH_AFTER` — thirty days — ago, or
  anything at all under `refresh`. `Waits` carries `retry_after`, `refused_again_after` and
  `refresh_after` together rather than as three `Duration`s a caller could hand over in the wrong
  order, `WAITS` is what this build runs on, and a test builds its own. What a failure does
  depends on which it is, through
  `Pass::heard`: `Error::Unreachable` ends the pass and stamps nothing, so
  `EnrichSummary::stopped_by` names the `LookupOp` and a run with no network asks the same albums
  again next time rather than
  writing a day's silence into every one of them; `Refused` and `Unreadable` count in `refusals`,
  stamp `asked` and carry on, because one refused album says nothing about the next. Landing a
  release or a profile stamps both, inside the transaction that writes it.
- **A row that answers nothing is asked again half as often each time, and an answer puts the wait
  back.** `asks` counts the stampings a row has taken without an answer — every `stamp_*_asked`
  carrying `Fruitless::Missed` steps it — and `due_again` waits `RETRY_AFTER` doubled that many
  times, `WAITS_DOUBLE_AT_MOST`
  capping it at thirty-two days, so a library of files the reference cannot name costs one pass
  rather than one a day for ever. Every landing writes `asks = 0` beside the `answered` it
  stamps, and a retagging is the scan's own reset: `store::apply` puts `asks` back to nothing
  wherever it nulls `answered`, so a file whose tags moved is asked about at once rather than a
  month later. `refresh` still reaches every row, which is what `resonate enrich --refresh` is
  for.
- **A refusal is bounded the way a miss is, by a count of its own.** `refusals` is the asks in a
  row that ended in `Fruitless::Refused`, stepped by the same `stamp_*_asked` that steps `asks`
  and put back to nothing by a miss or a landing, and `waited_its_turn` reads whichever of the two
  counts the last ask left standing: `doubled` writes one clause for both, so a refused row waits
  `REFUSED_AGAIN_AFTER` doubled per refusal under the same `WAITS_DOUBLE_AT_MOST` and a missed one
  waits `RETRY_AFTER` doubled per ask. A service that refuses one row for ever is therefore asked
  about it every thirty-two hours rather than every hour for ever, and a service having one bad
  minute still comes back to the row an hour later. The two counts are exclusive rather than added:
  the last answer is what says which wait the row is serving, which is why a miss after a refusal
  waits the day a first miss earns rather than the hour the refusal had reached.
- **A pass says it is running, so the next launch can carry it on.** The `enrichment` table holds
  one row while a pass is under way, carrying the `refresh` it was started with:
  `note_began` writes it as `run` starts and `note_finished` takes it away where the pass reached
  the end of its queue or the listener stopped it. A pass the reference ended — `stopped_by` is
  `Some` — and a pass that was never ended at all, because the process went away, both leave the
  row standing, and `Library::unfinished_enrichment` is what reads it back. `LibraryModel::new`
  asks at start and, where a row stands and the build can reach the network,
  carries the pass on with the refresh it was asked for; the mark cannot fail a pass, so a library
  written before the table warns through `tracing` and enriches as it always did. Nothing else
  needs picking up: a row the interrupted pass never reached was never stamped, so it is due.
  Headless, the binary's `carrying_on` is the same reading, and both ways in take it —
  `resonate enrich` and a scan handing over — because an ordinary pass that runs to the end takes
  the mark away with it, so a scan's handover would otherwise *discard* an interrupted refresh
  rather than carry it. A run that asked for `--refresh` itself is already doing the wider pass
  and reads nothing.
- **A release is taken by tag or by a strict match, an artist by tag or by an exact one, and a
  near miss writes nothing.** `Identified` is `Found`, `Group` or `Nothing`. An album whose
  `albums.mbid` is set is asked for directly, and where the reference answers `None` under it the
  album goes on to the search rather than stopping — a `Refused` under a tagged id stops it, one
  bad minute saying nothing about the tag — which
  `a_tagged_release_id_the_reference_does_not_hold_falls_back_to_a_search_that_lands` is the
  claim of, and a tagged `albums.release_group` the reference holds nothing under falls through
  to `find_group` the same way, which
  `a_tagged_release_group_id_the_reference_does_not_hold_falls_back_to_a_group_search` is the
  claim of. `find_release` is asked with the title, the owner and the owner's `mbid`, the
  barcode and the catalogue number the tags gave, and `top_release` takes the top of `top_of`
  weighing `weighed_release` — `(agreed_owner, count_fits, year_agrees, score)`, in that order —
  and lands it only where `matches_strictly`: `agreed_owner`, which is a score of
  `STRICT_SCORE`, 95, or over with a credit the owner agrees with, and `count_fits`, a
  `track_count` equal to `declared_count` — `tagged_tracks`, the `TRACKTOTAL` the files
  declared, or the rows held where none did, so a rip short a track is weighed against the
  pressing it was ripped from rather than against its own hole. A top hit that agrees on the
  owner and the score but not on the count, and carries a `group`, is `Identified::Group` — the
  group is fetched with no `find_group` between, the bullet below being what it lands — and
  anything short of that is `Nothing`;
  `a_hit_is_weighed_on_its_owner_its_count_its_year_and_then_its_score`
  and `a_strict_hit_of_the_wrong_count_names_its_group_and_one_without_a_group_names_nothing`
  in `enrich.rs` are the claims of the weighing. An album with no owner is weighed on the score
  and the count alone, which is what a compilation has to offer. An artist is `profile_of`: a
  tagged `artists.mbid` is asked for directly and falls through to `find_artist` where the
  reference holds nothing under it, which
  `a_tagged_artist_id_the_reference_does_not_hold_falls_back_to_a_search_that_lands` is the
  claim of, and a search hit is `matches_exactly`: `EXACT_SCORE`, 100, and the same name,
  because a name has nothing but itself to be checked against. A hit that falls short is a debug
  record naming what it was and how it fell, and the album or artist is stamped `asked` alone,
  so nothing a person did not tag is ever written on a guess.
- **A name agrees in one of six ways, and `Spelling`'s derived `Ord` is the whole of the
  ranking.** `same_name` answers `Marked` where the two `folded_title`s agree, marks and all,
  `Stripped` where only the `stripped_title`s do, and `Dequalified` where they agree only once
  `dequalified` has taken a version qualifier off the end of each; `names_it` weighs a MusicBrainz
  artist's aliases the same way and lowers each answer through `as_an_alias` to `AliasMarked` or
  `AliasStripped`; and `same_credit` answers `ById` above them all where a credited artist's
  `mbid` is the one the tags gave, because an id the tagger wrote is the one thing no spelling
  can outweigh. The order the variants are written in is the order they rank —
  `Dequalified < AliasStripped < AliasMarked < Stripped < Marked < ById` — so a real name beats
  an alias however well the alias is spelled, and a title that had to give up a qualifier is the
  last thing taken. All six are an agreement, so a tagger who wrote
  *Marcin Przybylowicz* is identified against the *Marcin Przybyłowicz* MusicBrainz answers with,
  where the marked fold alone left that artist stamped `asked` and asked again every `RETRY_AFTER`
  for ever. Which of the answers is taken is `top_of` weighing `(Option<Spelling>, score)`, so an
  agreement beats none, a better spelling beats a worse one whatever either scored, and the
  score decides between two of the same kind — two artists who differ only by a mark therefore
  stay two, the marked tag taking the marked row and never the other way round. Nothing else
  moves: where no answer agrees at all, the weight is `(None, score)` for every one of them and
  the debug record still names the highest-scored. A credit is weighed by `same_credit` twice
  over — the names joined the way MusicBrainz bills them, and each credited artist singly — and
  the better of the two is the answer, so *The Weeknd with JENNIE & Lily-Rose Depp* agrees with
  a file tagged *The Weeknd* alone, which
  `a_credit_agrees_where_any_one_of_its_names_does_and_an_id_beats_every_spelling`,
  `a_release_owned_by_the_tagged_id_agrees_however_the_credit_spells_it` and
  `a_collaboration_credit_agrees_where_the_file_names_one_of_its_artists` in `enrich.rs` are the
  claims of and `an_album_whose_owner_holds_an_id_is_searched_for_by_that_id_and_agrees_by_it`
  proves through the pass. Only the artist routes read aliases: `owned_by` weighs a release's or
  a group's credit against the album's owner and `matches_a_recording` a recording's against
  what the track was asked with, both through `same_credit`, and where a recording's title and
  its credit agree by different spellings the weaker of the two is what the match is weighed as.
- **A version qualifier is a closed list, because everything outside it names a different
  recording.** `VERSION_QUALIFIERS` is the fourteen spellings `dequalified` will take off the end
  of a bracketed title — *Album Version*, *Radio Edit*, *Explicit*, *Clean*, *Remastered*,
  *Bonus Track*, *Original Mix* and the rest — each weighed through `folded_title`, so the case
  and the punctuation inside the bracket do not matter, and `a_dated_qualifier` adds a remaster
  carrying a four-digit year on either side of it, so *(Remastered 2011)* and *(2011 Remaster)*
  both go while *(Remastered by Ada)* stays. `(with Justin Bieber)`, `(Live)`, `(Remix)`,
  `(feat. …)`, `(Acoustic)`, `(Demo)` and anything the list does not name are left where they
  stand, because those are a different recording rather than a different pressing of one and
  taking them off would pair the wrong take. It strips from the end inwards, in `(` `)` and
  `[` `]` alike, repeats until nothing more comes off and refuses to leave nothing behind, so
  *Know (Album Version) (Radio Edit)* folds to *Know* while *Explicit* stays a title of its own.
- **An album no pressing matches is still an album, and the release group is what it is landed
  as.** `Pass::album` reads four routes in order: `albums.mbid`, which the tags gave;
  `find_release`; `albums.release_group`, which `MUSICBRAINZ_RELEASEGROUPID` gave; and
  `find_group`. The group is the answer to a release search that keeps failing on the
  count — a rip missing a track, a bonus disc, a reissue with two more — because
  `matches_as_a_group` weighs the score and the folded owner and **no track count at all**, a
  group having none, which is the whole reason it can answer where `matches_strictly` cannot;
  and it is reached from the release search too, where the top hit is strict in every way but
  the count and names its group. What it costs is that a group names many pressings and the
  catalog wants one, and `settle_group` is where that is decided: `closest_release` answers a
  pressing and a `Fit` — `Fit::Exact` for one whose `track_count` *is* the `declared_count`,
  then `Fit::Wider` for the smallest pressing holding more tracks than the album does, and where
  there is none `Fit::Narrower` for the widest holding no more than it, the earliest dated among
  equals every way, `earliest` sorting an undated release last so a dated pressing is preferred
  and an undated one is taken only where nothing else fits — and `land_release` then runs on that
  id as though the search had found it. A wider
  pressing is landed rather than refused because what it costs is honest: the rows the rip lacks
  are release rows with no `track_id`, so `Album::missing` counts them and the Missing pane
  lists them, where a rip weighed against its own hole was landed as nothing at all — which
  `a_group_with_no_exact_pressing_lands_the_smallest_wider_one_or_the_widest_narrower_one` in
  `enrich.rs` and `a_hit_one_track_short_lands_the_pressing_its_group_names_and_lists_the_missing_row`
  through the pass are the claims of. A narrower one is landed for the same reason read the other
  way: a rip carrying bonus tracks no pressing has is short of nothing, so the widest pressing is
  the most of it the reference can account for, and landing it writes the release columns, the
  links and the rows it *does* name rather than leaving the album with a group id and nothing
  else — the count the fallback weighs is the rip's own rather than the `declared_count`, so a
  `TRACKTOTAL` naming more tracks than the files hold still lands the pressing as wide as the rip,
  which `an_album_wider_than_every_pressing_of_its_group_lands_the_widest_one` is the claim of.
  Only where no pressing in the group declares a count at all does `take_group` run
  `land_release_group`, and what it writes is deliberately thin:
  `albums.release_group`, and `kind`, `date`, `year` and `disambiguation` coalesced so the
  release columns a pressing would have filled are never guessed at. It writes no `albums.mbid`
  and no `release_tracks`, because a pressing that cannot hold the rip is not the one it came
  from. It is therefore half an answer, and the pass goes on asking: `album_due` is
  `due_again` with `HOLDS_A_GROUP_ALONE` beside it — a `release_group` and no `mbid` — under the
  same `waited_its_turn` a fruitless ask waits out, and the landing leaves `asks` where
  `stamp_album_asked` put it rather than zeroing it, so the retries spread a day, two, four and
  on to the `WAITS_DOUBLE_AT_MOST` ceiling. What makes a retry worth taking is that the album
  moves under it: a rip finished, a track retagged, a count that now matches a pressing the group
  already held — where the month `REFRESH_AFTER` names was the only thing that would look again.
  A pressing that lands puts `asks` back to nothing, which is what takes the album out of the
  retry.
- **A search is asked as a phrase, and as words wherever the phrase *landed* nothing.**
  `found_either_way` is how `find_release`, `find_group` and `Route::Search` all ask: once with
  `Wording::Phrase`, the fielded query the online crate writes, and once more with
  `Wording::Words`, the loose dismax one, wherever weighing the first answer took nothing. What
  lets one function serve three is that it weighs as well as asks — it takes the `weigh` its
  caller would have applied and answers what that answered — and `Landed` is the one thing the
  three results have in common, `Identified::Nothing` and two `None`s being three spellings of
  nothing landing. An empty answer and a near miss are therefore one case, which is the
  correction: a phrase that found the wrong pressing used to be left there, on the argument that
  the strict rule had already weighed it, but the two queries are not the same query — the fielded
  phrase matches a title exactly and the dismax words match it loosely, so a pressing the phrase
  ranked under the wrong one, or missed over a subtitle the tagger dropped, is reachable only by
  asking again. A refusal is still not asked again, being the service's bad day rather than an
  answer. What it costs is one more search per album the phrase could not settle, which the
  doubling retry already bounds.
  `a_phrase_that_answers_nothing_is_asked_again_in_words_and_lands_under_the_strict_rule`,
  `a_phrase_that_answers_a_near_miss_is_asked_again_in_words_and_lands_there` and
  `a_track_the_phrase_answered_nothing_for_is_searched_again_in_words` are the three claims, and
  the group searches standing beside them are each asked once, which is the fourth: a phrase that
  lands is never asked again. `Looking` is what keeps one `found_either_way` for all three — a
  matchable pair of an id and a title, so the record saying the words are being asked names an
  album or a track rather than either being spelled into a string.
- **An album is billed by the release where one landed, and by the tags until then.**
  `albums.title` is what the scan read the files as and no landing ever rewrites it, because it is
  half of `store::album_key` and of `sleeve_key` and rewriting it would move an album's grouping
  key under it. `albums.release_title` is what the reference answered, written by `land_release`
  alone, and the `album_title!` macro in `db.rs` — `coalesce(a.release_title, a.title)` — is the
  billed title every listing, sort, want and `organise` destination reads. A macro rather than a
  `const` because three of those four are `const` SQL built with `concat!`, which takes literals.
  What it buys is the two discs of a set: they share a release id, so they group as one album
  whose `title` is whichever disc the walk reached first, and the release's own title is what the
  pane and the layout then say. A rescan cannot undo it, `release_title` being a column the scan
  does not name.
- **A medium is a row, because a disc is a thing rather than a number on a track.**
  `release_media` is `(album_id, position)` with the `format` and the `title` MusicBrainz answers,
  written by `land_release` and read back as `ReleaseDetail::media`; `release_tracks.disc` stays
  what says which medium a row sits on. `models::album_rows` puts an `AlbumRow::Disc` above each
  run of rows where the album spans more than one disc — read off the rows themselves, so a set
  the reference never answered for still heads its discs — and `browser::disc_heading` names it
  from the medium where one landed, preferring its title over its format. The heading is drawn
  with the same `row` every track row takes, because the tracks pane is a `uniform_list` and a
  taller row would lay the whole list out wrong. `browser::pressings` is the other reader: the
  release line says `2 × CD` where every medium agrees and `N discs` where they do not.
- **Landing a release is one transaction, and pairing its rows is a second.** `land_release`
  writes the release columns on the album, fills `year` only where the scan left none, stamps
  `answered`, deletes and reinserts `release_tracks`, `release_media` and `album_links` and writes
  each row's `release_track_links`; the wants under the album are read first through `wants_under`
  and put back once the rows exist again, so a want survives a refresh that changed the row ids.
  `Carried` is what a want is put back *by*, most exact first: `Carried::Track` is the release track's own
  mbid, `Recording` the recording's, and `Seat` the `(disc, position)` it used to sit at —
  because a reference that re-edited a release moves a track to another seat, and the seat alone
  would carry the want to whatever now sits there. `want_again` lands the exact ones first and the
  insert is `ON CONFLICT DO NOTHING`, so two wants that would land on one row leave it to the
  better-carried of the two, and a want whose track the release no longer holds is dropped as it
  always was. `rematch_release_tracks` then pairs release rows with catalog rows in four passes,
  each taking only rows the earlier passes left and catalog rows not yet taken: the recording
  mbid against `tracks.mbid`, the track mbid against `tracks.release_track_mbid`, the disc and
  position
  against `disc_number` and `track_number` — with a missing disc read as 1 only on a one-medium
  release, so a two-disc set never pairs a disc-less row with the wrong disc — and last the
  `folded_title`. It writes `release_tracks.track_id` where the pairing *moved* and answers how
  many rows it moved, so `EnrichStats::matched` says what a pass changed rather than what was
  already true; and for every row it paired, moved or not, it fills `tracks.mbid`,
  `release_track_mbid` and `isrc` by `coalesce` from the release row, so a file tagged with no
  identifier learns the ones its seat on the release carries, which
  `a_paired_track_receives_the_identifiers_its_release_row_holds` is the claim of. It is a fill
  and not a correction — a code the file itself carries stands — and a rescan
  overwrites the three only where the file names one, so the scan rule above holds.
  `AlbumToAsk::rematch_only` is what makes an album holding release rows rematch
  whether or not it is due, so a rescan that added a file pairs it without asking the network —
  but only where a pairing could change anything: `albums_to_ask` offers such an album only where
  `HOLDS_AN_UNPAIRED_ROW` or `HOLDS_AN_UNPAIRED_TRACK`, because an album whose every row and every
  track are already paired has nothing for the four passes to find and would cost a read and a
  write transaction each time a pass ran.
- **An album is not the only thing a file belongs to, so a track is asked about in its own right.**
  A file carrying no `ALBUM` tag has `album_id = NULL`, is under no album the pass could reach and
  was therefore enriched by nothing at all — which is a whole shape of library, the singles and
  the loose rips a tagger never filed. `Ask::Track` is the answer: `tracks_to_ask` is
  `WHERE NOT IS_PAIRED AND TRACK_DUE`, so a row `rematch_release_tracks` has already paired to a
  release row is never in the queue, and `release_tracks_by_track` is the index that keeps
  `IS_PAIRED` a lookup rather than a read of every release row in the catalog per track. The row
  itself is read at the moment it is asked, through `Library::track_to_ask`, which asks
  `NOT IS_PAIRED` a second time — an album landing earlier in the same pass pairs the tracks under
  it, so those rows retire from the queue silently instead of being asked about over the network a
  moment after the answer arrived, which
  `a_track_the_album_pass_already_paired_is_never_asked_about` is the claim of. It is the same
  discipline `Library::artist_to_ask` already followed for a credit, and the reason the queue is
  albums, then tracks, then artists.
- **Four routes, most exact first, and the first that answers ends the track.** `Route::ALL` is
  `Isrc`, `Recording`, `Search`, `Fingerprint`, and `Pass::track` walks them in that order until
  one answers, stamping `asked` alone where none does. An ISRC the file carries is asked through
  `recordings_of_isrc`, which answers a *list* and not one recording, because a code names every
  take released under it — so `best_recording` tells them apart by length and answers the take
  with a `Certainty`: one answer is `Exactly` where the lengths agree or either is unmeasured,
  and `Nearly` where the length disagrees but the title agrees by some `Spelling`
  (`the_only_take`), because a code the file carries under a title it also carries is evidence
  of the recording and not of which take, which
  `an_isrcs_only_take_is_nearly_the_file_where_the_title_agrees_and_the_length_does_not` in
  `enrich.rs` is the claim of; several are narrowed to those within `RECORDING_MAY_DIFFER_BY`,
  five seconds, of the file and then to the closest, `Exactly`. A `MUSICBRAINZ_TRACKID` is the
  recording id and is asked for directly. A search is `find_recording` with the title, the
  length and what `asked_with` answers — the track's artist and its `artists.mbid`, or where the
  file names none the album owner's and its — and the album's billed title as `release` only
  where neither is known; it is weighed by `matches_a_recording`: `STRICT_SCORE`, a title
  agreeing by some `Spelling`, a credit agreeing through `same_credit` with whatever was asked
  with, and a length inside the same five seconds. It is refused before the request where
  `tagged_title` is missing, because a title that is only the file's own name is the one thing a
  text search must not be handed, and where no artist, no owner and no album is known, because
  a bare title names nothing;
  `a_track_that_names_no_artist_is_never_searched_for` is the claim of both halves, and
  `a_track_naming_no_artist_under_an_owned_album_is_searched_with_the_owner_and_lands` and
  `a_track_naming_no_artist_under_an_unowned_album_is_searched_with_its_release` are what the
  two fallbacks buy: a file naming no artist under an album that names one is asked about as
  that artist's, and one under an album nobody owns is asked about by the album's name.
- **A search answer says where the recording sits, so nothing is asked twice.** `RecordingMatch`
  carries the credit and the `RecordingRelease`s the search named as well as the score, the title
  and the length, and `RecordingMatch::into_recording` is what `take_match` lands — through
  `told_where_it_sits`, the same rule the ISRC route takes, so the `/recording` lookup is made
  only where the answer named no release at all. MusicBrainz's search index carries a recording's
  releases in full, and places the match on each by the medium's `track-offset` where the
  document gives no `position`, which is what the search shape names it; it carries the
  recording's `isrcs` as well, so `RecordingMatch` reaches `land_recording` with the code and the
  `coalesce(isrc, ?8)` fills the column. That is the trade come good: one request a track rather
  than two, on the route a library of loose files spends nearly all of its pass in, and the code
  an *identifier* route would have written arrives on it anyway. A recording registered under no
  code writes none, which is a different answer from not having asked.
- **What a lookup may overwrite is `Certainty`, and it is a type rather than a rule each caller
  remembers.** `Exactly` is a recording id the *file itself* named, or an ISRC it named whose
  take is as long as the file; `Nearly` is a text search, a fingerprint, or an ISRC whose only
  take is another length under the same title. `land_recording` reads it in one `CASE` per
  column: a name is filled
  wherever the file named none — `tagged_title IS NULL`, `tagged_artist IS NULL` — whatever the
  certainty, and a name the file *did* carry is corrected only under `Exactly`. So a lookup tidies
  *one of these days* into *One of These Days* on the strength of an identifier a tagger wrote,
  and can never rename a track on the strength of a score, which
  `an_exact_identification_corrects_a_title_the_file_carried` and
  `a_search_match_leaves_the_title_the_file_carried_and_writes_the_identifiers_alone` are the two
  halves of. Everything else it writes fills and never replaces — `track_number` and `disc_number`
  from where the recording sits on the release, `mbid` and `isrc`, all `coalesce`d — except
  `release_title`, which is whichever release the route chose, and `artist_id`, which is repointed
  through `store::artist_named_in` so the row is billed to the artist the catalog already holds
  under that fold. `asked` and `answered` are stamped in the same statement, whose `RETURNING` is
  what `EnrichStats::named` counts a moved name off and what `store::index_row` rewrites
  `tracks_fts` from, so a corrected title is searchable at once rather than at the next scan.
- **A track that names its release lands the album its own pass never reached.** `best_release` is
  which of a recording's releases the row is filed under: the album's `albums.mbid` where it has
  one, then a release whose `folded_title` is the album's, then the earliest dated by `earliest`.
  Where the album has never been answered — `TrackToAsk::album_answered` is false —
  `take_recording` goes on to `land_release` on that id, so identifying one track of an untagged
  album lands the whole release and pairs every row of it in the same turn. Where the album *has*
  answered, the track stops at its own row, because the album's identification was the more
  considered of the two and a recording's idea of which pressing it belongs to is not.
- **`fingerprint.rs` is the seam a recogniser fills.** `Fingerprints` answers
  a `Printed` — `Nothing`, or `Recognised` with `RecordingMatch`es — for a `Sounded`, which is the
  location, the span, the length, what the *file* said its title and artist were rather than
  what the catalog settled on, and the `Chromaprint` the study took of it, so no printer decodes.
  `resonate-online`'s `AcoustId` is the one this build registers, where an `acoustid-key` is set;
  `analysis.md` has the studies the print comes out of and how a recognition is weighed. `NoFingerprints` is the stub, registered under the source name
  `unprinted`; `Fingerprinters::none()` is the registry holding it alone, `and` registers one per
  name the way `Providers::and` and `Lyricists::and` do, and `has_a_source` is how a caller asks
  whether anything real is behind it. `Library::enrich` takes one beside the `Reference`, and
  `Fingerprinters::recognise` asks each printer in turn for the first answer that is not empty,
  answering a `Recognition` that says whether any printer refused, which the pass counts in
  `refused` and carries on past rather than ending. **A fingerprint is weighed on
  its score alone**: `recognised` takes the top match at `STRICT_SCORE` and asks nothing about the
  title or the artist, because the audio is the evidence and a file worth fingerprinting is
  exactly one whose name is not.
- **A collaboration is listed under every artist it credits, and never as an artist of its own.**
  `track_credits` — a migration step — holds each member of a track's credit, and
  `credits::credit_the_members` rebuilds it from `tracks.artist` every time the orphans are swept:
  `members_of` splits the text on the joins a credit is written with — `&`, `and`, a comma, a
  semicolon, `/`, `+`, `x`, `with`, `feat.`, `ft.`, `featuring`, `vs.` — and the split is taken
  **only where every part names an artist the catalog already holds**, so *Adam Skorupa &
  Krzysztof Wierzynkiewicz* becomes the two composers while *Simon & Garfunkel*, whose halves
  name nobody, stays one artist. A split track's `artist_id` is its first member unless it already
  names one of them, the text stays the credit the file gave, and the row the whole credit was
  filed under is swept once nothing names it. The artist scope, `artist_albums`, `artist_tracks`,
  `WHAT_AN_ARTIST_HOLDS` and `BY_OR_HOLDING_THE_ARTIST` read a credit beside `artist_id`, the
  sweep keeps an artist a credit names, and `take_over_artist` carries its credits across a merge.
  The members come from two places. A landed recording now makes a row for *every* artist it
  credits, not only the first, so the split has names to find; and where an artist's own lookup
  lands nothing and its name splits, `Pass::bill_the_members` searches each member not already
  held under the same exact-name rule and `Library::bill_an_artist` makes a row for each one
  found, which the pass then asks about like any artist it brought in. Before, the Witcher 2
  score's tracks were split between *Adam Skorupa*, where a recording had been identified, and a
  row for the whole credit, where none had, and neither composer's page held the other half.
  `a_collaboration_is_listed_under_each_artist_it_credits_and_not_as_one_of_its_own`,
  `a_name_whose_halves_name_nobody_held_is_one_artist` and
  `a_collaboration_the_reference_cannot_name_is_asked_about_one_member_at_a_time` are the claims.
- **A credit names an artist the catalog may already hold, and it is identified rather than
  asked.** `Pass::credits` walks the `Credit`s of a landed release or, through `take_group`, of a
  landed release group: where one carries an mbid and
  `Library::artist_named` finds the folded name, `write_artist_mbid` fills `artists.mbid` where
  it was empty, so the artist ask that follows reads the row afresh, asks for that artist's profile
  by id and spends no search on a name the release already settled. The fold it looks the name up
  by has to be the one the column holds: `artist_named` keyed on `name.to_lowercase()` where
  `artists.key` is `store::folded_letters(name)`, so every marked spelling missed silently and
  *Marcin Przybyłowicz* was searched for by name after the release had just named him by id.
  `an_artist_is_found_by_the_fold_of_a_credit_name_however_it_is_spelled` is what holds the two
  together now.
- **An artist's discography is kept once its profile lands, and what the catalog is short of is
  read off it.** `Pass::artist` calls `discography` after `land_artist`: `release_groups_of` is
  asked for every release group the artist is credited on, `worth_keeping` keeps those whose
  primary type is one of `KEPT_KINDS` — `Album` and `EP` — and whose secondary types are none or
  `Soundtrack` alone, so a live album, a compilation, a remix, a single and a group with no type
  are left out, which `an_album_and_an_ep_are_worth_keeping_and_a_soundtrack_is_still_one` and
  `a_live_album_a_compilation_a_remix_a_single_and_an_unkinded_group_are_left_out` in
  `enrich.rs` are the claims of; and `land_artist_releases` deletes and reinserts
  `artist_releases` — `(artist_id, mbid)` with the title, the kind, the first release date and
  the `folded` haystack `spelt_out` writes, which is those three run through
  `store::folded_letters` — answering how many rows it wrote, which
  `EnrichStats::releases_found` counts. A release is
  *unheld* where no album's `release_group` is its mbid — `unheld_by_any_album!` in `db.rs`,
  read off `albums_by_release_group` — so a landed pressing takes its group out of the list and
  a group landed thin does the same; `Library::unheld_releases` lists them under a cap by artist
  and first release date, `ArtistDetail::releases_unheld` counts one artist's and
  `Library::missing_counted` answers a `Missing` — those beside the release rows with no
  `track_id`, which `Library::missing_tracks` lists in album order with each row's `WantId`.
  `an_artists_discography_is_kept_once_its_profile_lands_and_the_catalog_says_what_it_does_not_hold`
  and `the_rows_an_album_is_short_of_are_listed_with_their_wants` are the claims of the readers.
- **What the catalog is short of is narrowed by the words typed, and each half answers with what
  it has.** All three readers take an `Option<&str>`, so the pane's list, its counts and the
  sidebar's figure narrow together. A missing track belongs to an album the catalog *holds*, so
  it is narrowed through `narrowed_onto("album_id", "a.id")` — the same grouped `tracks_fts`
  join the albums pane takes, so the whole search grammar reaches it and typing an album, its
  owner or a track it holds brings up what that album lacks. An unheld release is in no catalog
  and has only its own row, so `unheld_holding` writes one
  `r.folded LIKE ? OR ar.key LIKE ?` per lone word of the search, each folded through
  `store::folded_letters` the way `artists.key` already is — which is how a title, a kind, a
  year and an artist's name all narrow it, and how *przybylowicz* finds *Przybyłowicz*. Only the
  lone words are read: a `plays:>20` term says nothing about a release nobody holds, and
  `playlist::words_of` is where that reading already lived. `what_the_catalog_is_short_of_is_narrowed_by_the_words_typed`
  and `an_unheld_release_is_found_by_a_name_spelt_either_way` are the claims.
- **A cover from the archive lands only where the files embedded none, and the archive is asked
  until it has answered.** `land_archive_cover` writes `cover_art`, `cover_format` and
  `cover_source` under `WHERE cover_art IS NULL`, so a file's picture is never overwritten, and
  `Pass::album` asks the reference for a cover in the pass that landed the release, where
  `has_cover` is false. `albums.cover_asked` — the first step in `MIGRATIONS` — is stamped once the
  archive has *answered*, with a picture that landed or with none, and never where the fetch
  failed, was refused or was cut off by a pass ending under it; `Pass::look_again_for_covers` asks
  at the end of every run for each album holding a release or a group, no picture and no stamp,
  passing over those `Pass::covered` says the run already asked about. Before it, one failed fetch
  left an album with its release and no sleeve for good, while Discord drew the same release's
  cover off its id. A cover the archive answered it does not hold is not asked for again
  inside `COVERS_ASKED_AGAIN_AFTER` — thirty days — unless the release is refreshed or
  `Library::ask_again_for_covers` clears the stamps, which is the settings pane's *Look for missing
  covers*; past it `albums_wanting_a_cover` reads the stamp as none, because somebody may have
  uploaded the sleeve since, and the answer stamps it for another month either way. At the
  archive's pace that is one request a month per uncovered album, which a library of hundreds
  spends in a few minutes. `a_cover_the_archive_said_it_lacked_a_month_ago_is_asked_for_again`
  is the claim. An album landed as its release group asks `group_cover` under those same two
  conditions, so what the group route is short of is a release's rows and never a sleeve. A
  portrait is the same shape: `land_portrait` writes under `portrait IS NULL`, and it
  is asked for only where the profile's links name a picture and none is held.
- **A portrait that missed is looked for again out of the links already held.** `land_artist`
  stamps `answered` and zeroes `asks` *before* the picture is asked for, and the picture is
  fetched on a thread whose outcome feeds back into no column — so a fetch that failed on a bad
  day used to wait out the thirty-day `REFRESH_AFTER` with nothing recording that it had.
  `Library::artists_wanting_a_portrait` reads every artist with `portrait IS NULL` whose stored
  `artist_links` name a picture, and `Pass::look_again_for_portraits` asks for each at the end of
  the run. It needs no column and no MusicBrainz request — the links have been stored since
  enrichment landed and nothing read them back — and `Pass::pictured` is what keeps it from
  asking twice, an artist the pass itself already asked about not being asked again by the sweep.
  Without that set the pictures, being fetched on their own threads, would race the sweep's read
  of `portrait IS NULL`.
- **A link is a relation, a service and a URL, and both names are read off the reference's own
  words.** `Relation::of_type` matches the exact type string MusicBrainz writes — `streaming`,
  `free streaming`, `official homepage`, `image` and the rest of `Relation::TYPES` — and anything
  else is `Relation::Other`; `Service::of_url` reads the host, folds it to lowercase, drops a
  leading `www.`, and matches it or a parent domain against `Service::HOSTS`, with `x.com` and
  `twitter.com` both `Twitter` and any host carrying an `amazon` label `AmazonMusic`, and anything
  else `Service::Other`, which keeps the URL. `Service::name` is the lowercase figure a pane
  draws. All three live in `resonate-core`'s `link.rs`; the tables still name the column
  `provider` and `store::service_code` and `store::service_of` are its encoding.
- **A want is a release row the catalog holds no file for, and a provider is what fills one.**
  `wants` is one row per `release_track_id`, so `Library::want` refuses a row it does not hold
  with `Error::UnknownReleaseTrack` and answers the same `WantId` twice for the same row; `unwant`
  drops it and `wants` reads them all, newest first, each a `Want` carrying the album's title, the
  row's title and artist, its recording and track MBIDs, the album's release MBID, its ISRC,
  length, disc and position, its own links and the release's. An identifier that does not parse
  is read as absent through `store::mbid_in` and `store::isrc_in`, the readers a tag goes through.
  `Want::identity` is the turn from a want into the `resonate_providers::Identity` a provider is
  handed, and `supply.rs` is the pass: `Library::poll` walks the wants due under
  `PollOptions::again_after` — `POLL_AGAIN_AFTER`, six hours — on a thread named `resonate-poll`,
  asks `Providers::first`, and lands what it answers. A `Delivery::File` goes through
  `Vault::keep` and a `Delivery::Stream` through `Vault::keep_delivered`; either kept writes the
  `vault_objects` row through `note_supplied` with `taken_from` the file's URI or
  `<provider>:<key>`, and `wants.offered` is the vault object's URI. With no vault a file's own URI
  is written as the offer and a stream is dropped, because it has nowhere to be kept; that, a vault
  refusal and a vault failure are each counted `unkept` and offer nothing, so `offered` never names
  what cannot be opened. `note_tried` keeps an earlier offer where the new pass found none.
- **A lyric fetched once is kept, and so is a miss.** `lyrics_kept` is keyed by `(path,
  span_start)` the way `tracks` is, so a cue row keeps its own words apart from the file's;
  `text` is `NULL` for a remembered miss and `taken` says when, which is what a provider weighs a
  miss's age against. `Library::kept_lyrics` and `keep_lyrics` take the `MediaLocation` and the
  `Option<FrameSpan>` and refuse a location that is not local through `playlist::local_path`,
  because a row here is a path like every other. The provider that reads and writes it is the
  online crate's, so the catalog holds words it never parses.
- **The panes read what landed through seven calls, and two counts ride on the listings.**
  `Library::release_of` answers a `ReleaseDetail` — the release columns, the `CoverSource`, the
  two clocks and the album's links — `release_tracks` the `HeldReleaseTrack`s with each row's links
  and the `TrackId` it paired with, `artist_detail` an `ArtistDetail` with the genres, the links
  and `releases_unheld`, `portrait` the `CoverArt`, sniffed where the stored format code is
  missing, and `missing_tracks`, `unheld_releases` and `missing_counted` what the discography
  bullet above describes, the first two under an `Option<usize>` cap.
  `Album::missing` is the count of release rows whose `track_id` is `NULL` and `Album::mbid` and
  `Artist::mbid` are read off the same listing rows, so an album grid says which albums are short
  a track without a second read; `Artist::has_portrait` is what lets a list draw a placeholder
  without asking for bytes.

## Writing the catalog back into the files

- **A guess is never written, which is why a name is written only where a lookup answered for the
  row that holds it.** `tracks.title` falls back to the file's stem where the tags named nothing
  and `albums.title` to whatever grouped the folder, so writing either back would put this build's
  own reading into the file as though a reference had said it. `offered` therefore gates the track
  name and artist on `tracks.answered`, the album name and its two totals on `albums.answered` —
  and it is `albums.release_title` rather than `albums.title` that is offered, because only a
  landed release names a pressing, an album settled as its release group alone naming none — and
  the album artist and its id on the *artist's* own `answered`, since `land_release` never
  repoints `albums.artist_id` and the credit there is the scan's attribution. An identifier is
  never a guess: `tracks.mbid`, `release_track_mbid`, `artist_mbid`, `isrc`, `albums.mbid`,
  `release_group` and what the tags declared about the pressing are offered whatever answered,
  because each of them is either the file's own or a strict match's.
- **What is written is the difference, so a file already saying it is left alone.** `wanted` reads
  the file's own `TagSet` through the same `TagSource` the rest of the build reads it through and
  keeps only the fields whose value differs, so a run over a library that has already been written
  costs one probe a file and no writes at all, and `RetagStats::unchanged` is how many said so. A
  blank value names nothing and is dropped before the comparison, the same rule `tags::given`
  applies on the way in.
- **A picture is the catalog's to give and the file's to keep, and it rides into the same write.**
  `Writing` carries the edits and an optional front cover, so a file that wants both costs one
  `save_to_path` rather than two rewrites of its whole tag; `offered_picture` is what fills the
  second half, and the rule is the mirror of `albums.cover_source`'s — the album's cover is offered
  only where the file's own read answers that it carries none, so an archive cover reaches a file
  that carries none and a file with a picture of its own is left with it. The plan reads each file
  **once**, through `TagSource::read` under `Picturing::Whether`, so the fields it weighs and
  whether a picture is there come off one open and the picture is never copied out to answer a
  `bool`. The catalog's cover is asked for only where the file carries none, and `Sleeve` holds one
  album's bytes at a time while `TRACKS_TO_TAG` reads in path order, so a run of tracks out of one
  folder shares one read of the blob. `written` reads the file back once as well, under
  `Picturing::Copied` where a picture went in, so the fields and the picture are weighed off the
  same open. A file under no album is offered nothing, having no
  cover to be given one from. What is written is a `PictureType::CoverFront` under the format's own
  media type, and it *replaces* the front cover rather than standing beside it, so a file cannot
  collect two. `written` weighs the picture that reads back against the bytes that went in, exactly
  as it weighs each field, so a container that quietly drops one is `Unwritten::Unconfirmed` and the
  catalog is not moved. `RetagStats::pictures` counts them apart from `fields`, because a picture is
  not a field and a write of one alone is still a write.
- **A write never touches the file it is writing until it is whole.** lofty's `save_to_path`
  splices a FLAC's metadata and shifts the audio behind it in place, so a full disc or a run killed
  halfway left a truncated file. `FileTags::write` copies the file to a staged sibling —
  `.<stem>.<pid>-<n>.<ext>`, the extension kept because lofty reads the kind off it — writes the
  tags into the copy, `sync_all`s it, renames it over the file and syncs the folder, and takes the
  copy away again wherever any of that failed. What it costs is a copy of the file per write, which
  is the price `config::edited` and `organise`'s sheets already pay for the same promise.
- **A row cut out of a file it shares is never written to.** Twelve cue rows are twelve readings
  of one file and there is one set of tags between them, so a path holding more than one row — or
  one row carrying a span — is `Unwritten::Cut` and passed over whole. That is the same reason
  `Library::track_played` keys a play on the pair: a cut is a row of its own everywhere but in the
  file.
- **The catalog follows the file, because the file is what the next scan will read.** A write
  changes the size and the mtime, which is exactly what `Known::under` compares, so
  `files_retagged` writes both back and the next walk reads the row as unchanged rather than
  re-probing it. It writes `tagged_title` and `tagged_artist` too, and only for the names it
  actually wrote: those two columns are how a rescan tells a retagging from an identification, so
  a corrected title written into the file and not recorded here reads as the tagger having moved
  it, takes the file's names back over the reference's and nulls `answered` — the whole library
  asked about again on the next non-incremental scan.
  `a_rescan_reads_this_builds_own_write_as_the_names_it_already_knew` is that claim.
- **A write is confirmed by reading it back, and an unconfirmed one is not followed.** `written`
  re-reads the file through the `TagSource` after `TagSink::write` and weighs every edit against
  what now comes back; a field that does not read back leaves the row `Unwritten::Unconfirmed` and
  the catalog is not moved, so a format quirk shows up as a refusal rather than as a preview that
  offers the same edit for ever. It is what `FileTags::writes` refusing a format lofty cannot
  write is the cheap half of. A WAV whose ID3v2 tag stands in front of its `RIFF` header is read
  here and not recognised by lofty, so its write is `Unwritten::Refused` and the file is left as
  it was.
- **`Library::retag` takes the `Walk` guard, so a scan and a write-back cannot run at once.** The
  pass rewrites the sizes and mtimes a scan's snapshot was taken against, which is the same hazard
  `organise` has, and a second caller gets `Error::AlreadyWalking`.
- **The preview is the plan.** One `Retagging` is built and then handed to the apply or not, so
  `resonate tag` and `resonate tag --apply` cannot disagree about what would happen; a write that
  fails moves out of `Retagging::writes` and into `passed_over`, so what is printed after an apply
  is what was done rather than what was intended. The settings pane's *Tagging* group draws that
  same plan — see `ui.md` — so the command line and the window are two presenters of one pass.

## Organising

`Library::organise` files every scanned track under a layout, and `resonate organise` and the
settings pane's *Organising* group under Library are the two ways in. It takes the same `Walk`
guard `Library::scan` does and re-keys a sleeve-keyed album after the moves land; both of those
are under *Schema and grouping* above, because they are facts about the catalog rather than about
the pass.

- **A `Layout` is segments of pieces, and the segments are split before the pieces are read.**
  `Layout::read` splits the template on `/` first and only then reads each part into
  `Piece::Literal`s and `Piece::Named(Field)`s, which is why a `/` can never reach a literal or a
  field: by the time pieces exist there is no separator left to put in one. An empty segment is
  `LayoutFault::EmptySegment`, so a leading `/` is refused rather than making the path absolute,
  and a segment that is `.` or `..` is `Error::LayoutEscapes` naming its index. That, with
  `as_one_component` writing every `/` a *value* holds as a `-`, is the whole of the guard:
  neither what a tagger wrote nor what the template says can name a path outside the root the file
  came from, which is why `Refusal` has no `Escapes` variant — nothing could construct one.
  `{{` and `}}` write a brace, an unclosed `{` is `LayoutFault::Unclosed` at its own byte offset,
  and a name `Field::read` does not know is `Error::UnknownLayoutField` carrying it as a
  `FieldName` rather than as prose. All of it is refused when `organise-as` is *read*, so a
  mistyped template is a startup error and never a half-moved library. `Display for Layout` writes
  the template back as it was read, which is what the settings pane's field draws and what
  `DEFAULT_LAYOUT` — `{albumartist}/{album}/{disc}{track} {title}` — is asserted against.
- **A component is what a filesystem will take, and a segment resolving to nothing is dropped.**
  `as_one_component` maps `/` to `-`, drops the control characters and the delete, trims leading
  whitespace and trailing whitespace and dots — so *...And Justice for All* keeps its dots while
  `..` resolves to nothing — and cuts what is left to `COMPONENT_BYTES`, 255, on a character
  boundary. The extension is appended only where the layout does not name `{ext}` itself, and the
  last segment's budget is 255 less what that extension will take, so a name cut to the limit
  still ends in `.flac`. A middle segment that resolves to nothing is skipped and the path closes
  up; the *last* one answers `None`, which the planner reads as `Refusal::Unidentified`, because a
  file with no name to be given is one to leave where it stands. **What a volume takes is read
  per root**: `Naming::of` finds the root's mount in `/proc/self/mounts` — the deepest mount point
  it sits under, the table's octal escapes read back — and a root on vfat, exFAT or NTFS is
  `Naming::Portable`, which writes `\ : * ? " < > |` as `-` as well, so a title ending in a
  question mark is a file such a drive will take rather than a rename refused on every run. A
  root anywhere else keeps every character but the separator.
- **`{albumartist}` falls back to nothing and never to `{artist}`.** The column behind it is the
  album's own owner, and an album whose tracks disagree about one has none — which is what the
  `COMPILATION` flag already meant. Falling back to the track artist would scatter a compilation
  into one folder per singer, which is the split the grouping's third tier exists to prevent, so
  the segment resolves to nothing and the album folder sits directly under the root instead.
  `{disc}` is the same judgement in miniature: it writes `N-` only where the album has more than
  one disc, so a single-disc album is never filed under a number it does not need.
- **Which disc a set is on is read the way the scan reads it, and `max(disc_number)` alone is not
  enough.** `scan::disc_in_folder` is one reading with two callers — `scan::sleeve`, which gathers
  `CD1` and `CD2` into one album, and `organise::disc_of`, which names a row's disc where no tag
  does. A set filed that way with no disc tags has no `disc_number` at all, so `{disc}` would write
  nothing and both discs' track 1 would name one file; `Planner::over` therefore raises each
  album's count to the larger of its stored `max(disc_number)` and what the folders spell, and
  holds it per album rather than per row so every track of a set agrees about how many discs there
  are.
- **The preview is the plan the apply performs, not a description of it.** `organise::run` builds
  one `Plan` and hands that same `Plan` to `apply` only where `OrganiseOptions::apply` says so, so
  there is no second walk and no second set of rules for the two to disagree about, and
  `Pass::Preview` and `Pass::Apply` in the settings pane are one call with one flag. What a
  preview still does is read the filesystem — `symlink_metadata` on every destination, `read_dir`
  on every source folder — because a collision and a sidecar are facts about the disc rather than
  about the catalog. `Plan::folders` is what those listings answer with: the folders every file of
  which is going, deepest first. An apply does not read that list — `prune` takes the source
  folders of the moves that actually landed and `climb_out_of` walks each upward, removing a
  folder while it is empty and stopping at the first one that still holds something or at the root
  itself — but both are sorted `deepest_first`, so a preview names the folders an apply would take
  away and in the order it would take them.
- **A file moves before the catalog does, in batches, and a batch that cannot finish is put back.**
  `apply` walks the moves `MOVES_PER_BATCH` — 256 — at a time. Inside a batch, each move is
  weighed against the disc once more (`standing`: a source that has gone is `SourceGone`, a
  destination now taken is `Collided`), renamed with its sidecars and recorded in `done`; then
  `settle` fsyncs every folder written and `Library::files_moved` rewrites `tracks.path`,
  `playlist_entries.path`, `lyrics_kept.path` and `resume_rows.uri` in one transaction, so a play
  count, a playlist row, a kept lyric and a kept queue all follow the file rather than being
  rescanned into a new row. It deletes any `tracks` and `lyrics_kept` row standing at the
  destination first, because both are keyed by the path and the `UPDATE` would otherwise be
  refused: `standing` weighs the *file* at the destination, so a row whose file has gone —
  a scan of another root having not yet tidied it — used to fail all 256 moves of its
  batch for one row the prune should have taken. `playlist_entries` and `resume_rows` need no such
  delete, neither being unique on the path. A rename that fails puts back only the steps of its
  own move — the audio and whichever sidecars had already gone — and is refused as
  `Refusal::Unmoved`, carrying the `io::ErrorKind` the volume answered, while the rest of the
  batch goes on; a batch used to be put back whole for one refused file, and since the plan is
  the same next time it failed the same way on every run. A catalog write that fails still runs
  `put_back` over all of `done` in reverse and the whole batch counts `failed`. The order is the
  point: a crash between the two leaves a moved file the catalog has not followed, which is
  exactly what the next scan already reconciles, where the reverse would leave the catalog naming
  files that are not there. `a_move_the_volume_refuses_is_refused_alone_and_the_rest_of_its_batch_lands`
  is the claim, and the batch size is what bounds how much of a run a failed catalog write can
  undo.
- **A batch takes back the folders it made and did not fill.** `renamed_onto` calls
  `create_dir_all`, which says nothing about which levels were new, so `not_there_yet` walks up
  from the destination's parent first and records the ones that are not there; `batch_moved` runs
  `take_back_the_empty` over that list whatever the outcome. `taken_away` only ever removes an
  empty folder, so one a landed file is sitting in survives and one left behind by a rollback goes
  — one path for both, rather than an unwind per exit.
  Deepest first, so a tree comes away a level at a time. What it does *not* do is climb: only what
  this batch made is taken, never a folder that was already standing and happened to be empty.
- **A chain is ordered and only a cycle is refused.** `Planner::in_the_way` answers
  `InTheWay::Stands` for a destination another planned move has already claimed and for one the
  filesystem holds that no scanned row names — with the single exception of a file being moved onto
  itself, which is the same device and inode — and `InTheWay::MayGo` for one that *is* a scanned
  row, because whether that row is itself going cannot be known until every row has been read.
  `Planner::order_the_chains` is where it is decided: a planned move waits on at most one other, so
  the graph is functional and `walked` follows each chain to its end and emits the deepest first,
  which puts `B → C` in front of `A → B` and lands both in one run rather than converging over two.
  A chain that closes on itself is `Collided`, and so is every move leading into one and every move
  waiting on a row that turned out to be staying: breaking a cycle needs a temporary name, and a
  crash between the renames would leave a file under a name nothing knows. A move refused there
  gives its source and its sidecars back out of `going`, so `empties` still counts only the folders
  every file of which is really leaving. `Refusal` is `Unidentified`, `Loose` — a destination with
  no folder between it and the root — `Collided`, `SharesASheet` and `SourceGone`, and every one of
  them is printed against the path it left standing.
- **A rename that crosses a filesystem is a copy, and the source goes only once the catalog has
  followed.** `landed_onto` reads `io::ErrorKind::CrossesDevices` off `fs::rename` and falls back to
  `copying`, which copies the bytes, carries the source's modification time onto the copy — so the
  next scan reads it as the file it already knows rather than as one to probe again, which would
  re-identify a stem-named row against its new name — and `sync_all`s it before anything else
  happens. It is written under the destination's name with `STAGED` after it and renamed into
  place only once it is whole and synced, so a run killed mid-copy leaves a staging file beside
  the destination rather than half a file under its name. The order is what makes it safe:
  nothing is deleted inside the batch, so `put_back`
  undoes a copy by *removing* the copy while the original is still standing, and `left_behind`
  takes the sources away only after `Library::files_moved` has committed. A run killed after the
  rename and before the commit leaves the whole copy standing on the other filesystem beside its
  source, and `already_copied` is what the next run reads it as: another device, the same size,
  the same modification time — the copy carries the source's — and the same bytes, read through
  in `COMPARED_AT_ONCE` pieces only once the three cheap readings agree. Such a destination is
  not in the way, is not copied again, and the move lands as `Landing::Copied` so the catalog
  follows and the source goes as it would have.
  `a_copy_a_killed_run_left_whole_on_the_other_filesystem_is_taken_as_landed` and
  `a_file_of_the_same_size_and_time_but_other_bytes_is_still_in_the_way` are the claims. That is why `Refusal` has
  no `AcrossDevices` variant any more — nothing constructs one.
- **A staging file is written down before it is written, so a killed run's is swept by the next.**
  A staging file stands beside a destination the next run may never plan again — the layout moved,
  the file was retagged — so it cannot be found by walking what a plan names. `noted_staging`
  writes its path and this process's pid into `staged_writes`, the fourth step in `MIGRATIONS`,
  and commits that before a byte of the copy or the rewritten sheet is written; `staged_left`
  takes the file away where it is still there and lets the row go once it is gone, landed or
  not, and a file it could not take away keeps its row. A run that applies begins with
  `sweep_what_a_killed_run_staged`, which takes each noted file away — but only a regular file
  whose name ends in `STAGED`, so a row can never cost a file it did not name — and passes over a
  row whose pid is another process `/proc` still holds, because the `Walk` guard is this
  process's alone and a window and a `resonate organise --apply` beside it may each be copying.
  A preview writes nothing and sweeps nothing.
  `what_a_run_that_was_killed_staged_is_taken_away_by_the_next_run_that_applies` is the claim.
- **A run files the roots it is given, and one the catalog does not hold is refused.**
  `OrganiseOptions::roots` empty is every root, which is what the settings pane and a bare
  `resonate organise` ask for; naming one puts a `roots.path IN (…)` on `TRACKS_TO_FILE`, so the
  rows the run does not want never leave SQLite. The check that each named root is one the catalog
  holds runs on the pass thread rather than in `organise::start`, because `start` may not read the
  catalog before it has the `Walk` guard — a read there blocks behind a writer the guard is waiting
  on, which `a_scan_refuses_to_start_while_an_organise_is_running` is what caught. `--as` is the
  layout for one run, read through the same `Layout::read` `organise-as` is, so a mistyped template
  is refused before anything moves.
- **A file beside a track that shares its name travels with it.** `Planner::sidecars` takes the
  files in the track's folder that are not scanned rows themselves, whose name is the track's stem
  followed by a `.` and something more, and whose extension is not audio — so `Meddle.cue` and
  `Meddle.wav.log` follow `Meddle.wav` onto the destination's stem. A cue-cut file is the exception
  that proves the rule: the rows cut out of one file are filed by the folder their layouts agree
  on and keep the name they already have, because the sheet beside them names that file by name, so
  the sidecar lands under an unchanged stem and goes on naming its audio. Rows out of one file that
  name two different folders are `Unidentified` rather than filed under whichever came first. A
  sheet that cuts *one* row out of a file is the case that does not hold: the file is rendered by
  the layout like any other, so `sheets_follow_their_audio` rewrites the landed sheet after the
  batch's catalog write. `cue::renamed` is the whole of how — it reads the sheet to learn its
  encoding and to check that exactly one `FILE` line names that file, encodes the old and the new
  name in that encoding and splices the one byte run that follows a `FILE` command on its own line
  — `the_run_on_the_file_line`, read a unit of the encoding at a time, so a UTF-16 sheet is walked
  in pairs and a `REM` or a `TITLE` naming the same file is left as it was — so a BOM, a line
  ending and every other byte survive; and `staged_over` renames a staged file over the sheet, so
  a crash mid-write cannot truncate it. A file two `FILE` lines name leaves the sheet as it was.
  A name the sheet's encoding cannot hold can only be Windows-1252's, UTF-8 and UTF-16 holding
  every name, and that sheet is carried into UTF-8 with a byte-order mark rather than left naming
  a file that has gone: `carried_into_unicode` reads every byte around the run through the same
  `legacy` table the reader decodes with — one character a byte, so nothing is lost and every line
  ending stays — and writes the new name between them. The mark is what tells a player that reads
  a sheet without one as the system's code page that this one is not.
  `a_sheet_whose_encoding_has_no_letters_for_the_new_name_is_carried_into_unicode` is the claim.
- **A sheet that cuts a file travels with it whatever it is called, and one that names several
  files takes them all as one move.** The stem rule above only finds `Meddle.cue` beside
  `Meddle.wav`; a rip whose sheet is `Meddle.cue` and whose audio is `CDImage.wav` left the sheet
  behind, and the next scan read the file whole and pruned every row it had cut — their plays,
  their listens, their favourites and their vault links with them. `Planner::sheets_in` reads each
  source folder's sheets once, through the scan's own `read_sheet` and `beside`, and
  `sheets_travelling` weighs every one that names the file being moved: one naming only that file
  is a sidecar that must land — under the renamed stem where it shared the audio's, under its own
  name otherwise — and a destination it cannot take refuses the whole move as `Collided`, where a
  loose sidecar is merely left behind. A sheet naming more than one file that exists — an EAC rip
  with one `FILE` per track — ties them: `Planner::tied_by_sheets` gathers every file such a sheet
  names, and every file any sheet naming one of those names, and `file_together` plans them as
  **one `Move`**, the first file its `from` and `to` and the rest its `companions`, with the sheet
  a sidecar landing under its own name. One move is what keeps them together through everything
  after the plan: a batch never splits it, `renamed_onto` puts every file back where one of them
  fails, `files_moved` has the catalog follow each file, and `sheets_follow_their_audio` renames
  every `FILE` line in the sheet that landed. `Move::files` is the one walk all of them take, and
  `Plan::files_moving` is what a preview counts. The layout must land every file in one folder
  and every file must be one this pass files, or each file is `Refusal::SharesASheet`, the one
  that failed on its own account taking its own refusal instead, and such a sheet is kept out of
  the loose stem pass for the same reason. **A unit is a link in a chain like any move.** A
  destination held by a file this pass moves on is not a refusal but something to wait for, and a
  unit may wait on as many as it has members: `Planned::waits_for` is every source standing where
  the move lands, and `order_the_chains` puts a move behind each move that vacates one of them.
  `walked` is a depth-first topological order over those waits rather than the walk of one chain
  it was, iterative so a long chain costs no stack: a move is ordered once everything it waits on
  is, and a move that meets one still being walked — a cycle — or one already doomed dooms every
  move on the walk with it, because each of those waits on it in turn. A move left out is refused
  as `Collided` with the first source it waited on, each member of a unit alike. A unit whose
  member lands where another member stands waits on itself and is doomed with it, which is the
  refusal it always met: a rename inside one move cannot be ordered. At the apply, `standing`
  weighs every file of a move, so a chain the disc has changed under since the plan is still
  refused rather than renamed over.
  `the_files_a_sheet_names_wait_for_a_file_standing_where_one_lands_to_move_on_first` is the
  claim.

## The search grammar

- **A search is words and terms, and what a term means is typed.** `Search::read` is the whole of
  the grammar: a token is a word unless it names a field, where `title:`, `artist:`, `album:`,
  `genre:` and `lyrics:` (or `lyric:`) scope the words beside them and `year:`, `added:`, `plays:`, `played:`, `length:`, `rate:`,
  `depth:`, `codec:` and `is:` are `Term`s — each with the aliases `KEYS` lists, `heard:`,
  `duration:`, `samplerate:`, `bits:` and `format:`. A token naming no field, or one whose value the
  grammar cannot read, is the words it was written as — the rule `lrc.rs` follows for a bracket that
  is neither a moment nor an id tag — so a search never fails to parse and the box stays live as it
  is typed, at the cost of a mistyped term going quietly. Every numeric reader in it is written to
  be overflow-safe, so a value past what its type holds is one the grammar cannot read rather than
  one it panics on: `length:400000000000000000:30` is three words. `Display for Term` is the canonical text
  and reads back as the same term, so the window's *Reads* row, `resonate playlist --query` and the
  grammar's own tests all say the same thing rather than each writing prose of its own. `is:hires`
  is lossless above CD, so `CD_SAMPLE_RATE` and `CD_SAMPLE_DEPTH` in `db.rs` are where that claim
  lives, and `depth:` reads the stored `SampleFormat`, which is 24 valid bits for a float file.
- **What a term narrows on is what the catalog stored, and two of them are ages rather than dates.**
  `added:` and `played:` are measured from the moment the query runs — a month is 30 days however
  long the month was and a year is 365 — which is what makes "added this year" `added:<1y` and what
  makes a saved query answer differently tomorrow. `plays:` is a count beside them, so `plays:0` is
  what has never been heard and `plays:>5` what has, and `heard:` is the other spelling of
  `played:`. A `@` after the count is a window over `listens` rather than the lifetime column:
  `plays:>20@30d` is more than twenty plays inside the last thirty days, the age after it reads
  exactly as `added:` and `played:` read theirs, a range carries the window on both bounds
  (`plays:5-10@30d`), and two `Plays` terms fold back into a range only where their windows agree.
  A window the grammar cannot read makes the whole token a plain word, as everywhere else. What it
  costs is the one term no index serves — a correlated `count(*)` over `listens_by_track` per
  candidate row — which is why it narrows and never orders. `year:` is the *album's*, so a track whose album declares no year answers no year term
  at all and a single that was never grouped has nothing to answer with. `rate:` and `depth:` are
  what the file is rather than what the sink is asked for, which is what the inspector reports
  instead, so `depth:32` names the 32-bit integer files alone. `is:lossy` names the codecs known to
  be lossy rather than everything that is not lossless, so a file whose codec the scan could not
  name is in neither it nor `is:lossless`. Nothing narrows on a rating, because the catalog stores
  none, and nothing sorts on a term either: a term narrows and `SortOrder` orders. A search is
  parsed on every keystroke and a saved query parses its text on every read, and nothing caches
  either, because it is a handful of tokens rather than a cost worth holding.
- **Everything a search says has to hold at once, unless a `-` denies it or an `or` offers the
  alternative.** `Search` is `Clause`s that all have to hold, a `Clause` is `Asked` alternatives one
  of which must, and an `Asked` is the `Condition`s one token read together with whether it was
  denied — which is why a range is a shape rather than two searches: `year:1970-1979` is the two
  bounds that both hold, and `-year:1970-1979` is each bound denied in one clause, De Morgan done at
  the parse so nothing downstream has to bracket. `Display for Asked` folds an undenied pair of
  bounds back into the range it was typed as, so `year:1970-1979 or is:hires` reads back with the
  alternation against the whole range rather than against its second bound, and a denied range is
  already its two denials joined by `or` and reads back as itself. `-` denies only at the start of
  an unquoted token, so `well-known` and `1970-1979` are untouched, and `or` joins only between two
  tokens and only unquoted and undenied, so a dangling one is the word it was written as — the same
  rule a mistyped term follows, and quoting is the way to search for either literally. There is no
  bracketing: a denial reaches one token and an alternation one flat run of them, so `-a or b` is
  "not a, or b" and `-(a b)` cannot be written — De Morgan by hand is what says it. `and` is not a
  word the grammar knows, because everything is already joined by it.
- **`tracks_fts` holds the fold of a name, not the name, and the query is folded with it.**
  What is indexed is `store::folded_letters` of the title, the billed artist and the album, and
  `db::indexed` folds every piece of a typed word the same way before it becomes a `MATCH`, so the
  two sides can only ever disagree by being changed apart. What that buys is a letter with two
  spellings: the tokenizer's `unicode61 remove_diacritics 2` folds `Ç` to `c` and even `İ` to `i`,
  but it leaves `ı` alone, because a dotless i is a letter of its own rather than an i with
  something taken off — so *Kıskanç* was reachable only by typing the dotless ı and *KISKANÇ* only
  by not typing it. The same fold is what lets *Przybylowicz* find *Przybyłowicz*.
  `remove_diacritics` stays on the tokenizer although the fold has already done its work, because
  it costs nothing and the index should not rest on the fold being complete. Nothing reads a
  column of `tracks_fts` back — only `MATCH` and `rank` — so there is no spelling to keep beside
  the fold.
- **What a search lights up is read off the name, not out of the index.** `Search::lit` answers the
  byte runs of a display string a search matched, for one `Column`, which is what the panes draw in
  the accent. It cannot come from FTS5: `highlight` and `snippet` answer with the *stored* column,
  which here is the fold, so a row would read `bjork` where the pane draws *Björk*. So it folds the
  other way round — the name is split into tokens on `char::is_alphanumeric` and each token is
  folded whole, which keeps every run's byte range in the original — and weighs each token against
  the same `search::pieces_of` that `db::indexed` builds the `MATCH` from, so what is lit is what
  matched: a prefix for a bare word, consecutive tokens for a phrase, nothing for a denied word or
  a term, and a word scoped to a column lights nothing in another. A whole token lights rather than
  the matched prefix alone, because a half-lit word reads as a typo. Runs that meet are merged, so
  a word typed three ways lights its token once. **The words are taken before the name is**, because
  `lit` is called once per drawn cell per frame and the tokenising it used to do first — a `Vec`
  and a `store::folded_letters` per token, each of which lowercases into one `String` and
  NFD-normalises into a second — ran even where the search box was empty and no word could reach
  that column. A pane of eighteen rows with two lit cells each was some five hundred `String`s and
  as many normalising passes a frame, for nothing. **It is a schema break without a migration**: an index written before the fold holds
  spellings no folded query will match, and the answer is to delete the catalog and scan again,
  because an incremental rescan passes over a file it has already seen and never rewrites its row.
- **What a track sings is indexed, and only a word that asks for it reaches it.** `tracks_fts`
  holds a fifth column, `lyrics`, and `store::index_row` fills it itself — `sung_by` reads the
  row's own `tracks.lyrics` or, where the file carried none, the `lyrics_kept` a provider fetched,
  and `sung_words` drops every `[…]` and `<…>` run before folding, so an LRC's timestamps and a
  karaoke line's word marks are not words to find. `Library::keep_lyrics` rewrites the column for
  the rows at that path and span whose file carried none, in the transaction that keeps them, so a
  lyric fetched while a track plays is searchable at once and a rescan indexes the same words
  again. A bare word is written `{title artist album genre} : "…"*`, `Column::NAMES` being the
  columns it reaches, so typing *love* does not bring back every song that sings it; `lyrics:` is
  how a listener says they mean the words, and `Search::as_sung` is the turn from a search of
  nothing but plain words into the one phrase `lyrics:"…"` — which `Library::sung` counts, so the
  window can offer it. `spelling.rs` holds no vocabulary for the column and corrects no lyric
  word. `a_track_is_found_by_the_words_it_sings_and_only_when_they_are_asked_for` and
  `a_search_of_plain_words_is_offered_as_the_words_a_track_sings` are the claims. It is a schema
  break, and a catalog written before it is deleted and scanned again.
- **A search reaches what the catalog lacks as well as what it holds.** `release_tracks.folded`
  is the row's title, its artist — the track's own credit, or the release's — and the release
  title, run through `store::folded_letters` by `land_release`, and `Library::unheld_matching`
  asks it one `LIKE` per piece of every word a search asks by name: the lone words, bare or
  scoped to a title, an artist or an album, which is `elsewhere::words_asked`. A term, a denial, a
  genre and a lyric have nothing to answer with on a row nobody holds, so a search of those alone
  answers nothing. It lists wanted rows first, and it and the Missing pane read the same
  `SHORT_OF_WHAT_IS_HELD_OR_WANTED` rule: a release row is missing where its album holds a track
  or where the row itself is wanted, so a release landed for one song does not list the eleven
  nobody asked for. `a_search_reaches_the_rows_the_catalog_knows_it_is_short_of` is the claim.
- **A song the catalog has never heard of is found elsewhere and wanted by landing its release.**
  `Library::found_elsewhere` sends the words a search asks by name to `Reference::find_songs`
  and answers `Found`s: a recording, its title, its credit, its length and the release it first
  came out on — `elsewhere::first_released`, the earliest dated — with every recording the
  catalog already names in `tracks.mbid` or `release_tracks.recording_mbid` left out and a second
  recording of the same folded title by the same folded credit dropped, up to
  `FOUND_ELSEWHERE_AT_MOST`. `asks_elsewhere` is the guard a caller weighs first: a search whose
  words hold fewer than three letters is not sent. `Library::want_found` is the want: the release
  the `Found` names, or the one its recording first came out on where the search answered none,
  is read whole through `Reference::release`, an album already carrying that mbid is taken as it
  stands, and otherwise a new one is made — billed to `store::artist_named`, stamped
  `albums.found_elsewhere` — and `land_release` writes its rows the way the enrichment does, so
  the want is an ordinary `wants` row a provider is asked for and a delivery lands on. A release
  the reference does not know is `Error::UnknownRelease`, a recording with no release is
  `Error::Unreleased`, and a release that turns out not to carry the recording is
  `Error::NotOnTheRelease`. `store::ORPHANS` spares an album holding no track only where it was
  found elsewhere and still wanted, so a scan keeps it while the want stands and takes it once it
  goes, and an album scanned from a root still leaves with the root. The albums pane never shows
  one, because `HOLDS_A_BEST_COPY` already asks for a track.
  `a_song_found_elsewhere_is_wanted_by_landing_the_release_it_first_came_out_on` and
  `a_song_with_no_release_named_is_wanted_from_the_one_its_recording_first_came_out_on` are the
  claims.
- **A word that stands alone is ranked; one that is denied or alternated is looked up.** The
  unnegated, unalternated words are what `indexed` folds into the single FTS5 `MATCH` the index is
  joined for, so `rank` and `SortOrder::Relevance` mean what they always did. Any other word reaches
  the `WHERE` as `INDEX_LOOKUP`, a subquery against the same index that scores nothing, so a search
  whose every word is denied or alternated has no join at all and relevance falls back to album
  order. A denial is written `NOT coalesce(…, 0)`, so a row that cannot answer the condition — no
  album for `year:`, no duration for `length:` — satisfies the denial rather than dropping out of
  the search. `Matching::grouped` carries the lot into the album and artist listings, so one text
  narrows every browse pane rather than the tracks pane alone.
- **A search that matched nothing is answered in the catalog's own spelling, and only then is the
  catalog read for one.** `spelling.rs` is the whole of it. `Spellings` is three `Vocabulary`s —
  titles, artists and albums, mirroring the three columns `tracks_fts` holds — each mapping a
  folded word to the spelling it is drawn in and how many rows hold it, and `store::spellings`
  fills them from `tracks.title`, `tracks.artist`, `artists.name` and `albums.title` through the
  same `search::runs_in` splitting that lights a matched run, so a vocabulary word is exactly a
  word a search could match. `Spellings::did_you_mean` then walks the parsed `Search` and corrects
  the words in place, so a term, a denial, a phrase and a scope all survive: `year:1973` is carried
  through untouched, `-floid` is left alone because a denial is not what a listener mistyped, and
  `title:` weighs its word against the titles alone. What comes back is the whole query written
  again through `Display for Search`, which is the same rendering the *Reads* chips already draw,
  so what a press puts in the box reads as what the pane was already saying.
- **A word is corrected only where the catalog cannot already match it, and never further than it
  can afford.** `Vocabulary::holds` passes over a run the index would have found — one the
  vocabulary holds outright *or* one that begins a word it holds, because a bare word is matched as
  a prefix — so *floy* is not corrected to *Floyd* while *floid* is. `furthest_from` is the budget:
  nothing under four letters is corrected at all, a word up to seven may be one letter wrong and a
  longer one two, and `apart_by` is a bounded optimal-string-alignment distance, so two letters
  typed the wrong way round cost one rather than two — which is what most mistypings are. The
  nearest wins, then the word the most rows hold, then the spelling itself, so the answer is the
  same twice running; among spellings of one word the marked one is drawn, the same rule
  `folded_letters` follows for an artist. A suggestion is only ever a word the catalog holds, so it
  cannot send a listener at a search that matches nothing in turn.
- **A run-together and a split are corrections too, and neither invents a word.** `Spellings::whole`
  is the *exact* reading of a vocabulary — the entry a folded run names, rather than the nearest one
  — and it is what both rest on: `run_together` answers where two runs joined name one held word
  and the two apart do not both name one, and `split_apart` answers where one run cuts into two
  that each do. So *pinkfloyd* is **Pink Floyd** and *pink floy d* is **pink Floyd**, where a word
  at a time could read neither — `floy` being a prefix `Vocabulary::holds` passes over, and
  `pinkfloyd` being four letters from anything. The three readings are weighed in order: the join
  first, because it explains two runs where the others explain one; the ordinary nearest word
  second, because a mistyped letter is the commoner accident; the split last. A split cuts into as
  many pieces as the run needs, up to `MOST_PIECES`, and it is a walk rather than a scan of the
  cuts: `reached[to][pieces]` carries the most rows a segmentation of the first `to` letters into
  that many held words can hold, so what is written is the fewest pieces the whole run cuts into
  and, among those, the most rows — *thegreatgig* is **the Great Gig**, which a single cut could
  not reach, neither *thegreat* nor *greatgig* being a word the catalog holds. It is refused below
  the length `furthest_from` refuses to correct at, so one rule bounds both.
  `run_tokens_together` is the join that spans two *tokens* rather than two runs of one word,
  because *pink floy d* is three clauses and not one word: it takes only a clause that is a single
  plain undenied word of one run scoped the same way as its neighbour, so a phrase, a denial and an
  alternation are all left as they were. What a split writes is two words where there was one,
  which reads back as two words — unless the word was scoped, where it is written as a phrase so
  `artist:` reaches both halves rather than the first alone. `worth_asking` weighs an adjacent pair
  joined as well as each run alone, so *flo yd* is worth the read that *flo* and *yd* are not.
- **A phrase is weighed against a whole name before it is corrected a word at a time.**
  `Vocabulary` holds the names beside the words — every title, artist and album of more than one
  run, keyed by its runs folded and joined by a space, spelt the way `better_spelt` picks for a
  word — and `instead_of_the_whole` is the reading: a quoted token is weighed against them first,
  under the budget its own whole length earns from `furthest_from`, and only where nothing is near
  it does the run-by-run walk take over. That is what a word at a time cannot reach:
  *"the great gig in teh sky"* is **The Great Gig in the Sky** although `teh` is three letters and
  nothing that short is ever corrected on its own. `names` is `holds`'s counterpart and guards it
  the same way, so a phrase that *begins* a name the catalog holds is left alone.
- **A run of tokens is weighed as a name too, and only where a word in it is beyond correcting.**
  Quoting is how a listener says *this is one name*, and most of them do not: the same title typed
  without quotes is several clauses, so `name_the_tokens` gathers a run of adjacent clauses that
  are each a single plain undenied word of one run scoped the same way — the reading
  `run_tokens_together` already takes, through the same `one_run_of` — joins them with a space and
  hands the result to `instead_of_the_name`, which is `instead_of_the_whole`'s second half and the
  one place a name is weighed. It shrinks from the longest run down to two, so the most specific
  reading wins and a token beside the name is left where it stands rather than drawn into it; a
  term, a denial or a change of scope ends the run rather than being read through, and
  `MOST_TOKENS_IN_A_NAME` bounds how long a run may be at all.
  What keeps it from overreaching is `beyond_a_word`: a run is weighed as a name only where one of
  its tokens is a word the catalog neither holds nor can spell nearer, which is exactly the case a
  word at a time cannot reach. So *the great gig in teh sky* is **The Great Gig in the Sky** and
  *pink floid* is still **pink Floyd** rather than *Pink Floyd* — the narrower fix stands wherever
  it is enough, and the catalog's own casing is not written over a word the listener typed right.
  The name is written back unquoted, so what is offered means what was typed: a run of words the
  search still ANDs, with the spelling put right — unless the run was scoped, where it is written
  as a phrase the way a split is, because `artist:Pink Floyd` reads back as `artist:Pink` and a
  loose `Floyd` over every field. The budget `furthest_from` answers is weighed in letters through
  `letters_in` rather than in bytes, so a folded Cyrillic or CJK word earns what a Latin one of
  the same length does and *мир* is not offered as *мор*.
- **The read is the cost, so it is paid once and kept until a name could have moved.**
  `Library::did_you_mean` walks every title, artist and album name in the catalog, which is why
  the window asks only where the albums, the artists and the tracks all came back empty —
  `browsed` reads them first and asks afterwards, on the background executor like the rest of the
  load — and why `spelling::worth_asking` refuses before the read wherever the query holds no word
  long enough to be corrected. What that read built is then kept: `Inner::vocabulary` stamps the
  `Spellings` with a counter and hands back an `Arc` of it until the counter moves, so a listener
  typing past the end of what they hold pays one read rather than one per settled keystroke.
  **What moves the counter is SQLite itself.** `watch_the_names` lays `NAMES_MOVED_TRIGGERS` on
  the writer — `TEMP` triggers, so they live on that one connection and never reach the schema, a
  migration or another program writing the catalog — which step a row of a `TEMP` table wherever
  a row of `tracks`, `albums`, `artists` or `artist_genres` is inserted or deleted, or one of the
  columns the vocabulary is read from — a track's title, artist and genre, an album's title, an
  artist's name — takes a value other than the one it held; an `update_hook` on the writer sees
  that temporary row move and steps the counter. So the invalidation is derived from what was
  actually written rather than from a list of write paths somebody has to keep in step — a pass
  written tomorrow is covered by having used the writer at all — and it is per *column*: a counted
  play, a favourite, a scan writing a title back as it stood drop nothing. The update hook answers
  per table and per row and SQLite offers per column only through `sqlite3_preupdate_hook`, a
  compile-time flag on the bundled library, which is why the triggers do the weighing and the
  hook only the counting; they cost some 0.35 µs a row of a scan's inserts. What it is not is
  another process's writer, which this one cannot see. That half is `PRAGMA
  data_version`, read off the writer connection beside the counter into one `NamesStamp`: it moves
  only for a commit some *other* connection made, so a `resonate scan` running beside a window
  drops the window's vocabulary the next time it is asked for, and this process's own writes are
  still the hook's alone. It is read under `try_lock`, because the writer may be held through a
  whole scan batch and a search must not wait behind one; a busy writer is this process writing,
  and the stamp is then weighed by the counter alone. Every delete on the three tables carries a
  `WHERE`, so the truncate optimisation — the one case SQLite skips the hook for — cannot arise.
  `Library::written_elsewhere` hands the same reading out as a `WrittenElsewhere`, which is what
  the window watches to see another process's edits — see `ui.md`.

## Playlists

- **A playlist is a list of cuts, not of library rows.** `Cut` is a `MediaLocation` and the
  `Option<FrameSpan>` of it, which is what a playable item has been everywhere else — `Track`,
  `QueueItem` and `Resumable` all carry the pair — and `playlist_entries` now stores it as
  `path`, `span_start` and `span_frames` under the same `store::span` convention `tracks` and
  `resume_rows` use. A file no scan has seen is still a row, and a file that leaves the library
  leaves its playlists named but unresolved. Reading one back is a `LEFT JOIN` onto `tracks` on
  `path` *and* `span_start`: a `PlaylistEntry` carries its `Cut` always and its `Track` only where
  the catalog has one, which is the same fallback the queue pane draws for an unscanned row.
- **A row cut out of a file is the cut rather than the file, all the way to the graph.** The join
  used to take the lowest `span_start` the path held, so the three rows a `.cue` cuts out of one
  FLAC all drew as the first of them, counted that one's length three times over and played the
  whole file; `Held` and `Reaching` carried bare locations, so a drag lost the span before the
  edit did, and both `queue_items` built `span: None` even where the joined `Track` had one.
  `Cut::of` is the one turn from a catalog row into a queueable cut and `Cut::whole` is what a
  sheet's path, a command-line file and a bus `AddTrack` all are, none of the three formats
  having a vocabulary for a region. Folding doubles reads the whole row rather than the path, so
  two cuts of one file are two rows while the same cut twice is one.
  `a_cue_row_put_in_a_playlist_is_the_cut_it_was_rather_than_the_file_it_came_out_of`,
  `the_rows_a_sheet_cut_are_a_playlist_of_their_own_lengths` and
  `two_rows_one_sheet_cut_out_of_a_file_are_not_doubles_of_each_other` are the three claims. An edit writes only the rows
  it moved: appending goes at `max(position) + 1`, removing shifts the rows after it, moving shifts
  the span between the two and tidying rewrites from the first row whose file has gone. A shift
  parks the rows at `-1 - position` and unparks them in a second statement, because
  `PRIMARY KEY (playlist_id, position)` refuses a collision even a transient one and SQLite does not
  promise the order an `UPDATE` visits rows in. `position` stays dense, which is what lets a row's
  index and its position be the same number, and it is why every span is weighed against the list
  before it is written rather than cast to an `i64` and trusted: `move_in_playlist` answers false
  where either end of the span or the row it is dropped on is past what the playlist holds, and
  `remove_from_playlist` takes the rows from the first named to the end of the list and refuses a
  span that starts past it. A span of any length costs the same two writes one row does, which is
  the whole reason `Span` reaches the SQL rather than the pane sending one edit per row. What every
  edit that moves a row reads is the whole playlist, because `undo::edited` takes a restore point
  before each: an append that wrote one row reads a thousand paths on a list of a thousand. Only the
  local source can be in one — `add_to_playlist` refuses a `MediaLocation` that names another.
- **A name is one name however it is written, and the column is what says so.** `playlists.folded`
  holds `playlist::folded` — Rust's `to_lowercase`, which folds every alphabet rather than
  SQLite's `NOCASE` ASCII, and then NFC, which folds the spellings of one letter into each other —
  and carries the `UNIQUE`, so two spellings of one name cannot both be
  in the table whatever `refuse_duplicate` does. `Library::playlist_named`, `HOLDS_THE_WORD` and
  `PlaylistOrder::Name` all read that column, so addressing, narrowing and ordering fold the same
  way, which is what lets `resonate playlist <NAME>` and the pane address a playlist by its name.
  The order is lowercase and then compose, because `to_lowercase` does not promise composed output;
  it is one function applied to what is stored and to what is asked, so the two cannot disagree. It
  is NFC rather than NFKC: "Café" typed with a combining acute is the playlist typed with a
  precomposed one, while ﬁ and fi stay two names, because folding a ligature is a different claim.
  What the column holds is written when the row is, so a database from a build before the fold
  changed keeps the folds it had — which is the "delete it and rescan" the schema note above
  already assumes.
  `Library::rename_playlist` refuses a spelling another playlist already holds through
  `refuse_duplicate`, which excludes the playlist being renamed, so a name re-cased is not a
  duplicate of itself. Renaming and discarding are not row edits, so a saved query takes both the
  way a list does; `resonate playlist <NAME> --rename <NEW>` and `--discard` are the same two
  gestures on the command line, where there is no undo behind them.
- **Every row edit refuses inside the transaction that writes it.** `only_a_list` asks whether the
  playlist is there and whether it fills itself from a query, against the transaction rather than a
  reader connection, so `Error::UnknownPlaylist` and `Error::NotAList` cannot be answered from a
  state the write no longer sees. The three passes that drop rows share the same discipline:
  `Going` is the question — `Gone`, `Doubled` or `Matching` — and `asked_of` asks it of the rows
  `numbered` read inside that transaction, so no closure names a row it never looked at. What it
  costs is a refused edit still paying the whole-playlist read `undo::edited` takes before it.
  `copy_playlist` is the exception by design: it reads the source outside the transaction it lands
  in, because a copy is the source as it stood.
- **A playlist either holds a list or fills itself from a query, and both answer through
  `playlist_entries`.** A row in `playlist_queries` is what makes one a saved query: the search
  text, the `SortOrder` and the row cap, which is a `TrackQuery` with the album and artist left out.
  `Library::playlist_entries` runs it rather than reading `playlist_entries` where one is there, so
  the queue, the pane, MPRIS and every sheet writer take a query playlist for an ordinary one and
  need to know nothing. The counts a listing carries are the query's too, which costs one `count(*)`
  per saved query on top of the grouped pass. Editing the rows is refused — `Error::NotAList` out of
  `add`, `remove_rows`, `move_rows` and `prune` — because there is no row there to move; renaming,
  playing, exporting and dropping all work as they do for a list. What a query has instead is
  `Library::revise_query`, which rewrites those three columns where they stand and refuses a list
  with `Error::NotAQuery`, the mirror of `NotAList`, so each kind refuses exactly the edit the other
  takes. A query holds no album or artist id, so saving one while an album is selected cannot
  quietly widen to the library: the window offers *Save this search* only where the search box is
  what scoped the pane, and `resonate playlist <NAME> --query` is the same gesture on the command
  line, saving one under a name nothing holds and revising the one already named. A revision is the
  whole edit: nothing keeps what a query held before it was changed. What a query holds is read at
  the moment it is asked, so it can answer differently twice in a row and a queue loaded from one is
  a snapshot — nothing re-reads a query into a queue already playing it. `SortOrder::Plays` or
  `Played` beside a row cap is what writes a *Top 25 most played*, and `plays:0` is what has never
  been heard. `Library::playlist_lists` is the read for the one gesture that can only reach the
  other kind — the window's picker, which puts rows in a list — so it is the listing's own grouped
  pass with the queries taken out in SQL and no `count(*)` behind it.
- **A list is put in order by the library, not a row at a time by the pane.**
  `Library::sort_playlist` takes a `RowOrder` and a `Direction` and rewrites the positions where
  they stand — one `DELETE` and a dense reinsert from the first row the order moves, rather than the
  park-and-unpark pair a move costs — and answers with how many rows moved, so a list already in
  that order costs no write and moves no revision. `RowOrder::Album`, `Artist`, `Title` and `Length`
  read the catalog through the same columns the tracks pane orders by and put a row no scan has seen
  at the end whichever way round the order is read; `RowOrder::File` reads the path every row
  carries, so it is the one order that places the unscanned rows too. `Length` is seconds rather
  than frames, because two files at different rates do not count time the same way. A saved query
  refuses it with `Error::NotAList` like every other row edit, because its order is the query's. It
  is an edit rather than a property, so a list in hand still puts the next row appended at the end;
  what makes an order a property is `Kept` below. The pane reaches it through the opened playlist's
  *Sort* control, which opens the same *In order* and *Reading* chips the index carries, and
  `resonate playlist <NAME> --order <ORDER> [--reverse]` is the same gesture on the command line.
- **A list is either in hand or kept in an order, and a kept one refuses the edits that place a row
  by hand.** `Library::keep_playlist_in_order` takes an `Option<Kept>` — a `RowOrder` and a
  `Direction` — writes it to `playlists.kept_order` and `kept_reading` and puts the rows in it in
  the same transaction, so keeping a list is the gesture that sorts it once and then holds it.
  `playlist::add` runs that same pass after it appends, which is what lands a row added by hand, by
  `resonate playlist --add` or by an imported sheet where the order says rather than at the end.
  What a kept list refuses is `move_in_playlist` and `sort_playlist`, both with
  `Error::KeptInOrder` — the mirror of `NotAList`, because its order is the sort's and not a
  hand's — while removing, tidying, renaming, exporting and adding all work as they do for a list in
  hand. `keep_playlist_in_order(id, None)` puts one back in hand and leaves the rows where they
  stand. A saved query refuses to be kept at all, with `NotAList` like every other row edit, because
  its order is already the query's. The window reads the states through `views/playlists.rs::Rows`:
  `InHand` is moved and edited, `Kept` is edited and not moved, `Matched` is neither, so the movers,
  the drag, the reach and the ✕ each ask the one question they care about rather than testing for a
  query. The *Sort* control carries the choice as a third chip row, *Keeps*, and the index draws a
  kept list under the sort mark rather than the playlist one;
  `resonate playlist <NAME> --order <ORDER> --keep` and `resonate playlist <NAME> --by-hand` are the
  same two gestures on the command line. Keeping a list in the order it is already kept in, or
  putting one back in hand it is already in, moves nothing and so writes nothing: `keep` answers
  `Change::Nothing` there, the way every other edit that wrote nothing does. What keeping costs is
  the whole list read on every append: the order is re-read and the rows rewritten from the first
  one it moves, so a row sorting to the end of a kept list of a thousand costs the one write a list
  in hand costs and a row sorting to its top still costs a thousand. The order is only re-read when
  the list is written to, so a scan that renames a track or fills in the album it belongs to leaves
  a kept list in the order it had — nothing re-sorts one on its own.
- **What a search showed is what a drop takes out, and nothing empties a list in one gesture.**
  `Library::remove_matching` reads the positions a search matched and rewrites the list from the
  first row that goes, which is the pass `prune_playlist` already took — they share `dropped_where`,
  because a narrowing breaks the adjacency a `Span` needs and both are one question asked of every
  row. A saved query refuses it with `Error::NotAList` like every other row edit; a kept list takes
  it, because dropping a row places none. It asks for the text rather than an `Option<&str>` the way
  `copy_playlist` does, so there is no call that empties a list: one with nothing left in it is one
  to discard. The window draws *Drop shown* beside *Copy* only under `Rows::Narrowed`, and
  `resonate playlist <NAME> --matching <TEXT> --drop` is the same gesture on the command line, which
  is why `--drop` requires `--matching`.
- **A row doubled is a row to fold away, and the first of each is the one that stays.**
  `Library::fold_doubles` reads the paths a list holds and drops every row naming a file an earlier
  row already named, through the same `dropped_where` pass `prune_playlist` and `remove_matching`
  take — a third question asked of every row rather than a third way of rewriting one. It is the
  companion to `copy_playlist`, which reconciles nothing and so doubles what it lands, and to
  `add_to_playlist`, which lets the same file be put in twice deliberately: nothing folds on its
  own, because putting a track in a playlist twice is a thing to be able to do. It reconciles on the
  path the way `import_playlist` does, so two names for one file are two rows to it and stay two. A
  saved query refuses it with `Error::NotAList` like every other row edit; a kept list takes it,
  because dropping a row places none. The window draws *Fold doubles* beside *Tidy*, and
  `resonate playlist <NAME> --fold` is the same gesture on the command line, which is why `--fold`
  and `--tidy` refuse each other: each is a whole gesture and the pass is not shared. It is one
  press for the whole list — nothing folds the doubles out of a span, and nothing says which rows a
  press would take before it takes them.
- **A playlist is copied into another, never moved into it.** `Library::copy_playlist` reads one
  playlist's rows — the whole of it, or only what a search matched — and lands them through
  `add_to_playlist`, so the source keeps them, a kept target puts them in its order, and a saved
  query refuses to receive them with `NotAList` like every other row edit. `Error::IntoItself`
  refuses both sides being the same playlist, because that would only double it. Copying out of a
  query is what freezes a search into a list, and the window offers no separate gesture for it: the
  index row's +, an opened playlist's *Copy* and each row's + all open the one picker
  `hold_for_a_playlist` owns, which carries a `Held` — the rows and the playlist they came out of —
  so the picker can leave that playlist off its own list and say *Copy* rather than *Add*. The
  picker's *or a new one* is therefore what duplicates a playlist, and
  `resonate playlist <NAME> --into <OTHER>` is the same gesture on the command line, creating OTHER
  where nothing is named that. What it costs is the whole source read into memory as locations and a
  write per row, plus the order re-read where the target is kept in one.
- **A search narrows a playlist, and what is shown is what plays.** `Library::playlists` takes the
  words a search holds and matches each against the name, because a playlist has only a name to
  answer with: a term is passed over, and a search holding no standalone word leaves the index
  whole. `Library::playlist_entries` takes the whole text and runs it against the catalog, so a row
  no scan has seen answers nothing and falls out, and a saved query's own text and the typed one
  both have to hold. A `PlaylistEntry` carries its `position` for that reason — a narrowed row still
  knows where it stands, so the number it draws and the row a ✕ drops are the list's rather than the
  view's. `Rows::Narrowed` sits beside `InHand`, `Kept` and `Matched`: edited, not reached and not
  moved, because a reach is a run of adjacent rows and a `Span` is what reaches the SQL, and
  adjacency is what the narrowing broke. What acts on the playlist as a whole — Rename, Sort, Tidy,
  Export, Discard — acts on the whole whatever is shown; what acts on rows — Play, Play next, Add to
  queue, Copy, Drop shown and every row gesture — acts on what is shown, which is why playing a
  narrowed playlist leaves `Library::playing_playlist` unset: the queue is a part of it rather than
  the thing itself. `resonate playlists --named` and `resonate playlist <NAME> --matching` are the
  same two gestures on the command line. A saved query's own text and the typed one are read
  *beside* each other rather than joined into one string: `db::matching` takes the texts, reads each
  through `Search::read` and asks their clauses together, so an unbalanced quote closes at the end
  of the text it was written in and a trailing `or` stays the word it was. Both run under the
  query's own cap, so the cap falls on what the two match rather than on what the query alone would
  have, and the count the listing carries is still the query's. Nothing says which of the two a row
  answered, and the same box is what *revises* a query through *Edit search*, so the text narrowing
  one and the text defining it are the same field read two ways.
- **A playlist listing is an order and a `Direction`, and whoever draws it picks both.**
  `Library::playlists` takes the two and `order_by` writes the sense into the SQL, so a reading is
  the query's rather than a `reverse()` over what came back — which is what leaves a playlist
  nothing has played at the bottom of *Most recent first* and the top of *Longest ago first*, where
  SQLite puts a NULL. `PlaylistOrder::reads` is the direction an order *opens* at: ascending for
  `Name`, descending for the other four, because a person expects the newest or the most played at
  the top. That is a default and not a rule — the pane's *Reading* row and
  `resonate playlists --reverse` each turn one around, and choosing an order resets the reading to
  what that order opens at. The bus opens ascending whatever the order, because the MPRIS Playlists
  spec defines `CreationDate`, `ModifiedDate` and `LastPlayDate` as oldest first, and maps its own
  `reverseOrder` onto `Direction::Descending` rather than reversing a list it has already read.
- **Which playlist is in play is the library's, and it is the queue it was loaded as that keeps
  it.** `Library::playing_playlist` is a cell beside the catalog rather than a column, and what it
  holds is a `Playing` — the `PlaylistId` and the `QueueStamp` that `engine::stamp_of` reads off
  the rows the queue was loaded with, their locations and spans rather than the ids they were
  handed. It answers `Library::playing_playlist(queue)` only where the two stamps agree, so the cell
  *is* the claim that the queue is that playlist rather than a note every caller has to remember to
  tear up. `Queue::rows_changed` restamps whenever a row arrives or leaves and the engine publishes
  it as `PlayerState::queue_stamp`, so an `Insert` or a `Remove` takes the badge off wherever it
  came from — `RootView::queue`, a row's ✕, or `AddTrack` and `RemoveTrack` over the bus without the
  window in between. A reorder restamps nothing, because `move_rows` and `set_shuffle` move `order`
  and not `items`, so a queue move and a shuffle both leave the playlist in play and
  `PlayerState::loaded_position` still names the playlist row being heard. Three callers set it,
  each stamping the items it is about to send: `RootView::play_playlist`, `Collection::activate` and
  the binary's `play_queue`. MPRIS's `ActivePlaylist` reads the same cell through the same stamp, so
  the bus and the pane cannot disagree about what is on. `QueueItem` derives no `Hash`, so `engine::stamp_of`
  is the only way a queue's rows can be stamped: `Collection::activate` once hashed whole items,
  ids and all, which no queue the engine publishes could match, and `ActivePlaylist` never named
  the playlist the bus had just activated. It holds only a playlist the catalog holds,
  because `set_playing_playlist` fills it from whether `played_now` counted the play:
  an id nothing holds leaves the cell and `ActivePlaylist` empty rather than naming a playlist that
  is not there. It is not persisted because the queue is not either. *When* one was last played is,
  and so is how often: the same call writes `playlists.played` and steps `playlists.plays`, which is
  what `PlaylistOrder::Played` and `PlaylistOrder::Plays` order on and what the bus answers
  `LastPlayDate` from. Loading one counts, so loading the same playlist twice counts twice.
  `Library::playlists_revision` is the companion cell, bumped by every playlist write, which is what
  a 200 ms bus poll watches so a listing is re-read only when an edit moved it. What the stamp costs
  is a poll: the engine publishes it only once it has applied the load, so the badge arrives a poll
  behind the queue. It is a 64-bit hash of the rows in their loaded order, so two queues that
  collide are one queue to it and a queue edited back to what it was is the playlist again.

## Undo

- **An edit to a playlist is one step to walk back, and a step is the playlist as it stood.**
  `undo.rs` is the whole of it. `undo::edited` wraps every mutator's transaction: it reads the
  playlist's name, kept order, saved query and rows before the change and keeps that restore point
  only where the change moved something, which is what `Change::Made` and `Change::Nothing` say — an
  edit that wrote nothing leaves nothing to put back. `undo::started` is the other half, for the
  gestures that create a playlist, whose restore point is that it did not exist. `Library::undo`
  takes the newest step and writes it back whole — one `DELETE` of the playlist row, which cascades
  its entries and its query away, then a dense rewrite — so a discarded playlist comes back under
  the id it had, while `played` and `plays` are read off the row rather than restored, because a
  play is not an edit. It is why `Library::start_playlist` and `Library::revise_query` are single
  calls: the window's gesture is name-and-rows and name-and-search, and two library calls would be
  two steps. The stack is `Inner::steps`, bounded at 32 steps and 50 000 rows and living only as
  long as the run, so the command line has no undo. The newest step is always kept, so a long run of
  edits drops the oldest rather than the largest. A play counted between an edit and its undo is not
  taken back with it, because `played` and `plays` are read off the row rather than restored — but
  the date the playlist was last changed is, so an edit walked back does not leave it climbing a
  *Last changed* listing.
- **A step holds the rows only where its edit could have moved one.** `Edit::moves_rows` is where
  that is said, and `Renamed` and `Revised` are the two it answers `false` for, so neither reads the
  paths under a playlist nor counts them against the stack's 50 000. What such a step is put back by
  is `written_over`, an `UPDATE` of the playlist row and a rewrite of its query where it has one,
  rather than the `DELETE` that would cascade the entries away — which is why the rows-holding path
  is the one that has to read `played` and `plays` back off the row it is about to delete. Both
  halves of `undo::walk` read the shape from the step's own `Edit`, so the inverse of a rename is a
  rename and holds no more than the rename did.
- **A step is walked either way, and walking it is what writes the step back the other way.**
  `undo::walk` is `Library::undo` and `Library::redo` alike: it pops a step off one stack, reads the
  standing it is about to overwrite, applies the step and pushes what it read onto the other — so
  `Inner::walked` holds the playlist as each edit *left* it, no step carries an inverse of its own,
  and a redone step is a step to walk back again. A step that finds no playlist under its id is a
  `Standing::Fresh`, which is how a discard and a create are the same shape read from opposite ends;
  walking one back that discards the playlist in play clears `playing_playlist` with it. What ends a
  redo is the next edit: `undo::note` clears `walked` before it keeps a step, so an edit made after
  a walk back forgets what was walked, while one that wrote nothing clears nothing because it never
  reaches `note`. That is also what keeps the id safe. Undo is strictly last-first, so every gesture
  that frees an id leaves a step under every gesture that takes one; every gesture that takes an id
  is an edit, so a step on `walked` can never name a playlist SQLite has since handed the id to.
  `RootView::undo_edit` and `RootView::redo_edit` are the playlists headings' *Undo* and *Redo*, and
  `ctrl-z`, `ctrl-shift-z` and `ctrl-y` away from the search field, where the field's own three are
  bound inside it. `Inner::walked` is bounded like `steps` — 32 steps or 50 000 rows — but it is a
  second bound rather than a shared one, so a long run of undos over long playlists can hold both
  ends of each of them, and nothing collapses a step walked back and forth again into the one it
  came from: a press either way costs the read and the rewrite the edit did.

## Sheets

- **A playlist leaves and arrives as M3U, PLS or XSPF, and the sheet names files rather than
  tracks.** `sheet.rs` is the seam and `Library::import_playlist` / `export_playlist` the way in: it
  reads the bytes, settles the encoding, decides which of the three the *text* is and hands
  `m3u.rs`, `pls.rs` or `xspf.rs` the job. What all three share lives there rather than three times
  over — a row's seconds, artist and title as a `Described`, the relative-or-absolute path rule,
  percent escaping both ways, and the staged rename over the target that stops a crash mid-write
  truncating a sheet. Writing picks the format from the target's extension and falls back to M3U
  where the name declares nothing; reading picks it from the content, so a sheet under the wrong
  extension still reads. Everything a sheet says about a *track* is read past — a `playlist_entries`
  row is a path, so `#EXTINF:`, `TitleN`/`LengthN` and `<title>`/`<creator>`/`<duration>` are
  written and never believed. A row naming a scheme that is not `file://` is counted rather than
  refused, because one stream in a sheet must not cost the other fifty rows. The scheme and a
  `localhost` authority are read whatever their case, as RFC 3986 has them, so `FILE:///a.wav` and
  `file://LocalHost/a.wav` are local rows and an `xml:base` written either way still resolves what
  sits under it; `file:/a.wav`, the form with no authority at all, reads as `file:///a.wav` does,
  while `file:track.flac`, with no slash, stays a relative path. A row that is empty once trimmed
  — a PLS `File1=` with nothing after it — names nothing and is counted as elsewhere, where it
  used to resolve to the sheet's own folder and be stored as a row. A sheet that is not
  UTF-8 is read as Windows-1252, unless its name declares UTF-8 — `.m3u8` and `.xspf` do — or its
  bytes hold a NUL, which no text sheet does. Importing reconciles by count rather than by set: a
  row the playlist already holds is counted as already there, so the same sheet read twice is a
  no-op, while a file the *sheet itself* names twice is two rows, because `add_to_playlist` lets the
  same file be put in twice deliberately. A name already taken is appended to rather than refused,
  which is what makes `import` and `resonate playlist <NAME> --add` the same gesture. A `.cue`
  handed to `--add` is added as the rows it cuts, through the same `sheet_cuts` `resonate play`
  and `resonate queue` read one through, rather than as a row naming the sheet. A row is
  written as a path only where `sheet::as_a_row` finds that path reads back as itself, and as an
  escaped `file://` URI where it does not, which is what carries a `#`, a line break or a scheme of
  its own through M3U and PLS. `sheet::canonical` settles a path the filesystem cannot answer for
  lexically — absolute, its `.` and `..` taken out — so a sheet imported while a mount is down still
  names what a later scan will store. A row holding a backslash and no forward slash is a path a
  Windows player wrote, and `forward_separated` reads it with its separators turned, so
  `..\Music\01.mp3` resolves beside the sheet rather than as one file of that literal name. The
  reverse is held to it too: `reads_back_as_itself` refuses a row `forward_separated` would turn,
  so a file of ours named `AC\DC.wav` is written as an escaped `file://` URI rather than as a row
  that would read back as `AC/DC.wav`. What is escaped is not ambiguous that way: a literal
  backslash cannot stand in a `file://` URI or an XSPF location, whose writers escape one as
  `%5C`, so `sheet::forward_escaped` turns every literal one into a separator *before* the text
  is unescaped — `file:///music\a.wav` and an XSPF `album\a.wav` or `xml:base="discs\"` name
  folders — while `AC%5CDC.wav` still reads back as the one file it names. The 8 MiB ceiling is weighed against what the name declares
  and again against what the read took, so a FIFO reporting zero is refused rather than read
  unbounded. `Library::prune_playlist` is the companion that drops the rows whose files have gone,
  and it is asked for rather than automatic. A sheet that says how many rows it holds is taken at
  its word and then weighed — PLS's `NumberOfEntries` against what the text held, the shortfall
  carried as `Imported::short` — so a truncated sheet says so rather than importing quietly short.
  M3U and XSPF declare no count at all, so only a PLS sheet can say it lost rows, and a sheet
  holding more than it promised is taken whole and says nothing. Import resolves and stats every
  row, so a sheet naming a network mount that is down reports each of its rows as missing rather
  than as waiting, while a symlink stays unresolved because nothing but the filesystem knows where
  it points; nothing tells a file that grew between the two reads from one that lied about its size.
- **`xspf.rs` reads its own markup, and every leniency in it is deliberate.** A tag ends at the
  first `>` *outside* a quoted attribute value, so an attribute holding one does not cut the tag in
  half. `xml:base` is resolved down an element stack, so a relative `<location>` answers to the base
  in force rather than always to the sheet's folder, and a base naming a scheme that is not
  `file://` makes every relative row under it count as elsewhere rather than resolve to nonsense. A
  track's `<location>`s are alternates, so the first that names a local file is the row and a track
  offering none is what costs one `elsewhere`. `<!DOCTYPE …>` and `<?…?>` are skipped rather than
  pushed, because an element pushed and never popped would carry its base to the rest of the sheet.
  `<album>`, `<image>`, `<annotation>` and `<meta>` stay unread on purpose: a row is a path, so a
  sheet is never a source of tags. What it takes on trust is the rest: an element is matched by its
  local name, so two namespaces both calling something `track` are one element to it; it knows the
  five entities XML defines and a numeric reference and nothing else; and it trusts the nesting, so
  a sheet that never closes an element it opened carries that element's base to everything after it.

## The MPRIS seam

- **`resonate-mpris` reaches playlists through a seam, never the library.** `Playlists` is the trait
  `Mpris::start` takes beside `Host`, and the binary fills it with a `Collection` over the `Library`
  and the `Player`. The interface is served only when one is supplied, so a run with no catalog
  advertises no `org.mpris.MediaPlayer2.Playlists` at all rather than an empty one. `Orderings` is
  built from `mpris::PlaylistOrder::ALL` — Alphabetical, CreationDate, ModifiedDate and
  LastPlayDate, the spec's four, distinct from the library's five — so an ordering the player never
  advertised is refused rather than quietly answered under another, `UserDefined` among them,
  because a playlist's own order is not one the bus can ask for by name. `PlaylistCount`,
  `ActivePlaylist` and `PlaylistChanged` are diffed from the same 200 ms poll the player properties
  use, against a listing re-read only when `Library::playlists_revision` moves. `unheard_of` is the
  question the diff asks of every row — a playlist the listing held under another name, or did not
  hold at all — so `PlaylistChanged` announces one that arrived as well as one renamed, which is
  what a sample holding an addition and a removal at once has to say: the count is the same and
  correctly stays, and the spec names no signal for a playlist that left, so a client learns of that
  one by re-reading `GetPlaylists` on the arrival it was told about. Neither of the seam's two reads
  is `Library::playlists`, because that is a grouped pass over every row of every playlist and one
  `count(*)` per saved query and the bus keeps only an id and a name of each row: `Playlists::count`
  is `Library::playlist_count`, one `count(*)` over `playlists`, and `Playlists::listing` is
  `Library::playlist_names`, which selects the id and the name with no join under it and takes the
  `index` and `maxCount` of the `GetPlaylists` call as `OFFSET` and `LIMIT`, so a client asking for
  ten reads ten. `PlaylistCount` is a property every client reads and every announced change
  re-reads, which is why it is the one that had to stop measuring a listing. Both query SQLite on
  the bus thread, so a client that asks for a listing pays for it there. What the bus cannot ask for
  is a narrowing, so the seam takes none.