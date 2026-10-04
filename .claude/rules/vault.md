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

`resonate-vault` is the managed archive this build writes itself, beside the catalog that
describes it. An import decodes a track, throws every tag and picture away, keeps the smallest
bit-exact copy it can make, validates it by reading it back, and points the catalog row at it.
**The library it came from is read and never written to** (`the_file_the_vault_read_is_left_exactly_as_it_was`).
`tracks.path` keeps naming the original, so a rescan reads an untouched source as unchanged and
`organise` goes on filing it.

A leaf beside `resonate-codec`, with no SQL: every row belongs to `resonate-library`.
`cargo tree -p resonate-vault` stays free of gpui, the engine, the library and `ureq`.

## What it promises

- **Bit-exact or nothing.** Every object is read back through the player's decoder and its PCM
  weighed against what went in, by MD5. A mismatch removes the file and answers
  `Refusal::NotValidated`; an encode of no frames is `Refusal::Empty`.
- **Never larger than what it came from.** A re-encode that does not beat the source is discarded
  and the source kept; unconditional, not a setting. A row a sheet cuts out of a file cannot keep
  the file (`Refusal::CutFromAnother`), so it is weighed against its *share* (file size x its
  frames / the file's frames) and a re-encode no smaller is `Refusal::NoSmaller`. `Kept::was` is
  that weight and is what the library's `was_bytes` counts, so a single-file rip is not counted
  once per row. **The weighing happens before anything lands**: `landed_or_standing` weighs the
  staged object or the one already under its key and answers `Landing::NoSmaller` without renaming,
  so two imports can land one key at once without one deleting what the other's row names.
- **A kept object is weighed like an encoded one.** `kept_whole` decodes source and staged copy
  through the same `pcm_of` and refuses differing digests; a stripped copy that fails is copied
  again whole, so a source whose decoder read a tag as a frame is still kept. `bare` rewrites a
  FLAC's metadata only where its walk reached the last block, else hands the file back to be copied
  as it stands.
- **One copy of one thing.** The key is the MD5 of the decoded PCM, so one audio in two containers
  is one object; a cover is keyed by its bytes.
- **Nothing outside the root.** `Vault::inside` guards every read, write and delete
  (`Error::OutsideTheVault`); `within` requires plain names after the root, so `<root>/../x` is
  outside though a lexical `starts_with` passes it.

## The three forms

`Form::of` reads codec, spec and declared depth and answers before anything is decoded; the size
comparison may overrule it.

- **`Form::Flac`**: integer PCM, 8-24 bits, <= 8 channels, <= 96 kHz. Streamed through
  `flacenc` over `resonate_codec::Decoder`, so a track is never held whole. It carries STREAMINFO
  and **nothing else**: stripping is construction. **It stops paying once it has lost**:
  `flac::encode` stops encoding and writing the moment the output reaches the source's weight but
  keeps hashing the samples into the key (`Encoded::outgrew`). An encode that outgrew, or whose key
  already has an object where the import is not a renewal, is weighed against that object before
  any read-back (`Deduped` or `NoSmaller`). **A source names its key before the encode does where
  it can**: a native 16- or 24-bit FLAC's STREAMINFO MD5 is the digest the encode would lay down
  (`bare::declared_digest`), and where an object stands under it `foretold_flac` decodes the source
  once into the digest instead of encoding (`Foretold::Misdeclared` when the samples hash to
  something else, so the declaration is only a candidate). For other sources `Taking::foretold`
  carries a key from the catalog, where `TRACKS_TO_VAULT` finds exactly one noted object with the
  row's sound (frame count, rate, channels, from `vault_objects_by_sound`); a sound two objects share
  foretells nothing. Either way a second copy of a rip costs a decode, not an encode.
- **`Form::Wave`**: PCM FLAC cannot hold (`SampleFormat::F32`, > 24 bits, > 96 kHz). A canonical
  `fmt `+`data` WAVE with no `LIST` or `id3 ` chunk, zstd'd at `ARCHIVED_AT`; refused over
  `LARGEST_PCM`, the RIFF ceiling, before anything is staged where the source declares its length
  (`wave::outgrows_a_wave`), and such a file goes to `kept_whole` rather than never being vaulted.
  `wave::compressed` gives up (`Packed::NoSmaller`) the moment its output reaches the source's
  weight. **A pass is foretold before it is paid for**: past `FORETOLD_FROM_BYTES`,
  `wave::hopeless` compresses four `SLICE_BYTES` slices at the same level and scales them; landing
  more than an eighth past the source's weight is `NoSmaller` before the pass starts. **The pass is
  a run of zstd frames**, one per `PACKED_FRAME_BYTES` of WAVE, each pledged its length so any zstd
  reader reads the run as one stream and `Unpacking` can find where each frame begins.
- **`Form::Kept`**: the source's own bytes, where re-encoding would lose something or cost more:
  a lossy codec, DSD, > 8 channels, or a re-encode no smaller. What a container keeps its tags in
  *around* the audio is left behind without touching an audio frame. `bare::bare` decides by the
  container symphonia opened:
  - FLAC, MPEG, ADTS, WavPack, Monkey's Audio and DSF shed their ID3, APE and Lyrics3 tags (a FLAC's metadata blocks are rewritten to STREAMINFO alone; a DSF ends where its metadata pointer pointed).
  - **Chunked containers shed their describing chunks.** `chunks::shed` walks a WAVE, AIFF/AIFC, CAF
    and DSDIFF chunk by chunk, leaving out the tag-bearing ones, copying every other with its pad
    byte and rewriting the header size. An unreadable walk copies the file whole; a data chunk
    declaring more than the file holds is read to the end (a streamed WAVE). RF64, BW64 and Wave64
    are kept whole, since their `ds64` table would need rewriting for every chunk left out.
  - **An MP4 or Matroska has its tags blanked where they stand**, since they sit inside the
    structure indexing the audio (MP4 chunk offsets, Matroska SeekHead and Cues): `udta` and `meta`
    boxes become same-length `free` boxes (`blanks::blanked_movie`), `Tags`, `Attachments` and the
    `Title` become same-length EBML `Void`s (`blanks::blanked_segment`). Nothing moves.
  - Ogg Vorbis, Opus and Ogg FLAC are rewritten (below).
  - A tag claiming more than the file holds, or tags leaving no frames, leave the file whole.
  `a_kept_wave_sheds_the_tags_its_chunks_carry_and_keeps_every_sample` and its MP4, Matroska and Ogg
  siblings are the claims.

