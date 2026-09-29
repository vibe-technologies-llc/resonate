---
paths:
  - "crates/resonate-providers/**/*.rs"
  - "crates/providers/**/*.rs"
  - "crates/resonate/src/providers.rs"
  - "crates/resonate-library/src/supply.rs"
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
  it answered stopped. The pump thread is left in the `read` that never returned, as a late
  provider's is, and ends on the first read that does, its channel gone.
- **Staging, validating, deduping and keeping**, through `Vault::keep` and `Vault::keep_delivered`,
  so a delivery is held to the import's bit-exact, never-larger promise.
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

## A Subsonic server

`resonate-subsonic` reaches a network: a server of the listener's — Navidrome, Airsonic, Gonic,
anything speaking the Subsonic API — named by `subsonic`, `subsonic-user` and `subsonic-password`.
`providers::sourced` registers it after the inbox only where all three are given and `online` is
on, behind the binary's `online` feature, so a build with no HTTP client carries none of it; the
window builds its registry through the same closure (`Sourcing::register`), and the Library
category's *A Subsonic server* group writes the keys for the next start.

- **Asked by the identifiers, never by a title.** A want with neither a recording MBID nor an ISRC
  answers `Nothing` without a request. Otherwise `search3` is asked with the title — the only words
  the API searches — for `SONGS_ASKED` songs, and a song is taken only where its `musicBrainzId` is
  the recording or, failing that, its `isrc` (one code or a list, as OpenSubsonic writes it) holds
  the want's, so a tribute band's *Echoes* is never delivered for Pink Floyd's.
  `a_song_is_taken_by_its_recording_and_then_by_its_isrc_and_never_by_its_title` is the claim, over
  a captured Navidrome answer.
- **It delivers the server's original file.** `download` answers the bytes as they sit on the
  server, as a `Delivery::Stream` keyed by the song's id and hinted by its `suffix`, kept and
  validated like any other; a suffix that is no `Extension` answers `Nothing` rather than a guess.
- **The password never leaves as typed.** Every request carries the user, a fresh salt and `t`,
  the MD5 of password and salt (the API's token scheme), beside `v` and `c=resonate`; the
  User-Agent is `resonate/<version>` alone. `status: failed` is `Error::TurnedAway` with the
  server's code (40 for a wrong password), an HTTP refusal `Error::Refused`, an answer that is not
  the document `Error::Unreadable`, a failed connection `Error::Io`, each naming
  `ProviderOp::Search` or `Download`.
