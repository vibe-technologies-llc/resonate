---
paths:
  - "crates/resonate-musicbrainz/**"
  - "crates/resonate/src/musicbrainz.rs"
  - "crates/resonate/src/online.rs"
  - "crates/resonate-mobile/src/enrichment.rs"
  - "crates/resonate-mobile/src/sync/consulted.rs"
  - "fuzz/fuzz_targets/musicbrainz.rs"
---

# MusicBrainz kept here, asked of a device, and the answers kept

`resonate-musicbrainz` sits between the library's `Reference` seam and `resonate-online`. It reaches
core and `resonate-library` (for the vocabulary), with `resonate-fetch` and `ureq` only under its
`download` feature, which the binary turns on under `online`. It is reached by the binary and
`resonate-mobile` alone (the layering job holds both).

## The order a lookup is answered in

**Kept, mirror, a device, the network.** `Layered` implements `Reference`. Each MusicBrainz method
is first turned into an `Asked`. Then:

1. The kept answers are read (`Kept::answer`).
2. If none answers, the mirror is asked (`Mirrored::held`, swapped live as a build lands or the
   setting moves).
3. If the mirror misses, the devices `Peers` names are asked.
4. Last, the fallback `Online` is asked.

A miss falls through every layer: an empty or `None` answer from the mirror or a device is no
verdict, since the export is up to a month old and MusicBrainz's own search is fuzzier. An answer a
later layer gives is kept (`Layered::kept_here`). A local failure (a store that will not read) is
logged and passed over, never a library error, so only the network's errors steer the pass as
before. The methods MusicBrainz does not answer are the fallback's alone, or `Ok(None)` without
one: covers, portraits, lyrics, `streamed_at`, `*_linked`, `share_file`, `artist_at`
(`a_device_holding_the_answer_is_asked_before_the_network_and_what_either_answers_is_kept`,
`the_network_is_asked_only_where_nothing_nearer_answers_and_its_answer_is_kept`).

**The binary composes it in `online::reference`.** `musicbrainz::layered` wraps `Online` (present
only while `online` is on) with the kept answers, the mirror and the `Registered` peers, which the
sync start fills. With `online` off the reference is the layers alone, and only where a mirror is
held. Otherwise it is `None`, as before, so `online = false` still sends nothing to the internet
and `enrich` still says `OnlineOff`. The phones wrap their `Online` the same way
(`Asking::layered_by`), with kept answers and peers and no mirror.

## The vocabulary on the wire

**`Asked` and `Answer` are hand-encoded, a version byte first** (`asked.rs`, over `wire.rs`'s
LEB128 `Writer` and `Reader`, MBIDs as 16 raw bytes). One encoding serves the kept store, the peer
payloads and the mirror's documents. `Asked::key` is the encoding of the ask with every searched
text folded (`folded_letters`, trimmed), so `ÉCHOES ` and `echoes` share a key.

**An answer must answer what it names** (`Asked::is_answered_by`):

- An id lookup's entity must carry that id.
- An ISRC's recordings must each list that code.
- A search must be the matching kind.

So a device cannot poison a kept answer under another key
(`answers_taken_from_a_device_are_marked_and_never_told_back_to_it`). `fuzz/fuzz_targets/musicbrainz.rs`
decodes both and requires a decoded value to read back as itself.

## Kept answers

