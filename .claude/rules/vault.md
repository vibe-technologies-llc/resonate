# The vault

`resonate-vault` is the managed archive: one store this build writes itself, beside the catalog
that describes it. An import decodes a track, throws every tag and picture away, keeps the
smallest bit-exact copy it can make, validates that copy by reading it back, and points the
catalog row at it. **The library it was imported from is read and never written to** — no source
file is moved, renamed, retagged or deleted, and `the_file_the_vault_read_is_left_exactly_as_it_was`
is the claim.

It is a leaf beside `resonate-codec`: `resonate-core`, `resonate-codec`, `flacenc`, `zstd`,
`zune-jpegxl`, `jxl-oxide` and `image`, and no SQL at all. Every row belongs to
`resonate-library`, the way every other row does. `cargo tree -p resonate-vault` must stay free of
gpui, the engine, the library and `ureq`.

## What it promises

- **Bit-exact or nothing.** Every object is read back through the same decoder the player uses and
  the PCM it answers with is weighed against the PCM that went in, by MD5. A mismatch removes the
  file and answers `Refusal::NotValidated`; nothing half-written is ever pointed at.
- **Never larger than what it came from.** A re-encode that does not beat the source is discarded
  and the source is kept instead, which is why `Form` is decided twice — once by what the stream
  *is* and once by what the encode turned out to cost. An archive that inflates a library is worse
  than no archive, so the comparison is unconditional rather than a setting. A row a sheet cuts
  out of a file cannot keep the file, so it is weighed against its *share* of it — the file's size
  times the frames it holds over the frames the file holds — and a re-encode no smaller is
  `Refusal::NoSmaller`. `Kept::was` is that weight, whole or shared, and it is what `was_bytes`
  counts, so a single-file rip is not counted once per row. **The weighing happens before
  anything lands**: `landed_or_standing` compares the staged object — or the one already standing
  under its key — against that weight and answers `Landing::NoSmaller` without renaming, so no
  object is ever put in `audio/` and then taken away again, which is what lets two imports land
  the same key at once without one deleting what the other's row now names.
- **A kept object is weighed like an encoded one.** `kept_whole` decodes the source and the
  staged copy through the same `pcm_of` and refuses the copy where the two digests differ, so a
  stripped copy is proved to hold the source's audio rather than merely to decode. A stripped
  copy that does not is copied again whole and weighed again, so a source whose decoder read a
  tag as a frame is still kept, tags and all, rather than refused. `bare` rewrites a FLAC's
  metadata only where its walk reached the last block; one that stops at the bound or on a short
  read hands the file back from its start and it is copied as it stands, rather than a STREAMINFO
  marked last ahead of blocks still in the copy.
- **One copy of one thing.** The key is the MD5 of the decoded PCM, so the same audio arriving in
  two containers is one object; a cover is keyed by its own bytes, so an album's twelve tracks
  embedding one picture cost one JXL and eleven dedup hits.
- **Nothing outside the root.** `Vault::inside` guards every read, write and delete, and a path
  that is not under the root is `Error::OutsideTheVault` rather than an action.

## The three forms

`Form::of` reads the codec, the spec and the declared depth and answers before anything is
decoded; the size comparison may then overrule it.

- **`Form::Flac`** — integer PCM at 8 to 24 bits, up to 8 channels, up to 96 kHz. A streaming
  `flacenc::source::Source` over `resonate_codec::Decoder`, so a track is never held whole in
  memory, written frame by frame with the header rewritten at the end. The object carries
  STREAMINFO and **nothing else** — no VORBIS_COMMENT, no PICTURE, no SEEKTABLE — which is what
  stripping *is* here rather than a pass that removes them afterwards.