**Speaker positions are part of what is kept.** `Form::placing` weighs `MediaInfo::speakers` after
`Form::of`: FLAC fixes an assignment per channel count and STREAMINFO has no room for a mask, so a
source FLAC would read back differently (a 2.1 whose LFE FLAC calls a centre) goes to `Wave`, which
writes `WAVE_FORMAT_EXTENSIBLE` with the source's mask; a mask past the eighteen WAVE names is
`Kept`. Validation weighs positions read back beside digest and frame count (`Heard::held_by`).

**Three bounds are flacenc's, not FLAC's** (96 kHz, 24 bits, a Rice parameter stopping at 14): 16-bit rips are re-encoded and 24-bit ones generally kept and stripped, with no rule naming depths.

## Keys and layout

```
<root>/audio/ab/abcdef…7f.flac
<root>/covers/3c/3cd1…9a.jxl
<root>/staging/<pid>-<n>.<ext>
```

A `VaultKey` is sixteen bytes as thirty-two lowercase hex letters, fanned two deep. For `Flac` and
`Wave` it is the MD5 of the decoded interleaved PCM (FLAC's STREAMINFO digest, so `metaflac` can be
asked the same); for `Kept` and covers the MD5 of the stored bytes. Writes go to `staging`, are
`sync_all`'d and renamed into place, and `landed` syncs the folder the object now sits in (and the
one above where the fanout folder was made). **Every staging file carries the pid that wrote it, and
that is how it is swept**: `Vault::open` removes each whose process `/proc` no longer holds
(`Sweeping::WhatCrashed`); `Vault::sweep_the_staging` (what `--prune` uses) also takes any file not
named by a pid, never a living process's. With no `/proc` every pid reads as living.

