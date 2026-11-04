---
paths:
  - "crates/resonate-sync/src/shared/**"
  - "crates/resonate/src/together.rs"
  - "crates/resonate-ui/src/views/sharing.rs"
  - "crates/resonate-mobile/src/shared.rs"
  - "crates/resonate-library/src/synced.rs"
---

# Shared playlists

A playlist shared with other people is its own sealed space on the sync server. Nothing about it
touches the pairing key, there are no accounts, and the server reads nothing it keeps.

## Keys and what a link gives

- **An edit key derives a view key, never the reverse** (`shared/key.rs`, HKDF salt
  `resonate-shared/1`, so one set of bytes is never both a pairing code and a shared list). The view
  key derives the space, the sealer and a read token; the edit key adds a write token. A code is the
  pairing code's spelling behind `E-` or `V-`.
- **A view link cannot write because the server refuses it**, not because anything is signed: `View`
  has no way to ask (`server.md`). So editors need no signatures, and trust each other: an editor can
  compact, and could write nonsense, and removal is the answer to that.
- **A link is `resonate://shared/<code>`, with `?server=` where the list is not on the hosted server**,
  or the page `https://<host>/s/#<code>`, which chat apps linkify (`invitation.rs`). The fragment
  never reaches the server.

A list on a server other than the hosted one names it in its links, and its page is on that server
(`Running::named_in_a_link`), so a self-hosted list is never sent to the hosted page.

## The merge

**An op log, not a whole list last writer wins** (`shared/op.rs`), since two people editing at once is
the ordinary case here. A row has a random `EntryId`, a fractional `Place` (`shared/place.rs`, last
writer wins) and a removal that is final; adding a song again is a new row, so the same song may stand
twice. The name is last writer wins, and a `Member` carries a typed name and an x25519 public key. Every
op is idempotent and commutative (`however_the_records_cross_every_device_ends_on_the_same_list`). Two
people adding at one spot both keep their song, ordered by entry id. A snapshot is the state as ops, so
compaction needs no second format; an op of a later build is passed over.

## The seam

**A shared list is an ordinary catalog playlist, so every pane, the CLI, the bus and the phones read it
unchanged** (`SharedCatalog`). `playlist_shares` flags it by `(id, created)`, so a reused id is not
shared by accident; flag and row are written in one transaction, so no look can mint a personal key
between them. `synced_playlists` leaves flagged rows out, so the personal ledger never offers them.

- **A look diffs the catalog against what was last written** (`diff.rs`): rows pair by name, then
  `held_as`, in order, so duplicates survive; a row a rescan left unnamed removes nothing; unheld rows
  stay in the list and keep their place, shown from `SharedShown::unheld`; a moved row is the longest
  rising run's complement, so one drag is one op
  (`whatever_the_listener_did_the_list_comes_to_read_as_the_catalog_does`). The alias a catalog wrote is
  not told as a rename.
- **A followed copy refuses every edit** (`undo::edited`, `Error::PlaylistReadOnly`), but may be
  removed, which is leaving; a copy changed anyway is written back.
- **Removing a list here is leaving it**, never removing its songs for the others: a `Left` op goes,
  and the list is forgotten once told. A friend's edit drops the undo steps of the list
  (`undo::overtaken`).

## Your own devices

**Sharing a list never tells your other devices to delete it.** A mapped playlist gone from the
personal view would be a tombstone, so the look asks `Catalog::hides_as_shared` first and sets the
ledger's `released` instead (`SHARED_PLAYLISTS_STAND_APART`): the row stays, nothing is told, and a note
for it writes nothing
(`sharing_a_playlist_tells_no_other_device_to_delete_it_and_a_later_note_writes_nothing_here`).
`Change::Share` (wire tag 10) then carries the code, the member and its secret under the pairing key, so
a listener's devices are one `Member`, each list keeping its own (`store::Me` per list); it names the
released key, so a current device takes its own copy over, and an older build keeps its copy untouched
and skips the tag. `Catalog::shares_here` answers `None` where the service is not running, which says
nothing; only an empty list says everything was left. An older build compacting the tag away owes it
again (`owe_queries`).

## Removing someone

**Removal is a reset** (`moved.rs`, `Running::reset`): mint new keys, seal the new code to each
remaining member's x25519 key, `Close` the old space with those seats, and move. `Close` is first come
and kept apart from the log (`server.md`), so two resets race to one answer and a person being removed
cannot compact it away. Whoever finds a seat moves without noticing, and whoever finds none, a view
link included, keeps their copy as their own list (`shared_unflag`)
(`removing_someone_moves_everyone_else_and_leaves_them_only_the_copy_they_held`).

## The service

**No session is held open** (`shared/service.rs`): one thread, `Heads` every five minutes and every five
seconds while a list is shown (`Shared::watch`), and a session only for a space that moved, is unknown
to the server or owes something, so a person in many lists holds no connections and stays inside
`--attempts-per-minute`. Offline, sharing and editing work, and what is owed goes when the server is
back. Under *This network only* nothing can be shared. An old server drops the frames it does not know,
read as `ServerTooOld`. `shared.lock` keeps one process per catalog, since two would mint two entries for
one edit (`SharedBusy`). `resonate shared` is the terminal's way in (`binary.md`).
