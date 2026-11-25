---
paths:
  - "crates/resonate-musicbrainz/**"
  - "crates/resonate/src/musicbrainz.rs"
  - "crates/resonate/src/online.rs"
  - "crates/resonate-mobile/src/enrichment.rs"
  - "crates/resonate-mobile/src/sync/consulted.rs"
  - "fuzz/fuzz_targets/musicbrainz.rs"
  - "fuzz/fuzz_targets/replication.rs"
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
later layer gives is kept (`Layered::kept_here`; a device's answer through `kept_as_told`, with
the age and the device it came with, *Between devices*). A local failure (a store that will not read) is
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
A list's count is held to the bytes left, one an item, but an item decodes to a value of hundreds
of bytes, so a list reserves at most `ITEMS_RESERVED_AT_MOST` (256) items ahead and grows past them
as they are read: a device's answer cannot make the receiver reserve gigabytes
(`a_count_past_what_is_reserved_reserves_no_more_and_the_list_still_reads_whole`).

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
- **A lookup by id is answered by its own answer alone.** A recording an ISRC answer holds was
  asked with fewer includes (no release groups), and a pressing a list holds with fewer than a
  release by id (no labels, no links), so taking either for the lookup would land a single as the
  album and refresh a label, a catalogue number and links away. So no entity inside another answer
  is indexed; `recording(id)` misses until that lookup itself is kept
  (`a_thinner_copy_held_inside_another_answer_never_answers_a_lookup_by_its_id`). A store that
  indexed them before drops the index as it opens
  (`a_recording_indexed_inside_an_isrc_answer_before_this_build_no_longer_answers_its_id`).
