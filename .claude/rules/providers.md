---
paths:
  - "crates/resonate-providers/**/*.rs"
  - "crates/providers/**/*.rs"
  - "crates/resonate/src/providers.rs"
  - "crates/resonate-library/src/supply.rs"
  - "crates/resonate-library/src/filed.rs"
---

# Providers

A provider is a crate that turns an identity into media. `resonate-providers` is the seam, crates
under `crates/providers/` fill it, the binary registers them, and `resonate-library`'s poll does
everything else. The seam is on `resonate-core`, `thiserror` and `tracing` alone, and
`cargo tree -p resonate-providers` stays free of the library, codec, vault and gpui: a provider
that depended on the catalog could not be written without it. `Providers` is the registry:
`Providers::none()` holds the `Unprovided` stub and `and` registers one per name, as
`Lyricists::and` does.

## What a provider is handed

`Want::identity()` builds an `Identity`, everything the catalog knows about the row it wants filled:

- `recording`, `track` and `release` as `Option<Mbid>` and `isrc` as `Option<Isrc>` — the
  identifiers, most exact first.
- `title`, `artist`, `album`, `length`, `disc` and `position` — what a service would search on.
- `links` and `release_links` — the `Link`s MusicBrainz gave the track and its release, each a
  `Relation`, a `Service` and a URL. `track_on(Service::Tidal)` and `release_on(Service::Tidal)`
  find a provider's own page; the track's link names the recording on that service and the
  release's the album, and a provider that can use either asks for the track first.

A want's identifiers come from `release_tracks` and `albums.mbid`, so a row the enrichment has not
answered carries its titles alone. A provider needing an identifier answers `Nothing` without one
rather than guessing — **a guess is never written** — which is why `resonate-inbox` never matches
on a title.

## What a provider answers

