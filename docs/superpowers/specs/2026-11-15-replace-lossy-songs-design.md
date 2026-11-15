# Replace a playlist's lossy songs with lossless copies

## Intent

A listener has playlists full of lossy copies (Opus from YouTube Music, MP3s) and wants them
upgraded to FLAC without doing it one song at a time. *Find a higher quality version* already does
this per track, but it only chooses a version for the queue. The playlist and the library keep the
lossy file. This feature works on a whole playlist, and it **replaces** each song: a lossless copy
is obtained, everything the lossy copy carried moves over to it, and the lossy file goes to the
trash.

## Goals

- One action on a playlist, *Replace lossy songs…*, opens a sheet listing every lossy song in it.
- Each row has a tick box, and the sheet has *Tick all*. *Replace* acts on the ticked rows.
- For each ticked song, a lossless copy is obtained:
  - a lossless copy **already held** is used at once, with no download;
  - otherwise a **want** is placed on the song's MusicBrainz release row, and the poll fetches it
    through the providers already set up.
- Once a lossless copy is held, the lossy copy is **folded into it**, like *Identical songs*
  (`fold_copies`): plays, last played, favourite, listens, playlist entries in every playlist,
  `kept_songs`, resume rows and `songs_received` move to the lossless track, and the lossy file
  goes to the trash (`Why::Folded`).
- The promise survives the app closing mid-download. A pending replacement is stored and settled
  whenever a lossless copy turns up: from a poll, from the inbox, from a scan of a copy put in by
  hand, or one already held.

## What counts as better

**Any lossless copy beats any lossy one**, whatever bit depth and rate each one reports. This is
`TheSong::is_bettered_by`'s rule (`!self.lossless && heard.lossless`), not `Quality::is_above`.
`is_above` can't be used here: Opus is billed at 48 kHz (`opus::rate_of`), and a lossy codec
declares no depth, so it reads as `F32` and counts as 24 bits (`container::undeclared_format`). An
Opus therefore reads (lossy, 24, 48000), and a 16-bit/44.1 kHz FLAC would never be `is_above` it.
Among several lossless candidates, the best goes first (`Quality::best_first`).

## Non-goals

- Lossy to better lossy (MP3 128 kb/s to 320 kb/s). Only a lossless copy replaces anything, and
  the quality vocabulary has no bitrate to compare two lossy copies by.
- Cue-sheet cuts, on either side. A lossy track that is a cut of a larger file isn't listed, and a
  lossless cut doesn't count as a held copy. `remove_tracks` and `handed_over` work by whole path,
  and identical-copy folding admits only whole files for the same reason (`copies::whole_files`).
  *(My call.)*
- Upgrading one lossless copy to another (16/44.1 to 24/96).
- **Replacing anything the listener didn't tick.** An ordinary want (an album, a missing song)
  still skips songs already held, lossy or not, and nothing upgrades the library in the
  background. Only a song ticked in the sheet is ever replaced. *(The listener chose this over
  upgrading the held lossy songs of every want, and over a library-wide pass.)*
- Songs not matched to a MusicBrainz release row. They are listed but greyed out, and can't be
  ticked unless a lossless copy is already held. Matching them is left to enrichment and *Match*.
- The phones, the terminal and MCP. The stored replacement makes a `resonate playlist --replace…`
  or a phone sheet possible later; neither is built now. *(My call: the desktop window is where
  this was asked for.)*
- Rewriting playlist sheets on disk beyond what `handed_over` already does to `playlist_entries`.

## Anti-goals (a failure even if it works)

- **A lossy file is trashed with no lossless copy held.** A want that gives up, is cancelled or is
  refused leaves the lossy copy exactly as it was.
- **A replacement want fetches another lossy file.** The YouTube Music last resort and any provider
  offering lossy media must never fill it, and a lossy delivery must never be kept for it.
- **A different song replaces the original.** A wrong match loses a listener's history to the
  wrong track.
- **Play history or favourites are lost** in the hand-over.

## How it works today

- `resonate-library/src/versions.rs`: `Quality { lossless, bits, rate }`, where `is_above` means
  higher on at least one and lower on none. `TheSong::names` matches a song by bare title, artist
  and a length within 3 s. `better_held` lists the held copies above a track's quality.