- **Freshness.** An answer serves for `KEPT_FRESH_FOR` (30 days, the library's refresh). Older ones
  are deleted when the store opens and never taken from a device. **A stamp is never later than
  now**: one a device tells from a clock running ahead is kept as of now (`Kept::take`,
  `keep_told`), and one kept so before this rule is brought back to now as the store opens, since a
  row is replaced only by a newer stamp and a future one would pin the answer on every device past
  the month (`an_answer_a_device_stamped_in_the_future_is_kept_as_of_now_and_ages_as_any_other`,
  `an_answer_stamped_in_the_future_before_this_build_is_brought_back_to_now_as_the_store_opens`).
  A miss is never kept, so it is always asked again (`a_search_is_kept_under_its_folded_words_and_a_miss_is_never_kept`,
  `a_kept_answer_older_than_a_month_is_not_served`).
- **The schema is `V1` plus `MIGRATIONS`**, with `user_version` counting the steps laid; the
  second step is `numbered`, seeded at the store's highest `seq`
  (`answers_kept_before_they_were_numbered_go_on_from_the_newest_number`), the third
  `answers_by_age` (`AN_ANSWERS_AGE_IS_READ_OFF_AN_INDEX`), so the open's sweep of the stale and
  its bringing of a future stamp back to now read only the rows they change
  (`answers_kept_before_their_age_was_indexed_stay_and_are_aged_off_the_index`), the fourth drops
  the index of entities held inside other answers
  (`A_LOOKUP_BY_ID_IS_ANSWERED_BY_ITS_OWN_ANSWER_ALONE`). The steps a release shipped are
  pinned by their text's hash in `kept/released.rs` (`release.md`), since a counted `user_version`
  would leave an edited step and the shipped one apart silently. A later
  build's store is `Error::KeptVersion`
  (`kept_answers_laid_out_by_a_later_build_are_refused_and_this_builds_open_again`).

## The mirror

**What it holds is the full export.** `mirror::import` streams `mbdump.tar.bz2` and
`mbdump-derived.tar.bz2` (`decoded_across`, `mibz2`'s `BzReader::parallel` on every core but `THREADS_THE_READ_KEEPS`,
the one parsing and inserting and the decoder's own reader, then `tar`) and reads each
table `copy::TABLES` names as PostgreSQL COPY text. A field is tab-separated, `\N` is null, and
backslash escapes are read, octal and hex included (`\x` with no hex digit is an `x`, as PostgreSQL reads it). A row that is short of the projected columns is `Error::Columns`, and a final row with no line end is `Error::CutRow`, since PostgreSQL ends every row with one: a cut table is refused, never half kept. A row is read by `row_read`, up to `ROW_LONGEST_BYTES` (8 MiB) so the allocation itself is bounded, and a longer one is `Error::RowTooLong`; an entry the import does not read is read through `drained`, which looks at the cancel as it goes (`an_entry_nobody_wants_is_read_through_checking_the_cancel_it_is_handed`). Only the projected columns go into
`musicbrainz.staging.sqlite`.

- **Column positions follow MusicBrainz's `CreateTables.sql`** at schema sequence 31
  (`SCHEMA_THE_COLUMNS_FOLLOW`), so the schema is pinned: an archive whose `SCHEMA_SEQUENCE` names
  another is `DumpSchema` as that entry is read, and one naming none is `DumpPart`, so a schema
  that inserts or moves a column is refused rather than read into the wrong fields
  (`an_export_of_another_schema_is_refused_before_its_columns_are_read`,
  `an_export_naming_no_schema_is_refused`). The core and derived archives must name the same
  `REPLICATION_SEQUENCE`, else `ArchivesApart`, since tables of two exports follow neither
  (`archives_of_two_replication_sequences_are_refused_as_apart`). A row shorter than a projection
  needs is `Error::Columns`, and a table missing from its archive is `MissingTable`.
- **A table is one plain file.** An entry is matched by its name, so one the import reads (a table
  of its archive or a sequence file) must be a regular file, else `EntryNotAFile`, and come once,
  else `EntryTwice`: a link named `mbdump/artist` is no present, empty table and a repeated entry
  is never loaded twice (`a_table_held_as_a_link_is_refused_and_so_is_one_held_twice`).
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
- **Documents are built on every core but one, and kept in id order.** For each kind,
  `built_by_a_builder` threads, each with a read-only connection of its own to the staging
  (`reader_of`, `BUILDER_CACHE_KIB` of pages; the staging's own connection drops its exclusive lock
  first), build the documents of `IDS_A_CHUNK` ids at a time from the shared `Shared` lookups. The
  writing thread hands out at most `CHUNKS_AHEAD_A_BUILDER` chunks a builder ahead of the one it
  awaits and takes them back in order, so ordinals, blocks and dictionaries are exactly the serial
  build's. A builder that fails, or panics in a build that unwinds, answers its chunk with the
  error; builders all gone is `BuildersStopped`.
- **The ids are walked as held, never as a span of numbers.** A chunk is the next `IDS_A_CHUNK`
  ids after the last one walked (`last_of_a_chunk`), so ids far apart cost no empty chunks
  (`a_recording_numbered_far_past_the_rest_is_built_from_the_ids_held`,
  `recordings_walked_over_many_chunks_are_each_built_once_at_every_boundary`). An id no `u32`
  holds is `IdOutOfRange` (`read_as_an_id`), refused as a kind's ids are first looked at and as
  `Shared` places a held id in a list indexed by it; a list the memory cannot grow to an id is
  `IdsPastMemory`, never an aborted allocation
  (`an_id_past_what_an_id_is_read_up_to_is_refused_quickly`).
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
- **Packing.** Blocks are packed at `BLOCKS_PACKED_AT` on every core but one. A kind's dictionary
  is digested once, as an `EncoderDictionary` every worker shares, never once a block: digesting a
  112 KiB dictionary at that level costs about what packing the block does. Replication's
  amendments do the same (`prepared_for_amending`, once a replication).
- **Indexes.** An MBID (`entities`), an ISRC (`isrcs`) or a barcode (`barcodes`) leads to an
  ordinal. The contentless, `detail=none` FTS5 tables of folded words (`recording_words`,
  `release_words`, `group_words`, `artist_words`) lead to it too, through a rowid that places the
  document by its weight (*Weighed*, below). A release's, recording's
  and group's words also carry one token for its folded bare title (`bare_title_token`: `qt` and
  the FNV-1a of `titles::bare` folded), and an artist's one for each of its names, sort name and
  aliases (`words_of_names`), so a search can ask for the names that can agree before every row
  holding a word (*Search*). A copy built before the artists' tokens has none, which only means its
  artist search is asked without them, so no `FORMAT` step was owed. Each table holds `WORDS_HELD_BEFORE_A_FLUSH` (64 MiB) of terms
  before writing a segment (FTS5's `hashsize`, 1 MiB by default), so 54 million rows leave a few
  hundred segments to merge rather than tens of thousands.
- **Weighed.** A document's weight is how likely it is to be the one asked for: a recording's the
  releases it is on, a group's its pressings, a release's its group's pressings, an artist's the
  groups credited to it (`Document::weight`, counted before any list is cut). `placed_by_weight`
  makes the word row's rowid its `heaviness` (the weight counted exactly up to 3, then in quarter
  octaves, up to 127 steps) above its ordinal's `ORDINAL_BITS` (32), the heaviest lowest, so FTS5,
  which answers in rowid order, answers heaviest first and, among alike weights, in ordinal order;
  `ordinal_of` masks the ordinal back out. A build stages each kind's words in the placement file
  under that rowid while the documents are written and inserts them in its order as the settling
  begins, since FTS5 writes a segment whenever a rowid is not above the last
  (`a_heavier_document_is_placed_before_a_lighter_and_alike_ones_keep_their_order`). A copy built
  before the weights has rowids that are its ordinals, which `ordinal_of` reads as themselves, so it
  is searched in ordinal order as before and no `FORMAT` step was owed; the next build weighs it
  (`a_rowid_placed_by_no_weight_is_its_own_ordinal`). An ordinal past `ORDINAL_BITS` is
  `OrdinalPastWeighing`, a build refused rather than a rowid misread.
- **Reading.** A lookup by id is one index probe and one block unpacked. The block's place is read
  off `blocks_by_first` alone and its packed bytes only on a miss of the last `BLOCKS_KEPT_UNPACKED`
  blocks, kept most recently used last. Each kind's dictionary is digested once as the copy opens
  (`unpackers`, a decompressor a kind), and the connection keeps `MIRROR_CACHE_KIB` (64 MiB) of
  pages, so the upper levels of every index stay in memory.
- **An artist's pressings are listed once.** `releases_of_artist` flattens the artist's groups into
  one list of release ids and pages that; the list of the last `ARTISTS_KEPT_LISTED` artists is kept,
  so a page after the first unpacks only the releases it returns.

**Search scores as MusicBrainz's does** (`mirror/search.rs`).

- **Candidates.** Up to `CANDIDATES_WEIGHED` are taken (`SONGS_FOUND_AT_MOST` for songs), in the
  table's order, which is heaviest first (*Weighed*), never by bm25. FTS5's bm25 is zero for every
  row of a contentless `detail=none` table, so `ORDER BY rank` gave the rowid's order anyway and
  only cost a read of the size of every row holding the words; the copy's tables are asked with
  `LIMIT` alone, the same rows
  (`a_word_table_weighs_every_row_alike_so_its_candidates_are_the_same_unranked`). The score sorts
  them stably, so of candidates scoring alike the heaviest stays first: the recording on the most
  releases, the group pressed most, the artist credited most, though numbered later
  (`of_names_alike_the_one_pressed_or_credited_most_comes_first_though_numbered_later`). The
  amendments' tables keep their content, so bm25 means something there and they are still asked by
  rank, after the copy's.
  `Phrase` requires every title word and, where an artist is asked, every artist word. `Words`
  requires the title's alone.
- **Narrowed first.** Each query of `asked_queries` is first asked with the bare title's token
  added (`narrowed_first`), and so are the songs', albums' and artists' words searches (`find_songs`,
  `find_albums`, `find_artist` through `words_asked`, the typed words taken as a name), so only
  names alike once bare are weighed; the query without it is asked only where that finds nothing.
  The copy and its amendments each take their own first query that finds anything (`first_found`),
  so a copy without the artists' tokens is still searched whole beside amendments that have them.
  Only titles alike once bare score 96 or more, so the strict rules take what they took before,
  from every candidate rather than the table's first 64; a common word asked alone weighs the
  names it is rather than the first rows holding it
  (`a_common_word_is_narrowed_to_the_names_alike_once_bare_before_every_row_holding_it`).
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
- **After.** The new store is `musicbrainz.sqlite.building` until `settled` puts it in place. The
  staging and placement files are removed whatever happened.