`Provider::obtain` answers `Obtained::Nothing` or `Obtained::Found(Delivery)`, or an `Err` where it
could not be asked, which `Providers::first` logs, counts as `refused` and carries on past.
`Error::is_the_provider_away` says which errors are about the provider rather than the want — an
`Io` (a connection that failed, a folder that is not there), an `Unwelcome` (the server turned the
listener away as a whole: a wrong password, an account barred), a `StillQueued` (a server holding
the request in a queue of its own past the provider's patience) and a `Refused` of 500 or over — and
`TurnedAway` or a 404 are the want's alone, a Subsonic code 70 being one song the server lacks.

- **`Delivery::File(PathBuf)`** is audio already on disk. The vault reads it and copies what it
  keeps, leaving the file where it stood: a provider's folder, like the library an import reads,
  is never written to.
- **`Delivery::Stream { key, extension, reader }`** is bytes from anywhere. `key` is the provider's
  own name for what it delivered, so the object is recorded as taken from `<provider>:<key>`;
  `Extension` is one to eight ASCII letters and digits, lowercased, without its dot — the format
  hint the decoder probes with.

## What the infrastructure owns

A provider does none of this, so none of it is written twice:

- **Which wants are due** — `POLL_AGAIN_AFTER` since the last try — and asking providers in
  registration order, the first delivery winning.
- **A want is tried only when every provider answered it.** `Answer::heard_from_every_provider` is
  false where any provider refused, ran late or was passed over, and a want answered so is not
  stamped and not counted `nothing`: it stays due, so a Subsonic server that was down, or a wrong
  password, is asked again on the next poll after it is put right rather than `POLL_AGAIN_AFTER`
  later. The cost is a failing provider asked once a poll for as long as it fails
  (`a_want_no_provider_could_answer_is_left_untried_and_asked_again_by_the_next_poll`,
  `a_want_one_provider_answered_and_another_refused_stays_due`).
- **A want dismissed or withdrawn while it is asked about is passed over.** The poll reads the wants
  once as it starts; stamping one the window or another process took away meanwhile answers
  `Error::UnknownWant`, which `supply::tried` logs and passes over, so the poll goes on to the wants
  after it rather than ending
  (`a_want_withdrawn_while_a_poll_asks_about_it_is_passed_over_and_the_poll_goes_on`).
- **A provider that is not there is asked once a poll, not once a want.** The poll holds one `Away`
  for its run and hands it to every `Providers::first`; a provider whose error
  `is_the_provider_away`, or that ran late, is noted there and passed over for every want after,
  counted in `Answer::passed_over` — so an unreachable Subsonic host costs its connect timeout once
  a poll, and a wrong password one login, not a login a want that a server banning repeated
  failures would lock the listener out over; the wants it was not asked about stay due
  (`a_provider_that_cannot_be_reached_is_asked_once_a_poll_rather_than_once_a_want`,
  `a_wrong_password_is_tried_once_a_poll_rather_than_once_a_want`).
- **How long a provider is waited on.** `Providers::first` takes an `Asking` — `within` (the poll's
  `answers_within`, `ANSWERS_WITHIN` by default) and a `cancelled` the poll reads off its progress —
  and asks each provider on a thread of its own, looking at both every `LOOKED_AT_EVERY`. One that
  has not answered by the deadline is left behind and counted `late`, not `refused` (it said
  nothing was wrong), and the next is asked; its thread runs on to whatever end it reaches and its
  answer is dropped. A cancel ends the wait at once and asks nobody else, and the want is not
  stamped as tried, because it was not.
- **How long a stream is read.** `Pumped` reads a delivery's reader on a `resonate-delivery` thread,
  `CHUNK_BYTES` at a time and at most `CHUNKS_AHEAD` ahead, and the keep reads the chunks off a
  channel, looking at the cancel every `HEEDED_EVERY`. A cancel answers an error on the next read,
  so the staging is thrown away and the want left untried rather than a cancelled pass copying on
  to `LARGEST_DELIVERY`; a stream yielding nothing for the poll's `answers_within` is given up the
  same way and counted `late`, and the want *is* stamped as tried — the provider answered and what
  it answered stopped — while the provider is noted `Away` for the rest of the poll. The pump thread is left in the `read` that never returned, as a late
  provider's is, and ends on the first read that does, its channel gone.
- **Staging, validating, deduping and keeping**, through `Vault::keep` and `Vault::keep_delivered`,
  so a delivery is held to the import's bit-exact, never-larger promise.
- **Where no vault is open, a delivery is filed in the music folder instead.** `Library::deliver_into`
  names a `DeliveryFolder` — the `music-folder` key's path and the `organise-as` layout — which the
  binary sets as it opens the library and the window sets afresh before every poll from its live
  globals; with neither a vault nor a folder a stream is `unkept` and a file only offered, as before.
  `filed.rs` does the rest: the want's album is read (`Library::album_to_file`: its title, its owner,
  its year and how many discs its release holds), the path is the layout rendered over the want's
  names under the folder's `Naming`, the bytes are staged as a hidden `.<name>.<pid>-<n>.resonate-delivery`
  beside it — at most `LARGEST_FILED` — and landed by `hard_link` under the first free name
  `take_in::candidates` offers (`name (2).ext` and on), so nothing is overwritten. A landing the
  decoder cannot probe is removed and counted `unkept`. The file is then tagged through `FileTags` with
  everything the want and album say — title, artist, album, album artist, track and disc, the year,
  the recording, release-track and release ids and the ISRC — so a later rescan reads the same row.
  **It joins the album it was wanted for, not one of its own**: before any scan reads it,
  `Library::claim_album_keys` names the album by the keys the scan will compute for the file — the
  release key alone where the release is known, else the album artist's key and the folder's
  sleeve key — so the scan finds the wanted album rather than founding a new one. At the end of the
  poll every root a filing landed under (the folder itself, registered as a root, where no root
  reaches it) is scanned incrementally and `Library::pair_what_landed` pairs each unheld want with
  the rooted row at the path it was offered, so the want is held and the row is an ordinary library
  track — moved by `organise`, written by `tag`, pruned by a scan — never a vault object. Pairing also
  runs as a poll starts, so a filing whose scan was refused (another pass held the walk) is paired by
  the next poll once anything has read it
  (`a_delivery_with_no_vault_is_filed_in_the_music_folder_and_joins_the_album_it_was_wanted_for`).
