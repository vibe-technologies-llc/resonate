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
**The library it came from is read and never written to** — no source file is moved, renamed,
retagged or deleted (`the_file_the_vault_read_is_left_exactly_as_it_was`). `tracks.path` keeps
naming the original, so a rescan reads an untouched source as unchanged and `organise` goes on
filing it.

A leaf beside `resonate-codec`: chiefly `resonate-core`, `resonate-codec`, `symphonia`, `flacenc`,
`zstd`, `zune-core`, `zune-jpegxl`, `jxl-oxide` and `image`, and no SQL — every row belongs to
`resonate-library`. `cargo tree -p resonate-vault` stays free of gpui, the engine, the library and
`ureq`.

## What it promises

- **Bit-exact or nothing.** Every object is read back through the player's decoder and its PCM
  weighed against what went in, by MD5. A mismatch removes the file and answers
  `Refusal::NotValidated`; nothing half-written is pointed at. An encode of no frames is
  `Refusal::Empty`.
- **Never larger than what it came from.** A re-encode that does not beat the source is discarded
  and the source kept, so `Form` is decided twice — by what the stream *is* and by what the encode
  cost. Unconditional, not a setting: an archive that inflates a library is worse than none. A row
  a sheet cuts out of a file cannot keep the file (`Form::Kept` for a cut is
  `Refusal::CutFromAnother`), so it is weighed against its *share* — file size × its frames ÷ the
  file's frames — and a re-encode no smaller is `Refusal::NoSmaller`. `Kept::was` is that weight,
  whole or shared (the stored bytes for a dedup hit), and is what the library's `was_bytes`
  counts, so a single-file rip is not counted once per row. **The weighing happens before anything
  lands**: `landed_or_standing` weighs the staged object — or the one already under its key —
  and answers `Landing::NoSmaller` without renaming, so nothing is put in `audio/` and taken away
  again, and two imports can land one key at once without one deleting what the other's row names.
- **A kept object is weighed like an encoded one.** `kept_whole` decodes source and staged copy
  through the same `pcm_of` and refuses differing digests, so a stripped copy is proved to hold
  the source's audio, not merely to decode. A stripped copy that fails is copied again whole and
  weighed again, so a source whose decoder read a tag as a frame is still kept, tags and all.
  `bare` rewrites a FLAC's metadata only where its walk reached the last block; one stopping at
  the bound or on a short read hands the file back from its start to be copied as it stands,
  rather than a STREAMINFO marked last ahead of blocks still in the copy.
- **One copy of one thing.** The key is the MD5 of the decoded PCM, so one audio in two containers
  is one object; a cover is keyed by its bytes, so twelve tracks embedding one picture cost one
  JXL and eleven dedup hits.
- **Nothing outside the root.** `Vault::inside` guards every read, write and delete; a path not
  under the root is `Error::OutsideTheVault`. `holds`, which it asks, is `within`: what follows the
  root must be plain names, as `Vault::at` asks of what it is handed, so `<root>/../x` — which a
  lexical `starts_with` passed — is outside
  (`a_path_climbing_out_of_the_vault_is_not_held_by_it`).

## The three forms

`Form::of` reads codec, spec and declared depth and answers before anything is decoded; the size
comparison may overrule it.

- **`Form::Flac`** — integer PCM, 8–24 bits, ≤ 8 channels, ≤ 96 kHz. A streaming
  `flacenc::source::Source` over `resonate_codec::Decoder`, so a track is never held whole, written
  frame by frame with the header rewritten at the end. It carries STREAMINFO and **nothing else**
  (no VORBIS_COMMENT, PICTURE or SEEKTABLE): stripping is construction, not a later pass.
