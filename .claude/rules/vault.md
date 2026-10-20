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
`tracks.path` keeps naming the original (rescan sees an untouched source, `organise` keeps filing
it). Leaf beside `resonate-codec`, no SQL; `cargo tree -p resonate-vault` stays free of gpui,
engine, library, `ureq` (CI `refuse`).

## What it promises

- **Bit-exact or nothing.** Every object is read back through the player's decoder, PCM compared by
  MD5. Mismatch: file removed, `Refusal::NotValidated`; no frames: `Refusal::Empty`.
- **Never larger than its source.** A re-encode not beating the source is discarded. A sheet-cut row
  cannot keep the file (`Refusal::CutFromAnother`), so it is weighed against its *share* (file size
  x row frames / file frames); no smaller: `Refusal::NoSmaller`. `Kept::was` is that weight and what
  `was_bytes` counts. **Weighing precedes landing**: `landed_or_standing` weighs the staged object or
  the one under its key and answers `Landing::NoSmaller` without renaming, so concurrent imports of
  one key never delete what the other's row names
  (`imports_of_one_flac_key_racing_land_one_object_and_leave_nothing_staged`, its `_wave_` sibling and
  `dedup_hits_and_renewals_racing_a_sweep_never_lose_the_object_or_trip_the_sweep`: eight threads over
  a barrier, forty rounds, one landing, the rest deduped, nothing left staged). **A standing object is taken only where its head
  declares what went in, never on its size alone** (`standing_holding`, `holds_the_sound`): a FLAC's
  STREAMINFO (`standing::of_a_flac`: rate, channels, frames and the samples' MD5), a packed WAVE's
  RIFF header (`standing::of_a_packed_wave`: rate, channels, frames), a kept object's size against
  the staged copy of the same bytes. One that does not hold it is no dedup hit: the import encodes,
  validates and lands over it (warned)
  (`an_object_under_the_key_that_holds_something_else_is_not_taken_for_it`).
- **A kept object is weighed like an encoded one**: `kept_whole` decodes source and staged copy
  through the same `pcm_of`, refuses differing digests; a failing stripped copy is recopied whole.
  `bare` rewrites a FLAC's metadata only if its walk reached the last block, else copies as is.
- **One copy of one thing, and only of one thing.** Key = MD5 over the decoded PCM's MD5 and the
  sound it is heard as (`key::Sound`: rate, channel count, sample word, speaker mask), so one audio
  in two containers is one object while the same samples at another rate, count, word or placing
  (digital silence above all) are objects apart
  (`the_same_samples_at_another_rate_or_count_land_as_objects_of_their_own`); covers keyed by their
  bytes.
- **Nothing outside the root.** `Vault::inside` guards every read, write, delete
  (`Error::OutsideTheVault`); `within` requires plain names after the root (`<root>/../x` passes
  lexical `starts_with`).

## The three forms

`Form::of` reads codec, spec, declared depth before decoding; size comparison may overrule.

- **`Form::Flac`**: integer PCM, <= 24 bits, <= 8 channels, <= 96 kHz. Streamed via `flacenc` over
  `resonate_codec::Decoder`, STREAMINFO **and nothing else**. **Stops paying once it has lost**:
  `flac::encode` stops encoding and writing when output reaches the source's weight, still hashing
  into the key (`Encoded::outgrew`); an encode that outgrew, or whose key already has an object (not
  a renewal), is weighed against that object before read-back (`Deduped` or `NoSmaller`).
  **Key named before encoding where possible**: a native 16/24-bit FLAC's STREAMINFO MD5 is the
  PCM digest the encode would lay down (`bare::declared_digest`), keyed with the source's sound;
  with an object under it holding that sound `foretold_flac` decodes once instead of encoding
  (`Foretold::Misdeclared` if samples hash otherwise or the object holds another sound; never for a
  renewal or cut row). Other sources: `Taking::foretold` carries a catalog key where
  `TRACKS_TO_VAULT` finds exactly one noted object keyed by the sound (`Encoding::KEYED_BY_THE_SOUND`)
  with the row's sound (frames, rate, channels; `vault_objects_by_sound`); a sound two objects share
  foretells nothing.
- **`Form::Wave`**: PCM FLAC cannot hold (`SampleFormat::F32`, > 24 bits, > 96 kHz). Canonical
  `fmt `+`data` WAVE, zstd at `ARCHIVED_AT` (19). Over `LARGEST_PCM` (RIFF ceiling): refused before
  decoding where the source declares its length (`wave::outgrows_a_wave`), else `wave::tally` stops
  at the ceiling, then `kept_whole`. **Two passes, no uncompressed copy ever written.** `wave::tally`
  decodes once, hashing the PCM into the key and counting frames and bytes (and holding evenly spread
  `SLICE_BYTES` samples, `Samples`), so a dedup hit is decided before anything is staged
  (`a_wave_already_in_the_vault_is_deduped_before_anything_is_staged`). A miss reopens the source
  (`reopened`, span included) and `wave::pack` streams the decoder through `Pcm`, a `Read` yielding
  the header (sizes exact from the tally) and the PCM, straight into `packed_in_frames`. A source
  that decodes to other bytes the second time is `Packed::Differed`, `Refusal::NotValidated`, nothing
  left behind (`a_wave_whose_source_changes_between_being_weighed_and_being_packed_is_refused_and_leaves_nothing`).
  `packed_in_frames` gives up (`Packed::NoSmaller`) when output reaches the source's weight. **Passes
  are foretold**: past `FORETOLD_FROM_BYTES` (32 MiB) `wave::hopeless` compresses the whole
  `SLICE_BYTES` (2 MiB) samples the tally kept (4 to 8, their windows' stride doubling and odd ones
  dropped as the stream grows, so they stay spread over all of it) and scales; over an eighth past
  the source's weight: `NoSmaller` before the pass. **The pass is a run of zstd frames**, one per
  `PACKED_FRAME_BYTES` (8 MiB) of WAVE, each pledged its length: any zstd reader sees one stream,
  `Unpacking` finds frame starts.
- **`Form::Kept`**: source bytes where re-encoding loses something or costs more: lossy codec, DSD,
  over 8 channels, re-encode no smaller. Tag containers *around* the audio go without touching a
  frame; `bare::bare` decides by the container symphonia opened:
  - FLAC, MPEG, ADTS, WavPack, Monkey's Audio, DSF shed ID3, APE, Lyrics3 (FLAC rewritten to
    STREAMINFO alone; DSF ends where its metadata pointer pointed).
  - **Chunked containers** (WAVE, AIFF/AIFC, CAF, DSDIFF): `chunks::shed` omits tag chunks, copies
    the rest with pad byte (not CAF), rewrites the header size. Unreadable walk: copied whole; a data
    chunk declaring more than the file holds reads to the end. RF64, BW64, Wave64 kept whole (`ds64`).
  - **MP4, Matroska: tags blanked in place** so nothing indexing the audio moves: `udta`, `meta`
    (file, movie and track level) become same-length `free` boxes (`blanks::blanked_movie`);
    `Tags`, `Attachments`, `Chapters`, the `Info` `Title` and each `TrackEntry` `Name` same-length
    EBML `Void`s (`blanks::blanked_segment`). An element of unknown size (a live `Cluster`) is
    walked through its children to the next Segment-level ID. **A walk that cannot reach the end
    blanks nothing**: an `Unwalked` reason (a box or tag-bearing element running past its parent,
    a misformed header, an unknown size inside an unknown size, past `BOXES_WALKED_AT_MOST` (2^20)
    or `ELEMENTS_WALKED_AT_MOST` (2^24)) keeps the file whole rather than landing part of its tags as
    stripped; a last element cut short with nothing to blank in it ends the walk as complete.
  - Ogg Vorbis, Opus, Ogg FLAC: rewritten (below).
  - A tag claiming more than the file holds, or leaving no frames: kept whole.

  Tests: `a_kept_wave_sheds_the_tags_its_chunks_carry_and_keeps_every_sample`,
  `a_kept_mp3_sheds_its_tags_and_keeps_every_frame_it_decodes_to`,
  `a_kept_mp4_blanks_the_tags_its_movie_holds_and_keeps_every_sample_it_decodes_to`,
  `a_kept_matroska_blanks_its_tags_and_keeps_every_sample_it_decodes_to`, and the
  `a_kept_ogg_vorbis_`, `a_kept_ogg_opus_`, `a_kept_ogg_flac_` siblings.

