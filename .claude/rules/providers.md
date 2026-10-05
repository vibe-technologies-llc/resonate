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
everything else. The seam is on `resonate-core`, `thiserror` and `tracing` alone, so
`cargo tree -p resonate-providers` stays free of the library, codec, vault and gpui: a provider
that depended on the catalog could not be written without it. `Providers` is the registry:
`Providers::none()` holds the `Unprovided` stub and `and` registers one per name, as
`Lyricists::and` does. `Providers::only` answers a registry holding the one provider named, for a
poll only that provider can answer (below).

## What a provider is handed and answers

`Want::identity()` builds an `Identity`, everything the catalog knows about the row it wants filled:
the identifiers (`recording`, `track`, `release` as `Mbid`, `isrc`), what a service would search on
(title, artist, album, length, disc, position) and the MusicBrainz `Link`s of the track and its
release (`track_on(Service::Tidal)`, `release_on(..)` find a provider's own page; ask the track's
first). A provider needing an identifier (a row the enrichment has not answered carries titles alone) answers
`Nothing` without one rather than guessing: **a guess is never written**, which is why
`resonate-inbox` never matches on a title.

`Provider::obtain` answers `Obtained::Nothing` or `Obtained::Found(Delivery)`, or an `Err` where it
could not be asked, which `Providers::first` logs, counts `refused` and carries on past.
`Error::is_the_provider_away` says which errors are about the provider rather than the want (`Io`,
`Unwelcome`, `StillQueued`, `Untrusted`, a `Refused` of 500 or over); `TurnedAway` and a 404 are the
want's alone. **A file still arriving is neither a miss nor the provider away**: `StillArriving` is
counted `refused`, the want stays unstamped and due, and the provider is still asked about the rest.

- **`Delivery::File(PathBuf)`** is audio already on disk, copied by the vault and left where it
  stood: a provider's folder is never written to.
- **`Delivery::Stream { key, extension, reader }`** is bytes from anywhere; the object is recorded as
  taken from `<provider>:<key>`, and `Extension` is the format hint the decoder probes with.

## What the infrastructure owns

A provider does none of this, so none of it is written twice.

- **Wants are asked side by side, `PollOptions::lanes` (three) at a time.** The first want is asked
  alone, so a provider that is away or refuses a login is met once and noted in the shared `Away`
  before the other lanes begin; each lane then claims the next due want, ask and landing together.
  Idle lanes wait for the pool to drain. A want marked while a poll runs is picked up by that poll:
  `PollProgress::wants_changed` makes the next claim read the wants again (newest first) and answers
  false once the poll has closed, which the window reads as *owe a poll*. `PollProgress` keeps a lane
  per want being asked (`asking_all`, `provider_of`, `received_for`), so each download row says its own
  provider and bytes.
- **Due wants, in registration order, the first delivery winning.** `wants.misses` counts the tries
  in a row that every provider answered with nothing. `Want::due_at` is at once where never tried,
  `RETRY_WAITS` after the last try for the miss it is on, `POLL_AGAIN_AFTER` after an offer the
  catalog does not hold yet, and `None` once misses reach `TRIES_BEFORE_GIVING_UP`
  (`Want::gave_up`). `want_in` (`library.md`) puts the count back and `ASKING_EVERY_WANT` asks a
  given-up want too (*Poll now*). `Library::next_want_due` is what the window wakes for.
- **A want is tried only when every provider answered it.** `Answer::heard_from_every_provider` is
  false where any provider refused, ran late or was passed over; such a want is not stamped and not
  counted `nothing`, so a server that was down or a wrong password is asked again by the next poll
  once put right, and is never given up for it.
- **A poll only one provider can answer asks that one alone, and its silence says nothing of the
  rest.** `Providers::only` marks the registry narrowed wherever it left another real provider out;
  `Providers::first` copies the mark onto `Answer::narrowed`, which is never
  `heard_from_every_provider`, so a want the inbox alone has nothing for is neither stamped nor
  counted a miss. The narrowing is the registry's, not an option beside it, so it cannot reach a poll
  that takes silence for everybody's. The window's inbox poll (`Prompted::ByTheInbox`) uses it.
