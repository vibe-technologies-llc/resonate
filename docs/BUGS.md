# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Device sync

## Android

## Catalog
- [S · 0.1.1] A playlist entry keeps the length a cue track had when it was added:
  `follow_a_cut_that_moved` and `CUT_FOLLOWERS` rewrite `playlist_entries.span_start` when a sheet
  moves a track but nothing rewrites `span_frames`, the read-back joins `tracks` on path and
  `span_start` alone (`ON_THE_SAME_CUT`) and hands the row's own cut to the queue, so after the
  sheet is edited the pane shows the catalog's length while playback loads the old span, and the
  stale row no longer equals `Cut::of` the fresh track in folding and import. Reproduced in
  `tests/library.rs` (catalog 17640 frames against the playlist's 11760)
- [M · 0.1.1] The phones' next listen can be lost after the newest one was deleted: the mobile
  catalog's `listens` has `id INTEGER PRIMARY KEY` without `AUTOINCREMENT` (the desktop's does),
  `forgotten_in` deletes a deleted song's listens, SQLite hands the next insert `max(rowid) + 1`,
  which is the deleted id, and both the ledger (`listens_mark`) and scrobbling
  (`submissions.through`) read `id > mark`, so that listen is never sent to the other devices nor
  scrobbled. `V1` is the baseline of 0.1.0, so the fix is a `MIGRATIONS` step rebuilding the table
- [S · 0.1.1] A scan that did not walk a root forgets its files when a folder is readable but not
  searchable: `tidy_the_roots_beside` and `forget_at_or_under` test `Path::exists()`, which is
  false on `EACCES` as on `NotFound`, and `sizes_of_files_not_catalogued` lists the folder fine,
  so every catalogued file in it counts as gone and `forget_paths` deletes the rows with their
  plays, favourites and playlist links; the walked-root path keeps what it cannot read. Checked
  with `test -e` on a `chmod 400` folder, not through a scan

## Tagging
- [S · 0.1.1] An in-room ID3 rewrite can cut the last frame short: `id3_content` trims every
  trailing zero byte, the frame's own as well as the padding, and `id3_absorbed` accepts a head
  under `Room::Held` when that trimmed content fits the standing length, so a tag with no slack
  whose last frame ends in zeros (a UTF-16 text ending in an ASCII letter, the `00 00 00 00`
  counter of the POPM written for a favourite) that grows by no more than those zeros is written
  `written.len() - standing` bytes short while the frame header still declares the whole size. The
  audio is not touched and the read-back reports it unconfirmed, but the file was already changed.
  Reproduced on `Head::rebuilt` with one byte of growth
- [S · 0.1.1] A crashed tag write to a file whose name is not UTF-8 is never mended or swept:
  `stamped_with` (`journal.rs`) and `staged_by` (`writing.rs`) start with `to_str()?`, so
  `kept_by`, `names_a_cut_short_write` and `sweep_what_a_dead_writer_staged` ignore the journal,
  whole copy or torn copy of such a track, which stays half written and its staged copies pile
  up, though `KeptBy::names` and `named_within` already compare bytes. Shown on
  `names_a_cut_short_write`

## Playback and output
- [S · 0.1.1] With `dsd-like-pcm` on, a volume, ReplayGain or levelling change on a decimated DSD
  track loses the +6 dB raise from the gain stage: `plan_for` raises the plan's gain
  (`raised_like_pcm`) and the chain is built from it, but `retune`'s same-shape arm calls
  `chain.set_gain` with the track's unraised `replay_gain`, so the ring's trim after the
  true-peak guard and dither supplies the missing factor of two (the heard level is right, a
  peak the guard would have ridden can clip, and the ring stays on its per-frame path). Reproduced
  on the chain against a fresh plan (0.216 against 0.432); not driven through the engine
- [S · 0.1.1] An insert asked past the end lands before the queued rows while a play-next row is
  playing: `Placement::At(row)` clamps to `InOrder(order.len())` and `land_in_order` reads
  `at <= after` on the `Next` seat as inside the passed rows, so `[A]` with `Q1 Q2` queued and `Q1`
  heard becomes `[A, X, Q1, Q2]` for `At(10)` instead of `[A, Q1, Q2, X]`. Needs a stale row (a
  snapshot the queue shrank under, the UI's `put_the_queue_back`); `At(row <= len)` is correct.
  Reproduced in the queue's tests

## Formats
- [S · 0.1.1] A UTF-8 BOM followed by any non-UTF-8 byte keeps the BOM in the text: `detected`
  strips it to test validity and returns `Legacy`, `decoded_as`'s `Legacy` arm decodes the whole
  slice without stripping it, and the first line reads `ï»¿FILE ...`, so a cue sheet with one
  Latin-1 letter loses every track (`read_cue` finds 0 files, 1 without the BOM). Every
  `text::decoded` reader has the first line wrong the same way (LRC, playlist sheets,
  EqualizerAPO). Reproduced on `read_cue`
- [M · blocked: symphonia] A file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader
- [M · blocked: opus-rs] A stereo Opus stream's mono packets (libopus drops to mono near 16-24 kbps)
  go to a second `OpusDecoder` (`opus.rs` `other`) whose state is minutes stale or empty, and the
  main decoder misses them: a click, and band energies mispredicted for some frames, at every
  mono/stereo switch of a low-bitrate podcast; `opus-rs` 0.1.37 refuses a packet whose TOC channel
  bit differs from the decoder's (`Channel count mismatch`) and keeps no stream-channel state a
  mono packet could be decoded against, so the fix is a decoder that switches (upstream or vendored)

## Binary and desktop
- [S · 0.1.1] `file:/music/a.flac` (the single-slash form of RFC 8089, which Java's `File.toURI`
  writes) is not a local file: `MediaLocation::from_uri` takes only the `file://` prefix, so the
  URI falls to the source branch as a key on a source named `file`, and `resonate play`, `queue`
  and `OpenUri` fall back to the literal relative path and fail as a missing file. Run on
  `from_uri`
- [S · 0.1.1] MPRIS `xesam:contentCreated` can carry a date that does not exist: `iso_8601` checks
  the day against 1 to 31 for every month, so a `DATE` of `2021-02-30` is sent as
  `2021-02-30T00:00:00Z` where `mpris.md` says an out-of-range day falls to the first, and a strict
  client drops the date

## Providers and network
- [S · 0.1.1] A `Retry-After` in the asctime form with a huge or negative year panics on integer
  overflow in dev and test builds: the `[_, month, day, clock, year]` arm of `waited.rs` takes any
  `i64`, and `CivilDate::day_since_the_epoch` multiplies unchecked; a release build wraps and the
  callers clamp the wait, so only the thread of a dev build dies. Reproduced

## iOS
- [S · 0.1.1] A download whose title holds `? : * " < > |` cannot be created in a folder on exFAT
  or SMB: `file_stem_of` (`downloads.rs`) replaces control letters, `/` and `\` alone while the sync
  landing strips all nine (`UNFILEABLE`), so `Folders.fresh` fails on `open` with a refusal that
  only says the song could not be created. Read, not run
