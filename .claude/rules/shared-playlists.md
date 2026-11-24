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
  never reaches the server; the page opens the list in the browser with it, as a follower does,
  and shows its name and songs beside *Open in Resonate* (`server.md`).

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
op is idempotent and commutative (`however_the_records_cross_every_device_ends_on_the_same_list`),
but for one thing no honest writer does: **the first key heard for a `MemberId` stands** (`Person::public`).
A later `Member` may rename the person, never rekey them, since editors are not told apart and one
about to be removed could otherwise put their own key under everyone else's id and open every seat
of the reset (`a_later_member_op_from_another_writer_renames_someone_but_never_replaces_their_key`).
A member's key never changes honestly (a listener's devices share one `Me` per list through
`Change::Share`), and an id is random, not derived from the key, so the rule needs no stored change.
An all-zero key is no key: a snapshot writes it for someone heard only leaving, and the key they join
with is taken after it
(`someone_heard_leaving_before_joining_takes_the_key_they_join_with_and_a_snapshot_keeps_none`). **A stamp is believed at most a day past the wall clock, and every list keeps its own clock**:
`heard_ops` passes over an op stamped past `STAMP_AHEAD_OF_THE_WALL_AT_MOST_NANOS` as it is heard,
and stamps what this device writes from the list's own `Clock` (`Kept::stamped`, started afresh with
each run as the one shared clock was). One op stamped near `i64::MAX` would otherwise hold the clock
there, so a second move or rename of a row compared equal to the first and was dropped, in every list
the service held
(`a_stamp_far_past_the_wall_clock_is_refused_and_later_moves_still_land`,
`a_stamp_within_a_day_ahead_is_taken_and_moves_only_its_own_lists_clock`). A snapshot's parts are
heard through the same `heard_ops`, so a compaction an older build sealed with such a stamp folds
without that op
(`a_snapshot_compacted_with_a_stamp_far_past_the_wall_clock_is_folded_without_it`). **A list a
device folded before the bound is restamped as it loads**: the kept state is an encoded blob SQL
cannot reach, so `Store::lists` asks `Kept::restamped_past_the_bound` of each list and keeps any it
changed. Every stamp past the bound, in the folded list and in what it owes, is brought back to the
wall clock keeping its device, ranked from the wall in their old order (so someone who joined and
left past the bound is still here or gone as before); the list's clock sees the latest, so the next
edit here outranks them; and an editable list owes the ops it restamped, so the other members, which
refused the far stamp and never folded it, converge on what this device holds, as the ledger's
restamp owes its rows (`sync.md`)
(`a_list_kept_with_stamps_past_the_wall_clock_is_restamped_as_it_loads_and_takes_later_honest_edits`,
`restamping_keeps_the_order_of_the_stamps_it_brings_back_and_a_followed_list_owes_nothing`). Two
people adding at one spot both keep their song, ordered by entry id. A snapshot is the state as ops, so
compaction needs no second format; an op of a later build is passed over. **A list shows at most
`ROWS_AT_MOST` (10 000) rows**, the first by place and entry id after the fold, so a stranger's
server cannot fill a catalog playlist with millions and every device cuts the same rows whatever
order the ops arrived in; the diff, the catalog write and a look all read `rows()`, so none sees a
row the others do not (`a_list_shows_only_so_many_rows_the_same_ones_whatever_order_they_arrived_in`).
**A record read off the wire holds at most `BUNDLE_OPS_AT_MOST` (256) ops and inflates to at most `LARGEST_BUNDLE_OPENED`** (`decoded_ops`; the kept list and the owed ops read under `decoded_kept_ops`, which allows a whole list), and `SharedList::apply` answers whether the list changed, so the service never clones the list to compare.
**What a device owes goes up in as many `Append`s as it takes**: `append_frames` seals the bundles in
turn into `AppendFrame`s of at most `BLOBS_A_FRAME_AT_MOST` blobs and `APPEND_BYTES_AT_MOST` (half a
frame), a new frame begun where the next bundle would not fit; they are sent one after another, each
waiting for its `Appended`, and the ops it covers leave the owed ones (`owed_sent`), so an owner owing
a million ops is never refused whole and one refused partway owes only what was not yet taken
(`an_append_of_more_bundles_than_a_frame_carries_is_split_across_frames`).
**A list's compaction is staged as the device ledger's is** (`sync.md`): the server's default space
(64 MiB) holds more than a frame (32 MiB), so `compaction_pieces` seals the list's bundles into
frames of at most `BLOBS_A_FRAME_AT_MOST` parts and `APPEND_BYTES_AT_MOST` bytes. One frame is one
`Compact`; more ask `Stage`, and on the server's `Staging` go up as a `Staged` a frame and a
`CompactStaged` naming `through` with the last, which the server swaps in as one compaction
(`compaction_answered`,
`a_compaction_of_more_parts_than_a_frame_carries_is_staged_and_one_that_fits_is_one_compact`,
`a_compaction_heavier_than_half_a_frame_is_cut_where_half_a_frame_ends`,
`a_server_staging_is_sent_every_frame_and_named_through_its_last`,
`a_list_past_a_frames_share_is_compacted_in_parts_and_someone_joining_reads_it_whole`). **A `Stale`
answering `Stage` (a server before staging) and a `SpaceFull` answering any of them back the
compaction off**: the service keeps the count it was found at (`Oversized`, as the device ledger
does), seals again only once the records since double, and ends the round as it would have, with a
`Bye` and `Reached`, so a list its server cannot compact still syncs
(`a_server_refusing_to_stage_takes_no_parts_and_a_stale_compaction_was_overtaken`,
`a_full_space_backs_the_compaction_off_until_the_records_double_and_other_refusals_stand`,
`a_compaction_finding_the_space_full_backs_off_and_the_round_still_ends_well`). A visit waits on
each frame's answer before the next, so a `SpaceFull` heard while compacting is the compaction's; one
answering an `Append` still turns the list away. A `Stale` answering a `Compact` or a
`CompactStaged` is a compaction another member's overtook.

