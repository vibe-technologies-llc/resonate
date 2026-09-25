# Audit

High-confidence correctness defects in the library, its import paths, and the root watch.
Read against `65350dc`. Each item was confirmed in the code it names. Nothing here is a fix.

## Watch and scan

### A renamed folder whose name contains a dot is forgotten as a deletion

`crates/resonate-library/src/watch.rs` classifies `Modify(Name(From))` as gone and returns
before the event can also count as a change. A rename counts as a change only when some path
has no extension. `Dr. Dre`, `Vol. 2` and `R.E.M.` all have one, so the `To` event is unread
and nothing records that a change is in flight.

`forget_the_gone` in `crates/resonate-library/src/scan.rs` then deletes every catalog path
equal to that directory or under it, once `exists` is false. Inotify reports a directory
rename as those two events, not one event per file inside it. About 250 ms later the tracks
are gone — plays, listens and favourites with them — and the new path is never scanned. A
folder with no dot still sets the in-flight flag, and the watch holds the deletion until the
scan that follows the files has the walk lock.

Treat a directory rename as a change as well as recording the old path as gone, and keep the
hold that refuses to forget while a change is in flight.

### One missing root fails every automatic scan in that batch

`scan::roots` returns `RootNotADirectory` on the first path in a non-empty list that is not a
directory, before any root is walked. An empty list is the one that skips a root that is not
mounted. `scan::start` returns success as soon as the thread is spawned, so the window's
`start_scan` clears the pending list. The thread's later error raises a notice and does not
put the roots back.

A drive that unplugs after its path was queued therefore drops every other root queued with
it. The same call registers any path that is still a directory, including one just removed
from the catalog, because `register_root` runs for every path the walk is handed.

A watch-driven scan should skip a path that is not a directory, the way a bare scan does, and
should scan only paths the catalog still holds. Clear a root from the pending list only once
the scan has accepted it.

### Rebuilding the root watch throws away events still waiting

`Watching::following` in `crates/resonate-ui/src/models.rs` replaces `RootsWatch` whenever the
mounted-root list changes. Pending roots and gone paths live only in the watcher that was
dropped. A root that was absent and is a directory again is queued for a scan. A root that
stayed mounted is not. If `RootsWatch::over` returns nothing, the previous watcher is still
discarded and the new list is marked tried, so a later poll does not try again until the set
changes.

A drive appearing or disappearing throws away deletions still inside the 250 ms quiet period
and changes still inside the 2 s quiet period on every other root, overflow included. A failed
re-watch stops automatic scans for the rest of the session.

Carry the previous watcher's unhanded roots and gone paths onto the new one before dropping
it. If the watcher cannot be created, keep the old one and leave the tried list unchanged.

### One non-UTF-8 path aborts the whole forget batch

Every `RenameMode::From` is stored as gone, including a name that is not UTF-8.
`taken_away` removes those paths from the watcher before `forget_the_gone` runs.
`store::path_text` fails the transaction on the first such path, before any row is deleted,
and the write rolls back. Any error other than `AlreadyWalking` then clears the window's
queue.

Deleting or renaming a Latin-1 file in the same quiet window as real audio deletions leaves
those audio rows in the catalog. The watch will not report them again, and no scan is
scheduled. A non-UTF-8 name on its own names nothing the scan stored.

Skip a path the catalog cannot store, and on a real store error leave the queue in place the
way a walk that is already running does.

### Archive objects inside a scanned root become library tracks

The walk in `crates/resonate-library/src/scan.rs` descends into every directory under a root
and treats `.flac`, `.mp3`, `.wav`, `.dsf` and the other archive extensions as audio. The
default vault sits outside the music folders. A vault placed inside a scanned root does not.

A library import writes an object the catalog does not already name by that path, so the next
scan inserts it as its own rooted track, titled from the hash stem, with no `vault_key`. A
delivery is inserted at the object path with `root_id` null and `answered` null, and
`ON CONFLICT DO NOTHING` does not repair a row a scan inserted first. The track upsert sets
`root_id` unconditionally and, while `answered` is null, replaces the title and the artist.
An incremental scan skips that upsert only when it already knows the path and the size and
mtime match. A full reread does not. Organise can then move the object out of `audio/`, and
the source row's `vault_path` points at a file that is no longer there.

Do not descend into the open vault's root. A path that is already a vault object must not
gain a second row, and must not take the upsert's `root_id`, title or artist assignments.

## Vault import

### A whole-file open of a vaulted cue album plays the first cut

`Vaulted::stands_in` matches `path` and `span_start` only. A missing span is asked as start
`0`. The first cue cut, `INDEX 01` at `00:00:00`, is stored at start `0` with `span_frames`
set, and a sheet-cut file has no whole-file row beside it. `Decoder::open` returns that
object immediately and does not confine it. The decoder's own test states the contract this
breaks: a row read whole must not be stood in for by a cut's object. `query_row` would error
if a whole-file row and the first cut both matched; the cue album has only the cut.