- **`Form::Wave`** — PCM FLAC cannot hold: `SampleFormat::F32`, > 24 bits or > 96 kHz. A canonical
  `fmt `+`data` WAVE with no `LIST` or `id3 ` chunk, stripped by construction, zstd'd at `ARCHIVED_AT`;
  refused over `LARGEST_PCM`, the RIFF ceiling — before anything is staged where the source declares
  its length (`wave::outgrows_a_wave`, frames × channels × four bytes), so 46 minutes of 192 kHz
  stereo no longer writes 4 GiB of staging on every import, and a whole file refused `TooLarge` is
  handed to `kept_whole` as a `NoSmaller` one is, rather than never vaulted
  (`a_source_past_what_a_wave_holds_is_kept_as_it_stands_without_being_staged`). A cut row has no
  kept form, so it is still weighed by writing it. `wave::compressed` counts what the encoder wrote and
  gives up — `Packed::NoSmaller` — the moment it reaches the source's weight, before the read-back, so a
  hi-res source whose WAVE would be thrown away costs a fraction of a level-19 pass; `keep` hands such a
  whole file to `kept_whole` as it does one whose finished object came out too large. **A pass is
  foretold before it is paid for**: past `FORETOLD_FROM_BYTES` (32 MiB staged), `wave::hopeless`
  compresses four `SLICE_BYTES` slices spread through it at the same level and scales them to the whole;
  landing more than an eighth past the source's weight is `NoSmaller` before the pass starts. Measured
  on a five-minute 24/192 FLAC of 251 MB: foretold 504.9 MB of a pass that came to 508.3 MB and took 40
  s on five cores, so a hi-res rip is kept after ~8 MB of zstd; a forecast within the eighth pays the
  pass, which still decides.
- **`Form::Kept`** — the source's own bytes, where re-encoding would lose something or cost more
  than it saves: a lossy codec, DSD, > 8 channels, or a re-encode no smaller. What a container
  keeps its tags in *around* the audio is left behind, without touching an audio frame.
  `bare::bare` decides by the container symphonia opened: a FLAC's metadata blocks are rewritten
  to STREAMINFO alone; an MPEG, ADTS, WavPack or Monkey's Audio stream sheds every ID3v2 tag
  stacked before the frames and, from the end inward, ID3v1 with its enhanced `TAG+`, APEv2 with
  or without header, Lyrics3v2 and an appended ID3v2 read by its footer; a DSF ends where its
  metadata pointer pointed, header rewritten to that length with a null pointer; an Ogg Vorbis,
  Opus or FLAC stream gets an empty comment packet (`ogg::bare`). A tag claiming more than the file
  holds, or tags leaving no frames, leave the file whole.
  **Chunked containers shed their describing chunks.** `chunks::shed` walks a WAVE, AIFF/AIFC, CAF
  and DSDIFF chunk by chunk — past an ID3v2 a tagger stacked before the header, which goes too —
  and leaves out a WAVE's `LIST`, `id3 `, `ID3 `, `bext`, `iXML`, `axml`, `_PMX`; an AIFF's `NAME`,
  `AUTH`, `(c) `, `ANNO`, `COMT`, ID3; a CAF's `info`; a DSDIFF's `DIIN`, `COMT`, `ID3 `. Every
  other chunk is copied with its pad byte; anything after the header's declared end (an appended
  ID3v1) is left behind; the header size is rewritten (a CAF has none). An unreadable walk — a
  chunk past the end, a header of the wrong kind — copies the file whole. The copy is
  `chunks::Passing`, a reader seeking over the left-out ranges; a data chunk declaring more than
  the file holds is read to the end rather than refused, which is how a streamed WAVE is written.
  `a_kept_wave_sheds_the_tags_its_chunks_carry_and_keeps_every_sample` is the claim.
  **An MP4 or Matroska has its tags blanked where they stand**, since they sit inside the
  structure indexing the audio (MP4 chunk offsets count from file start; a Matroska SeekHead and
  Cues name positions) and cutting them would mean rewriting every offset behind.
  `blanks::blanked_movie` walks the boxes into `moov` and each `trak` and turns every `udta` and
  every top-level or `moov`-level `meta` into an empty `free` box of the same length;
  `blanks::blanked_segment` lays an EBML `Void` of exactly the same length over every top-level
  `Tags` and `Attachments` and over the `Title` in `Info` (where ffmpeg writes one). Nothing moves;
  `Blanked` lays the blanks over the copy as it passes; symphonia reads a SeekHead entry pointing
  at a Void as an element it does not want. A box or element running past its parent, or an
  unknown-sized one before the tags, copies the file whole.
  `a_kept_mp4_blanks_the_tags_its_movie_holds_and_keeps_every_sample_it_decodes_to` and
  `a_kept_matroska_blanks_its_tags_and_keeps_every_sample_it_decodes_to` are the claims.

