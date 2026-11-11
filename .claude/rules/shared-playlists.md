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

- **An edit key derives a view key, never the reverse** (`shared/key.rs`, so one set of bytes is
  never both a pairing code and a shared list). The view key derives the sealer and a read token;
  the edit key adds a write token. A code is the pairing code's spelling behind `E-` or `V-`.
- **A list's space commits to its write token, so a view link cannot claim it first** (`EditKey`,
  `ViewKey`, `Committed`). The server cannot tell an owner from a viewer by anything but the space id,
  and a list may be shared offline, links handed out while its space still waits: were the space
  derived from the view key alone, whoever reached the server first with a token of their own would
  claim it and write records every viewer opens as the owner's, and the owner would be refused
  `Token`. So a list minted now has a 32-byte edit key (HKDF salt `resonate-shared/2`): it derives the
  write token and a 20-byte view secret, and its space is `SpaceId::committed_to` the write token's
  digest (four marker bytes and 12 of a salted SHA-256, `key.rs`), which the server checks before it
  makes the space (`server.md`). The view key is the secret and those 12 bytes, 32 bytes again, so
  both codes are 55 symbols where an older one is 35, and the length is the version
  (`a_new_list_names_a_space_only_its_write_token_may_make_and_its_links_say_so`). This is chosen
  over signing records with a key the edit key derives: a viewer would need the public key, 32 bytes
  in the view link beside a secret, every record and snapshot would carry a signature checked on
  every device, and the trust model stays the one the server already enforces (a view link cannot
  write because the server refuses it); the commitment needs nothing of the record format or the
  merge and nothing of an older server, which simply does not check it.
- **A link handed out before commitments keeps working, claimed first come as it was**
  (`EditKey::FirstToClaim`, `ViewKey::FirstToClaim`, salt `resonate-shared/1`): a 35-symbol code
  reads the space and keys it always did
  (`a_link_handed_out_before_spaces_committed_reads_the_same_space_and_keys_as_it_did`), and its
  unmarked space is claimed by the first token. No step moves it: a list its owner has reached the
  server with is already held by the owner's token, and moving it to a committed space would strand
  every view link that never joined (a reset leaves those without a seat). What stays open is only a
  list shared offline before this build whose space a viewer claims before its owner arrives; the
  owner sees `SharedStanding::TurnedAway(Token)`, and sharing the list anew gives it a committed space. An older build reads a
  55-symbol code as a wrong length, so a list minted now cannot be joined, adopted from
  `Change::Share` or found in a reset's seat by a build that predates it (it keeps its copy).
- **A view link cannot write because the server refuses it**, not because anything is signed: `View`
  has no way to ask (`server.md`). So editors need no signatures, and trust each other: an editor can
  compact, and could write nonsense, and removal is the answer to that.
- **A link is `resonate://shared/<code>`, with `?server=` where the list is not on the hosted server**,
  or the page `https://<host>/s/#<code>`, which chat apps linkify (`invitation.rs`). The fragment
  never reaches the server.

A list on a server other than the hosted one names it in its links, and its page is on that server
(`Running::named_in_a_link`), so a self-hosted list is never sent to the hosted page.

**A link from outside that names a server other than this device's asks before it joins**, since joining
makes the device reach that server every five minutes. `together::accepted` with `Arrival::FromOutside`
(an argument, `OpenUri`) compares the link's server with `HOME_SERVER`, the address `server_of` gave
the settings (the hosted one by default; `Invitation::naming_a_server_other_than` ignores case and a trailing slash): a link
naming none, or the device's own, joins at once as before, and any other goes to `party_room::offer`,
which tells the window `Followed::ListOffered(ListOffered { link, server })` as it does a party's. A
build with no window refuses it. The window's sheet is `views/list_offer.rs` (`Sheet::OfferingAList`,
*Join a playlist on another server?*, saying the server named; focus lands on *Cancel*, no key
confirms, escape declines and the window is raised), and *Join* hands the link to
`Together::accept`, as pasting does. A link pasted or typed (`Arrival::Pasted`) joins at once. The phones ask the same way (`mobile.md`)
(`a_link_names_another_server_only_where_it_is_not_the_one_this_device_uses`,
`a_list_on_a_stranger_server_from_outside_asks_first_and_focus_lands_on_the_control_that_declines`).

## The merge

**An op log, not a whole list last writer wins** (`shared/op.rs`), since two people editing at once is
the ordinary case here. A row has a random `EntryId`, a fractional `Place` (`shared/place.rs`, last
writer wins) and a removal that is final; adding a song again is a new row, so the same song may stand
twice. A place read off the wire ends in a byte other than zero, as every place minted here does, or its record is refused (`Malformed::NotAPlace`), and `Place::between` stops at the high place's end, answering a place past the low one where the two leave no room, so no place a member sends can hold another's client in a loop. The name is last writer wins, and a `Member` carries a typed name and an x25519 public key. Every
op is idempotent and commutative (`however_the_records_cross_every_device_ends_on_the_same_list`). Two
people adding at one spot both keep their song, ordered by entry id. A snapshot is the state as ops, so
compaction needs no second format; an op of a later build is passed over. **A list shows at most
`ROWS_AT_MOST` (10 000) rows**, the first by place and entry id after the fold, so a stranger's
server cannot fill a catalog playlist with millions and every device cuts the same rows whatever
order the ops arrived in; the diff, the catalog write and a look all read `rows()`, so none sees a
row the others do not (`a_list_shows_only_so_many_rows_the_same_ones_whatever_order_they_arrived_in`).
**A list keeps at most `ENTRIES_KEPT_AT_MOST` (4 × `ROWS_AT_MOST`) entries in memory**, a removed one's tombstone counted: an op naming an entry past the smallest that many entry ids seen so far is dropped, and one that pushes the largest out evicts it for good. Since the set of the smallest ids only ever shrinks its largest, the entries kept are the smallest ids among every entry any op named, whatever order the ops came in, so every device folds to the same rows and a snapshot carries the same ones; a hostile writer within the server's space limit costs a follower no more than that map (`a_list_keeps_only_so_many_entries_the_same_ones_whatever_order_the_ops_arrived_in`). Removals do not free a slot, since freeing one would need the evicted rows back.

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
  removed, which is leaving; a copy changed anyway is written back. **On the phones the catalog tells
  it, not the service**: `MusicPlaylist.followed` is `playlist_shares.editable = 0`, so a followed list
  hides its edit controls and reads *Followed* with sync off, where `shared_playlists()` is empty; a
  `SharedPlaylist` still decides when it is there (`mobile.md`).
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
read as `ServerTooOld` only after `CLOSES_BEFORE_ANSWER_FOR_TOO_OLD` (3) clean closes in a row from one
server before any answer to `Heads`; one close, as a tunnel or a draining server sends, is `Unreached`
and tried again (`one_clean_close_is_a_server_lost_and_not_a_server_too_old`,
`a_server_that_closes_every_connection_reads_as_too_old`); a close while a reset waits for its answer
is `Error::ServerSilent`. `shared.lock` keeps one process per catalog, since two would mint two entries for
one edit (`SharedBusy`). `resonate shared` is the terminal's way in (`binary.md`).
