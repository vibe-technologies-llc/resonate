---
paths:
  - "crates/resonate-providers/**/*.rs"
  - "crates/resonate-fetch/**/*.rs"
  - "crates/providers/**/*.rs"
  - "crates/resonate/src/providers.rs"
  - "crates/resonate/src/served.rs"
  - "crates/resonate-library/src/supply.rs"
  - "crates/resonate-library/src/filed.rs"
  - "crates/resonate-library/src/keeps.rs"
  - "crates/resonate-signin/**/*.rs"
  - "crates/resonate/src/signing_in.rs"
---

# Providers

A provider turns an identity into media. `resonate-providers` is the seam (depends on
`resonate-core`, `parking_lot`, `thiserror`, `tracing` only: a provider needing the catalog could
not be written); crates under `crates/providers/` fill it; the binary registers them;
`resonate-library`'s poll does the rest. `Providers` is the registry: `Providers::none()` holds the
`Unprovided` stub, `and` registers one per name, `Providers::only` answers a registry of the one
provider named, for a poll only it can answer.

## What a provider is handed and answers

`Want::identity()` builds an `Identity`: identifiers (`recording`, `track`, `release` as `Mbid`;
`isrcs`), search material (title, artist, album, length, disc, position), and the MusicBrainz
`Link`s of track and release (`track_on(Service::Tidal)`, `release_on(..)`; ask the track's first).

## Which listing is the song (`listing.rs`)

**An explicit take is taken, and a clean one never is.** `Listed::stated` is what a service says
(TIDAL, hifi-api and Monochrome's `explicit` flag, `Rating::flagged`: true is `Explicit`, false says
nothing, since a song with nothing to bleep is false too; OpenSubsonic's `explicitStatus`), and
`Rating::of` falls back on the qualifier in the title or version (`clean`, `edited`, `censored`;
`explicit`, `uncensored`, `dirty`; a song called *Clean* with no bracket is neither). `Choosing::weigh`
drops a `Clean` listing whatever its code, so a song offered only clean is `Nothing` rather than the
clean take, counted in `clean_passed_over`; `ranked` takes every `Explicit` listing ahead of the rest,
even one only named alike over one holding the code wanted, because services file the clean and
explicit takes under different ISRCs and a want often names the clean one's
(`a_clean_listing_is_never_taken_though_it_is_the_very_code_wanted`,
`an_explicit_listing_is_taken_over_one_the_want_is_coded_for_and_over_one_unmarked`). **A search does
the same** (`Found::explicit_first`, in each provider's `PlaysInPlace::search`): a clean song or album
is left out, and an unmarked sibling of an explicit one (same bare title, artist and length) gives way
to it in its place. `the_take_worth_offering` ranks MusicBrainz's takes the same way and folds a song's
takes together past their rating, so a found song offers the explicit take where one is named.

**A listing is taken by any of the want's codes, else only named alike, and the seam alone
decides.** A searching provider turns each listing into a `Listed` and hands it to a `Choosing`
with the item it would deliver. `Choosing::ranked` answers listings holding one of
`Identity::isrcs` within `LENGTHS_AGREE_WITHIN` (5 s, nearest first, so a wrong master is not
downloaded and then refused for 30 days); only where none is coded, listings
`Identity::named_alike`: lengths within `A_LISTING_NAMED_ALIKE_MAY_DIFFER_BY` (2 s), titles equal
once `resonate_core::titles::bare` and `words_of_a_name` folded, the want's
`Identity::lead_artist` one of the listing's. A live, remix, acoustic or instrumental take is no
qualifier. Why: re-releases carry codes MusicBrainz never lists, and the strict name rule is no
guess; the delivered file is weighed again (`lasts_as_long_as`).

`Identity::may_be_listed` (a code, or an artist and a length) is asked before any request;
`Identity::wordings` is what a provider searches with (`"{title} {lead}"`, `"{lead} {title}"`,
`"{bare title} {lead}"`, `"{title}"`, deduplicated, stopping at the first wording holding a coded
listing: Monochrome's search recalls by wording order). A provider answering `Nothing` says at debug
how many wordings it asked and listings it saw
(`any_of_the_wants_codes_takes_a_listing_as_long_as_it_and_the_nearest_wins`,
`a_listing_under_another_code_is_taken_only_named_alike_and_only_where_none_is_coded`).
`resonate-inbox` matches no name.

`Provider::find` answers `Obtained::Nothing`, `Obtained::Found(Delivery)`, or `Err` where it could
not be asked (`Providers::first` logs, counts `refused`, goes on). **Finding is not downloading**: a
stream offer carries an `Opening` the poll opens only for the offer it takes, so an untaken offer
costs nothing more (`a_search_is_answered_without_the_download_being_asked_for`,
`an_offer_the_race_did_not_take_is_never_opened`). `Error::is_the_provider_away` marks provider
errors: `Io`, `Unwelcome`, `StillQueued`, `Untrusted`, `ToolFailed` (a program the provider runs
exited badly, `code` its exit code), `ToolNotRun` (the program could not be started, raised where it
is spawned and the only error `Error::why_away` reads as `WhyAway::ToolMissing`: an `Io` of kind
`NotFound` from a folder or a server says nothing of a missing tool), `NotTheService` (a page where the API's
document was asked: `resonate_providers::is_a_page`), `Redirected` (a redirect `followed` refused,
below), `Refused` 401, 403, 429 (exhausted retry) and
500+; `TurnedAway`, 404 and `Busy` (a server still busy with another download after its `Retry-After`
waits) are the want's, the want left unstamped. **A file still arriving is neither miss nor provider
away**: `StillArriving` counts `refused`, the want stays unstamped and due.

- **`Delivery::File(PathBuf)`**: audio on disk, copied by the vault, left in place.
- **`Delivery::Stream { key, extension, opening }`**: recorded as taken from `<provider>:<key>`;
  `Extension` is the decoder's probe hint. `Opening::open` answers `Opened::Reading { reader,
  length }`, `Opened::Gone` (track went between search and download: 404/410) or the provider's
  `Err`; `Opening::ready` wraps an open reader of no known length. **`length` is what the server
  said the whole is**: `Ranged::length` reads the first answer's `Content-Length` (`None` where it
  sent none or ureq decompresses), so Subsonic and Monochrome carry it, TIDAL only for a one-URL
  FLAC (segments and a remux say none); the phones show it as the download's total
  (`an_opened_download_says_how_long_the_server_said_it_is`).
  **An `Opening` is whole or resumable.** `Opening::new` opens from the start; `Opening::resumable`
  takes the `Held` a caller already has (`bytes`, the whole's `total`, its `Validators`) and answers
  `Reopened::Reading(Resumed { reader, length, from, validators })` or
  `Reopened::Gone`, `from` being where the reader begins (0 for a server that started over). `open` of a
  resumable opening is `open_resuming(None)`; `open_resuming` of a whole one reads from 0 with no
  validators, so a caller asks `is_resumable()` first. **`Validators` keeps each kind apart**: the
  answer's `ETag` as an `EntityTag` (`Strong`, or `Weak` under `W/`, said back as given) and its
  `Last-Modified` as a `Modified`, either absent. Two sets of validators conflict only where both
  name the same kind and it differs (`conflicts_with`; tags by their opaque part, so `W/"a"` agrees
  with `"a"`), a set vouches for an answer where they do not conflict and one kind agrees
  (`vouch_for`), and `joined` fills a kind one lacks from the other, so a date-validated
  continuation bringing its unchanged date beside a new tag is taken and the tag kept
  (`validators_conflict_only_where_both_name_the_same_kind_and_differ`,
  `a_date_vouches_for_a_continuation_that_brings_a_tag_and_keeps_the_date`). Subsonic, Monochrome and a keyed hifi-api are
  resumable; TIDAL and the hosted hifi-api, which rebuild a FLAC from segments, are whole.

## What the infrastructure owns

- **Wants are asked `PollOptions::lanes` (`WANTS_ASKED_AT_ONCE`, three) at a time, fewer where a
  registered provider says `Provider::downloads_at_once`** (`Providers::downloads_at_once`, the
  narrowest; the keyed `resonate` provider says one, since the server downloads one song at a time). **Wants are
  claimed in the order they were asked for** (`WANTS_UNHELD`): a want pressed alone (no other want shares its
  `wanted` stamp) first, newest first, so a song pressed to play is not held behind an album; then the wants asked
  together in one transaction (an album wanted whole, *Get the rest*), oldest batch first, each batch by disc and
  position, so one album downloads in its order and before an album asked for after it
  (`an_album_asked_for_whole_is_asked_song_by_song_in_its_order_and_before_an_album_asked_later`). The first want
  is asked alone, so an away provider or refused login is met once and noted in the shared `Away`.
  **A lane hands offers to its own `resonate-keeper` thread** via a `Keep` over a channel of none;
  the keeper opens, downloads, keeps or files while its lane searches the next; a lane offered a
  second while the first is kept waits (`a_lane_asks_about_its_next_want_while_the_last_delivery_is_kept`).
  The keeper owns the want to the end; a want stays claimed until it settles (`Claimed` releases on
  drop, a panicking keeper failing the poll as `Error::Stopped`); a lane whose keeper did not start
  keeps its own offers. `PollProgress::wants_changed` makes the next claim reread wants (false once
  the poll closed or was cancelled). **Every way out of a poll closes it**: `supply::start`'s thread
  runs `PollProgress::close` after `run` whatever it answered, so a poll nudged and then cancelled
  or failed is `owes_a_poll`, whether or not a lane had reread the wants the nudge named: a reread
  queues a want it may never claim, or cuts one short (`a_cancelled_poll_is_not_nudged_and_owes_nothing`,
  `a_poll_nudged_before_it_was_cancelled_owes_another`). `PollProgress` keeps a lane per want asked
  (`asking_all`, `provider_of`, `received_for`).
- **A metered connection holds a poll's downloads when `download-on-metered` is off.**
  `Library::stall_downloads_by(Gate)` (set by the binary at open from `metered::for_downloads`; the
  gate is shut only while `Meter::Metered`, so off, unknown and unmetered hold nothing). A lane waits
  in `PollProgress::waits_out_the_meter` at three places: after claiming a want and before
  announcing it (so it is not shown as searching), before opening an
  offer, and before each chunk `Pumped` takes from the delivery thread. The last stops the channel
  being drained, the delivery thread blocks on the full channel, the network read stops, and
  `Silence`'s clock starts again after the wait, so a hold is never a `Stalled` delivery nor a
  miss; a connection that was cut meanwhile is the reader's to resume (`Ranged`, `RESUMES_AT_MOST`,
  Subsonic, Monochrome, a keyed hifi-api) or an `unkept` the next poll takes again (TIDAL's
  segments are whole). A cancel ends the wait, and one that arrived while the provider was answering still lets the offer be kept (only a cancel that cut a wait on the meter stops before the keep); `PollProgress::is_waiting_on_the_meter` is what a front end
  reads (the settings pane's *Metered connections* group says so through `Library::is_waiting_on_the_meter`). A want not yet claimed is untouched, still `due`; a poll with nothing to claim
  closes as before. **Playing a song in place is not a download**: `PlaysInPlace` never meets the
  gate (`a_queued_download_waits_while_the_connection_is_metered_and_goes_on_when_it_is_not`,
  `an_offer_found_as_the_connection_turned_metered_is_not_opened_until_it_is_not`,
  `a_download_under_way_stops_reading_when_the_connection_turns_metered_and_finishes_when_it_is_not`,
  `a_connection_that_cannot_be_told_is_not_a_reason_to_hold_a_download`).
- **A pressed song is searched for while its release is read.** `Library::foresee` (`foresight.rs`)
  asks the registry (`Providers::first`, every provider, never a narrowed registry) from the
  `Found` alone: recording, release, every code, title, credit, release title and length; no disc,
  position or link, which only the landed row has. An offer is kept for the recording with the
  providers heard, `FORESEEN_FOR` (2 minutes: a stream offer may lapse), at most
  `WANTS_ASKED_AT_ONCE` searches at once (more are skipped, not queued). **Only an offer is kept**:
  a foresight that found nothing leaves the poll to ask with the full identity, since a link may
  find what the codes and words did not. `Lanes::asked` takes it once (`Foresight::take`) in place
  of its first `Providers::first`, keeping only offers of a provider the poll registers, not found
  gone and not declined (`foreseen_answer`); an offer falling through is followed by the ordinary
  rounds with the heard providers passed. The window runs it beside `want_found_uncovered` and
  starts the poll when both are done (`ui.md`); `forget_foresight` lets one go when the want fails
  (`a_song_foreseen_from_what_the_search_found_is_not_asked_of_the_provider_again_by_the_poll`,
  `a_foresight_is_spent_by_the_poll_that_takes_it_and_asked_afresh_by_the_next`,
  `an_offer_foreseen_of_a_provider_the_poll_does_not_register_is_not_taken`).
- **Due wants.** `wants.misses` counts consecutive tries every provider answered nothing.
  `Want::due_at`: at once if never tried; `RETRY_WAITS` after the last try; `POLL_AGAIN_AFTER` after
  an offer the catalog does not hold yet; `None` once misses reach `TRIES_BEFORE_GIVING_UP`
  (`Want::gave_up`). `want_in` (`library.md`) resets the count; `ASKING_EVERY_WANT` asks a given-up
  want too. Poll claims read `wants_unheld`; `is_a_want_due`, `next_want_due`, `last_tried` and the
  window's shelves read `wants_as_they_stand`; `Library::wants` is the whole read.
- **A want is tried only when every provider answered it.** `Answer::heard_from_every_provider` is
  false if any provider refused, ran late or was passed over; such a want is neither stamped nor
  counted `nothing`, so a downed server or wrong password never gives a want up.
- **A poll only one provider can answer asks it alone.** `Providers::only` marks the registry
  narrowed; `Providers::first` copies the mark to `Answer::narrowed` (never
  `heard_from_every_provider`), so a want the inbox alone lacks is neither stamped nor a miss. The
  window's inbox poll (`Prompted::ByTheInbox`) uses it.
- **An absent provider is asked once a poll, not once a want.** One `Away` goes to every
  `Providers::first`; a provider that `is_the_provider_away` or ran late is passed over
  (`Answer::passed_over`): an unreachable host costs its timeout once, a wrong password one login.
- **How long a provider is waited on.** `Providers::first` takes an `Asking`: `within`
  (`ANSWERS_WITHIN`), `cancelled`, `turning_to`, `declined`, `passing`, `apart`, `grace`. Only real
  providers race (`Unprovided` never). Each is asked on its own thread; one unanswered by the
  deadline is left behind, counted `late` (not `refused`). A cancel ends the wait at once, the want
  unstamped, unless an offer is held.
- **Registration order decides.** Rank *n* is asked `n × apart` (`TURNED_TO_APART`, 1.5 s) after the
  ask began, or at once where every earlier provider answered; none after the best held offer is
  asked. An offer is taken where no earlier provider is still being asked, or once `grace`
  (`WAITED_ON_AFTER_AN_OFFER`, 3 s) passed since the first offer: the inbox or a listener's server
  beats a faster hosted service, and a hanging provider costs `apart`, not `within`
  (`the_second_provider_registered_is_turned_to_one_pace_in_and_not_two`,
  `a_slow_provider_does_not_hold_back_one_registered_after_it`,
  `an_earlier_provider_answering_within_the_grace_is_taken_over_a_later_one_that_offered_first`,
  `a_later_offer_is_taken_once_the_grace_runs_out`,
  `a_provider_is_not_asked_before_its_turn_while_one_before_it_may_still_answer`). `Answer::held_back`
  returns the other held offers unopened in rank order
  (`an_offer_not_taken_is_handed_back_unopened_behind_the_one_taken`); `Answer::heard` names the
  providers answered, which a later round passes.
- **A last resort answers only once every provider ahead of it said no.** Asking it last is not
  enough: a later offer is taken once `grace` runs out, and a want once held is never due again, so
  a lossy file would stand in for a FLAC for good. `Provider::last_resort` (false by default,
  `resonate-ytmusic` true) has `Providers::first` turn to it only once every earlier turn is over,
  and pass it over (`Answer::passed_over`, so the want stays due) where an earlier provider refused,
  ran late or was away (`Answer::every_earlier_turn_said_no`). **It binds across all a want's
  rounds**: `Asking::earlier_rounds_said_no` is false once any round of the want was not heard from
  everyone (`Rounds::every_provider_heard` in `supply.rs`), since a later round passes the providers
  already heard. **Foresight never holds its offer**: `Library::foresee` asks
  `Providers::without_last_resorts`, and `foresight::Unheard` carries what that ask refused, ran late
  or passed over into the poll that takes it, so a foreseen first round hides no refusal. **An offer
  declined or refused as `NotTheSong` still counts as heard**: `heard_from_every_provider` is the
  check, and an unwanted offer leaves no better file to protect, so a last resort may answer after
  one. Named alone (`Providers::only`) nothing stands ahead of it, which is how a keep asks it
  (`a_last_resort_is_not_asked_until_every_provider_before_it_has_answered`,
  `a_last_resort_offer_is_not_taken_while_an_earlier_provider_was_late`,
  `a_last_resort_is_passed_over_when_an_earlier_round_of_the_want_did_not_say_no`,
  `a_last_resort_is_asked_after_an_earlier_offer_the_poll_declined`,
  `a_provider_named_alone_is_asked_though_it_is_a_last_resort`). **A want replacing a lossy song
  (`Want::replacing`, `library.md`'s *Replacing a lossy song*) is never asked of a last resort**:
  `Lanes::asked` turns to `Providers::without_last_resorts` for it, so a lossy file is never
  downloaded for it at all (`a_replacement_is_never_asked_of_a_last_resort`).
- **An offer that cannot be opened falls through.** `Pumped` opens on the `resonate-delivery` thread
  under the same stall deadline and cancel as its bytes. `Gone` passes to the next offer; an `Err`
  counts `refused`, notes the provider `Away` where `is_the_provider_away`, leaves the want
  unstamped unless a later offer is kept
  (`an_offer_that_cannot_be_opened_falls_through_to_the_next_provider`). Held-back offers are tried
  before any provider is asked again
  (`an_offer_held_back_by_the_race_is_opened_when_the_one_taken_cannot_be`).
- **An offer found `Gone` is told to its provider, which is asked again without it.** The poll notes
  the offer's `taken_from` in `Rounds::gone`, hands it to the next ask as `Identity::gone` and drops the
  provider from `Asking::passing`, so a dead TIDAL link MusicBrainz holds no longer ends that
  provider's turn: `played::found` skips a track whose location is gone and goes on to the codes and
  the words, and `Providers::first` passes over any offer already found gone
  (`an_offer_found_gone_is_told_to_its_provider_which_is_asked_again_without_it`,
  `a_linked_track_found_gone_is_not_offered_again_and_the_codes_are_asked_instead`).
- **Stream reading.** `Pumped` reads `CHUNK_BYTES` at a time, at most `CHUNKS_AHEAD` ahead. A cancel
  discards the staging, want untried. **A server is given longer for its first bytes than between
  them**: `Silence` waits `first_bytes_within` (`FIRST_BYTES_WITHIN`, 90 s, past the keyed server's
  60 s relay) before the first chunk and `answers_within` between chunks. A stream gone quiet is
  given up as `Landed::Stalled`, counted `late`, provider noted `Away`, and **is no verdict on the
  song**: the next offer is taken and the want stays unstamped and due, never a miss
  (`a_stream_that_stops_sending_is_given_up_as_late_and_the_want_left_due`). A refusal is logged
  with the error it wraps (`Error::cause`). **A read carrying a provider's `Error` is that provider
  refusing**: `Pumped::next_chunk` takes it out of the `io::Error` as `Unopened::Refused`, so a
  download its provider failed part way counts `refused`, notes it `Away` where it says so, and is
  never a miss or a cut file. Bytes bounded by the vault's `LARGEST_DELIVERY`.
- **Keeping** is `Vault::keep_delivered`: a delivery meets the import's bit-exact, never-larger
  promise.
- **A delivery is weighed against the row's length.** Where the release row names one,
  `Want::lasts_as_long_as` takes a delivery only within `LENGTHS_AGREE_WITHIN`; further off counts
  `unkept`, the object left to `--prune`, a filed landing removed. `landed` answers a `Landed`:
  `Kept`; `Stalled` (above); `Unkept` (vault or folder failed, row held meanwhile); `NotTheSong` (wrong
  length, vault `Keeping::Refused`, filing too large or undecodable); `Unopened`. **On `NotTheSong`
  the offer goes to `refused_offers`** (want, `taken_from`, when) and the next
  held-back offer is taken; once spent, the want is asked again in the same claim with every heard
  provider in `Asking::passing`, so the provider registered after the wrong-song one is asked at
  once (`a_delivery_refused_from_one_provider_is_asked_of_the_next`). Rounds end at a keep, an
  unkept delivery or a round of nothing; the want is stamped once, a miss where nothing was kept and
  every round heard from everyone. **A refused offer is declined by the next poll too**:
  `declined_offer_rows` reads `refused_offers` beside `forgotten_deliveries` into `declined`,
  refusals older than `REFUSALS_REMEMBERED_FOR` (30 days) dropped
  (`an_offer_refused_as_not_the_song_is_declined_by_the_next_poll_which_starts_elsewhere`);
  `want_in` forgets a want's refusals
  (`a_refused_offer_is_offered_again_once_the_want_is_asked_for_again`). **A want replacing a
  lossy song keeps only a lossless delivery, vault and music folder alike**: `landed` answers a
  vault `Keeping::Kept` whose `codec` is lossy `NotTheSong` before its length is weighed, and
  `filed_for` probes the filed file before claiming album keys, removing a lossy one as
  `Unfiled::Lossy` (`NotTheSong` too), so the next offer is taken
  (`a_lossy_delivery_for_a_replacement_is_refused_and_the_next_provider_asked`,
  `a_lossy_file_filed_for_a_replacement_is_taken_away_and_refused`). Its delivery lands as a new
  track while the lossy song still holds the release row: a filing as a new rooted row, a vault
  delivery as a new rootless row, `note_delivered` passing over its `still_unheld` refusal for it
  (the `track_id IS NULL` pairing still leaves the row alone), and `wants.offered` names it either
  way. **The poll settles replacements once its lanes end**: `run` calls
  `rejoin_the_replacements` before the lanes start and, after the last `scanned_and_paired` and
  only where the poll was not cancelled, `settle_replacements`, whose `Replaced` is
  `PollSummary::replaced` (a failure traced at warn, nothing replaced), which the window's
  toast tells (`told::polled`) and whose folds it relocates its zones by
  (`a_lossless_delivery_filed_in_the_music_folder_replaces_the_lossy_song_at_the_polls_end`).
- **No vault open: a delivery is filed in the music folder.** `Library::deliver_into` names a
  `DeliveryFolder` (`music-folder`, `organise-as`), set by the binary at open and by the window
  before every poll and before it asks whether a want is due; with neither, a stream is `unkept` and a file only offered, except for a want replacing a lossy song, which nothing could keep and so is never asked of a provider: `Lanes::refill`, `is_a_want_due` and `next_want_due` pass it over while no vault is open and `delivering_into` names no folder (`KeptWhere`), so it is neither fetched nor due again every `POLL_AGAIN_AFTER` (`a_replacement_with_neither_a_vault_nor_a_music_folder_is_never_asked_of_a_provider`). `filed.rs` renders
  the path under the folder's `Naming`, stages bytes as hidden `.<name>.<pid>-<n>.resonate-delivery`
  beside it (at most `LARGEST_FILED`), noted in `staged_writes` before a byte is written so a poll
  killed mid-filing leaves a file the next poll sweeps (`organise::sweep_what_a_dead_writer_staged`,
  `rust-style.md`; `what_a_killed_poll_staged_in_the_music_folder_is_taken_away_by_the_next_poll`),
  lands by `hard_link` under the first free name `take_in::candidates` offers, tags via `FileTags`.
  A landing the decoder cannot probe, whose decode disagrees with the wanted length, or whose album
  keys cannot be claimed, is removed and counted `unkept`. **It joins the album it was wanted for**:
  `Library::claim_album_keys` names the album by the keys the scan will compute. As each filing is
  noted the lane scans the roots it landed under and `Library::pair_what_landed` pairs each unheld
  want with the rooted row at its offered path, playable before the keeper opens the next delivery
  (`a_delivery_with_no_vault_is_held_before_the_lane_opens_the_next`). A filing whose scan cannot
  start waits for the scan the poll runs at its end.
- **Kept becomes a track row.** `Library::note_delivered` writes the `vault_objects` row and a
  `tracks` row in one transaction and pairs the want's release track. The row is named by the
  object's own path with `vault_key`, `vault_path` set; names and identifiers are the release
  track's; genre, ReplayGain, words from `Kept::declared`. **No root** (`tracks.root_id` is
  nullable for this) and every pass over the user's files joins `roots`, so a scan's prune, forget,
  `organise`, `tag`, `vault --import`, `vault --release` never reach it. **A row held meanwhile
  keeps its pairing**: `note_delivered` checks inside its transaction that the row is still unheld,
  else writes nothing, answers `None`, counts `unkept`; a replacement's want alone is noted on a
  held row (above).
- **Forgetting a delivery.** `offered` is the object's URI; a want whose release track holds a row
  is `held`, never due again. `Library::forget_delivered` removes the rootless row a path names
  (`resonate forget`, the window's *Forget this delivery*), nothing a scan filed; the object is left
  to `--prune`. **What was forgotten is remembered; the want is due at once**: the same transaction
  resets its offer, `tried`, `misses` and adds a `forgotten_deliveries` row for **every delivery the
  object came from**: `vault_objects.taken_from` and each `object_deliveries` row (written by
  `note_delivered` for every delivery noted, cascading with the object), so a
  provider that brought the same audio later is not fetched from again
  (`a_delivery_forgotten_once_a_second_provider_brought_the_same_audio_is_declined_from_both`). The
  poll hands them to `Providers::first` as `Asking::declined`, which passes a matching delivery over
  (`Answer::declined`, never `refused`). A stream is declined by key alone; a file only where its
  later modification or status change is no later than the forgetting.

## Writing one

1. Crate `crates/providers/<name>/`, package `resonate-<name>`, on `resonate-core`,
   `resonate-providers` and what reaches its source. A network provider uses `ureq` through
   `resonate-fetch` (not `resonate-online`) and identifies itself per `online.md`.
2. A workspace member and `[workspace.dependencies]` entry.
3. A variant of `resonate_library::ServiceAccount` (what is typed) and of `Reachable` (what is
   built), with its readiness and its rank in `services.rs`, then a binary dependency and an arm of
   `Made::built` (`providers.rs`) and of the phones' `Downloads::reached`. **Which services a set of
   accounts makes, in which order, is decided once** (`resonate_library::planned`, `reachable`; pure,
   since the library may not reach a provider crate): blank fields trimmed away (a password kept as
   typed but never blank), a Subsonic needing server, user and password, a Jellyfin a server and
   either a password or a whole kept session, a TIDAL a client id and refresh token, a hifi-api
   account's key the `resonate` service *beside* whatever its server names (a hosted one only while
   `HostedServices::Allowed`), then the fixed rank (Subsonic, Jellyfin, TIDAL, `resonate`, hifi-api,
   Monochrome, YouTube Music), the first account of a scheme kept;
   `Reachable::fetches_for_wants` is false for Jellyfin alone
   (`a_key_is_the_resonate_server_beside_whatever_hifi_api_the_account_names`,
   `services_are_planned_in_one_order_whatever_order_the_accounts_were_given_in`). The binary's
   `Accounts` is that plan: `Accounts::of` off `Config` for a headless run (`providers::registered`),
   `Accounts::given` off the window's live `resonate_ui::Online` (`providers::sourced`, via
   `resonate_ui::Sourcing::register`); `registry` (after the inbox, `with_inbox`) and `in_place` each
   walk it, so a changed setting applies from the next poll. `Made` returns the same provider
   (`Kept`) until its login changes. A provider with a setting of its own widens `Online`, `SettingsFields`
   and the plan together.
4. `Error::Io` names the provider and a `ProviderOp`; an error the seam lacks a variant for is added
   to the seam with its op, never as prose.

**The window polls on its own** as well as on *Poll now*: when the soonest want is due
(`Shelves::next_try`, `LibraryModel::ask_when_due`, bounded by `FIRST_ASKED_AFTER`,
`RETRIES_ASKED_AT_LEAST`, `ASKED_EVERY`) and when a want is marked, each only where a provider is
registered and `Library::is_a_want_due`. A want marked while another pass holds the library is owed
(`LibraryModel::poll_owed`, taken up by `take_up_what_waited`); a poll refused because another
process holds the poll lock is owed too, retried `OWED_ASKED_AGAIN_AFTER` (30 s) later. *Poll now*,
the inbox poll and `resonate poll --again` use `PollOptions::ASKING_EVERY_WANT`; the timer and bare
`resonate poll` keep `Want::due_at`, sparing network services. `PollProgress::{asking,
asking_provider, received}` feed the window's *Downloading…*, read every frame: the provider under a
`parking_lot::Mutex` held for a clone, the count an atomic. A `Delivery::File` counts its whole
length when the vault takes it up
(`a_file_delivered_into_the_vault_counts_the_bytes_it_was_as_received`).

**The window watches the inbox folder** via `RootsWatch`: once a write under it is quiet for
`INBOX_QUIET_FOR`, it polls as `Prompted::ByTheInbox`, raising no notice. **What landed while no
window was open is asked about on open**: the folder's newest file (later of modification and change
time) is weighed against `Library::last_tried` (`landed_since`).

## What every network provider shares

`resonate-fetch` (on core, the seam and `ureq` alone; the layering job holds it) is the HTTP
plumbing, so a `ureq` bump is made once:

- **Agents**: `USER_AGENT`, `Patience` (`answered_within`, `broken_off_after`, `resumed_after`),
  `Trusted` (`BuiltInRoots`, or `SystemStoreToo` for a listener's own server: `trust.rs` builds the
  policy from the built-in roots beside the system's through `built_in_beside`, so a test holds a
  private root as the system's own and reaches a TLS server signed by it,
  `a_server_signed_by_a_private_root_is_reached_once_the_system_holds_that_root`, which the built-in
  roots alone refuse, `a_server_signed_by_a_private_root_is_refused_by_the_built_in_roots_alone`), `asking_agent`
  (`timeout_global` of `answered_within`), `downloading_agent` (`stall.rs`'s `BrokenOffAfter`: a
  silent body closes after `broken_off_after`, one still delivering is read however long; no
  whole-body deadline, and the response phase keeps the `answered_within` the agent was configured
  with, so a first answer slower than the stall opens). Every agent carries `timeout_resolve`, and
  `downloading_agent` resolves on a helper thread of its own (`agents.rs`'s `Bounded`), so a host
  lookup stuck in the system resolver ends at the budget on a new request and on a reopening alike,
  while a download already moving runs as long as it delivers. **A downloading agent asks for
  uncoded bytes on every request**: `downloading_agent` takes the caller's `ConfigBuilder` and sets
  `accept_encoding` to identity before building, so the first request of a stream a provider makes
  itself (Jellyfin's, TIDAL's one FLAC file, the hifi relay's `/file/`) is asked as its reopenings
  are, and its answer carries the uncoded `Content-Length` and the validators a range answer is
  checked against; a provider cannot forget it, since every stream and download goes through such
  an agent
  (`a_stream_a_downloading_agent_opens_is_asked_uncoded_from_its_first_request_and_seeks_within_its_length`,
  `a_song_from_a_server_offering_gzip_is_asked_uncoded_from_the_first_request_and_seeks_within_its_length`
  in `resonate-jellyfin` and `resonate-tidal`).
- **No agent follows a redirect itself; a GET follows one through `followed`, within the origin it
  asked.** `configured` sets `max_redirects(0)`, so `asking_agent`, `downloading_agent` and every
  agent a provider builds hand a `3xx` back. `followed(url, asked)` takes the request as a closure
  of the address, so each hop is asked again with every header and per-request setting the first
  carried (`Range`, `If-Range`, `Authorization`, `Accept-Encoding`, the deadline), and follows a
  301, 302, 303, 307 or 308 only to the same host: never from https to plain http, the port changing
  only with a move from http up to https, at most `REDIRECTS_FOLLOWED_AT_MOST` (5) hops, a relative
  `Location` resolved against the address asked. Anything else is refused before a byte is sent
  elsewhere, as an `Unfollowed`: `ElsewhereThanAsked` (another host or port, or a scheme other than
  http's two), `Downgraded`, `Unlocated` (no `Location`, or one that does not read as an address)
  or `Endless`; `followed` answers it as `Unanswered::Redirected` beside `Unanswered::Failed` (a
  `ureq::Error`), and `unreached` makes it `Error::Redirected { redirect, .. }`, the provider away,
  since a server sending its answers to another host does so for every want
  (`a_redirect_to_another_port_is_refused_and_never_asked`,
  `a_redirect_is_refused_off_its_host_or_port_and_down_to_plain_http`,
  `a_redirect_is_followed_on_its_host_and_port_and_up_to_https_on_its_host`,
  `redirects_past_the_most_followed_are_refused`). Why: a server that answers `302` to loopback, a
  LAN address or plain http would otherwise have its choice read as audio, a cover or JSON, again on
  every seek. `Ranged` asks each resume through `followed` (so a seek is followed afresh from the
  original address), and a refused one is `Unfetched::Redirected`, not retried and read as
  `ErrorKind::PermissionDenied` carrying the `Unfollowed`
  (`a_resume_redirected_within_its_origin_is_asked_there_with_its_range_and_validator`,
  `a_resume_redirected_to_another_origin_is_refused_once_and_never_asked_there`). Every GET of
  Subsonic, Jellyfin and Emby, Monochrome, TIDAL, hifi-api (hosted, keyed or not) and YouTube
  Music's client version goes through it, TIDAL's media and relay agents included; a POST or DELETE
  follows none, its `3xx` a refusal. Each provider holds it with a fake server answering `302` to a
  second loopback port that is never asked (`a_stream_redirected_to_another_port_is_refused_and_that_port_is_never_asked`,
  `a_stream_or_a_cover_redirected_to_another_port_is_refused_and_that_port_is_never_asked`,
  `a_search_or_a_download_redirected_to_another_port_is_refused_and_that_port_is_never_asked`,
  `a_hifi_api_answer_redirected_to_another_port_is_refused_and_that_port_is_never_asked`) and a
  same-server redirect still followed (`a_stream_redirected_within_the_server_is_followed_and_a_seek_asks_there_from_its_place`,
  `a_hosted_hifi_answer_redirected_within_its_server_is_followed_with_its_token`). The MusicBrainz
  export's agent (`musicbrainz.md`) alone puts ureq's own following back.
- **`unreached` classes a `ureq::Error` once**: a refused certificate is `Untrusted`; I/O, timeout,
  unknown host, connect/proxy/TLS failure is `Io` (away); `BadUri`/`Http` is `Unaddressable` (away);
  a body over its limit, protocol error or redirect failure is `Unreadable`, the want's
  (`an_answer_too_large_or_malformed_is_unreadable_and_no_refused_connection`); a refused redirect
  is `Redirected` (`a_redirect_refused_is_the_provider_away_and_denied_to_a_reader`). `as_io` is the
  same reading for a `Read` path.
- **`retry_after` reads seconds or any of HTTP's three date forms** against the clock; a past date
  is no wait. The date's parts are checked (a day within its month, a leap day only in a leap year, a
  four-digit year but in the two-digit form), and a two-digit year is the latest one not more than
  fifty years ahead of the clock (RFC 9110 §5.6.7); a date that fails is `None`, so the caller's own
  backoff stays. `resonate-online` reads it too.
- **`Ranged` is a download that resumes** with `Range: bytes=<read>-` and `Accept-Encoding: identity` (every agent has ureq's `gzip`, which adds `Accept-Encoding: gzip` unless the request names one and decodes by the response's `Content-Encoding`, while a range counts the bytes of the coded representation; `asking_to_resume` and `Ranged` ask identity on every request, `a_download_is_asked_for_uncoded_so_a_resume_counts_the_bytes_the_server_ranges_over`), up to `RESUMES_AT_MOST` (5)
  times running: a `206` whose `Content-Range` continues from there is read on, a `200` is read past what was
  held, another refusal ends it. A resume carries `If-Range` with the first answer's strong
  tag and nothing else (`strong_tag`; a weak tag or a date never, since RFC 9110 lets a date stand
  in `If-Range` only with evidence it is strong: `if_range_carries_only_a_strong_tag`), keeps the
  validators (`Ranged::validators`, the first answer's joined with each later one's, whichever way it
  was opened) and holds the answer to the total it has seen. **A `206` is one interval**:
  `Interval::read` takes `Content-Range` whole (`bytes first-last/complete`, `complete` a number or
  `*`), refusing a range whose last byte is before its first or at or past its complete length, and
  the answer continues only where the interval starts at the byte asked from and the body's
  `Content-Length`, where sent, is the interval's length; the body is read no further than the
  interval, so a shorter valid interval is read and the rest asked for again, and anything else is
  `Unfetched::Misranged`, read as `ErrorKind::InvalidData` and never retried
  (`a_range_whose_bounds_disagree_is_no_interval`,
  `a_resume_whose_interval_disagrees_with_its_body_or_its_whole_is_refused`). A `206` or `200`
  naming another total, validators that conflict, or a `200` where a strong tag is held whose
  validators do not vouch for it (a `200` answering `If-Range` meaning the file was replaced) is
  `Unfetched::Changed`, not retried and read as `ErrorKind::InvalidData`, so a file a server
  re-transcoded meanwhile is never joined old head to new tail; so is a `200` whose
  `Content-Length` is under the place asked from, which cannot hold what was read
  (`a_whole_answer_shorter_than_the_place_asked_from_is_another_file`), and a later answer bearing only the
  held date is not taken for a change
  (`a_resume_bearing_only_the_date_of_a_tagged_first_answer_is_read_on`,
  `a_resume_naming_another_validator_of_the_same_kind_is_refused`,
  `a_file_replaced_between_the_break_and_the_resume_is_not_joined_old_head_to_new_tail`). The first resume is at once, each later one waits `waited_before`
  (`resumed_after` doubling, at most 8 s), or a 429/502/503/504's `Retry-After`. `Ranged::opened`
  asks a first request answered 429/502/503/504 again the same way. **A body that ends under its
  total is a break-off too**: a server capping each `206` (an open range answered with a slice) ends
  an answer at `Content-Range`'s end, so a read of `0` while `read` is under `total` asks again from
  `read`, `RESUMES_AT_MOST` asks with no byte between them ending in `ErrorKind::UnexpectedEof`
  (`a_server_capping_every_partial_answer_still_yields_the_whole_body`,
  `a_server_answering_nothing_more_under_the_total_ends_in_an_error_after_a_bounded_number_of_asks`).
- **A `Held` that can be checked is asked with `Range` and `If-Range`** (`asking_to_resume`; a partial
  is checkable by a strong tag or a known total, never by neither and never empty). The answer
  is read by `Ranged::continuing_held`: a `200` begins again from 0; a `206` is taken only if its
  `Content-Range` is an interval starting at `bytes` and framed as its body is, where a total was
  held names it, and its validators pass the same check a resume's do against the held ones (a
  conflicting tag or date refused, a new kind joined), so a saved partial is never handed on with
  another file's tail; anything else is `None`
  and the provider asks again with no `Range`, as it does for a `412` or `416`. The `Ranged` it makes
  counts from `from`, so a break-off later resumes at the absolute offset (`Ranged::into_resumed`)
  (`a_held_partial_answered_with_a_conflicting_strong_tag_is_not_continued`,
  `a_held_partial_answered_with_an_inconsistent_interval_is_not_continued`,
  `a_keyed_download_opened_with_what_is_held_is_asked_from_there_and_read_on`,
  `a_keyed_download_whose_length_is_not_what_is_held_is_read_again_from_the_start`).
- **`Seeking` is a stream a decoder can seek**: it reads on the first answer, and a seek moves
  only its place; the next read asks `Range: bytes=<place>-` (`Ranged::opened_bearing`, so it resumes
  as any `Ranged` does), carrying the first answer's validators and total as a `Known` (`If-Range`
  and the `Changed` guard, so a file replaced between two seeks is refused, never joined old to
  new: `a_file_replaced_between_two_seeks_is_not_joined_old_to_new`). A seek back to byte 0 asks
  with no `Range` and is held to the same `Known`, so a probe rewinding over a file replaced
  meanwhile reads `InvalidData` and an unchanged one plays from the start
  (`a_rewind_after_the_file_was_replaced_reports_a_changed_stream`,
  `an_unchanged_file_rewound_plays_from_the_start`), except a seek at most `SKIPPED_RATHER_THAN_ASKED` (256 KiB) ahead, which
  reads on and discards (a skip that fails drops the reading in hand, so the seek's error leaves the
  place where it was and the next read asks from there:
  `a_seek_whose_skipping_copy_fails_leaves_the_place_where_it_was_and_reads_on_from_there`). Its length is the first answer's `Content-Length`; a read at or past it
  answers nothing without asking (`a_seek_a_little_ahead_reads_on_rather_than_asking_again_and_one_past_the_end_reads_nothing`).
  A server ignoring `Range` is read from its start past the place (`asked`'s `WHOLE`). `Seeking::bearing`
  sends a key on the reading in hand and on every range asked after (`Ranged::opened_bearing`).
- `escaped` (every byte but the unreserved percent-encoded), `is_a_document` (`text/`, JSON or XML:
  an error document, never a song).
- **One odd listing never fails the answer**: each provider reads its list through a `*_readable`
  deserializer taking `null` as empty, passing over an item that does not read, and an id as a
  string or a number (`a_numeric_id_is_read_an_odd_listing_passed_over_and_a_null_list_is_none`).

## The inbox

`resonate-inbox` is the reference provider: `Inbox::at` over the folder the `inbox` key names, read
never written. A file directly inside whose stem is the recording MBID, then track MBID, then ISRC,
ignoring case, matches. Only audio is offered (`resonate_core::names_audio` over `AUDIO_EXTENSIONS`):
a `.cue`, `.jpg` or rip log is never delivered ahead of it. **A symbolic link is passed over**: the
listing skips an entry whose own type is a link and `settled` looks at the file with
`symlink_metadata`, so a link named for a recording delivers nothing, even one swapped in after the
listing was read
(`a_link_named_for_a_recording_is_never_delivered_whatever_it_points_at`,
`a_file_swapped_for_a_link_after_the_listing_was_read_is_not_delivered`).

- **Folder read once a poll**: the listing is held with the folder's modification and change times,
  reread only if either moved or it is older than `LISTING_TRUSTED_FOR`.
- **Most exact name wins; of one name's files, the lossless one**: sorted by `Fidelity` (`Lossless`,
  `LosslessOrLossy`, `Lossy`) then name, so `<mbid>.flac` beats `<mbid>.mp3` while a recording's
  `.mp3` still precedes an ISRC's `.flac`.
- **A file still being copied in is not delivered.** If the best-named file's modification or change
  time is within `SETTLES_FOR` of now it answers `Error::StillArriving`, and no lesser file is
  delivered instead. The change time is the one a copy cannot keep old (`cp -p`); a future time holds
  nothing back. **Quiet is not enough: the size and both times must also be unchanged between two
  looks `SETTLES_FOR` apart.** `Inbox` keeps a `Sighting` (size, stamp, when first seen so) per
  file asked for, reset whenever either moved, so a torrent client pausing between writes is not
  taken for done; a file quiet for `SETTLED_ALONE_AFTER` (60 s) needs no earlier look, so a file
  already settled is delivered at once
  (`a_file_quiet_when_first_seen_is_delivered_only_once_it_stays_unchanged`,
  `a_file_that_grew_between_two_looks_is_seen_afresh`).

## A Subsonic server

`resonate-subsonic` reaches the listener's server named by `subsonic`, `subsonic-user`,
`subsonic-password`; `providers::registry` registers it after the inbox only where all three are
given and `online` is on.

- **Asked by recording, then by the seam's choice.** No recording MBID and not `may_be_listed`:
  `Nothing`, no request. Else `search3` by each of `Identity::wordings` (title alone last),
  `SONGS_A_PAGE` at a time up to `PAGES_AT_MOST`. A song whose `musicBrainzId` is the recording is
  taken at once; else the songs go to a `Choosing`.
- **The log never sees a request's address.** Every URL carries the user, token and salt and ureq's
  `BadUri`/`Http` errors print all of it, so `told` logs those two as a sentence alone
  (`an_address_without_a_scheme_is_never_told_to_the_log_with_its_token`).
- **Forgetting a session logs it out** (`Jellyfin::log_out`, `POST Sessions/Logout` with the token in
  its header): the held session, or a kept one built without asking the server, and nothing at all
  where none is held, so a fresh client never signs in only to leave. The binary's `resonate jellyfin
  --forget` and the window's forget, a typed password or a changed address
  (`SignsInToJellyfin::logged_out`, given the session held before the fields moved, run off the
  window's thread) call it first, and a log-out the server refuses or never answers is told to the log
  and never stops the forgetting (`a_kept_session_is_logged_out_with_its_token_in_the_header`,
  `forgetting_goes_on_when_the_server_cannot_log_the_session_out`).
- **A deadline for an answer, for a download silence alone.** API requests carry
  `Patience::answered_within` as `timeout_global`, and the whole search is held to it, each page
  asked within what is left (`Error::Io` `TimedOut` once spent). A download's agent bounds each read
  alone. The connector is ureq's `unversioned` API, outside its semver promise; weigh a ureq bump
  against the stall tests.
- **A download that breaks off is resumed** through `Ranged`
  (`a_download_cut_off_part_way_is_asked_for_again_from_where_it_stopped`).
- **Private CA reached.** Both agents trust `Trusted::SystemStoreToo`; a refused certificate is
  `Error::Untrusted`.
- **It delivers the server's original file, only a file**, keyed by song id, hinted by `suffix`. The
  API answers a failed download with its error document and a 200, so a download whose
  `Content-Type` names text, JSON or XML is read as that document, never streamed.
- **Paces itself**: requests `ASKED_APART`; 429/503 asked again after `Retry-After` (capped
  `LONGEST_RETRY_AFTER`) up to `RETRIES_AT_MOST`, then the `Refused` it was. The seam's `Pacing`
  reserves a turn under its lock and sleeps towards it outside; a server-requested wait is
  `Pacing::cool_for`, held by every later caller
  (`turns_are_reserved_apart_and_nobody_waits_on_the_lock_while_another_sleeps`,
  `a_server_asking_to_be_asked_later_holds_back_every_caller`). TIDAL's `Asker` and Monochrome use
  the same type.
- **The password never leaves as typed.** Each request carries user, a fresh salt (a v4 UUID, 128 bits drawn from the OS, never derived from the clock or the pid:
  `a_salt_is_a_hundred_and_twenty_eight_bit_draw_and_no_two_repeat`) and `t` = MD5 of
  password and salt; User-Agent bare; `Server`'s `Debug` prints `<withheld>`. `status: failed` is
  `Error::Unwelcome` where the code concerns the account (`ACCOUNT_REFUSALS`), else
  `Error::TurnedAway` (70, a song the server lacks).

## A Subsonic server played in place

The same server is also a source of its own: its catalog is the server's, browsed through the API,
never copied into `resonate-library` (the local source's catalog alone). `SourceId` `subsonic`.

- **What the server holds is named by a location** (`resonate_providers::Named`, the seam's
  `in_place.rs`, re-exported by `resonate-subsonic`): `subsonic:track/<id>` a song,
  `subsonic:album/<id>` and `subsonic:artist/<id>` what they hold, and `<scheme>:mix/<id>` a mix
  (only a service that has them names one; Subsonic's `named_by` never reads a mix), the key read
  back by `Named::read` (`a_song_album_or_artist_is_named_by_a_location_that_reads_back_as_itself`).
  Only a song is ever a queue row. The vocabulary (`Id`, `Named`, `Artist`, `Album`, `Mix`, `Song`,
  `ArtistAlbums`, `AlbumSongs`, `MixSongs`, `Found`, `Streamed`) and the `PlaysInPlace` trait are the
  seam's, so every service played in place speaks it (*Other services played in place*, below). A
  `Song` says whether the service holds it as a favourite in `favourite` (`Some` only where the
  service's own answer says so: Subsonic's `starred`, TIDAL's favourite songs on the first page;
  `None` is unknown, never no).
- **Browsing is four calls**: `artists` (`getArtists`), `artist` (`getArtist`, its albums), `album`
  (`getAlbum`, its songs), `search` (`search3`, up to `BROWSED_AT_MOST` of each kind), with `song`
  (`getSong`); `songs_of` turns any `Named` into its songs in the server's order, an artist's
  album by album through `resonate_providers::songs_of_an_artist`, which refuses an artist of more
  than `ALBUMS_OF_AN_ARTIST_EXPANDED_AT_MOST` albums before asking any (`Error::TooManyAlbums`),
  stops once the songs pass `SONGS_OF_AN_ARTIST_EXPANDED_AT_MOST` (`Error::TooManySongs`) and
  checks the cancel `songs_until` takes between albums (`Error::Cancelled`), the same for Jellyfin
  (`an_artist_of_five_thousand_albums_is_refused_before_any_album_is_asked`,
  `an_artist_whose_albums_hold_past_the_most_songs_stops_at_the_album_that_passes_it`,
  `a_cancel_between_albums_stops_an_artists_expansion`). Each is one paced request under `Patience::answered_within`, `ProviderOp::Browse`,
  the answer read part by part (`part`) through the same `readable`/`id_spelt` tolerance as a search,
  a number written as text read too (`a_servers_artists_albums_and_songs_are_browsed_by_their_ids`,
  `an_album_or_an_artist_named_by_its_location_is_the_songs_it_holds_in_order`). The seam's
  vocabulary is built by hand from private documents, as every provider's is.
- **A song is streamed as stored**: `streamed` asks `stream` with `format=raw`, so the server never
  transcodes and a lossless file stays bit-accurate; an answer naming a document is the refusal it
  carries (`ProviderOp::Stream`). The `Streamed` it answers names the `Content-Type` (the decoder's
  hint, since the key carries no extension) and is `Streamed::seeking` over a
  `resonate_fetch::Seeking` (`a_song_is_streamed_as_stored_and_a_seek_back_asks_the_server_from_there`).
- **Every service played in place is a `MediaProvider` through `resonate_library::PlayedInPlace`**
  (`played_in_place.rs`; no provider crate may see `resonate-codec`, and the library reaches both
  seams), one per scheme, which opens a song through whichever service the closure it was made with
  finds for the scheme, so the binary and the phones share it over their own slots. The binary's
  (`served.rs`, under `online`): `Served::of(config, settings)` holds
  the `PlaysInPlace` services `providers::in_place_named` builds from the same settings and `online`
  switch the provider registry reads, in the registry's order; `Served::sources` adds one
  `PlayedInPlace` per service's scheme to every `Sources` that plays (`playing_from`: `play`, the
  window and its zones) and to the one reading arguments, so a `subsonic:` (or `tidal:`, …) URI is
  the service's only where it is set (`a_song_uri_is_a_file_here_unless_a_server_is_named_and_then_it_is_queued_as_named`,
  `a_uri_of_a_service_not_set_is_a_file_here`). A stream is seekable where `Streamed::is_seekable`
  (a range reader of known length), so `Sources`' deadlines apply and nothing is spooled; one read
  onward is spooled by the codec (`audio.md`); one that would not open is `codec::Error::Io` carrying
  the seam's error. `Served::songs_at` expands an album or artist argument into its songs as
  `queue_items_served` reads it
  (`an_album_on_the_server_is_queued_as_its_songs_and_each_decodes_as_streamed_bit_for_bit`,
  `a_streamed_song_seeks_back_and_decodes_on_from_where_it_landed`). A queued row's tags are what
  `Player::media` probes off the stream's head.
- **A cover is the server's `getCoverArt`**: `Artist`, `Album` and `Song` carry `cover` (the
  `coverArt` name the server gave, read as a string or a number), `Subsonic::cover(&Id, side)` asks
  for it at a size (`ProviderOp::Cover`, one paced request under `answered_within`, an error
  document the refusal it carries, at most `LARGEST_ANSWER`), and `Subsonic::cover_of(&Named)` is
  the name to ask by: a song's is read off `getSong` (a server's `coverArt` for a song is not its
  id, and some refuse the bare id), an album's or artist's is its own id
  (`a_cover_is_asked_for_by_the_name_the_server_gives_it_and_a_song_is_asked_what_that_is`,
  `a_cover_the_server_does_not_hold_is_the_refusal_it_carries`).
- **The window's accounts are live, the command line's are fixed.** `Served::for_the_window` holds
  the services in a slot (`Arc<RwLock<Vec<Arc<dyn PlaysInPlace>>>>`) its `PlayedInPlace` providers read on every
  open, one for each of `PLAYED_IN_PLACE` (`subsonic`, `tidal`, `resonate`, `hifi-api`,
  `monochrome`) always registered in the window's `Sources`; `Served::serving()` is the
  `resonate_ui::Serving` seam, which on each call builds the services from the window's live
  `Online` (`providers::in_place_given`, the same `Accounts::given` the registry reads, so online off
  or a blank field is none), writes the slot and answers a `ServedBy` per service (the
  `resonate_ui::Serves` adapter: browse, search, expand a location to its songs, covers), the same
  `Arc` while the service is (`a_window_offers_every_service_set_and_keeps_each_while_its_settings_stand`).
  An account typed, changed or cleared in Settings therefore plays at once, and `resonate play` and
  `queue` (`Served::of`) still register only what the settings named at start
  (`a_window_plays_a_server_once_an_account_is_given_and_not_before`). **One `Made` per process**
  (`providers::made_once`): the window's registry, its served services and every `Served::of` take
  their providers from the same `Kept` cells, so one TIDAL account is one `Tidal` signing in once,
  and a refresh token it rotates is handed to `Renewed` once rather than left stale in a second
  copy. The window's pane is `ui.md`'s *The server pane*; the phones' boundary is `mobile.md`'s
  *Services played in place*; the terminal's is `resonate served` (`binary.md`).

## A Jellyfin or Emby server played in place

`resonate-jellyfin` reaches the listener's server named by `jellyfin`, `jellyfin-user`,
`jellyfin-password`, one `Jellyfin` for both products (`Flavour`, read off where the sign-in
answered). It is a source played in place beside Subsonic and no `Provider`: nothing is asked of it
for a want, no delivery is made, and `providers::registry` never registers it; `providers::in_place`
does, after Subsonic and only where all three are given and `online` is on. `SourceId` `jellyfin`:
`jellyfin:track/<id>`, `jellyfin:album/<id>`, `jellyfin:artist/<id>`; a mix is never named. The
catalog is the server's, browsed through its API and never copied into `resonate-library`.

- **The password is sent once, to sign in.** `POST Users/AuthenticateByName` with `{"Username",
  "Pw"}` in the body and the `Authorization: MediaBrowser Client="Resonate", Device="Resonate",
  DeviceId="<id>", Version="<crate version>"` header (also as `X-Emby-Authorization`, which Emby
  reads); the answer's `AccessToken` and `User.Id` are the `Session`, held under a `Mutex` so
  concurrent callers sign in once. **The device id of a password sign-in is the install's own**: a random v4 UUID
  minted once and kept where the install keeps its Jellyfin state, so the server sees one device per
  install however often the player starts, and two installs signing in as one user (the desktop and
  a phone, two desktops) never share one, since the server revokes the token an earlier sign-in
  as the same device held. `Jellyfin::keeping_device` takes the stored id (an account's `device`
  with no token beside it, `JellyfinLogin::install_device`) and otherwise tells the id it minted to
  the caller, which keeps it; a kept session and a Quick Connect attempt are left as they were. The
  binary keeps it as `jellyfin-device` (`Renewed::keep_device`, a settings file that lacks it is
  given one on first use, so older files load); a forgotten session clears it with the rest and the next sign-in mints another. The phones
  pass the account's `device` through; `Downloads::reached` remembers the id it mints per server and
  user and hands each new one to `minted_jellyfin_devices` (a `MintedJellyfinDevice`), which Android
  (`ProviderAccountBook`) and iOS (`ProviderAccounts`) store as the
  account's `device` beside no token, so a phone signs in under one id for as long as the account
  stands (`a_phone_signing_in_twice_to_jellyfin_sends_one_device_id`); it is never a name, host or setting, and the User-Agent stays
  bare. A kept session names the device it was issued to
  (`a_password_sign_in_names_the_device_this_install_keeps_on_every_launch`,
  `two_installs_signing_in_as_one_user_name_devices_of_their_own`,
  `a_jellyfin_password_account_keeps_the_device_it_mints_and_reuses_the_one_kept`).
  **Every later request carries the token in a header**, `Authorization` and `X-Emby-Authorization`
  with `, Token="<token>"` added to the sign-in's value (`Jellyfin::bearing`), never in the query: only
  the stream's address, which a `Seeking` asks again by alone, carries `api_key`
  (`Session::reopenable_url`;
  `the_token_is_in_a_query_only_where_a_stream_is_reopened_by_its_address`). No error's text carries the
  token (`no_error_names_the_token_whichever_way_a_request_failed`). A 401 on a request drops the
  session, signs in again and asks once more; a second is the `Refused` it was. A 401 or 403 at the sign-in is `Error::Unwelcome` under
  `ProviderOp::SignIn`. `Server`'s `Debug` prints `<withheld>`, `Session` has no `Debug`, and
  `told` keeps ureq's address-printing errors out of the log, as Subsonic's does
  (`an_address_without_a_scheme_is_never_told_to_the_log_with_its_token`).
- **Quick Connect signs in with no password** (`quick_connect.rs`, Jellyfin 10.8+): `QuickConnect::at`
  asks `QuickConnect/Enabled` (`offered`: false for an address under `/emby`, a refusal below 500 or
  any answer but `true`, so a proxy's page is no offer), `begun` posts `QuickConnect/Initiate` with the
  `MediaBrowser` authorization (`Call::SigningIn`; a `405` asks again by `GET`, `Call::Introducing`)
  and answers the `Code` (`Approving`'s `Debug` withholds the secret), and `approved` polls
  `QuickConnect/Connect?secret=` every `QUICK_CONNECT_ASKED_EVERY` (5 s) until `Authenticated`, then posts
  the secret to `Users/AuthenticateWithQuickConnect` and answers a `SignedIn`: the `KeptSession`
  (token, user id, and the device id the code was asked under) and the user's name. A secret the server
  forgot (404) or a wait past `A_QUICK_CONNECT_LAPSES_AFTER` (10 minutes) is `AuthorizationLapsed`, a
  401/403 at `Initiate` `AuthorizationDenied`, a cancel heard within a tenth of a second `Ok(None)`, and
  a poll the server did not answer is asked again (`a_code_approved_elsewhere_signs_in_and_yields_the_session_the_device_was_issued`,
  `a_code_never_approved_lapses_and_keeps_nothing`, `a_cancel_ends_the_wait_and_keeps_nothing`,
  `quick_connect_is_not_offered_by_emby_or_a_server_with_it_off_or_one_answering_a_page`,
  `an_older_server_is_begun_with_get`).
- **A kept session stands in place of the password.** `Server::kept` (`KeptSession`, its token
  withheld): the provider names the kept device in its authorization, takes the session without asking
  `AuthenticateByName`, and a 401 is `Unwelcome` under `ProviderOp::SignIn` (away), never a sign-in by
  name, since no password stands beside it
  (`a_kept_session_is_asked_with_its_token_and_never_signs_in`,
  `a_kept_token_turned_away_is_the_account_turned_away_and_not_a_new_sign_in`). The binary keeps it as
  `jellyfin-token`, `jellyfin-user-id` and `jellyfin-device` (`binary.md`); an account is ready with an
  address and either user and password or the whole session, and **a token and a password never both
  stand**: a Quick Connect sign-in clears `jellyfin-password` and writes the user's name into
  `jellyfin-user`, and a password typed, an address changed or the account forgotten lets the session go.
- **Emby is the same client under `/emby`.** The address is tried as given and, where the sign-in
  is answered 404 and the address does not already end in `/emby`, under `/emby`; the route that
  answered is the session's root and its `Flavour`. No other call differs, so Emby is no second
  crate or setting (`an_emby_server_that_lacks_the_plain_route_is_signed_in_to_under_its_own`,
  `an_address_that_names_the_emby_route_is_signed_in_to_there_alone`). Proved against a fake only:
  no real Jellyfin or Emby has been asked.
- **Browsing** (`ProviderOp::Browse`, each one paced request under `answered_within`, ids as
  strings or numbers, odd listings passed over by `readable`, as Subsonic's): `artists`
  (`Artists/AlbumArtists`, `ARTISTS_A_PAGE` at a time by `StartIndex` until `TotalRecordCount`,
  at most `ARTIST_PAGES_AT_MOST` pages), `artist` (`Users/<user>/Items/<id>` and the
  `MusicAlbum`s with `ArtistIds`), `album` (the item and the `Audio` items with `ParentId`, in
  disc, track and name order), `song`, `search` (`Artists?SearchTerm`, then `MusicAlbum` and
  `Audio` items by `SearchTerm`, `BROWSED_AT_MOST` each). **An id reaches a path or query only
  where it is letters, digits, dashes and underscores** (`item_id`; `Unreadable` otherwise, no
  request: `an_id_that_would_leave_its_path_is_never_asked`). A song's `suffix` is `Container`
  (`mov,mp4,m4a,...` read as the most trusted name in it), its length `RunTimeTicks` (100 ns), its
  `favourite` `UserData.IsFavorite`; a cover is the item's own where `ImageTags.Primary` is
  there, else its album's.
- **A song is streamed as stored**: `Audio/<id>/stream?static=true`, which transcodes nothing and
  asks for no download permission, a `Streamed::seeking` over `resonate_fetch::Seeking` with the
  `Content-Type` as the hint; an answer naming a document is `Unreadable`
  (`a_song_is_streamed_as_stored_and_a_seek_back_asks_the_server_from_there`). A cover is
  `Items/<id>/Images/Primary` at `maxWidth` and `maxHeight` of the side. The star is the server's
  own mark: `POST` and `DELETE Users/<user>/FavoriteItems/<id>`.
- **Paces and trusts as Subsonic does**: `ASKED_APART`, 429 and 503 asked again after
  `Retry-After`, `Trusted::SystemStoreToo`, a deadline for an answer and silence alone for a
  download, a refused certificate `Error::Untrusted`.
- **The binary and the phones treat it as every service played in place** (*A Subsonic server
  played in place* above): `PLAYED_IN_PLACE` holds `jellyfin` after `subsonic`, `Made::jellyfin` is
  its `Kept` cell, the window's Settings has a *Jellyfin or Emby server* group beside Subsonic's
  (`ui.md`), `resonate served --service jellyfin` browses it, and the phones' `MusicProvider`
  has `Jellyfin` (`mobile.md`).

## Other services played in place

TIDAL, hifi-api (hosted, a listener's own, or the keyed Resonate server) and Monochrome are sources
of their own beside a provider, as Subsonic is: each implements the seam's `PlaysInPlace`
(`scheme`, `title`, `artists`, `artist`, `album`, `mix`, `search`, `streamed`, `cover`, `cover_of`,
`favourites`, `keeps_favourites`, `favour`, `first_page`, `songs_of`, `named_by`, `location_of`), named by `<scheme>:track/<id>`, `<scheme>:album/<id>`,
`<scheme>:artist/<id>` with the scheme its provider's name (`tidal`, `hifi-api`, `resonate`,
`monochrome`), and registered by `providers::in_place` wherever its provider is registered, in the
same order. A service that lists nothing without being asked (every one but Subsonic) answers
`artists` empty; a browse its API does not offer is `Error::NotOffered` (never away, never the
want's). **`PlaysInPlace::plays_songs` (true by default) says a service's songs can be queued**:
YouTube Music's cannot, so `streamed` is `NotOffered` and every route honours it: the pane draws no
*Play*, *Play next* or *Add to queue* (`ServerModel::plays_songs`), `Served::songs_at` answers an
argument with no rows, `served::is_played_in_place` keeps the scheme off the bus's list and
`found_exactly` searches past it. `Streamed::seeking` is a reader a decoder may seek; `Streamed::onward` one it may not,
whose `Seek` is `Unsupported`; both are held under a `parking_lot::Mutex` so a stream holding a
`Receiver` is still `Sync` (`a_stream_read_onward_refuses_a_seek_and_says_it_cannot_and_one_of_known_length_seeks`).

- **A first page is what the listener keeps there.** `PlaysInPlace::first_page` is `artists` where
  the service lists any (Subsonic), else `favourites` (none by default). TIDAL's are the account's
  own, read from v1 `users/<id>/favorites/artists`, `albums` and `tracks` (`FAVOURITES_AT_MOST`
  each, newest first: `order=DATE&orderDirection=DESC`, each entry's `item` read as a browse's;
  `AsksV1::favoured_by`), the user id taken off the token's `user.userId` or `user_id`, else asked
  of `sessions` once and kept in the session (`Tidal::user`). The hosted hifi-api holds no
  listener's account and a listener's own hifi-api has no favourites route, so theirs is empty, as
  Monochrome's and the keyed server's are. **A TIDAL account's favourite mixes are on its first
  page too** (`Found::mixes`, a `Mix`: id, name, optional subtitle): v1 `users/<id>/favorites/mixes`
  with the same paging and order, each entry read bare or under `item`, an id of letters, digits,
  dashes and underscores alone since it goes into a path (`V1Mix::mix`; one that does not read is
  passed over, and an account the route answers 403 or 404 has none). A mix is named
  `tidal:mix/<id>` (`Named::Mix`), `PlaysInPlace::mix` lists its songs (`mixes/<id>/items`,
  `SONGS_OF_A_MIX_AT_MOST`, the entries' `item` read as a browse's), `songs_of` expands it, so a
  mix is browsed, played and queued as an album is; its cover is not offered. A service with no
  mixes answers `mix` with `NotOffered` under `ProviderOp::Browse`
  (`a_tidal_accounts_favourite_mixes_are_on_its_first_page_and_a_mix_is_browsed_and_expanded_to_its_songs`,
  `a_mix_id_that_would_leave_the_path_is_never_asked`,
  `a_mix_is_refused_by_a_service_that_names_none`,
  `a_tidal_accounts_first_page_is_its_favourite_artists_albums_and_songs`,
  `a_hifi_api_server_has_no_favourites_to_offer_and_lists_nothing_first`).
- **A song played in place is a favourite in the service's own mark.** `PlaysInPlace::keeps_favourites`
  (false by default) says a service has one and `favour(song, favoured)` writes it
  (`ProviderOp::Favourite`; `NotOffered` where the service has none): Subsonic's `star` and `unstar`
  (`Subsonic::favour`, one paced request under `answered_within`, the server's failed answer the
  refusal it carries), TIDAL's v1 `POST users/<id>/favorites/tracks` (form `trackIds=<id>&onArtifactNotFound=FAIL`)
  and `DELETE users/<id>/favorites/tracks/<id>` with the session's country, signed in and renewed
  as every request is (`Tidal::exchanged`, one loop for GET, POST and DELETE; a removal TIDAL
  answers 403 or 404 is already done, an addition it will not take is `TurnedAway` 404). The
  hifi-api, the keyed server and Monochrome hold no account and keep none. This is the service's
  mark, never the catalog's: nothing is rated into a file or synced
  (`a_tidal_song_is_marked_a_favourite_and_taken_out_through_the_accounts_own_favourites`,
  `a_song_tidal_will_not_add_is_the_refusal_it_carries_and_a_service_with_no_account_keeps_none`,
  `a_favourite_song_is_known_to_be_one_on_the_first_page_and_unknown_elsewhere`,
  `a_song_is_starred_and_unstarred_by_the_servers_own_calls_and_a_song_it_lacks_is_the_refusal_it_carries`,
  `a_song_the_server_has_starred_is_known_to_be_a_favourite_and_one_it_has_not_is_known_not_to_be`).
- **TIDAL's catalog is its v1 API** (`catalog.rs`, the `AsksV1` trait): `search?query=&types=ARTISTS,ALBUMS,TRACKS`
  (`BROWSED_AT_MOST` of each), `artists/<id>/albums`, `albums/<id>/tracks`, `tracks/<id>`,
  `albums/<id>`, `artists/<id>`, each with the session's country. A TIDAL account asks it with its
  own session (`Tidal::v1`, signed in and renewed as a delivery is); the hosted hifi-api asks it at
  `Hosting::tidal_api` with the hosted TIDAL token and country `US` (`hifi_served.rs`'s `Hosted`).
  A version is part of the title (*A Pillow of Winds (Remastered)*); a song's cover is its
  album's. A cover is TIDAL's picture service (`Endpoints::images`, `Hosting::images`,
  `resources.tidal.com/images/`): an `Id` `album:<uuid>` or `artist:<uuid>` (hex and dashes alone,
  so nothing else reaches the path) asked at the least side TIDAL draws that covers the one wanted
  (`ALBUM_SIDES`, `ARTIST_SIDES`; `a_picture_is_asked_for_at_the_least_side_tidal_draws_that_covers_the_one_wanted`).
- **A listener's own hifi-api is asked by its routes** (`hifi_served.rs`): `/search/?s=` (songs,
  `data.items`), `?a=` and `?al=` (artists and albums, read from `data` as a v1 search is),
  `/album/?id=` (`data` the album, `data.items[].item` its songs), `/artist/?f=` (`albums.items`,
  at the top or under `data`) and `/info/?id=` (a song's album cover), every answer read tolerantly
  (`an_album_the_server_answers_is_read_from_its_entries_and_its_own_name`). **The keyed Resonate
  server** reaches only `/search/` and `/file/` (`server.md`), so it is searched for songs alone and
  any album or artist is `NotOffered`.
- **A song is the lossless original, opened as a delivery would be, and seekable where one file
  holds it.** `streamed` asks for the playback manifest exactly as `find` does (`Tidal::media`,
  `HifiApi::media`: the same `FULL`, lossless, unencrypted, on-its-hosts checks), then
  `Player::streamed`: a BTS manifest naming one FLAC URL is a `Seeking` over it (Range seeks, the
  media agent, a redirect followed only within its origin); a DASH manifest is `Fetched` segments through `Remuxed` read onward.
  **A seek over segments is not offered, by design**: the remuxed FLAC's byte offsets are the sum of
  the `mdat` payloads before them, which only fetching every earlier segment tells, so a byte seek
  cannot be turned into a segment and an offset; the codec spools such a stream on disc and seeks
  once it has all arrived (FEATURES.md keeps a seek before then blocked on whether TIDAL's segment
  hosts answer `Range`, which would map them a head at a time; `audio.md`'s *Non-seekable sources are spooled*;
  `a_dash_song_played_in_place_is_read_onward_as_native_flac_and_never_seeks`). A manifest offering
  less than the whole lossless track is `NotOffered` under `ProviderOp::Stream`
  (`a_tidal_song_offered_only_in_part_is_not_streamed`). The keyed server's `/file/?id=` is a
  `Seeking` that carries the key on every range it asks (`Seeking::bearing`, `Ranged::opened_bearing`;
  `a_keyed_server_streams_its_file_with_the_key_on_every_ask_and_browses_by_search_alone`).
- **Monochrome is searched, never browsed**: `search` lists the playable songs of
  `/search/tracks` (artists joined, length from milliseconds); `/track/<id>` is a `Seeking`; an
  album or artist location is not one it names, and covers are `NotOffered`
  (`a_search_played_in_place_lists_the_playable_songs_and_a_song_streams_and_seeks_back`).
- Proved against loopback servers in `resonate-tidal`'s `tests/in_place.rs` and the binary's
  `served.rs` tests, which decode each service's song through `Sources` bit for bit and seek back
  (`a_tidal_album_is_queued_as_its_songs_and_a_song_in_one_flac_file_decodes_and_seeks`,
  `a_monochrome_song_decodes_bit_for_bit_and_seeks`,
  `a_song_the_keyed_server_sends_decodes_bit_for_bit_and_seeks_with_the_key`).

## A media server on the network played in place

`resonate-dlna` (`Dlna`) is the network's DLNA and UPnP media servers (MiniDLNA, Plex's or Jellyfin's
DLNA server, another Resonate offering its catalog) as one service played in place beside the
accounts: `SourceId` `dlna`, titled *DLNA* (a proper name, so no catalog word). It is no `Provider`,
never registered for wants and never asked for a delivery. **Where it lives is the layering's
answer**: the client (the search, the description, paged Browse, the DIDL-Lite reader, a song
fetched by range) is `resonate-upnp`'s (`upnp.md`'s *Media servers on the network, browsed*), since
that crate already hand-writes every part of it over plain HTTP; this crate only turns it into the
seam's vocabulary, so it reaches core, the seam and `resonate-upnp` (no `resonate-fetch`, `ureq`,
`rustls` or serde: `dependencies.md`, the `layering` job), and `resonate-upnp` stays blind to the
seam.

- **The tree is the seam's vocabulary bent as little as it will go.** A server is an `Artist` (its
  UDN the id, its `friendlyName` the name), its root's containers that artist's albums; every
  container is an `Album` (title, `upnp:artist`, `childCount`, `albumArtURI`) whose songs are the
  audio items directly in it, and whose own containers are `PlaysInPlace::albums_within` (the
  seam's one addition, empty for every other service), so a server's folders go as deep as it
  keeps them. An item is a `Song` (title, artist, album, track, length from its `res`, `suffix`
  from the type the decoder is hinted by). An album or song id is `<udn>/<object id>`, the object
  id the server's own spelling (`Object::read` splits at the first `/`, a UDN holding one being no
  server listed here); a cover is `<udn>/<address>`. `artists` (the first page) is one search
  round, so the list follows the servers as they come and go; `search` asks every server held
  (one round first where none is) for songs named, a server that refuses `Search` passed over.
  Browsing a container reads it once for its albums and its songs (`A_LISTING_IS_KEPT_FOR`, 10 s).
  An artist (a whole server) is never expanded into songs (`NotOffered`), nor is a mix named.
- **A song is played as stored**: `streamed` asks the item's `BrowseMetadata`, takes the `res`
  `Item::played_from` names (the original before a transcode) and answers `Streamed::seeking` over
  `Fetched`, its length the answer's (else the `res`'s size), its type the answer's `Content-Type`
  (else the `res`'s). A queued `dlna:track/…` is found again by its server's UDN in a later run
  (`MediaServers::server`), wherever the server now answers.
- **Failures say what happened**: a server that answers no search is `Error::Io` with
  `io::ErrorKind::NotFound` around `NoSuchServer`, a connection that fails `Io`, both away (the
  window's *The server could not be reached*, with a network server's own advice and *Search
  again*, `ui.md`); a SOAP fault `TurnedAway` with the UPnP code (701, no
  such object), an HTTP status `Refused`, anything unreadable `Unreadable`, an address off the server
  `Unaddressable` (`a_server_gone_is_the_provider_away_and_a_fault_is_the_servers_refusal`). A
  fault, an unreadable answer and a refusal not counted away are the window's *The server did not
  answer in a way that could be read*, with a network server's own advice and *Search again* too.
- **The window draws it as the network** (`PlaysInPlace::lists_the_network`, true here alone, which
  the window's adapter forwards): `ui.md`'s pane reads its first page only once shown and names its
  pages as servers and folders.
- **Asked only when the listener asks, and only with `online` on.** `providers::in_place` appends
  the one `Dlna` a process holds (`Made::media_servers`, so the servers found are kept between a
  pane's reads) wherever `online` is on, after the accounts; it searches nothing until it is asked
  for a page, a search or a song, so nothing is multicast unasked. `PLAYED_IN_PLACE` holds `dlna`.
- **Proved over loopback** (`tests/server.rs`): `resonate-upnp`'s own `MediaServer` answers the
  search and is listed by its typed name, browsed from its top to an album's songs, a song streamed
  whole and from a seek, searched, its cover fetched, and gone once it leaves
  (`a_media_server_on_the_network_is_listed_by_name_and_browsed_down_to_its_songs`,
  `a_song_on_a_media_server_is_named_by_a_location_and_streamed_as_served_and_seeks`,
  `a_search_asks_every_server_and_a_cover_is_the_servers_own_picture`,
  `a_server_that_has_left_the_network_is_said_to_be_away_and_is_no_longer_listed`); the binary
  queues a server's album as its songs and decodes one bit for bit through `Served::sources`, seeking
  back (`an_album_on_a_media_server_on_the_network_is_queued_as_its_songs_and_each_decodes_bit_for_bit`).
  No MiniDLNA, Plex or Jellyfin server has been asked: their shapes are the fuzz seeds'.

## A TIDAL account

`resonate-tidal` is the listener's own subscription, named by `tidal-client-id`,
`tidal-client-secret`, `tidal-refresh-token`; `providers::registry` registers it after the inbox
and Subsonic only where client id and refresh token are given and `online` is on. A delivery is
fetched whole and lands as a track row.

- **Asked by link, then code, then name.** The linked TIDAL track (`Identity::track_on`) is asked
  first; failing that, the OpenAPI is asked for each of the want's codes (up to
  `CODES_ASKED_AT_MOST`) and a listing taken only where its own `isrc` is that code, at most
  `TRACKS_TRIED_AT_MOST` in all (`tracks?filter[isrc]` has no text search). **Where no code lists
  anything, or the want has none, the want is searched by its words** (`searchResults/{words}` with
  `include=tracks,tracks.artists`, JSON:API, each of `Identity::wordings` until one holds a taken
  listing, `LISTED_AT_MOST` read from each) through a `Choosing`, as hifi-api does, since a
  re-release's codes are often not the ones MusicBrainz holds. `searched.rs` reads the document: the
  tracks relationship in TIDAL's order, each resolved in `included` with its artists' names and its
  ISO 8601 `duration` (`PT23M16S`; one that does not read is no length, so only a coded listing can
  be taken); an identifier or resource that does not read is passed over
  (`a_want_with_no_code_is_searched_by_its_name_and_only_the_track_named_alike_is_taken`,
  `a_want_whose_code_tidal_lists_nothing_under_is_searched_by_its_name`,
  `a_track_tidal_lists_under_another_artist_or_another_length_is_never_taken_by_its_name`,
  `a_search_result_that_does_not_read_is_passed_over_and_the_rest_taken`). Not `may_be_listed`:
  `Nothing`, no request, no sign-in.
- **It signs in with the listener's refresh token alone.** No client id or secret is built in. 400/401
  at the token endpoint is `Error::Unwelcome` under `ProviderOp::SignIn` (away). **A rotated token is
  handed back**: the provider signs in with the new one and calls what `Tidal::telling` registered,
  once per rotation; the binary's `Renewed::note` writes it to `tidal-refresh-token` via
  `config::store` and remembers it against the token the settings gave, so a rebuilt provider uses
  the rotated token. **A token another process rotated is heard before a sign-in**: `Tidal::following`
  asks the binary's `Renewed::heard_elsewhere` first, which reads `tidal-refresh-token` again where
  the file's mtime moved (as Spotify's holder does), so the window and `resonate poll` do not each
  rotate the one they started with (`a_refresh_token_another_process_rotated_is_signed_in_with_instead_of_the_one_held`,
  `a_refresh_token_written_by_another_process_is_heard_once_the_file_moves`). An API 401 signs in again once and asks again; a second is `Unwelcome` with
  TIDAL's `subStatus`, except 4005 (asset not ready), `TurnedAway`. 403/404 is `Nothing`.
- **Only the whole track, lossless, in the clear.** `HI_RES_LOSSLESS` asked; `assetPresentation`
  other than `FULL` is `Nothing`. `manifest.rs` reads both manifests (base64 BTS JSON; DASH MPD,
  selecting its FLAC rendition). `Withheld::Encrypted` and `Withheld::Lossy` answer `Nothing`.
- **A number the service gives is checked before it is added to.** A DASH `startNumber` numbering
  past `u64` is `Unread::NumberedPastTheEnd`; an `expires_in` no clock can add is
  `Error::Unreadable`, never an `Instant` overflow panic
  (`a_session_said_to_last_past_any_clock_is_refused_as_unreadable`,
  `a_dash_manifest_numbered_past_the_last_number_is_refused`).
- **A manifest is bounded before it is expanded.** `dash` checks the initialization and media
  templates against `MediaHosts` (`Unread::OffItsHosts`, which `Player::media_from` raises as
  `Error::OffItsHosts`) before reading the timeline, then each segment URL as it is built. A `%0<width>d`
  wider than `PADDING_DIGITS_AT_MOST` (20, a `u64`'s digits) is `Unread::PaddedTooWide`; a segment
  URL over `URL_BYTES_AT_MOST` (2048) is `Unread::UrlTooLong`; a timeline of more than
  `SEGMENTS_AT_MOST` (10 000, about 11 hours at TIDAL's 4 s segments) is `Unread::TooManySegments`,
  refused before a URL is made. A BTS manifest holds the same bounds (no more than `SEGMENTS_AT_MOST`
  URLs, none over `URL_BYTES_AT_MOST`), and in both manifests a URL named twice is
  `Unread::RepeatedSegment`, since a media template with no `$Number$` would make the client fetch
  one object ten thousand times (`a_template_naming_no_number_repeats_one_url_and_is_refused`,
  `a_bts_manifest_of_too_many_urls_is_refused_unread`,
  `a_bts_manifest_repeating_a_url_or_naming_a_long_one_is_refused`;
  `a_padding_wider_than_any_number_is_refused_without_being_written`,
  `a_segment_naming_a_foreign_host_is_refused_before_the_timeline_is_expanded`,
  `a_segment_url_past_any_real_one_is_refused`,
  `a_timeline_of_the_most_segments_is_read_and_one_more_is_refused`).
- **Media from TIDAL's audio hosts alone.** `MediaHosts::holds` takes a URL only over `https` with
  host `audio.tidal.com` or under it, refusing user-info `@`, a bracketed literal, a host merely
  ending in the name; one URL off them fails the manifest as `Error::OffItsHosts` before a byte is
  fetched. The media agent follows a redirect only within the origin asked (`followed`), so a
  same-host hop stays on the audio hosts.
- **A segment is a `Ranged`**, bounded per read, not per body
  (`a_segment_the_cdn_asks_to_be_asked_for_later_is_asked_for_again`). `find` reads the playback
  answer and manifest and offers the media; the first segment is fetched when the offer is opened,
  so a CDN refusal is `Refused` under `ProviderOp::Download` (the offer falls through).
- **A DASH template is filled as the standard spells it**: `$RepresentationID$`, `$Bandwidth$`,
  `$Number$`, `$Time$` (a timeline `S`'s `t` restarting it), `%0<width>d` padding, `$$`; an unknown
  identifier is left as written. A relative URL resolves as a URI reference against the `BaseURL`s in
  scope (MPD, Period, AdaptationSet, Representation, each against the one above).
- **Later segments are fetched ahead.** `Fetched` streams the first and keeps `SEGMENTS_AHEAD` (3)
  more coming, each on its own `resonate-tidal-segment` thread over a channel of one, read in order
  (`a_dash_stream_is_fetched_segments_ahead_and_read_in_order`). A segment read ahead is bounded by
  `LARGEST_SEGMENT` and fails the stream with its own error; dropping the stream ends the threads.
- **What lands is a FLAC file, never an MP4.** `remux.rs` repacks DASH's FLAC-in-fragmented-MP4
  without decoding: `fLaC` and the metadata blocks from the `dfLa` box, then every `mdat` payload,
  STREAMINFO's total sample count filled from the timeline. Keyed `track/<id>`, extension `flac`.
- **Signed in from the window or `resonate tidal`.** `resonate_providers::SignsIn` is the seam,
  `TidalSignIn` its implementation, handed over as `Lookups::signs_in` by `providers::signs_in`. The
  device flow asks the `r_usr` scope alone and polls every `interval` (at least
  `ASKED_EVERY_AT_LEAST`, longer on `slow_down` and 429: `SLOWED_BY` is added to the wait, saturating, so an interval or a widening no clock holds waits out the code's life and lapses rather than overflowing, held by `a_device_sign_in_told_to_slow_down_by_more_than_any_clock_holds_waits_out_its_code_and_lapses`, `a_device_code_asked_for_at_an_interval_no_clock_holds_waits_out_its_life_and_lapses` and
  `a_device_sign_in_told_to_slow_down_waits_longer_than_its_interval_before_asking_again` and
  `a_device_sign_in_turned_away_for_asking_too_often_waits_longer_before_asking_again`; the tests set the
  widening with `TidalSignIn::slowing_by` and a short `Authorizing::asked_every`, as `polling_within`
  sets a poll's bound, so none waits out the real seconds) until a refresh token,
  `AuthorizationLapsed`, `AuthorizationDenied` or the cancel (`Ok(None)`), each poll bounded to
  `A_POLL_ANSWERED_WITHIN` (3 s, not the API agent's 20 s) so a cancel is heard within one poll
  even when TIDAL does not answer (`a_cancel_is_heard_within_one_poll_even_when_the_server_does_not_answer`),
  asking again through an
  unreached server or a 500+ (`sign_in::Passing`;
  `a_device_sign_in_asks_again_through_a_server_that_did_not_answer_this_time`); `Debug` never prints
  the device code. **The page the listener is sent to is TIDAL's or none**: `verification_page` accepts
  `https` (port 443, no userinfo) on `tidal.com` and its subdomains alone, whichever of
  `verificationUriComplete` and `verificationUri` was named, and anything else (`file:`, plain
  `http`, another host) is `Error::Unreadable` under `ProviderOp::SignIn`, since the window and the terminal
  open it (`only_a_page_on_tidals_own_hosts_over_https_is_a_verification_page`,
  `a_verification_page_off_tidals_hosts_or_not_over_https_refuses_the_sign_in`). The window's *Sign in to TIDAL* writes the token into the field, the global
  `Online` and `tidal-refresh-token`. `resonate tidal` is the same windowless
  (`providers::sign_in_to_tidal`): refuses with `OnlineOff` or `NoTidalClient` before asking, waits
  with the first interrupt as cancel (`SignInCancelled`), writes via `config::store`
  (`a_tidal_sign_in_with_online_off_or_no_client_is_refused_before_anything_is_asked`) and tells a
  running window the account (`Reloadable::Tidal`, `binary.md`), which takes it as if signed in there.
- **Paces and identifies itself as Subsonic does**; `Account`'s `Debug` prints neither secret nor
  token. `asker.rs` is the pacing, retrying and reading both TIDAL providers share; `played.rs` what
  follows a playback answer, `played::found` (link first, then `Finds::tracks_for`) written once over
  the `Finds` trait.

## A hifi-api server

`HifiApi`, same crate, is the second way to a TIDAL subscription. With `online` on,
`providers::registry` registers it after `Tidal` as `hifi-api`: on a server the `hifi-api` setting
names, else on the hosted service **only where `hosted-services` (default on) is on**, so the listener can
turn every third party's service off with one switch (`a_hifi_api_server_the_listener_named_is_registered_without_hosted_services`,
`the_hosted_hifi_api_is_registered_only_when_the_listener_allowed_hosted_services`). The hosted
service holds no listener credential: its tokens are cached until shortly before expiry and sent
only to their own issuer or API, asking in this build's name alone, never with another site's
`Origin` or `Referer` (`a_hosted_token_is_asked_for_in_this_builds_name_alone`). **The hosted endpoints are one `Hosting`** (`Hosting::live()`: the server, the two token endpoints, TIDAL's API, the manifest hosts), which `HifiApi::hosted_through` takes with the media hosts, so a fake server on loopback stands in for the whole flow
(`the_hosted_hifi_service_is_searched_and_its_track_delivered_through_two_tokens`,
`the_hosted_hifi_service_is_asked_in_this_builds_name_alone_and_each_token_once`,
`a_hosted_manifest_named_off_the_manifest_hosts_is_never_fetched`). `played.rs` reads the manifest through the same checks as a TIDAL account; the
manifest URL must be HTTPS on `manifest.tidal.com` or a subdomain.

- **Asked by link, then by the seam's choice**, via `played::found`: the linked track first; else the
  hosted flow searches TIDAL's web API by each of `Identity::wordings` through a `Choosing`, up to
  `TRACKS_TRIED_AT_MOST` ranked tracks. A custom server keeps the hifi-api routes, asked by each code
  (`/search/?i=`, up to `CODES_ASKED_AT_MOST`) with `data.items` checked by ISRC; **where no code
  lists anything the want is searched by its words** as one with no code is, since a re-release's
  codes are often not the ones MusicBrainz holds
  (`a_want_whose_codes_the_server_lists_nothing_under_is_searched_by_its_name`). **A want with no
  code at all is searched by its words** (`/search/?s=`, each of `Identity::wordings` until one holds a
  taken listing, only where `may_be_listed`) through a `Choosing`
  (`a_want_with_no_code_is_searched_by_its_name_and_a_listing_with_a_code_is_asked_for_first`,
  `a_want_with_no_artist_and_no_code_is_not_searched_at_all`). **A listing with no code of its own is
  ranked below any that carries one** however near its length (`Choosing` sorts the named by
  `(uncoded, apart)`; `a_listing_with_no_code_at_all_is_ranked_below_one_that_carries_a_code_however_near_its_length`).
- **A request the server queues is waited for, withdrawn past the provider's patience.** A `202` with
  a `requestId`: the provider asks `/playback/requests/<id>` again after `Retry-After` (between
  `QUEUE_LOOKED_AT_LEAST_EVERY` and `QUEUE_LOOKED_AT_MOST_EVERY`) for at most `QUEUED_FOR_AT_MOST`,
  inside `ANSWERS_WITHIN`, then sends `DELETE` and answers `Error::StillQueued`. The request id is
  used only as letters, digits, dashes; the path is the provider's own, never the server's
  `statusUrl`.
- **401 is the server's account turned away** (`Unwelcome`, away); 403/404 that track unavailable
  (`Nothing`); else `Refused`. A custom server gets no `Authorization` header **unless `hifi-secret` is set**: `HifiApi::with_secret` sends it as `Bearer` on every request to the server, only over `https://` or to loopback, the host read by `http::Uri` and compared exactly, an authority carrying user-info `@` refused (`a_secret_travels_only_over_https_or_to_the_machine_itself`). **A keyed server is the listener's own `resonate-server` gateway** (`server.md`): the client still searches by code through `/search/`, but a delivery is `GET /file/?id=<track>` on that server, which downloads through its hifi-api and sends the FLAC, so the client never asks TIDAL's CDN, and a delivery that breaks off is resumed through `Ranged` with the key, the server answering `206` (`a_keyed_download_cut_off_part_way_is_asked_of_the_server_again_from_where_it_stopped`); with nothing held any 2xx is read through `Ranged::continuing`, so a server answering a plain ask `206` is read once, never asked again for ever (`a_keyed_server_answering_a_plain_ask_as_partial_content_is_read_and_asked_once`) (`a_keyed_hifi_api_has_the_server_download_the_track_and_sends_its_secret_to_it_alone`; 404/403/410 `Opened::Gone`, 401 `Unwelcome`; a 503 is the server busy with another download and is asked again after its `Retry-After` up to `RESUMES_AT_MOST` times, then `Error::Busy`: `a_keyed_server_busy_with_another_download_is_asked_again_when_it_says_to`). `HifiApi::track` is the same download as a call, which the server uses. **The key is a provider of its own, apart from `hifi-api`**: a secret registers `resonate` (`HifiApi::at(resonate_tidal::OWN_SERVER).with_secret(..).known_as_resonate()`, `OWN_SERVER` being `resonate_core::RESONATE_WEB_ADDRESS`, `https://resonate.tecnio.dev`) after TIDAL and before `hifi-api`, so the listener's server outranks the hosted services, and `hifi-api` and `monochrome` are registered by their own settings as if there were no key (`a_key_registers_the_resonate_server_beside_whatever_hifi_api_is_named`); the key is never sent to a server `hifi-api` names, and `hosted-services` does not touch it. The key is a masked field in the settings pane's *Resonate server key* group in the Library category (`Group::ResonateKey`, `Setting::HifiSecret`; resetting the TIDAL group leaves it) and withheld from every `Debug`. The phones carry it in `DownloadAccount.password` of the hifi-api account (no record change), read by the same plan as the desktop's two keys: a non-blank one makes that account the `resonate` provider on `OWN_SERVER`, beside the `hifi-api` its server field names (or the hosted one while hosted services are allowed), and `resonate` is asked though hosted services are declined (`a_hifi_api_account_with_a_key_is_asked_though_hosted_services_are_declined`).
- **Custom server trusted as Subsonic's is**: `HifiApi::at` uses `Asker::of_the_listeners_server`
  (`Trusted::SystemStoreToo`); hosted service, TIDAL's API and CDN keep built-in roots alone. A
  refused certificate is `Error::Untrusted` via `asker::unreached`.

## A Monochrome server

`resonate-monochrome` asks the track streamer behind Monochrome. With `online` on,
`providers::registry` registers it after `hifi-api` as `monochrome`, asking the server the
`monochrome` setting names, else `tracks.monochrome.st` only where `hosted-services` is on (the default). The
binary's `Hosting` (`Hosted`/`Custom`) builds both overridable services; `hosting` answers `None`
for neither a name nor the opt-in. The settings pane's TIDAL group carries the switch *Ask the
hosted services* (`Online::hosted_services`, `Setting::Switched(Switch::HostedServices, _)`); *Forget* in that group
turns it off. Two routes:
`/search/tracks?q=<words>&limit=` answers `tracks` listings (`trackId`, `isrc`, `playable`, `title`,
`artistNames`, `duration` in milliseconds), `/track/<trackId>` the whole FLAC.

- **Asked in words, chosen by the seam.** The search does not read an ISRC as its query; not
  `may_be_listed`: `Nothing`; else searched by each of `Identity::wordings` (`LISTED_AT_MOST` each)
  through a `Choosing`, a `playable: false` listing passed over. A `trackId` is used only if all
  digits (it goes into the path).
- **What lands is the server's FLAC**, keyed `track/<trackId>`, extension `flac`, opened only when
  the offer is taken, so a refused download is `Refused` under `ProviderOp::Download`. A 404/410 is
  `Opened::Gone`; a download whose `Content-Type` names text, JSON or XML is `Unreadable`.
- **A download that breaks off is a `Ranged`**; each read, not the whole, is bounded
  (`downloading_agent`).
- **Paces as Subsonic does**: `ASKED_APART`; 429/502/503/504 asked again after `Retry-After` up to
  `RETRIES_AT_MOST`. User-Agent bare.
- **Custom server trusted as Subsonic's is** (`Trusted::SystemStoreToo`); hosted keeps built-in
  roots alone. A refused certificate is `Error::Untrusted`.

## A YouTube Music account

`resonate-ytmusic` keeps songs from the listener's YouTube Music account, for the songs no other
service reaches. Source and scheme `ytmusic`, `online` feature, desktop alone (`yt-dlp` cannot run on
a phone, and a Flatpak holds none). Nothing is registered until `youtube-music` is on, and `online =
false` outranks it; `providers::registry` registers it last, after Monochrome, and `in_place` serves
it there too. Everything needing the account or the player's JS goes through `yt-dlp`, a program of
the listener's, by `Tool`; searching does not. It works with no cookies at the free tier.

- **Why a program of the listener's and not a Rust client.** YouTube's player changes often, a
  sidecar is mended by updating `yt-dlp` rather than by releasing Resonate, and Premium formats were
  never proven through a Rust client (RustyPipe). `ffmpeg` is not needed, since nothing is
  re-encoded.
- **What it is for, and what it never does.** It exists for songs on no other service Resonate
  reaches, so it works for a song MusicBrainz has never named; the listener connects once and
  the rest runs from their playlists and from search. It never writes to the account (no liking, no
  playlist edit), keeps no playlist in step on a timer (a keep is one press), takes audio alone,
  touches nothing until `youtube-music` is turned on, and never fills a want a better source could
  answer (the strict last resort, *What a provider is handed*): a library quietly filling with 160 kbps
  files because cookies lapsed is the failure it is built against, which is why a lapse stops the
  poll. Saving files this way breaks YouTube's terms even on a paid account, and the settings group
  says so in one sentence beside the switch (`terms_note`).
- **What was measured** (`yt-dlp` 2026.10.26, 2026-11-06). Premium audio is reachable with the
  `web_music` client: an album song listed `774` (Opus 261k) and `141` (AAC 258k), and a music video on
  the same account only `251` (134k) and `140`, so a kept video lands at the lower tier by nature and
  the check song is an album song. `deno` solves the player's JavaScript challenge unasked. **`yt-dlp`'s
  own Songs search names an id and a title alone**, no length, artist or album, so the named-alike
  rule could never pass on it, which is why the search is InnerTube's: one request answers id, title,
  artist, album, length and the explicit badge, signed out. A private playlist's flat listing names
  `id`, `title`, `duration` and `channel` (the artist: `<name> - Topic` on an album song, the plain
  name on an upload) and no album. A real `774` download was a `.mka` whose DocType is `webm`, with a
  `SeekHead`, `Info`, `Tracks`, `Cues` ahead of the clusters and no `Tags` or `Void`, so an element
  added ahead of the clusters moves every cue position (`library.md` holds how the tag writer copes).

- **`Tool` runs every call with `--ignore-config`**, so a listener's own `yt-dlp.conf` cannot add a
  re-encode or a remux, and each song call with `--no-playlist`, naming the song
  `https://music.youtube.com/watch?v=<id>`, the address on which `yt-dlp` adds its `web_music` client.
  `deno` is found by `yt-dlp` itself; `--js-runtimes` is passed only where it is absent, naming the
  first of `node`, `bun` on `PATH`. **A window started from Finder has the system's short `PATH`**, so
  on macOS `searched_path` adds `FOLDERS_A_FINDER_START_LEAVES_OUT` (Homebrew's `/opt/homebrew/bin`,
  `/usr/local/bin`) after it: the bare `yt-dlp`, the runtimes and what `yt-dlp` runs are looked up
  there, and the child is handed the same `PATH`. A call is held to `ASKED_WITHIN` (60 s), a whole listing to twice
  that, a download's silence to `LEFT_SILENT_FOR`, and a child past its limit is killed.
- **What lands is the stream as served.** Tiers best first: `Opus774` (`mka`, about 260 kbps),
  `Aac141` (`m4a`), `Opus251` (`mka`), the two first Premium's (`Tier::is_premium`). `-f <id> -o -`
  hands stdout to the poll untouched: never re-encoded or remuxed, the name alone changing, since WebM
  is Matroska. A non-zero exit is an `Err` from `read`, never an end of file. AAC has not been measured
  on a real download; a fragmented `m4a` the tag writer refuses would land untagged.
- **Lapsed cookies stop the poll, never fall to the free tier.** `yt-dlp` saying `COOKIES_LAPSED`
  answers `Unwelcome` under `THE_SIGN_IN_LAPSED`, and `Lapse` holds it for `A_POLL_LASTS` (10 minutes),
  so the rest of the poll asks nothing; *Check connection* looks afresh (`formats_checked`). **A
  signed-in song with no Premium format** re-checks `CHECKED_WITH` (`9RfVp-GhKfs`, an album song, since
  a music video has no Premium format on any account), held as long: gone there too, the account lost
  them (`Unwelcome`, `THE_SUBSCRIPTION_LAPSED`); still there, this one song is taken at its tier, warned
  of. Lanes wanting that look wait on one mutex for one run, bounded by `ASKED_WITHIN`, since each would
  otherwise touch the account itself and all need the one answer
  (`lanes_asking_at_once_wait_on_one_look_at_the_account`). **The check song taken down is no lost
  Premium**: `holds_up` takes the song at its tier
  (`a_check_song_taken_down_is_no_lost_premium_and_the_song_is_taken_at_its_tier`).
- **`ERROR: [youtube] <id>:` is the video's verdict.** `yt-dlp` having reached the extractor and refused
  this video (removed, private, DRM, `Requested format is not available`) reads as gone:
  `formats` lists no tier, `download` opens `Opened::Gone`, whatever the reason's words, so one
  video can never stop a poll. **Unless the line is the account's or the network's**
  (`NO_VERDICT_ON_THE_VIDEO`: a bot check, a captcha, a rate limit, a failed request,
  `please report this issue`, `Your IP is likely being blocked`, and what depends on the account or
  the place: an age gate, Premium or members-only content, a country or location): that says nothing of the video, and a verdict there would give every
  queued song up in one outage, so it stays `ToolFailed`, provider-away. **So is a failed challenge
  (`solving failed`) or a video left with storyboards alone (`Only images are available`) anywhere in
  what was said**, whichever line, since either drops formats and ends in `Requested format is not
  available` (`a_listing_cut_short_by_a_failed_challenge_or_storyboards_alone_is_no_verdict_on_the_video`).
  So does a tool that never reached the extractor (a usage error, a missing runtime, a traceback). A lapse is weighed first
  (`a_video_refused_for_its_own_reason_opens_as_gone_and_lists_no_tier`,
  `trouble_of_the_account_or_the_network_is_no_verdict_on_the_video`,
  `a_tool_that_never_reached_the_video_is_the_tool_failed`).
- **The live test is gated and by hand.** `tests/live.rs` runs only under `RESONATE_YTMUSIC_TESTS` (cookies from
  `RESONATE_YTMUSIC_COOKIES`, the song from `RESONATE_YTMUSIC_SONG`, a playlist from
  `RESONATE_YTMUSIC_PLAYLIST`): the album song's best tier read to the head of the container it names, the
  account's check and a playlist listed. Off in CI.
- ***Check connection* has six answers** (`Connection`, `resonate ytmusic check`, the pane's notice):
  `Premium`, `SignedInWithoutPremium`, `NotSignedIn` (no cookies, or turned away), `ToolMissing`,
  `CheckSongGone` (the song it asks about has left YouTube, which forgets no lapse) and `CouldNotCheck`
  (a bot check, a network failure, an answer that does not read: nothing is known of the account)
  (`the_check_song_gone_leaves_a_lapse_standing`,
  `a_check_that_could_not_be_made_is_told_apart_from_not_being_signed_in`).
- **One download at a time, without narrowing the poll.** It says no `downloads_at_once`, which is the
  narrowest across the registry; `OneAtATime` holds a `Turn` inside the crate for as long as the reader
  lives, and an opening waits `A_TURN_IS_WAITED_ON_FOR` (45 s) for it, then answers `Busy`, the want's.
- **A search is signed out**: InnerTube's `search` with `WEB_REMIX` and the Songs filter, `ASKED_APART`
  paced, no cookies, so it never touches the account. The client version is read from `/sw.js_data`,
  not pinned: under the Resonate User-Agent the home page redirects to a consent page and then a *browser
  out of date* page with no version in it. A want follows a `Service::YoutubeMusic` link before
  `may_be_listed`, since a keep may have no length or artist; otherwise it is the named-alike rule, as
  YouTube gives no code. `Listed::stated` is `MUSIC_EXPLICIT_BADGE`.
- **The account's lists are `feed/playlists`.** `music.youtube.com/library/playlists` is refused by
  `yt-dlp`, so `first_page` lists `LM` first as *Liked songs*, then `www.youtube.com/feed/playlists`
  without `LL` and `WL`, read through the cookies; with none it is empty. `mix` is one flat `-J`, no
  call per song, the channel read as the artist with ` - Topic` cut and the top `title` the list's name
  (`LM` is *Liked songs* whatever it says). Albums and artists are `Found` left empty.
- **Every child is reachable on the way out.** `pre_exec` would need `unsafe`, so `Reaped` registers
  each live child in a process-wide list and `stop_the_tools` kills them and refuses new ones; `main`,
  `signals::leave` and the window's end call it. `interrupt_the_tools` kills them and refuses nothing:
  `yt-dlp` saves its cookie jar back into `--cookies` as it exits, so a sign-out and a finished
  sign-in kill every running child first (SIGKILL saves nothing) lest one write the old session over
  the jar, or back after it was taken away
  (`a_tool_interrupted_is_dead_and_the_next_one_still_starts`). A SIGKILLed parent still orphans a child until its pipe
  closes.
- **A cookie value names a file where it holds a path separator or an extension.** A browser's name
  (`firefox`, `chromium+gnomekeyring:Default`) goes to `yt-dlp` as a browser; a bare word with no dot
  (`zen`, a typo) goes to it as a browser too, which refuses an unknown name loudly, since as
  `--cookies` it would write a jar in the working directory; `cookies.txt` is a file
  (`a_bare_word_goes_to_the_tool_as_a_browser_and_a_separator_or_an_extension_names_a_file`).
- **A cookie value is withheld from the logs too.** `Cookies`' `Debug` prints neither browser nor path,
  and `Telling` replaces `Cookies::withheld` (the whole value, the profile, each short of a `~`) in
  every line of stderr before it reaches `tracing`.

## Signing in to YouTube Music in a window

**A separate program owns the browser engine: `resonate-signin`** (`crates/resonate-signin`), the
only executable linking `wry` (WebKitGTK 4.1 on Linux, WebView2 on Windows) and `tao`, so no package
of the player gains WebKitGTK. Its library (no `window` feature: no wry, no tao) is the binary's
dependency; the layering job refuses `wry`, `tao`, `webkit2gtk` and `webview2-com` from the
`resonate` tree and lets `resonate-signin` reach core alone.

- **What the window does**: `resonate-signin --jar FILE --profile FOLDER` opens
  `SIGN_IN_PAGE` (Google's sign-in for YouTube, continuing to `music.youtube.com`) on its own
  `WebContext` profile (on macOS a non-persistent store, WKWebView taking no data directory, so
  the jar is all that is kept), with the engine's own user agent, and on every finished page and once a
  second (`LOOKED_AT_EVERY`) reads the address; on `music.youtube.com` itself (`is_youtube_music`:
  `https`, that host, no user-info) it reads every cookie, keeps those under `youtube.com` and
  `google.com` (`kept`: the domain or a subdomain, never a lookalike, and none whose fields hold a
  tab, CR or LF, which would forge a jar line) and, where `LOGIN_INFO` and `SAPISID` stand under
  `youtube.com` (`signed_in`; Google's cookies alone are no YouTube sign-in), writes the jar and
  exits `SIGNED_IN` (0). Closing the window first exits `CLOSED` (3); anything else `FAILED` (1).
  `Outcome::of` reads the status back.
- **The window leads nowhere else**: `may_lead_to` is the one allow-list, wired as the web view's
  navigation handler, and says yes to `https` (port 443 or none, no user-info, a host of letters,
  digits, `-` and `.` alone, so a `@`, a `\\`, a `%` or a stray port never hides another host) at
  `google.com`, `youtube.com` or a subdomain of either: `accounts.google.com`,
  `consent.google.com`, `consent.youtube.com`, `music.youtube.com`. Google's country domains
  (`google.<cc>`) are left out, since sign-in and consent run on those hosts and a pattern for
  every country would take in a name Google may not own. A new window is denied and a download
  refused (`a_navigation_may_lead_to_https_google_and_youtube_hosts_alone`,
  `only_a_page_of_youtube_music_itself_is_youtube_music`).
- **A domain cookie keeps its dot.** `cookie::Cookie::domain()` strips the leading dot wry's engines
  hand over, which would write `youtube.com FALSE` (host-only) and make `yt-dlp` keep a rotated
  `.youtube.com` value beside the stale one; `domain_cookie_of` dots the bare `youtube.com` and
  `google.com` again, leaving a `__Host-` cookie and any subdomain's host-only
  (`a_domain_the_engine_handed_over_undotted_is_dotted_again_unless_it_names_one_host`).
- **The profile is owner-only before the window opens** (`0700`, `owner_only`): WebKit keeps the
  whole Google session there in plain text, a superset of the jar
  (`the_windows_profile_is_made_owner_only_before_the_window_opens`).
- **The jar is Netscape's** (`netscape`: `#HttpOnly_` before an HttpOnly cookie, a session cookie
  expiring `0`), **written whole, owner-only**: `written` stages `<jar>.staged` under
  `writer::Held::taken_over` (a second sign-in writing is refused `WouldBlock`), `0600` on Unix
  before a byte, `sync_all`, `file_facts::replace`; a folder that cannot be synced after the rename is a jar written (`a_jar_in_a_folder_that_cannot_be_synced_is_still_a_jar_written`). `Cookie`'s `Debug` withholds the value.
- **The binary** (`signing_in.rs`, `online`): `the_program` is `resonate-signin` beside
  `current_exe`, else the first on `PATH`; `Kept::in_the_data_folder` names
  `youtube-music-cookies.txt` and the profile `youtube-music-sign-in` in `config::data_dir`;
  `sign_in` runs the program, holds each child by pid so `stop_the_tools` kills it on the way out,
  and reads the outcome (`YoutubeMusicSignInClosed`, `YoutubeMusicSignInFailed { code }`,
  `YoutubeMusicSignInNotStarted`); `sign_out` takes jar and profile away, a missing one no failure;
  `names_the_jar` says whether `youtube-music-cookies` names the window's own jar.
- **Signing in points `youtube-music-cookies` at the jar**, whatever it named before (a browser is
  not kept); a cancel or failure leaves the setting as it was; signing out clears the setting only
  where it names that jar, so a listener's own cookies file is never touched
  (`a_sign_in_closed_or_failed_leaves_the_setting_as_it_was`,
  `signing_out_deletes_only_what_the_window_kept`). `yt-dlp` keeps refreshing the jar through
  `--cookies`, which is wanted.
- **The browser and `cookies.txt` ways stay beside the window** because Google may tighten its check
  of an embedded engine at any time; the window needs no listener's browser profile (it keeps its own,
  never theirs) and holds nothing that a sign-out does not wipe. No engine is bound directly and no
  further `unsafe_code` exception is made for it (`dependencies.md`).
- **Checked by hand on Linux** (2026-11-09, Hyprland, WebKitGTK 2.54): Google let the sign-in
  through, and `yt-dlp` read the jar the window wrote: the account's *Liked Music* and the Premium
  formats 774 and 141 of `CHECKED_WITH`. WebView2 is cross-checked only; the sign-in there waits on
  a Windows machine (`docs/FEATURES.md`). How long a jar lasts is not known.

## Keeping a song from a service

**A keep is its own queue, not a want** (`keeps.rs`): `wants.release_track_id` is a release row, and a
song MusicBrainz never heard of has none, so a keep is a row of `kept_songs` (title, artist, album,
length, video, and for a list its name and place). `Library::keep_song` and `keep_playlist` write it,
`keep_pending` walks it. `PlaysInPlace::keeps_songs` (false by default, `resonate-ytmusic` true) says a
service offers it.

- **A keep is pinned to the video pressed.** `keep_pending` builds an `Identity` carrying the video's
  link and asks that source's provider alone (`Providers::only`), one song at a time, filing through
  `filed` with the same length check, so no other service can answer with a same-named file. It lands
  in the delivery folder and is refused without one (`NoDeliveryFolder`); a vault is not a keep's
  destination. A blank title is refused at the press (`KeptWithNoTitle`, a toast on a song; a list
  leaves such a row out and says how many). **A keep is given `ANSWERED_ALONE_WITHIN` (120 s)**, not the
  poll's `ANSWERS_WITHIN`: asked alone, its `find` may run the tool twice, and late is away. The tool's
  `ASKED_WITHIN` twice over stays within it (`a_keep_may_outlast_two_runs_of_the_tool`).
- **A keep carries the picture its service showed for it.** `SongToKeep::cover` is the address the
  browse row gave (`ServedCover`, `Song::cover`), kept as `kept_songs.cover` (a pressed-again song
  keeps the first's: `coalesce`), because a kept YouTube Music song is the stream as served, tagged
  with a title, artist and album and no picture, and the library list draws a track's own picture
  (`ui.md`: an album's, else the file's) where the server pane draws the service's. Before the
  download `keeps::pictured` asks `Providers::picture_from` (the provider's `Provider::picture`,
  `NotOffered` by default; YouTube Music's is its `cover`, so only `lh3.googleusercontent.com`,
  `yt3.googleusercontent.com` and `i.ytimg.com`) and `filed::tagged` writes it with the tags, a
  Matroska file taking it as an attachment (`library.md`); a picture that cannot be fetched or
  written costs the song nothing but its picture. **A landed keep with no picture address is asked
  for one once**: each keep pass first walks the landed `kept_songs` rows with no `cover` and no
  `picture_asked` whose source is registered (`picture_what_landed_bare`), asks
  `Providers::picture_named_from` (`Provider::picture_named_for`, by the video; YouTube Music's runs
  the tool's `-J` read of the video and names its widest thumbnail that is not WebP), and where the
  file carries no picture writes the fetched one in and reads its root again; the row keeps the
  address and the stamp, so nothing is asked twice, and a provider that could not answer is asked
  on the next pass
  (`a_song_kept_bare_before_keeps_carried_their_picture_is_given_one_once`,
  `a_songs_picture_is_named_by_its_widest_thumbnail_that_is_not_webp`).
  (`a_song_kept_lands_with_the_picture_its_service_showed_for_it`,
  `a_song_whose_picture_cannot_be_fetched_is_kept_all_the_same`,
  `a_song_pressed_again_keeps_the_picture_it_was_first_pressed_with`)
- **A song already held is not fetched.** `store::held_named_alike` (`Identity::named_alike`'s rule:
  the bare title, the lead artist, a length within 2 s, a clean title standing for no other; a copy
  standing or on an absent volume) answers `AlreadyHeld`, and a list puts the
  held file in its place. A landed row of the same video under another list is that file too, so a song
  in two lists is downloaded once. **A want and a keep of one video land one copy.** `copy_of` also
  takes a standing file whose vault object came from `<source>:track/<video>` (`vault_objects.taken_from`,
  `object_deliveries`; `wants.offered` holds the landed file's URI, so it says nothing of the video). A
  want a last resort answered notes a landed row of its own, which is what remembers the video of a file
  filed in the music folder. The poll declines a stream whose `track/<video>` a landed row names while
  that file stands (`Library::a_keep_holds_the_offer`), and a file gone declines nothing. A want of any
  other provider writes no row, so one song wanted on two albums still lands twice
  (`a_video_a_want_filed_in_the_music_folder_is_not_fetched_again_for_a_keep`,
  `a_video_a_want_kept_in_the_vault_is_not_fetched_again_for_a_keep`,
  `a_video_a_keep_landed_is_declined_by_the_poll_while_its_file_stands`).
- **A list a keep cannot enter holds nothing up.** A list named as a followed or saved-query one stands
  beside it under the first free name (`playlist::name_a_list_may_hold`), and a row whose list was
  followed since the press is left queued before anything is asked, the rows behind it going on
  (`a_kept_list_named_as_a_followed_list_stands_beside_it`,
  `a_list_followed_since_the_press_asks_for_nothing_and_holds_nothing_up`).
- **A landing registers its root before asking for its file to be read** (`landed_read_if_free`, the
  file alone: `library.md`), so a walk that refused the read leaves a
  root the next pass can read under, in any process. The head of a pass reads a landed file no track
  names only through a root that reaches it (`roots_owed_a_read`), never by adding one, so a folder the
  listener forgot stays forgotten unless a new keep lands there
  (`a_folder_never_scanned_is_read_by_the_next_run_though_the_walk_refused_the_landing`,
  `a_folder_forgotten_stays_forgotten_whatever_landed_in_it`).
- **Order is remembered, since songs land over hours.** With `youtube-music-keep-order` on, each row
  keeps its `place` and a landing is seated among those there; off, it appends. The list appears with
  its first song, and a song given up leaves a gap. Pressing a list again adds what is new in its
  places and leaves rows the listener removed removed.
- **What gives a row up** (`gone` set, the video being pinned, so a retry would fetch the same refused
  bytes every open): `Nothing`, a length or decode refused (`NotAsLongAsWanted`, `Undecodable`,
  `TooLarge`), counted `unkept`; `Opened::Gone`, counted `gone`. A press again brings a given-up row
  back, with its tries spent forgotten.
- **A failure never heads two passes running.** The queue reads `ORDER BY tried, asked, id`, songs never
  tried first. Away, at `find` or carried in a read, ends the pass with `stopped_by` and the row queued;
  `Busy`, a stalled stream and a filing that failed go on to the next row. Each stamps `tried`, moving
  the row to the back, and counts in `tries`; at `KEEP_TRIES_AT_MOST` (5) the row is given up, counted
  `unkept`. **What the listener must mend costs no try** (`WhyAway::says_nothing_of_the_song`): a lapse,
  a lost subscription and a missing tool, and a music folder that cannot be written (`Unfiled::Io`, `Turn::Mend`) move the row back and count nothing, since the song was never
  weighed and would otherwise be given up by waiting
  (`a_video_yt_dlp_refuses_for_its_own_reason_gives_the_keep_up_and_the_pass_goes_on`,
  `a_song_that_stopped_a_pass_goes_to_the_back_and_the_next_pass_begins_elsewhere`,
  `a_keep_that_keeps_failing_is_given_up_at_its_fifth_try_and_a_press_brings_it_back`,
  `a_lapsed_account_moves_a_keep_back_and_costs_it_no_try`,
  `a_music_folder_that_cannot_be_written_moves_a_keep_back_and_costs_it_no_try`). **A cancel costs no
  try**: an open refused after the cancel is `Turn::Over`, not a failure.
- **A pass that stopped says why and where.** `Error::why_away` reads a provider-away error as a
  `WhyAway` (`SignInLapsed`, `SubscriptionLapsed`, `ToolMissing`, `NotAnswering`), `Away` keeps it, and
  `KeepSummary::stopped` carries it beside `stopped_by` with the song's title; the window's toast and
  `KeepSignInLapsed`, `KeepPremiumLost`, `KeepToolMissing`, `KeepStopped` on the command line name the
  song and say what to do (`a_pass_stopped_says_why_and_at_which_song`,
  `a_pass_stopped_names_the_song_and_says_what_to_do`).
- **`keep_pending` holds the poll's lock for its whole walk**, so a second pass or a poll is refused.
  A poll refused by a pass in this process asks it to give way: it ends at its next row with
  `KeepSummary::yielded`, and the window begins it again after the poll. A pass in another process
  never yields.
- **In the window** a press writes the row on the background executor and starts a pass on a
  `resonate-keep` thread of its own, no `Work`, since a pass may last hours and a scan may run under it
  (`LibraryModel::keep`, `keep_mix`). **A press during a pass is never dropped and never starts a second
  one**: the pass read its queue at its start, so `Keeps::owed` is set and the pass after it takes the
  song up; a press during a poll waits for the poll, and a pass another process refused is tried again
  `OWED_ASKED_AGAIN_AFTER` later. A poll asked for meanwhile is owed and goes first, a
  pass that gave way following it. A pass runs at open only where `keeps_pending` (rows queued and landed files a root owes a read) is above zero and
  `youtube-music` is on, or, with it off, where a keep replacing a lossy song waits (`keeps_replacing_queued`: such a
  keep is asked of TIDAL, Subsonic or whichever service offered the lossless copy, `library.md`'s *Replacing a lossy
  song*); the window's once-a-second look at a running pass counts `keeps_queued` alone (one query), since the pass reads what landed at its head. **A service that keeps songs reads its first page only once the server pane is
  shown** (`ServerModel::shown`), since that page is `yt-dlp` on the account's cookies and a Chromium-family
  browser may raise a keyring prompt, and a window opening touches nothing unasked
  (`a_service_that_keeps_songs_is_asked_for_its_first_page_only_once_the_pane_is_shown`). *Stop* in the *Downloads* panel cancels it, the rest queued for the next open,
  and so does turning `online` off, or `youtube-music` off where the pass began with it on, heard within a second
  (`keep_only_while_turned_on`, `KeepPass::began_with_youtube_music`:
  `a_keep_replacing_a_lossy_song_is_taken_up_at_open_and_kept_on_with_youtube_music_off`); neither is undone by a yield or a refusal. What a pass that gave way
  did is added to `Keeps::tally` and told in one toast when a pass ends without yielding
  (`a_song_pressed_while_a_pass_runs_waits_for_it_and_is_asked_by_the_next_pass`,
  `a_poll_asked_for_during_a_pass_goes_first_and_the_pass_is_begun_again_after_it`,
  `what_was_left_queued_is_taken_up_at_open_only_while_youtube_music_is_on`).

### Songs offered by Explore

An Explore row has the service's song ID and its catalog namespace. A desktop keep remembers
both through `A_KEEP_CAN_NAME_ITS_STREAMED_SONG` so a restarted pass can ask the original
catalog directly. The file goes through the usual staging, validation, tagging and scan.
The phones put the same namespace and ID into their exact served download offer, which goes
through the usual document destination and indexing. Streaming from either Explore surface
uses the services played in place. Online and the hosted-services setting determine which
services are reachable. Searching sends artist names through the service's existing client.