**Speaker positions are part of what is kept.** `MediaInfo::speakers` is the source's positions
as symphonia reads them (its `Position` bits = the WAVE channel mask), and `Form::placing` weighs
them after `Form::of`: FLAC fixes an assignment per channel count and an object with STREAMINFO
alone has no room for a mask, so a source FLAC would read back differently — a 2.1 whose LFE FLAC
calls a centre, a 5.1 on the sides FLAC calls rear — goes to `Wave`, which writes
`WAVE_FORMAT_EXTENSIBLE` with the source's mask wherever it is not the order a plain header
implies; a mask past the eighteen WAVE names is `Kept`. Validation weighs the positions read back
beside digest and frame count (`Heard::held_by`), so an object whose speakers moved is refused; a
source naming none is held by any reading.

**Three bounds are flacenc's, not FLAC's.** The format holds 32 bits and 655 kHz; `flacenc` 0.5.1
verifies `sample_rate <= 96_000` and `bits_per_sample <= 24`, and its Rice parameter stops at 14
where the second partition method reaches 30. The last shapes a real library: on this material
flacenc **beats** `flac -8` at 16/44.1 (23,597,708 vs 24,829,499 bytes) and loses by ~15 % at
24/48, because 24-bit residuals need Rice parameters a 4-bit field cannot name. So 16-bit rips are
re-encoded and 24-bit ones kept and stripped — the size comparison arrives there with no rule
naming depths.

## Keys and layout

```
<root>/audio/ab/abcdef…7f.flac        # or .wav.zst, or .mp3, .dsf, … for a kept object
<root>/covers/3c/3cd1…9a.jxl
<root>/staging/<pid>-<n>.<ext>
```

A `VaultKey` is sixteen bytes as thirty-two lowercase hex letters, fanned two deep. For `Flac` and
`Wave` it is the MD5 of the decoded interleaved PCM — FLAC's STREAMINFO digest, so `metaflac` can
be asked the same — and for `Kept` and covers the MD5 of the stored bytes. Writes go to `staging`,
are `sync_all`'d and renamed into place (like `config::write` and `organise`'s staged sheet), so a
crash leaves a staging file, never a half-written object; `Vault::sweep_the_staging` is what
`--prune` clears them with.

**A staging file is a `Staged`, and dropping one removes the file.** Every write that can fail
between `File::create` and the rename — a source read, a full disc, `sync_all`, a refused landing —
returns through `?` leaving nothing, so a thousand failed imports leave no partial copies. `keep`'s
fallback to `Kept` follows a `NoSmaller` that landed nothing, so no object is orphaned in `audio/`.

**The catalog names an object by where it sits inside the vault, never where the vault sits.**
`Vault::open` canonicalises a root that is there and makes the three folders inside it, never the
root itself — `Vault::make` makes the root first, for the callers asked to (`binary.md`);
`Vault::within` turns an answered path into the relative one
`vault_path`, `vault_objects.path` and `cover_path` hold (`audio/ab/ab…7f.flac`); `Vault::at` turns
it back, refusing anything not a run of plain names, so no stored value reaches outside. A vault
opened as `./vault`, through a symlink or after a move is the same vault to catalog, stand-in and
prune.

**Validation happens on the staging file, before the rename**, so a dedup hit is never at risk
from a failed import of the same audio and a refused object never reaches `audio/`. **It reads the
form that lands**: a WAVE object is weighed from its staged `.wav.zst` through `VaultFiles` — the
`Unpacking` stream the player reads it through — not from the uncompressed WAVE it was packed from,
which is discarded first; so every object, not only FLAC and kept ones, is proved by the path that
plays it. `Vault::verify` reads a WAVE object the same way, streaming rather than unpacking it into
one buffer and staging it again
(`a_packed_wave_that_has_been_meddled_with_is_not_verified`).