- **A provider that is not there is asked once a poll, not once a want.** The poll holds one `Away`
  handed to every `Providers::first`; a provider whose error `is_the_provider_away`, or that ran
  late, is passed over for every want after (`Answer::passed_over`), so an unreachable host costs its
  connect timeout once and a wrong password one login, where a server banning repeated failures would
  lock the listener out. The wants it was not asked about stay due.
- **How long a provider is waited on.** `Providers::first` takes an `Asking`: `within`
  (`ANSWERS_WITHIN` by default), a `cancelled` read off the poll's progress, a `turning_to` called with
  each real provider's name and a `declined` every delivery is weighed against. Each provider is asked
  on a thread of its own; one not answered by the deadline is left behind and counted `late`, not
  `refused`, its answer dropped. A cancel ends the wait at once and the want is not stamped.
- **How long a stream is read.** `Pumped` reads a delivery's reader on a `resonate-delivery` thread,
  `CHUNK_BYTES` at a time and at most `CHUNKS_AHEAD` ahead, the keep looking at the cancel. A cancel
  throws the staging away and leaves the want untried; a stream yielding nothing for `answers_within`
  is given up the same way, counted `late`, with the want stamped as tried (the provider answered and
  what it answered stopped) and the provider noted `Away`. Bytes are bounded by `LARGEST_DELIVERY`.
- **Keeping** goes through `Vault::keep_delivered`, so a delivery is held to the import's bit-exact,
  never-larger promise.
- **A delivery is weighed against the length of the row it was wanted for.** Where the release row
  names a length, `Want::lasts_as_long_as` takes a delivery only within the library's
  `LENGTHS_AGREE_WITHIN` of it (the vault's `Kept::frames`; in the music folder a whole decode of the
  landing). One further off is counted `unkept` and noted as nothing; the object is left to
  `--prune` and the filed landing is removed. A refused delivery waits like a want that found nothing:
  `note_tried` steps `misses` for any try that offered nothing, so a provider offering the wrong song,
  or a file still being copied into the inbox, is asked after the retry's wait, not at every poll.
- **Where no vault is open, a delivery is filed in the music folder instead.** `Library::deliver_into`
  names a `DeliveryFolder` (the `music-folder` path and the `organise-as` layout), set by the binary
  at open and by the window before every poll; with neither, a stream is `unkept` and a file only
  offered. `filed.rs` renders the path under the folder's `Naming`, stages the bytes as a hidden
  `.<name>.<pid>-<n>.resonate-delivery` beside it (at most `LARGEST_FILED`), lands by `hard_link` under
  the first free name `take_in::candidates` offers so nothing is overwritten, and tags it through
  `FileTags`. A landing the decoder cannot probe, or whose decode disagrees with the wanted length, is
  removed and counted `unkept`. **It joins the album it was wanted for, not one of its own**:
  `Library::claim_album_keys` names the album by the keys the scan will compute, before any scan reads
  it. After the poll the roots a filing landed under are scanned and `Library::pair_what_landed` pairs
  each unheld want with the rooted row at the path it was offered, so the row is an ordinary library
  track, never a vault object (pairing runs as a poll starts too, for a scan another pass refused).
- **Turning what was kept into a track row.** `Library::note_delivered` writes the `vault_objects`
  row and a `tracks` row in one transaction and pairs the want's release track with it, so a delivery
  is playable, searchable and held the moment it lands. The row is named by the object's own path with
  `vault_key` and `vault_path` set, so the stand-in answers for it and `--prune` spares it; its names
  and identifiers are the release track's, its genre, ReplayGain and words come from `Kept::declared`.
  It belongs to **no root** (`tracks.root_id` is nullable for this) and every pass walking the user's
  files joins `roots`, so a scan's prune, a forget, `organise`, `tag`, `vault --import` and
  `vault --release` never reach it. **A row held meanwhile keeps its pairing**: a scan can pair the
  wanted release row with the listener's own file while a provider is answering, so `note_delivered`
  checks inside its transaction that the row is still unheld and otherwise writes nothing, answers
  `None`, counted `unkept`.