- **Turning what was kept into a track row.** `Library::note_delivered` writes the `vault_objects`
  row and a `tracks` row in one transaction and pairs the want's release track with it, so a
  delivery is playable, searchable and held the moment it lands. The row is named by the object's
  own path, as a vaulted row whose file has gone is, with `vault_key` and `vault_path` set, so the
  stand-in answers for it and `--prune` spares it; its names, disc, number and identifiers are the
  release track's and its album the want's, the object carrying no tags and the release being what
  the want was identified against. What the release cannot say the delivery can: `Vault::keep`
  hands back the tags the source declared before the object shed them as `Kept::declared`, and the
  row takes its genre, ReplayGain and words from there, so it is found by `genre:` and `lyrics:`,
  levelled by the stand-in and sung by the lyrics pane before any study or lookup reaches it
  (`a_delivered_row_keeps_the_genre_the_gain_and_the_words_its_file_declared`). It belongs to
  **no root** — `tracks.root_id` is nullable for this — and every pass walking the user's files
  joins `roots`, so a scan's prune, a forget, `organise`, `tag`, `vault --import` and
  `vault --release` never reach it: a vault object is not the user's library to move, write into or
  hand back. One object delivered for two wants pairs the second with the row the first made, and
  that row keeps the first want's names in the search index as on the row
  (`one_object_delivered_for_two_wants_is_searched_for_by_the_row_it_stayed`).
- **Recording what landed on the want** — `offered` is the object's URI — and counting `offered`,
  `kept`, `unkept`, `nothing`, `refused` and `late`. A want whose release track holds a row has its
  `held` set and is never due again, so a filled want is the record of where its delivery went and
  removing it changes nothing about the row. **Forgetting the row is the other way round**:
  `Library::forget_delivered` removes the rootless row a path names — the object's own, which
  `resonate wants` prints as `OFFERED` and `resonate forget` reads beside a root, URI or path — and
  nothing a scan filed, leaving the object to `--prune` and the want standing, so a wrong file
  dropped in the inbox is replaced by the next poll. The window reaches it too: `Track::delivered`
  reads `root_id IS NULL` with every other column, and a delivered row's menu offers *Forget this
  delivery* (`LibraryModel::forget_delivered`).

## Writing one

1. A crate under `crates/providers/<name>/`, package `resonate-<name>`, on `resonate-core`,
   `resonate-providers` and whatever reaches its source. A network provider reaches `ureq` itself,
   not through `resonate-online`, and paces and identifies itself as `online.md` says — by name
   and version and nothing else.
2. A workspace member and a `[workspace.dependencies]` entry.
3. A dependency of the binary and one `.and(Arc::new(..))` in `providers::sourced` (the inbox is
   registered by `with_inbox`), gated on the `Config` key saying it is wanted. `providers::registered`
   is the public entry and the one place any is registered — in code, never loaded at run time.
   The window builds its registry through the same function, handed over as
   `resonate_ui::Sourcing::register` beside the settings it reads, so a folder chosen in *The
   inbox* group is polled from at once; a provider with a key of its own widens `Sourcing` and that
   function together.
   **The window polls on its own** as well as on *Poll now*: `FIRST_ASKED_AFTER` a start, every
   `ASKED_EVERY` after, and as soon as a want is marked — each only where a provider is
   registered, nothing else runs and `Library::is_a_want_due` says one is due, so an idle window
   with nothing wanted reads the wants and nothing else. A self-started poll raises no notice when
   it cannot run and clears none when it does. *Poll now* and `resonate poll --again` poll under
   `PollOptions::ASKING_EVERY_WANT`, asking every unheld want whenever last tried — somebody who
   just dropped a file in the inbox means *now*; the timer and a bare `resonate poll` keep to
   `POLL_AGAIN_AFTER`, which spares a network service.
   **The window watches the inbox folder**, through the same `RootsWatch` as the roots: once a
   write of an audio file or sheet under it has been quiet for `INBOX_QUIET_FOR`, it polls as
   `Prompted::ByTheInbox` — every unheld want, as a press does, but raising and clearing no notice,
   as the timer does. A poll that could not start because a pass ran stays owed and is asked again
   on the next look, `INBOX_LOOKED_AT_EVERY` later, and a folder chosen in the pane is watched from
   the next look. **What landed while no window was open is asked about when one opens**: the first
   look weighs the newest file directly in the folder — the later of its modification and change
   times, since a copy keeping its old time still changed status on landing — against
   `Library::last_tried` (the latest try of any unheld want), and a newer file is owed a
   `ByTheInbox` poll at once rather than waiting out the timer's `POLL_AGAIN_AFTER` for wants tried
   earlier. `a_file_dropped_in_the_inbox_after_the_last_poll_is_what_the_window_opens_to_ask_about`
   is the claim on `landed_since`.
