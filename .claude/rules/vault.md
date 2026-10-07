---
paths:
  - "crates/resonate-vault/**/*.rs"
  - "crates/resonate-library/src/import.rs"
  - "crates/resonate-library/src/vaulted.rs"
  - "crates/resonate-library/src/supply.rs"
  - "crates/resonate/src/vault.rs"
  - "crates/resonate-ui/src/views/settings/library.rs"
---

# The vault

`resonate-vault`: managed archive beside the catalog. Import decodes a track, drops every tag and
picture, keeps the smallest bit-exact copy, validates by reading back, points the catalog row at it.
**The source library is never written** (`the_file_the_vault_read_is_left_exactly_as_it_was`);
`tracks.path` keeps naming the original (rescan reads an untouched source as unchanged, `organise`
keeps filing it). Leaf beside `resonate-codec`, no SQL (rows belong to `resonate-library`);
`cargo tree -p resonate-vault` stays free of gpui, engine, library, `ureq` (CI `refuse`).

## What it promises

- **Bit-exact or nothing.** Every object is read back through the player's decoder, PCM compared by
  MD5. Mismatch: file removed, `Refusal::NotValidated`; no frames: `Refusal::Empty`.
- **Never larger than its source**, unconditional: a re-encode not beating the source is discarded.
  A sheet-cut row cannot keep the file (`Refusal::CutFromAnother`), so it is weighed against its
  *share* (file size x row frames / file frames); no smaller: `Refusal::NoSmaller`. `Kept::was` is
  that weight and what the library's `was_bytes` counts (a single-file rip is not recounted per
  row). **Weighing precedes landing**: `landed_or_standing` weighs the staged object or the one
  under its key, answers `Landing::NoSmaller` without renaming; concurrent imports of one key never
  delete what the other's row names.
- **A kept object is weighed like an encoded one**: `kept_whole` decodes source and staged copy
  through the same `pcm_of`, refuses differing digests; a failing stripped copy is recopied whole
  (decoder read a tag as a frame). `bare` rewrites a FLAC's metadata only if its walk reached the
  last block, else the file is copied as is.
- **One copy of one thing.** Key = MD5 of decoded PCM (one audio in two containers = one object);
  covers keyed by their bytes.
- **Nothing outside the root.** `Vault::inside` guards every read, write, delete
  (`Error::OutsideTheVault`); `within` requires plain names after the root (`<root>/../x` is outside
  though lexical `starts_with` passes).

## The three forms

`Form::of` reads codec, spec, declared depth before decoding; size comparison may overrule.

- **`Form::Flac`**: integer PCM, <= 24 bits, <= 8 channels, <= 96 kHz. Streamed via `flacenc` over
  `resonate_codec::Decoder` (never held whole); STREAMINFO **and nothing else** (stripping is
  construction). **Stops paying once it has lost**: `flac::encode` stops encoding and writing when
  output reaches the source's weight, still hashing samples into the key (`Encoded::outgrew`). An
  encode that outgrew, or whose key already has an object (not a renewal), is weighed against that
  object before read-back (`Deduped` or `NoSmaller`). **Key named before encoding where possible**:
  a native 16/24-bit FLAC's STREAMINFO MD5 is the digest the encode would lay down
  (`bare::declared_digest`); with an object under it `foretold_flac` decodes once into the digest
  instead of encoding (`Foretold::Misdeclared` if samples hash otherwise: declaration is only a
  candidate; never for a renewal or cut row). Other sources: `Taking::foretold` carries a catalog
  key where `TRACKS_TO_VAULT` finds exactly one noted object with the row's sound (frames, rate,
  channels; whole files; `vault_objects_by_sound`); a sound two objects share foretells nothing. A
  second copy of a rip costs a decode, not an encode.
- **`Form::Wave`**: PCM FLAC cannot hold (`SampleFormat::F32`, > 24 bits, > 96 kHz). Canonical
  `fmt `+`data` WAVE (no `LIST`/`id3 `), zstd at `ARCHIVED_AT` (19). Over `LARGEST_PCM` (RIFF
  ceiling): refused before staging where the source declares its length (`wave::outgrows_a_wave`),
  then `kept_whole`, never unvaulted. `wave::compressed` gives up (`Packed::NoSmaller`) when output
  reaches the source's weight. **Passes are foretold**: past `FORETOLD_FROM_BYTES` (32 MiB)
  `wave::hopeless` compresses four `SLICE_BYTES` (2 MiB) slices at the same level and scales; over
  an eighth past the source's weight: `NoSmaller` before the pass. **The pass is a run of zstd
  frames**, one per `PACKED_FRAME_BYTES` (8 MiB) of WAVE, each pledged its length: any zstd reader
  sees one stream, `Unpacking` finds frame starts.