- **`Form::Wave`** — PCM that FLAC cannot hold: `SampleFormat::F32`, over 24 bits, or over
  96 kHz. A canonical `fmt `+`data` WAVE with no `LIST` and no `id3 ` chunk, so metadata is
  stripped by construction, zstd'd at `ARCHIVED_AT`. Refused over `LARGEST_PCM`, the RIFF ceiling.
  `wave::compressed` counts what the encoder has written and gives up — `Packed::NoSmaller` — the
  moment it reaches what the source weighs, before the read-back is spent on it, so a hi-res
  source whose WAVE would only be thrown away costs a fraction of a level-19 pass rather than
  the whole of one; `keep` hands a whole file that answered `NoSmaller` to `kept_whole` the way
  it hands one whose finished object came out too large.
- **`Form::Kept`** — the source's own bytes, because re-encoding would lose something or cost more
  than it saves: a lossy codec, DSD, more than 8 channels, or a re-encode that came out no smaller.
  What a container keeps its tags in *around* the audio is left behind, so the promise about tags
  and pictures holds without touching a single audio frame. `bare::bare` is the whole of it and
  it decides by the container symphonia opened: a FLAC has its metadata blocks rewritten to
  STREAMINFO alone; an MPEG or ADTS stream sheds every ID3v2 tag stacked in front of the frames
  and, from the end inward, ID3v1 with its enhanced `TAG+`, APEv2 with or without its header,
  Lyrics3v2 and an appended ID3v2 read by its footer; and a DSF ends where its metadata pointer
  pointed, its header rewritten to that length and a pointer of nothing; and an Ogg Vorbis, Opus
  or FLAC stream is given an empty comment packet — `ogg::bare`. A tag that claims more
  than the file holds, or tags that would leave no frames at all, leave the file whole.

**Where the speakers sit is part of what is kept.** `MediaInfo::speakers` is the source's
positions as symphonia reads them — its `Position` bits, which are the WAVE channel mask — and
`Form::placing` weighs them after `Form::of`: FLAC has a fixed assignment per channel count and
no room for a mask in an object that carries STREAMINFO alone, so a source naming positions FLAC
would read back as others — a 2.1 whose LFE FLAC calls a centre, a 5.1 on the sides FLAC calls
the rear — goes to `Wave`, which writes `WAVE_FORMAT_EXTENSIBLE` with the source's mask wherever
it is not the order a plain header already implies, and a mask past the eighteen WAVE names is
`Kept`. Validation weighs the positions read back beside the digest and the frame count, through
`Heard::held_by`, so an object whose speakers moved is refused rather than landed; a source that
names none is held by any reading.

**Three of those bounds are flacenc's rather than FLAC's.** The format holds 32 bits and 655 kHz;
`flacenc` 0.5.1 verifies `sample_rate <= 96_000` and `bits_per_sample <= 24`, and its Rice
parameter stops at 14 where the format's second partition method reaches 30. The last one is what
decides the shape of a real library: measured on this material, flacenc **beats** `flac -8` at
16/44.1 — 23,597,708 bytes against 24,829,499 — and loses by some 15 % at 24/48, because 24-bit
residuals need Rice parameters a 4-bit field cannot name. So 16-bit rips are re-encoded and
24-bit ones are kept and stripped, and the size comparison arrives at that on its own without a
rule naming depths.

## Keys and layout

```
<root>/audio/ab/abcdef…7f.flac        # or .wav.zst, or .mp3, .dsf, … for a kept object
<root>/covers/3c/3cd1…9a.jxl
<root>/staging/<pid>-<n>.<ext>
```

A `VaultKey` is sixteen bytes written as thirty-two lowercase hex letters, fanned out two deep.
For `Flac` and `Wave` it is the MD5 of the decoded interleaved PCM — the same digest FLAC carries
in STREAMINFO, so `metaflac` can be asked the same question — and for `Kept` and for a cover it is
the MD5 of the bytes stored. Writes go to `staging` and are `sync_all`'d and renamed into place,
the way `config::write` and `organise`'s staged sheet already are, so a crash mid-write leaves a
staging file and never a half-written object; `Vault::sweep_the_staging` is what `--prune` clears
them with.