**Speaker positions are kept.** `Form::placing` weighs `MediaInfo::speakers` after `Form::of`: FLAC
fixes an assignment per channel count and STREAMINFO has no mask, so a source that would read back
differently (2.1 whose LFE FLAC calls a centre) goes to `Wave` (`WAVE_FORMAT_EXTENSIBLE` with the
source mask); a mask past the eighteen WAVE names is `Kept`. Validation compares positions with
digest and frame count (`Heard::held_by`).

**Three bounds are flacenc's, not FLAC's** (96 kHz, 24 bits, Rice parameter stopping at 14): 16-bit
rips re-encoded, 24-bit generally kept and stripped, no rule naming depths.

## Keys and layout

```
<root>/audio/ab/abcdef…7f.flac
<root>/covers/3c/3cd1…9a.jxl    (or .jpg, .png, … where kept as it came)
<root>/staging/<pid>-<n>.<ext>
```

`VaultKey`: 16 bytes as 32 lowercase hex letters, fanned two deep. `Flac`/`Wave`:
`Sound::keyed`, MD5 of a tag, the decoded interleaved PCM's MD5 (FLAC's STREAMINFO digest, which
`metaflac` still agrees with), rate, channel count, sample word and WAVE mask. The mask is placed as
the form will read back (`Sound::placed`): named speakers as named, unnamed ones in the order the form
assigns (FLAC's for `Flac`, WAVE's for `Wave`), so a key is the same from the source and from the
object; `Kept`, covers: MD5 of stored bytes. Writes go to `staging`, `sync_all`, rename; `landed` syncs the object's folder, a failing sync after the rename being a warning (the object is there, so the keep is reported landed rather than failed and left unreferenced) (`an_object_whose_folder_could_not_be_synced_after_the_rename_is_reported_landed`). **A staging
file is held by its writer, which is how it is swept**: `Vault::staged` creates it `Held`
(`rust-style.md`); `Vault::open` removes those `writer::writer_of` reads as gone
(`Sweeping::WhatCrashed`); `Vault::sweep_the_staging` (`--prune`) also takes files not named by a
pid, never a living writer's. A staging file is a `Staged`; drop removes it, so a failure between
`File::create` and rename leaves nothing.