- `resonate-ui/src/views/better.rs`: the per-track *Find a higher quality version*, which looks
  through held copies and every service that plays in place, then chooses a version
  (`Library::choose_version`, the `versions_chosen` table). The queue plays the chosen version
  and the playlist is untouched.
- `resonate-library/src/copies.rs::handed_over(transaction, &IdenticalCopy)` moves everything from
  one track to another, keyed by the paths and ids an `IdenticalCopy` holds. `Library::fold_copies`
  (`db/roots.rs`) runs it, then `remove_tracks(.., Why::Folded)`, which trashes the files and their
  sidecars, prunes emptied folders and forgets the rows (vault objects included).
- Wants (`db/wants.rs`, `elsewhere::want_in`) are keyed by `release_track_id`. The poll reads
  `WANTS_UNHELD` (`db/statements.rs`), which keeps only rows with `rt.track_id IS NULL`, so **a held
  song is never fetched today**.
- A delivery lands in `supply.rs::landed`. A delivery shorter than the want is refused as
  `Landed::NotTheSong`, noted in `refused_offers`, and the next offer is taken. A vault delivery
  pairs to its release row only `WHERE ... track_id IS NULL` (`db/vault.rs`), and a file delivery
  pairs through `pair_what_was_filed`, so **a lossless delivery for a row the lossy copy holds will
  not pair to that row by itself**.
- The playlist menu is `playlist_menu` in `resonate-ui/src/views/playlists.rs`, and sheets are the
  `Sheet` enum in `views/sheets.rs`.

## Approaches

1. **A stored replacement, settled whenever a lossless copy turns up (chosen).** A
   `replacements` table names the lossy track and the release row it is replaced on. The wants
   query lets held rows through only where a replacement names them, and lossless offers alone are
   kept for them. One settle step, run after a poll's deliveries pair and after each scan, folds a
   lossy track into a held lossless copy of the same song. It survives a restart and covers every
   way a lossless copy can arrive. It costs a migration step, a change to the wants query and the
   landing check, a library function and the sheet.
2. **The window does it alone.** The sheet places wants and watches downloads, with no table.
   Closing the app mid-download leaves both copies held, and the wants query still has to change,
   so the saving is small.
3. **Swap inside the download thread at the moment of landing.** Precise, but it misses lossless
   copies that arrive any other way (already held, inbox, copied in), and it puts trashing on the
   poll's thread.

The listener chose 1, the only one that keeps the promise across a restart with one settle path.
For the lossy copy, they chose **A**: hand everything over and trash it, rather than keeping it as
a second copy (B) or adding a per-sheet "keep the lossy copies" tick (C).

## Design

### Stored: `replacements`

A step appended to `MIGRATIONS` in `resonate-library/src/schema.rs` (`V1` and the steps already
there are never edited), named in the house style, e.g. `A_LOSSY_SONG_WAITS_TO_BE_REPLACED`:

```sql
CREATE TABLE replacements (
    track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    release_track_id INTEGER REFERENCES release_tracks(id) ON DELETE SET NULL,
    asked INTEGER NOT NULL
);
CREATE INDEX replacements_by_release_track ON replacements(release_track_id);
```

- `track_id` is the lossy track to replace. If it is deleted or forgotten some other way, the
  replacement goes with it.
- `release_track_id` is the row whose want fetches the lossless copy. It is `NULL` when the
  replacement was settled straight from a held copy and never needed a want.
- The fingerprint and `STAMPS` follow the usual migration rules.

### Library API (`resonate-library`)

- `Library::lossy_songs_of(playlist) -> Result<Vec<Replaceable>>`: every entry of the playlist
  whose track is not lossless and is a whole file, in playlist order and once per track. For each,
  it gives:
  - the track;
  - the best **held** lossless whole-file copy of the same song (`TheSong::is_bettered_by`,
    `best_first` among several, or the version already chosen for it where that copy is held and
    lossless);
  - its release row (`release_tracks.track_id = track.id`), if matched;
  - whether a replacement is already pending.

  The sheet's state comes from this alone.
