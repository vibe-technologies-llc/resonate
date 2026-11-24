---
paths:
  - "crates/resonate-server/**"
  - "packaging/server/**"
---

# The sync server

`resonate-server` keeps what devices of one space append, hands it to the others and introduces
devices to each other. It reaches `resonate-sync` (the wire, framing, transport) and core alone,
and reads nothing it keeps: records are sealed on the devices (`sync.md`).

## What it keeps

- **SQLite, five tables** (`store.rs`, `V1` in `MIGRATIONS`, the 0.1.0 baseline never edited, and
  the steps after it, those a release shipped pinned in `store/released.rs`, `release.md`): `spaces` (id, the SHA-256 of
  the token, created, touched, bytes, head, snapshot through, and the incarnation the
  `EVERY_SPACE_CARRIES_AN_INCARNATION` step added, below), `records` (space, seq, device,
  blob, and the blob's SHA-256 `digest` the second step `RECORDS_ARE_KNOWN_BY_THEIR_DIGEST` added,
  NULL for a record stored before it), `snapshots` (space, part, blob), and the
  `A_COMPACTION_MAY_BE_STAGED_IN_PARTS` step's `stagings` (space, id, touched, bytes, parts) and
  `staged` (space, part, blob). Every store call runs on `spawn_blocking` behind one lock.