**A staging file is a `Staged`, and dropping one takes the file away.** Every write that can
fail between `File::create` and the rename — a source read, a full disc, a `sync_all`, a refused
landing — returns through `?` and leaves nothing behind, so an import that errors a thousand times
leaves no partial copies for `--prune` to find. `keep`'s fallback to `Kept` discards the object it
replaces *before* it tries the fallback, so a fallback that errors cannot orphan it in `audio/`.

**The catalog names an object by where it sits inside the vault, never by where the vault sits.**
`Vault::open` canonicalises its root, `Vault::within` turns a path the vault answered with into
the relative one `vault_path`, `vault_objects.path` and `cover_path` hold — `audio/ab/ab…7f.flac`
— and `Vault::at` turns it back, refusing anything that is not a run of plain names, so no stored
value can reach outside the root. A vault opened under `./vault`, through a symlink or after it was
moved is therefore the same vault to the catalog, the stand-in and the prune alike.

**Validation happens on the staging file, before the rename.** A dedup hit is therefore never at
risk from a failed import of the same audio, and a refused object never reaches `audio/` at all.

## An object is weighed again when the encoder moves

`Encoding::OF_THIS_BUILD` names what this build's encoders are, and every `vault_objects` row
carries the one it was weighed under. **It is bumped by hand whenever what an import would write
changes** — a new `flacenc`, another zstd level, a bound `Form::of` draws differently — because
nothing else can tell that an object on disc was made by a worse encoder than the one now linked.

`--import` walks a vaulted row again wherever its object's stamp is behind, and leaves out the
two it cannot improve: a row whose source has gone, since the object is then the only copy and
nothing better can be made of it, and a row `Form::of` would keep as it stands whatever encoder is
behind it — a lossy codec, DSD, more than eight channels — unless it is MP3, AAC or DSD, whose
kept copies encoding 2 began stripping, or Vorbis or Opus, whose encoding 3 did; an MP4's AAC, a
DSDIFF or a Vorbis in Matroska among those is copied again to the same key and stamped. Encoding 4
began stripping an Ogg FLAC, which `Form::of` never keeps and so is walked again regardless. The
preview marks such a row as *weighed again*. A renewal is a `Taking` with `renewing` set, and what it changes is the one rule
that would otherwise hide the new encode: an object already standing under the same key is not a
dedup hit but a rival, and the new one replaces it — `Kept::replaced`, a rename over the standing
file — only where it comes out smaller. A renewal that loses keeps the standing object and stamps
it current, because it has now been weighed against this build. `keep` never discards an object
it replaced, so the rows the same audio already names keep their object whatever this row goes
on to decide; a renewal that settles on another form or key leaves the old object to `--prune`.

A plain dedup hit is stamped `Encoding::UNRECORDED` where the catalog has no row for its object —
the vault standing from before the catalog was deleted and scanned again — so what was made under
an encoder nobody wrote down is weighed again on the next import rather than trusted.

## Covers

A cover is decoded with `image`, encoded as lossless JXL with `zune-jpegxl` at its highest effort,
and read back with `jxl-oxide` and compared pixel for pixel before it lands — lossless means an
exact compare is the real check rather than a proxy for one. An opaque picture is written as three
channels and one with transparency as four, so the common case does not carry an alpha plane it
does not need.

**The vault decodes JXL back to PNG, and that is why `resonate-ui` did not have to change.**
`Vault::picture` answers a `CoverArt { format: Png, bytes }`, which is exactly the type every
caller already takes, so the window keeps handing gpui encoded bytes plus a `gpui::ImageFormat`
and `mpris::art::Pictures` keeps writing a file a notification daemon can draw. `dependencies.md`
forbids an image crate in `resonate-ui`, and a JXL a notification daemon cannot read would have
broken `mpris:artUrl`; one decode in the crate that owns the format answers both.