- **Put in place in an order no failure tears.** The build writes with no journal and no sync, so
  `settled` first syncs the new copy and tables (`sync_file`) and the folder, then renames the copy,
  then the tables (or removes the old ones where none were kept), and removes the old amendments
  last, syncing the folder after each step (`Replacing`). A failure or a power loss at any step
  leaves the old copy with its amendments or the new one whole: amendments of another export beside
  a copy are passed over as it opens, and old tables beside it are `TablesOfAnotherExport`
  (`a_settling_failing_at_each_step_leaves_the_old_copy_with_its_amendments_or_the_new_whole`,
  each step failed in turn through `settled_stepping`). A file put in place over another has the
  other's `-journal`, `-wal` and `-shm` removed first, so a hot journal of the old file is never
  played back into the new, and a file removed loses them before itself. A leftover `.building`
  file is removed by the next build.
- **Cancelling.** `Building` carries the phase, the bytes and rows read, the documents written, and
  a cancel. `gaming::wait` holds it between batches.
- **Every step is measured.** `Building::bytes` is the step's progress and its whole: the bytes of
  the download, the checksum and the archive read; the indexes made of `copy::TABLES`' `indexed`
  (and `ANALYZE`, capped at `ROWS_WEIGHED_BY_ANALYZE` rows an index, since a whole one reads the
  41 GB staging again); the tracks placed (`Phase::Placing`, of the track rows the read counted);
  the documents written, of the release, recording, group and artist rows read; and the
  `SETTLING_STEPS` of the settling. The settings pane draws a row and a bar for every step
  (`MirrorStep`, mapped from `Phase` by the binary's `step_of`): done, under way with its share
  (gigabytes for the byte steps, a percentage for the rest), or to come.

**What the build cost before `FORMAT` 2 (the prepared dictionaries, the larger FTS5 flush, the capped
`ANALYZE`, the title tokens) and before the decode and the documents were spread over the cores, measured on the export of 2026-11-04** (sixteen cores, 30 GB of memory
under other work): 274 million rows read in 16 minutes (one core's bzip2 then, the decode being
block-parallel since), 54 million documents written in 40 (releases about 6 000 a second, recordings 60 000),
71 minutes in all. The staging peaks at 41 GB beside the 7.5 GB download; the copy kept is 10.3 GB.
A lookup by id answers in under 3 ms, a strict search with its artist in about 10 to 70 ms. A bare
common title asked in words alone was the slow case (*love* about 2.5 s, bm25 reading the size of
every row holding the word). Asked unranked it stops at the rows it takes: on a synthetic contentless
`detail=none` table of 4 million rows, 400 000 holding *love* and 8 000 titled so, warm, the word
alone took 135 ms by rank and 0.02 ms unranked, narrowed 13 ms by rank and 0.04 ms unranked, the rows
taken the same either way. A loose search with an artist still requires one of the artist's words
before asking the title alone.

**Why the weight is the rowid, measured on the same synthetic table** (weights drawn from a Pareto
tail, 56 per cent of rows weighing 1; 64 taken, warm): joining a table of weights and ordering by
it took 6 to 9 ms narrowed to the 8 000 titled *love*, 18 to 24 ms over 100 000 titled *intro*, and
60 ms over the 408 000 holding *love*, growing with every row holding the words as bm25 did, and
probing a weight per row at random over a table of 54 million (a copy whose pages the cache does
not hold reads one page a probe); weighing only the first 4 096 matched cost 0.6 to 3 ms but took
lighter rows (a mean weight of 110 to 127 where the heaviest 64 weigh 202 to 1 368). Tokens for weight bands asked heaviest band first took 2 to 12 ms, each band an
`AND` against a doclist of millions. The local score over a wider set unpacks a 64 KiB block a
candidate (45 to 70 µs each) and still cannot tell apart names that agree alike. The rowid placed by
weight took 0.01 to 0.03 ms, as unranked did, and its 64 were the heaviest within a quarter octave
(mean weight 201.6 of an exact 201.8). Its cost is in the copy: the word tables grew from 117 to
146 MB on the synthetic table, a rowid with its weight above the ordinal taking a varint of 6 bytes
where an ordinal took 4, and the build writes the words to the placement file once more before
inserting them.

**A mirror is a cache rebuilt from the dump, not stored data.** A file of another `FORMAT` is
`MirrorFormat`, left serving nothing until the next build replaces it. No migration is owed, since
the export it came from is the source.

## Fetching the export

**The download (`download.rs`, feature `download`).** `download::agent` is `https_only`, since
`SHA256SUMS` comes over the same agent as the archives and a redirect to plain HTTP would let the
network replace both, and a packet's query carries the token; only the tests' loopback servers are
asked over plain HTTP (`Carried::Plain`, `the_export_is_never_asked_for_over_plain_http`). It
follows no redirect itself (`resonate_fetch::configured`'s none); `LATEST`, `SHA256SUMS` and a
packet are asked through `followed` and each archive through `Ranged`, which follows the same way,
so a redirect is taken only within the origin asked (`providers.md`). MetaBrainz needs no more:
`data.metabrainz.org` serves the export's files itself, and the replication API serves a packet
from its own host through nginx's internal redirect, never a `Location` elsewhere
(`a_redirect_off_the_origin_is_never_followed_for_a_listing_a_packet_or_an_archive`,
`a_redirect_within_the_origin_is_followed`).

1. `latest` reads `LATEST` under `DUMP_SERVER` (a name of digits and dashes alone).
2. `fetched` reads `SHA256SUMS`, then downloads both archives into `downloads/<dump>/`.
   - Each download is resumable: `.part` grows through `Ranged::opened_from`, under `Held`.
     Its total is the whole archive's (`Ranged::total`, else the length to come plus what the part
     held), so a resumed one never reads past it (`whole_size`).
   - It waits while the download gate is shut (`download-on-metered` off).
   - Each archive is checked against its sum before being renamed whole and marked `.verified`.
   - A mismatch removes the part and is `Error::Checksum`, unless the archive grew on a saved part
     (any copy of the pass began past byte 0): then it is fetched again once from the start, so a
     part saved from another file never ends the job for good
     (`a_resumed_answer_failing_its_checksum_is_fetched_again_once_from_the_start`). A whole fetched
     from the start that fails is refused at once, and so is the second pass
     (`a_damaged_whole_answer_still_fails_its_checksum_with_a_saved_part_or_without`).
   - A saved part the answer proves stale before any byte is copied (`Unfetched::Changed`, as a
     `200` shorter than the part, or `Unfetched::Misranged`) is removed and fetched again once from
     the start (`Copied::Stale`), never left for the next attempt to trip over
     (`a_whole_answer_shorter_than_the_saved_part_is_fetched_again_once_from_the_start`).
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

**The listener's Stop stops that build alone, until asked or until the next export.** The settings
pane's *Stop building* (`Mirroring::cancel`, the binary's `stop_this_build`) cancels the build
running and nothing else: `musicbrainz-mirror` stays on and the hourly follower keeps following. The
build's end (`ended`) writes the export it was fetching to `stopped` in the folder
(`remember_stopped`); a cancel by the switches writes nothing, so a build they cut short begins again
at the next start as before. `keep_fresh` then asks `LATEST` even with no copy held, and builds only
for an export the copy held and the one stopped are not (`worth_building`): a stopped build is
never retried by itself for the same export, since the listener stopped it for a reason (room, the
connection, the hour) that a restart does not change, while a newer export is a new build the
listener has not refused. *Build it now*, turning the switch on and any build that lands forget it
(`forget_stopped`). The pane tells `MirrorTold::Stopped` where no copy is held (a held copy is told
as held, since the stop left it as it was), never `Failed`; the `.part` downloads stay for the next
build to resume
(`a_stop_asked_through_the_pane_cancels_the_build_and_is_told_as_stopped_not_failed`,
`a_build_cancelled_by_a_switch_is_not_remembered_as_stopped`,
`a_stopped_build_is_begun_again_by_itself_only_for_a_newer_export`).

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
`TokenRefused`). A packet is streamed from the response into `packet_read`, never held whole, and
read up to `LARGEST_PACKET` (64 MiB compressed): one declaring a larger `Content-Length`, or running
one byte past it, is `PacketTooLarge`, never cut short into a bzip2 or parse error
(`a_packet_larger_than_a_packet_is_read_up_to_is_refused_as_too_large`). What it unpacks to is
bounded too, at `LARGEST_PACKET_UNPACKED` (512 MiB, `PacketInflated`), so a small packet expanding
to a gigabyte is refused after the cap's worth of work
(`a_packet_unpacking_to_a_gigabyte_is_refused_within_a_bounded_read`). The figures are ceilings a
stream of hourly changes stays far under, not measurements of a packet. Its `mbdump/pending_data`
lines are dbmirror2's JSON, one change a line, each read up to `ROW_LONGEST_BYTES` (8 MiB;
`PacketRowTooLong`), read by a parser written here (no serde in this crate) that answers `None` past
`JSON_DEEPEST` (64) levels of nesting, so a column of brackets is a malformed change and no stack
overflow (`json_nested_past_the_deepest_is_refused_where_a_stack_would_overflow`): `packet_read` keeps only the tables `copy::TABLES`
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
- **The amended file is laid in one transaction and checked against the tables.** `made_at` lays
  the schema and its one `amended_from` row together, and keeps a file already there only where
  that row names the tables' dump and sequence; any other (the schema alone, from a creation cut
  short, or another export or sequence) is cleared and laid again, so no amendment is hidden
  while replication reports success
  (`an_amendments_file_holding_only_its_schema_is_laid_again`,
  `amendments_where_the_tables_stand_are_kept_and_any_others_are_laid_again`). A sequence moved
  where no single row keeps it, in the tables or the amendments, is `SequenceUnkept`
  (`a_sequence_moved_where_no_row_keeps_it_is_refused`).
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
  `a_packet_naming_another_number_than_the_one_asked_for_is_refused`). A packet is read off the
  network every hour, so `fuzz/fuzz_targets/replication.rs` reads one whole (`packet_read`) and its
  `pending_data` rows alone (`read_pending_changes`, the hand-written JSON `Parser`); its seeds are
  the packets this suite replicates
  (`the_replication_seeds_hold_the_packets_this_suite_replicates`).