- `Library::replace(tracks: &[TrackId]) -> Result<Replacing>`: for each track, in one write:
  - a held lossless copy: fold at once (below);
  - else a release row: insert the replacement and want the row. `want_in` alone isn't enough: its
    insert is `ON CONFLICT DO NOTHING`, and it resets `tried`/`misses` only where `offered IS
    NULL`. A lossy song that arrived through a want (YouTube Music answering as last resort)
    already has a want whose `offered` names the lossy file. So `replace` re-arms an existing want
    for the row: `offered`, `tried` and `misses` cleared, and its `refused_offers` kept;
  - else: skip it and count it as unmatched.

  The sheet already showed the listener the held copy, so the immediate fold accepts the
  `TheSong::names` match the sheet showed. It returns counts (folded now, wanted, unmatched) for
  the toast, and nudges the poll the way `want_release_tracks` does.
- `Library::settle_replacements() -> Result<Deleted>`: for each pending replacement, look for a
  held lossless whole-file copy, by evidence only. Nobody is watching, so a name match isn't
  enough. The copy must be either:
  - **the track the replacement's want delivered**, which is `wants.offered` once the poll keeps a
    lossless delivery (re-armed by `replace`, so it can't name the lossy file); or
  - **a track holding the same recording MBID** as the lossy track or its release row.

  When one is found, fold it. A replacement with no evidence yet stays pending. *(My call: a
  `TheSong::names` match is loose (a track with no artist matches any artist, a missing length
  passes, and one artist name containing another counts), and an unattended fold on it risks the
  "different song" anti-goal.)*
- **The fold** generalises `copies::handed_over` to take the kept and taken tracks' ids and
  paths. Its playlist, `kept_songs`, `resume_rows` and `songs_received` updates are keyed by path,
  and since both sides are whole files (above), resetting `span_start`/`span_frames` stays right.
  The identical-copy fold and the replacement fold share one hand-over. On top of what it moves
  today, a replacement fold also:
  - hands the release row to the kept track (`UPDATE release_tracks SET track_id = kept WHERE
    track_id = taken`), **only when the kept track holds no release row of its own**. A FLAC held
    on another album keeps its own pairing, and the lossy copy's row is left unheld on its album.
    *(My call: pairing a track to a second album it isn't filed under would fight
    `rematch_release_tracks`.)*;
  - forgets a `versions_chosen` row on the taken track, since it no longer exists;
  - deletes the replacement and its want.

  Then `remove_tracks(.., Why::Folded)` trashes the file.

### The poll (`supply.rs`, `db/statements.rs`)

- `WANTS_UNHELD`'s `WHERE rt.track_id IS NULL` becomes `WHERE rt.track_id IS NULL OR EXISTS
  (SELECT 1 FROM replacements r WHERE r.release_track_id = rt.id)`.
- `Want` carries a **replacement flag**, read in the same query. Today every reader treats a held
  want (`want.held`, i.e. `rt.track_id` set) as done: `supply::due`, `Want::due_at`, `gave_up`,
  `last_tried`, `is_a_want_due`, `next_want_due` and the window's shelves. Each one must treat a
  held replacement want as still open. Otherwise the poll reads the want and never claims it.
- **For a replacement want, a delivery is kept only if it is lossless**, checked in `landed`
  beside `lasts_as_long_as`. A lossy delivery is `Landed::NotTheSong` and goes to
  `refused_offers`, so the next offer is taken.
- **Last-resort providers are not asked for a replacement want** (`is_a_last_resort`, YouTube
  Music), so no lossy download happens at all.
- A replacement want's delivery lands as a **new track**, and the fold hands the row over later:
  - **Music folder:** this already happens. It lands as a new rooted track, and pairing skips the
    held row.
  - **Vault:** `note_delivered` (`db/vault.rs`) calls `still_unheld` first, and on a held row it
    writes no `vault_objects` or `tracks` row, so the poll reports `Landed::Unkept`. For a
    replacement want it must instead note the object and its track as an unpaired new track
    (the `WHERE track_id IS NULL` pairing still skips the row).
  - **Either way**, the kept delivery's location goes into `wants.offered`, as for any want, and
    that is the evidence `settle_replacements` reads.
- `settle_replacements` runs **once the poll's lanes have finished**, from the poll's own run
  (window or headless `resonate poll`), never inside `landed` on a lane. *(Approach 3 was turned
  down mainly because it misses copies arriving any other way. Trashing at the end of a poll is
  the same step `fold_copies` takes from the window.)*

### Scan

`alternatives::settle_if_owed` runs inside the scan while it holds the `Walk` guard (`scan.rs`),
and `remove_tracks` takes `walk_the_tree`, so settling there would fail with
`Error::AlreadyWalking`. Instead, `settle_replacements` runs **after the scan has released its
guard**, wherever the scan's caller learns it has finished. That covers a lossless copy dropped in
the inbox or copied in by hand that carries the recording MBID once enriched.

**Retries:** a settle refused because the tree is being walked (a scan, organise or retag
running) changes nothing and is logged at debug. Replacements stay pending and are tried again at
the next poll's end or scan's end, so nothing has to own a retry.

### The sheet (`resonate-ui`)

A *Replace lossy songs…* entry in `playlist_menu` opens it, shown only when the playlist holds at
least one lossy song. It is a new `Sheet` variant. Its words go in a new `replacing.ftl` area,
counts as Fluent selectors.

```
Replace lossy songs · Road trip                    [✓] Tick all
 [✓] Song A — Artist   Opus 160k   →  wanted through MusicBrainz
 [✓] Song B — Artist   MP3 320k    →  you hold a FLAC
 [ ] Song C — Artist   Opus        →  not matched on MusicBrainz       (greyed)
 [ ] Song D — Artist   Opus        →  already being replaced           (greyed)
                                            [Cancel]  [Replace 2]
```

- Every replaceable row starts ticked. *(My call: the listener asked for this to replace the lot,
  and *Tick all* stays as the way back after unticking.)*
- *Tick all* toggles every row that can be ticked.
- *Replace* calls `Library::replace` off the main thread, closes the sheet and toasts what
  happened: so many replaced now, so many wanted, so many not matched.
- A song not matched on MusicBrainz and with no held lossless copy can't be ticked. A song already
  pending shows as such and can't be ticked.
- The sheet notes once, in plain words, that the lossy copies go to the trash when their lossless
  copy arrives. *(My call: the trash is reversible, so this is a note, not a confirmation dialog.)*

### Seeing and undoing a pending replacement

- A pending replacement's want shows in the Wants pane like any want, marked as replacing a song.
  Dismissing that want there (`unwant`) cancels the replacement, and the lossy copy stays.
  *(My call: one place to see what is being fetched.)*
- A replacement whose want keeps failing just stays pending. Nothing is trashed until a lossless
  copy is held.

## Constraints

- Every rule in `.claude/rules/` holds: no comments, `thiserror` errors with typed variants,
  every word through the crate's catalog (`language.md`), and a schema change as an appended
  migration step (`library.md`, the `V1` rule in `CLAUDE.md`).
- The poll keeps its pacing, the metered gate and cancellation. A replacement want is a want like
  any other in every other respect.
- `remove_tracks` already takes the tree walk lock, so `settle_replacements` must not hold a write
  transaction across it, as `fold_copies` does not.
- Tests through `cargo nextest run`. The library's store tests cover migration, query, fold and
  refusal. The poll's loopback providers (`supply.rs`'s tests) cover a lossy offer refused and a
  lossless one kept and folded.

## Left to the builder

- Where `Replaceable` and `Replacing` live and their exact fields.
- Whether the landed path is read from the want row or from `note_landed_for_a_want`'s record,
  whichever the poll already writes for a non-last-resort provider.
- A lossy track held in the vault: check that the generalised hand-over and `remove_tracks` treat
  its location and vault object correctly (identical-copy folding skips vault tracks today).
- The rules files: `library.md` gains a *Replacing a lossy song* section, `providers.md` a line on
  replacement wants refusing lossy offers and passing over the last resort, and `ui.md` a line for
  the sheet.