**A list keeps at most `ENTRIES_KEPT_AT_MOST` (4 × `ROWS_AT_MOST`) entries in memory**, a removed one's tombstone counted: an op naming an entry past the smallest that many entry ids seen so far is dropped, and one that pushes the largest out evicts it for good. Since the set of the smallest ids only ever shrinks its largest, the entries kept are the smallest ids among every entry any op named, whatever order the ops came in, so every device folds to the same rows and a snapshot carries the same ones; a hostile writer within the server's space limit costs a follower no more than that map (`a_list_keeps_only_so_many_entries_the_same_ones_whatever_order_the_ops_arrived_in`). Removals do not free a slot, since freeing one would need the evicted rows back. **The people map is bounded the same way** (`PEOPLE_KEPT_AT_MOST`, 1024, `Member` and `Left` ops alike): the smallest member ids seen are kept whatever order the ops came in (`a_list_keeps_only_so_many_people_the_same_ones_whatever_order_the_ops_arrived_in`).

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
again (`owe_queries`). A share takes every catch-up path the ledger's other tables take: it is owed
again with everything (`through_a_server`, `space_made_anew`), counted in `owed_count`, and forgotten
with the space (`enter_space`), so the next look writes each list held here anew and owes it to the
devices met there
(`a_share_is_told_again_in_another_space_and_reaches_a_device_met_there`,
`a_share_is_owed_again_when_the_space_is_made_anew_and_reaches_a_device_joining_it`,
`a_share_told_without_a_server_is_owed_again_once_there_is_one`,
`an_owed_share_is_counted_among_what_is_owed`).

## Removing someone

**Removal is a reset** (`moved.rs`, `Running::reset`): mint new keys, seal the new code to each
remaining member's x25519 key, `Close` the old space with those seats, and move. `Close` is first come
and kept apart from the log (`server.md`), so two resets race to one answer and a person being removed
cannot compact it away. Whoever finds a seat moves without noticing, and whoever finds none, a view
link included, keeps their copy as their own list (`shared_unflag`)
(`removing_someone_moves_everyone_else_and_leaves_them_only_the_copy_they_held`).

