---
paths:
  - "crates/resonate-providers/**/*.rs"
  - "crates/providers/**/*.rs"
  - "crates/resonate/src/providers.rs"
  - "crates/resonate-library/src/supply.rs"
---

# Providers

A provider is a crate that turns an identity into media. `resonate-providers` is the seam, the
crates under `crates/providers/` fill it, the binary registers them and `resonate-library`'s poll
does everything else. The seam is on `resonate-core`, `thiserror` and `tracing` alone, and
`cargo tree -p resonate-providers` must stay free of the library, the codec, the vault and gpui: a
provider crate that depended on the catalog could not be written without it.

## What a provider is handed

`Identity` is everything the catalog knows about the row it wants filled:

- `recording`, `track` and `release` as `Option<Mbid>` and `isrc` as `Option<Isrc>` — the
  identifiers, most exact first.
- `title`, `artist`, `album`, `length`, `disc` and `position` — what a service would search on.
- `links` and `release_links` — the `Link`s MusicBrainz gave the track and its release, each a
  `Relation`, a `Service` and a URL. `track_on(Service::Tidal)` and `release_on(Service::Tidal)`
  are how a provider for one service finds its own page; the track's link names the recording on
  that service and the release's the album, and a provider that can use either asks for the track
  first.

A want's identifiers come out of `release_tracks` and `albums.mbid`, so a row the enrichment has
not answered carries its titles alone. A provider that needs an identifier answers `Nothing`
without one rather than guessing — **a guess is never written**, and `resonate-inbox` never matches
on a title for that reason.

## What a provider answers

`Provider::obtain` answers `Obtained::Nothing` or `Obtained::Found(Delivery)`, and an `Err` where
it could not be asked — which `Providers::first` logs, counts as `refused` and carries on past.

- **`Delivery::File(PathBuf)`** is audio already on disk. The vault reads it and copies what it
  keeps, and the file is left where it stood: a provider's folder, like the library an import
  reads from, is never written to.
- **`Delivery::Stream { key, extension, reader }`** is bytes from anywhere. `key` is the provider's
  own name for what it delivered, so the vault object is recorded as taken from
  `<provider>:<key>`; `Extension` is one to eight ASCII letters and digits, lowercased and without
  its dot, and is the format hint the decoder probes with.

## What the infrastructure owns

A provider does none of this, so none of it is written twice:

- which wants are due — `POLL_AGAIN_AFTER` since the last try — and asking the providers in the
  order they were registered, the first delivery winning;
- how long a provider is waited on. `Providers::first` takes an `Asking` — `within`, the poll's
  `answers_within` and `ANSWERS_WITHIN` by default, and a `cancelled` the poll reads off its own
  progress — and asks each provider on a thread of its own, looking at the two every
  `LOOKED_AT_EVERY`. One that has not answered by the deadline is left behind and counted `late`
  rather than `refused`, since it said nothing was wrong, and the next is asked; its thread runs
  on to whatever end it reaches and what it answers is dropped. A cancel ends the wait at once and
  asks nobody else, and the want is not stamped as tried, because it was not;
- how long a stream is read. `Pumped` reads the reader a delivery hands over on a
  `resonate-delivery` thread of its own, `CHUNK_BYTES` at a time and at most `CHUNKS_AHEAD` ahead,
  and the keep reads the chunks off a channel looking at the cancel every `HEEDED_EVERY`. A cancel
  answers an error on the next read, so the staging is thrown away and the want left untried
  rather than a cancelled pass copying on to `LARGEST_DELIVERY`; a stream that yields nothing for
  the poll's `answers_within` is given up the same way and counted `late`, and the want is stamped
  as tried, because the provider did answer and what it answered stopped. The pump thread is left
  behind in the `read` that never returned, as a late provider's is, and ends on the first read
  that does, its channel having gone;
- staging, validating, deduping and keeping, through `Vault::keep` and `Vault::keep_delivered`,
  so what a provider delivers is held to the same bit-exact, never-larger promise as an import;
- **turning what was kept into a track row.** `Library::note_delivered` writes the
  `vault_objects` row and a `tracks` row in one transaction and pairs the want's release track
  with it, so a delivery is playable, searchable and counted as held the moment it lands. The
  row is named by the object's own path, as a vaulted row whose file has gone is, with
  `vault_key` and `vault_path` set, so the stand-in answers for it and `--prune` spares it; its
  names, disc, number and identifiers are the release track's and its album is the want's,
  because the object carries no tags and the release is what the want was identified against. It
  belongs to **no root** — `tracks.root_id` is nullable for exactly this — so a scan's prune, a
  forget, `organise`, `tag`, `vault --import` and `vault --release` never reach it, each of them
  joining `roots`: an object in the vault is not the user's library to move, write into or hand
  back. Where one object is delivered for two wants the second is paired with the row the first
  made, and that row keeps the first want's names in the search index as well as on the row —
  `one_object_delivered_for_two_wants_is_searched_for_by_the_row_it_stayed`;
- recording what landed on the want — `offered` the object's URI — and counting `offered`,
  `kept`, `unkept`, `nothing`, `refused` and `late`. A want whose release track holds a row is
  `Want::held` and is never due again, so a filled want is kept as the record of where its
  delivery went and taking it away changes nothing about the row.

## Writing one

1. A crate under `crates/providers/<name>/`, package `resonate-<name>`, depending on
   `resonate-core` and `resonate-providers` and whatever it needs to reach its source. A network
   provider reaches `ureq` itself rather than through `resonate-online`, and paces and identifies
   itself the way `online.md` says a request does — by name and version and nothing else.
2. A workspace member and a `[workspace.dependencies]` entry.
3. A dependency of the binary and one `.and(Arc::new(..))` in `providers::with_inbox`, gated on
   whatever `Config` key says it is wanted. The window builds its registry through the same
   function, handed over as `resonate_ui::Sourcing::register` beside the settings it reads, so a
   folder chosen in the settings pane's *The inbox* group is polled from at once rather than from
   the next run; a provider with a key of its own widens `Sourcing` and that function together.
   The window polls on its own as well as on *Poll now*: once `FIRST_ASKED_AFTER` a start and
   every `ASKED_EVERY` after it, and as soon as a want is marked, each time only where a provider
   is registered, nothing else is running and `Library::is_a_want_due` says a want is due, so an
   idle window with nothing wanted reads the wants and does nothing else. A poll the window
   started on its own raises no notice when it cannot run and clears none when it does. A press
   is different in one more way: *Poll now* and `resonate poll --again` poll under
   `PollOptions::ASKING_EVERY_WANT`, so every want not yet held is asked whenever it was last
   tried, because somebody who has just dropped a file in the inbox means *now*; the timer and a
   bare `resonate poll` keep to `POLL_AGAIN_AFTER`, which is what spares a network service. A provider is registered in code or not at all; there
   is no loading at run time.
4. An `Error::Io` names the provider and a `ProviderOp`; an error a provider raises that the seam
   has no variant for is added to the seam when that provider lands, with its op, never as prose.

`resonate-inbox` is the reference: `Inbox::at` over the folder the `inbox` key names, one directory
read per want, a file directly inside whose stem is the recording MBID, then the track MBID, then
the ISRC, ignoring case, and never a nested folder.