**A cover is drawn once a run.** The PNG `Vault::picture` answers is written at `image`'s fast
compression, because it is a transient form rather than something kept, and `Drawings` holds it
under the cover's key — the least lately asked for going first past `DRAWN_BYTES_AT_MOST` — so the
window and `retag` asking for one album's cover again cost a lookup rather than a JXL
decode and a PNG encode. `Vault::forget` takes a drawing with the file, so a pruned cover is not
answered from memory.

## What the catalog holds

`tracks.vault_key` and `tracks.vault_path`, `albums.cover_key` and `albums.cover_path`,
`cover_source` gaining `Vault`, and one `vault_objects` table. All of it is edited into `V1` where
it stands, so `SCHEMA_FINGERPRINT` moved and a catalog written before it was scanned again — the
rule of the day, which `library.md`'s migration steps have since replaced.

- **A vaulted row is still named by its own file, and the vault stands in for it only when bytes
  are wanted.** `tracks.path` and `span_start` are the row's identity everywhere — a `Track`'s
  location, a playlist's `Cut`, a counted play, a share, a resumption — so a vaulted track counts
  its plays, exports as the file it came from and resumes as the cut it was. What the player opens
  is decided at the `Sources` it opens through: `Library::stand_in` is a
  `resonate_codec::StandIn`, and the binary registers it on the player's sources with
  `Sources::standing_in`. `Decoder::open`, `Decoder::open_span`, `probe` and `probe_span` ask it
  first, and where the catalog names a `vault_path` for that `(path, span_start)` they decode the
  object *whole* — a cue row's object is that row alone, so the span is not applied twice — under
  a `TagSet` the catalog fills: the names, the numbers, the MusicBrainz ids, `rg_*` as the
  ReplayGain the engine resolves its gain from, and `tracks.lyrics`, which the scan keeps for
  exactly this. An object that will not open falls back to the row's own file. `resonate info`,
  the scan and the import open through sources with no stand-in, so they always read the file.
- **`vault_path` is denormalised beside the key on purpose.** The stand-in reads it by the row's
  own unique key, so resolving an open is one indexed read rather than a join through
  `vault_objects`.
- **A vaulted row outlives the file it came from, and a file that changed forgets its object.**
  The prune and the tidy of the roots beside a scan leave a vaulted row whose file has gone, because
  the vault then holds the only copy and a pruned row is what `--prune` would have taken the object
  away for; a vaulted row the walk did not see while its file is still there was superseded — a
  sheet that no longer cuts it — and goes as any row would. The upsert clears `vault_key` and
  `vault_path` wherever the size, the mtime or the span moved, so a file ripped again where it
  stood is weighed again by the next `--import` rather than played from the old object.
- **`Library::prune_the_vault` is what `--prune` runs, and it weighs a cover by its key.** It
  takes the `Walk` guard, so it cannot take away an object an import has landed and not yet noted,
  removes the audio objects no row names and their `vault_objects` rows, then every JXL under
  `covers/` whose key no album's `cover_key` holds, then the staging folder. A cover is matched by
  key rather than by path, so no spelling of the root can make a named cover look loose.
- **`Library::import` takes the `Walk` guard**, because it reads every source file and must not run
  beside a scan, an organise or a retag. A second caller gets `Error::AlreadyWalking`.
- **An import runs on `ImportOptions::workers` threads**, the machine's parallelism by default,
  because the encode is the cost and it is per track. The workers draw the next row off one
  shared counter — over `costliest_first`, which claims a WAVE-bound row before a FLAC-bound one
  and a kept one last, the larger file first within each, so the one slow encode starts at once
  rather than running alone after everything else has finished — and write their own catalog
  rows, and an album's cover is kept by whichever worker claims the album first in a shared set;
  the plan is put back into the order the rows were asked for before it is handed out, so a
  preview and an apply list alike. `Vault` is what
  makes it sound: `landed_or_standing` decides and renames under one lock, so two rows of the
  same audio settle on one object, one `Landed` and the other `Deduped`. The first error stops
  every worker at its next row and is the pass's answer; a worker that panics is
  `Error::Stopped` naming the import.