## An object is weighed again when the encoder moves

`Encoding::OF_THIS_BUILD` names this build's encoders and every `vault_objects` row carries the one
it was weighed under. **Bump it by hand whenever what an import writes changes** — a new
`flacenc`, another zstd level, a bound `Form::of` draws differently — since nothing else can tell
an object was made by a worse encoder.

`--import` walks a vaulted row again wherever its stamp is behind, except two it cannot improve: a
row whose source has gone (the object is the only copy), and a row `Form::of` keeps as it stands
whatever the encoder — lossy, DSD, > 8 channels — unless MP3, AAC or DSD (whose kept copies
encoding 2 began stripping) or Vorbis or Opus (encoding 3); an MP4's AAC, a DSDIFF or a Vorbis in
Matroska is copied again to the same key and stamped. Encoding 4 began stripping Ogg FLAC, which
`Form::of` never keeps and so is walked regardless. Encoding 5 began shedding WAVE, AIFF, CAF and
DSDIFF tag chunks, whose kept objects are DSD, AAC (already walked) or integer PCM (never kept by
`Form::of`), so it needs no rule; nor does encoding 6, blanking kept MP4 and Matroska tags, whose
audio is AAC, Vorbis or Opus (already walked) or lossless (never kept). The preview marks such a
row *weighed again*. A renewal is a `Taking` with `renewing` set, and changes the one rule that
would hide the new encode: an object already under the same key is a rival, not a dedup hit, and
the new one replaces it — `Kept::replaced` set from `Landing::Replaced`, a rename over the standing
file — only where smaller. A losing renewal keeps the standing object and stamps it current, having
now been weighed against this build. `keep` never discards an object it replaced, so rows naming
the same audio keep their object whatever this row decides; a renewal settling on another form or
key leaves the old object to `--prune`.

A plain dedup hit is stamped `Encoding::UNRECORDED` where the catalog has no row for its object —
a vault standing from before the catalog was deleted and rescanned — so an unrecorded encoder's
object is weighed again next import rather than trusted.

## Covers

Decoded with `image`, encoded as lossless JXL by `zune-jpegxl` at its highest effort, read back
with `jxl-oxide` and compared pixel for pixel before landing — lossless makes an exact compare the
real check. Opaque pictures are three channels, transparent ones four, so the common case carries
no alpha plane.

**The vault decodes JXL back to PNG, which is why `resonate-ui` did not change.** `Vault::picture`
answers `CoverArt { format: Png, bytes }`, the type every caller already takes: the window hands
gpui encoded bytes plus a `gpui::ImageFormat`, and `mpris::art::Pictures` writes a file a
notification daemon can draw. `dependencies.md` forbids an image crate in `resonate-ui`, and a JXL
a daemon cannot read would break `mpris:artUrl`; one decode in the crate owning the format answers
both.

**A cover is drawn once a run.** `Vault::picture`'s PNG is written at `image`'s fast compression
(a transient form), and `Drawings` holds it under the cover's key — least lately asked first out
past `DRAWN_BYTES_AT_MOST` — so the window and `retag` asking again cost a lookup, not a JXL
decode and PNG encode. `Vault::forget` takes a drawing with the file, so a pruned cover is not
answered from memory.

## What the catalog holds

`tracks.vault_key`, `tracks.vault_path`, `albums.cover_key`, `albums.cover_path`, `cover_source`
gaining `Vault`, and a `vault_objects` table — laid into `V1` while the schema still broke (so a
catalog written before was scanned again); a change now is a `MIGRATIONS` step (`library.md`).

