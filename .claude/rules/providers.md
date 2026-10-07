---
paths:
  - "crates/resonate-providers/**/*.rs"
  - "crates/providers/**/*.rs"
  - "crates/resonate/src/providers.rs"
  - "crates/resonate-library/src/supply.rs"
  - "crates/resonate-library/src/filed.rs"
---

# Providers

A provider turns an identity into media. `resonate-providers` is the seam (depends on
`resonate-core`, `parking_lot`, `thiserror`, `tracing` only, so `cargo tree -p resonate-providers`
stays free of library, codec, vault, gpui: a provider needing the catalog could not be written);
crates under `crates/providers/` fill it; the binary registers them; `resonate-library`'s poll does
the rest. `Providers` is the registry: `Providers::none()` holds the `Unprovided` stub, `and`
registers one per name (as `Lyricists::and`), `Providers::only` answers a registry of the one
provider named, for a poll only it can answer (below).

## What a provider is handed and answers

`Want::identity()` builds an `Identity`: identifiers (`recording`, `track`, `release` as `Mbid`;
`isrc`), search material (title, artist, album, length, disc, position), and the MusicBrainz `Link`s
of track and release (`track_on(Service::Tidal)`, `release_on(..)` find a provider's own page; ask
the track's first). A provider needing an identifier (an unenriched row has titles alone) answers
`Nothing` without one: **a guess is never written**, hence `resonate-inbox` never matches a title.

`Provider::find` answers `Obtained::Nothing`, `Obtained::Found(Delivery)`, or `Err` where it could
not be asked (`Providers::first` logs, counts `refused`, goes on). **Finding is not downloading**: a
stream offer carries an `Opening` the poll opens only for the offer it takes, so a want's providers
race searches and an untaken offer costs nothing more
(`a_search_is_answered_without_the_download_being_asked_for` in each hosted provider's tests,
`an_offer_the_race_did_not_take_is_never_opened`). `Error::is_the_provider_away` marks provider (not
want) errors: `Io`, `Unwelcome`, `StillQueued`, `Untrusted`, `Refused` 500+; `TurnedAway` and 404
are the want's. **A file still arriving is neither miss nor provider away**: `StillArriving` counts
`refused`, the want stays unstamped and due, the provider is still asked about the rest.

- **`Delivery::File(PathBuf)`**: audio on disk, copied by the vault, left in place (a provider's
  folder is never written).
- **`Delivery::Stream { key, extension, opening }`**: bytes from anywhere, recorded as taken from
  `<provider>:<key>`; `Extension` is the decoder's probe hint. `Opening::open` answers
  `Opened::Reading(reader)`, `Opened::Gone` (track went between search and download: Monochrome's
  404/410) or the provider's `Err`; `Opening::ready` wraps an open reader.

## What the infrastructure owns

- **Wants are asked `PollOptions::lanes` (three) at a time.** The first want is asked alone, so an
  away provider or refused login is met once and noted in the shared `Away` before other lanes
  start; each lane then claims the next due want. **A lane hands offers to its own keeper and goes
  on**: each lane starts a `resonate-keeper` thread and passes it a `Keep` (claimed want, rounds so
  far, offers) over a channel of none; the keeper opens, downloads, keeps or files, reads back one
  delivery while its lane searches the next; a lane offered a second while the first is kept waits
  (`a_lane_asks_about_its_next_want_while_the_last_delivery_is_kept`). The keeper owns the want to
  the end (falls through to held-back offers, asks again after a refused offer), so a lane may have
  two wants asked at once. A want stays claimed until the keeper settles it (`Claimed` releases on
  drop, a panicking keeper included, failing the poll as `Error::Stopped`); a lane whose keeper did
  not start keeps its own offers. Idle lanes wait for the pool to drain. A want marked mid-poll is
  picked up: `PollProgress::wants_changed` makes the next claim reread wants (newest first),
  answering false once the poll closed or was cancelled (the window reads that as *owe a poll*).
  **Every way out of a poll closes it**: `supply::start`'s thread runs `PollProgress::close` after
  `run` whatever it answered, so a want nudged into a poll cancelled or failed before any claim read
  it is `owes_a_poll` and the window owes one at the end
  (`a_cancelled_poll_is_not_nudged_and_owes_nothing`,
  `a_poll_nudged_before_it_was_cancelled_owes_another`; cancelling one download once dropped every
  other want to the timer). `PollProgress` keeps a lane per want asked (`asking_all`, `provider_of`,
  `received_for`): each download row names its own provider and bytes.
- **Due wants: registration order, earliest registered offer wins.** `wants.misses` counts
  consecutive tries every provider answered nothing. `Want::due_at`: at once if never tried;
  `RETRY_WAITS` after the last try for the miss it is on; `POLL_AGAIN_AFTER` after an offer the
  catalog does not hold yet; `None` once misses reach `TRIES_BEFORE_GIVING_UP` (`Want::gave_up`).
  `want_in` (`library.md`) resets the count; `ASKING_EVERY_WANT` asks a given-up want too (*Poll
  now*). `Library::next_want_due` is what the window wakes for. Poll claims read `wants_unheld`
  (held never due; the links a provider is asked by); `is_a_want_due`, `next_want_due`, `last_tried`
  and the window's shelves read `wants_as_they_stand` (every want, no links grouped);
  `Library::wants` is the whole read for the `wants` command.
- **A want is tried only when every provider answered it.** `Answer::heard_from_every_provider` is
  false if any provider refused, ran late or was passed over; such a want is neither stamped nor
  counted `nothing`, so a downed server or wrong password is asked again once put right, never
  given up for it.
- **A poll only one provider can answer asks it alone; its silence says nothing of the rest.**
  `Providers::only` marks the registry narrowed wherever it left another real provider out;
  `Providers::first` copies the mark to `Answer::narrowed` (never `heard_from_every_provider`), so a
  want the inbox alone lacks is neither stamped nor a miss. Narrowing is the registry's, not an
  option beside it, so it cannot reach a poll taking silence for everybody's. The window's inbox
  poll (`Prompted::ByTheInbox`) uses it.
- **An absent provider is asked once a poll, not once a want.** One `Away` goes to every
  `Providers::first`; a provider whose error `is_the_provider_away`, or that ran late, is passed
  over for later wants (`Answer::passed_over`): an unreachable host costs its connect timeout once,
  a wrong password one login (a server banning repeated failures would lock the listener out).
  Wants it was not asked about stay due.
- **How long a provider is waited on.** `Providers::first` takes an `Asking`: `within`
  (`ANSWERS_WITHIN` default), `cancelled` (off poll progress), `turning_to` (called with each real
  provider's name as asked), `declined` (every delivery weighed against it), `passing` (left out of
  this ask), `apart`, `grace`. Only real providers race (`Unprovided` is never asked): the first
  registered is rank 0, the second is turned to one `apart` in
  (`the_second_provider_registered_is_turned_to_one_pace_in_and_not_two`). Each is asked on its own
  thread; one unanswered by the deadline is left behind, counted `late` (not `refused`), answer
  dropped. A cancel ends the wait at once, the want unstamped, unless an offer is held (taken).
- **One want's providers are asked side by side, registration order deciding.** Rank *n* is asked
  `n × apart` (`TURNED_TO_APART`, 1.5 s) after the ask began, or at once where every earlier
  provider answered; none after the best held offer is asked. An offer is taken where no earlier
  provider is still being asked, or once `grace` (`WAITED_ON_AFTER_AN_OFFER`, 3 s) passed since the
  first offer: the inbox or a listener's server still beats a faster hosted service, and a hanging
  provider costs the next `apart`, not `within`
  (`a_slow_provider_does_not_hold_back_one_registered_after_it`,
  `an_earlier_provider_answering_within_the_grace_is_taken_over_a_later_one_that_offered_first`,
  `a_later_offer_is_taken_once_the_grace_runs_out`,
  `a_provider_is_not_asked_before_its_turn_while_one_before_it_may_still_answer`). A provider still
  asked when the offer is taken is abandoned on its thread, answer dropped. `Answer::held_back`
  returns the other held offers unopened in rank order
  (`an_offer_not_taken_is_handed_back_unopened_behind_the_one_taken`); `Answer::heard` names the
  providers answered, which a later round passes (one still being asked is asked again).
- **An offer that cannot be opened falls through.** `Pumped` opens on the `resonate-delivery`
  thread under the same stall deadline and cancel as its bytes. `Gone` passes to the next offer like
  an answer of nothing; an `Err` counts `refused`, notes the provider `Away` where
  `is_the_provider_away`, leaves the want unstamped unless a later offer is kept
  (`an_offer_that_cannot_be_opened_falls_through_to_the_next_provider`). Held-back offers are tried
  before any provider is asked again
  (`an_offer_held_back_by_the_race_is_opened_when_the_one_taken_cannot_be`).
- **Stream reading.** `Pumped` reads on a `resonate-delivery` thread, `CHUNK_BYTES` at a time, at
  most `CHUNKS_AHEAD` ahead, the keep watching the cancel. A cancel discards the staging, want
  untried; a stream silent for `answers_within` is given up likewise, counted `late`, want stamped
  tried (the provider answered, what it answered stopped), provider noted `Away`. Bytes bounded by
  the vault's `LARGEST_DELIVERY`.
- **Keeping** is `Vault::keep_delivered`: a delivery meets the import's bit-exact, never-larger
  promise.
- **A delivery is weighed against the row's length.** Where the release row names one,
  `Want::lasts_as_long_as` takes a delivery only within the library's `LENGTHS_AGREE_WITHIN` (the
  vault's `Kept::frames`; in the music folder a whole decode of the landing); further off counts
  `unkept`, the object is left to `--prune`, a filed landing removed. **A delivery that is not the
  song is passed over for the next provider.** `landed` answers a `Landed`: `Kept`; `Unkept` (vault
  or folder failed, stream stalled, row held meanwhile); `NotTheSong` (wrong length, vault
  `Keeping::Refused`, filing too large or undecodable); `Unopened` (fell through, above). On
  `NotTheSong` the offer goes to `refused_offers` (`MIGRATIONS` step: want, `taken_from`, when) and
  the next held-back offer is taken; once spent, the want is asked again in the same claim with
  every heard provider in `Asking::passing` (left out, not counted passed over), so the provider
  registered after the wrong-song one is asked at once, not never
  (`a_delivery_refused_from_one_provider_is_asked_of_the_next`). Rounds end at a keep, an unkept
  delivery or a round of nothing; the want is stamped once, a miss where nothing was kept and every
  round heard from everyone. **A refused offer is declined by the next poll too**:
  `declined_offer_rows` reads `refused_offers` beside `forgotten_deliveries` into the same
  `declined`, refusals older than `REFUSALS_REMEMBERED_FOR` (30 days) dropped, so a provider
  offering the same wrong stream is passed over and the next asked
  (`an_offer_refused_as_not_the_song_is_declined_by_the_next_poll_which_starts_elsewhere`);
  `want_in` forgets a want's refusals, so asking again hears the offer again
  (`a_refused_offer_is_offered_again_once_the_want_is_asked_for_again`). A re-landed release's wants
  carry forgotten deliveries, not refusals (relearned next poll).
- **No vault open: a delivery is filed in the music folder.** `Library::deliver_into` names a
  `DeliveryFolder` (`music-folder` path, `organise-as` layout), set by the binary at open and by the
  window before every poll; with neither, a stream is `unkept` and a file only offered. `filed.rs`
  renders the path under the folder's `Naming`, stages bytes as hidden
  `.<name>.<pid>-<n>.resonate-delivery` beside it (at most `LARGEST_FILED`), noted in
  `staged_writes` before a byte is written as organise notes its own, so a poll killed mid-filing
  leaves a file the next poll sweeps (`organise::sweep_what_a_killed_process_staged`, sparing this
  process and live ones;
  `what_a_killed_poll_staged_in_the_music_folder_is_taken_away_by_the_next_poll`), lands by `hard_link`
  under the first free name `take_in::candidates` offers (nothing overwritten), tags via `FileTags`.
  A landing the decoder cannot probe, or whose decode disagrees with the wanted length, is removed
  and counted `unkept`. **It joins the album it was wanted for, not one of its own**:
  `Library::claim_album_keys` names the album by the keys the scan will compute, before any scan
  reads it. As each filing is noted the lane scans the roots it landed under (incrementally) and
  `Library::pair_what_landed` pairs each unheld want with the rooted row at its offered path: an
  ordinary library track, never a vault object, playable before the keeper opens the next delivery
  (`a_delivery_with_no_vault_is_held_before_the_lane_opens_the_next`). A filing whose scan cannot
  start (another lane or pass walking the tree) waits for the scan the poll runs at its end
  (pairing also runs as a poll starts, for a scan another pass refused).
- **Kept becomes a track row.** `Library::note_delivered` writes the `vault_objects` row and a
  `tracks` row in one transaction and pairs the want's release track: playable, searchable, held on
  landing. The row is named by the object's own path with `vault_key`, `vault_path` set (stand-in
  answers for it, `--prune` spares it); names and identifiers are the release track's; genre,
  ReplayGain, words from `Kept::declared`. **No root** (`tracks.root_id` is nullable for this) and
  every pass over the user's files joins `roots`, so a scan's prune, forget, `organise`, `tag`,
  `vault --import`, `vault --release` never reach it. **A row held meanwhile keeps its pairing**: a
  scan can pair the wanted release row with the listener's own file while a provider answers, so
  `note_delivered` checks inside its transaction that the row is still unheld, else writes nothing,
  answers `None`, counts `unkept`.