**A staging file is a `Staged`, and dropping one removes the file**, so every failure between
`File::create` and the rename leaves nothing. `keep`'s fallback to `Kept` follows a `NoSmaller`
that landed nothing, so no object is orphaned in `audio/`.

**The catalog names an object by where it sits inside the vault, never where the vault sits.**
`Vault::open` canonicalises a root that is there and makes the three folders inside it, never the
root itself (`Vault::make` does, for the callers asked to: `binary.md`); `Vault::within` turns an
answered path into the relative one `vault_path`, `vault_objects.path` and `cover_path` hold;
`Vault::at` turns it back, refusing anything not a run of plain names.

**Validation happens on the staging file, before the rename**, so a dedup hit is never at risk from
a failed import of the same audio. **It reads the form that lands**: a WAVE object is weighed from
its staged `.wav.zst` through `VaultFiles`, the path that plays it, not from the WAVE it was packed
from. `Vault::verify` reads a WAVE object the same way, streaming.

## An object is weighed again when the encoder moves

`Encoding::OF_THIS_BUILD` names this build's encoders and every `vault_objects` row carries the one
it was weighed under. **Bump it by hand whenever what an import writes changes** (a new `flacenc`,
another zstd level, a bound `Form::of` draws differently, a new kept-form stripping), since nothing
else can tell an object was made by a worse encoder.

`--import` walks a vaulted row again wherever its stamp is behind, except a row whose source has
gone (the object is the only copy) and a row `Form::of` keeps as it stands whatever the encoder,
unless the bump changed how that kind is kept. The preview marks such a row *weighed again*. A
renewal is a `Taking` with `renewing` set: an object already under the same key is a rival, not a
dedup hit, and the new one replaces it (`Kept::replaced`, `Landing::Replaced`) only where smaller.
A losing renewal keeps the standing object and stamps it current. `keep` never discards an object it
replaced, so rows naming the same audio keep theirs; a renewal settling on another key leaves the
old object to `--prune`. A plain dedup hit is stamped `Encoding::UNRECORDED` where the catalog has
no row for its object, so it is weighed again next import rather than trusted.

**A refusal is stamped against the encoder too.** Every `Keeping::Refused` writes
`vault_refused (track_id, under)` with `OF_THIS_BUILD`, and `TRACKS_TO_VAULT` passes over a row
stamped at or past this build's encoder, so a refused row is weighed once per encoder, not by every
`--import`. The trigger drops the stamp where size, mtime or span moves, and `note_vaulted` drops
it once the row lands. A source that failed to read (`Unreadable`, `SourceGone`) is stamped nothing
(`a_refused_row_is_not_weighed_again_until_its_file_or_the_encoder_moves`).

## Covers

Decoded with `image`, encoded as lossless JXL by `zune-jpegxl` at its highest effort, read back with
`jxl-oxide` and compared pixel for pixel before landing. Opaque pictures are three channels,
transparent four.

**The vault decodes JXL back to PNG, so `resonate-ui` does not change.** `Vault::picture` answers
`CoverArt { format: Png, bytes }`, the type every caller already takes (the window hands gpui
encoded bytes; `mpris::art::Pictures` writes a file a notification daemon can draw).
`dependencies.md` forbids an image crate in `resonate-ui`, and one decode in the crate owning the
format answers both.

A cover already under its key is a dedup hit sized off the JXL's head alone (`cover::size_of_jxl`). **A cover is drawn once a run**: `Drawings` holds the PNG under the cover's key, evicting past `DRAWN_BYTES_AT_MOST`; `Vault::forget` takes a drawing with the file, and a `Forgetting` count drops a drawing made while its cover was being pruned.

