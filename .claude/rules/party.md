---
paths:
  - "crates/resonate-sync/src/party/**"
  - "crates/resonate/src/party.rs"
  - "crates/resonate/src/party_room.rs"
  - "crates/resonate-engine/src/seat.rs"
  - "crates/resonate-ui/src/views/party.rs"
  - "crates/resonate-mobile/src/together.rs"
---

# Listening parties

A party is one room on the sync server, its key passed as a link, QR code or code. The host plays,
and each guest hears the same song at the same moment **from their own copy or a service they already
have**. No sound passes between people; a guest with neither sits that song out.

## What is where

- **`resonate-sync/src/party/`** holds what all three front ends share, in plain numbers since this
  crate may not see the engine: `key` (salt `resonate-party/1`, so the same bytes are a different room
  than anything else), `clock`, `said`, `follow` (pure), `service` (one connection), `seat` (the loop
  that joins them over the `PartySeat` and `PartyFinder` traits, tested with a fake seat on a real
  server in `crates/resonate-server/tests/together.rs`).
- **`resonate_engine::Seat`** is the ordered commands on a `Player`. It sees no party.
- **`crates/resonate/src/party.rs`** (with `party_room.rs`, the window's one party, behind `ui`) and
  **`crates/resonate-mobile/src/together.rs`** each implement `PartySeat` and `PartyFinder` over their
  own catalog, since those crates may not see each other. On the phones a party, like the desktop's,
  refuses where online is off (`Error::OnlineOff`, `party_settings`).
  `resonate_providers::found_exactly` is the strict search both use.

## The room's seats

A guest mints one random seat token for the life of its `Party` and sends it with every `Room`
frame, so the server binds the guest's `DeviceId` to it on the first join and refuses a second guest
who names the same id (`server.md`). A reconnect sends the same token; a new `Party` is a new id and a
new token, so a guest who leaves and comes back through the link is a new member.

## The clock

Each seat pings the room once a second and sets `ServerClock` from the **quickest answer of the last
60 s** (`RoomPong`'s nanos come from the server's monotonic clock, since `now()` is wall seconds). A gap
over 10 s, a suspended machine, forgets the window. Min-RTT removes queueing but not a path's asymmetry,
so the error between two people is bounded by half their round trip, not known.

**Measured**, `RESONATE_PARTY_SERVER=wss://resonate.tecnio.dev/ cargo test -p resonate-server --test
together the_clock_of_two_seats -- --nocapture`: from one machine through the Cloudflare tunnel, a 76 ms
round trip and two clocks 0.13 ms apart on average, 0.5 ms at most, over 45 s. Two real players on one
local PipeWire graph and a loopback server stood 0.02 ms apart, steady, for a whole song (`apart_ms` in
`resonate_sync=trace`). People in different places differ by up to half their round trip, some tens of
milliseconds, which has not been measured: the aim is friends in different places, not two speakers in
one room. A song whose copies differ (another master, leading silence) lines up by time, not waveform.

## Following

`Follow::step` takes what the host says and what this seat hears and answers one `Doing`.

- **A song arrives by `Find`, once per row.** The finder runs on a thread of its own, since a service
  search takes seconds. The catalog is asked by recording, again with a blank album so a compilation's
  copy counts, then by ISRC where tags give one; then `found_exactly` over `PlaysInPlace::search`: folded
  title and artist equal, both lengths known and within 3 s, since `Song` has no ISRC. The current song
  and the next three are found so songs hand on gaplessly; a next song not held ends the run, and the
  queue behind the song playing is kept current as the host edits it (below).
- **The queue behind the song is kept current, not found again on reaching it** (`party/ahead.rs`,
  `PartySeat::keep_ahead`, `Seat::keep_ahead`). While the guest is on the host's row, `Ahead::step` weighs
  the first `SONGS_FOUND_AHEAD` (3) rows of the host's `next` against the host rows standing behind
  the playing row in the guest's own queue; where they differ and that wanted run has not been asked
  for, one finder thread looks for those songs (one search in flight, a run the host has edited since
  is not taken, a run asked for once is not asked again until the host's moves) and `keep_ahead`
  replaces what lies behind the playing row with the held run. The engine's `Seat` keeps the longest
  common prefix by `QueueStamp` where it stands (the next row's gapless preparation is untouched),
  removes the rest and inserts the remainder, and **leaves a song within
  `TOO_NEAR_THE_END_TO_CHANGE_WHAT_FOLLOWS` (2 s) of its end alone** (the row could change under the
  edit), answering `false` so the run is asked for again once the next song plays. The seat's
  own stamps are put in `rows` right after the call, in the look that made it, so `added_here`
  never reads them as songs added here. A song added or moved in the host's queue a few rows ahead is
  thereby in the guest's queue by the time the song before it ends; the full `Find`/`take_up` remains
  for the row the guest is not on. A look that took a song up reads the seat again before
  deciding, so one `take_up` is not followed by a second for the same row
  (`a_guests_queue_is_kept_current_as_the_host_edits_past_the_songs_found_ahead`,
  `a_seat_keeps_the_queue_behind_the_song_heard_changing_only_what_differs_and_not_near_its_end`).
- **`Seat::take_up` is ordered as a zone handover is** (`zones.rs`): `SetRepeat(Off)`, `Resume` at the
  start to read the row's rate, `Resume` at the place, tell the clock, `KeepInStepWith`, `Play`. `Load`
  would keep the guest's `next` rows; a wrong row or rate steps in at once (`engine.rs`) and no clock
  releases after 3 s. A row that will not resume part way sits out until the next song
  (`SittingOut::CannotStartMidSong`).
- **The row is a `QueueStamp`**, the one a `HeardAt` already names; the seat maps its own stamps to the
  host's rows, which it only compares. A guest a moment past or behind the host at a song's end is
  handing on and is left alone for `HANDED_ON_WITHIN`, 2 s, rather than reloaded.
- **A pause here is sitting out**, seen on `PAUSES_SEEN_BEFORE_SITTING_OUT` looks (a Play still landing
  reads as paused for a moment), so bus, media keys and audio focus all do the same; play rejoins where
  the host is. Next and previous on a guest move it off the host's row, which the next look puts back.
- **Drift is corrected only when it lasts**: over `OUT_OF_STEP_PAST` (40 ms) on `SAMPLES_BEFORE_A_NUDGE`
  looks running (1 s), at most every `NUDGES_APART` (5 s). Over `FAR_ENOUGH_TO_SEEK` (2 s), a host that
  jumped, it seeks instead, since the ring holds less than that and the wait would run out.
- **`Command::Nudge` is a ring correction, not a rebind**: a rebind is an audible gap, and on a renderer
  seconds of buffering each time. Fade out, `RingProducer::hold_in_step`, then `keep_in_step` as at a
  handover (`drop_ahead` and `release_at`), fade in. It reads a stamp taken under the hold, since the one
  before it is a whole pull old. Refused while a boundary waits, a splice is pending, without a fade
  (DoP), or on a `holds_seconds_downstream` sink (`a_zone_behind_a_clock_told_from_afar_skips_to_it_with_a_fade_and_carries_on`,
  `a_zone_ahead_of_a_clock_told_from_afar_waits_for_it_and_carries_on`).
- **A renderer is lined up once a song** (`PartyNow::coarse` where the host's sink holds seconds, or the
  guest's): its clock cannot be stepped.

## A guest's own queue

**The saved queue stays the one from before the party, so even a crash resumes it**: `Together::following`
stops `keep_the_queue` for the zone, and on the phones `save_zone` skips it. Handoff reads what is kept,
so it hands on the same queue. `Seat::put_back` restores queue, shuffle and repeat on leaving (an empty queue comes back empty, through the same `Resume`), the room ending or
being turned out. A song added to the queue during a party (from any front end, the CLI or the bus) is
taken back out and asks the host (`PartySeat::added_here`, `PartySaid::Ask`).

## The room

First frame `Room` (`server.md`): a guest's frame reaches only the host and a host's everyone, so nobody
forges the host, who proves itself with a random token that is not in the link. **The party's words are
sealed under its key**, and a typed name, empty by default, travels only in them: `Hello` to the host, who
tells everyone through `People`. `Now` (the song and next rows, playing, position and the server moment it
was true at) is told on any change, on a jump past `HOST_MOVED_PAST`, and every 2 s; a late guest is told
it as it arrives. A reconnecting seat tries from 1 s to 15 s and the room waits 30 s for its host. **A request, a host's turn-out and a dismissal made while a seat reconnects are not lost**: a guest's `Ask` and a host's `Kick` wait in a queue of `SAID_WHILE_AWAY_AT_MOST` (16, the oldest dropped) and are sent right after the next `RoomWelcome` (also one whose send failed), and a dismissal is applied to the shown requests at once, naming the request (`PartyWantingASong`: its sender and row), never an index, so a *Play next* whose search took seconds removes the request it found and no other (`a_guests_request_made_while_it_reconnects_reaches_the_host_once_it_is_back`, `a_request_dismissed_is_the_one_named_whatever_else_has_come_and_gone`). An old server answers the opening frame by closing, so **`ServerTooOld` is read only after `CLOSES_BEFORE_WELCOME_FOR_TOO_OLD` (3) clean closes in a row before any welcome**; one close, as a tunnel or a draining server may send, is a connection lost and tried again (`one_clean_close_before_the_welcome_is_a_connection_lost_and_not_a_server_too_old`, `a_server_that_closes_every_connection_before_the_welcome_reads_as_too_old`).

## The lock

**The room's lock is never held across a search, a thread join or a command**: a front end reads the
party from its drawing thread (`Together::party`, `following`, `party_shown`), and a search takes seconds.
Playing a request next clones what it needs out of the lock, and leaving takes the room out and drops it
outside it, since dropping joins the seat's thread. Beginning or joining leaves any party first, so the
old seat puts its queue back before the new one remembers it.

## Surfaces

The window's **Listening Party** pane (`views/party.rs`; it says that in the sidebar with `Icon::Party`, headphones, though `party` stays its tab name in `last-tab`, so a kept tab still reads): with no party a welcome, what happens in three steps and *Start a listening party*; at one, a card of the song, with the cover where the zone shown is hearing it, the standing and the round trip, the host's invitation (the QR code, the link and *Copy link*), who is here and what is asked for, in two columns where the pane has room, and *End the party* or *Leave* in the heading. And `resonate party
start FILES` and `resonate party join LINK`, which run until interrupted or the room ends. Pasting a link
into the search field joins at once, being asked for; a `resonate://party/` argument or `OpenUri` asks first (below, `binary.md`). The phones read
one `ListeningParty` through `party_shown`, polled while shown. **A party link that arrives from outside the
app asks first on the phones** (*Join this listening party?*, as pairing asks *Join these devices?*): iOS's
`Library.open` hands it to `TogetherState.arrive`, which holds a party link as `partyOffered` for the
`PartyOffer` alert and joins at once any other invitation; Android's `MainActivity` does the same through
`Together.arriving` (`InvitationLinks.isAParty` tells a party link by scheme, host and page without the
native library) and `PartyOffer`. A link the listener pastes or types joins at once, being asked for.
**The window asks the same way** (`views/party_offer.rs`, a `Sheet::Offering` of `views/sheets.rs`): `together::accepted` with `Arrival::FromOutside` (an argument, `OpenUri`) hands a party link to `party_room::offer`, which tells the window `Followed::PartyOffered(link)` down the channel the bus already uses (links that came before the window held in `Offering::Waiting` and told in order once it opens), and `RootView::offer_a_party` sets `party_offered`. The sheet reads *Join this listening party?* and says what the link takes over as the phones' words do, naming the zone shown (*the zone Kitchen*) where there are several and *this computer* alone; focus lands on *Cancel*, no key confirms, escape declines and the window is raised. *Join* sends `Together::join_party(link, zone)` for the zone shown and opens the Listening Party pane, a refusal a toast (`a_party_link_from_outside_asks_first_and_focus_lands_on_the_control_that_declines`). `Arrival::Pasted` (the search field) still goes through `Together::accept`, the first zone, at once.