- **`Form::Kept`**: source bytes where re-encoding loses something or costs more: lossy codec, DSD,
  over 8 channels, re-encode no smaller. Tag containers *around* the audio go without touching a
  frame; `bare::bare` decides by the container symphonia opened:
  - FLAC, MPEG, ADTS, WavPack, Monkey's Audio, DSF shed ID3, APE, Lyrics3 (FLAC metadata rewritten
    to STREAMINFO alone; DSF ends where its metadata pointer pointed).
  - **Chunked containers shed describing chunks**: `chunks::shed` walks WAVE, AIFF/AIFC, CAF, DSDIFF
    chunk by chunk, omitting tag chunks, copying the rest with pad byte (not CAF), rewriting the
    header size (CAF has none). Unreadable walk: copied whole; a data chunk declaring more than the
    file holds reads to the end (streamed WAVE). RF64, BW64, Wave64 kept whole (`ds64` would need
    rewriting per omitted chunk).
  - **MP4, Matroska: tags blanked in place** (inside the structure indexing the audio: MP4 chunk
    offsets, Matroska SeekHead/Cues): `udta`, `meta` boxes become same-length `free` boxes
    (`blanks::blanked_movie`); `Tags`, `Attachments`, `Title` same-length EBML `Void`s
    (`blanks::blanked_segment`). Nothing moves.
  - Ogg Vorbis, Opus, Ogg FLAC: rewritten (below).
  - A tag claiming more than the file holds, or leaving no frames: file kept whole.

  Tests: `a_kept_wave_sheds_the_tags_its_chunks_carry_and_keeps_every_sample`,
  `a_kept_mp3_sheds_its_tags_and_keeps_every_frame_it_decodes_to`,
  `a_kept_mp4_blanks_the_tags_its_movie_holds_and_keeps_every_sample_it_decodes_to`,
  `a_kept_matroska_blanks_its_tags_and_keeps_every_sample_it_decodes_to`, and the
  `a_kept_ogg_vorbis_`, `a_kept_ogg_opus_`, `a_kept_ogg_flac_` siblings.

**Speaker positions are kept.** `Form::placing` weighs `MediaInfo::speakers` after `Form::of`: FLAC
fixes an assignment per channel count and STREAMINFO has no mask, so a source FLAC that would read
back differently (2.1 whose LFE FLAC calls a centre) goes to `Wave` (`WAVE_FORMAT_EXTENSIBLE` with
the source mask); a mask past the eighteen WAVE names is `Kept`. Validation compares positions with
digest and frame count (`Heard::held_by`).

**Three bounds are flacenc's, not FLAC's** (96 kHz, 24 bits, Rice parameter stopping at 14): 16-bit
rips re-encoded, 24-bit generally kept and stripped, no rule naming depths.

## Keys and layout

```
<root>/audio/ab/abcdef…7f.flac
<root>/covers/3c/3cd1…9a.jxl
<root>/staging/<pid>-<n>.<ext>
```

`VaultKey`: 16 bytes as 32 lowercase hex letters, fanned two deep. `Flac`/`Wave`: MD5 of decoded
interleaved PCM (FLAC's STREAMINFO digest, so `metaflac` agrees); `Kept`, covers: MD5 of stored
bytes. Writes go to `staging`, `sync_all`, rename; `landed` syncs the object's folder (and the one
above if the fanout folder was made). **Staging files carry the writer's pid, which is how they are
swept**: `Vault::open` removes those whose process `/proc` no longer holds
(`Sweeping::WhatCrashed`); `Vault::sweep_the_staging` (`--prune`) also takes files not named by a
pid, never a living process's. No `/proc`: every pid reads as living.

**A staging file is a `Staged`; drop removes it**: failure between `File::create` and rename leaves
nothing. `keep`'s `Kept` fallback follows a `NoSmaller` that landed nothing: no orphan in `audio/`.