- **A row leaves the vault only where its own file is still there.** `Library::release_from_vault`
  is `resonate vault --release`, under the same `--root` narrowing the import takes and the same
  `Walk` guard: it clears `vault_key` and `vault_path` on every vaulted row whose `tracks.path`
  still exists, so the stand-in stops answering and the player opens the file again, and it
  counts the rest as `Released::stranded` and leaves them alone, because the vault holds the only
  copy of each and releasing one would hand `--prune` the audio. The objects a release leaves
  are named by nothing and wait for `--prune`, the one gesture that deletes; the next `--import`
  weighs a released row again like any other. Covers stay where they are — an album's picture
  moved into the vault had `cover_art` cleared in the same statement, so there is nothing to
  point back at.
- **`retag` passes a vaulted row over** — `Unwritten::Vaulted` — because writing tags into a file
  nothing reads any more is work for nothing. **`organise` does not**: `tracks.path` still names
  the original, the original is still the user's library, and the vault is keyed by content, so
  filing it moves nothing the vault depends on.
- **An album holds a cover where it holds either column.** `ALBUM_COLUMNS` and `asking_albums`
  read `cover_art IS NOT NULL OR cover_path IS NOT NULL`, so a vaulted cover is warmed by the
  window and spares the enrichment an archive fetch `land_archive_cover` would only throw away.
  `gather`'s fill takes the loser's whole cover — art, format, source, key and path together —
  only where the survivor holds neither, so a gathered album never holds both and never names a
  source without the picture; it keeps the earlier of the two `favourite` stamps the same way.
- **A cover moves into the vault once per album**, and `enriched::vault_the_cover` clears
  `cover_art` in the same statement that writes `cover_path`, so an album never holds both.
  `land_archive_cover` grew `AND cover_path IS NULL`, and so did the scan's `store::cover`, which
  counts an album the vault covers as covered, so neither an archive cover nor a rescanned file's
  can land on an album the vault already holds a cover for. The import asks
  `cover_the_vault_lacks` rather than `cover_art`, which would hand back the vault's own PNG to be
  kept again under another digest, and a cover `image` cannot read is a warning and a
  `covers_passed` count rather than the end of the import.

## Reading an object back

`VaultFiles` is a `MediaProvider` registered under `SourceId::local()`, so it *replaces*
`LocalFiles` rather than standing beside it: a path under the vault root ending in `.zst` is
handed over as an `Unpacking` with the inner extension as its `FormatHint`, and everything else
is the plain file open `LocalFiles` always did. `Unpacking` decodes the stream as it is read
rather than decompressing the object whole into a `Cursor`, which held a five-minute 24/192
object — some 460 MB — resident for every open, the catalog's tag probe of each queued row
included, and two of them for a probe beside a play. It reads its length out of the `RIFF`
header this build wrote, seeks forward by decoding and throwing away, and seeks backward by
starting the stream again, so a probe costs the header and a play costs one pass, and only a
seek back pays for the stretch before it. It is what opens
the object the stand-in names, and the binary registers it wherever it builds a `Sources`: the
player, `resonate info` and `resonate explain`. The bus, the playlists, the resumption and the
queue never see an object's path at all, because a row is named by its own file.

## What a provider delivers