4. An `Error::Io` names the provider and a `ProviderOp`; an error the seam has no variant for is
   added to the seam, with its op, when that provider lands — never as prose.

`resonate-inbox` is the reference: `Inbox::at` over the folder the `inbox` key names, read and
never written to, one directory read per want, a file directly inside whose stem is the recording
MBID, then the track MBID, then the ISRC, ignoring case, never a nested folder or a shared title.
Only audio is offered: a file counts where `resonate_core::names_audio` says its extension is one of
`AUDIO_EXTENSIONS` — the list the scan walks by, moved into core so the inbox, which may not see the
library, reads the same one — so `<mbid>.cue`, `<mbid>.jpg` or a rip log beside the audio is never
delivered ahead of it (`only_audio_is_delivered_whatever_else_shares_its_name`).

## A Subsonic server

`resonate-subsonic` reaches a network: a server of the listener's — Navidrome, Airsonic, Gonic,
anything speaking the Subsonic API — named by `subsonic`, `subsonic-user` and `subsonic-password`.
`providers::sourced` registers it after the inbox only where all three are given and `online` is
on, behind the binary's `online` feature, so a build with no HTTP client carries none of it; the
window builds its registry through the same closure (`Sourcing::register`), and the Library
category's *A Subsonic server* group writes the keys for the next start.

- **Asked by the identifiers, never by a title.** A want with neither a recording MBID nor an ISRC
  answers `Nothing` without a request. Otherwise `search3` is asked in words — the title and the
  artist, then the title alone, a server such as Gonic matching the whole query against the title —
  `SONGS_A_PAGE` songs at a time, paging by `songOffset` until a page comes back short or
  `PAGES_AT_MOST` are read, so a title a hundred songs share still reaches the one wanted. A song is
  taken only where its `musicBrainzId` is the recording or, failing that, its `isrc` (one code or a
  list, as OpenSubsonic writes it) holds the want's — read through `Isrc::new`, so a code written
  with dashes or in lowercase is the same code — so a tribute band's *Echoes* is never delivered for
  Pink Floyd's. `a_song_is_taken_by_its_recording_and_then_by_its_isrc_and_never_by_its_title` is
  the claim over a captured Navidrome answer; `tests/server.rs` serves the rest from a local socket
  (`a_song_is_searched_for_by_title_and_artist_and_then_by_title_page_after_page`,
  `an_isrc_written_with_dashes_is_the_same_code`).
- **It delivers the server's original file, and only a file.** `download` answers the bytes as they
  sit on the server, as a `Delivery::Stream` keyed by the song's id and hinted by its `suffix`, kept
  and validated like any other; a suffix that is no `Extension` answers `Nothing` rather than a
  guess. The API answers a failed download with its error document and a 200, so a download whose
  `Content-Type` names text, JSON or XML is read as that document — `TurnedAway` with its code, or
  `Unreadable` — and never streamed as a song
  (`an_error_document_answering_a_download_is_a_refusal_and_never_a_song`).
- **It paces itself and waits when asked to.** Requests go out `ASKED_APART` apart, one at a time
  through `next_asked`; a 429 or 503 is asked again after its `Retry-After` in seconds, or one, two
  and four seconds where it names none, never more than `LONGEST_RETRY_AFTER`, and after
  `RETRIES_AT_MOST` is the `Refused` it was — a 503 the seam then reads as the provider away.