- **Forgetting a delivery.** `offered` is the object's URI; a want whose release track holds a row is
  `held` and never due again. `Library::forget_delivered` removes the rootless row a path names
  (`resonate forget` and the window's *Forget this delivery*) and nothing a scan filed, leaving the object to `--prune` and the want
  standing. **What was forgotten is remembered, and the want is due at once**: in the same transaction
  its offer, `tried` and `misses` are put back to nothing and it gains a `forgotten_deliveries` row
  (the `vault_objects.taken_from` and when). The poll hands each want's to `Providers::first` as
  `Asking::declined`, which passes a matching delivery over (counted in `Answer::declined`, never
  `refused`) and asks the next provider. A stream is declined by its key alone; a file only where its
  later modification or status change is no later than the forgetting, so a right file put back under
  the same name is delivered.

## Writing one

1. A crate under `crates/providers/<name>/`, package `resonate-<name>`, on `resonate-core`,
   `resonate-providers` and whatever reaches its source. A network provider reaches `ureq` itself, not
   through `resonate-online`, and identifies itself as `online.md` says: name and version, nothing
   else.
2. A workspace member and a `[workspace.dependencies]` entry.
3. A dependency of the binary and one `.and(..)` in `providers::registry` (the inbox by `with_inbox`),
   gated on the settings saying it is wanted. `Accounts` is what the network providers are built from,
   read off the `Config` by `Accounts::of` for a headless run (`providers::registered`) and off the
   window's live `resonate_ui::Online` by `Accounts::given` (`providers::sourced`, handed over as
   `resonate_ui::Sourcing::register` and called on every `LibraryModel::providers`). `registry` is
   the one place any is registered, in code, never at run time, so a changed setting is asked from the
   next poll with no restart. `Made` hands back the same provider (`Kept`) until its settings change,
   since a network provider holds a session, tokens and pacing worth keeping. A provider with a
   setting of its own widens `Online`, `Accounts` and `registry` together.
4. An `Error::Io` names the provider and a `ProviderOp`; an error the seam has no variant for is added
   to the seam, with its op, when that provider lands, never as prose.

**The window polls on its own** as well as on *Poll now*: when the soonest want is due
(`Shelves::next_try`, `LibraryModel::ask_when_due`, bounded by `FIRST_ASKED_AFTER`,
`RETRIES_ASKED_AT_LEAST` and `ASKED_EVERY`) and as soon as a want is marked, each only where a
provider is registered and `Library::is_a_want_due`. A want marked while another pass holds the
library is owed (`LibraryModel::poll_owed`) and asked about when `take_up_what_waited` finds it free. *Poll now* and `resonate poll --again` poll
under `PollOptions::ASKING_EVERY_WANT`, because somebody who just dropped a file in the inbox means
*now*; the timer and a bare `resonate poll` keep to `Want::due_at`, which spares a network service.
`PollProgress::{asking, asking_provider, received}` carry the want, provider and bytes landed for the
window's *Downloading…*; they are read every frame, so the provider sits under a `parking_lot::Mutex`
held for a clone and the count is an atomic. A `Delivery::File` counts its whole length when the vault takes it up, so an inbox
delivery shows as received as a stream does (`a_file_delivered_into_the_vault_counts_the_bytes_it_was_as_received`).

**The window watches the inbox folder** through the same `RootsWatch` as the roots: once a write under
it has been quiet for `INBOX_QUIET_FOR`, it polls as `Prompted::ByTheInbox`, raising no notice. **What
landed while no window was open is asked about when one opens**: the first look weighs the newest file
in the folder (the later of its modification and change times, since a copy keeping its old time still
changed status on landing) against `Library::last_tried` (`landed_since`).

## The inbox

`resonate-inbox` is the reference provider: `Inbox::at` over the folder the `inbox` key names, read
and never written to. A file directly inside whose stem is the recording MBID, then the track MBID,
then the ISRC, ignoring case, is a match; never a nested folder or a shared title. Only audio is
offered, by `resonate_core::names_audio` over `AUDIO_EXTENSIONS` (in core so the inbox, which may not
see the library, reads the list the scan walks by), so a `.cue`, `.jpg` or rip log beside the audio
is never delivered ahead of it.