**The catalog names an object by its place inside the vault, never the vault's place.**
`Vault::open` canonicalises an existing root and makes the three folders, never the root
(`Vault::make` does where asked: `binary.md`); `Vault::within` gives the relative path `vault_path`,
`vault_objects.path`, `cover_path` hold; `Vault::at` reverses it, refusing anything but plain names.

**Validation runs on the staging file before the rename** (a dedup hit is never at risk from a
failed import of the same audio) **and reads the form that lands**: a WAVE object is weighed from
its staged `.wav.zst` through `VaultFiles` (the playing path), not the WAVE it was packed from.
`Vault::verify` reads WAVE objects likewise, streaming.

## An object is weighed again when the encoder moves

`Encoding::OF_THIS_BUILD` names this build's encoders; each `vault_objects` row carries the one it
was weighed under. **Bump by hand whenever what an import writes changes** (new `flacenc`, other
zstd level, a `Form::of` bound, new kept-form stripping): nothing else says an object came from a
worse encoder.

`--import` re-walks a vaulted row whose stamp is behind, except a row whose source is gone (object
is the only copy) and a row `Form::of` keeps as is whatever the encoder, unless the bump changed how
that kind is kept (`sheds_tags_kept`: MP3, AAC, DSD, Vorbis, Opus). Preview marks it *weighed
again*. A renewal is a `Taking` with `renewing`: an object already under the key is a rival, not a
dedup hit; the new one replaces it (`Kept::replaced`, `Landing::Replaced`) only if smaller; a losing
renewal keeps the standing object, stamps it current. `keep` never discards a replaced object (rows
naming the same audio keep theirs); a renewal settling on another key leaves the old object to
`--prune`. A plain dedup hit is stamped `Encoding::UNRECORDED` where the catalog has no row for its
object: weighed again next import.

**A refusal is stamped against the encoder too.** Each `Keeping::Refused` writes
`vault_refused (track_id, under)` with `OF_THIS_BUILD`; `TRACKS_TO_VAULT` skips rows stamped at or
past this build's encoder (weighed once per encoder, not every `--import`). Trigger drops the stamp
when size, mtime or span moves; `note_vaulted` drops it when the row lands. Unreadable sources
(`Unreadable`, `SourceGone`) are stamped nothing
(`a_refused_row_is_not_weighed_again_until_its_file_or_the_encoder_moves`).

## Covers

`image` decode, lossless JXL by `zune-jpegxl` at highest effort, read back with `jxl-oxide`,
compared pixel for pixel before landing. Opaque: three channels; transparent: four.

**The vault decodes JXL back to PNG, so `resonate-ui` is unchanged.** `Vault::picture` answers
`CoverArt { format: Png, bytes }`, the type every caller takes (window hands gpui encoded bytes;
`mpris::art::Pictures` writes a file a notification daemon can draw). `dependencies.md` forbids an
image crate in `resonate-ui`; one decode in the format's owner serves both.

A cover already under its key is a dedup hit sized from the JXL head (`cover::size_of_jxl`). **Drawn
once a run**: `Drawings` holds the PNG under the cover's key, evicting past `DRAWN_BYTES_AT_MOST`
(48 MiB); `Vault::forget` takes a drawing with the file; a `Forgetting` count drops a drawing made
while its cover was being pruned.

## What the catalog holds

`tracks.vault_key`, `tracks.vault_path`, `albums.cover_key`, `albums.cover_path`, `cover_source`
gaining `Vault`, the `vault_objects` and `kept_tags` tables; changes are `MIGRATIONS` steps
(`library.md`).