- **The password never leaves as typed.** Every request carries the user, a fresh salt and `t`,
  the MD5 of password and salt (the API's token scheme), beside `v` and `c=resonate`; the
  User-Agent is `resonate/<version>` alone. `status: failed` is `Error::Unwelcome` with the
  server's code where the code is about the account rather than the song (`ACCOUNT_REFUSALS`: 20 and
  30 for a protocol too old or new, 40 to 44 for credentials, 50 unauthorised, 60 a trial over) and
  `Error::TurnedAway` with it otherwise (70, a song the server lacks), an HTTP refusal `Error::Refused`, an answer that is not
  the document `Error::Unreadable`, a failed connection `Error::Io`, each naming
  `ProviderOp::Search` or `Download`.

## A TIDAL account

`resonate-tidal` is the native form of `tidal-proxy`, the Fastify segment proxy TIDAL-DL leans on
when a browser is refused TIDAL's CDN: what the proxy did — fetch a signed segment from an allowed
TIDAL audio host, forward `Range`, stream it without holding it whole — is what the provider does
for itself, a native client meeting no CORS and needing no proxy in between. It is the listener's
own subscription, named by `tidal-client-id`, `tidal-client-secret` and `tidal-refresh-token`;
`providers::sourced` registers it after the inbox and the Subsonic server only where the client id
and the refresh token are given (the secret is sent where given) and `online` is on, behind the
binary's `online` feature, and the Library category's *A TIDAL account* group writes the keys for
the next start, the secret and the token drawn as marks. Nothing is downloaded to play: a delivery
is fetched whole into the vault — or, with none open, the music folder — and lands as a track row,
as every provider's does.

- **Asked by the link and the ISRC, never by a title.** The TIDAL track MusicBrainz links the
  recording to (`Identity::track_on(Service::Tidal)`, read by `TrackId::linked` out of
  `tidal.com/track/<id>` and `tidal.com/browse/track/<id>`) is asked first and needs no search;
  failing that, `openapi.tidal.com/v2/tracks?filter[isrc]=` is asked for the ISRC and a listing is
  taken only where its own `isrc` attribute is the want's, read through `Isrc::new`, at most
  `TRACKS_TRIED_AT_MOST` of them in turn. A want with neither answers `Nothing` with no request and
  no sign-in (`a_track_whose_isrc_is_not_the_wanted_one_is_never_taken`,
  `a_track_musicbrainz_links_to_tidal_is_taken_without_a_search`).
- **It signs in with the listener's refresh token and nothing else.** `auth.tidal.com`'s token
  endpoint is sent the `refresh_token` grant, the client id and, where given, the secret; the access
  token is held until `RENEWED_BEFORE` its `expires_in` runs out and the country is read off the
  grant's `user.countryCode` or else `/v1/sessions`. No client id or secret is built in: the
  listener brings the application the token was issued to. A 400 or 401 at the token endpoint is
  `Error::Unwelcome` under `ProviderOp::SignIn`, which the seam reads as the provider away, so a
  revoked token costs one sign-in a poll
  (`a_refresh_token_turned_away_is_the_account_and_not_the_want`). A 401 from the API signs in
  again once and asks again, and a second is `Unwelcome` with TIDAL's `subStatus`
  (`a_session_that_lapsed_signs_in_again_once`); a 401 whose `subStatus` is 4005 — the asset not
  ready for playback — is the want's, `TurnedAway`. A 403 or 404 is the track unavailable to this
  account or country and answers `Nothing` for that track.