- **The folder is read once a poll, not once a want**: the listing is held with the folder's
  modification and change times and read again only where either moved or it is older than
  `LISTING_TRUSTED_FOR` (coarse-timestamp filesystems).
- **The most exact name wins, and among the files one name matches the lossless one**: sorted by
  `Fidelity` (`Lossless`, `LosslessOrLossy` for containers able to hold either, `Lossy`) and then by
  name, so `<mbid>.flac` beats `<mbid>.mp3`, while a recording's `.mp3` is still taken before an
  ISRC's `.flac`.
- **A file still being copied in is not delivered.** Where the best-named file's modification or
  change time is within `SETTLES_FOR` (the window's `INBOX_QUIET_FOR`) of now it answers
  `Error::StillArriving`, and a lesser file is not delivered in its place. The change time is the one
  a copy cannot keep old (`cp -p` sets the modification time back); a time in the future holds
  nothing back.

## A Subsonic server

`resonate-subsonic` reaches a server of the listener's (Navidrome, Airsonic, Gonic, anything speaking
the Subsonic API) named by `subsonic`, `subsonic-user` and `subsonic-password`; `providers::registry`
registers it after the inbox only where all three are given and `online` is on.

- **Asked by the identifiers, never by a title.** A want with neither a recording MBID nor an ISRC
  answers `Nothing` without a request. Otherwise `search3` is asked in words (title and artist, then
  the title alone, a server such as Gonic matching the whole query against the title), `SONGS_A_PAGE`
  at a time up to `PAGES_AT_MOST`. A song is taken only where its `musicBrainzId` is the recording or
  its `isrc` (one code or a list, as OpenSubsonic writes it, read through `Isrc::new`) holds the
  want's, so a tribute band's *Echoes* is never delivered for Pink Floyd's.
