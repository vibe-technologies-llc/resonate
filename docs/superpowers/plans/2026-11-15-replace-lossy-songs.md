# Replace a Playlist's Lossy Songs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A *Replace lossy songs…* sheet on a playlist that replaces each ticked Opus/MP3 with a lossless copy (held now, or wanted through MusicBrainz), handing its history over and trashing the lossy file once the lossless copy is held.

**Architecture:** A `replacements` table (one migration step) names each lossy track waiting to be replaced and the release row its want fetches on. The poll lets those held rows through, skips last-resort providers for them and refuses lossy deliveries. One fold (the identical-copy hand-over, generalised) runs at once for a held copy, and from `settle_replacements` after a poll or scan for a copy that arrives later, on evidence only.

**Tech Stack:** Rust, rusqlite (SQLite), GPUI, Fluent catalogs through `resonate-words-build`, `cargo nextest`.

**Spec:** `docs/superpowers/specs/2026-11-15-replace-lossy-songs-design.md`

## Global Constraints

- No comments anywhere (`no_source_in_the_workspace_carries_a_comment`); name values and functions instead.
- Errors are `thiserror` variants of the crate's `Error`; no stringly descriptions; `size_of::<Error>() <= 128`.
- Every word a listener reads comes from a `.ftl` catalog; counts are Fluent selectors; no comment in `.ftl`.
- Schema: append one step to `MIGRATIONS` in `crates/resonate-library/src/schema.rs`; never edit `V1` or an existing step; tables are `STRICT`.
- Any lossless copy beats any lossy one (`!lossy.lossless && copy.lossless`); never `Quality::is_above` for this.
- Cue-sheet cuts are excluded on both sides: a track with `span.is_some()` is neither replaced nor a replacement.
- A lossy file is never trashed unless a lossless whole-file copy is held; a replacement want is never filled by a lossy file nor asked of a last resort.
- An unattended fold (`settle_replacements`) needs evidence: the want's delivered track, or a shared recording MBID. Never a name match.
- `remove_tracks` takes the `Walk` guard; never hold a write transaction across it.
- Format with `rust-formatter` (check with `rust-formatter --check`), never `cargo fmt`. Tests through `cargo nextest run`, never `cargo test`.
- Clippy clean: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo clippy --workspace --exclude resonate-ui --no-default-features --all-targets -- -D warnings`.
- Commits are Conventional Commits, lower case, no `Co-Authored-By` or other attribution trailer.
- Desktop window only: no phone, terminal or MCP surface.

## Review Focus

1. **A lossy song in several playlists, or twice in one.** Every entry in every playlist moves to the lossless copy and each playlist's `modified` moves; the sheet lists the song once. (Task 4.)
2. **Two lossy copies resolving to the same lossless copy.** Both fold into it, plays summed, both files trashed, no error on the second fold. (Task 4.)
3. **The release is landed again while a replacement waits.** `land_release` deletes and reinserts `release_tracks`, so `replacements.release_track_id` goes `NULL`; the replacement must rejoin the row its track now holds, or the want reads as done and the FLAC is never fetched. (Task 5.)
4. **A settle while a scan or organise holds the walk.** Nothing is written or trashed, the replacement stays pending, and the next poll's or scan's end folds it. (Task 5.)
5. **A vault on either side.** A vault-held lossy track and a FLAC delivered into the vault (a rootless row named by its object's path) fold and trash like plain files: the lossy file goes to the trash, its object is forgotten, playlist entries name the FLAC row's path. (Tasks 4 and 6.)

---

### Task 1: The `replacements` table

**Files:**
- Modify: `crates/resonate-library/src/schema.rs` (new const after `THE_PATHS_A_REMOVAL_OR_A_MOVE_LOOKS_UP_ARE_INDEXED`, appended to `MIGRATIONS`, a test in its `tests` module)

**Interfaces:**
- Produces: table `replacements (track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE, release_track_id INTEGER REFERENCES release_tracks(id) ON DELETE SET NULL, asked INTEGER NOT NULL) STRICT`, index `replacements_by_release_track ON replacements(release_track_id)`.

- [ ] **Step 1: Write the failing test** in `schema.rs`'s `tests`, shaped like `a_catalog_laid_before_the_lookup_indexes_keeps_its_rows_and_gains_them`:

```rust
#[test]
fn a_catalog_from_before_lossy_songs_were_replaced_keeps_its_rows_and_gains_the_table() {
    let connection = opened();
    let before = MIGRATIONS
        .iter()
        .position(|step| *step == A_LOSSY_SONG_WAITS_TO_BE_REPLACED)
        .expect("the step that makes the table");
    connection.execute_batch(V1).expect("the first release's schema");
    for step in &MIGRATIONS[..before] {
        connection.execute_batch(step).expect("an earlier step");
    }
    connection
        .pragma_update(None, "user_version", as_signed(STAMPS[before].0))
        .expect("the earlier stamp");

    lay_out(&connection).expect("the catalog is carried forward");

    let waiting: i64 = connection
        .query_row("SELECT count(*) FROM replacements", [], |row| row.get(0))
        .expect("the table stands");
    assert_eq!(waiting, 0);
    assert!(
        plan_of(&connection, "SELECT track_id FROM replacements WHERE release_track_id = 1")
            .contains("replacements_by_release_track")
    );
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo nextest run -p resonate-library a_catalog_from_before_lossy_songs_were_replaced`
Expected: compile error, `A_LOSSY_SONG_WAITS_TO_BE_REPLACED` not found.

- [ ] **Step 3: Add `const A_LOSSY_SONG_WAITS_TO_BE_REPLACED: &str`** holding the table and index above, and append it as the last entry of `MIGRATIONS`.

- [ ] **Step 4: Run the schema tests**

Run: `cargo nextest run -p resonate-library schema::`
Expected: PASS, including `every_step_of_the_chain_stamps_a_catalog_differently`.

- [ ] **Step 5: Commit**

```bash
git add crates/resonate-library/src/schema.rs
git commit -m "feat(library): keep the lossy songs waiting to be replaced"
```

---

### Task 2: A want knows it replaces a song

**Files:**
- Modify: `crates/resonate-library/src/model.rs` (`Want`)
- Modify: `crates/resonate-library/src/db/statements.rs` (`wants_selected!`, `WANTS_UNHELD`)
- Modify: `crates/resonate-library/src/db/rows.rs` (`RawWant::read`, `into_want`)
- Modify: `crates/resonate-library/src/supply.rs` (`Want::gave_up`, `Want::due_at`, `last_tried`, `due`, the test `Want` literals)
- Modify: `crates/resonate-library/src/filed.rs` (test `Want` literals)
- Modify: `crates/resonate-ui/src/downloads.rs` (`WantStanding`)

**Interfaces:**
- Produces: `Want::replacing: Option<TrackId>` (the lossy track a pending replacement names on this want's release row); `pub const fn Want::is_answered(&self) -> bool` = `self.held.is_some() && (self.replacing.is_none() || self.offered.is_some())`.
- Produces: `WantStanding::replacing(self) -> bool`.

- [ ] **Step 1: Write the failing tests** in `supply.rs`'s `tests`, using its `want(tried)` helper:

```rust
#[test]
fn a_held_want_replacing_its_song_is_due_until_a_lossless_copy_is_offered() {
    let lossy = TrackId::new(7).expect("an id");
    let replacing = Want {
        held: Some(lossy),
        replacing: Some(lossy),
        ..want(None)
    };
    assert!(!replacing.is_answered());
    assert!(replacing.due_at().is_some());
    assert!(due(&replacing, PollOptions::default(), SystemTime::now()));

    let offered = Want {
        offered: Some("file:///music/a.flac".to_owned()),
        ..replacing.clone()
    };
    assert!(offered.is_answered());
    assert_eq!(offered.due_at(), None);
    assert!(!due(&offered, PollOptions::ASKING_EVERY_WANT, SystemTime::now()));
}

#[test]
fn a_held_want_replacing_nothing_is_answered_as_before() {
    let held = Want {
        held: Some(TrackId::new(7).expect("an id")),
        ..want(None)
    };
    assert!(held.is_answered());
    assert_eq!(held.due_at(), None);
    assert!(!held.gave_up());
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo nextest run -p resonate-library supply::tests::a_held_want`
Expected: compile error, no field `replacing`.

- [ ] **Step 3: Implement.**
  - `Want` gains `pub replacing: Option<TrackId>` after `held`; every `Want { .. }` literal in `supply.rs` and `filed.rs` tests gains `replacing: None`.
  - `wants_selected!` selects one more column after `rt.track_id`: `(SELECT r.track_id FROM replacements r WHERE r.release_track_id = rt.id)`; `RawWant` gains `replacing: Option<i64>` read at the next index and turned into a `TrackId` as `held` is.
  - `WANTS_UNHELD`'s `WHERE rt.track_id IS NULL` becomes `WHERE (rt.track_id IS NULL OR EXISTS (SELECT 1 FROM replacements r WHERE r.release_track_id = rt.id))`.
  - `gave_up`, `due_at`, `due` and `last_tried`'s filter read `is_answered()` where they read `held` today.
  - `WantStanding::of`: `delivered: want.is_answered() || want.offered.is_some()` and a new `replacing: want.replacing.is_some()` field with its getter.

- [ ] **Step 4: Run the library and window tests that read wants**

Run: `cargo nextest run -p resonate-library supply:: && cargo nextest run -p resonate-library library:: -E 'test(/want/)' && cargo nextest run -p resonate-ui downloads`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/resonate-library crates/resonate-ui/src/downloads.rs
git commit -m "feat(library): let a want replacing a held song stay open until a lossless copy is offered"
```

---

### Task 3: Read a playlist's lossy songs

**Files:**
- Create: `crates/resonate-library/src/replacing.rs` (the vocabulary)
- Create: `crates/resonate-library/src/db/replacing.rs` (`impl Library`)
- Modify: `crates/resonate-library/src/lib.rs` (`mod replacing;`, `pub use replacing::{Replaceable, Replacing};`)
- Modify: `crates/resonate-library/src/db/mod.rs` (`mod replacing;`)
- Test: `crates/resonate-library/tests/library.rs` (beside the identical-copies tests, reusing `Tree`, `on_disc`, `scan`, `options`, `ffmpeg`, `opus_in_webm`)

**Interfaces:**
- Produces:

```rust
#[derive(Clone, Debug)]
pub struct Replaceable {
    pub track: Track,
    pub held: Option<Track>,
    pub release_track: Option<ReleaseTrackId>,
    pub pending: bool,
}
impl Replaceable {
    pub const fn can_be_ticked(&self) -> bool
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replacing {
    pub folded: u64,
    pub wanted: u64,
    pub unmatched: u64,
    pub trashed: Deleted,
}
impl Library {
    pub fn lossy_songs_of(&self, playlist: PlaylistId) -> Result<Vec<Replaceable>>;
    pub fn playlists_with_lossy_songs(&self) -> Result<AHashSet<PlaylistId>>;
}
```

`can_be_ticked` = `!self.pending && (self.held.is_some() || self.release_track.is_some())`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_playlists_lossy_songs_are_listed_once_each_with_the_lossless_copy_held() -> Result<()>
```
Encode with ffmpeg (skip without it): `A.opus` (title *Echoes*, artist *Pink Floyd*), `A.flac` of the same tags and length in another folder, `B.mp3` (*Time*), and `C.flac` (*Money*). A playlist holds `A.opus`, `B.mp3`, `A.opus` again and `C.flac`. Assert `lossy_songs_of` answers two rows in playlist order, `track` titles `["Echoes", "Time"]`; the *Echoes* row's `held` is the `A.flac` row; the *Time* row's `held` is `None`, `release_track` `None`, `can_be_ticked()` false; `pending` false on both. Assert `playlists_with_lossy_songs()` holds the playlist, and a second playlist holding only `C.flac` is not in it.

```rust
#[test]
fn a_cut_of_a_sheet_is_neither_a_lossy_song_to_replace_nor_a_copy_to_replace_one_with() -> Result<()>
```
A lossy `whole.opus` cut by a `.cue` into two rows, and a FLAC cut by a sheet into a row named as a lossy whole-file song: `lossy_songs_of` lists no cut row and offers no cut as `held`.

```rust
#[test]
fn the_version_chosen_for_a_lossy_song_is_the_copy_offered_where_it_is_lossless() -> Result<()>
```
Two FLAC copies of the lossy song; `choose_version` picks the lower-quality one; `lossy_songs_of`'s `held` is the chosen one, not the best by `Quality::best_first`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p resonate-library library::a_playlists_lossy_songs library::a_cut_of_a_sheet_is_neither library::the_version_chosen_for_a_lossy`
Expected: compile error, no method `lossy_songs_of`.

- [ ] **Step 3: Implement `lossy_songs_of(playlist)`** in `db/replacing.rs`: read `playlist_entries(playlist, None)`, keep entries whose `track` is `Some`, `span.is_none()` and `!codec.is_lossless()`, first occurrence of each track id only. For each, candidates are `alternatives_of(track.id)`, the tracks sharing the lossy row's `tracks.mbid` where it has one, and `tracks(&TrackQuery { text: Some(TheSong::words(&title, artist)), limit: Some(HELD_WEIGHED_AT_MOST), ..Default::default() })` (move `HELD_WEIGHED_AT_MOST` or a library-side const of the same value; the window's is UI-side), kept where `span.is_none()`, `id != track.id` and either `TheSong::of(&track).is_bettered_by(SongHeard::of(copy))` or (same MBID and `copy.codec.is_lossless()`). `held` = the `version_chosen_for(track.id)` held copy where it is among those, else the first by `Quality::of(copy).best_first()`. `release_track` = `SELECT id FROM release_tracks WHERE track_id = ?1`. `pending` = a `replacements` row for the track.

- [ ] **Step 4: Implement `playlists_with_lossy_songs()`** as one query: distinct `playlist_entries.playlist_id` joined to `tracks` on the same cut (`ON_THE_SAME_CUT`) where `tracks.span_frames IS NULL` and not `lossless_codecs!()` (the macro is `#[macro_use]` from `db/ordered.rs`).

- [ ] **Step 5: Run the tests**

Run: same as Step 2.
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/resonate-library
git commit -m "feat(library): list a playlist's lossy songs beside the lossless copies held"
```

---

### Task 4: Replace now or want: `Library::replace` and the fold

**Files:**
- Modify: `crates/resonate-library/src/copies.rs` (generalise `handed_over`)
- Modify: `crates/resonate-library/src/db/roots.rs` (`fold_copies`, split `remove_tracks`)
- Modify: `crates/resonate-library/src/replacing.rs` (the fold's transaction steps)
- Modify: `crates/resonate-library/src/db/replacing.rs` (`replace`)
- Modify: `crates/resonate-library/src/elsewhere.rs` (re-arming a want)
- Modify: `crates/resonate-library/src/db/wants.rs` (`unwant` cancels a replacement)
- Modify: `.claude/rules/library.md` (the step's place in the migration list; a *Replacing a lossy song* section)
- Test: `crates/resonate-library/tests/library.rs`

**Interfaces:**
- Consumes: `Replaceable`, `Replacing` (Task 3); the `replacements` table (Task 1).
- Produces:

```rust
pub(crate) struct HandOver<'a> {
    pub(crate) kept: TrackId,
    pub(crate) kept_path: &'a Path,
    pub(crate) taken: TrackId,
    pub(crate) taken_path: &'a Path,
}
pub(crate) fn handed_over(transaction: &Transaction<'_>, over: &HandOver<'_>) -> Result<()>;
impl IdenticalCopy { pub(crate) fn hand_over(&self) -> HandOver<'_> }

pub(crate) fn folded(transaction: &Transaction<'_>, kept: TrackId, taken: TrackId) -> Result<()>;
pub(crate) fn rearmed(transaction: &Transaction<'_>, release_track: ReleaseTrackId, now: SystemTime) -> Result<WantId>;

impl Library {
    pub fn replace(&self, chosen: &[Replaceable]) -> Result<Replacing>;
    pub(super) fn remove_tracks_under(&self, walk: &Walk, tracks: &[TrackId], why: Why) -> Result<Deleted>;
}
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_lossy_song_with_a_lossless_copy_held_is_replaced_at_once_and_its_history_goes_with_it() -> Result<()>
```
`A.opus` and `A.flac` held (Task 3's fixture), `trash_into` a trash tree. `A.opus` has 3 plays, a favourite, two `listens` rows (play it through `track_played` three times), sits in playlist *Road trip* twice and in *Night* once, and has a `versions_chosen` row naming `A.flac`. `replace(&lossy_songs_of(road_trip)?)` answers `Replacing { folded: 1, wanted: 0, unmatched: 0, .. }` with `trashed.files == 1`. Then: `A.flac`'s row has 3 plays and the favourite; both playlists' entries name `A.flac` (three entries in all); `A.opus` is gone from disc and its row from the catalog; `versions_chosen()` is empty; `lossy_songs_of(road_trip)` is empty.

```rust
#[test]
fn two_lossy_copies_of_one_song_are_both_replaced_by_the_one_lossless_copy() -> Result<()>
```
`A.opus` (2 plays) and `A.mp3` (1 play) of the same song both in a playlist, `A.flac` held: `replace` folds both (`folded: 2`), `A.flac` has 3 plays, both lossy files are in the trash.

```rust
#[test]
fn a_lossy_song_matched_on_musicbrainz_is_wanted_and_waits_while_one_with_neither_is_counted_unmatched() -> Result<()>
```
`A.opus` paired to a release row (land a release through the test `Fake` reference as other want tests do), `B.mp3` unmatched with no copy. `replace` answers `wanted: 1, unmatched: 1, folded: 0`; `wants()` holds one want for that row with `replacing == Some(A.opus's id)` and `!is_answered()`; `A.opus` is still on disc; `lossy_songs_of` reports it `pending`.

```rust
#[test]
fn a_lossy_song_a_want_once_delivered_is_wanted_again_with_its_offer_forgotten_and_its_refusals_kept() -> Result<()>
```
A want for the release row whose `offered` names `A.opus` and a `refused_offers` row: after `replace`, that want's `offered`, `tried` are `None`, `misses` 0, and the `refused_offers` row is still there (read it with `beside(&database)`).

```rust
#[test]
fn a_replacement_waits_while_another_pass_walks_the_tree_and_writes_nothing() -> Result<()>
```
Hold the walk (start a scan held open as the existing walk tests do) and `replace`: answers `Err(Error::AlreadyWalking { .. })`, and no `replacements` row, want or hand-over was written.

```rust
#[test]
fn a_vaulted_lossy_song_is_replaced_and_its_file_and_object_let_go() -> Result<()>
```
`vault --import` `A.opus` first (the vault tests' helpers), then `replace`: the lossy file is in the trash, `vault_objects_nothing_names` no longer lists its object once `remove_tracks` ran, and `A.flac` holds its history.

```rust
#[test]
fn dismissing_a_replacements_want_cancels_it_and_leaves_the_lossy_song() -> Result<()>
```
After the wanted case, `unwant(want.id)`: no `replacements` row, `lossy_songs_of` reports it not `pending`, the file stays.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p resonate-library library:: -E 'test(/replaced|wanted_again|walks_the_tree_and_writes|cancels_it_and_leaves/)'`
Expected: compile error, no method `replace`.

- [ ] **Step 3: Generalise the hand-over.** `handed_over` takes `&HandOver` and keeps today's statements, bound from its four fields; `IdenticalCopy::hand_over` builds one from `kept_id`, `kept`, `copy_id`, `copy`; `fold_copies` calls `copies::handed_over(transaction, &copy.hand_over())`. Split `remove_tracks` into the guard (`let walk = self.walk_the_tree()?;`) and `remove_tracks_under(&walk, tracks, why)` holding the rest.

- [ ] **Step 4: Run the existing fold tests to prove nothing moved**

Run: `cargo nextest run -p resonate-library library:: -E 'test(/copies|folded|twin/)'`
Expected: PASS.

- [ ] **Step 5: Implement `replacing::folded(transaction, kept, taken)`**: read both rows' `path`, run `handed_over`; `UPDATE release_tracks SET track_id = ?kept WHERE track_id = ?taken AND NOT EXISTS (SELECT 1 FROM release_tracks WHERE track_id = ?kept)`; `DELETE FROM wants WHERE release_track_id IN (SELECT release_track_id FROM replacements WHERE track_id = ?taken)`; `DELETE FROM replacements WHERE track_id = ?taken`. The `versions_chosen` rows of the taken track go by their `ON DELETE CASCADE` when `remove_tracks_under` forgets it.

- [ ] **Step 6: Implement `elsewhere::rearmed(transaction, release_track, now)`**: the insert `want_in` makes (`ON CONFLICT DO NOTHING`), then `UPDATE wants SET offered = NULL, tried = NULL, misses = 0 WHERE release_track_id = ?1`, answering the want's id. It leaves `refused_offers` alone.

- [ ] **Step 7: Implement `Library::replace(chosen)`**: take the walk first (`AlreadyWalking` returns before any write). In one write, for each `Replaceable` whose track still is the lossy whole file it names: with `held`, still a lossless whole file, `replacing::folded(held.id, track.id)` and count `folded`; else with a `release_track` still held by the track, `INSERT OR REPLACE INTO replacements (track_id, release_track_id, asked)` and `rearmed`, count `wanted`; else count `unmatched`. Then `playlists_changed()` where anything folded, and `remove_tracks_under(&walk, &folded_ids, Why::Folded)` into `trashed`.

- [ ] **Step 8: `unwant(id)`** deletes `replacements` rows whose `release_track_id` is the want's before deleting the want, in the same transaction.

- [ ] **Step 9: Run the tests**

Run: same as Step 2.
Expected: PASS.

- [ ] **Step 10: Update `.claude/rules/library.md`**: name `A_LOSSY_SONG_WAITS_TO_BE_REPLACED` as the latest step in the migration list (moving *the latest* off the paths indexes), and add a *Replacing a lossy song* bullet after *A version chosen*: the table, `lossy_songs_of`, `replace`, the fold (`handed_over` shared with identical copies, the release row handed over only where the kept track holds none, the want and replacement deleted), `unwant` cancelling, the test names of this task. Run `cargo nextest run -p resonate-words-build` (the rules and comments checks).

- [ ] **Step 11: Commit**

```bash
git add crates/resonate-library .claude/rules/library.md
git commit -m "feat(library): replace a lossy song with the lossless copy held, or want one"
```

---

### Task 5: Settle a replacement when its lossless copy arrives

**Files:**
- Modify: `crates/resonate-library/src/db/replacing.rs` (`settle_replacements`, `rejoin_the_replacements`)
- Modify: `crates/resonate-library/src/scan.rs` (`start`, `start_over_held` run a closure after the guard drops)
- Modify: `crates/resonate-library/src/db/passes.rs` (`scan`, `scan_what_is_held` hand it `settle_after_a_scan`)
- Modify: `.claude/rules/library.md` (the settle in *Replacing a lossy song*)
- Test: `crates/resonate-library/tests/library.rs`

**Interfaces:**
- Consumes: `replacing::folded`, `remove_tracks_under` (Task 4).
- Produces:

```rust
impl Library {
    pub fn settle_replacements(&self) -> Result<Deleted>;
    pub(crate) fn rejoin_the_replacements(&self) -> Result<u64>;
}
pub(crate) fn start(inner: Arc<Inner>, options: ScanOptions, after: impl FnOnce() + Send + 'static) -> Result<ScanHandle>;
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_lossless_copy_delivered_for_a_replacement_takes_the_lossy_songs_place() -> Result<()>
```
A pending replacement of `A.opus` (Task 4's wanted case); copy `A.flac` into a root, scan it; set the want's `offered` to `MediaLocation::local(A.flac).to_uri()` (as a poll's `note_tried` would). `settle_replacements()` answers `files == 1`; `A.flac` holds the plays and the playlist entries; the release row's `track_id` is `A.flac`'s; no want and no replacement remain.

```rust
#[test]
fn a_lossless_copy_holding_the_same_recording_takes_the_lossy_songs_place() -> Result<()>
```
No offer; `A.flac` tagged with the recording MBID `A.opus`'s release row names. A scan of the folder settles it with no further call (the scan's closure).

```rust
#[test]
fn a_lossless_copy_named_alike_alone_never_replaces_a_song_unattended() -> Result<()>
```
`A.flac` of the same title, artist and length but no MBID and no offer: `settle_replacements()` answers zero files, the replacement stays, `A.opus` is on disc.

```rust
#[test]
fn a_lossy_copy_offered_for_a_replacement_is_never_taken_for_it() -> Result<()>
```
`offered` names another Opus: nothing folds.

```rust
#[test]
fn a_settle_while_another_pass_walks_the_tree_leaves_the_replacement_for_the_next() -> Result<()>
```
Evidence present, the walk held: `settle_replacements()` answers `Ok` with zero files and nothing written; released, a second call folds.

```rust
#[test]
fn a_replacement_whose_release_is_landed_again_rejoins_the_row_its_song_now_holds() -> Result<()>
```
After the wanted case, land the same release again (the `Fake`'s lookup, `refresh`): the replacement's `release_track_id` reads `NULL` before `rejoin_the_replacements()` and the new row's id after, and `wants_unheld()` (or `wants()`) shows a want for that row with `replacing` set.

```rust
#[test]
fn a_lossless_copy_on_another_album_keeps_its_own_row_and_leaves_the_lossy_songs_row_unheld() -> Result<()>
```
`A.flac` paired to its own release row elsewhere: after the settle its own row still names it and the lossy song's row has `track_id IS NULL`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p resonate-library library:: -E 'test(/takes_the_lossy_songs_place|named_alike_alone|offered_for_a_replacement|leaves_the_replacement_for_the_next|rejoins_the_row|keeps_its_own_row/)'`
Expected: compile error, no method `settle_replacements`.

- [ ] **Step 3: Implement `rejoin_the_replacements()`**: `UPDATE replacements SET release_track_id = (SELECT id FROM release_tracks WHERE track_id = replacements.track_id) WHERE release_track_id IS NULL`, then for each joined row the `INSERT ... ON CONFLICT DO NOTHING` of a want, answering how many rejoined.

- [ ] **Step 4: Implement `settle_replacements()`**: `rejoin_the_replacements()`; take the walk, and on `Error::AlreadyWalking` trace at debug and answer `Ok(Deleted::default())`; read each pending replacement's evidence in one read: the track at the `wants.offered` path (`MediaLocation::from_uri`, local only, `span_start = 0`) and the tracks whose `tracks.mbid` is the lossy row's `mbid` or its release row's `recording_mbid`; keep a candidate only where `span_frames IS NULL`, lossless (`lossless_codecs!()`) and not the lossy track, the offered one first. Fold every pair in one write through `replacing::folded`, then `remove_tracks_under(&walk, …, Why::Folded)`.

- [ ] **Step 5: Run settle after every scan.** `scan::start` and `start_over_held` take `after` and call it after `drop(walking)` when the outcome is `Ok`; `Library::scan` and `scan_what_is_held` pass a closure over `self.shared()` that calls `settle_replacements()` and traces a failure at warn.

- [ ] **Step 6: Run the tests and the scan tests**

Run: same as Step 2, then `cargo nextest run -p resonate-library library:: scanned::`
Expected: PASS.

- [ ] **Step 7: Update `.claude/rules/library.md`**'s *Replacing a lossy song* with the settle (evidence only, after a scan's guard drops, a refused walk left for the next, the rejoin after a release lands again) and its tests; run `cargo nextest run -p resonate-words-build`.

- [ ] **Step 8: Commit**

```bash
git add crates/resonate-library .claude/rules/library.md
git commit -m "feat(library): settle a replacement once its lossless copy is held"
```

---

### Task 6: The poll fetches only lossless copies for a replacement

**Files:**
- Modify: `crates/resonate-library/src/supply.rs` (provider ask, `landed`, `filed_in_the_music_folder`, `run`, `PollSummary`)
- Modify: `crates/resonate-library/src/filed.rs` (`filed_for`, `Unfiled`)
- Modify: `crates/resonate-library/src/db/vault.rs` (`note_delivered`)
- Modify: `crates/resonate-ui/src/models/told.rs` (`polled` says what was replaced)
- Modify: `crates/resonate-ui/words/en/models.ftl` (the count)
- Modify: `.claude/rules/providers.md`
- Test: `crates/resonate-library/tests/library.rs` (beside the poll tests that serve fake providers)

**Interfaces:**
- Consumes: `Want::replacing` (Task 2), `settle_replacements`, `rejoin_the_replacements` (Task 5).
- Produces: `PollSummary::replaced: Deleted`; `Unfiled::Lossy`.

- [ ] **Step 1: Write the failing tests**, each with a pending replacement of `A.opus` on its release row:

```rust
#[test]
fn a_replacement_is_never_asked_of_a_last_resort() -> Result<()>
```
Register only a fake provider whose `last_resort()` is true and that counts its `find` calls: a poll with `ASKING_EVERY_WANT` asks it zero times and `A.opus` stays.

```rust
#[test]
fn a_lossy_delivery_for_a_replacement_is_refused_and_the_next_provider_asked() -> Result<()>
```
With a vault: provider one offers an Opus, provider two the FLAC. The poll keeps the FLAC (`stats.kept == 1`), `refused_offers` holds provider one's `taken_from`, `summary.replaced.files == 1`, and `A.opus` is in the trash with its plays on the vault's FLAC row.

```rust
#[test]
fn a_lossless_delivery_filed_in_the_music_folder_replaces_the_lossy_song_at_the_polls_end() -> Result<()>
```
No vault, a `deliver_into` music folder, one provider offering the FLAC: the poll files it, `summary.replaced.files == 1`, the playlist names the filed FLAC.

```rust
#[test]
fn a_lossy_file_filed_for_a_replacement_is_taken_away_and_refused() -> Result<()>
```
Music folder, provider offering an MP3: `stats.kept == 0`, the filed MP3 is gone from the folder, the replacement stays pending.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo nextest run -p resonate-library library:: -E 'test(/a_replacement_is_never_asked|for_a_replacement_is_refused|replaces_the_lossy_song_at_the_polls_end|filed_for_a_replacement_is_taken/)'`
Expected: compile error, no field `replaced`.

- [ ] **Step 3: Skip last resorts.** Where the lane calls `self.providers.first(…)`, a want with `replacing.is_some()` asks `self.providers.without_last_resorts()` instead.

- [ ] **Step 4: Refuse lossy deliveries.** In `landed`, an arm before the length check: `Ok(Keeping::Kept(kept)) if want.replacing.is_some() && !kept.codec.is_lossless()` warns, counts `unkept` and answers `Landed::NotTheSong`. In `filed_for`, a want with `replacing` probes the landed file (`resonate_codec::probe` over `Sources::local()`) before claiming album keys; a lossy codec removes the file and answers `Err(Unfiled::Lossy)`, which `filed_in_the_music_folder` maps to `Landed::NotTheSong` beside `TooLarge | Undecodable`.

- [ ] **Step 5: Keep a vault delivery for a held row.** `note_delivered`'s `still_unheld` refusal applies only where `want.replacing.is_none()`; the insert and the `track_id IS NULL` pairing stay as they are, so the FLAC is a new rootless row.

- [ ] **Step 6: Settle at the poll's end.** `run` calls `library.rejoin_the_replacements()` before the lanes start and, after the final `scanned_and_paired`, `library.settle_replacements()` into `PollSummary::replaced` (a failure traced at warn, `Deleted::default()` kept). `told::polled` adds a part for `replaced.tracks > 0` through a new `models.ftl` message `replaced-lossy-songs = { $count -> [one] replaced 1 lossy song *[other] replaced { $count } lossy songs }`, in the style its other parts are joined.

- [ ] **Step 7: Run the tests and the poll tests**

Run: same as Step 2, then `cargo nextest run -p resonate-library supply:: library:: -E 'test(/poll|deliver|want/)'` and `cargo nextest run -p resonate-ui told`
Expected: PASS.

- [ ] **Step 8: Update `.claude/rules/providers.md`**: under *A last resort answers only once…* and *A delivery is weighed against the row's length*, a want replacing a song is never asked of a last resort and keeps only a lossless delivery (vault and music folder alike), a vault delivery for it lands as a new rootless row, and the poll settles replacements once its lanes end; name the tests. Run `cargo nextest run -p resonate-words-build`.

- [ ] **Step 9: Commit**

```bash
git add crates/resonate-library crates/resonate-ui .claude/rules/providers.md
git commit -m "feat(library): fetch only lossless copies for a song being replaced"
```

---

### Task 7: The *Replace lossy songs…* sheet

**Files:**
- Create: `crates/resonate-ui/src/views/replacing.rs`
- Create: `crates/resonate-ui/words/en/replacing.ftl`
- Create: `crates/resonate-ui/src/models/replacing.rs`
- Modify: `crates/resonate-ui/src/views/mod.rs`, `crates/resonate-ui/src/models.rs` (`mod` lines; `Shelves::lossy: AHashSet<PlaylistId>`)
- Modify: `crates/resonate-ui/src/models/watched.rs` (`shelves` reads `playlists_with_lossy_songs`)
- Modify: `crates/resonate-ui/src/views/sheets.rs` (`Sheet::Replacing`, `REPLACING_PANEL`, `KEEP_THE_LOSSY_SONGS`, `sheet_in_front`)
- Modify: `crates/resonate-ui/src/views/root.rs` (field `replacing: Option<ReplacingSheet>`, drawn over a scrim beside the other sheets), `views/root/made.rs` (`replacing: None`), the escape chain and `something_stands_over_the_pane`
- Modify: `crates/resonate-ui/src/views/playlists.rs` (`playlist_menu` gains `lossy: bool`; its callers pass `this.library.read(cx).holds_lossy_songs(id)`)
- Modify: the Wants pane rows (where `downloads.rs`' `Fetching` is worded) to mark `WantStanding::replacing`, and `crates/resonate-ui/words/en/downloads.ftl`
- Modify: `.claude/rules/ui.md`
- Test: `crates/resonate-ui/src/views/root/tests.rs` (driven), unit tests in `views/replacing.rs`

**Interfaces:**
- Consumes: `Library::lossy_songs_of`, `replace`, `playlists_with_lossy_songs`, `Replaceable`, `Replacing` (Tasks 3, 4); `fetch_or_say_nobody_can`, `wants_landed`'s reload (existing).
- Produces:

```rust
pub(crate) struct ReplacingSheet {
    pub(crate) playlist: PlaylistId,
    pub(crate) name: String,
    pub(crate) rows: Option<Vec<Replaceable>>,
    pub(crate) ticked: AHashSet<TrackId>,
}
impl ReplacingSheet {
    pub(crate) fn tick(&mut self, track: TrackId);
    pub(crate) fn tick_all(&mut self);
    pub(crate) fn ticked_rows(&self) -> Vec<Replaceable>;
}
impl RootView {
    pub(crate) fn open_replacing(&mut self, playlist: PlaylistId, cx: &mut Context<Self>);
    pub(crate) fn keep_the_lossy_songs(&mut self, cx: &mut Context<Self>);
    pub(crate) fn replacing_sheet(&self, cx: &mut Context<Self>) -> Div;
}
impl LibraryModel {
    pub fn holds_lossy_songs(&self, playlist: PlaylistId) -> bool;
    pub fn replace_lossy_songs(&mut self, chosen: Vec<Replaceable>, cx: &mut Context<Self>);
}
pub(super) fn replaced(replacing: &Replacing) -> Notice;
```

`tick` flips one row that `can_be_ticked`; `tick_all` ticks every row that can be ticked, or unticks all where every such row is ticked already; rows arrive with every tickable row ticked.

`replacing.ftl` (English):

```
replace-lossy-songs = Replace lossy songs…
replace-lossy-songs-hint = Swap this playlist's lossy songs for lossless copies
heading = Replace lossy songs · { $playlist }
tick-all = Tick all
tick-all-hint = Tick every song that can be replaced
held = you hold a { $codec } copy
wanted = wanted through MusicBrainz
not-matched = not matched on MusicBrainz
pending = already being replaced
reading = Reading the playlist…
nothing-lossy = Every song here is lossless
trash-note = Each lossy copy goes to the trash once its lossless copy is in your library.
keep = Cancel
keep-hint = Keep the lossy songs — { $keys }
replace = { $count ->
    [one] Replace 1 song
   *[other] Replace { $count } songs
}
replace-hint = Replace the ticked songs with lossless copies
folded = { $count ->
    [one] 1 song replaced
   *[other] { $count } songs replaced
}
wanted-count = { $count ->
    [one] 1 song wanted
   *[other] { $count } songs wanted
}
unmatched-count = { $count ->
    [one] 1 song not matched
   *[other] { $count } songs not matched
}
told = { $parts }
```

`downloads.ftl` gains `replacing = replacing a lossy copy`.

- [ ] **Step 1: Write the failing unit tests** in `views/replacing.rs`:

```rust
#[test]
fn every_row_that_can_be_ticked_starts_ticked_and_tick_all_turns_them_together()
```
Three `Replaceable`s (one held, one wanted, one pending): fresh `ticked` holds the first two; `tick_all` empties it; `tick_all` again holds the two; `tick` on the pending row changes nothing.

```rust
#[test]
fn the_toast_says_what_was_replaced_wanted_and_not_matched()
```
`replaced(&Replacing { folded: 1, wanted: 2, unmatched: 0, .. })` reads `1 song replaced, 2 songs wanted` and is `Notice::Done`.

- [ ] **Step 2: Write the failing driven test** in `views/root/tests.rs`, planting tracks the way its playlist tests do:

```rust
#[test]
fn a_playlists_lossy_songs_are_replaced_from_its_menu_and_the_sheet_closes_with_a_toast()
```
A playlist holding `A.opus` with `A.flac` held: right-press the playlist (`playlist-…` row), the menu holds *Replace lossy songs…*; press it; the sheet (`replacing-sheet`) lists *Echoes* ticked; press `replace-the-lossy-songs`; `Driven::until` the library's `lossy_songs_of` is empty; the sheet is gone and a toast says *1 song replaced*. A second playlist holding only FLAC shows no such entry. Escape with the sheet open closes it and changes nothing.

- [ ] **Step 3: Run them to see them fail**

Run: `cargo nextest run -p resonate-ui replacing a_playlists_lossy_songs_are_replaced`
Expected: compile error.

- [ ] **Step 4: Implement the model side.** `Shelves::lossy` from `library.playlists_with_lossy_songs()` in `watched::shelves`; `holds_lossy_songs` reads it. `replace_lossy_songs` runs `library.replace(&chosen)` on the background executor, then on the window: a toast from `told::replaced`, `fetch_or_say_nobody_can` where `wanted > 0`, and a `Wanted::Everything` reload; a failure toasts through `toast::could_not` as the model's other passes do (an `AlreadyWalking` says a scan is running, as `Tend` does).

- [ ] **Step 5: Implement the sheet** after `list_offer.rs`'s shape: `kit::sheet` with `dialog`, `track_focus(REPLACING_PANEL)`, `kit::sheet_heading` (an existing `Icon` variant in the accent; the list is `icons!` in `icons.rs`, and no new SVG is added), a `kit::checkbox` *Tick all* in the heading row, a scrolled list of rows (a `kit::checkbox` per row named `replacing-row-<track id>`, `spoken::song`, the codec through `kit::format_badge`, the state words above, greyed rows through `kit::Press::Greyed`), the trash note in `muted`, and `kit::sheet_actions` with *Cancel* (`KEEP_THE_LOSSY_SONGS`, outlined, the welcome control) and *Replace N songs* (`replace-the-lossy-songs`, primary, greyed while nothing is ticked). `open_replacing` closes the menu, sets the sheet with `rows: None`, reads `lossy_songs_of` on the background executor and fills `rows` and `ticked` unless the window has moved on (`press_a_lookup`/`window_has_moved_on_from`). *Replace* takes `ticked_rows()`, closes the sheet and calls `replace_lossy_songs`.

- [ ] **Step 6: Wire it in**: `Sheet::Replacing` with its panel and welcome, `sheet_in_front` after `Deleting`, the escape chain and `something_stands_over_the_pane`, `playlist_menu`'s entry after *Duplicate* only where `lossy`, and the Wants pane row's `replacing` mark. Add `views/replacing.rs` to nothing in `views/audit.rs`'s lists unless the audit asks.

- [ ] **Step 7: Run the window's tests and checks**

Run: `cargo nextest run -p resonate-ui`
Expected: PASS, including `every_word_the_window_shows_is_said_through_its_catalog` and the click audit.

- [ ] **Step 8: Update `.claude/rules/ui.md`**: a line under *Panes* for the sheet (menu entry only where the playlist holds a lossy song, rows start ticked, *Tick all*, greyed rows, the trash note in place of a confirmation, the toast, the Wants pane mark and its `unwant`), naming the driven test. Run `cargo nextest run -p resonate-words-build`.

- [ ] **Step 9: Format, lint, commit**

Run: `rust-formatter && rust-formatter --check && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy --workspace --exclude resonate-ui --no-default-features --all-targets -- -D warnings`
Expected: no diff, no warnings.

```bash
git add crates/resonate-ui .claude/rules/ui.md
git commit -m "feat(ui): replace a playlist's lossy songs from its menu"
```

---

### Task 8: Whole-branch check

- [ ] **Step 1: Run everything**

Run: `cargo nextest run --workspace`
Expected: PASS (tests needing ffmpeg skip where it is absent; say so if they skipped).

- [ ] **Step 2: Lint the feature lines**

Run: `cargo clippy -p resonate --no-default-features --features online,mcp,discord,sync --all-targets -- -D warnings` and the same with `--features ui`.
Expected: no warnings.

- [ ] **Step 3: Drop a roadmap entry if one names this.** `grep -n -i "lossy\|replace" docs/FEATURES.md docs/ROADMAP.md`; remove an entry this lands, add one for the phones, terminal and MCP surfaces left out. Commit as `docs: ...` if anything changed.