- **The store.** `Kept` keeps every non-miss answer this device obtained from any layer in
  `kept.sqlite` (the data folder's `musicbrainz/`; the phones' sync folder's). Each row is
  zstd-packed (`KEPT_PACKED_AT`) under its key, with `at`, a `seq` and the device it `came_from`.
- **A `seq` never goes back.** It is drawn from `numbered`, a one-row counter, never from
  `max(seq)`, so the open's sweep taking the newest answer cannot hand its number out again under a
  mark a device has already passed. A device asking after a mark past the counter (a store swept
  before the counter, or one made anew) raises it there, so what is kept next is told to it; a mark
  past `A_MARK_NO_DEVICE_REACHES` is not one this store gave and moves nothing
  (`an_answers_number_never_goes_back_when_the_newest_is_swept_so_a_device_past_it_is_told_the_next`,
  `a_device_holding_a_mark_past_every_number_here_is_told_what_is_kept_next`).
- **Answers are found by id too.** `entities` indexes every whole entity an answer holds (a
  release, a recording, a group, an artist, every pressing of a list), so `recording(id)` is
  answered by an ISRC answer that held it
  (`an_entity_learnt_inside_another_answer_answers_a_lookup_by_its_id`).
- **Freshness.** An answer serves for `KEPT_FRESH_FOR` (30 days, the library's refresh). Older ones
  are deleted when the store opens and never taken from a device. A miss is never kept, so it is
  always asked again (`a_search_is_kept_under_its_folded_words_and_a_miss_is_never_kept`,
  `a_kept_answer_older_than_a_month_is_not_served`).
- **The schema is `V1` plus `MIGRATIONS`**, with `user_version` counting the steps laid; the
  second step is `numbered`, seeded at the store's highest `seq`
  (`answers_kept_before_they_were_numbered_go_on_from_the_newest_number`). A later
  build's store is `Error::KeptVersion`
  (`kept_answers_laid_out_by_a_later_build_are_refused_and_this_builds_open_again`).

## The mirror

**What it holds is the full export.** `mirror::import` streams `mbdump.tar.bz2` and
`mbdump-derived.tar.bz2` (`bzip2`'s pure-Rust backend, `MultiBzDecoder`, `tar`) and reads each
table `copy::TABLES` names as PostgreSQL COPY text. A field is tab-separated, `\N` is null, and
backslash escapes are read, octal and hex included (`\x` with no hex digit is an `x`, as PostgreSQL reads it). A row that is short of the projected columns is `Error::Columns`, and a final row with no line end is `Error::CutRow`, since PostgreSQL ends every row with one: a cut table is refused, never half kept. Only the projected columns go into
`musicbrainz.staging.sqlite`.

- **Column positions follow MusicBrainz's `CreateTables.sql`** at schema sequence 31. A row shorter
  than a projection needs is `Error::Columns`, and a table missing from its archive is
  `MissingTable`, so a schema change refuses the build rather than misreading it.
- **The staging is indexed, then the small tables are read into memory** (`LookedUp`: names of
  types, credits, release groups and releases by id, about a gigabyte for the full export). The
  documents are then written kind by kind.
- **Every read during the build is in order, never at random over the staging.** A random read per
  track over a 16 GB staging was fine while the page cache held it (3 700 releases a second) and
  fell to 500 a second once other work took the cache, which on a 16 GB machine would be a day.
  So the staging is read only in ways that stay in order:
  - **Covering indexes.** Each staging index covers the columns its readers take (`copy::TABLES`'
    `indexed`), so a read never goes back to the table.
  - **Main tables in id order.** Each kind's main table is walked `ORDER BY id`, and what a document
    holds is read off an index in the same order.
  - **Tracks placed with their recordings.** A release's tracks need their recording's id, length
    and codes. `placed_with_their_recordings` merges `track`, `recording` and `isrc`, all walked in
    recording order, into `placed_tracks` (`WITHOUT ROWID`, keyed by medium and position, inserted
    sorted). A release then reads its media's tracks almost in order, since media are numbered as
    releases are.
- **One document per entity:**
  - A release (`ReleaseDoc`: the `Release` the online client answers, with its status).
  - A recording (`Recording`, with at most `RELEASES_OF_A_RECORDING_KEPT` releases, earliest
    first, each placed by disc and position).
  - A release group (`GroupDoc`: with its pressings' track counts per disc and its secondary
    types).
  - An artist (`ArtistDoc`: the profile, its album, EP and single groups up to
    `GROUPS_OF_AN_ARTIST_KEPT`, and how many it is credited on).
- **The documents mirror the online client's mapping.** A release's date and country are its
  earliest event. Its front cover is `release_meta`'s `present`. A label of `[none]` or
  `[no label]` states nothing. A track's own credit is kept only where it differs from the
  release's. An artist's country is its area's ISO code, and its genres are its tags with a
  positive count, heaviest first.

**It stays compressed and indexed** (`musicbrainz.sqlite`, format `FORMAT`):

- **Blocks.** Documents are packed into blocks of about `BLOCK_BYTES` (64 KiB), each addressed by
  the ordinal of its first document. Each kind has its own zstd dictionary, trained on the first
  `SAMPLED_BYTES` of that kind.
- **Packing.** Blocks are packed at `BLOCKS_PACKED_AT` on every core but one.
- **Indexes.** An MBID (`entities`), an ISRC (`isrcs`) or a barcode (`barcodes`) leads to an
  ordinal. The ordinal is the rowid of the contentless, `detail=none` FTS5 tables of folded words
  (`recording_words`, `release_words`, `group_words`, `artist_words`).
- **Reading.** A lookup by id is one index probe and one block unpacked; the last
  `BLOCKS_KEPT_UNPACKED` blocks are kept.

**Search scores as MusicBrainz's does** (`mirror/search.rs`).

- **Candidates.** Up to `CANDIDATES_WEIGHED` are taken by bm25. `Phrase` requires every title word
  and, where an artist is asked, every artist word. `Words` requires the title's alone.
- **The score** is `agreement` of title times `credit_agreement`, out of 100:
  - Folded titles alike are whole.
  - Titles alike once `titles::bare` strips a qualifier are 0.96.
  - Otherwise a title scores its shared words at most 0.9.
  - A credit agrees by MBID, else by name.
- **Only whole agreement scores 100.** So the library's strict rules (95 and 100) take exactly what
  they would take from MusicBrainz and nothing looser
  (`a_local_search_scores_whole_agreement_and_nothing_less_at_a_hundred`).
- **What a `Phrase` also holds to:**
  - The duration within `LENGTH_MAY_DIFFER_BY` (10 s, the online window).
  - The release named.
  - The barcode and catalogue number given.
  - A group's year.

**A build is held, staged and swapped, never edited in place.**

- **The hold.** `held_for_building` holds `musicbrainz.building` (`writer::Held::taken_over`); a
  second build is `AlreadyBuilding`.
- **Before.** Leftover staging and a `.building` file, from a run that died, are removed first.
- **After.** The new store is `musicbrainz.sqlite.building` until `settled` renames it over the old
  one. The staging and placement files are removed whatever happened.
- **Cancelling.** `Building` carries the phase, the bytes and rows read, the documents written, and
  a cancel. `gaming::wait` holds it between batches.

**What the build costs, measured on the export of 2026-11-04** (sixteen cores, 30 GB of memory
under other work): 274 million rows read in 16 minutes (the export is one bzip2 stream, so the read
is one core's), 54 million documents written in 40 (releases about 6 000 a second, recordings 60 000),
71 minutes in all. The staging peaks at 41 GB beside the 7.5 GB download; the copy kept is 10.3 GB.
A lookup by id answers in under 3 ms, a strict search with its artist in about 10 to 70 ms. A bare
common title asked in words alone is the slow case (*love* about 2.5 s), so a loose search with an
artist first requires one of the artist's words (`asked_queries`) and asks the title alone only where
that finds nothing.

**A mirror is a cache rebuilt from the dump, not stored data.** A file of another `FORMAT` is
`MirrorFormat`, left serving nothing until the next build replaces it. No migration is owed, since
the export it came from is the source.

## Fetching the export

**The download (`download.rs`, feature `download`).**

1. `latest` reads `LATEST` under `DUMP_SERVER` (a name of digits and dashes alone).
2. `fetched` reads `SHA256SUMS`, then downloads both archives into `downloads/<dump>/`.
   - Each download is resumable: `.part` grows through `Ranged::opened_from`, under `Held`.
   - It waits while the download gate is shut (`stall-downloads-on-metered`).
   - Each archive is checked against its sum before being renamed whole and marked `.verified`.
   - A mismatch removes the part and is `Error::Checksum`.
   - A part already whole (a cancel during the checksum, a kill before the rename) is answered
     `416` when ranged past its end: that goes straight to the checksum, and a part that fails it
     is fetched again once from the start, never refused for good
     (`a_whole_part_the_server_answers_past_the_end_is_verified_or_fetched_again`).
3. Older dumps' folders are removed.

What was downloaded goes once the build lands (`forget_downloads`). A build `--from` a folder
leaves that folder alone.

**The binary keeps it fresh.** With `musicbrainz-mirror` on and `online` on, the window looks at
start (`musicbrainz::keep_fresh`). With no copy, or a copy built over `LOOKED_AT_AGAIN_AFTER`
(30 days) ago from a dump `LATEST` no longer names, it builds in the background
(`build_in_the_background`, one at a time). The settings pane's switch calls `musicbrainz::follow`
live.

**`resonate musicbrainz`** prints what is held and how many answers are kept.

- `--build [--from FOLDER]` builds in the foreground, logging the phase every few seconds; the
  first interrupt cancels it.
- `--remove` removes the copy and its downloads.
- `--search WORDS` lists recordings by score.

## Between devices

- **Consulted.** `Consultation` is what a device serves (`resonate_sync::Consultable`, adapted in the
  binary's `musicbrainz.rs` and the phones' `sync/consulted.rs`). A lookup is answered from the
  mirror, misses included, since a held export's miss is an answer. Without a mirror it is answered
  from the kept answers, else `Unconsulted`.
- **Asking.** `Consulting` asks the online devices, nearby first. A device that answered
  `Unconsulted` or nothing is passed over for `CONSULTED_AGAIN_AFTER` (10 minutes); only a device
  holding a mirror is worth asking every time.
- **The kept answers travel device to device** as `sync.md`'s *Peers* says: on meeting, each pulls
  what the other kept after the mark it holds for that device, never the server.