**The seats are read after the space is caught up**: `reset_once` opens the session, reads every
record to the `Welcome`'s head (`caught_up`) and only then seals a seat for each person the list
holds (`seats_after`: someone with no key gets none, and the resetting device's own seat is always
sealed to its own personal key, whatever the list holds for its id:
`a_removal_seals_the_removers_seat_to_their_own_key_whatever_the_list_holds_for_them`), so someone who joined since this device last read the list keeps theirs, and the songs heard
on the way reach the catalog (`written_if`)
(`a_reset_seats_someone_who_joined_since_this_device_last_read_the_list`). A join reaching the server
between that read and the `Close` (one round trip) is as one made after the reset: its link is
already spent.

**A reset knows its own close by its bytes.** The server keeps the first `Close` and answers a later
one with that first word (`Closing::Before`), and a space already closed says so after its records
before any `Close` is sent, so the first `Closed` a reset hears may be an earlier reset's. The word
sealed here holds seats sealed to one-time keys, so only this reset's own `Closed` is byte for byte
the word it sent; no wire field is needed and an older server answers the same. Any other word
(heard while catching up, or answering the `Close`) means an earlier reset won: `followed` takes it
as every member does, applying a `Left` for each person this device holds whom it left out (the
people that reset removed, so they are not seated again from a list that still names them), moves
to its space, and the reset begins again there, catching up and closing that space with the person
asked removed. After `RESETS_OVERTAKEN_AT_MOST` (4) spaces in a row closed under it the reset fails
`Error::ResetOvertaken`; an earlier word that seats nobody here leaves the copy as removal does and
fails `Refused::Unavailable`. It never stops in an older reset's space
(`a_reset_meeting_an_earlier_reset_follows_it_and_still_removes_whom_it_was_asked_to`).

## The service

**A space swept and made anew is told by its incarnation, as the device ledger's is** (`sync.md`):
a list keeps the `Incarnation` its server last told beside its cursor (the shared store's second
step, `A_LIST_KNOWS_ITS_SPACE_BY_ITS_INCARNATION`: `lists.incarnation`, NULL for a list kept before
it and for one just shared, joined or moved until a server tells one), and an editor says its
`Share`, a view link its `View`, at `INCARNATION_VERSION`. `Kept::welcomed` weighs each `Welcome`:
another incarnation than the one kept, or a head below the cursor, owes an editor's whole list
again from cursor 0 (`owe_everything`, as a move does) and only sets a view link's cursor back to 0
with nothing owed (`read_from_the_start`, since a view link cannot write and a list owing ops would
be visited at every poll), and the visit ends and begins again after 0, so a member back in a space
another member remade and wrote past reads it from its start and sends its own ops again
(`a_member_back_in_a_shared_space_made_anew_and_written_past_their_cursor_reads_it_whole_and_their_own_edits_reach_the_others`,
`a_space_of_another_incarnation_is_made_anew_though_its_head_is_past_the_cursor`), and a view link
back at such a space reads it whole
(`a_viewer_back_at_a_shared_space_made_anew_and_written_past_their_cursor_reads_it_whole`,
`a_view_of_a_space_of_another_incarnation_reads_it_from_its_start_and_owes_nothing`). A `Welcome`
telling none is judged by its head alone and leaves the kept one
(`a_server_that_tells_no_incarnation_is_judged_by_its_head_alone_and_the_one_kept_stays`); one told
where none is kept is learnt and trusted
(`a_store_of_before_incarnations_keeps_its_lists_and_learns_the_first_incarnation_each_is_told`).
`Heads` carries no incarnation, so a list is visited wherever its head differs from its cursor,
below it too; a remade space whose head stands exactly at the cursor is noticed at the next write
to it. A server before incarnations refuses the version as `Refusal::Version`: the list is visited
again at `PROTOCOL_VERSION` and the server remembered for the run (`servers_before_incarnations`; a
reset and a view link's `View` fall back the same way), so only the first visit pays the refusal
(`a_shared_list_on_a_server_before_incarnations_is_said_at_the_first_version_from_then_on`,
`a_view_link_on_a_server_before_incarnations_is_said_at_the_first_version_from_then_on`). The
browser's page says its `View` at `PROTOCOL_VERSION` and is told no incarnation: it holds no cursor
and reads the space whole each time.

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