## What the catalog holds

`tracks.vault_key`, `tracks.vault_path`, `albums.cover_key`, `albums.cover_path`, `cover_source`
gaining `Vault`, and a `vault_objects` table; a change now is a `MIGRATIONS` step (`library.md`).

- **A vaulted row is still named by its own file; the vault stands in only when bytes are wanted.**
  `tracks.path` and `span_start` are its identity everywhere (location, playlist `Cut`, plays,
  share, resumption). What the player opens is decided at its `Sources`: `Library::stand_in` is a
  `resonate_codec::StandIn`, registered with `Sources::standing_in`; `Decoder::open`,
  `open_span`, `probe` and `probe_span` ask it first, and where the catalog names a `vault_path`
  for that `(path, span_start, span_frames)` (matched whole) they decode the object *whole* (a cue
  row's object is that row alone) under a `TagSet` the catalog fills, including `rg_*` and
  `tracks.lyrics`. An object that will not open falls back to the row's file. `info`, the scan and
  the import open through sources with no stand-in, so they read the file.
- **A vault inside a scanned root is not part of the library it holds.** The walk steps past the
  open vault's root, links into it included; `store::apply` leaves a rootless row alone wherever a
  scan reaches one anyway, so a delivery keeps its name, pairing and place.
- **`vault_path` is denormalised beside the key on purpose**: the stand-in reads it by the row's
  unique key, one indexed read.
- **A vaulted row outlives its file, and a changed file forgets its object.** Prune and root tidy
  leave a vaulted row whose file has gone (the vault holds the only copy); one the walk did not see
  while its file is there was superseded and goes. The upsert clears `vault_key` and `vault_path`
  where size, mtime or span moved, so a re-ripped file is weighed again.
- **`Library::prune_the_vault` (`--prune`) weighs a cover by its key.** Under the `Walk` guard it
  removes audio objects no row names and their `vault_objects` rows, then every JXL under
  `covers/` no album's `cover_key` holds, then the staging folder. **An object's row goes only with
  its file**: one `Vault::forget` refuses keeps its row and is counted in `Pruned::left`. **It walks
  `audio/` as well** (`Vault::objects`), taking any object neither `vault_objects` nor a
  `tracks.vault_key` names, but only once its mtime is `LANDING_GRACE` old, since a poll lands
  deliveries outside the `Walk` guard.
- **`Library::import` takes the `Walk` guard** (a second caller gets `Error::AlreadyWalking`) and runs on `ImportOptions::workers` threads drawing rows over `costliest_first` so the slow encode starts at once; workers write their own catalog rows, and the plan is put back into request order so preview and apply list alike. `landed_or_standing` decides and renames under one lock. The first error stops every worker; a panicking one is `Error::Stopped`.
- **A row leaves the vault only where its own file is still there.** `Library::release_from_vault`
  (`vault --release`) clears `vault_key` and `vault_path` on every vaulted row whose `tracks.path`
  exists and counts the rest as `Released::stranded`. Objects wait for `--prune`, the one deleting
  gesture. Both are previews until `--apply`: `vault_release_foretold` and `vault_prune_foretold`
  count what the real pass would do through the same `Applying`, `Vault::staging_a_sweep_would_take`
  counting the staging folder without sweeping it. Covers stay (a moved picture had `cover_art` cleared).
- **`retag` passes a vaulted row over** (`Unwritten::Vaulted`); **`organise` does not**, the vault being keyed by content.
- **An album holds a cover where it holds either column.** `ALBUM_COLUMNS` and `asking_albums` read
  `cover_art IS NOT NULL OR cover_path IS NOT NULL`. `gather`'s fill takes the loser's whole cover
  only where the survivor holds neither, so an album never holds both nor names a source without
  the picture.
- **A cover moves into the vault once per album**: `enriched::vault_the_cover` clears `cover_art` in
  the statement that writes `cover_path`, and only where `cover_art` is still the bytes the import
  encoded. `land_archive_cover` and the scan's `store::cover` carry `AND cover_path IS NULL`. The
  import asks `cover_the_vault_lacks` rather than `cover_art`, and a cover `image` cannot read, or
  whose format code names nothing (`UntypedCoverArt`, `UnknownImageFormat`), is a warning and a
  `covers_passed` count, not the end of the import.
- **A vault that fails ends the import; a source that fails is passed over.** `Vault::failed_itself`
  tells them apart: an `Error::Io` on a path under the root, a full, over-quota or read-only disc
  wherever it was, and `OutsideTheVault` are the vault's, and `keep_one` answers them as
  `Error::Vault`, stopping every worker; anything else is `Passing::Unreadable` and the import goes
  on.

## Reading an object back

`VaultFiles` is a `MediaProvider` registered under `SourceId::local()`, *replacing* `LocalFiles`: a path under the vault root ending in `.zst` is an `Unpacking` with the inner extension as its `FormatHint`, decoded as read rather than decompressed into memory; everything else is the plain file open. **A seek backward, or more than `FAR_AHEAD` forward, starts again at the frame holding the target** (`unpacking::frames_of` indexes the frame headers once); an object with no content size restarts from the start. The binary registers it wherever it opens a track to play or read (`held_over`). The bus, playlists, resumption and queue never see an object's path.

## What a provider delivers

`Library::poll` hands a `Delivery::File` to `Vault::keep` as a local location and a `Delivery::Stream` to `Vault::keep_delivered`, which copies the reader into `staging/` through a `take` one byte past `LARGEST_DELIVERY` (past it is `Refusal::TooLarge`, never decoded), keeps it like any local file and discards the staging file whatever came of it; the extension is filtered to ASCII letters and digits so a delivered `../flac` stays under `staging/`. The poll writes the `vault_objects` row, a rootless `tracks` row named by the object's path and paired with the wanted release track (carrying what `Kept::declared` says the source declared), and records `wants.offered` as the *vault* object's URI. `providers.md` has why the row belongs to no root. A
poll cancelled mid-keep still notes a landed object before it ends. Two landed objects are
deliberately not noted and wait for `--prune`: one whose `Kept::frames` disagree with the length of
the release row it was wanted for, and one landing after that row was paired with a file of the
listener's. **`Kept::frames` is what the source decoded to in every form**, for `Form::Kept` the
frames `kept_whole` weighed rather than the length the container declared.

## An Ogg stream's comments are rewritten, and every page after renumbered

A Vorbis or Opus stream keeps its tags in a header packet of its own (the comment packet after the
identification header, and for Vorbis the setup packet after it), so stripping is a rewrite of
structure. `ogg::bare` names the codec by the first page's magic, walks the pages of that serial in
sequence until the header packets are whole, and refuses (copying the file as it stands) header
pages of another serial, a skipped sequence number, headers past `HEADER_BYTES_AT_MOST`, or a last
header packet ending anywhere but at its page's end. The comment packet is rewritten with its
vendor string alone (count zero, plus Vorbis's framing bit).

**Ogg FLAC is the same walk over metadata blocks.** Its first packet is `\x7fFLAC`, the mapping
version, a count of following header packets (zero meaning *unknown*, so headers are whole at the
block marked last) and STREAMINFO. Kept: one VORBIS_COMMENT with its vendor alone, marked last; a
stream whose first header is not its comment is copied as it stands. The first page then says one
header follows and is restamped, unless it counted none.

New header packets go on fresh pages under the next sequence numbers (granule zero on a page a
packet ends on, `NO_PACKET_ENDS` on one none does), each stamped with the Ogg CRC-32
(`a_page_is_stamped_with_the_checksum_libogg_gives_it`). The headers now take fewer pages, so
`Renumbering` is the difference and `Renumbered` rewrites sequence and checksum of every later page
of that serial, passing other serials' pages and anything unparseable verbatim. `kept_whole`
makes this safe: a copy not holding the same audio is copied again whole.