- **An answer has a deadline, and a download one for silence alone.** API requests carry
  `Patience::answered_within` as the whole-answer `timeout_global`. A download may run for minutes, so
  its agent bounds the head alone and `stall.rs` bounds each read (`BrokenOffAfter`, a connector
  chained after ureq's `DefaultConnector`, caps each read at `Patience::broken_off_after`): a silent
  socket is closed while one still delivering is read however long it takes. The connector is ureq's `unversioned` API, outside its semver promise; a ureq bump is weighed
  against the stall tests.
- **A server behind a private CA is reached.** Both agents trust `trust::system_and_built_in`: the
  Mozilla roots plus the system's store. A certificate the handshake still refuses is
  `Error::Untrusted`, not a refused connection.
- **It delivers the server's original file, and only a file**, keyed by the song's id and hinted by its
  `suffix` (one that is no `Extension` answers `Nothing`). The API answers a failed download with its
  error document and a 200, so a download whose `Content-Type` names text, JSON or XML is read as that
  document and never streamed as a song.
- **It paces itself**: requests `ASKED_APART`, a 429 or 503 asked again after `Retry-After` (capped at
  `LONGEST_RETRY_AFTER`) up to `RETRIES_AT_MOST`, then the `Refused` it was.
- **The password never leaves as typed.** Every request carries the user, a fresh salt and `t`, the
  MD5 of password and salt (the API's token scheme); the User-Agent is bare, and `Server`'s `Debug`
  prints `<withheld>` for the password. `status: failed` is `Error::Unwelcome` where the code is about
  the account (`ACCOUNT_REFUSALS`) and `Error::TurnedAway` otherwise (70, a song the server lacks).

## A TIDAL account

`resonate-tidal` is the listener's own subscription, named by `tidal-client-id`, `tidal-client-secret`
and `tidal-refresh-token`; `providers::registry` registers it after the inbox and the Subsonic server
only where the client id and the refresh token are given (the secret is sent where given) and `online`
is on. Nothing is downloaded to play: a delivery is fetched whole and lands as a track row.

- **Asked by the link and the ISRC, never by a title.** The TIDAL track MusicBrainz links the recording
  to (`Identity::track_on(Service::Tidal)`) is asked first; failing that, the OpenAPI is asked for the
  ISRC and a listing is taken only where its own `isrc` is the want's, at most `TRACKS_TRIED_AT_MOST`.
  A want with neither answers `Nothing` with no request and no sign-in.
- **It signs in with the listener's refresh token and nothing else.** No client id or secret is built
  in: the listener brings the application the token was issued to. A 400 or 401 at the token endpoint
  is `Error::Unwelcome` under `ProviderOp::SignIn`, which the seam reads as the provider away. **A
  token TIDAL rotates is handed back**: the provider signs in with the new one from then on and calls
  what `Tidal::telling` registered, once per rotation; the binary's `Renewed::note` writes it to
  `tidal-refresh-token` in the settings file the run was read from (`config::store`) and remembers it
  against the token the settings gave, so a provider rebuilt for the same settings uses the rotated
  token, not the refused one. A 401 from the API signs in again once and asks again; a second is
  `Unwelcome` with TIDAL's `subStatus`, except 4005 (asset not ready for playback), the want's,
  `TurnedAway`. A 403 or 404 is the track unavailable to this account or country: `Nothing`.
- **Only the whole track, lossless and in the clear, is taken.** `HI_RES_LOSSLESS` is asked for; an
  `assetPresentation` other than `FULL` (the preview a lapsed subscription is given) is `Nothing`.
  `manifest.rs` reads both manifests TIDAL answers (a base64 BTS JSON naming one URL, and a DASH MPD,
  selecting its FLAC rendition). Encryption is `Withheld::Encrypted` and no FLAC rendition
  `Withheld::Lossy`; both answer `Nothing`: the provider decrypts nothing and keeps no lossy stream.
- **Media is fetched from TIDAL's audio hosts alone.** `MediaHosts::holds` takes a URL only over
  `https` whose host is `audio.tidal.com` or under it, refusing a user-info `@`, a bracketed literal
  and a host merely ending in the name; one URL off them fails the whole manifest as
  `Error::OffItsHosts` before a byte is fetched. The media agent follows no redirect.
- **A segment that breaks off is asked for again from where it stopped** with `Range`, up to
  `RESUMES_AT_MOST` times running. The first segment is opened inside `obtain`, so a CDN refusing it
  is the provider's `Refused` under `ProviderOp::Download`, not a failed keep.
- **What lands is a FLAC file, never an MP4.** A DASH stream is FLAC frames in fragmented MP4, and
  `remux.rs` repacks it decoding nothing: `fLaC` and the metadata blocks from the `dfLa` box, then
  every `mdat` payload, STREAMINFO's total sample count (zero in a fragmented file) filled in from the
  timeline. Every delivery is keyed `track/<id>` with the extension `flac`.
- **It is signed in to from the window or `resonate tidal`.** `resonate_providers::SignsIn` is the seam, `TidalSignIn` its
  one implementation, handed to the window as `Lookups::signs_in` by `providers::signs_in`. The device
  flow polls every `interval` (at least `ASKED_EVERY_AT_LEAST`, longer on `slow_down`) until a refresh
  token, `AuthorizationLapsed`, `AuthorizationDenied` or the cancel (`Ok(None)`); the device code is
  never printed by `Debug`. The *A TIDAL account* group's *Sign in to TIDAL* writes the token into
  the field, the global `Online` and `tidal-refresh-token`.
  `resonate tidal` is the same flow without a window (`providers::sign_in_to_tidal`): it refuses with
  `OnlineOff` or `NoTidalClient` before asking, prints the link and the code, waits with the first
  interrupt as the cancel (`SignInCancelled`) and writes the token to the settings file with `config::store`
  (`a_tidal_sign_in_with_online_off_or_no_client_is_refused_before_anything_is_asked`).
- **It paces and identifies itself as the Subsonic client does**, and `Account`'s `Debug` prints neither
  secret nor token. `asker.rs` is the pacing, retrying and reading both TIDAL providers share, and
  `played.rs` what follows a playback answer, with `played::obtained`, the link-then-ISRC order,
  written once over the `Finds` trait each implements.

## A hifi-api server

`HifiApi`, in the same crate, is the second way to a TIDAL subscription. With `online` on,
`providers::registry` registers it after `Tidal` as `hifi-api`, using a hosted service by default; the
`hifi-api` setting and the *hifi-api server* field override it with a server the listener runs, and
clearing the field restores the hosted one. The hosted service holds no listener credential: its
search and playback tokens are cached until shortly before expiry and sent only to their own issuer or
API. `played.rs` reads the manifest through the same checks as a TIDAL account; the manifest URL the
service returns must be HTTPS on `manifest.tidal.com` or a subdomain.

- **Asked by the link and the ISRC, never by a title**, through `played::obtained`: the linked track
  first; otherwise the hosted flow searches TIDAL's web API by title and artist and takes only listings
  whose own `isrc` is the want's, up to `TRACKS_TRIED_AT_MOST`. A custom server keeps the hifi-api
  routes with `data.items` checked by ISRC.
- **A request the server queues is waited for, and withdrawn past the provider's patience.** A server
  whose accounts are all busy answers `202` with a `requestId`; the provider asks
  `/playback/requests/<id>` again after its `Retry-After` (held between `QUEUE_LOOKED_AT_LEAST_EVERY`
  and `QUEUE_LOOKED_AT_MOST_EVERY`) for at most `QUEUED_FOR_AT_MOST`, inside the poll's
  `ANSWERS_WITHIN`, then sends `DELETE` and answers `Error::StillQueued`, which the seam reads as the
  provider away. The request id is used only where it is letters, digits and dashes, and the path is
  the provider's own, never the `statusUrl` the server names.
- **A 401 is the server's account turned away**, `Unwelcome` and away; a 403 or 404 is that track
  unavailable, `Nothing`; anything else is `Refused`. A custom server gets no `Authorization` header.
- **A custom server is trusted as the Subsonic server is**: `HifiApi::at` asks through
  `Asker::of_the_listeners_server`, whose agent trusts the system's store beside the built-in roots
  (`trust.rs`); the hosted service, TIDAL's API and its CDN keep the built-in roots alone. A
  certificate refused anywhere in the crate is `Error::Untrusted` through `asker::unreached`.

## A Monochrome server

`resonate-monochrome` asks the track streamer behind [Monochrome](https://monochrome.st). With
`online` on, `providers::registry` registers it after `hifi-api` as `monochrome`, asking
`tracks.monochrome.st` by default; the `monochrome` setting and the *Monochrome server* field (in
the *A TIDAL account* group, beside the hifi-api override) name another server, and clearing the
field restores the hosted one. The binary's `Hosting` (`Hosted` or `Custom`) is what both
overridable services are built from. Two routes are all it uses: `/search/tracks?q=<words>&limit=`
answers `tracks` listings (`trackId`, `isrc`, `playable`), and `/track/<trackId>` the whole FLAC.

- **Asked by the ISRC alone, never by a title.** The listings carry no MusicBrainz id and the
  search does not read an ISRC as its query, so a want with no ISRC answers `Nothing` with no
  request; otherwise it is searched in words (title and artist, then the title alone,
  `LISTED_AT_MOST` listings) and a listing is taken only where its own `isrc`, read through
  `Isrc::new`, is the want's and it is not `playable: false`. A `trackId` is used only where it is
  all digits, since it goes into the path.
- **What lands is the server's FLAC**, keyed `track/<trackId>` with the extension `flac`, opened
  inside `obtain` so a refused download is the provider's `Refused` under `ProviderOp::Download`. A
  404 or 410 there is the track gone, `Nothing`; a download whose `Content-Type` names text, JSON or
  XML is `Unreadable`, never streamed as a song.
- **A download that breaks off is asked for again from where it stopped** (`fetched.rs`) with
  `Range: bytes=<read>-`, up to `RESUMES_AT_MOST` times running: a `206` whose `Content-Range`
  starts there is read on, a `200` is read past what was already held. A file runs to hundreds of
  megabytes, so each read, not the whole, is bounded: `stall.rs`'s `BrokenOffAfter` as Subsonic's.
- **It paces itself as Subsonic does**: requests `ASKED_APART`, a 429, 502, 503 or 504 asked again
  after `Retry-After` up to `RETRIES_AT_MOST`, then the `Refused` it was. The User-Agent is bare.
- **A custom server is trusted as the Subsonic server is** (`trust.rs`, the system's store beside the
  built-in roots); the hosted service keeps the built-in roots alone. A refused certificate is
  `Error::Untrusted`.