- **Forgetting a delivery.** `offered` is the object's URI; a want whose release track holds a row
  is `held`, never due again. `Library::forget_delivered` removes the rootless row a path names
  (`resonate forget`, the window's *Forget this delivery*), nothing a scan filed; the object is left
  to `--prune`, the want standing. **What was forgotten is remembered; the want is due at once**:
  same transaction resets its offer, `tried`, `misses` and adds a `forgotten_deliveries` row
  (`vault_objects.taken_from`, when). The poll hands each want's to `Providers::first` as
  `Asking::declined`, which passes a matching delivery over (`Answer::declined`, never `refused`)
  and asks the next provider. A stream is declined by key alone; a file only where its later
  modification or status change is no later than the forgetting, so a right file put back under the
  same name is delivered.

## Writing one

1. Crate `crates/providers/<name>/`, package `resonate-<name>`, on `resonate-core`,
   `resonate-providers` and what reaches its source. A network provider uses `ureq` itself (not
   `resonate-online`) and identifies itself per `online.md` (name and version).
2. A workspace member and `[workspace.dependencies]` entry.
3. A binary dependency and one `.and(..)` in `providers::registry` (inbox by `with_inbox`), gated
   on settings. `Accounts` builds network providers: `Accounts::of` off `Config` for a headless run
   (`providers::registered`), `Accounts::given` off the window's live `resonate_ui::Online`
   (`providers::sourced`, handed over as `resonate_ui::Sourcing::register`, called on every
   `LibraryModel::providers`). `registry` is the one place any is registered, in code, never at run
   time, so a changed setting applies from the next poll, no restart. `Made` returns the same
   provider (`Kept`) until its settings change (a network provider holds session, tokens, pacing). A
   provider with a setting of its own widens `Online`, `Accounts`, `registry` together.
4. `Error::Io` names the provider and a `ProviderOp`; an error the seam lacks a variant for is added
   to the seam with its op when that provider lands, never as prose.

**The window polls on its own** as well as on *Poll now*: when the soonest want is due
(`Shelves::next_try`, `LibraryModel::ask_when_due`, bounded by `FIRST_ASKED_AFTER`,
`RETRIES_ASKED_AT_LEAST`, `ASKED_EVERY`) and when a want is marked, each only where a provider is
registered and `Library::is_a_want_due`. A want marked while another pass holds the library is owed
(`LibraryModel::poll_owed`), asked when `take_up_what_waited` finds it free. A poll refused because
another process holds the poll lock (`resonate poll`, the MCP server) is owed too, retried
`OWED_ASKED_AGAIN_AFTER` (30 s) later (no pass of this window's ending to take it up); only *Poll
now* says the providers are busy. *Poll now*, the inbox poll and `resonate poll --again` use
`PollOptions::ASKING_EVERY_WANT` (someone who just dropped a file in the inbox means *now*); the
timer and bare `resonate poll` keep `Want::due_at`, sparing network services.
`PollProgress::{asking, asking_provider, received}` carry want, provider, bytes landed for the
window's *Downloading…*; read every frame, so the provider sits under a `parking_lot::Mutex` held
for a clone and the count is an atomic. A `Delivery::File` counts its whole length when the vault
takes it up, so an inbox delivery shows as received like a stream
(`a_file_delivered_into_the_vault_counts_the_bytes_it_was_as_received`).

**The window watches the inbox folder** via the roots' `RootsWatch`: once a write under it is quiet
for `INBOX_QUIET_FOR`, it polls as `Prompted::ByTheInbox`, raising no notice. **What landed while no
window was open is asked about on open**: the first look weighs the folder's newest file (later of
modification and change time: a copy keeping its old time still changed status on landing) against
`Library::last_tried` (`landed_since`).

## The inbox

`resonate-inbox` is the reference provider: `Inbox::at` over the folder the `inbox` key names, read
never written. A file directly inside whose stem is the recording MBID, then track MBID, then ISRC,
ignoring case, matches; never a nested folder or shared title. Only audio is offered
(`resonate_core::names_audio` over `AUDIO_EXTENSIONS`, in core so the inbox, which may not see the
library, reads the list the scan walks by): a `.cue`, `.jpg` or rip log beside the audio is never
delivered ahead of it.

- **Folder read once a poll, not once a want**: the listing is held with the folder's modification
  and change times, reread only if either moved or it is older than `LISTING_TRUSTED_FOR`
  (coarse-timestamp filesystems).
- **Most exact name wins; of one name's files, the lossless one**: sorted by `Fidelity`
  (`Lossless`, `LosslessOrLossy` for containers holding either, `Lossy`) then name, so
  `<mbid>.flac` beats `<mbid>.mp3` while a recording's `.mp3` still precedes an ISRC's `.flac`.
- **A file still being copied in is not delivered.** If the best-named file's modification or
  change time is within `SETTLES_FOR` (the window's `INBOX_QUIET_FOR`) of now it answers
  `Error::StillArriving`, and no lesser file is delivered instead. The change time is the one a
  copy cannot keep old (`cp -p` sets the modification time back); a future time holds nothing back.

## A Subsonic server

`resonate-subsonic` reaches the listener's server (Navidrome, Airsonic, Gonic, any Subsonic API)
named by `subsonic`, `subsonic-user`, `subsonic-password`; `providers::registry` registers it after
the inbox only where all three are given and `online` is on.

- **Asked by identifiers, never a title.** No recording MBID and no ISRC: `Nothing`, no request.
  Else `search3` in words (title and artist, then title alone: Gonic matches the whole query against
  the title), `SONGS_A_PAGE` at a time up to `PAGES_AT_MOST`. A song is taken only where its
  `musicBrainzId` is the recording or its `isrc` (one code or a list as OpenSubsonic writes it, read
  via `Isrc::new`) holds the want's, so a tribute band's *Echoes* is never delivered for Pink
  Floyd's.
- **The log never sees a request's address.** Every URL carries the user, token and salt, and
  ureq's `BadUri` and `Http` errors print the whole of it (a `subsonic` setting with no scheme), so
  `told` logs those two as a sentence alone
  (`an_address_without_a_scheme_is_never_told_to_the_log_with_its_token`).
- **Deadline for an answer, for a download silence alone.** API requests carry
  `Patience::answered_within` as `timeout_global`. A download may run minutes: its agent bounds the
  head alone and `stall.rs` each read (`BrokenOffAfter`, a connector chained after ureq's
  `DefaultConnector`, capping reads at `Patience::broken_off_after`): a silent socket closes, one
  still delivering is read however long. The connector is ureq's `unversioned` API, outside its
  semver promise; weigh a ureq bump against the stall tests.
- **Private CA reached.** Both agents trust `trust::system_and_built_in` (Mozilla roots plus system
  store); a certificate the handshake still refuses is `Error::Untrusted`, not a refused
  connection.
- **It delivers the server's original file, only a file**, keyed by song id, hinted by `suffix` (not
  an `Extension`: `Nothing`). The API answers a failed download with its error document and a 200,
  so a download whose `Content-Type` names text, JSON or XML is read as that document, never
  streamed as a song.
- **Paces itself**: requests `ASKED_APART`; 429/503 asked again after `Retry-After` (capped
  `LONGEST_RETRY_AFTER`) up to `RETRIES_AT_MOST`, then the `Refused` it was. The seam's `Pacing`
  reserves a turn under its lock and sleeps towards it outside, so lanes asking one provider queue
  for turns, not for a lock held through another's sleep; a server-requested wait is
  `Pacing::cool_for`, held by every later caller, not slept by the one that heard it
  (`turns_are_reserved_apart_and_nobody_waits_on_the_lock_while_another_sleeps`,
  `a_server_asking_to_be_asked_later_holds_back_every_caller`). TIDAL's `Asker` and Monochrome use
  the same type.
- **The password never leaves as typed.** Each request carries user, fresh salt and `t` = MD5 of
  password and salt (the API's token scheme); User-Agent bare; `Server`'s `Debug` prints
  `<withheld>` for the password. `status: failed` is `Error::Unwelcome` where the code concerns the
  account (`ACCOUNT_REFUSALS`), else `Error::TurnedAway` (70, a song the server lacks).

## A TIDAL account

`resonate-tidal` is the listener's own subscription, named by `tidal-client-id`,
`tidal-client-secret`, `tidal-refresh-token`; `providers::registry` registers it after the inbox
and Subsonic only where client id and refresh token are given (secret sent where given) and
`online` is on. Nothing is downloaded to play: a delivery is fetched whole and lands as a track row.

- **Asked by link and ISRC, never a title.** The TIDAL track MusicBrainz links the recording to
  (`Identity::track_on(Service::Tidal)`) is asked first; failing that, the OpenAPI is asked for the
  ISRC and a listing taken only where its own `isrc` is the want's, at most `TRACKS_TRIED_AT_MOST`.
  Neither: `Nothing`, no request, no sign-in.
- **It signs in with the listener's refresh token alone.** No client id or secret is built in: the
  listener brings the application the token was issued to. 400/401 at the token endpoint is
  `Error::Unwelcome` under `ProviderOp::SignIn` (provider away). **A rotated token is handed
  back**: the provider signs in with the new one from then on and calls what `Tidal::telling`
  registered, once per rotation; the binary's `Renewed::note` writes it to `tidal-refresh-token` in
  the settings file the run was read from (`config::store`) and remembers it against the token the
  settings gave, so a provider rebuilt for the same settings uses the rotated token, not the
  refused one. An API 401 signs in again once and asks again; a second is `Unwelcome` with TIDAL's
  `subStatus`, except 4005 (asset not ready for playback), the want's, `TurnedAway`. 403/404 is the
  track unavailable to this account or country: `Nothing`.
- **Only the whole track, lossless, in the clear.** `HI_RES_LOSSLESS` asked; `assetPresentation`
  other than `FULL` (a lapsed subscription's preview) is `Nothing`. `manifest.rs` reads both
  manifests TIDAL answers (base64 BTS JSON naming one URL; DASH MPD, selecting its FLAC rendition).
  Encryption is `Withheld::Encrypted`, no FLAC rendition `Withheld::Lossy`; both answer `Nothing`
  (no decrypting, no lossy stream).
- **A number the service gives is checked before it is added to.** A DASH `startNumber` whose
  segments would number past `u64` is `Unread::NumberedPastTheEnd`; an `expires_in` (session,
  hifi-api token, device code) no clock can add is `Error::Unreadable`, never an `Instant`
  overflow panicking the provider's thread
  (`a_session_said_to_last_past_any_clock_is_refused_as_unreadable`,
  `a_dash_manifest_numbered_past_the_last_number_is_refused`).
- **Media from TIDAL's audio hosts alone.** `MediaHosts::holds` takes a URL only over `https` with
  host `audio.tidal.com` or under it, refusing a user-info `@`, a bracketed literal, a host merely
  ending in the name; one URL off them fails the whole manifest as `Error::OffItsHosts` before a
  byte is fetched. The media agent follows no redirect.
- **A segment that breaks off is asked again from where it stopped** with `Range`, up to
  `RESUMES_AT_MOST` times running. `find` reads the playback answer and manifest and offers the
  media; the first segment is fetched when the offer is opened, so a CDN refusing it is the
  provider's `Refused` under `ProviderOp::Download` (offer falls through, not a failed keep).
- **Later segments are fetched ahead.** `Fetched` streams the first and keeps `SEGMENTS_AHEAD` (3)
  more coming whole, each on its own `resonate-tidal-segment` thread handing bytes over a channel
  of one, read in order, topped up as each drains: a segment's round trip overlaps reading the
  previous (`a_dash_stream_is_fetched_segments_ahead_and_read_in_order`). A segment read ahead is
  resumed as one read in turn is, bounded by `LARGEST_SEGMENT`, fails the stream with its own
  error; dropping the stream drops the channels, a thread ends with its one request.
- **What lands is a FLAC file, never an MP4.** DASH is FLAC frames in fragmented MP4; `remux.rs`
  repacks without decoding: `fLaC` and the metadata blocks from the `dfLa` box, then every `mdat`
  payload, STREAMINFO's total sample count (zero in a fragmented file) filled from the timeline.
  Every delivery keyed `track/<id>`, extension `flac`.
- **Signed in from the window or `resonate tidal`.** `resonate_providers::SignsIn` is the seam,
  `TidalSignIn` its one implementation, handed to the window as `Lookups::signs_in` by
  `providers::signs_in`. The device flow polls every `interval` (at least `ASKED_EVERY_AT_LEAST`,
  longer on `slow_down`) until a refresh token, `AuthorizationLapsed`, `AuthorizationDenied` or the
  cancel (`Ok(None)`); `Debug` never prints the device code. The *A TIDAL account* group's *Sign in
  to TIDAL* writes the token into the field, the global `Online` and `tidal-refresh-token`.
  `resonate tidal` is the same flow windowless (`providers::sign_in_to_tidal`): refuses with
  `OnlineOff` or `NoTidalClient` before asking, prints link and code, waits with the first
  interrupt as cancel (`SignInCancelled`), writes the token to the settings file via
  `config::store`
  (`a_tidal_sign_in_with_online_off_or_no_client_is_refused_before_anything_is_asked`).
- **Paces and identifies itself as the Subsonic client does**; `Account`'s `Debug` prints neither
  secret nor token. `asker.rs` is the pacing, retrying and reading both TIDAL providers share;
  `played.rs` what follows a playback answer, with `played::found` (link-then-ISRC order) written
  once over the `Finds` trait each implements.

## A hifi-api server

`HifiApi`, same crate, is the second way to a TIDAL subscription. With `online` on,
`providers::registry` registers it after `Tidal` as `hifi-api`, on a hosted service by default; the
`hifi-api` setting and *hifi-api server* field override it with the listener's own server, clearing
the field restores the hosted one. The hosted service holds no listener credential: its search and
playback tokens are cached until shortly before expiry and sent only to their own issuer or API.
`played.rs` reads the manifest through the same checks as a TIDAL account; the service's manifest
URL must be HTTPS on `manifest.tidal.com` or a subdomain.

- **Asked by link and ISRC, never a title**, via `played::found`: the linked track first; else the
  hosted flow searches TIDAL's web API by title and artist, taking only listings whose own `isrc` is
  the want's, up to `TRACKS_TRIED_AT_MOST`. A custom server keeps the hifi-api routes with
  `data.items` checked by ISRC.
- **A request the server queues is waited for, withdrawn past the provider's patience.** A server
  whose accounts are all busy answers `202` with a `requestId`; the provider asks
  `/playback/requests/<id>` again after its `Retry-After` (held between
  `QUEUE_LOOKED_AT_LEAST_EVERY` and `QUEUE_LOOKED_AT_MOST_EVERY`) for at most `QUEUED_FOR_AT_MOST`,
  inside the poll's `ANSWERS_WITHIN`, then sends `DELETE` and answers `Error::StillQueued` (provider
  away). The request id is used only as letters, digits, dashes; the path is the provider's own,
  never the server's `statusUrl`.
- **401 is the server's account turned away** (`Unwelcome`, away); 403/404 that track unavailable
  (`Nothing`); else `Refused`. A custom server gets no `Authorization` header.
- **Custom server trusted as Subsonic's is**: `HifiApi::at` uses `Asker::of_the_listeners_server`
  (system store beside built-in roots, `trust.rs`); hosted service, TIDAL's API and CDN keep
  built-in roots alone. A refused certificate anywhere in the crate is `Error::Untrusted` via
  `asker::unreached`.

## A Monochrome server

`resonate-monochrome` asks the track streamer behind [Monochrome](https://monochrome.st). With
`online` on, `providers::registry` registers it after `hifi-api` as `monochrome`, asking
`tracks.monochrome.st` by default; the `monochrome` setting and *Monochrome server* field (in the
*A TIDAL account* group, beside the hifi-api override) name another, clearing restores the hosted
one. The binary's `Hosting` (`Hosted`/`Custom`) builds both overridable services. Two routes:
`/search/tracks?q=<words>&limit=` answers `tracks` listings (`trackId`, `isrc`, `playable`),
`/track/<trackId>` the whole FLAC.

- **Asked by ISRC alone, never a title.** Listings carry no MusicBrainz id and the search does not
  read an ISRC as its query, so no ISRC: `Nothing`, no request; else searched in words (title and
  artist, then title alone, `LISTED_AT_MOST` listings), a listing taken only where its own `isrc`
  (via `Isrc::new`) is the want's and it is not `playable: false`. A `trackId` is used only if all
  digits (it goes into the path).
- **What lands is the server's FLAC**, keyed `track/<trackId>`, extension `flac`, opened only when
  the offer is taken, so a refused download is the provider's `Refused` under
  `ProviderOp::Download`. A 404/410 there is the track gone, `Opened::Gone`; a download whose
  `Content-Type` names text, JSON or XML is `Unreadable`, never streamed as a song.
- **A download that breaks off is asked again from where it stopped** (`fetched.rs`) with `Range:
  bytes=<read>-`, up to `RESUMES_AT_MOST` times running: a `206` whose `Content-Range` starts there
  is read on, a `200` is read past what was held. A file runs to hundreds of megabytes, so each
  read, not the whole, is bounded: `stall.rs`'s `BrokenOffAfter` as Subsonic's.
- **Paces as Subsonic does**: `ASKED_APART`; 429/502/503/504 asked again after `Retry-After` up to
  `RETRIES_AT_MOST`, then the `Refused` it was. User-Agent bare.
- **Custom server trusted as Subsonic's is** (`trust.rs`); hosted keeps built-in roots alone.
  A refused certificate is `Error::Untrusted`.