**The catalog names an object by its place inside the vault, never the vault's place.**
`Vault::open` canonicalises an existing root and makes the three folders, never the root
(`Vault::make` does where asked: `binary.md`). **A folder is a vault only where it says so**:
`Vault::make` writes `.resonate-vault`; `open` refuses a folder holding neither it nor an `audio/`
(an older vault, marked as it opens) as `Error::NotThere`, so an unmounted fstab mount point is never
filled on the root filesystem; the settings pane makes the folder a vault (off the window's thread)
before storing the key. `Vault::within` gives the relative path `vault_path`, `vault_objects.path`,
`cover_path` hold; `Vault::at` reverses it.

**Validation runs on the staging file before the rename** (a dedup hit is never at risk from a failed
import) **and reads the form that lands**: a WAVE object is weighed from its staged `.wav.zst`
through `VaultFiles` (the playing path). `Vault::verify` reads WAVE objects likewise, streaming.

## An object is weighed again when the encoder moves

`Encoding::OF_THIS_BUILD` names this build's encoders; each `vault_objects` row carries the one it
was weighed under. **Bump by hand whenever what an import writes changes** (new `flacenc`, other zstd
level, a `Form::of` bound, new kept-form stripping): nothing else says an object came from a worse
encoder. `the_encoders_an_object_is_stamped_for_are_the_ones_this_build_runs` pins the FLAC and zstd
output of fixed noise to the stamp: a `flacenc` upgrade or a changed `TUKEY_ALPHA` or `ARCHIVED_AT`
fails it until the stamp is bumped and the pins retaken.

`--import` re-walks a vaulted row whose stamp is behind, except a row whose source is gone and a row
`Form::of` keeps as is, unless the bump changed how that kind is kept (`sheds_tags_kept`: MP3, AAC,
DSD, Vorbis, Opus). Preview marks it *weighed again*. A renewal is a `Taking` with `renewing`: an
object already under the key is a rival, not a dedup hit; the new one replaces it (`Kept::replaced`,
`Landing::Replaced`) only if smaller; a loser keeps the standing object, stamps it current. **A
renewal settling on another key is weighed against the object it would replace**: `Taking::replacing`
carries that object's key and size (`TRACKS_TO_VAULT`), and a result no smaller is
`Refusal::NoSmaller`, so the vault never grows by a renewal
(`a_renewal_under_another_key_is_weighed_against_the_object_it_would_replace`). `keep` never discards
a replaced object; the old object of a renewal on another key, like one landed then refused, waits
for `--prune`. A plain dedup hit is stamped `Encoding::UNRECORDED` where the catalog has no row for
its object: weighed again next import.

**Objects from before `Encoding::KEYED_BY_THE_SOUND` (8) are named by their PCM alone, and are
carried forward, not broken.** The stamp moving makes every such row weighed again; its renewal lands
under the sound's key even at the same weight (`Replacing::keyed_by_the_pcm_alone`, read off the
standing object's stamp) (`a_renewal_off_a_key_of_the_samples_alone_lands_under_the_sound_at_the_same_weight`).
A row that cannot be renewed (its file gone) keeps its object: `Vault::verify` and `reframe` take a
name that is the sound's key or the PCM's alone (`Heard::answers_to`). The catalog step bringing the
stamp in releases every listener's row vaulted under a FLAC or WAVE object of another rate or channel
count (the objects such rows wrongly shared), kept tags with them; a delivered row, whose path is its
object, is left alone
(`a_row_vaulted_under_an_object_of_another_rate_or_count_leaves_the_vault_as_carried_forward`).

**A refusal is stamped against the encoder too.** Each `Keeping::Refused` writes `vault_refused
(track_id, under)` with `OF_THIS_BUILD`; `TRACKS_TO_VAULT` skips rows stamped at or past this
build's encoder. Trigger drops the stamp when size, mtime or span moves; `note_vaulted` when the row
lands. Unreadable sources (`Unreadable`, `SourceGone`) are stamped nothing
(`a_refused_row_is_not_weighed_again_until_its_file_or_the_encoder_moves`).

## Covers

`image` decode at the picture's own layout (grey or colour, with or without alpha, 8 or 16 bits per
sample), lossless JXL by `zune-jpegxl` at highest effort in that layout, an alpha every pixel of which
is opaque dropped. **Validation reads the staged file, not the bytes in memory**, through `jxl-oxide`
at the depth its header declares, and compares it sample for sample with the source's own decode
(`Drawn::held_by`), so a quantised or misread JXL never lands
(`a_sixteen_bit_picture_reads_back_at_sixteen_bits_and_is_not_held_by_its_eight_bit_shadow`).
**What a JXL of its first frame's pixels would lose is kept as it came** (`Drawing::AsItCame`): an
ICC profile, a second frame (APNG, animated WebP or GIF), a float layout, a side under two pixels
(`a_picture_carrying_a_colour_profile_or_more_than_one_frame_is_kept_as_it_came`,
`an_animated_cover_is_kept_as_it_came_with_every_frame`).