- **A vaulted row is still named by its own file; the vault stands in only when bytes are wanted.**
  `tracks.path` + `span_start` are its identity everywhere (location, playlist `Cut`, plays, share,
  resumption). `Library::stand_in` is a `resonate_codec::StandIn` registered via
  `Sources::standing_in`; `Decoder::open`, `open_span`, `probe`, `probe_span` ask it first; where
  the catalog names a `vault_path` for that `(path, span_start, span_frames)` (matched whole) they
  decode the object *whole* (a cue row's object is that row alone) under a catalog-filled `TagSet`,
  including `rg_*` (gain heard, `TagSet::heard_gain`, not only declared) and `tracks.lyrics`. An
  object that will not open falls back to the row's file. `info`, scan, import use sources with no
  stand-in: they read the file.
- **What the catalog cannot fill, the import keeps.** `vaulted::FILLED_FROM_THE_CATALOG` names the
  nineteen `TagField`s the stand-in reads off `tracks`, `albums` and `artists`; every other field
  `Kept::declared` holds (credits, comment, totals, label, catalogue number, barcode, grouping,
  copyright, BPM, compilation, the four sort names) is written to `kept_tags (track_id, field,
  value)` by `note_vaulted` and by a delivery's row, spelled by `TagField::read` and set back by
  `TagField::set` (`every_field_set_into_a_tag_set_reads_back_as_it_was_spelled`), so the bus and
  the inspector see what the file said
  (`a_vaulted_row_stands_in_under_the_credits_totals_and_notes_its_file_declared`). A release
  drops them with the vault columns. A row vaulted before the table keeps nothing until weighed
  again.
- **An object stands in at its source's depth, not its container's.** A 24-bit master past 96 kHz
  lands as a 32-bit WAVE, a 20-bit one as a 24-bit FLAC; `Kept::bits` is the depth the source
  declared (`bits_per_coded_sample`, else its format's), written to `tracks.vault_bits` (cleared
  with the other vault columns), carried on `StoodIn::bits` and laid over the object's own by
  `StoodIn::told_over` (decoder and probe alike), so the inspector says what the master was and a
  study through the object does not read the container's padding as a fake
  (`a_twenty_four_bit_master_kept_in_a_wider_wave_stands_in_as_twenty_four_bits`).
- **A vault inside a scanned root is not part of the library it holds.** The walk steps past the
  open vault's root (links into it included); `store::apply` leaves a rootless row alone wherever a
  scan reaches one (a delivery keeps name, pairing, place).
- **`vault_path` is denormalised beside the key on purpose**: stand-in reads it by the row's unique
  key, one indexed read.
- **A vaulted row outlives its file; a changed file forgets its object.** Prune and root tidy keep a
  vaulted row whose file is gone (only copy); one the walk missed while its file is there was
  superseded and goes. The upsert clears `vault_key`/`vault_path` where size, mtime or span moved
  (re-ripped file weighed again).
- **`Library::prune_the_vault` (`--prune`) weighs a cover by its key.** Under the `Walk` guard it
  removes audio objects no row names plus their `vault_objects` rows, every JXL under `covers/` no
  album's `cover_key` holds, then staging. **An object's row goes only with its file**: one
  `Vault::forget` refuses keeps its row, counted in `Pruned::left`. **It walks `audio/` too**
  (`Vault::objects`), taking any object neither `vault_objects` nor a `tracks.vault_key` names, only
  once its mtime is `LANDING_GRACE` (10 minutes) old (a poll lands deliveries outside the `Walk`
  guard).
- **`Library::import` takes the `Walk` guard** (second caller: `Error::AlreadyWalking`), runs on
  `ImportOptions::workers` threads drawing rows over `costliest_first` (slow encode starts at once);
  workers write their own catalog rows; the plan is re-sorted to request order (preview and apply
  list alike). `landed_or_standing` decides and renames under one lock. First error stops every
  worker; a panicking one is `Error::Stopped`.
- **A row leaves the vault only where its own file is still there.** `Library::release_from_vault`
  (`vault --release`) clears `vault_key`/`vault_path` on every vaulted row whose `tracks.path`
  exists, counting the rest in `Released::stranded`. Objects wait for `--prune`, the one deleting
  gesture. Both are previews until `--apply`: `vault_release_foretold` and `vault_prune_foretold`
  count through the same `Applying`; `Vault::staging_a_sweep_would_take` counts staging unswept.
  Covers stay (a moved picture had `cover_art` cleared).
- **`retag` passes a vaulted row over** (`Unwritten::Vaulted`); **`organise` does not** (vault keyed
  by content).
- **An album holds a cover where it holds either column**: `ALBUM_COLUMNS` and `asking_albums` read
  `cover_art IS NOT NULL OR cover_path IS NOT NULL`. `gather`'s fill takes the loser's whole cover
  only where the survivor holds neither: never both, never a source without the picture.
- **A cover moves into the vault once per album**: `enriched::vault_the_cover` clears `cover_art` in
  the statement writing `cover_path`, only where `cover_art` is still the bytes the import encoded.
  `land_archive_cover` and the scan's `store::land_covers` carry `AND cover_path IS NULL`. Import asks
  `cover_the_vault_lacks`, not `cover_art`; a cover `image` cannot read, or whose format code names
  nothing (`UntypedCoverArt`, `UnknownImageFormat`), is a warning and a `covers_passed` count, not
  the end of the import.
- **A failing vault ends the import; a failing source is passed over.** `Vault::failed_itself`
  separates them: `Error::Io` on a path under the root, a full, over-quota or read-only disc
  anywhere, and `OutsideTheVault` are the vault's (`keep_one` answers `Error::Vault`, stopping every
  worker); anything else is `Passing::Unreadable`, import continues. A read of the source names
  the source, never the staging file: `Error::Source { location, .. }` (the copy of a source kept
  whole, `bare::bare`'s rewind), never the vault's. A source kept whole whose decode fails out of
  reach (`codec::Error::is_out_of_reach`: a dropped share, a timed-out read) answers the
  `Error::Codec`, unstamped, as the FLAC and WAVE paths do; only a decode that fails on the bytes
  themselves is `Refusal::NotValidated`
  (`a_source_that_drops_partway_is_neither_refused_nor_blamed_on_the_vault`).

## Reading an object back

`VaultFiles` is a `MediaProvider` under `SourceId::local()`, *replacing* `LocalFiles`: a path under
the vault root ending `.zst` is an `Unpacking` (inner extension as `FormatHint`), decoded as read,
not decompressed into memory; anything else is a plain open. **A seek backward, or more than
`FAR_AHEAD` (4 MiB) forward, restarts at the frame holding the target** (`unpacking::frames_of`
indexes frame headers once); no content size: restart from the start. The binary registers it
wherever it opens a track to play or read (`held_over`). Bus, playlists, resumption, queue never see
an object's path.

## What a provider delivers

`Library::poll` hands `Delivery::File` to `Vault::keep` as a local location and `Delivery::Stream`
to `Vault::keep_delivered`: copies the opened reader into `staging/` through a `take` one byte past
`LARGEST_DELIVERY` (past: `Refusal::TooLarge`, never decoded), keeps it like any local file,
discards the staging file whatever happened; extension filtered to ASCII letters and digits (a
delivered `../flac` stays under `staging/`). The poll writes the `vault_objects` row, a rootless
`tracks` row named by the object's path and paired with the wanted release track (carrying what
`Kept::declared` says the source declared), and records `wants.offered` as the *vault* object's URI
(`providers.md`: why the row belongs to no root). A poll cancelled mid-keep still notes a landed
object. Two landed objects are deliberately not noted and wait for `--prune`: one whose
`Kept::frames` disagree with the release row's length (`LENGTHS_AGREE_WITHIN`), one landing after
that row was paired with a listener's file. **`Kept::frames` is what the source decoded to in every
form**: for `Form::Kept`, the frames `kept_whole` weighed, not the container's declared length.