- **The binary follows hourly.** `musicbrainz::follow_the_changes` (started by `keep_fresh`, by
  the settings pane when the token is given, and by `online::reach` when Online comes back on,
  through `musicbrainz::reached`) runs a `resonate-musicbrainz-follow` thread while the mirror,
  Online and the token are on: every `FOLLOWED_EVERY` (an hour) it calls `replicate`, never while a
  build runs, the lookup gate is shut or no tables are held, and `stop_building` cancels it.
  **The hold belongs to one follower.** `begun_following` takes it unless a follower not cancelled
  holds it, so one cancelled and still asleep is no follower, and a follower leaving lets go only of
  its own (`still_following`, `let_go_of`), so a switch turned off and on within the minute leaves
  the successor following
  (`a_follower_stopped_by_online_going_off_is_begun_again_when_it_comes_back_on`,
  `a_cancelled_follower_leaving_cannot_let_go_of_the_hold_of_the_one_begun_after_it`).
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
(`writer_of`); a copy following no changes offers no amendments. SQLite writes and reads the held
snapshot through a `nolock` URI (`unlocked_uri_of`, every letter of the path but `/-_.~`
percent-escaped), and the amended file it is taken from waits for a writer as `Amended` does: on
macOS a `flock` and SQLite's own `fcntl` locks are one lock, so the hold made its own snapshot busy
(`a_held_snapshot_is_taken_in_a_folder_whose_name_a_uri_would_misread`). `Consultation::offered` serves it
to sync's `Consultable::export`. The receiver lands them with `received_copy` (opened as a `Mirror`
first, then synced and renamed over `MIRROR_FILE`, the amendments of the dump before removed last,
as `settled` does) and `received_amendments` (opened as amendments of the copy's dump, else
`AmendmentsOfAnotherExport`, then synced and renamed over `AMENDED_FILE`); `removed_copy` takes the copy and
everything beside it away. Which of two exports is the later is `exported_after`, reading each dump's
name as its figures (`an_export_is_after_another_only_where_both_are_dumps_and_it_was_made_later`), so
a receiver replaces its copy only with a later one
(`a_copy_and_its_amendments_offered_by_one_device_land_whole_on_another`,
`a_received_file_that_is_no_copy_is_refused_and_the_copy_held_stays`). A phone is the receiver
(`mobile.md`'s *A copy of MusicBrainz fetched from a desktop*); the desktop builds its own and only
offers.

**Withheld.** `offer-musicbrainz-copy` (on unless said) lets the desktop keep its copy to itself:
off, the binary's `ConsultedHere::export` answers none for either part (`offered_unless_withheld`,
never asking the `Consultation`), which sync sends as `Missing`, exactly as a device holding no copy
does, so a phone tells it as no device holding one. The setting is read from the file as it stands
on each ask (`offers_the_copy`). It withholds only the whole copy: a lookup a device asks is still
answered from it, one at a time
(`a_copy_withheld_in_the_settings_is_refused_as_one_never_held`). Phones take the copy by default
(`mobile.md`'s *Taken by default*).

## Between devices

- **Consulted.** `Consultation` is what a device serves (`resonate_sync::Consultable`, adapted in the
  binary's `musicbrainz.rs` and the phones' `sync/consulted.rs`). A lookup is answered from the
  mirror, misses included, since a held export's miss is an answer. Without a mirror it is answered
  from the kept answers, else `Unconsulted`. **An answer carries its age** (`Consulted`): a kept
  one the `at` it was kept with (`Kept::answer_dated`), the mirror's now.
- **Kept as told.** A device's answer (`Heard`: the answer, the device, its `at`) is kept with that
  `at` and the device as `came_from` (`Kept::keep_told`), never as this device's own and fresh, so
  it ages from when it was learnt and is never told back to the device it came from; two devices
  cannot keep a stale answer alive by telling it to each other
  (`an_answer_consulted_from_a_device_keeps_its_age_and_the_device_and_is_never_told_back_to_it`).
  A device of an older build says no `at`, and its answer is kept as of now, from that device.
- **Asking.** `Consulting` asks the online devices, nearby first. A device that answered
  `Unconsulted` or nothing is passed over for `CONSULTED_AGAIN_AFTER` (10 minutes); only a device
  holding a mirror is worth asking every time.
- **The kept answers travel device to device** as `sync.md`'s *Peers* says: on meeting, each pulls
  what the other kept after the mark it holds for that device, never the server.