After the album is imported, opening the audio file with no span — a queue row of the file,
an M3U of the wav, `resonate play` of the file — plays the first track's object, its length
and its tags. A span that only shares that start is answered the same way. A cue row opened
with its own span still matches that cut.

Match `span_frames` as well. No span is `span_start = 0 AND span_frames IS NULL`. A bounded
span is that start and that length. A span that does not match exactly falls through to the
file.

### A later probe files an organised album under a new key and deletes the old one

`re_key_the_sleeves` leaves an album's keys untouched when it has more than one — a folder
key plus the release key enrichment added — and when the new folder's sleeve key already
names another album. That refusal is deliberate. The next probe does not use the album the
row already belongs to.

`grouping_keys` groups a compilation, or an album with no album-artist tag, by the sleeve of
the path the file is in now, unless the file itself carries a release id. `album_already_named`
returns the first of those keys that exists. The upsert writes `album_id` from that answer
even for an answered row. `ORPHANS`, run from `prune`, deletes an album that no longer has
tracks.

This holds only while scans take the unchanged fast path. *Read every file again*, or any
probe after the size, the mtime or the span no longer match, files every track under whoever
owns the new folder key, or under a brand-new album, then deletes the album that was left
empty — its MusicBrainz id, its cover, its release rows and its wants. Two compilations of
the same title with no album-artist tag are the ordinary case: the default layout puts both
in `<root>/<album>/`.

On a probe, if the row already belongs to an album and the new sleeve key names a different
album or nobody, keep this album and attach a free sleeve key to it.

### A cancelled delivery leaves an object nothing names

`keep_delivered` renames a validated object into `audio/` before it returns. `landed` in
`crates/resonate-library/src/supply.rs` then returns nothing if the poll was cancelled, so
`note_delivered` never runs, and the poll also skips `note_tried`. `prune_the_vault` removes
`vault_objects` rows nothing names, then loose covers and staging files. It does not walk
`audio/` for a file that never gained a row.

Cancel during the encode still leaves a full object on disk. The want stays due. A later poll
of the same bytes can adopt it. A different file, or no later poll, leaves the object there.
Forgetting it must skip a dedup of an object some row already names.

## Playlist import

### A file URI is accepted only as lowercase `file://`

`scheme_of` treats any alphabetic scheme as a URI, so `FILE:///tmp/a.wav` never falls through
to a plain path. `local_file` strips only the exact prefixes `file://` and `localhost`, so
`FILE:///…` and `file://LocalHost/…` return nothing and the row is counted as not local.
`file:/tmp/a.wav` has no `://`, so it is stored as a relative path beside the sheet. An
uppercase `xml:base` takes the same `local_file` path and becomes `Elsewhere`, so every
relative XSPF row under it is dropped.

Match the scheme and the `localhost` authority without regard to case, and treat `file:/`
plus an absolute path like `file:///`. `file:track.flac`, with no slash, stays a relative path.

### A blank PLS `FileN=` becomes the sheet's folder

`File1=` still calls `located` after the value is trimmed to empty. An empty string has no
scheme, so `resolved` joins it onto the sheet's directory and `canonical` stores that
directory. An empty XSPF location is rejected. The directory is inserted like any other row.
`is_file` only adds it to the missing count. A second import of that sheet counts the folder
as already held, so the row stays.

An empty PLS value counts as not local and stores nothing.

### Windows separators are rewritten only on plain M3U and PLS rows

`forward_separated` rewrites a backslash-only row, and `located` calls it only when the row
has no scheme. `from_uri` never runs it, so `file:///music\a.wav` is stored with a literal
backslash. A scheme-less XSPF location is `PathBuf::from` of the unescaped text, so
`album\a.wav` is one filename. M3U and PLS plain rows go through `located`.

`forward_separated` as it stands also refuses a string that already contains `/`. A decoded
`file:///` path always does, so the URI case has to replace backslashes on its own. A Linux
name that really contains `\` still round-trips: export percent-encodes it, and the decoded
path contains both separators, which this rewrite must leave alone.

### An uppercase hex character reference is left in the path

The hex marker is the character `x` only. `&#X2F;` fails the hex strip and then fails the
decimal parse, so `entity` returns nothing. The failure path keeps the `&` and resumes one
byte later, and the location or `xml:base` keeps the literal `&#X2F;` text. `&#x2F;` already
becomes `/`. `inherited` and `referenced` both run `plain_text` first, so the bad decode is
the path that is stored.

Accept `x` and `X` before the hex parse.