## An Ogg stream's comments are rewritten, and every page after renumbered

Vorbis/Opus tags sit in a header packet of their own (comment packet after identification; Vorbis
also the setup packet after it): stripping rewrites structure. `ogg::bare` names the codec by the
first page's magic, walks that serial's pages in sequence until the header packets are whole, and
refuses (file copied as is) header pages of another serial, a skipped sequence number, headers past
`HEADER_BYTES_AT_MOST` (64 MiB), or a last header packet ending anywhere but at its page's end. The
comment packet is rewritten to its vendor string alone (count zero, plus Vorbis's framing bit).

**Ogg FLAC is the same walk over metadata blocks.** First packet: `\x7fFLAC`, mapping version, count
of following header packets (zero = *unknown*: headers whole at the block marked last), STREAMINFO.
Kept: one VORBIS_COMMENT, vendor alone, marked last; a stream whose first header is not its comment
is copied as is. The first page then says one header follows and is restamped, unless it counted
none.

New header packets go on fresh pages under the next sequence numbers (granule zero on a page a
packet ends on, `NO_PACKET_ENDS` on one none does), each stamped with the Ogg CRC-32
(`a_page_is_stamped_with_the_checksum_libogg_gives_it`). Headers now take fewer pages, so
`Renumbering` is the difference and `Renumbered` rewrites sequence and checksum of every later page
of that serial, passing other serials' pages and anything unparseable verbatim. `kept_whole` makes
this safe: a copy not holding the same audio is recopied whole.