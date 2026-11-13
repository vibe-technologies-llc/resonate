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
(`Asking::layered_by`), with kept answers, peers and the copy a desktop gave them where they keep one
(*A copy handed to another device*).

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
- **An artist's pressings are listed once.** `releases_of_artist` flattens the artist's groups into
  one list of release ids and pages that; the list of the last `ARTISTS_KEPT_LISTED` artists is kept,
  so a page after the first unpacks only the releases it returns.

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

**A build is held, staged and swapped, never edited in place** (replication too: *Following the
changes*).

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
     Its total is the whole archive's (`Ranged::total`, else the length to come plus what the part
     held), so a resumed one never reads past it (`whole_size`).
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
(`build_in_the_background`, one at a time). `LATEST` is asked past the
metered lookup gate, never while it is shut. The settings pane's switch calls `musicbrainz::follow`
live.

**Online off and the mirror off stop a build.** A build reads the settings file as it stands
(`musicbrainz::current`), never the config cloned as the window opened: `build` refuses a download
with `OnlineOff`, a landing build holds the mirror only while `musicbrainz-mirror` is still on, and a
build is looked at again before it starts once `LATEST` is answered. `online::reach(false)` and
`follow(false)` cancel the build running (`stop_building`); a cancelled build is no failure the pane
reports.

**`resonate musicbrainz`** prints what is held and how many answers are kept.

- `--build [--from FOLDER]` builds in the foreground, logging the phase every few seconds; the
  first interrupt cancels it.
- `--remove` removes the copy, its tables, its amendments and its downloads
  (`removed_beside_the_mirror`).
- `--replicate` applies the packets published since (*Following the changes*).
- `--search WORDS` lists recordings by score.

## Following the changes