`Library::poll` hands a provider's `Delivery::File` to `Vault::keep` as a local location and a
`Delivery::Stream` to `Vault::keep_delivered`, which copies the reader into
`staging/<pid>-<n>.<ext>` through a `take` one byte past `LARGEST_DELIVERY` — the RIFF ceiling —
`sync_all`s it, keeps it the way any local file is kept and discards the staging file whatever
came of it. A stream past the cap is `Refusal::TooLarge` and never decoded. The extension is
filtered to its ASCII letters and digits the way `named_extension` filters a location's, so a
delivered `../flac` stages as `flac` under `staging/` and nowhere else. The poll writes the
`vault_objects` row, a `tracks` row with no root named by the object's own path and paired with
the release track the want was for, and records `wants.offered` as the *vault* object's URI
rather than the provider's, because that is where the bytes now are. `providers.md` has why the
row belongs to no root.
`a_delivered_file_lands_in_the_vault_and_the_want_names_where_it_went` and
`a_streamed_delivery_lands_in_the_vault_and_leaves_nothing_in_staging` are the claims.

## An Ogg stream's comments are rewritten, and every page after them numbered again

A Vorbis or Opus stream keeps its tags in a header packet of its own — the comment packet after
the identification header, and for Vorbis the setup packet after that — so stripping one is a
rewrite of the stream's structure rather than a cut around it. `ogg::bare` reads the first page,
which both specifications give to the identification header alone, and names the codec by that
packet's magic; walks the pages of the same serial, in sequence, until the header packets are
whole; and refuses — copying the file as it stands — a stream whose header pages carry another
serial, skip a sequence number, run past `HEADER_BYTES_AT_MOST`, or end the last header packet
anywhere but at the end of its page, since the first audio packet must begin on a page of its
own. The comment packet is rewritten with its vendor string and nothing after it — a count of
nothing, and Vorbis's framing bit — and a stream whose packet is already that is copied as it
stands.

**An Ogg FLAC stream is the same walk over metadata blocks.** Its first packet is the 51 bytes of
`\x7fFLAC`, the mapping's version, a count of the header packets that follow and the STREAMINFO
block, and each header packet after it is one native metadata block, the VORBIS_COMMENT first as
the mapping requires. There is no fixed count to walk to — the count may be zero for *unknown* —
so the headers are whole at the block marked last, and one whose STREAMINFO is itself marked last
has none to shed. What is kept is one VORBIS_COMMENT with its vendor alone, marked last, so the
PICTURE, the PADDING, the SEEKTABLE and the CUESHEET go the way a native FLAC's do, and a stream
whose first header is not its comment is copied as it stands. The first page then says one header
follows and is stamped again — unless it counted none, which is left saying so.

The first page is otherwise kept byte for byte; the new comment packet and the setup packet are laid
out on fresh pages under the next sequence numbers, a granule of nothing on a page a packet ends
on and of `NO_PACKET_ENDS` on one none does, each stamped with the CRC-32 the format names — the
polynomial `04c11db7`, unreflected, over the page with its checksum field zeroed, which
`a_page_is_stamped_with_the_checksum_libogg_gives_it` holds to a page ffmpeg wrote. A picture in
the comments usually made them several pages long, so the headers now take fewer pages than they
did and every audio page after them would skip numbers: `Renumbering` is the difference, and
`Renumbered` wraps the copy that follows, reading it a page at a time and rewriting the sequence
and the checksum of every page of that serial, passing another serial's pages — a chained stream's
next link — through as they are, and passing whatever does not parse as a page through verbatim
from there on. Validation is what makes the last two safe: `kept_whole` decodes the copy and
weighs it against the source, and a copy that does not hold the same audio is copied again whole.
`a_kept_ogg_vorbis_sheds_its_comments_and_keeps_every_packet_it_decodes_to`, its Opus twin and
`a_kept_ogg_flac_sheds_its_comment_and_keeps_every_frame_it_decodes_to` — a 24-bit, 192 kHz
stream whose zstd'd WAVE does not beat it, which is how a FLAC is ever kept — are the claims,
each checking the object's sequence numbers and checksums with a CRC written bit by bit rather
than through the table the vault uses.

## What it does not do

- A `Form::Kept` object in a container whose tags sit *inside* its structure — MP4's `udta`,
  DSDIFF's `ID3 ` and `DIIN` chunks, Matroska's `Tags` — keeps the tags its container was written
  with, because stripping those means a writer per format.