- **A vaulted row is still named by its own file; the vault stands in only when bytes are
  wanted.** `tracks.path` and `span_start` are its identity everywhere — a `Track`'s location, a
  playlist's `Cut`, a counted play, a share, a resumption — so it counts plays, exports as its
  file and resumes as its cut. What the player opens is decided at its `Sources`:
  `Library::stand_in` is a `resonate_codec::StandIn`, registered with `Sources::standing_in`.
  `Decoder::open`, `Decoder::open_span`, `probe` and `probe_span` ask it first, and where the
  catalog names a `vault_path` for that `(path, span_start, span_frames)` — the span matched
  whole, so an open with no span, or one sharing only a cut's start, reads the file rather than
  the first cut's object — they decode the object *whole* (a cue row's object is that row alone,
  so the span is not applied twice) under a `TagSet` the catalog fills: names, numbers, MusicBrainz
  ids, `rg_*` as the ReplayGain the engine resolves gain from, and `tracks.lyrics`, which the scan
  keeps for exactly this. An object that will not open falls back to the row's file.
  `resonate info`, the scan and the import open through sources with no stand-in, so they always
  read the file.
- **A vault inside a scanned root is not part of the library it holds.** The walk steps past the
  open vault's root, a link into it included, so an object is never scanned as a track titled by
  its digest; `store::apply` leaves a rootless row alone wherever a scan reaches one anyway (a
  catalog opened without its vault), so a delivery keeps its name, pairing and place outside every
  root. `a_vault_kept_inside_a_root_is_never_scanned_as_tracks_of_its_own` and
  `a_delivered_row_a_scan_walks_over_keeps_its_name_and_belongs_to_no_root` are the claims.
- **`vault_path` is denormalised beside the key on purpose**: the stand-in reads it by the row's
  unique key, one indexed read rather than a join through `vault_objects`.
- **A vaulted row outlives its file, and a changed file forgets its object.** The prune and root
  tidy beside a scan leave a vaulted row whose file has gone (the vault holds the only copy, and a
  pruned row would hand `--prune` the object); a vaulted row the walk did not see while its file
  is still there was superseded — a sheet no longer cuts it — and goes as any row would. The upsert
  clears `vault_key` and `vault_path` wherever size, mtime or span moved, so a file ripped again in
  place is weighed again by the next `--import`.
- **`Library::prune_the_vault` (`--prune`) weighs a cover by its key.** Under the `Walk` guard (so
  it cannot remove an object an import landed and has not yet noted), it removes audio objects no
  row names and their `vault_objects` rows, then every JXL under `covers/` whose key no album's
  `cover_key` holds, then the staging folder. Matching by key, no spelling of the root makes a
  named cover look loose. **An object's row goes only with its file**: one `Vault::forget` refuses
  keeps its row for the next prune and is counted in `Pruned::left`, where forgetting the row anyway
  left the file on the disc with nothing to find it by
  (`an_object_a_prune_could_not_take_away_keeps_its_row_for_the_next`). **It walks `audio/` as well
  as the rows** (`Vault::objects`), taking any object whose key neither `vault_objects` nor a
  `tracks.vault_key` names — one that landed before `note_vaulted` failed. A poll lands deliveries
  outside the `Walk` guard, so an unnamed object is taken only once its mtime is `LANDING_GRACE` (ten
  minutes) old, the landing-to-noting gap being milliseconds
  (`a_prune_takes_away_an_object_no_row_names_once_it_has_stood_a_while`).
- **`Library::import` takes the `Walk` guard**, reading every source file; a second caller gets
  `Error::AlreadyWalking`.