**MusicBrainz publishes a replication packet every hour, and a copy built with its tables follows
them** (`mirror/replicated.rs`, `tables.rs`, `amended.rs`). A packet is
`replication-<N>-v2.tar.bz2` under `REPLICATION_SERVER`, asked with the listener's MetaBrainz
token in the query string (`download::packet`; a 404 is a packet not yet published, 400, 401 or 403
`TokenRefused`). Its `mbdump/pending_data` lines are dbmirror2's JSON, one change a line, read by a
parser written here (no serde in this crate): `packet_read` keeps only the tables `copy::TABLES`
projects, maps each row to the projected columns (`projected`: a column missing is `PacketColumn`,
a boolean is PostgreSQL's `t` or `f`) and orders the changes by their transaction's last change,
then their own. `REPLICATION_SEQUENCE` naming another number than the one asked is
`PacketSequence`.

- **The tables are the build's staging, kept.** Where `musicbrainz-replication-token` is set the
  build runs as `Keeping::TheMirrorAndItsTables`: `kept_beside_the_build` adds the indexes a change
  needs to find what it touches (a track by id and by medium, a credit's users, the country rows by
  release, each url link by entity), turns `placed_tracks` into a view and writes the `kept` row
  (`TABLES_FORMAT`, the dump, its `REPLICATION_SEQUENCE` and `SCHEMA_SEQUENCE`); `settled` renames
  it to `TABLES_FILE`. The export's sequence is the last packet it holds, so the next asked is one
  past it; an export naming none is `Unsequenced`. They cost some 30 GB beside the copy. Without the
  token the staging goes as before and nothing can follow (`NoTables`).
- **A change is applied to the tables by id, else by every value.** `applied` deletes the old row
  and inserts the new, matching on `id` where the projection's first column is it (`identified`) and
  on every projected column (`IS`, so nulls match) where not
  (`a_row_with_an_id_is_inserted_updated_and_deleted_by_that_id`,
  `a_row_with_no_id_is_found_by_every_value_it_held_nulls_included`).
- **What a change touches is rewritten whole, from the tables.** `touched_by` names the documents a
  change reaches, read before and after it lands: a recording or its ISRC the recording and the
  releases holding it; a track its release and recording; a medium its release, the release's group
  and the medium's recordings; a release or its country rows the release, its group and its
  recordings; a label the releases carrying it; a group (its meta, its secondary types) the group,
  its releases and their recordings and its credited artists; a credit name its artist and
  everything credited to it; an alias, tag or url link its entity, a url every entity linking it.
  Dimension tables (areas, types, links) touch nothing. `rewritten` builds each touched document
  through the same `Builders` the build uses, over a `LookedUp::asking` that reads names and
  credits per id instead of holding them, so a replicated document is shaped as a built one is.
- **The copy is never edited; the amendments stand beside it.** `AMENDED_FILE` holds every
  rewritten document (`Amending::rewritten`, packed at `AMENDED_PACKED_AT` with the copy's own
  dictionaries) or the mark that it is gone (`Amending::gone`), with its own ISRC, barcode and word
  tables, and `amended_from` (the dump, the last packet applied, when). `Mirror` opens it beside the
  copy for the same dump alone (`Amended::open`): a lookup asks it first, and a base document it
  `supersedes` is left out of codes, lists and searches, so an amended release is found by its new
  title and no longer by its old one. A new build removes the amended file, so the copy keeps one
  dump's worth of base.
- **Packets are applied a batch at a time, in one transaction.** `replicated` takes the build hold,
  asks packet after packet up to `PACKETS_A_BATCH` (24) inside `BEGIN IMMEDIATE`, each change applied
  and its touches gathered, then rewrites every touched document once and commits the tables and
  the amendments together (the amended file is attached), so a failure or a cancel loses the batch
  and nothing is half applied. It stops at the first packet not yet published. Tables of another
  dump than the copy are `TablesOfAnotherExport`; a packet of another `SCHEMA_SEQUENCE` is
  `PacketSchema`, and the copy stays where it stood until the next export's build, which carries the
  new schema
  (`a_replication_packet_rewrites_the_documents_its_changes_touch_and_nothing_is_built_again`,
  `a_packet_of_another_schema_stops_replication_where_it_stood_and_a_new_build_lets_the_changes_go`,
  `a_mirror_built_without_its_tables_cannot_follow_the_changes`,
  `a_packet_naming_another_number_than_the_one_asked_for_is_refused`).
- **The binary follows hourly.** `musicbrainz::follow_the_changes` (started by `keep_fresh`, and by
  the settings pane when the token is given) runs a `resonate-musicbrainz-follow` thread while the
  mirror, Online and the token are on: every `FOLLOWED_EVERY` (an hour) it calls `replicate`, never
  while a build runs, the lookup gate is shut or no tables are held, and `stop_building` cancels it.
  A replication that moved the copy swaps the `Mirrored` held. `resonate musicbrainz --replicate`
  applies what is published now, the first interrupt cancelling the batch; the bare command says
  the packet the copy follows to and when it was applied, and so does the settings pane.

## A copy handed to another device

**A device holding a copy offers it whole, and its amendments as a snapshot** (`mirror/offered.rs`).
`Mirror::offered` answers an `Offered` (the dump, the packet followed to, the size and an
`OfferedFile` reading it): `CopyPart::TheCopy` is the copy's own file, immutable for its dump, read
as it stands; `CopyPart::ItsAmendments` is a `VACUUM INTO` snapshot of the amended file beside it,
named `musicbrainz.amended.offered-<pid>-<n>.sqlite`, held under `writer::Held` while read and
removed when the `OfferedFile` drops, a snapshot whose writer is gone swept by the next offer
(`writer_of`); a copy following no changes offers no amendments. `Consultation::offered` serves it
to sync's `Consultable::export`. The receiver lands them with `received_copy` (opened as a `Mirror`
first, then the amendments of the dump before removed and the file renamed over `MIRROR_FILE`) and
`received_amendments` (opened as amendments of the copy's dump, else
`AmendmentsOfAnotherExport`, then renamed over `AMENDED_FILE`); `removed_copy` takes the copy and
everything beside it away
(`a_copy_and_its_amendments_offered_by_one_device_land_whole_on_another`,
`a_received_file_that_is_no_copy_is_refused_and_the_copy_held_stays`). A phone is the receiver
(`mobile.md`'s *A copy of MusicBrainz fetched from a desktop*); the desktop builds its own and only
offers.

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