- **Only the whole track, lossless and in the clear, is taken.** `playbackinfopostpaywall` is asked
  for `HI_RES_LOSSLESS` as `STREAM` and `FULL`; an `assetPresentation` other than `FULL` — the
  thirty-second preview a lapsed subscription is given — is `Nothing`, never kept as the track
  (`a_preview_is_never_delivered_for_the_track`). `manifest.rs` reads both manifests TIDAL answers:
  `application/vnd.tidal.bts`, base64 JSON naming one URL, and `application/dash+xml`, an MPD whose
  `SegmentTemplate` and `SegmentTimeline` name an initialisation segment and every media segment.
  A BTS `encryptionType` other than `NONE` or an MPD carrying `ContentProtection` is
  `Withheld::Encrypted`, and a codec other than FLAC `Withheld::Lossy`, both `Nothing`: the provider
  decrypts nothing and keeps no lossy stream. A timeline past `SEGMENTS_AT_MOST` is unread rather
  than allocated.
- **Media is fetched from TIDAL's audio hosts alone.** `MediaHosts::holds` takes a URL only over
  `https` whose host is `audio.tidal.com` or under it — the proxy's `sp-ad-fa` and `sp-ad-cf` and
  every other CDN node the manifests name — refusing a user-info `@`, a bracketed literal and a host
  merely ending in the name; one URL off them fails the whole manifest as `Error::OffItsHosts`
  before a byte is fetched (`media_named_off_the_audio_hosts_is_never_fetched`). The media agent
  follows no redirect, so a host it was not given cannot be reached through one.
- **A segment that breaks off is asked for again from where it stopped.** `Fetched` reads the URLs
  in turn, each through `Piece`; a read that fails mid-body is asked again with
  `Range: bytes=<read>-`, taken where the answer is a 206 whose `Content-Range` starts there, or a
  200 read past what was already given, `RESUMES_AT_MOST` times running before the error stands
  (`a_segment_that_breaks_off_is_asked_for_again_from_where_it_stopped`). Each request has
  `MEDIA_READ_WITHIN` to deliver its body, so a stalled CDN connection is broken and resumed rather
  than held; the first segment is opened inside `obtain`, so a CDN refusing it is the provider's
  `Refused` under `ProviderOp::Download`, not a failed keep.
- **What lands is a FLAC file, never an MP4.** A DASH stream is FLAC frames in fragmented MP4, and
  `remux.rs` repacks it on the way through, decoding nothing: `Remuxed` reads the boxes, takes the
  FLAC metadata blocks out of the `moov`'s `fLaC` sample entry's `dfLa` box, writes `fLaC` and
  those blocks — the final one alone marked last — and then copies the payload of every `mdat`
  after it, skipping `styp`, `sidx` and `moof`. STREAMINFO's total sample count, which a fragmented
  file leaves at zero, is filled in from the timeline where its timescale is the stream's rate. A
  `mdat` ahead of the `moov` or a file with no FLAC track is `Unreadable`. Every delivery is keyed
  `track/<id>` with the extension `flac`, so the vault takes a native FLAC and keeps it as one —
  re-encoded where that is smaller, kept stripped where not. Checked against ffmpeg's fragmented
  output at 44.1 kHz/16 bit and 96 kHz/24 bit: `flac -t` passes and the decoded audio's MD5 is the
  source's (`a_fragmented_flac_track_becomes_a_native_stream_of_the_same_frames`).
- **It is signed in to from the window.** `resonate_providers::SignsIn` is the seam — a `Client`
  (an id and an optional secret), an `Authorizing` (the code the listener types, the page it is typed
  at, how long it lasts and how often to ask, and the device code, never printed by `Debug`) and a
  `RefreshToken` — and `TidalSignIn` the one implementation, handed to the window as
  `Lookups::signs_in` by `providers::signs_in` under `online`. `authorizing` posts the client id and
  `r_usr w_usr w_sub` to `oauth2/device_authorization`, a page named without a scheme reached over
  `https`; `authorized` asks the token endpoint with the device-code grant every `interval` (at least
  `ASKED_EVERY_AT_LEAST`, five seconds more on `slow_down`) until it answers a refresh token,
  `expired_token` or the code's `expiresIn` runs out (`Error::AuthorizationLapsed`), `access_denied`
  (`Error::AuthorizationDenied`), or the cancel it is handed reads true (`Ok(None)`), looking at the
  cancel every `LOOKED_AT_EVERY`. The *A TIDAL account* group's *Sign in to TIDAL*, greyed until a
  client id is given and while Online is off, runs both on the background executor, draws the code,
  *Open the page* and *Stop* while it waits, and writes the token it is handed into the refresh-token
  field and `tidal-refresh-token`, the provider asking from the next start
  (`a_device_sign_in_waits_while_it_is_pending_and_answers_the_refresh_token`,
  `a_device_sign_in_turned_down_or_left_to_lapse_says_which`,
  `a_device_sign_in_cancelled_while_waiting_stops_asking`). No client id is built in.