**A cover is kept as it came where that is no larger than its JXL** (lossless pixels of a lossy JPEG
can weigh several times the JPEG): `keep_cover` encodes the JXL first and, where the bytes it was
handed are no heavier, lands them under the same key with their own extension
(`PICTURE_EXTENSIONS`), read back byte for byte. `held_cover` finds a key under any extension (an
older build's JXL stays the dedup hit), `covers` lists them all, `picture` hands such a file back
undrawn (`a_cover_smaller_as_it_came_than_as_lossless_pixels_is_kept_as_it_came`).

**The vault decodes JXL back to PNG, so `resonate-ui` is unchanged.** `Vault::picture` answers a JXL
as `CoverArt { format: Png, bytes }`, the type every caller takes (window, `mpris::art::Pictures`);
`dependencies.md` forbids an image crate in `resonate-ui`.

A cover already under its key is a dedup hit sized from its head, read in chunks off the file
(`cover::size_of_file`), never the whole file. A cover decodes through `resonate_codec::decode_cover`
(sides within `LARGEST_COVER_SIDE`, at most `COVERS_DECODED_AT_ONCE` at a time, the window's
drawing included), so a burst of imports holds a bounded number of decoded pictures. **Drawn once a
run**: `Drawings` holds the PNG under the cover's key, evicting past `DRAWN_BYTES_AT_MOST` (48 MiB);
`Vault::forget` takes a drawing with the file; a `Forgetting` count drops a drawing made while its
cover was being pruned.

## What the catalog holds

`tracks.vault_key`, `tracks.vault_path`, `albums.cover_key`, `albums.cover_path`, `cover_source`
gaining `Vault`, the `vault_objects` and `kept_tags` tables; changes are `MIGRATIONS` steps
(`library.md`).

- **A vaulted row is still named by its own file; the vault stands in only when bytes are wanted.**
  `tracks.path` + `span_start` are its identity everywhere (location, playlist `Cut`, plays, share,
  resumption). `Library::stand_in` is a `resonate_codec::StandIn` registered via
  `Sources::standing_in`; `Decoder::open`, `open_span`, `probe`, `probe_span` ask it first; where the
  catalog names a `vault_path` for that `(path, span_start, span_frames)` they decode the object
  *whole* under a catalog-filled `TagSet`, including `rg_*` (`TagSet::heard_gain`) and
  `tracks.lyrics`. An object that will not open falls back to the row's file. `info`, scan, import
  have no stand-in: they read the file.
- **What the catalog cannot fill, the import keeps.** `vaulted::FILLED_FROM_THE_CATALOG` names the
  nineteen `TagField`s the stand-in reads off `tracks`, `albums` and `artists`; every other field
  `Kept::declared` holds (credits, comment, totals, label, catalogue number, barcode, grouping,
  copyright, BPM, compilation, sort names) goes to `kept_tags (track_id, field, value)` via
  `note_vaulted` and a delivery's row, spelled by `TagField::read`, set back by `TagField::set`
  (`every_field_set_into_a_tag_set_reads_back_as_it_was_spelled`,
  `a_vaulted_row_stands_in_under_the_credits_totals_and_notes_its_file_declared`). A release drops
  them with the vault columns; a row vaulted before the table keeps nothing until weighed again.
- **An object stands in at its source's depth, not its container's.** `Kept::bits` is the depth the
  source declared, written to `tracks.vault_bits`, carried on `StoodIn::bits` and laid over the
  object's own by `StoodIn::told_over`, so a study does not read a 32-bit WAVE's padding as a fake
  (`a_twenty_four_bit_master_kept_in_a_wider_wave_stands_in_as_twenty_four_bits`).
- **A vault inside a scanned root is not part of the library it holds.** The walk steps past the open
  vault's root (links included); `store::apply` leaves a rootless row alone wherever a scan reaches
  one.
- **`vault_path` is denormalised beside the key on purpose**: stand-in reads it by the row's unique
  key, one indexed read.
- **A vaulted row outlives its file; a changed file forgets its object.** Prune and root tidy keep a
  vaulted row whose file is gone (only copy); one the walk missed while its file is there was
  superseded and goes. The upsert clears `vault_key`/`vault_path` where size, mtime or span moved.
- **`Library::prune_the_vault` (`--prune`)**, under the `Walk` guard, removes audio objects no row
  names plus their `vault_objects` rows, every picture under `covers/` no album's `cover_key` holds,
  then staging. **An object's row goes only with its file**: one `Vault::forget` refuses keeps its
  row (`Pruned::left`). It walks `audio/` too (`Vault::objects`), taking any object neither
  `vault_objects` nor a `tracks.vault_key` names, only once its mtime is `LANDING_GRACE` (10
  minutes) old (a poll lands deliveries outside the `Walk` guard).
- **`Library::import` takes the `Walk` guard** (second caller: `Error::AlreadyWalking`), runs on
  `ImportOptions::workers` threads drawing rows over `costliest_first`; workers write their own
  catalog rows; the plan is re-sorted to request order. `landed_or_standing` decides and renames
  under one lock. First error stops every worker; a panicking one is `Error::Stopped`.
- **A cancel is heard inside a row.** `Taking::halt` is a `Halt` over the import's cancel flag
  (`Halt::NEVER` for a delivery); `wave::tally` and `flac::encode` look each decoded block,
  `packed_in_frames` each read, `pcm_of` each decoded block (read-back, the dedup decode of `foretold_flac`, `kept_whole`'s weighing) and the `copied` loop each read, answering `Error::Halted` with the staged file discarded; a read-back's failure is `NotValidated` but its `Halted` is never swallowed into one (`held_unless_halted`) (`a_cancel_is_heard_while_a_source_is_decoded_for_its_digest_and_while_it_is_copied_as_it_came`). `keep_one`
  answers a halted keep as no outcome: not counted, stamped or passed over, weighed whole next import
  (`a_keep_halted_mid_encode_lands_nothing_and_leaves_nothing_in_staging`).
- **An import yields cores to what plays.** `ImportProgress::hold_to` lets the first *n* workers draw
  rows, the rest waiting `HELD_BACK_LOOKS_AGAIN_AFTER` between looks; `None` lets every one work. The
  window holds an import to half the cores while the transport plays (`LibraryModel::heard_playing`)
  (`workers_past_what_the_import_is_held_to_wait_and_every_one_works_once_let_go`).
- **A row leaves the vault only where its own file is still there.** `Library::release_from_vault`
  (`vault --release`) clears `vault_key`/`vault_path` on every vaulted row whose `tracks.path`
  exists, counting the rest in `Released::stranded`. Objects wait for `--prune`, the one deleting
  gesture. Both are previews until `--apply` (`vault_release_foretold`, `vault_prune_foretold`;
  `Vault::staging_a_sweep_would_take` counts staging unswept). Covers stay.
- **`retag` passes a vaulted row over** (`Unwritten::Vaulted`); **`organise` does not** (vault keyed
  by content).
- **An album holds a cover where it holds either column**: `ALBUM_COLUMNS` and `asking_albums` read
  `cover_art IS NOT NULL OR cover_path IS NOT NULL`. `gather`'s fill takes the loser's whole cover
  only where the survivor holds neither.
- **A cover moves into the vault once per album**: `enriched::vault_the_cover` clears `cover_art` in
  the statement writing `cover_path`, only where `cover_art` is still the bytes the import encoded.
  `land_archive_cover` and the scan's `store::land_covers` carry `AND cover_path IS NULL`. Import asks
  `cover_the_vault_lacks`; a cover `image` cannot read or whose format code names nothing
  (`UntypedCoverArt`, `UnknownImageFormat`) is a warning and a `covers_passed` count, not the end of
  the import.
- **A failing vault ends the import; a failing source is passed over.** `Vault::failed_itself`
  separates them: `Error::Io` on a path under the root, a full, over-quota or read-only disc
  anywhere, and `OutsideTheVault` are the vault's (`keep_one` answers `Error::Vault`, stopping every
  worker); anything else is `Passing::Unreadable`. A read of the source names the source, never the
  staging file: `Error::Source { location, .. }`. A source kept whole whose decode fails out of reach
  (`codec::Error::is_out_of_reach`: a dropped share, a timed-out read) answers `Error::Codec`,
  unstamped; only a decode failing on the bytes themselves is `Refusal::NotValidated`
  (`a_source_that_drops_partway_is_neither_refused_nor_blamed_on_the_vault`).

## Reading an object back

`VaultFiles` is a `MediaProvider` under `SourceId::local()`, *replacing* `LocalFiles`: a path under
the vault root ending `.zst` is an `Unpacking` (inner extension as `FormatHint`), decoded as read,
never decompressed into memory; anything else is a plain open. **A seek backward, or more than
`FAR_AHEAD` (4 MiB) forward, restarts at the frame holding the target** (`unpacking::frames_of`
indexes frame headers once); no content size: restart from the start. **Every restart reads the file
the reader opened, not its path** (one `File` in an `Arc`, read by position via `Opened`), so a
renewal landed mid-play leaves the reader on the object it began with
(`a_seek_back_reads_the_object_it_opened_after_another_is_landed_under_its_name`). An object an
earlier build packed as one frame naming no length is repacked by `Vault::reframe`
(`vault --verify --apply`) through `wave::packed_in_frames`, read back to its key, landed over itself
(`a_wave_packed_before_the_frames_is_repacked_in_frames_and_reads_back_the_same`). `Vault::offer` hands an object to be sent to another device: a `.zst` object as the WAVE it
unpacks to (size from the RIFF header, extension `wav`), anything else as its file (`sync.md`). The binary
registers it wherever it opens a track to play or read (`held_over`). Bus, playlists, resumption,
queue never see an object's path.

## What a provider delivers

`Library::poll` hands `Delivery::File` to `Vault::keep` as a local location and `Delivery::Stream` to
`Vault::keep_delivered`: copies the reader into `staging/` through a `take` one byte past
`LARGEST_DELIVERY` (past: `Refusal::TooLarge`, never decoded), keeps it like any local file, always
discards the staging file; extension filtered to ASCII letters and digits. The poll writes the
`vault_objects` row, a rootless `tracks` row named by the object's path and paired with the wanted
release track (carrying `Kept::declared`), and records `wants.offered` as the *vault* object's URI
(`providers.md`). A poll cancelled mid-keep still notes a landed object. Two landed objects are
deliberately not noted and wait for `--prune`: one whose `Kept::frames` disagree with the release
row's length (`LENGTHS_AGREE_WITHIN`), one landing after that row was paired with a listener's file.
**`Kept::frames` is what the source decoded to in every form**: for `Form::Kept`, the frames
`kept_whole` weighed, not the container's declared length.

## An Ogg stream's comments are rewritten, and every page after renumbered

Vorbis/Opus tags sit in header packets of their own (comment after identification; Vorbis also setup
after it): stripping rewrites structure. `ogg::bare` names the codec by the first page's magic, walks
that serial's pages in sequence until the header packets are whole, and copies the file as is on
header pages of another serial, a skipped sequence number, headers past `HEADER_BYTES_AT_MOST` (64
MiB), or a last header packet ending anywhere but at its page's end. The comment packet is rewritten
to its vendor string alone (count zero, plus Vorbis's framing bit).

**Ogg FLAC is the same walk over metadata blocks.** First packet: `\x7fFLAC`, mapping version, count
of following header packets (zero = *unknown*: headers whole at the block marked last), STREAMINFO.
Kept: one VORBIS_COMMENT, vendor alone, marked last; a stream whose first header is not its comment
is copied as is. The first page then says one header follows and is restamped, unless it counted
none.

New header packets go on fresh pages under the next sequence numbers (granule zero on a page a packet
ends on, `NO_PACKET_ENDS` on one none does), each stamped with the Ogg CRC-32
(`a_page_is_stamped_with_the_checksum_libogg_gives_it`). `Renumbering` is the difference in header
pages and `Renumbered` rewrites sequence and checksum of every later page of that serial, passing
other serials' pages and anything unparseable verbatim. `kept_whole` makes this safe: a copy not
holding the same audio is recopied whole.