- **A record a device sends again is kept once.** `Store::append` passes over a blob whose digest
  that device already has stored in the space (and a repeat inside one append), counting neither its
  bytes nor a seq, and the append is answered `Appended` as if stored: a device resends the very
  bytes of an append whose `Appended` it lost (`sync.md`), and only a resend can match, since every
  seal takes a fresh nonce (`a_record_sent_again_by_the_device_that_sent_it_is_kept_once`,
  `a_store_of_the_first_release_is_carried_forward_with_its_records`). **An append costs the lock
  time linear in its blobs, at most `BLOBS_A_FRAME_AT_MOST`** (the decoder's bound, `sync.md`): the
  repeats inside it are told apart by a set of digests, the caps are weighed first against what no
  lookup could take away (a resend cannot be more than the space already holds, so the frame's
  distinct bytes less the space's are added for certain), and only then is each digest looked up by
  the index, once (`many_repeats_inside_one_append_are_kept_once_each`,
  `an_append_no_lookup_could_make_fit_is_refused_and_a_resend_within_the_cap_is_not`).
- **The first token claims a space; any other is refused** (`Claim::Refused` → `Refusal::Token`).
  The digest is compared as stored: a digest of a 32-byte random token leaks nothing by timing.
  **A space whose id is marked as committed is made only by the token it commits to**
  (`SpaceId::may_be_made_by`, checked in `Store::claim` before a space is made): a shared list minted
  now names `SpaceId::committed_to` its write token's digest (`shared-playlists.md`), so a view link
  that knows the space cannot claim it first. An unmarked id is claimed first come as before; a
  random one bears the four marker bytes once in 2^32, and only a space not yet made is checked
  (`a_space_committed_to_a_write_token_is_made_by_that_token_alone_and_held_by_it_after`,
  `a_view_link_cannot_claim_a_shared_list_before_its_owner_does`).
- **A compaction replaces what it covers and only moves forward**: `through` must be past the
  snapshot held and within the head (`Compacted::Stale` otherwise); records at or below it and the
  old snapshot go in one transaction. The server cannot read what a snapshot leaves out, so the
  compacting device carries every change it could not read into the parts it sends (`sync.md`'s
  `Ledger::snapshot_bundles`); the store keeps the parts as they come. **A compaction that grows the space is weighed against
  `--total-bytes` as an append is** (the snapshot's bytes less the bytes the space held, against
  `totals_in`), so claiming spaces and compacting each to a snapshot near `--space-bytes` cannot
  pass the total; one that shrinks a space is never refused by a full total
  (`a_compaction_that_grows_a_space_is_weighed_against_the_total_cap_as_an_append_is`).
- **A compaction past one frame is staged and swapped in whole** (`sync.md`): `Stage` opens a
  staging for the space under a random `StagingId` the session keeps, putting aside any other the
  space had (`a_later_staging_of_the_space_puts_the_earlier_aside`), and is answered `Staging`;
  `Staged` appends parts to it; `CompactStaged` moves the staged parts and its own into `snapshots`
  in the one transaction `Compact` uses, with the same checks (`Store::compact_staged`), and a
  `CompactStaged` with no staging of the session's is `Stale`
  (`a_compaction_staged_in_parts_replaces_the_log_only_once_its_last_part_is_taken`). Until then
  catch-up reads the old log alone, so a session ending part way leaves it whole: the session's
  staging is dropped as it ends (`Store::unstage`), and one left by a crash goes once untouched for
  `A_STAGING_UNTOUCHED_IS_KEPT_FOR` (an hour, `Store::sweep_staged` on the sweep's round)
  (`a_staging_left_unfinished_leaves_the_log_whole_and_holds_nothing_once_unstaged_or_swept`,
  `a_device_leaving_part_way_through_a_compaction_leaves_the_old_log_whole`). **What is staged is
  counted toward `--total-bytes`** (`totals_in` adds `stagings.bytes`), so stagings cannot hold
  bytes past the total, and a staging's own bytes are weighed against `--space-bytes`, since the
  snapshot alone must fit the space; one past either is put aside and refused `SpaceFull`
  (`what_is_staged_is_weighed_against_the_space_and_the_total_cap_and_put_aside_past_either`). A
  space may so hold its log and a staging beside it, each within `--space-bytes`, for as long as the
  compaction takes. A `Compact` in one frame is taken as before
  (`a_compaction_in_one_frame_from_an_older_device_still_replaces_the_log`), and a `Compact` through
  0 with no parts, which is how `Stage` is written, is what a server before staging refuses `Stale`
  (`the_compaction_through_nothing_a_staging_is_asked_by_is_refused_as_stale_by_the_one_frame_form`).
- **A space untouched for `--idle-days` (365) is swept** (checked hourly, or every thirtieth of the
  limit if shorter, `sweep_wait`). **The limit is at least a day**: `Limits::idle_days` is a
  `NonZeroU64`, so clap refuses `--idle-days 0` before the store is opened and no caller of the
  library can make the sweeper spin or sweep what was touched this second
  (`a_retention_of_zero_days_is_refused_before_the_store_is_opened`,
  `a_sweep_waits_a_thirtieth_of_the_idle_limit_and_never_more_than_an_hour`). **A space never
  written to is swept once untouched for a day** (`Store::sweep_unused`: `head` and
  `snapshot_through` both 0), so claims made by a script and abandoned do not pile up.
- **Touched means used**: a claim, a view and `Heads` touch a space, and so does every write that
  persists (`Store::append`, a compaction whole or staged, a staged part, `Store::close`, each taking
  the time as `now`), so a device that stays connected and keeps writing is never swept
  (`every_write_to_a_space_keeps_it_past_the_idle_limit_and_an_idle_one_is_still_swept`). **A space
  a session holds is never swept, empty or not**: each sweep round first touches every space the
  hub has a device in (`Hub::spaces_held`, `Store::touch`), so neither the idle nor the unused pass
  takes a space out from under a live session, and a device connected to a space it has not written
  yet keeps it; a script holding an empty space so costs a connection held open, which the
  per-address limits bound, where one record would keep it for `--idle-days` anyway
  (`a_space_a_session_holds_is_kept_by_both_sweeps_and_swept_once_let_go`). Where that touch fails
  the round sweeps no space.
- **A sweep round runs as the server opens, then after every wait** (`Server::sweep_once`), so a
  staging a crash left is reclaimed at the restart rather than an hour on
  (`a_staging_an_earlier_run_left_behind_is_reclaimed_as_the_server_opens`). **Every pass reports
  its own outcome** (`Pass`: `Held`, `Staged`, `Unused`, `Idle`): what it took away is an info
  line, a failure a warning naming the pass, so capacity charged to a stale staging is never held
  without a word (`every_pass_of_a_sweep_that_fails_is_reported_by_name`).
- **A space made again is another incarnation.** `Store::claim` makes every space with SQLite's
  `random()` as its `incarnation`, and the step gave each space it found one of its own the same
  way, its records kept (`a_store_of_before_incarnations_gives_each_space_one_of_its_own_and_keeps_its_records`);
  a held space keeps it, one swept and made again draws another
  (`a_space_swept_and_made_again_carries_another_incarnation_and_a_held_one_keeps_its_own`). The
  server only tells it (`Store::incarnation`, in the `Welcome` of a `Hello`, `Share` or `View` said
  at `INCARNATION_VERSION`); a device, or a shared list edited or viewed, compares it with the one
  it kept (`sync.md`, `shared-playlists.md`).

## Sessions

- **A session's read-ahead is a budget in bytes, not a count of frames** (`reading.rs`): the reader
  waits for room in a `READ_AHEAD_BYTES_AT_MOST` (1 MiB) budget before it reads a frame, then
  weighs the frame (`A_FRAME_WEIGHS_AT_LEAST` 64 bytes, at most the whole budget) and hands it over
  with the permit, which is held until the session has handled the frame. A frame past the budget
  (an `Append` may be up to `LARGEST_FRAME`) takes all of it, so nothing more is read while it is
  handled; a session waiting on a write to a client that reads nothing holds one frame plus one
  being read, not a count of 32 MiB frames, and QUIC's flow control holds the client back
  (`a_frame_larger_than_the_budget_takes_all_of_it_and_nothing_is_read_until_it_is_handled`).
- **Every session's frames share one byte budget too** (`reading::FrameBytes`, `--frame-bytes`,
  256 MiB), through `resonate_sync::Holds` (`Incoming::read_heard`): on QUIC the announced length is
  held from it before the body is allocated (`read_frame_held`); on a web socket every byte is drawn
  from it as it comes off the socket (the `Budgeted` stream under tungstenite, `Holds::draw`), so a
  message still arriving holds what has arrived, and once it is whole its bytes as framed go with it
  and what was read past it is carried to the next, at most one read (`WEB_READ_AT_ONCE`, 16 KiB,
  the accepting side's read buffer). The hold is taken by `try_acquire_many_owned`, never waited for, and is kept with
  the session's permit until the frame is handled. A frame that finds no room, or a web socket's
  read that finds none, is `Error::NoRoom` and the session is refused `Busy`, so 2 048 connections
  each sending `LARGEST_FRAME` hold no more than the budget, not tens of GiB
  (`frames_announced_across_sessions_past_what_the_server_holds_are_refused_as_busy`,
  `a_message_still_arriving_on_one_web_socket_holds_its_bytes_from_every_other_session`,
  `the_bytes_of_a_message_not_yet_whole_are_held_from_the_budget_as_they_arrive`). **A web socket's
  every message counts**: a ping, pong or text message is drawn like any other, given back once
  read and handed to the session as `Read::PassedOver`, which counts toward `--frames-per-minute`
  (`a_ping_counts_toward_the_frames_a_web_socket_may_send_in_a_minute`). What the web path holds
  beyond the budget is tungstenite's read buffer, one read a connection.
- **A QUIC connection to the server carries one stream and nothing beside it**
  (`resonate_sync::server_transport`, the server's alone; devices dial with their own): one
  bidirectional stream at a time, no unidirectional stream, no datagrams, and a receive window of
  `A_SERVER_CONNECTION_BUFFERS_AT_MOST` (1.25 MB, quinn's own stream window) for the stream and the
  whole connection alike, so a client cannot open streams the server never accepts and fill each
  one's window with bytes quinn holds and the frame budget never weighs; 2 048 connections hold at
  most that much each
  (`a_client_may_open_no_stream_and_send_no_datagram_beside_the_one_its_session_reads`).
- **What a QUIC session said leaves before its connection goes**: dropping a `quinn::Connection`
  closes it at once and discards what its streams had not sent, so `welcome` waits up to
  `WHAT_WAS_SAID_LEAVES_WITHIN` (1 s) for the client to close first, and a `Refused` reaches it.
- **A session's reader lives no longer than its session**: `read_ahead` hands back a
  `ReadingAhead`, which aborts the reading task as it drops, so a session that ends on an error (a
  failed store write, a failed send) stops its reader at once and gives back the frame bytes it
  held, rather than leaving it to read on until the connection closes
  (`a_reader_let_go_stops_and_gives_back_the_bytes_its_frame_held`,
  `a_store_write_that_fails_ends_the_session_and_gives_back_the_bytes_its_reader_held`).
- **A session or a room's member ends with what its client still sends read out**
  (`finished_with_nothing_unread`): the server finishes its side, then keeps taking the read-ahead's
  frames until the client closes, within `WHAT_WAS_SAID_LEAVES_WITHIN`, before it lets the reader go.
  A web socket closed with bytes unread (a guest's ping landing after the last frame, say) resets the
  connection, and Windows and macOS drop what the client had not yet read on a reset, so a guest
  turned out lost its `RoomEnded`, read the reset as a connection lost and joined again, where Linux
  still hands the frame over
  (`a_party_tells_its_guests_the_song_and_their_names_and_ends_with_its_host_turning_one_out`).
- **A `Hello` within 10 s or nothing**, the allowance (`Limits::hello_within`) one deadline taken when the connection is accepted and shared by every stage up to the `Hello`: a web socket's request line, upgrade and first frame, a QUIC stream's opening and its first frame
  (`the_upgrade_and_the_hello_share_one_allowance_so_a_slow_client_does_not_get_two`); a wrong version, token or a full space is a `Refused` frame,
  then the stream ends. A `Hello`, `Share` or `View` is taken at `PROTOCOL_VERSION` (answered with
  the `Welcome` the first build reads) or `INCARNATION_VERSION` (its `Welcome` carrying the space's
  incarnation after the head)
  (`a_view_of_the_first_version_is_welcomed_as_its_build_reads_and_only_the_incarnations_version_is_told_one`);
  `Heads` and `Room` at `PROTOCOL_VERSION` alone. **The first frame is read to at most `FIRST_FRAME_AT_MOST`** (resonate-sync's
  `wire`: a full `Heads` plus an envelope, about 12 KiB) and a room's frames to `ROOM_FRAME_AT_MOST`
  (`LARGEST_ROOM_FRAME` plus an envelope), the first through `reading::read_the_first` and a room's
  through the read-ahead; a longer one ends the session before its bytes are held. **The first frame
  draws from the frame budget like every other** (`Incoming::read_heard`): its announced length or
  the bytes arriving are held until it is decoded, and one finding no room is refused `Busy`, so
  2 048 half-greeted sessions hold no more than the budget
  (`first_frames_still_arriving_hold_their_bytes_from_every_other_session`). On QUIC the announced length is weighed before anything is
  allocated; on a web socket the transport under tungstenite is a `Budgeted` stream that fails its
  next read once the bytes allowed for this frame are spent, since tungstenite's own message limit is
  fixed at `LARGEST_FRAME` for the life of the socket
  (`a_first_frame_announced_past_the_ceiling_ends_the_connection_without_waiting_for_it`,
  `a_budgeted_stream_stops_reading_once_its_budget_is_spent`).
- **Catch-up is snapshot then records past the cursor**, the snapshot in frames of at most
  `SNAPSHOT_BYTES_A_FRAME` (half a frame) and `BLOBS_A_FRAME_AT_MOST` parts, every one but the last
  naming the `through` the session last sent and the last the snapshot's own (`sync.md`); a
  `Reading` remembers the part to go on from and the snapshot it belongs to, so a compaction landing
  between frames starts the newer snapshot from its first part
  (`a_snapshot_past_a_frame_is_read_in_frames_and_begun_again_when_another_replaces_it_between`,
  `a_state_past_a_frames_share_is_compacted_in_parts_and_a_device_joining_reads_it_in_frames`); the
  records in frames of at most 256 records or 4 MiB;
  a device's own records go back as seqs with no blob. Each frame is read through
  `Store::catch_up`, which under one lock answers the snapshot wherever what was sent lies behind the
  snapshot held, so a compaction landing between frames, or while a session is behind on its
  notices, never skips the records it replaced; `Compacted` is told only after the session has
  caught up (`a_reader_behind_a_compaction_is_handed_the_snapshot_rather_than_what_is_left`). After it, every `Append` anywhere in the
  space tells each session to send what lies past what it last sent, so ordering never depends on
  which notice arrived first.
- **Presence is the connections themselves** (`hub.rs`): joining tells the others `Arrived`,
  leaving `Left`; a device connecting again replaces its old session; at most `--devices` (16) at
  once.
- **Every notice queued for a session is charged to its `backlog`** (`hub::notice_weight`: the bytes
  of a `Closed`, `Punched` or `Relayed`, at least `A_NOTICE_WEIGHS_AT_LEAST` for the rest) and
  credited back as the session takes it off its channel. A `Punched` or `Relayed` that would take the
  backlog past `RELAY_BACKLOG_BYTES_AT_MOST` is dropped; any other notice that would take it past
  `NOTICE_BACKLOG_BYTES_AT_MOST` (1 MiB) is dropped and the session is queued one `FellBehind`, which
  ends it, so the device reconnects and catches up from its cursor rather than the server holding
  what it will not read (`a_device_that_reads_nothing_is_told_it_fell_behind_rather_than_queued_everything`).
  A punch's `sealed` was already cut at `LONGEST_TEXT` by the decoder.
- **A punch goes only to a device online in the same space**, carrying the asker's observed address
  and its sealed candidates. A device on a web socket has no observed UDP address: the server
  files it as `0.0.0.0:0`, the asked device skips an unspecified address, and the candidates (its
  interfaces and the address STUN gave it) are all it dials (`sync.md`).

## Shared spaces (`shared-playlists.md`)

A shared playlist is one more space, its token a write capability, so nothing here reads what it
holds. The `A_SPACE_MAY_BE_SHARED_AND_CLOSED` step adds two columns.

- **`Share` is `Hello` for a shared space**: it carries the digest of a viewer token as well, kept
  once (`Store::allow_viewers`), and a new space it makes spends
  `--shared-spaces-per-address-per-day` (50), not the 5 a personal space does, so a reset behind one
  NAT cannot fail halfway through its work. **A space with a viewer is a shared space and a `Hello`
  to it is refused `Token`** (`Store::is_shared`, no stored mark of its own)
  (`a_space_made_by_a_share_is_never_answered_to_a_hello_so_the_two_budgets_stay_apart`). **A
  `Share` session does only what a shared list asks** (`SpaceKind::Shared`): it reads, appends,
  compacts (whole or staged), closes and says `Bye`, and nothing of a device's own space; its
  `Welcome` names nobody online, it is told no `Arrived` or `Left`, and a `Relaying`, `Relay` or
  `Punch` is refused `Shared` and ends the session. Personal sync rests on devices meeting
  (presence, punches, the relay that carries songs between them), so a script naming a viewer
  token it never uses cannot spend the 50 a day on personal spaces past the 5
  (`a_share_does_only_what_a_shared_list_needs_so_it_cannot_stand_in_for_a_personal_space`).
- **`View` is one read and no seat**: the viewer token opens `Welcome`, the snapshot and records and
  a `Closed` if there is one, then the connection ends. It joins no hub, so followers do not count
  against `--devices`, and appending or compacting is not a thing it can ask. It touches the space,
  so a list only followed is not swept.
- **`Heads` asks after many spaces on one connection** (at most `HEADS_ASKED_AT_MOST`, 256): each
  space whose token or viewer token fits answers its head and whether it is closed, and any other is
  left out, so a client polls with one connection and opens a session only for what moved.
- **`Close` is first come, kept apart from the log** (`spaces.closed`), **and weighed as an append
  is**: its bytes join `spaces.bytes`, against `--space-bytes` and `--total-bytes`, and a word that
  would pass either is refused `SpaceFull` (`Closing::Full`), so closing 10 000 spaces cannot hold
  gigabytes past the caps; the `A_CLOSING_WORD_IS_COUNTED_IN_ITS_SPACE` step counts the words a store
  kept before
  (`a_closing_word_is_weighed_against_the_space_and_total_caps_and_counted_once_kept`,
  `a_closing_word_kept_before_it_was_counted_is_counted_once_the_store_is_opened`). Compaction cannot reach it
  (a member being removed could otherwise compact the word that moves everyone away), a second one
  is answered to its sender alone with the first (`Store::close` answers `Closing::Now` or
  `Closing::Before`, and only `Now` is told to every session, so the stored blob is sent once, not
  once per repeated `Close`), `Append` and `Compact` then answer `Closed`, and every view is told it.

## Listening parties (`rooms.rs`, `party.md`)

A room lives in memory only: nothing is stored, and a restart ends every party. **The first frame
`Room` either makes one or joins one**: a host token makes the room and only that token is the host
from then on, and a guest finds `NoSuchRoom` until it exists. A room id marked as committed is made
only by the host token it commits to, else `Token`, so a guest cannot make a room whose host left past
its grace (`party.md`). A guest naming the host's `DeviceId` (which every guest is told in `RoomWelcome`) is refused `Token`, so the host's seat is never replaced by anyone without the host token. **A guest's place is bound to a secret of its own** the same way: `Room` carries an optional `seat` token after `member` (absent from a build that predates it, so its frame is byte for byte the old one), the room keeps the digest of the first seat a `member` joined with for its whole life (at most `SEATS_BOUND_AT_MOST`, 1024, past which a new place is `Busy`), and any later join under that id with another seat, or with none, is refused `Token`; the same seat again is the guest coming back and replaces its stale session. A guest from a build without seats joins as before, and is held to what that allows: **while its session is live no second join under its id, seated or not, replaces it** (refused `Busy`, not `Token`, since the old client reads `Token` as turned away for good and `Busy` as a connection lost, tried again, so its own reconnect over a stale session the server has not yet noticed succeeds once that session is gone), but **once it has left its id is takeable**, by a thief as much as by the old build coming back, since nothing it holds tells them apart (`a_guest_without_a_secret_cannot_be_replaced_while_its_session_is_live`, `a_guest_without_a_secret_keeps_its_place_while_its_session_lives_and_gets_it_back_after`, `a_guest_from_a_build_without_secrets_still_joins_and_comes_back`). A seated guest's id is bound for the room's life, so no seatless join ever takes it (`no_guest_can_take_another_guests_place_by_joining_under_its_id`). A server that predates the field refuses the longer frame as malformed, which the client reads as `ServerTooOld`. **The server enforces the star, so
nobody can forge the host**: `RoomSay` from the host reaches everyone (or the one member it names),
from a guest it reaches the host alone, whatever it names. `RoomKick` and a host's `Close` are the
host's; a guest asking changes nothing. **A host gone is waited for** (`Limits::host_back_within`,
30 s, `Leaving::TheHostLeft`) and then the room ends. Each wait belongs to one absence: the leaving
hands back an `Absence` naming the host session that left, the room keeps the one it is in, and
`end_unless_the_host_came_back` ends it only while that absence still stands, so the wait of a drop
the host already came back from does nothing during a later one
(`a_host_dropping_twice_is_waited_for_from_its_second_drop_not_its_first`). Each member's frames count against
`--frames-per-minute`, a member may have at most `Limits::room_bytes_per_second` (512 KiB) said, what
exceeds it is dropped, and a member who reads nothing holds at most `ROOM_BACKLOG_BYTES_AT_MOST`
(1 MiB). `--rooms` (1024), `--room-members` (32) and `--rooms-per-address-per-day` (50) bound the
rest. **`RoomPing` is answered with this server's monotonic clock**, nanoseconds since it began, so a
party's clocks are one clock whatever each machine's says.

## Limits

`--record-bytes` (`LARGEST_RECORD`, 256 KiB a blob, `TooLarge`; devices send nothing larger), `--space-bytes` (64 MiB a space, `SpaceFull`),
`--frames-per-minute` (240 a connection, `TooFast` and the session ends), `--frame-bytes` (256 MiB
held by frames being read across every session, `Busy`). Errors are typed
(`errors.md`); a session's failure is a debug line, never the server's.

There is no account or e-mail to tie a space to, so anyone can claim spaces; the limits are what
keeps a script from filling the store (`guard.rs`, `Store::limited`):

- **Per address** (an IPv6 `/64` counts as one, an IPv4-mapped IPv6 address as its IPv4):
  `--connections-per-address` (64 open at once), `--attempts-per-minute` (30 connections a minute,
  however briefly each lasts) and `--spaces-per-address-per-day` (5 new spaces, refused `TooFast`;
  a connection to a space that exists is never counted). A connection over either is dropped
  before a byte is read.
- **Per prefix, beside the address**: the connections open, the attempts a minute and the three
  daily allowances (spaces, shared spaces, rooms) are also counted by every wider prefix an address
  sits in, an IPv6 `/56` and an IPv4 `/24` four times the address's (`A_SITE_SPENDS_AS_ADDRESSES`),
  an IPv6 `/48` eight times (`A_NETWORK_SPENDS_AS_ADDRESSES`), so by default a `/56` or `/24` holds
  256 connections and a `/48` 512, never all of `--connections`. A connection or a making is allowed
  only where the address and every prefix have room, counted against all of them or none, so one
  routed prefix or a handful of neighbouring IPv4 addresses cannot fill `--connections`, `--spaces`
  or `--rooms` (a space holding one record outlives the unused sweep for `--idle-days`)
  (`the_sixty_fours_of_one_fifty_six_and_one_forty_eight_hold_only_so_many_connections_together`,
  `the_addresses_of_one_twenty_four_hold_only_so_many_connections_and_get_them_back`,
  `the_addresses_of_one_twenty_four_are_held_to_so_many_attempts_a_minute_together`,
  `a_connection_a_wider_prefix_refuses_is_not_counted_against_the_address`,
  `the_sixty_fours_of_one_fifty_six_and_one_forty_eight_share_an_allowance_of_their_own`,
  `the_addresses_of_one_twenty_four_share_an_allowance_of_their_own`,
  `what_a_wider_prefix_refuses_is_not_spent_from_the_address`). A prefix is kept from its first
  connection, forgotten a day after it was last seen unless it holds one open, and the table holds
  twice as many as the address table, letting the least recently seen holding nothing open go a
  hundredth at a time
  (`a_full_table_of_prefixes_lets_go_of_those_holding_nothing_open_and_keeps_the_rest`).
- **Everyone together**: `--connections` (2048 open), `--spaces` (10 000, refused `Busy`) and
  `--total-bytes` (16 GiB over all spaces, an append past it `SpaceFull`). The per-space limits
  above still hold under them.
- **QUIC proves the address first**: an `Incoming` whose address is not validated is sent a retry,
  so a spoofed source cannot spend an address's allowance or the server's connections.
- **A web socket's address is the proxy's header where one is trusted** (below), else the peer's.
  The address table holds at most 100 000 addresses and forgets a quiet one after a day; when it
  is full a new address evicts the least recently seen one holding no connection (a hundredth of the
  table at a time, so the scan is amortised), and is turned away only if every address holds one;
  the counters live in memory, so a restart forgets them.

## The hifi-api gateway (`keys.rs`, `gateway.rs`)

`--config <TOML>` with a `[hifi]` table (`listen`, `upstream` as socket addresses, `keys` as SHA-256
hex digests) opens an HTTPS gateway on TCP, beside QUIC on UDP (same certificate, ALPN `http/1.1`),
in front of a hifi-api the operator runs on `upstream` (plain HTTP, expected on loopback). No table
leaves it off; a table with no key, or a key that is not a digest, is a startup error
(`ConfigMissing`/`ConfigInvalid`). `resonate-server --hifi-key` mints a secret (32 random bytes,
hex) and prints it with the digest to list: the file never holds a secret.

- **Everyone without a listed key is turned away alike**: no `Authorization: Bearer`, another scheme
  or an unlisted secret is 401 before method or route is looked at, so a stranger learns nothing of
  the routes (`a_request_without_a_secret_or_with_one_not_listed_is_turned_away_and_never_forwarded`).
- **A listed key reaches three routes**: `GET /search/`, `GET`/`DELETE
  /playback/requests/<letters, digits, dashes>` (forwarded to the hifi-api) and `GET /file/?id=<digits>`;
  else 404, another method 405. `/track/` is not one: the manifest names TIDAL's CDN, which a client
  never reaches itself (`a_listed_secret_reaches_only_the_hifi_routes_and_methods`).
- **The secret stops at the gateway**: a forwarded request goes upstream as HTTP/1.0 with the target
  and a `Host`, no `Authorization`, no client header; the answer's status, `Content-Type`, digit-only
  `Retry-After` and body (at most 1 MiB, so no chunking to read) go back. An upstream that is down,
  silent for 30 s or unreadable is 502. Each connection is one request (`Connection: close`).
- **A seat is taken only by a request that holds a listed key.** At most 64 requests are served at
  once (`Crowd::seats`), taken with `try_acquire_owned` after the key check, so a stranger can never hold
  one and a key holder finding them all taken is answered `503` with `Retry-After`. Before the key
  is read a connection is a stranger, held in a pool of its own (`Crowd::strangers`, 256, taken
  with `try_acquire_owned` before a task is spawned, so the accept loop never waits and a connection past
  it is dropped unread), counted per address (`Crowd::strangers_per_address`, 16, an IPv6 `/64` as
  one, through `Guard`), and given one `REQUEST_WITHIN` (10 s) over the TLS handshake and the head
  together. A loopback peer is not counted per address, since behind the tunnel every peer is
  `cloudflared` and strangers cannot be told apart there; the pool alone bounds it
  (`strangers_who_say_nothing_hold_no_seat_a_listed_key_is_served_from`,
  `one_address_waits_on_the_gateway_for_no_more_than_its_share_and_gets_it_back_on_leaving`).
- **The server downloads, the client is only sent the file.** `/file/` runs `HifiApi::track` against
  the upstream (manifest, DASH segments and remux are `resonate-tidal`'s, `providers.md`), then sends
  the FLAC with `Content-Length` where known, else chunked, 64 KiB at a time over a channel of four
  so a slow client holds the download back. **One download at a time** (`DOWNLOADS_AT_ONCE`): a
  request finding the turn taken is answered `503` with `Retry-After` (`BUSY_RETRY_AFTER_SECONDS`) at
  once, never held, and the turn travels into the blocking open and then the pump, so it is kept until
  the last upstream read ends even if the client has left, and is released before the pump drops
  its end of the channel, so a client that has read a whole answer and asks again never finds the
  turn still taken (`pump_source` owns the source; `an_empty_answer_for_a_track_is_never_kept`)
  (`a_second_download_asked_for_while_one_runs_is_told_to_come_back_and_is_never_started`). At most 1 GiB
  a track, 60 s for any one write. A failure after the head is sent drops the connection, so a short
  body is never taken for a whole file; a track the hifi-api lacks is 404, an upstream failure 502. The
  key check comes first: nothing is asked of the hifi-api for a request without a listed key
  (`a_track_is_never_downloaded_for_a_request_without_a_listed_key`,
  `a_track_is_downloaded_by_the_server_through_the_hifi_api_and_sent_to_a_listed_key`).

## The relay (`hub.rs`, `relay.rs`)

A device that sent `Relaying` can be handed `Relayed` frames: `Hub::relay` passes a `Relay`'s datagram
(at most `LARGEST_DATAGRAM`) from one device of a space to another of the same space, nothing else,
and keeps nothing: the datagram is QUIC between two devices, sealed by their handshake
(`sync.md`). It is not a record, so it does not count toward `--frames-per-minute`; instead:

- **A device may have `--relay-bytes-per-second` (2 MiB) relayed**, a token bucket of one second's
  worth (`relay::Bucket`); what exceeds it is dropped, not refused, so the session lives and QUIC
  slows itself. Every datagram weighs at least `A_RELAYED_DATAGRAM_WEIGHS_AT_LEAST` (64 bytes, `hub::relayed_weight`) in the bucket and in the target's backlog, so a flood of empty datagrams spends both. `Relaying` is an ordinary frame and counts toward `--frames-per-minute`. `0` relays nothing and ignores `Relaying`.
- **A target holds at most `RELAY_BACKLOG_BYTES_AT_MOST` (256 KiB) unread**; the rest is dropped, so
  a slow reader costs the server a bounded queue. A `Relaying` from a session since replaced does not
  count (`a_relay_declared_by_a_session_that_was_replaced_does_not_count`).

## What the gateway keeps (`cache.rs`, `searches.rs`, `ranges.rs`)

Two things are kept so a repeated ask costs the hifi-api and TIDAL nothing. Neither changes who is
admitted: the key check comes first, so a stranger reads nothing from either.

- **Tracks, on disk, under a byte limit** (`--cache <DIR>` with the hifi table, `--cache-bytes`,
  2 GiB; 0 or no `--cache` keeps none; the unit passes its `CacheDirectory`). A `/file/` is looked
  up before it takes a download turn and again once it holds it (a download that finished meanwhile is
  served, not repeated). A miss is **teed into a staging file while it is sent**: `<id>.<pid>.part`
  held under `writer::Held`, renamed to `<id>.flac` only after a clean end of the stream (a known
  `Content-Length` must match, and the first bytes must be `fLaC`, however few each read hands over,
  so an empty body is never kept either, and a first read of one to three bytes still is
  (`a_track_whose_first_read_is_shorter_than_its_marker_is_still_kept`)), so a short or foreign body is never kept (`an_empty_answer_for_a_track_is_never_kept`). Eviction is least recently used (a hit bumps the file's mtime, which orders a
  restart); a restart sweeps stale stagings through `writer_of`, leaves foreign files alone and
  trims a lowered limit. **The directory never exceeds the limit**: each chunk reserves its bytes
  before it is written, evicting as needed, and a track may take at most half the limit (a known
  length over that is not staged at all). One id is staged by one download at a time. The download
  turn travels with the staging into the blocking task, so **a client that leaves does not stop a
  fill** (its resume then finds the file) and `DOWNLOADS_AT_ONCE` still bounds the work
  (`a_track_asked_for_twice_is_downloaded_once_and_the_second_answer_comes_from_the_cache`,
  `a_track_over_the_limit_is_sent_and_never_kept`).
- **`Range` on `/file/`**: one `bytes=N-`, `N-M` or `-S` is answered `206` with `Content-Range` and
  `Accept-Ranges: bytes`, from the cache by a seek, from a download by reading past what comes
  before (the whole track is still staged). A range past the end is `416` with `bytes */total`;
  several ranges, a malformed one, or any range of a track whose length the upstream did not give is
  answered whole, which `resonate_fetch::Ranged` reads on (`ranges.rs` holds the grammar).
- **Searches, in memory** (`--search-minutes`, 10; 0 keeps none): a `200` to a `GET /search/` is
  remembered by its target, at most 8 MiB in all, the oldest first out. Only searches: a
  `/playback/requests/` answer is a queue's state and is never kept, nor is a non-200.

## The web socket listener (`web.rs`, `serve_web`)

QUIC cannot cross a proxy that carries only HTTP, so `--config` with a `[web]` table (`listen`, and
optionally `forwarded_by`, a header name) also accepts the same frames over WebSocket on TCP, one
binary message a frame (`resonate-sync`'s `Outgoing`/`Incoming`, which hide which of the two a
session runs on). The sessions, limits, store and hub are the same code for both. No table leaves
it off.

- **Plain TCP, no TLS of its own**: it sits behind Cloudflare Tunnel (`cloudflared` connects out
  to it, so no inbound port opens and the origin address is never in DNS) or another proxy that
  terminates TLS. Bind it to loopback. Devices use `sync-server = "wss://sync.example.org/"`.
- **`forwarded_by = "CF-Connecting-IP"`** makes the address the allowance counts the first one in
  that header, and a connection without a parseable one is dropped, so only the proxy's path works.
  Left out, the peer address is counted, which behind a proxy is the proxy itself: set it. **The
  header is trusted only from a peer on this machine** (`header_trusted_from`: a loopback address,
  an IPv4 one mapped into IPv6 read as itself), where `cloudflared` connects from; a peer elsewhere,
  which reached a listener bound beyond loopback around the proxy, is counted by its own address and
  its header ignored, so no client chooses the address its limits count, and a `listen` beyond
  loopback with `forwarded_by` is warned of at start
  (`the_proxys_header_is_trusted_only_from_a_peer_on_this_machine`).
- **A plain `GET` of `/s/` or `/p/` is a page, not a socket** (`pages.rs`): `serve_web` peeks at the
  stream before tungstenite, which would refuse it, and answers a static page that hands its URL
  fragment to `resonate://`. The fragment never reaches here, so the page holds nothing of an
  invitation. **What was asked is read out before the connection closes** (`read_out`, until the
  asker closes or `A_PAGE_ASKER_LEAVES_WITHIN`, 2 s): the request was only peeked at, and a socket
  closed with bytes unread resets the connection, which a Mac or an iPhone reads as no page at all
  where Linux still hands over what arrived. `/.well-known/assetlinks.json` answers only where `[web] android_certificates` names
  signing fingerprints, else 404, so no build claims links it cannot verify. The script on the page
  takes the fragment up to its first `?`, `&` or `#` and keeps only letters, digits and hyphens, so a
  link written as `/s/#CODE?server=elsewhere` hands the app the code and the server the page was
  served from, never another (`a_page_hands_the_app_the_code_alone_and_never_a_query_of_the_fragment`). Both sides of a socket
  leave Nagle's algorithm (`set_nodelay`), since a party's clock is read over it
  (`a_page_is_answered_beside_the_socket_and_holds_nothing_of_the_invitation`).
- **A shared playlist's page reads the list in the browser** (`shared_list.js`, `shared_list.html`,
  `shared_reading.html` beside `pages.rs`): the script derives the space, read token and sealer
  from the fragment's code with WebCrypto (both key forms, `shared-playlists.md`), opens a web
  socket to the host it was served from, sends one `View` with a random device, opens each record
  with a ChaCha20-Poly1305 written in the script (WebCrypto has none), inflates it with
  `DecompressionStream("deflate-raw")`, folds the ops as `SharedList` does (stamps last writer wins,
  removals final, `ENTRIES_KEPT_AT_MOST` and `ROWS_AT_MOST`, which `shared_reading` writes into the
  script beside `PROTOCOL_VERSION` and `LARGEST_BUNDLE_OPENED`) and draws the name and songs as text.
  It settles when the server ends the view, so a `Closed` after the records reads as a list given a
  new link, and a `Refused` as one not held. The code never leaves the page: the server sees a view
  of a space, as from any follower. **A page runs only the scripts its policy names by hash**: the
  `Content-Security-Policy` lists the SHA-256 of each inline script (`script_hashes`, worked out once
  a kind from the page with its scripts filled and its words not, so no word can change a hash;
  the JSON block of words is not run), never `'unsafe-inline'`, with `connect-src 'self'` for the
  socket alone and `frame-ancestors 'none'` (and `X-Frame-Options: DENY`; an answer that is no page
  runs `script-src 'none'`), so the read key the
  fragment holds stays out of reach of an HTML sink a later change might add and of a page that
  frames this one (`a_page_runs_only_the_scripts_its_policy_names_by_hash_and_is_framed_nowhere`);
  the page's words are `pages.ftl`'s, handed to the script as a JSON block `in_json` writes with `<`,
  `>` and `&` escaped, so no word closes the script. The tests run the served script under Node
  against a real shared list, skipped without Node
  (`a_browser_given_either_link_opens_the_list_as_it_stands_and_a_link_nobody_shared_is_not_held`,
  `a_browser_given_the_link_of_a_list_shared_anew_is_told_it_moved`,
  `only_a_shared_playlists_page_reads_the_list_and_its_words_cannot_close_the_script`). **A real
  browser enforces the policy in a test**: `firefox_runs_both_pages_under_their_policy_and_shows_the_list_a_real_link_names`
  starts a headless Firefox on a profile of its own and drives it over Marionette (`tests/firefox.rs`,
  `marionette.port` 0 and the port read from the profile's `MarionetteActivePort`), opens the `/s/`
  and `/p/` pages a real view link and party key name on loopback, waits for the list to be drawn and
  the code and the `resonate://` link to be written, and reads the browser console in its chrome
  context (`MOZ_REMOTE_ALLOW_SYSTEM_ACCESS`) for any `Content-Security-Policy` message; skipped
  without `firefox` on the path, and skipped under a snap Firefox (the `snapped` check: a `snap`
  folder in its path or its resolved path, `/snap/` in a wrapper script, or `SNAP` in the
  environment), which cannot read the profile folder and would wait out the port file. Site icons
  stay on in that profile: each page names `<link rel="icon" href="data:,">` and the policy's
  `img-src data:` admits it, so Firefox never asks for `/favicon.ico` and the console holds no
  violation at all (`a_page_names_an_empty_icon_so_no_browser_asks_for_one_the_policy_refuses`).
- At most 256 upgrades are in flight at once and the upgrade and `Hello` share the one 10 s allowance (`Limits::hello_within`).
  The server pings every 30 s and a reader silent for 90 s ends the session.
  `a_listener_behind_a_proxy_turns_away_a_connection_the_proxy_did_not_forward`.

## Deployment

**The hosted server is one hostname behind one tunnel** (`resonate.tecnio.dev`, `resonate_core::RESONATE_HOST`).
`cloudflared` ingress sends `^/(search|playback|file)/` to the gateway's TLS listener
(`https://127.0.0.1:443`, `originServerName` the host, so the Let's Encrypt chain is verified) and
every other path to the web socket listener on loopback, so sync and the hifi gateway share an
address and no inbound port is open. The old `sync.tecnio.dev` name is not served.


`packaging/server/resonate-server.service`: `DynamicUser`, `StateDirectory=resonate-server` (the
store), `CAP_NET_BIND_SERVICE` for UDP 443 and the usual hardening; `resonate-server.env` sets `LISTEN`,
`RUST_LOG` and `SERVER_ARGS` (empty; `--config /etc/resonate-server/config.toml` turns the gateway on). **The log is
coloured only on a terminal**: `logs_in_colour` asks whether standard output is one and whether
`NO_COLOR` is set, so the unit's journal holds plain levels and messages with no escapes
(`what_the_server_logs_to_a_journal_carries_plain_levels_and_no_terminal_escapes`). SIGINT and SIGTERM close the endpoint and wait for it to go idle. The same directory holds the Arch and Fedora recipes that install the binary, the unit and
`resonate-server.env` (`packaging.md`).

### Renewing the certificate

**SIGHUP reads the chain and key again, and a pair that does not hold is refused while the working
one goes on being presented.** `Credentials` (`credentials.rs`) keeps the two paths and one
`Presented`, rustls' `ResolvesServerCert` over an `Arc<CertifiedKey>` behind a lock; both TLS
listeners are built on it (`serving` for QUIC, `gateway_tls` for the hifi gateway; the web socket
listener has no TLS of its own), so every handshake asks it for the pair and a swap reaches the
next handshake on both with no listener rebuilt and no session dropped. `Credentials::reload`
parses the chain (`NoCertificate` when it holds none, `Pem` when a file does not read) and the key,
and pairs them through `CertifiedKey::from_der`, which refuses a key that is not the leaf's or a
leaf that does not parse (`Unpaired`); only a pair that passes replaces the one presented. The
binary's `renewals` task reloads on each SIGHUP and logs the outcome, a refusal at `error`
(`the_next_handshake_after_a_renewal_presents_the_renewed_chain_on_both_listeners`,
`a_renewal_that_does_not_read_or_pair_keeps_the_working_chain_presented`,
`the_server_presents_a_renewed_chain_once_it_is_hung_up_on`). A session resumed from an earlier
ticket presents no chain at all, so a client sees the renewed leaf on its next full handshake.

**The unit hands the pair over itself, never as systemd credentials**, since `LoadCredential=`
copies the files once at start and a reload refreshes them only under systemd 260's
`RefreshOnReload=`, newer than the distributions served (Ubuntu 26.04 has 259).
`hand-over-credentials.sh`, installed as `/usr/lib/resonate-server/hand-over-credentials`, runs as
root (the `+` prefix) as `ExecStartPre=` and as `ExecReload=`, and copies
`/etc/resonate-server/fullchain.pem` and `privkey.pem` (a root-owned Let's Encrypt key a dynamic
user could not read) as `certificate` and `key` into `/run/resonate-server-credentials`, where
`--certificate` and `--key` point. **Root never writes inside a folder the server may change**: a
compromised server could plant a link in its own `RuntimeDirectory=` between a copy and its rename
and turn the hand-over into a root write elsewhere. So the folder is root's, directly under `/run`,
`0750` and of the dynamic user's group (read from the `RuntimeDirectory=` systemd made and gave
that user, `/run/resonate-server`, which the hand-over never writes into), and each file is `0440`,
root's and of that group, so the server reads the pair and can change none of it. The script runs
under `umask 077`, so a file is root's alone until its content is whole and only then opened to the
group; each is staged in the same folder and renamed so the server never reads half a file. On a
reload it then sends SIGHUP to `$MAINPID`; a copy that fails (a file missing) fails the reload and
sends nothing, so the running server keeps its pair. `ExecStopPost=` runs it with `withdraw`, which
removes the folder, so the key leaves `/run` with the server.
`RESONATE_SERVER_CREDENTIALS_FROM`, `RESONATE_SERVER_RUNTIME_DIRECTORY` and
`RESONATE_SERVER_CREDENTIALS_TO` move the three folders, which is how
`the_hand_over_writes_a_folder_of_its_own_the_servers_group_reads_and_cannot_write`,
`a_stopped_server_has_its_handed_over_pair_withdrawn` and
`a_reload_hangs_up_on_the_server_only_once_the_pair_is_handed_over` run it as the test's own user
(owner and group are then that user's; `the_hand_over_narrows_every_file_it_makes_before_writing_any`
holds the umask and that nothing is given away from root).

**After a renewal the operator runs `systemctl reload resonate-server`**, from the ACME client's
deploy hook (certbot's `--deploy-hook 'systemctl reload resonate-server'`, or a script in
`/etc/letsencrypt/renewal-hooks/deploy/`), with `fullchain.pem` and `privkey.pem` in
`/etc/resonate-server/` linked to the client's live copies. A reload never ends the server: a
renewed pair that does not hold is logged and the old one stays in use until the files are put
right and reloaded again. A restart reads the files afresh and fails on such a pair, so a restart
is no way to renew.