- **It paces itself and identifies itself as the Subsonic client does**: API requests `ASKED_APART`,
  a 429 or 503 retried after its `Retry-After` up to `RETRIES_AT_MOST`, the User-Agent
  `resonate/<version>` alone, and `Account`'s `Debug` printing neither the secret nor the token.
  `tests/server.rs` serves a fake TIDAL — token endpoint, OpenAPI, playback and segments cut from
  `tests/fixtures/tone.mp4` — from a local socket. `asker.rs` is the pacing, retrying and reading
  both TIDAL providers share, and `played.rs` what follows a playback answer — the presentation and
  manifest weighed, the hosts held, the segments fetched and remuxed — with `played::obtained`, the
  link-then-ISRC order, written once over the `Finds` trait each implements.

## A hifi-api server

`HifiApi`, in the same crate, is the second way to a TIDAL subscription: a
[hifi-api](https://github.com/binimum/hifi-api) server the listener runs on an account of their own,
named by the `hifi-api` key (empty by default — no server is built in, the public instances included)
and written by the *hifi-api server* field of the *A TIDAL account* group. `providers::sourced`
registers it after `Tidal`, as `hifi-api`, only where the key is given and `online` is on. It holds no
credential: the server signs in to TIDAL and hands back TIDAL's own playback answer, so what the
provider does with that answer is `Tidal`'s, through `played.rs` — only `FULL`, only FLAC in the
clear, media from TIDAL's audio hosts alone (the server's manifest naming another host is
`OffItsHosts`), the segments resumed and repacked into native FLAC, keyed `track/<id>`.

- **Asked by the link and the ISRC, never by a title**, through `played::obtained`: the linked
  track first, then `GET /search/?i=<isrc>&limit=25`, whose `data.items` are taken only where their
  own `isrc` is the want's, `TRACKS_TRIED_AT_MOST` of them; each is
  `GET /track/?id=<id>&quality=HI_RES_LOSSLESS`, the answer under `data`
  (`a_hifi_api_server_is_asked_by_the_isrc_and_its_track_delivered_as_native_flac`,
  `a_hifi_api_track_whose_isrc_is_not_the_wanted_one_is_never_asked_for`).
- **A request the server queues is waited for, and withdrawn past the provider's patience.** A
  server whose accounts are all busy answers `202` with a `requestId`; the provider asks
  `/playback/requests/<id>` again after its `Retry-After`, held between one and five seconds, until
  the playback answer comes — a `410` (cancelled there) being nothing for that track — for at most
  `QUEUED_FOR_AT_MOST` (20 s, inside the poll's `ANSWERS_WITHIN`), then sends `DELETE` to free the
  slot and answers `Error::StillQueued`, which the seam reads as the provider away, so a saturated
  server costs one wait a poll. The request id is used only where it is letters, digits and dashes,
  and the path is the provider's own, never the `statusUrl` the server names
  (`a_hifi_api_request_held_in_its_queue_is_waited_for`,
  `a_hifi_api_request_queued_past_its_patience_is_withdrawn_and_the_server_counted_away`).
- **A 401 is the server's account turned away**, `Unwelcome` and away; a 403 or 404 is that track
  unavailable, `Nothing`; anything else is `Refused`, a 429 or 503 retried as the TIDAL client
  retries (`a_hifi_api_track_the_server_cannot_play_is_nothing_and_a_refused_server_is_unwelcome`).
  It sends no `Authorization` header and the User-Agent `resonate/<version>` alone.