- **An import runs on `ImportOptions::workers` threads** (the machine's parallelism by default),
  the encode being per track. Workers draw the next row off one shared counter over
  `costliest_first` — a WAVE-bound row before a FLAC-bound one, kept last, larger files first
  within each — so the one slow encode starts at once rather than alone at the end; they write
  their own catalog rows, and an album's cover is kept by whichever worker claims the album first
  in a shared set. The plan is put back into request order before it is handed out, so preview and
  apply list alike. `Vault` makes it sound: `landed_or_standing` decides and renames under one
  lock, so two rows of one audio settle on one object, one `Landed`, one `Deduped`. The first
  error stops every worker at its next row and is the pass's answer; a panicking worker is
  `Error::Stopped` naming the import.
- **A row leaves the vault only where its own file is still there.**
  `Library::release_from_vault` (`resonate vault --release`, same `--root` narrowing and `Walk`
  guard) clears `vault_key` and `vault_path` on every vaulted row whose `tracks.path` exists, so
  the player opens the file again, and counts the rest as `Released::stranded`, left alone because
  the vault holds their only copy. Objects a release leaves wait for `--prune`, the one deleting
  gesture; the next `--import` weighs a released row again. Covers stay: an album's picture moved
  into the vault had `cover_art` cleared in the same statement, so there is nothing to point back
  at.
- **`retag` passes a vaulted row over** (`Unwritten::Vaulted`) — writing tags nothing reads is work
  for nothing. **`organise` does not**: `tracks.path` still names the user's file and the vault is
  keyed by content, so filing it moves nothing the vault depends on.
- **An album holds a cover where it holds either column.** `ALBUM_COLUMNS` and `asking_albums`
  read `cover_art IS NOT NULL OR cover_path IS NOT NULL`, so a vaulted cover is warmed by the
  window and spares the enrichment a fetch `land_archive_cover` would throw away. `gather`'s fill
  takes the loser's whole cover — art, format, source, key and path — only where the survivor
  holds neither, so a gathered album never holds both nor names a source without the picture; it
  keeps the earlier of the two `favourite` stamps likewise.
- **A cover moves into the vault once per album**: `enriched::vault_the_cover` clears `cover_art`
  in the statement that writes `cover_path`, so an album never holds both. `land_archive_cover`
  and the scan's `store::cover` carry `AND cover_path IS NULL`, the latter counting a
  vault-covered album as covered, so neither an archive cover nor a rescanned file's lands where
  the vault holds one. The import asks `cover_the_vault_lacks` rather than `cover_art` (which would
  hand back the vault's own PNG to keep again under another digest), and a cover `image` cannot
  read — or one whose stored format code names nothing, `UntypedCoverArt` and `UnknownImageFormat`
  — is a warning and a `covers_passed` count, not the end of the import
  (`a_cover_naming_no_format_the_vault_knows_is_passed_over_and_the_import_carries_on`).
- **A vault that fails ends the import; a source that fails is passed over.** `Vault::failed_itself`
  tells the two apart: an `Error::Io` on a path under the root, or a full, over-quota or read-only
  disc wherever it was, and `OutsideTheVault`, are the vault's, and `keep_one` answers them as
  `Error::Vault`, which stops every worker; anything else — a source that will not decode or encode
  — is `Passing::Unreadable` and the import goes on. Every failure was once `Unreadable`, so a full
  vault disc cost most of the work of every row left and billed each as unreadable
  (`a_vault_that_cannot_write_ends_the_import_rather_than_billing_every_row`,
  `a_failure_on_the_vaults_own_disc_is_told_from_one_of_the_source`).

## Reading an object back

`VaultFiles` is a `MediaProvider` registered under `SourceId::local()`, *replacing* `LocalFiles`: a
path under the vault root ending in `.zst` is an `Unpacking` with the inner extension as its
`FormatHint`; everything else is the plain file open `LocalFiles` does. `Unpacking` decodes the
stream as read rather than decompressing the object into a `Cursor`, which held a five-minute
24/192 object — ~460 MB — resident for every open, each queued row's tag probe included, and two
for a probe beside a play. It reads its length from the `RIFF` header this build wrote, seeks
forward by decoding and discarding and backward by restarting the stream, so a probe costs the
header, a play one pass, and only a backward seek pays for the stretch before it. It opens the
object the stand-in names, and the binary registers it wherever it opens a track to play or read
(`held_over`): the player, `resonate info`, `explain` and `analyse`. The bus, playlists,
resumption and queue never see an object's path; a row is named by its own file.

## What a provider delivers

`Library::poll` hands a `Delivery::File` to `Vault::keep` as a local location and a
`Delivery::Stream` to `Vault::keep_delivered`, which copies the reader into
`staging/<pid>-<n>.<ext>` through a `take` one byte past `LARGEST_DELIVERY` (the RIFF ceiling),
`sync_all`s, keeps it like any local file and discards the staging file whatever came of it. A
stream past the cap is `Refusal::TooLarge`, never decoded. The extension is filtered to ASCII
letters and digits (as `named_extension` filters a location's), so a delivered `../flac` stages as
`flac` under `staging/` and nowhere else. The poll writes the `vault_objects` row, a rootless
`tracks` row named by the object's path and paired with the wanted release track — carrying the
genre, ReplayGain and words `Kept::declared` says the source declared, since the object declares
nothing — and records `wants.offered` as the *vault* object's URI, where the bytes now are.
`providers.md` has why the row belongs to no root. A poll cancelled mid-keep stops at that file
boundary like every pass: a landed object is noted, row and want, before the poll ends, because
an unnamed object is otherwise left for `--prune` to find on the disc
(`a_delivery_that_landed_as_the_poll_was_cancelled_is_still_noted`).
`a_delivered_file_lands_in_the_vault_and_the_want_names_where_it_went` and
`a_streamed_delivery_lands_in_the_vault_and_leaves_nothing_in_staging` are the claims.

## An Ogg stream's comments are rewritten, and every page after renumbered

A Vorbis or Opus stream keeps its tags in a header packet of its own — the comment packet after the
identification header, and for Vorbis the setup packet after it — so stripping is a rewrite of
structure, not a cut. `ogg::bare` reads the first page (both specs give it to the identification
header alone) and names the codec by that packet's magic; walks the pages of that serial in
sequence until the header packets are whole; and refuses — copying the file as it stands — header
pages of another serial, a skipped sequence number, headers past `HEADER_BYTES_AT_MOST`, or a last
header packet ending anywhere but at its page's end (the first audio packet must begin a page).
The comment packet is rewritten with its vendor string and nothing after — a count of zero, plus
Vorbis's framing bit; a stream already so is copied as it stands.

**Ogg FLAC is the same walk over metadata blocks.** Its first packet is the 51 bytes of `\x7fFLAC`,
the mapping version, a count of following header packets and STREAMINFO; each header packet after
is one native metadata block, VORBIS_COMMENT first as the mapping requires. The count may be zero
(*unknown*), so headers are whole at the block marked last, and one whose STREAMINFO is marked last
has none to shed. Kept: one VORBIS_COMMENT with its vendor alone, marked last, so PICTURE, PADDING,
SEEKTABLE and CUESHEET go as in native FLAC; a stream whose first header is not its comment is
copied as it stands. The first page then says one header follows and is restamped — unless it
counted none, left saying so.

The first page is otherwise kept byte for byte; the new comment and setup packets go on fresh pages
under the next sequence numbers, granule zero on a page a packet ends on and `NO_PACKET_ENDS` on
one none does, each stamped with the format's CRC-32 — polynomial `04c11db7`, unreflected, over the
page with its checksum zeroed, which `a_page_is_stamped_with_the_checksum_libogg_gives_it` holds to
an ffmpeg-written page. A picture usually made the comments several pages long, so the headers now
take fewer pages and every later audio page would skip numbers: `Renumbering` is the difference,
and `Renumbered` wraps the copy that follows, reading a page at a time and rewriting sequence and
checksum of every page of that serial, passing another serial's pages (a chained stream's next
link) through, and passing anything that does not parse as a page verbatim from there on.
Validation makes the last two safe: `kept_whole` decodes the copy against the source, and one not
holding the same audio is copied again whole.
`a_kept_ogg_vorbis_sheds_its_comments_and_keeps_every_packet_it_decodes_to`, its Opus twin and
`a_kept_ogg_flac_sheds_its_comment_and_keeps_every_frame_it_decodes_to` — a 24-bit, 192 kHz stream
whose zstd'd WAVE does not beat it, which is how a FLAC is ever kept — are the claims, each checking
sequence numbers and checksums with a bit-by-bit CRC rather than the vault's table.
