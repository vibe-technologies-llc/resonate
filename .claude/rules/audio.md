---
paths:
  - "crates/resonate-engine/**/*.rs"
  - "crates/resonate-codec/**/*.rs"
  - "crates/resonate-dsp/**/*.rs"
  - "crates/resonate-pipewire/**/*.rs"
---

# The audio path

Invariants from the file to the sink. `realtime.md` covers the callback contract itself.

## Decode

- **A source's depth is read from the container, never guessed from the decoder's buffer.**
  symphonia leaves `bits_per_sample` unset for ALAC, so `container.rs` takes the depth out of the
  magic cookie in `extra_data`. Without it a 24-bit ALAC claims 32 bits, and `plan_output` asks the
  sink for a format the file never had — a converted path where the same music in FLAC stays
  bit-perfect. `declared_bits` is that question asked of every container that answers it, in order
  of how near the answer is to the codec: what symphonia declares as the coded width, then a WAV's
  `wValidBitsPerSample`, then FLAC's `STREAMINFO`, then Matroska's `BitDepth`, and only then
  `bits_per_sample` — which is the *decoded* width for most readers and the reason the chain exists.
  `wValidBitsPerSample` is what a `WAVE_FORMAT_EXTENSIBLE` file uses to say it holds 20 bits in
  24-bit words, and symphonia reports the 24; the depth a file was made at is what the inspector
  draws and what `resonate info` prints, so reading the padding instead is a lie about the master.
- **A layout is named only where the container places every channel where that layout does.**
  `positioned_layout` weighs symphonia's speaker mask against the named layouts — the side pair
  and the rear pair each count as the surrounds of a 5.1 or a quad — and anything else, a 6.0 or
  an LCRS, is `ChannelLayout::Discrete` rather than whichever layout shares its count, because a
  downmix reads a 5.1's fourth channel as the LFE and drops it. One and two channels are mono and
  stereo whatever the container placed them at — symphonia places a mono MP3 on the front left —
  and an unplaced set of more is `Discrete`.
- **A tag is read under the naming its writer used, not only the one the container's spec defines.**
  symphonia maps a raw key through the container's own vocabulary, so ffmpeg's Matroska — which
  writes `ALBUM`, `ALBUM_ARTIST`, `DATE` and `PART_NUMBER` at the album target where the spec says
  `TITLE` and `ARTIST` — reached the `TagSet` through nothing and filed an `.mka` under its file
  stem. `tags::Naming` decides from the key set which vocabulary is in play: one name the spec
  never defines means Vorbis comments, and then `ARTIST` is the track's rather than the album's and
  the names symphonia declined are read too. `VORBIS_COMMENT_ONLY` is that evidence and holds only
  names no container spec also defines — `ORIGINALDATE`, `TOTALTRACKS`, `COMPILATION`,
  `MUSICBRAINZ_TRACKID` and `_ALBUMID` beside `MUSICBRAINZ_ARTISTID`, `_ALBUMARTISTID`,
  `_RELEASEGROUPID` and `_RELEASETRACKID`, the `REPLAYGAIN_TRACK_`/`_ALBUM_` spellings, twenty-eight
  in all — because one name is enough to flip a whole revision. `DATE`, `BPM`, `ISRC`, `LABEL`, `PUBLISHER`, `LYRICS` and `DESCRIPTION` are
  deliberately not among them: Matroska defines each, so reading a WAV's `INFO` list beside a `DATE`
  as Vorbis comments would file the wrong vocabulary off one ambiguous key. What a reader never
  exposes at all is taken off the source before it is handed away — `riff.rs` for a WAV's `INFO`
  list, `matroska.rs` for the segment's `Title`, which is where ffmpeg puts a `-metadata title` —
  and `Prescan` is the one bundle that
  carries both, so a growing argument list does not follow every container that needs one. A segment
  names the file rather than a track, so its `Title` fills one only where the container holds a
  single audio track: ffmpeg writes a `.mka`'s title there and nowhere else, so for one track it is
  the whole of the title and for two it would otherwise have claimed to be both of theirs.
  symphonia's `wav` feature does turn on `symphonia-metadata/riff-info`, and `WavReader::try_new`
  does read the `INFO` list — into a log of its own, which it then throws away, building itself with
  the caller's external metadata instead. Not one of those tags reaches a caller, so the prescan is
  what rescues them rather than a second pass over what symphonia already published.
  **A WAV's `id3 ` chunk is read the same way, because symphonia's reader skips it.** A leading
  ID3v2 tag in front of the `RIFF` header is symphonia's probe's to read; one inside the file as an
  `id3 ` or `ID3 ` chunk — where lofty, and most taggers, write it — is not looked at by
  `WavReader` at all. `riff.rs` holds the chunk's bytes whole, up to `MAX_ID3_CHUNK_BYTES`,
  `tags::read_id3_chunk` hands them to symphonia's own `Id3v2Reader`, and the revision it answers
  is appended *last* to `Revisions`, so it outranks the `INFO` list and a leading tag alike.
  Its pictures ride on `Coded::chunk_pictures` and are weighed before the reader's, which is what
  lets a cover written into a WAV be read back.
  `standard_info` reads every `INFO` id that lands on a field the `TagSet` has, under each of the
  names writers use for it and whatever case it was written in. `IDIT` and `DTIM` are deliberately
  not among them: they date the digitisation rather than the release, so reading one would put the
  day a record was ripped where its year belongs.
- **What the `TagSet` holds is the vocabulary, and a name outside it stays a `RawTag`.** Every
  well-known name a music file carries has a typed field: `credits` for the people a recording names
  — composer, conductor, lyricist, performer, remixer, engineer, producer — beside `grouping`,
  `collection`, `edition`, `label`, `isrc`, `beats_per_minute`, `copyright`, `encoder` and
  `comment`, and the six a tagger writes about a release: `musicbrainz_track_id` and
  `musicbrainz_album_id` beside `musicbrainz_artist_id`, `musicbrainz_album_artist_id`,
  `musicbrainz_release_group_id` and `musicbrainz_release_track_id`, with `barcode` and
  `catalog_number` — which a cue sheet's `CATALOG` line fills too, because that is what the line
  is. The three naming paths all land on them, so `IENG`, `IMUS`, `IWRI`, `IPRO`, `ICOP`,
  `ISFT` and `ICMT` out of a WAV's `INFO` list, a Vorbis name a container's reader declined, and
  Matroska's `COLLECTION` and `EDITION` targets above the album all reach the same field. The
  inspector then reads a field rather than a key and MPRIS fills `xesam:composer`, `xesam:lyricist`,
  `xesam:comment` and `xesam:audioBPM` from it. A name the writer invented has no typed home by
  definition: it reaches `read_raw` and stops, which is where it belongs rather than a gap to close.
  Three ids are deliberately left out. RIFF's `ISRC` names the *source* a recording came from and
  not the recording code Vorbis and Matroska write under that name, so reading it would file a
  vendor where an identifier belongs. `EncodedBy` is who encoded rather than what encoded, so
  `encoder` takes `Encoder` alone — and `ISFT`, which is the software that wrote the file. And RIFF
  `INFO` defines no album artist at all, so a WAV tagged through `INFO` alone leaves `album_artist`
  empty whatever is read; `store::attribution` falling back to the track artist is the whole of the
  answer, and a compilation tagged that way still splits.
- **A text tag that is blank once trimmed is no tag, and what is stored is trimmed.** `tags::given`
  is the one door every text-valued `StandardTag` goes through on its way into a `TagSet` slot —
  the names, the credits, the six MusicBrainz ids, the barcode, the catalogue number, the lyrics —
  and it trims the value and leaves the slot as it was where nothing is left, so `"   "` names
  nothing, `"  Echoes \n"` is stored as *Echoes*, a blank frame written after a name does not
  unname it and a later name still wins, which
  `a_blank_text_tag_is_no_tag_and_a_padded_one_is_stored_trimmed` is the claim of. A tagger that
  writes an empty frame rather than omitting it otherwise files a track under an artist with no
  name and an album with no title, and `stem.rs` never runs for a title that is *there*; read as
  none, the same file is named from its stem and grouped as the tags that are not blank say.
- **An ID3 recording id arrives as a frame nobody standardised into a tag.** A tagger writes it in
  `UFID`, whose owner names the database it belongs to, and symphonia reads that frame as a
  `RawTag` keyed `UFID` with an `OWNER` sub-field and a *binary* value, mapping it to no
  `StandardTag` — so the `MusicBrainzRecordingId` arm was reached from the Vorbis and MP4
  spellings alone and an MP3 tagged by Picard carried no recording id at all.
  `musicbrainz_identifier` is what reads it: the key, then the owner against `MUSICBRAINZ_OWNER`,
  then the bytes as UTF-8, which is what MusicBrainz writes there. It is tried only where
  `Naming::standard` answered nothing, so a frame a reader already mapped is never read twice.
  The owner is the whole of the check, because `UFID` is also where CDDB and every other database
  puts its own id, and none of those names a recording.
- **A Matroska file's length is the segment's, not the track's, and it is counted as well as read.**
  symphonia leaves both `num_frames` and `duration` unset for mkv, and `rebind` seeks even to frame
  zero while a seek needs a length to range-check against, so an `.mka` could not be loaded at all.
  `matroska.rs` reads the `Duration` and `TimestampScale` elements out of the same `Info` element it
  takes the title from, and `rebind` does not seek to where the decoder already is. It also walks
  the clusters, taking each one's `Timestamp` and the furthest `SimpleBlock`/`Block` offset inside
  it, so a file that declares no `Duration` at all — which is every file a muxer streamed rather
  than seeked back to finish, and what `ffmpeg -f matroska -` writes — still has a length and
  therefore a seek bar. The count is deliberately a *lower* bound: it is where the last block
  starts, so it is short by that block's own length and never long. `Segment::duration` keeps the
  declaration wherever the clusters stay within it, because the declaration is exact and the count
  is not, and prefers the count only where blocks exist past the declared end — the one case the
  writer is provably wrong. A declaration that is too *long* is not detectable this way and is
  left alone, except for Opus, whose packets are counted exactly — see below. The walk is what `Prescan::buffered` paid for: over an hour-long `.mka` of about
  10^5 blocks it costs 440 ms a probe read four bytes at a time and 15 ms read through the window,
  so the case the writer is provably wrong stays caught for about a millisecond on a track-length
  file rather than being traded away for speed.
- **A seek lands on the exact frame only where the track timebase is the reciprocal of the sample
  rate.** Matroska ticks are milliseconds, so its landing is converted and approximate, and a format
  whose timebase is not the sample rate's reciprocal lands the same way.
- **Opus is decoded by `opus-rs`, registered beside symphonia's own decoders.** symphonia 0.6
  demuxes Opus out of Ogg, Matroska and MP4 and decodes none of it, so `opus.rs` wraps `opus-rs`
  — a pure-Rust port of libopus 1.6 — as an `AudioDecoder` and `opus::codecs` is a `CodecRegistry`
  holding `register_enabled_codecs` and it, which is what `Decoder::build` makes every decoder
  from. `Head` reads the identification header out of the track's extra data: Ogg's and
  Matroska's `OpusHead` little-endian, MP4's `dOps` big-endian under the magic symphonia puts in
  front of it, told apart by the version byte, which is 0 only in `dOps`. Mapping family 0 is one
  decoder of one or two channels; family 1 is `opus-rs`'s multistream decoder, whose channels come
  out in Vorbis order and are written to the plane each position takes in symphonia's bit order,
  so a 5.1's centre is not heard from the right. The header's output gain is applied as the
  samples are written, as RFC 7845 asks, and `R128_TRACK_GAIN` and `R128_ALBUM_GAIN` — a Q7.8
  gain against −23 LUFS, relative to that output gain — are read as ReplayGain 5 dB louder,
  because ReplayGain 2.0 references −18. A stereo decoder refuses a mono packet, which libopus
  writes into a stereo stream at a low enough bitrate, so a second decoder of the packet's own
  width answers it and its samples are laid across every channel.
  **Where the pre-skip lands is the container's to say, and each says it differently.** Ogg names
  it as the track's delay but counts it *inside* its granules, so `num_frames` holds the pre-skip
  and the music starts at `start_ts` plus it rather than at zero; Matroska names nothing and
  shifts its timestamps by `CodecDelay` instead, so the pre-skip is read off the head and the music
  starts at zero; MP4 names nothing either and its edit list is what the box scan already reads.
  `container::Carrying` is that distinction, and `priming` and `music_at` both read it.
  **Matroska's end is its last block's `DiscardPadding`**, which symphonia reads nowhere: the
  prescan's cluster walk reads each `BlockGroup`'s children and keeps the padding of whichever
  block came last, and `Segment::discarded_frames` is the priming's padding. A padding with no
  end to cut it from — `playable` open-ended, which is all Matroska can say, its timestamps being
  milliseconds — is what `Decoder::build` hands `Coded::trailing`: `fill` reads one packet ahead
  while it is non-zero and takes the padding off the packet that turns out to be the last, so the
  decode is exactly as long as what went in. **The declared length is counted, not read off the
  timestamps.** Every Opus packet says how long it is in its TOC byte — the frame size its config
  names times the frame count its code names, the count byte after it for code 3 — and the cluster
  walk reads that byte out of each block of the track whose `CodecID` is `A_OPUS`, so
  `Segment::opus_samples` is the whole stream in 48 kHz samples and `Segment::opus_music` less the
  pre-skip and the padding is `playable`'s end: the window is closed and the declared length is
  exactly what decodes. The count is trusted only where nothing escaped it — a laced block, a
  packet naming no frames or more than 120 ms, a cluster or a segment the walk did not reach the
  end of, a segment of unknown length — and any of those leaves it `None`, the window open-ended,
  and the length the segment's millisecond count less the pre-skip it includes,
  `Carrying::declared_before_the_music`, within three half-millisecond roundings. Reaching the
  segment's own end is what keeps a source that cannot seek honest: its prescan sees only
  `MAX_PRESCAN_HEAD` bytes, and a count cut short there would close the window on the music.
  **A surround stream Matroska leaves unplaced is placed by its head.** Matroska carries a channel
  count and no mask, so a 5.1 Opus track opened as `Discrete(6)`, which a downmix truncates one
  channel for one; mapping family 1 *is* the Vorbis order, so `opus::channels_of` answers those
  positions wherever the container named none and the head says family 1, and the layout, the
  speakers and the decoder's planes all read it. Family 255 names no order and stays discrete.
  `a_seek_into_opus_hears_what_decoding_from_the_start_hears_there` and
  `opus_decodes_to_what_libopus_decodes_it_to_in_every_container_that_carries_it` are the claims:
  in CELT, which is every music bitrate, the decode is within 10⁻⁵ RMS of full scale of ffmpeg's
  libopus in stereo and 10⁻⁴ in 5.1, and exactly as long as what went in. **A seek starts 400 ms
  early**, because CELT predicts each band's energy from the frame before it — at α = 0.5 for a
  20 ms frame — so a decoder started cold on the target frame is still halving its error 20 ms at
  a time; RFC 7845's 80 ms leaves a sixteenth of it, and `opus::pre_roll` is what `seek_reader`
  subtracts. The frames it lands early on are decoded and discarded like any other skip.
- **This build drops every priming itself, and asks nobody else to.** `Decoder::build` makes its
  decoder with `AudioDecoderOptions::gapless(false)`, so symphonia's decoders emit whole blocks and
  the only trimming anywhere is `MediaInfo::playable`. That is one rule rather than a list, and the
  list is what it replaces: `gapless` defaults to *on* but only `symphonia-bundle-mp3` and
  `symphonia-codec-vorbis` implement it, so who trimmed what was a fact about the codec crate and had
  to be kept in step with the symphonia version. Turning it off is also what stops a priming coming
  off twice now that a reader's own declaration fills `playable`.
- **A priming reaches `playable` from whoever named it — the reader or `boxes.rs`.**
  `container::priming` prefers `Track::delay`, `Track::padding` and `Track::num_frames`, which is
  what CAF reads out of its `pakt` chunk, mp3 out of the LAME tag and ogg off its first and last
  pages, and falls back to the box scan where the reader named nothing. Its isomp4 reader names
  nothing, which is why the scan exists: it reads the `iTunSMPB` free-form item under
  `moov/udta/meta/ilst` where a store wrote one, otherwise the first `elst` entry that is not an empty
  edit, its segment rescaled from the movie timescale to the media one, and weighs it against what
  the `stts` table says the decoder will emit, which is the sample count times the longest delta
  rather than the sum: the last sample's declared duration is short by exactly the padding. It answers
  for the `soun` track alone and only where the media timescale is the sample rate, because that is
  what makes a media tick a sample frame.
- **A fragmented MP4's length is in its fragments, not its `moov`.** A DASH-style file — `ftyp`
  `iso8`/`dash`, a `moov` whose `mvhd`, `mdhd` and sample tables are all empty, and the audio in
  `moof`/`mdat` pairs — declares a length of nothing, and symphonia hands that nothing back, so the
  seek bar and the catalog read 0:00 while it played whole. The box scan that finds the priming
  also reads the length such a file carries: `mvex/mehd`'s fragment duration in the movie
  timescale, or where there is none the summed subsegment durations of the first top-level `sidx`
  in its own timescale, rescaled to the stream's rate. `coded_info` reads a declared length of
  zero as no length at all and takes the fragmented one before the Matroska segment's, which is
  `Movie::fragmented_length`; it is what put an 8:37 FLAC-in-MP4 at 8:37 rather than 0:00. A
  catalog that scanned such a file before holds its zero until the file is read again, because
  an incremental scan passes over a file whose size and mtime have not moved — which is what the
  Library category's *Read every file again* is for.
- **`playable` is decoded frames and it may be open-ended.** `Decoder::build` drops
  `playable.start()` decoded frames before the first block and takes `playable.frames()` as its
  limit, so the priming and the padding both go, and `duration` is the playable count, which is what
  the seek bar, `mpris:length` and `heard.rs` have always read. `Priming` holds the count as an
  `Option`, because a reader can name a delay and not a length — an ogg over a pipe reaches no end
  bound, and a Xing header can carry a LAME delay and no frame count — and the priming still has to
  go where the end cannot be said.
- **`Timeline` is told where the music starts, not how long the priming is.** It holds one
  `music_at: Timestamp` and maps a playable frame to a container timestamp by adding to it, which is
  the only arithmetic a seek needs. A reader that named the delay has already put the music at
  timestamp zero — symphonia's convention is a negative PTS for an encoder delay frame — so
  `music_at` is `Timestamp::ZERO` for it; one that did not gets `Track::start_ts` plus the scanned
  priming. Carrying `start_ts` and the priming as two fields instead had ogg wrong both ways round,
  because ogg is the one reader that *does* set `start_ts` to `-delay`: a seek landed where it was
  asked and then decoded 896 frames short of the rest of the track. A seek can land *ahead* of
  `music_at` — back to the start, or anywhere inside Opus's pre-roll — and a frame count cannot
  say how far, so `seek_reader` answers a `Landing` carrying `short_of_the_music` beside the frame
  and `restart` drops that much before it counts; saturating it to frame zero played the priming
  as music and heard the rest of the track that many frames late.
- **Every revision of a file's metadata is read, not only the newest.** symphonia's
  `Metadata::skip_to_latest` discards the revisions it walks past, and a container publishes more
  than one as a matter of course: Matroska pushes one per `Tags` element, isomp4 one for a top-level
  `meta` atom and one for `moov`, AIFF one for its text chunks and one for its ID3. `tags::Revisions`
  drains the log once in `container::open` and hands the same owned set to `tags::read` and
  `tags::read_raw`, oldest first, so the newest still wins a tag two of them name and the rest are no
  longer thrown away. Each revision is absorbed on its own, so `Naming` weighs one writer's key set
  rather than every writer's flattened together. Only the tags are taken: the visuals stay in the
  reader's log for `probe_cover_art`, which is what keeps a cover out of the clone.
- **A picture is found once and copied once, and who copies it is the caller's to say.**
  `probe::picture` is the one walk — the attachments, then the newest revision's visuals, front
  cover first — and it hands what it found to a `taken` of the caller's choosing, so
  `probe_cover_art` copies the bytes and `Picturing::Whether` answers whether there is one
  without touching them.
  The `MAX_COVER_BYTES` weighing stays inside the walk rather than moving into `taken`, because a
  picture declined for being absurd has to be declined the same way whichever question was asked.
  **`probe_pictured` is the one open that answers a file's tags and its picture together**, and
  the caller's `Picturing` says how much of the picture: `Whether` answers `Pictured::Carried` or
  `Bare` and copies nothing, `Copied` answers the bytes as `Pictured::Copied`, and a row the
  sources stand in for answers `StoodIn`, because the vault's object carries no picture and the
  row's own file is not what was opened. The library scan reads every file through `Whether` —
  `library.md` has why the bytes are deliberately left to be read later — and so does
  `resonate tag`'s plan, which only ever wanted to know whether a file already carries a cover;
  its read-back asks `Copied`, so a written picture is weighed off the same open as the fields.
  `TagSource::read` takes the `Picturing` and answers a `Tagged`, so no implementation can answer
  the two through separate opens.
- **The engine's tag reader reads a picture on the open its tags came off wherever it can.** A
  queued row no scan has seen is asked for its name and, where the window draws it, its cover;
  the two used to be two opens of one file. `catalog::whole` probes a whole-file row under
  `Copied` where its picture is already waited on and under `Whether` otherwise, and
  `Shelf::keep_what_the_tags_saw` files what it learned: the bytes where it copied them, and
  `Look::Nothing` where the file carries no picture, so a later look for one answers at once
  rather than opening the file again. A file that carries one nobody has asked for is not copied
  — the bytes stay out of `ART_BYTES_HELD` until something draws them — and an art ask that
  arrives after the tags settled it is skipped rather than read again. A row cut out of a file by
  a sheet is read through `probe_span` and says nothing about the picture, because the picture is
  the file's rather than the row's and the whole-file row will be asked on its own.
- **A picture is weighed before it is copied.** symphonia has already read a visual into its own
  buffer by the time `probe_cover_art` sees it, so the `to_vec` beside it is a second copy of
  whatever the file embedded — and the engine's `ART_BYTES_HELD` bounds what the catalog *holds*,
  long after the allocation. `MAX_COVER_BYTES` is checked against `data.len()` before the copy, and
  an oversized visual is declined rather than failing the read: `choose` already prefers a front
  cover and then falls back to any visual, so the next candidate is tried and a file whose front
  cover is absurd still draws its back cover. A file whose every picture is oversized answers
  `None`, which is what a file with no picture answers, because nothing failed — a picture was
  declined, and that is a `tracing` record rather than an error.
- **A hand-rolled parser never allocates what a header declares.** `riff.rs` and `matroska.rs` both
  read `declared.min(MAX_…_BYTES)` — the `id3 ` chunk through a `take` that grows only as bytes
  arrive — and then seek absolutely past whatever the chunk or element claimed, so a thirty-byte file declaring a gigabyte costs one bounded read and a failed seek
  rather than the allocation. A declared size is what the walk advances by; it is never what a `Vec`
  is sized to.
- **A prescan reads the head of a source that cannot seek, rather than skipping it.** The `INFO`
  scan and the EBML title scan both bail on the first bytes where the magic is not theirs and
  restore the position they found, so a container that is neither pays a rejected read and nothing
  more. A source that cannot seek cannot be restored, so `container::open` reads `MAX_PRESCAN_HEAD`
  bytes into memory with `take`, scans a `Cursor` over those, and hands symphonia a `Replaying`
  stream that serves the head before the rest — still `is_seekable() == false` and still
  `byte_len() == None`, so the reader sees the stream it always saw, from the start. Both walks
  advance by absolute seek, which over a `Cursor` of the head simply stops at the end of it, so
  neither `riff.rs` nor `matroska.rs` knows the difference. What this reaches is what lives near the
  front of a file — Matroska's `Info`, a WAV's leading `LIST INFO` — and not a `LIST INFO` a writer
  put after `data`.
- **A prescan over a source that *can* seek reads through a window, because the four walks are made
  of four-byte reads.** `riff.rs`, `matroska.rs`, `boxes.rs` and `flac.rs` each read an id or a
  header a few bytes at a time and then seek absolutely past whatever it declared, and `Reading`
  delegates straight to the `File`, so one open cost about forty syscalls before symphonia was
  handed anything. `Prescan::buffered` is `Prescan::read` run over a `Window`: one `PRESCAN_WINDOW`
  buffer refilled by an absolute seek and a read, a `stream_position` that costs nothing because
  the window counts it itself, and a seek that moves only that count. A walk that reads four bytes,
  seeks and reads four more therefore pays one read for the lot, and a seek back to a byte already
  held pays nothing. A seek never invalidates the window — only a read that falls outside it
  replaces it — which is what makes `boxes.rs`'s one `SeekFrom::End` and the return to the head
  free; a read larger than the window goes straight through. `rewind_the_source` puts the real
  stream back where the prescan found it, because each sub-scan restores the window's count rather
  than the file's. A source that cannot seek keeps the `Cursor` over its head, which is memory
  already. The window is an array rather than a `Vec`, so a prescan allocates nothing on a path
  sixteen workers are on at once. What it buys, measured over this library: the reads a scan makes
  fall 58 % and the seeks 64 %, and what it costs is about 3 MiB of peak scan RSS against a 37 MiB
  baseline — a trade that only looks even on a warm page cache, and is plainly worth it on storage
  where a seek is a round trip.

- **A cue sheet's track is a window on a file, and the window lives in the decoder.**
  `Decoder::open_span` seeks to the span's first frame and stops at its last, and rewrites
  `MediaInfo::duration` to the span's own length, so everything upstream sees a short file and
  needs no notion of a cue sheet at all: the engine's seek clamp, the seek bar, `heard.rs`'s
  "half the track" and MPRIS's `mpris:length` all read `info.duration`. `self.position` stays
  absolute inside the decoder — `fill`, `discard_to` and `seek_reader` are untouched — and only
  `position()` and `seek()` convert at the boundary. `confine` takes the landing `seek_reader`
  reports rather than assuming the seek was exact and discards forward from there, which is what
  lands a span of a FLAC on a sample the container cannot address directly.
- **A window on a file is billed by the sheet that cut it, and the span is what names the row.**
  `confine` also writes the row's own `TagSet` over the file's wherever a cut names the frame the
  span starts on, so a single-file rip plays as twelve tracks rather than twelve copies of the
  album: the title, the number and — the reason it is the decoder's business rather than a front
  end's — `REPLAYGAIN_TRACK_GAIN`, which `Track::open` resolves off `info.tags` like any other
  file's. `CueFile::cut_at` is the match, and it is on the start frame alone, because that is the
  one edge both the queue row and the sheet compute from `CueTrack::start`. `CueTrack::titled` is
  the row's tags with `Track N` where the sheet named it nothing, and the scan writes its rows
  through the same call, so the catalog and the transport cannot bill one row two ways.
  `cue::cut_for` is where the cut comes from: the `MediaInfo::cue` the file embeds, and otherwise
  the sheet beside a *local* file — `<stem>.cue`, then `<stem>.CUE` — whose `FILE` line names it,
  or its only cut where it holds one, which is what a rip converted after its sheet was written
  still reads as. A source that is not the filesystem has no sidecar to find and keeps the
  embedded answer or none.
- **A row is probed the way it is played.** `probe_span` is `probe` put through the same cut, so
  the tags a queue row draws and the tags it plays under come off one reading rather than two —
  `crates/resonate-codec/src/decoder.rs` asserts the pair agree. It is what the engine's catalog
  reads a row with a span through, and the whole of why a cue row on the bus carries its own
  title.
- **The 1/75 s unit a cue sheet counts in is a `sector`, never a frame.** `Frames` means a PCM
  frame everywhere in this workspace and a cue sheet's `mm:ss:ff` does not; calling both "frame"
  is the mistake the format invites. `CueStamp::at` is the one conversion, and it is exact at
  every rate the workspace supports — 44100/75 is 588. A stamp's minutes are a `u32` and its
  seconds and sectors a `u8` each, so a minute count no sheet could mean is refused as the stamp
  is read rather than multiplied past `u64` on a scan worker or the engine thread.
- **A rip that embedded its sheet is cut by the same reader, and where a track starts is a type.**
  A `CueTrack::start` is a `CueStart`: `Written(CueStamp)` for the `mm:ss:ff` a text sheet counts
  in, `Sampled(Frames)` for the sample offset a FLAC `CUESHEET` block counts in, and `CueStart::at`
  is the one conversion — so routing a block through a stamp cannot round a non-CD-DA rip's
  boundaries by up to the 13 ms a sector is. `MediaInfo::cue` is what a file carries, filled in
  `container::coded_info` from two places and preferring the first: a Vorbis `CUESHEET` comment,
  which is a whole cue file and carries the titles, read through `tags::read_cue_sheet` and handed
  to `cue::read`; and otherwise the binary block, which `flac.rs` walks off the metadata headers
  beside `riff.rs` and `matroska.rs` in the `Prescan`, so a file that is not a FLAC costs the
  four-byte magic. A block carries no names at all, so its tracks come back with an empty `TagSet`.
  Its last track is the lead-out — 170 for CD-DA, 255 otherwise — and it is kept in
  `CueFile::tracks` as a `CueTrackKind::Data`, because `audio_tracks` then skips it while `span_of`
  still reads its offset as the end of the last audio track. A track's start is its own offset plus
  the offset of its index 1 where it declares one, which is what makes a pregap fall to the track
  before it, the way `INDEX 01` already does in a text sheet.
- **A track belongs to the file its `INDEX 01` is in, whichever `FILE` line its `TRACK` line
  followed.** EAC's default sheet for a rip of one file per track — gaps appended to the previous
  track — writes each track's `TRACK` line and its `INDEX 00` at the tail of the file before, then
  the next `FILE` line, then `INDEX 01 00:00:00`. Closing the track at the `FILE` line cut every
  file into its own track and a sliver of the next one's gap, a row of 0:00, and left the last
  file claimed by the sheet with no track at all, so it was never scanned. `Reading::file` carries
  a track that has not reached its `INDEX 01` across the `FILE` line with its start put back to
  the head of the new file, so the gap stays where it is — the end of the track before — and each
  file is its one whole row.
- **A cue sheet is read as far as it parses and never fails.** An unknown command is skipped, as
  `lrc.rs` skips a bracket that is neither a moment nor an id tag, and rubbish yields a sheet
  naming nothing. Only the source and the `LARGEST_CUE_SHEET` ceiling raise a `codec::Error`.
  Text is decoded by BOM first, then valid UTF-8, then Windows-1252, because EAC really does
  write UTF-16LE.

## DSD

- **A DSD file's carrier rate is what the rest of the workspace sees, and it is the same either
  way it is delivered.** `SampleRate` is bounded to 768 kHz, so 2 822 400 Hz is not one and never
  will be; `DsdRate` carries the native rate and `carrier()` is `hz / 16`. A DSD64 file is
  176.4 kHz `S24` stereo whether it reaches the sink DoP-packed or decimated, so `Frames`,
  `Timeline`, the seek bar, the ring and `plan_output` need no special case and the choice between
  the two is one switch. DSD512's 1.4112 MHz carrier is out of range and the file is refused at
  open with `RateNotRepresentable` naming the DSD rate.
- **`Packing` is the guard, not a rule someone has to remember.** It rides on `MediaInfo` and on
  `OutputPlan`, and the only constructor that can produce `DopMarked` is `untouched`, which writes
  `resample`, `equalisation`, `gain`, `dither_to` and `shaping` as literals. There is no reachable
  path from a packed source to a plan with a stage in it, and
  `no_setting_the_pane_offers_can_produce_a_marked_plan_with_a_stage_in_it` walks every sink shape
  crossed with every gain, equaliser, dither, shaping, volume and ReplayGain mode to say so — and
  counts the marked plans it reached, because its loop opens with a `continue` and an axis that
  refuses DoP throughout would otherwise prove nothing while looking thorough.
  `OutputPlan::delivery` then derives what to ask the decoder for from the same plan, so the two
  cannot disagree.
- **DoP is opt-in and off by default.** Nothing in ALSA or SPA advertises DoP support — it is
  invisible to everything but the DAC's own detector — and DoP sent to a DAC that does not decode
  it is full-scale white noise. So `EngineConfig::dop` defaults to false, the decimator is what an
  unconfigured run gets, and the gate is `SinkInfo::supports` exactly, never `best_spec_for`,
  whose fallback to a sink's widest format would truncate DoP to S16 noise and whose fold to a
  sink's layout would rewrite the markers channel by channel.
  `Decoder::set_output_format` means *samples in this format* and clears the packing; DoP is only
  ever reached through `deliver`. `resonate explain` prints which was chosen and why the other
  was not.
- **An all-zero DSD byte is negative full scale, not silence.** It bites in three places, and each
  has its own answer: the DSF final block's padding, which is truncated against the declared sample
  count; the decimator's FIR history, primed with `DSD_SILENCE` (`0x69`, density one half); and the
  gap a seek opens, which the ring fills from `Packing::silence` rather than with PCM zeros.
- **The ring is told what silence looks like on the wire; it does not assume zeros.** `ring` takes a
  `Silence` beside the spec and the capacity — `Unmarked`, or `Marked` with a four-byte word, its
  alternate and the `SampleFormat` that says how much of each word a sample holds — and
  `Silence::write` lays one word across the channels of a frame and swaps it with its alternate for
  the next, so the marker goes on alternating through a gap no audio fills. `Packing::silence` is
  the only constructor: `Samples` answers `Unmarked` and `DopMarked` answers `0x05` and `0xFA` over
  a `0x69 0x69` pair, which is DSD silence with the marker a DAC locks onto still running. A DAC
  therefore holds lock through the prime window a seek opens instead of dropping out of DoP and
  muting for it. `Unmarked` writes nothing and answers zero, because a short chunk is already
  silence to a PCM sink, so every PCM stream is handed exactly the bytes it was handed before.
  `Silence` is `Copy`, allocates nothing and indexes nothing, because `RingConsumer::fill` is the
  RT thread.
- **A gap is a gap however it opened, so a starved read is covered too.** A seek's prime window was
  the only one `Silence` filled; a ring that simply ran dry mid-track handed the graph a short
  chunk, and for a DoP stream that is a whole quantum with no carrier in it — about 6 ms at 176.4
  kHz — which is the one thing a DAC cannot hold lock through. `RingConsumer::fill` now pads the
  rest of the quantum after an underrun, and the reading that says whether it may is
  `Discard::finished`: the producer sets it when a track's last audio has been written, so a short
  chunk at the *end* of a track is what it always was and the graph's drain handshake still closes
  the boundary. The `RtFault::Underrun` is raised either way, so what the padding changes is what
  the sink hears and not what the engine is told. An unmarked ring pads nothing, `Silence::write`
  answering zero for it, so every PCM stream is still handed exactly the bytes it was handed
  before.
- **What a starve cost is counted in frames, apart from the faults that say it happened.** The
  ring adds the frames the graph asked for and was not handed to one `AtomicU64` beside the fault
  queue — an add, so the realtime rules hold — whether its own silence covered them or the graph's
  short chunk did, and never for the short chunk a track ends on. `RingMonitor::went_without` swaps
  the count out, so what the engine reads is what is new since it last asked, and
  `collect_faults` keeps it on the same terms it keeps the underrun count: while playing and before
  the track has ended. `OutputStatus::went_without` is the total at the sink's rate for as long as
  the output lives, the inspector's *Output* card draws it in milliseconds beside the underruns
  once there is any, and `Event::Underrun` carries the same figure. It is exact where the fault
  queue is not: a fault the queue had no room for arrives as a bare count in `RtFault::Dropped`,
  which is why `RtFault::Underrun` no longer carries frames of its own — the count is where they
  live. `what_a_starved_graph_went_without_is_published_in_frames` pulls a second from a ring a
  tenth as deep and weighs the published figure against what the pull was handed.
- **The marker a splice resumes on is the audio's, not the ring's, and both edges of a gap are
  made exact.** A DoP marker alternates by the decoder's *absolute* frame while `Silence`
  alternates by its own count, so silence dropped between two audio frames could put two like
  markers together at either end of it — and a discard makes it worse, the frame the ring refills
  from being a position nobody has read yet. Both edges are closed by reading the marker off the
  audio itself, and what carries it is the *sign*: `Dop::packed` puts `0x05` in the top byte of a
  24-bit word and `0xFA` in the same byte sign-extended, so the first marker reaches the sink as a
  positive sample and the second as a negative one, which `marks_negative` reads off the top byte
  of the word whatever the format's width. `Silence::follows` is the leading edge — the consumer
  hands it the last real frame of every fill, so the phase is always the one that comes *after*
  what the graph last heard — and `RingConsumer::realigned` is the trailing one: where padding has
  been written and audio is waiting, the first frame is peeked through an uncommitted
  `read_chunk`, and one more silence frame is written ahead of it where `Silence::would_write` says
  it would otherwise repeat. A frame inserted that way costs 5.7 µs and is what keeps the
  alternation unbroken across a seek, a starve and the first audio after a stream opens alike.
- **A decimation is a conversion, and DoP comes back the moment nothing stands in its way.** A
  decimated stream is the carrier rate at `S24`, which is exactly what `MediaInfo.spec` says, so
  `plan_for` reads the source's `Packing` rather than its spec to judge it: a `DopMarked` source
  delivered as samples is `OutputMode::Converted`, and the graph is not asked for `NO_CONVERT` on
  its behalf. Going the other way is `packs_again`: a DoP stream turned down is decimated through a
  rebind, and turning it back up would otherwise reshape the decimated chain in place and stay
  there until the next track, so `retune` asks it first — the source is DSD, the open plan is
  samples on the carrier's own spec, and `dop_survives` against the bound sink — and rebinds into
  the marked plan where it holds.
- **A DSDIFF file is tagged the two ways its writers tag it.** The `ID3 ` chunk a tagger appends
  after the sound is read by the same ID3v2 reader a DSF's metadata block goes through, and the
  `DIIN` chunk's `DIAR` and `DITI` — the edited master's artist and title — fill only what the ID3
  tag left empty. A `DIIN` text is bounded by `MAX_EDITED_TEXT_BYTES` and by its own chunk, so a
  count that claims more than the chunk holds names nothing.
- **DSF bit-reverses and DFF does not.** DSF's `fmt ` chunk declares `1` for LSB-first and `8` for
  MSB-first — checked against ffprobe, which reads a hand-built fixture of each as `dsd_lsbf_planar`
  and `dsd_msbf_planar` — while DFF is always MSB-first. Reading it the wrong way round yields
  audio that is recognisably music and grossly distorted rather than an obvious failure, so both
  orders decode the same tone in a test.
- **The `0xFA` marker has to sign-extend negative.** A DoP word is a sign-extended 24-bit value in
  the low bits of an `i32`, which is what `S24` means here and what `S24_32LE` puts on the wire, so
  `0xFA1234` is `-388_556`. The raw bytes look right either way in a hex dump, and getting it wrong
  only shows up once `retype` or `convert` touches the sample.
- **The decimator is the codec's own, because the layering forbids reaching `resonate-dsp`.** It is
  a 512-tap Kaiser-windowed sinc decimating by 16, evaluated as a byte-indexed table of partial
  sums: a 1-bit input makes an FIR a sum of taps selected by bits, so there are no multiplies and it
  is cheaper than a CIC plus the compensator a CIC would need. A box average is the wrong shape —
  DSD's shaped quantisation noise rises steeply above 30 kHz and a sinc¹'s sidelobes would fold it
  inward — so the cutoff is 45 kHz in absolute Hz, rebuilt per rate, with ≥90 dB at the output
  Nyquist proved by taking the DFT of the designed taps. A DC blocker follows it, because 1-bit
  modulators routinely leave a bit density off one half. What it decimates reaches an integer word
  through the conversion the chain's output takes, and the filter overshoots a step into all-ones
  by about 8 %, so full scale saturates at 8 388 607 rather than landing on 8 388 608 — which the
  old arithmetic reached from +1.0 and a 24-bit wire reads as negative full scale.

## Sources

  - **`LocalFiles` is the only `MediaProvider` this build registers**, so every `MediaLocation`
  outside it is refused by name. The seam is proved by a provider that serves a track out of memory
  in `crates/resonate-engine/tests/transport.rs` and by nothing else.
- **A provider hands back a `Read + Seek + Send + Sync` stream and says whether it is seekable**,
  which is symphonia's own contract. A source that can only stream forward therefore loses the
  prescan, the seek bar and the box walk rather than being handled differently.
- **`Sources` is resolved by a linear walk over the registered providers**, which is right for the
  handful a desktop player registers and wrong for hundreds.

## Transport

- **Bit-perfect output takes precedence at every track boundary. There is no gapless playback** —
  rejected, not deferred. On a track change the engine drains the ring and reopens the stream. Once
  the ring runs dry it asks the graph to drain and waits for `StreamEvent::Drained`, so the boundary
  falls where the graph says it has played the tail out rather than where a clock guesses;
  `SinkStream::latency` measured on the wall clock is the fallback for a backend that never answers. A
  stream takes every instruction through one `StreamCommand`, which is what keeps that handshake and
  `set_active` and `close` on the same ordered channel to the loop thread.
- **A seek does not reopen the stream.** rtrb gives the producer no way to drop what the consumer
  has not read, so the ring carries a discard epoch instead: the engine bumps it and stops writing,
  the graph thread drains every slot it holds on its next callback and acknowledges, and only then
  does the engine refill from the new position; one that has not acknowledged after
  `DISCARD_TIMEOUT` is taken as stalled, warns and has the stream rebuilt around the seek. The graph
  is fed the stream's own silence until the ring is half full again — the same mark `Output::primed`
  uses before a stream is opened at all — so a seek costs one clean gap rather than a run of
  underruns, and the gap is not reported as a fault. What that silence *is* comes off the plan and
  not from a `memset`: see the DSD note above, because for a DoP stream zeros are not silence. A
  seek while the transport is paused is the exception and reopens: a graph that is not calling
  `process` can never acknowledge the discard, so the in-place path would leave the engine unable
  to refill until playback resumed. Reopening leaves the ring primed
  at the new position instead, which is what makes a scrub while paused resume instantly. A pause
  that lands *after* an in-place seek is the same case caught late, and it is read as
  `Unanswered::NothingPulling` rather than as a stall: the stall clock is not a thing to hold at now
  while nothing pulls, because a clock that can never run out leaves the ring discarding, `fill`
  returning nothing and the next Play starting on an empty ring. A sink
  switch and a renegotiation still reopen too, because the negotiated format changes with them.
- **Leaving or returning to unity gain, and switching the equaliser on or off without a resampler,
  reshape the chain in place.** `retune` builds the wanted plan against the stream that is open.
  Where `OutputPlan::same_shape_as` holds it retunes the chain it has; where it does not but
  `OutputPlan::becomes_on_the_same_stream` does — the same `stream`, `packing`, `remix` and
  `resample`, and no resampler — it builds the new chain with `build_chain` on the engine thread and
  swaps it in under the ring, the stream and the consumer it already holds, so what the ring carries
  is still in the negotiated format and nothing reaches the realtime thread. The new chain's gain
  stage is handed what the old one was applying, `Chain::gain_amplitude` or unity where the old
  chain had none, through `Chain::ramp_gain_from`, and ramps from there to its own target over the
  usual ramp, so the level never steps. The decoder is pointed at the new plan's `delivery()` and the
  samples already decoded but not yet converted are retyped to it with `AudioBuffer::retype`, so
  nothing is dropped or read twice. A converting plan's delivery is the source's own word —
  `OutputPlan::decoded_as`, which is F32 only for a float source and a decimated DSD one — so the
  retype is exact for every source, a 32-bit integer one included. `Output::take_chain` writes
  the new plan and its mode into what is published and the swap emits `Event::OutputChanged` the way
  a rebind does. Going back to a plan with no gain stage — the volume back at exactly 100 % with
  nothing else in the chain, or the equaliser switched off with nothing left — first retunes the
  running chain's gain to unity, holds the wanted plan in `Output::settles_into`, and swaps only once
  `Chain::is_ramping` says the ramp is over, which `finish_reshaping` reads after every `pump`. A
  stage already at unity has nothing to ramp, because `GainStage` does not ramp to where it already
  stands, so the equaliser switched off at full volume swaps at once. A newer command drops what was
  waiting and is judged against the chain still running, and a rebind replaces the `Output` and the
  wait with it. A resampler forces a rebind: its history and the phase it has reached cannot be
  carried into a new chain without a click, so the equaliser switched on or off while resampling
  still costs a gap. A volume or ReplayGain change under a resampler never does, because a
  converting plan always carries a gain stage — see DSP — and is retuned rather than reshaped.
- **A renegotiation converges because the next plan asks for exactly what the graph answered.**
  `StreamEvent::FormatChanged` carries the spec the graph settled on, and `downgrade` hands it to
  `plan_for` as the target, which writes it into `OutputPlan::stream` whole — the rate, the format
  and the channel layout alike. Writing the source's channels back in is what used to make the two
  trade turns forever, each round costing a teardown, an enumeration and a gap. `MAX_RENEGOTIATIONS`
  is the belt to that braces: a graph that answers a third different spec fails the track with
  `Error::Renegotiation` rather than looping, and the count is reset by `Engine::start` so it is per
  track rather than per run.
- **The run loop waits on every channel that can change its mind, and the ring sets the timeout.** A
  command, a sink announcement and the stream's own events each wake it, so `StreamEvent::Drained`
  closes a track boundary the moment the graph reports it rather than on the next tick. The timeout
  is half of what the ring holds, floored at the 4 ms `SHORTEST_TICK` so a primed ring cannot spin
  it and capped at the 16 ms `PUBLISH_TICK` that `Published` is refreshed at, which is what a 60 Hz
  front end draws from — about a third of the wakeups a fixed 4 ms tick cost. Nothing streaming
  makes it the 100 ms `IDLE_TICK` flat. A receiver that has disconnected reports ready forever, so
  `changes` is dropped and `Output::deaf` set the first time one does, rather than spinning. What
  paces the loop while it plays is publishing rather than the ring, because a reader polls
  `Published` instead of being told: a full buffer is still woken over sixty times a second.
- **The set the loop parks on stands for as long as it stands, and `run` is two loops because of
  it.** A `Select` borrows the receivers it is registered over, and the stream's lives inside the
  `Output` that every `&mut self` method replaces, so one built from `&self` cannot outlive the
  body that rebinds — which is why it was rebuilt from scratch at up to 250 Hz, a
  `Vec::with_capacity(4)` and three registrations a wakeup, for a set that changes only when a
  stream opens, closes or goes deaf. `Heard` is that set lifted out: `Engine::heard` clones the
  two `Receiver`s, which share their channels with the originals rather than copying them, and
  the outer loop owns it while the inner one parks on it. `Heard::is_what` is read at the foot of
  each pass and compares by `same_channel`, so the inner loop breaks and the set is built again
  exactly when it has changed and never otherwise — which is also why the outer loop cannot spin,
  a set just built always being what the engine is holding. `unsafe_code = "forbid"` is what ruled
  out the self-referential field this replaces.
- **Enumerating sinks mid-stream is paid for out of the ring.** A `SinkChange` sets a flag rather
  than enumerating on the spot; the refresh runs on a later tick, and only while the ring holds at
  least twice the 50 ms `SINK_REFRESH_BUDGET`, so a daemon that has stopped answering costs a
  fraction of the buffer rather than an underrun. Nothing streaming means the `SINK_TIMEOUT` startup
  budget of 2 s applies instead.
- **A bind reads the published sink list; only an announcement refreshes it.** `select_sink` is on
  the path every track change, every seek that cannot be done in place, every settings change and
  every renegotiation takes, so enumerating there put a 2 s `SINK_TIMEOUT` in front of each of them
  against a daemon that had stopped answering. What the change stream announces is what makes the
  list stale, and `note_sink_changes` is read before the bind as well as on the tick, so a device
  that turned up a moment ago is bound to and a device that did not costs nothing. The three
  standing reasons to ask the graph again are a stale flag, an empty list and a change stream that
  has gone — and where the ask fails with a list still published, the bind takes what was published
  rather than failing the track.
- **A graph that lets go of the ring is waited for once, and fails the track the second time.**
  `RingProducer::is_abandoned` says the consumer has been dropped, which is the graph thread having
  gone with the `AudioSource` it was handed — what a daemon restart does, the client dropping every
  stream of the connection it lost. `watch_graph` reads it once the stream is open, closes the
  output, keeps the row with the heard position in `unbound` and publishes `Loading`; each pass
  after that asks the backend for its sinks and, once it answers, binds the row again at that
  frame, so a restarted daemon costs a gap and not the track. It waits `GRAPH_BACK_WITHIN`, ten
  seconds, before failing the row with what the last try said, and a pause, a stop or another row
  ends the wait. A graph that lets go again within those ten seconds of the last time is not waited
  for: it raises `Error::LoopStopped`, so the transport skips and eventually stops rather than
  opening and losing streams for ever — `graph_last_lost` is that memory. Asking the sinks first
  is not a courtesy: a bind succeeds against the stale list and fails only when the stream opens,
  so trying the bind alone read a daemon still down as one already back. A disconnected *event* channel is not the same reading and stays what it was —
  `Output::deaf`, dropped from the select — because a graph can stop reporting and go on pulling.
  A `backend.open` that fails takes the whole `Output` with it for the same reason: the consumer
  has gone into the call and cannot come back, so an output that outlived it would hold a ring with
  no stream and no way to open one.
- **The PCM ring carries `u8`, not `f32`.** An `f32` ring would forcibly convert every stream and
  break bit-accuracy for 32-bit sources, which a 24-bit mantissa cannot hold. How deep it is is the
  buffer setting's to ask for and `ring_capacity`'s to answer: the ask is clamped between
  `MIN_RING_FRAMES` — or `BLOCKS_A_RING_HOLDS` of the chain's widest block, where that is more —
  and whatever `LARGEST_RING` bytes come to at the negotiated format, so a mistyped `buffer-ms`
  sizes a ring rather than aborting the process on the first stream open. The block is weighed
  because the converting `fill` writes only while the ring has room for one, and `primed` waits for
  half the ring: an 8 kHz file upsampled onto a 192 kHz sink writes 24 577 frames a block, which a
  100 ms ring of 19 200 could never take, and the track sat in `Buffering` with no error.
  `Frames::from_duration` and `StreamSpec::frames_to_bytes` saturate for the same reason.
- **A track change does not start the transport; a command does.** `Engine::start` opens the row the
  queue is on at whatever the transport was already doing, so `Next`, `Previous` and removing the
  playing row leave a pause in place — which is what MPRIS says those methods do, and what
  `crates/resonate-engine/tests/transport.rs` asserts by watching the fake graph stay idle.
  `Load { autoplay }`, `Play`, `Command::JumpTo` and an `Insert` asked to be heard are what set
  `playing`, because each is a request to hear something now rather than to move through the queue.
  A track that fails mid-play still advances into playback, because the transport was playing when
  it failed.
- **A track change nothing will be heard through opens the file and binds nothing else.**
  `Engine::start` reaching a paused transport records the frame it would have bound at in
  `Engine::unbound` and stops there, so a run of skips through a paused queue costs one
  `Track::open` a row rather than a sink selection, a ring, a DSP chain, a decode to half the ring
  and a `Backend::open` each — all of it thrown away by the next skip. Every `rebind` while
  `unbound` is set records the frame instead of binding, which is what keeps a setting changed or a
  seek taken over a paused row from quietly paying the same cost; `Engine::play` is the one place
  that spends it, and a bind that fails anywhere leaves `unbound` set so the next `Play` tries
  again: `rebind` drops the old output before `select_sink`, `Output::open` or the decoder's seek
  can refuse, so `bind` is the part that may fail and `rebind` records the frame it was asked for
  whenever it does — an autoplaying load with no sink answers `NoSink`, and the `Play` after a sink
  appears binds the row rather than calling `set_active` on a stream that is not there.
  What the row still publishes is what it is — `TrackState` comes off the open decoder, so the id,
  the source spec and the duration are all there — and what it does not is `PlayerState::output`,
  which stays `None` until something is bound, because there is no plan yet to report.

## The queue

- **A queue position is an index into the play order, not the load order.**
  `PlayerState::queue_position`, `Command::JumpTo`, `Command::Insert` and `Command::Remove`'s `Span`
  all mean the rows a queue pane draws, so they stay right under shuffle. `Queue::position` is the
  load-order index behind them, published beside it as `PlayerState::loaded_position` for the one
  reader that draws the order the queue was *loaded* in rather than the order it plays: an opened
  playlist, which marks the row being heard from it. A file that no library scan has seen
  takes its `TrackId` from `unclaimed_id`, which counts down from `u64::MAX` past the ids the queue
  already holds, so no front end needs a counter of its own and two of them cannot mint the same id.
  `Unclaimed::beside` is that walk taken once and then counted down for a whole run of rows, which
  is what queueing a playlist of unscanned files goes through rather than walking the queue again
  per row.
- **A queue row's id names that row and no other, and the queue is where that is made true.**
  `one_id_each` runs over everything `Queue::load` and `Queue::insert` are handed: a row whose id
  is already claimed — by the queue it is joining or by an earlier row of the same batch — is
  given a minted one instead, `Unclaimed::claim` being what marks an id taken so a mint cannot
  hand it out again. It has to be the queue rather than the callers, because two of them mint
  against the queue the engine last *published*: a scanned row carries its library id, so one
  album queued twice arrived as two rows under one id, and two `AddTrack` calls inside one 200 ms
  poll minted the same id from the same stale snapshot. The bus is what the duplicate broke —
  `Tracks` published one object path twice and `row_of` resolved it to the first row either way,
  so `RemoveTrack` and `GoTo` could not reach the second — and
  `one_file_queued_twice_is_two_rows_the_track_list_can_tell_apart` is the claim, over a real
  session bus.
- **A queue is stamped by the rows it holds, not by the ids they arrived under.** `stamp_of` hashes
  each row's location and span, and `Queue::rows_changed`, `RootView::play_playlist` and the
  binary's `play_queue` all read through it, so a row the queue renamed on the way in still stamps
  as the playlist it came from. Hashing the whole `QueueItem` would have made `one_id_each` look
  like an edit to the queue.
- **Loading replaces the queue; queueing adds to it, and where a row lands is a `Placement`.**
  `Command::Insert` carries `Placement::Next`, `Placement::Last` or `Placement::At(row)` rather than
  an `Option<usize>`, and `Placement::row` is the one place that says what *next* means — the row
  after the one playing, or the end where nothing is — so the window, `resonate-mpris`'s `AddTrack`
  and the binary's `Host::open` all land a row by the same arithmetic. Queueing takes the queue
  out of `Library::playing_playlist`, because a queue with a row added to it is no longer the
  playlist it was loaded from, even where the row came from that playlist — it restamps the queue
  rather than clearing the cell, so the badge comes back if the row is dropped again. Whether it
  starts the transport is the `play` flag beside them, which the note below is the whole of.
  `RootView::queue` takes `PlaylistEntry`s, which is already the vocabulary for a location and the
  library row where there is one, so a scanned track and an unscanned playlist row reach the queue
  the same way — `root::listed` turns the first into the second.
- **`Command::Insert` carries whether to hear what it queued, and the row it starts is the one the
  queue answered with.** `Queue::insert` has always returned the play-order row the items landed on
  and the engine threw it away; `play: true` is what spends it, through the `Engine::hear` that
  `Command::JumpTo` goes through too — jump, set `playing`, `start(Frames::ZERO)` — so the two arms
  cannot disagree about what starting a row means. An insert of no items answers `None` and starts
  nothing, so queueing nothing can never open a track. It is also what makes MPRIS's `SetAsCurrent`
  one command rather than two: `TrackList::add_track` sent an `Insert` and then a `JumpTo(at)`
  against a row it had computed off the published queue, which is a reading taken before the insert
  and a second dispatch pass for the queue to move under, and the binary's `Host::open` had the same
  shape. `RootView::queue` asks for it wherever `PlayerState::current` is `None`: queueing into a
  queue that is already playing something queues the way it always did, and queueing when nothing is
  playing plays what was queued, which is the whole of *queue it and hear it now* — and `queued` has
  a third reading for it, because a run that started playing was never queued to play next.
- **Coming back is a command of its own, because a resumption is more than a row to start on.**
  `Command::Load` carries the rows and `start_at`; `Command::Resume` carries a whole `Resumption` —
  the rows as they were playing, where each of them was loaded, the row the queue was on, the frame
  into it and whether it was shuffled — and it is the only caller `Engine::start(at)` has that
  passes anything but `Frames::ZERO`, where `rebind`'s seek guard sees the decoder is already there
  and issues nothing. A frame past the end of what the row now holds — a file replaced by a shorter
  one since the last run — opens the row at its start rather than being handed to a seek that
  would refuse it on the resume and on every `Play` after. A load that is to play nothing and start nowhere still opens no track, which
  is what it always did; a resumption opens its row and stops, because `start` reaching a transport
  that is not playing records the frame in `Engine::unbound` and binds no sink — so a resumed queue
  costs one `Track::open` and one decoder seek, shows its position and its length at once, and
  spends the bind on the first `Play`. It is therefore not a seek: `Seeks` is not stepped, so a run
  that opens on a resumed row announces `Metadata` and no `Seeked`. What it replaces is a `from`
  frame on `Load` that every caller but one passed `Frames::ZERO` to, which is a field to take out
  rather than one to keep; ids are not kept either, so `Queue::restore` mints them through the same
  `Unclaimed::beside` an unscanned file goes through rather than the binary doing it on the way in.
- **The order a queue plays in unshuffled is kept apart from the order it was loaded in.**
  `Queue::unshuffled` is what the queue plays with shuffle off: a drag, a sort or a row played next
  while unshuffled edits it along with `order`, and turning shuffle on and off again comes back to
  it rather than to the load order. Under shuffle it keeps its shape — a removal drops the row from
  it, a row queued last lands at its end, and a row queued anywhere else lands after the row it
  follows in the shuffle, so a play-next made while shuffled still follows the playing row once
  unshuffled. It is not kept across runs: a resumption that was shuffled comes back with the load
  order beneath it, which is what the note below means by the album.
- **A queue comes back in the order it was loaded and plays in the order it was playing.**
  `Resumption::rows` is the load order and `Resumption::order` is the play order over it, one
  entry per playing position naming the loaded row that sits there — the same pair `Queue` holds
  as `items` and `order`, which is why `Queue::restore` takes them as they are rather than
  inverting anything. `resonate-core::plays_in` is what makes trusting a stored order safe:
  an order naming each row exactly once is taken as it stands, and an order of the wrong length,
  one naming a row twice or one naming a row the queue does not hold gives back the load order,
  so a queue is never refused and never left a row short over a number nobody can read.
  `Resumption::shuffle` rides beside the rows because the order alone does not say what the next
  wrap or the next toggle will do, and `Queue::restore` writes the flag rather than calling
  `set_shuffle`, which would reshuffle the order it was just handed. The volume and the repeat
  mode are deliberately not kept: they are settings a run makes rather than a place it reached.
- **What the transport was doing is sampled the way a play is, and what it answers says how much
  of it moved.** `Keeping` sits beside `Listening` and reads the same `PlayerState` and published
  queue, and the three things it can answer are the three tables the catalog holds.
  `QueueStamp` moving — rows arriving or leaving — answers `Keep::Queue` with the whole run.
  `Queued::revision` or the shuffle moving where the stamp did not answers `Keep::Order` with the
  order alone, which is a drag, a toggle, or the reshuffle a wrap under `RepeatMode::Queue` takes:
  the rows are the same rows and rewriting a URI per row said nothing. Anything else answers
  `Keep::Place`, and only where the row changed or the position moved `KEPT_EVERY` — five
  seconds — in either direction, so a 60 Hz observer writes to SQLite about as often as a 500 ms
  tick does. The two triggers are read together because `QueueStamp` is `stamp_of(self.items)`,
  the rows in load order, where the revision counts every republication including a reorder; and
  the shuffle is weighed beside the revision because it rides on `PlayerState` while the order
  rides on `Queued`, so a sample that read the two a beat apart is put right by the next one
  rather than settling wrong. Only the first of the three allocates a `Resumption`, which is what
  keeps a 2 000-row queue from being cloned every time the position ticks. What it costs is that a
  run killed rather than closed loses up to `KEPT_EVERY` of position. Worth knowing: an
  *unshuffled* wrap under `RepeatMode::Queue` moves neither the stamp nor the revision, because
  only `reshuffle` bumps it — the cursor alone returns to nought — so it has always taken the
  `Keep::Place` path. `Resumable`, `Resumption` and `Reordered` are `resonate-core`'s because the
  engine builds them and the catalog stores them and neither may depend on the other — the same
  argument `Span` makes.
- **A play is what was heard, not what was started, and the transport is sampled rather than
  hooked.** `Listening` is the whole of the rule: it is handed a `PlayerState` beside the queue in
  play order, accumulates the frames the position advanced while the transport was playing — a step
  larger than `A_SEEK` is a seek and adds nothing — and answers with the row's `MediaLocation` once
  half the track, or the four minutes `COUNTS_AS_HEARD` names, has gone by, whichever is the
  shorter. A track declaring no length counts at the four minutes. It answers once per visit, and a
  position that goes backwards starts the count again, so a track played from the top counts a
  second time. It is a sampler because the write behind it is SQLite: `Library::track_played` would
  leave the run loop waiting behind a scan that holds the writer, so `RootView::count_a_play` runs
  it off the window's observer of `PlayerModel` and `resonate play`'s loop off a `HEARD_SAMPLE`
  tick — which is why a run with no catalog counts nothing at all.
- **A visit says how long it was heard for, and it says it twice.** `Listening::heard` answers a
  `Counting`: `Counts(Played)` at the threshold above, exactly where it always answered, and
  `Settles(Played)` once when a visit that counted ends — the track changed, the transport
  stopped, or the position went back far enough to begin another. `Played` carries `heard`, which
  is what had been listened to at that moment and, on the settle, the whole of it. A visit that
  never counted answers neither, so what is recorded is the time inside plays that counted and a
  skip is not billed as listening; `docs/TODO.md` records what that leaves out. The pair is
  joined by an id rather than by a location: `Library::track_played` answers the `ListenId` it
  wrote, the caller keeps it beside its `Listening`, and `Library::listened` spends it — so a
  write that failed keeps nothing and a settle can never be attributed to the wrong row. The
  ending visit cannot be read off the queue, which has already moved on, so `Listened` holds the
  `Played` it counted with, which is why neither it nor `Listening` is `Copy` any more. Between
  the two, a counted visit answers `Hears` every `TOLD_EVERY` — thirty seconds — of listening,
  which the caller writes through the same `Library::listened` without spending the id, so a
  window closed mid-track or a `resonate play` that leaves on `QueueFinished` loses at most that
  much. `Listening::leaves` settles whatever visit is open, and `resonate play` calls it on the way
  out after one last sample.
- **The sleep timer is the engine's, because a headless run wants one too.**
  `Command::SleepUntil(Option<Until>)` carries `After`, `EndOfTrack` or `EndOfQueue`, `None`
  cancels, and the deadline lives on `Engine` — so `resonate play`, the window and the bus all
  get the same timer rather than three. It **pauses**: `Engine::stop` closes the stream, drops the
  decoder and clears the track, and somebody who falls asleep should wake where they were.
  `doze` clears the timer before it acts, and a timer running out against an idle transport
  simply clears, rather than asking `pause` for a transition it would refuse and logging the
  refusal. `budget()` takes the minimum of its usual wait and what is left, because an idle
  transport parks `IDLE_TICK` and would otherwise overshoot by up to 100 ms. The two end-of
  variants need no clock, only the edges in `skip` — and the wrap is the one that would have
  been missed, because under `RepeatMode::Queue` `advance` never answers `None`, so
  `Queue::wraps_next` reads the same two fields `advance` decides the wrap from and the question
  lives beside the decision. The timer outlives a track change, a seek, a pause and a new load:
  it is a timer on the listener, not on the transport. A delay is held to `LONGEST_SLEEP`, a day,
  as it is set, so a `SetSleep` of `u64::MAX` seconds or a minute count `resonate sleep` saturated
  is a timer that reads back as a day rather than an `Instant` overflowing on the engine thread.
- **A skip while one track repeats repeats the queue instead, unless the listener said otherwise.**
  `Command::Next` and `Command::Previous` are a person's skip — the end of a track reaches `skip`
  as `natural` and never through either — so `skipped_by_hand` turns `RepeatMode::Track` into
  `RepeatMode::Queue` before it moves, the way a streaming player does, and a skip past the last
  row then wraps rather than stopping. `EngineConfig::skip_under_repeat` is the policy,
  `SkipUnderRepeat::KeepsRepeatingTheTrack` the way out, `Command::SetSkipUnderRepeat` sets it and
  `PlayerState::skip_under_repeat` publishes it; it lives in the engine rather than in the window
  so a media key, an MPRIS `Next` and `resonate play`'s `n` all obey it. A jump to a chosen row is
  not a skip and leaves the repeat alone. The `skip-repeats-queue` key and the Library category's
  *Repeating a track* are where it is set.
- **What is published as the position never steps back within a stretch of listening.** The
  position is the decoder's less what the ring, the chain and the sink still hold, and the sink's
  latency is only known once a stream reports it, so every rebind and every seek used to publish
  a position a latency behind the one before it — which `Listening` read as a new visit, keeping a
  seek forward from counting and letting a rebind count a long track twice. `heard_position` holds
  the published value at or above the last one published, and what lets it go is a seek that
  landed, a track opened and a stop, which are the only three moves backwards a listener makes.
- **Giving up is counted per run of failures, not per track opened.** `Engine::fail` stops the
  transport once more rows have failed than the queue holds, and what puts the count back is a
  track played to its end, a stop, or a person choosing a row — `Next`, `Previous`, a jump, a load —
  and not a stream that opened: a daemon that takes every stream and then drops it would otherwise
  reset the count on each open and loop a repeating queue for ever.
- **A row that will not open is passed over for the next one, not stood on.** A deleted file, an
  unmounted disc or a playlist row `--tidy` would drop answers `Error::Decode` out of
  `Track::open`, and `past_what_will_not_open` is what every command that means *play something*
  runs the start through — `Load` with autoplay, `Play` on a row with no track, `Next`,
  `Previous`, a jump, an `Insert` to hear and a removal of the playing row — so where the
  transport was meant to be playing the failure goes to `Engine::fail`, which reports it and
  skips, rather than back to the caller with the transport standing on nothing. `skip` asks
  whether the *queue* is on a row rather than whether a track is open, so `Next` moves off a row
  that never opened, and `settle` hands a natural skip that failed to `fail` rather than to
  `stop`. `Error::track` is how `Event::Failed` names the row a failed open was for, `start`
  having cleared `self.track` before the open. A paused transport is left on the row: nothing is
  meant to be heard, so the `Play` that follows is what moves on.

## What the engine publishes

- **What the engine publishes is one `Published` bundle, not a growing argument list.** It holds the
  `PlayerState`, the `OutputSettings`, the `StreamDigest`, the queue, the sink list and the `Tapped`
  the visualiser reads, each behind its own lock so a 60 Hz poll clones a pointer, and the one
  `listening` flag the window writes back. The queue is a `Queued`: the rows in play order,
  where each of them was loaded and the revision the two were drawn at, under *one* lock, so a
  reader cannot pair rows with an order taken from another moment — `Player::queue` hands back the
  rows alone, which is all any pane wants, and `Player::queued` the three together, which is what
  `Keeping` reads. The queue and the digest are
  written *before* the `PlayerState` that advertises them, so a reader that sees `shuffle` turn on
  cannot still be holding the order from before it. `PlayerState::seeks` is a token beside
  `queue_stamp`: `Engine::seek` steps it where the seek landed and nowhere else, so a reader is told
  that the position moved on purpose rather than being left to infer it from the position — a seek
  the engine refused steps nothing, and a track change goes through `start`, which does not step it
  either, because moving to another track is not a seek within one. It is what `resonate-mpris`
  emits `Seeked` from. `PlayerState::sleeping` rides beside it and carries what is *left* on the
  timer rather than when it is due, so a front end draws a countdown off the publication it
  already polls and keeps no clock of its own; what that costs is a state that differs on every
  tick for as long as a timer is set, which `docs/TODO.md` records. The queue is republished only when
  `Queue::revision` moves, which loading and reshuffling bump and advancing does not — comparing the
  items themselves would mean cloning every path on every 4 ms tick.
- **A command is answered once the state that answers for it has been published.** `Engine::dispatch`
  keeps each reply as an `Answer` and `Engine::answer` sends the lot after `publish`, at the end of
  the same loop pass that applied the command, so `Outcome::wait` means *the published state already
  says so* rather than *the engine has seen it*. A command reaches the engine one of three ways, and
  `Reply` is which: `Player::send` is waited on by nobody and a refusal is announced as
  `Event::CommandFailed`; `Player::request` hands the refusal back in its `Outcome` and announces
  nothing, because the asker is the one that says it; and `Player::settle` answers a `Landing` that
  says only that the command has been applied, while a refusal is still *announced* — which is what
  a command from somebody the window cannot see wants, so a Next refused over the bus still reaches
  the window's notice. `resonate-mpris` settles every call that moves the engine — the transport,
  `Seek` and `SetPosition`, the three writable properties, `SetSleep`, the track list's `AddTrack`,
  `RemoveTrack` and `GoTo` and a notification's buttons — through `Shared::settle`, which waits
  `SETTLE` for the landing, so a client reading straight after a call reads what the call did. The
  setters needed it first: zbus emits `PropertiesChanged` by calling the getter the moment a setter
  returns `Ok`, so a setter that returned first announced the value it had just changed away from,
  and the right one arrived up to a poll later. A wait that runs out is a debug record and the old
  behaviour, not a refusal: the command is already on its way to the engine.
- **What a setting is *set* to comes from the engine, not from the pane that set it.**
  `OutputSettings` is the configured sink name, resampler quality, dither, noise shaping, ReplayGain
  mode, sample rate policy, graph rate request and buffer depth as the engine currently holds them,
  republished as a new `Arc` only when one of them moves, so the settings pane marks the chosen
  option from what is in force rather than from what it last sent. It is separate from `PlayerState`
  because the sink *name* is the choice and `OutputStatus::sink` is the `SinkId` that was actually
  opened: a name nothing answers to yet is still the selection. `EngineConfig::sink` being `None`
  means *follow the default*, which is a choice a pane has to be able to make, so `Setting::Sink`
  carries an `Option<NodeName>` and `config::clear` removes the key rather than writing an empty
  one.
- **The inspector never reads the playing file a second time.** The engine samples
  `Decoder::last_packet` through a `ProfileBuilder` and publishes a `StreamDigest` beside
  `PlayerState`, as an `Arc` so a 60 Hz poll clones a pointer rather than the series. `resonate-ui`
  renders that, and takes no dependency on `resonate-codec`. The box layout beside it is the one
  thing that does open the source again, and it is the inspector's to draw rather than the
  transport's to play, so `inspected` answers `None` and records the reason where `probe_boxes`
  fails: a provider serving one handle, an EMFILE or a file replaced underneath refuses a drawing,
  never a track that would have decoded. The analysis pane is the other: `Player::analyse` decodes
  the playing row whole through the player's `Sources`, on the window's background executor and
  never on the engine thread, and only while that pane is in front — `analysis.md` has it.
- **What is heard is tapped where it is written, and read back by where the graph has got to.**
  `Tapping` sits on `Output` beside the ring, and `Engine::fill`, `convert` and `drain` hand it
  exactly the frames `RingProducer` took, out of the buffer that was written: the decoded block on
  a transparent plan, the staged words the chain's carrier was narrowed into on a converting one —
  so the equaliser, the gain and the dither are all in it — and the staged tail on a drain. What it holds is therefore what
  reaches the sink, at the sink's rate, as f32 left and right: a mono stream on both sides, a wider
  one its first two channels. A `Tap` is a ring of `AtomicU32` holding f32 bits, a power of two
  long and sized to the PCM ring, `HEARD_SLACK` of the graph's own delay and `WIDEST_LOOK` of
  analysis window, under a `LARGEST_TAP` ceiling; it is allocated with the ring on every bind and
  never again, 1 MiB at 48 kHz under the default 500 ms buffer and 2 MiB at 192 kHz. A write
  *claims* its frames behind a release fence before laying them and publishes the count with
  release after; a read takes the count with acquire, reads, fences, reads the claim again and
  silences any frame the writer may have gone round onto — so nothing locks and the engine never
  waits on the window, which is the whole point of a thread with no slack. Where the graph has got
  to is an anchor `Engine::publish` fixes every pass: frames tapped less what the ring holds less
  `SinkStream::latency`, beside the instant and whether the transport moves, written through a
  three-atomic seqlock so the pair is never read torn and the writer never spins. `Tap::around` runs
  on from it by the clock while the transport moves — never more than `RUNS_AHEAD_AT_MOST` and
  never past what is written — and hands back the frames *centred* on the one being heard, because
  an analysis window describes its middle rather than its end; what `pw_time.queued` leaves out of
  the latency is the error left in it. `Player::listen_in` is the switch: unlistened, a block costs
  a load and a store and marks the tap empty, and listening again marks `valid_from` at the count,
  so what the ring already held when the pane opened reads as silence until it has turned over. A
  seek in place forgets the same way, which makes the silence the graph is fed while the ring
  refills silence here too. `Player::tap` answers `Tapped`: `Nothing` with no output bound,
  `Samples` with the tap, and `Markers` for a DoP stream, which carries no PCM to read — republished
  only when it changes, weighed by pointer. Measured on the dev profile, a listened tap costs
  0.006 % of a core at 48 kHz and 0.02 % at 192 kHz, an unlistened one 0.0001 % and 0.0004 %.
  `crates/resonate-engine/tests/transport.rs` pauses the fake graph and holds `Tap::around` to the
  very frames it was last handed, the frames either side of them included; holds a turned-down
  stream to what the chain handed the graph rather than to what the file holds; and holds an
  unlistened tap to silence and a DoP stream to `Markers`.
- **A profile holds at most `MAX_WINDOWS` points, whatever a packet claims to span.** The series is
  one point per `WINDOW` second and the windows a packet closes come from the duration its container
  declares, so a `dur` of `u64::MAX` on a sample-accurate timebase asked for 4x10^14 of them. Once
  the series is full the open window is abandoned rather than closed again, which bounds both the
  `Vec` and the walk — on the engine thread, where `Track::sample_packet` feeds the same builder
  every poll, as much as in `probe_stream`.
- **A queued row nothing is playing is read once, off the audio path.** `Player::media` answers out
  of a bounded catalog that a reader thread fills with `probe`; a miss requests the read and answers
  with nothing rather than blocking the caller on a disc, and `Player::media_revision` moves when
  one lands so a 60 Hz poll knows to redraw. It is what lets `resonate-ui` draw a title, artist,
  length and cover for a file no scan has seen while keeping its promise not to depend on
  `resonate-codec`, and what `resonate-mpris` answers `GetTracksMetadata` from — there under one
  deadline for the whole call, because a client may name every row at once. `Player::art` is the
  picture out of the same entry, asked for and landing the same way, and it moves the same revision.
  The two are bounded apart, because a picture is not a row: `ROWS_HELD` caps the entries and
  `ART_BYTES_HELD` what their pictures weigh, so a picture is dropped back to unasked while the tags
  beside it stay. They are asked for on channels of their own and the reader takes a picture only
  when no name is waiting, so a scroll through a queue cannot push `media_within` past the deadline
  MPRIS gives a whole call.
  What is read is a `Row` — a location *and* the span the queue row names — because a dozen rows
  cut out of one file are a dozen readings and one file: the span is what `probe_span` bills them
  apart by, and a row that names no span is the file itself. The two live in one `Entry` under the
  location, the whole file's reading beside a `Cut` per span, so the map is still keyed by
  `Arc<MediaLocation>` and a lookup still borrows rather than cloning a path, the cover is asked
  for once however many rows a sheet cuts, and eviction takes a file with every row of it at once.
  `ROWS_HELD` therefore counts readings rather than entries — `Held::rows` is that count — because
  a queue is what bounds how many spans of one file are asked for and a queue is not bounded.
- **Seeing a row unasked and claiming it are one operation under one lock.** `Claim` is what the
  shelf answers with — the look, or the read being *ours* to ask for — so the window's 60 Hz poll
  and the bus thread cannot both find a location `Unasked` and both enqueue it. The guard never
  leaves `Shelf`: a `match` scrutinee holds its temporary for the whole match, so a `Catalog` that
  locked the shelf itself would deadlock on the very arm that asks, which is why `Shelf::claim_tags`
  takes and drops the lock inside a call of its own rather than handing a guard out.
- **What may be asked for at once is bounded, and a row turned away is asked for again.** The two
  request channels hold `ROWS_ASKED` names and `PICTURES_ASKED` pictures; an ask that would not fit
  answers `Sent::Backlogged` and the claim is *released* — back to `Unasked`, not settled to
  `Nothing` — so the next poll asks for what is still on screen rather than for every row a scroll
  went past. Pictures are bounded tighter than names because a picture costs a read and a copy, and
  a cover that has just come into view should not queue behind a screenful that has gone. Bounding
  the backlog also bounds what `stalest_row` walks past, because a pending entry is one that has
  been asked for and not yet answered.
- **The catalog is ordered by use, not searched for the stalest.** `Held` keeps a `BTreeMap` from
  the use clock to the row and a second one holding only the rows that carry a picture, both re-keyed
  whenever a row is looked at, so evicting a row and trimming a picture are a step from the front
  rather than a scan of all `ROWS_HELD` entries — which `evict_until_one_fits` and `trim_pictures`
  each paid per row dropped, so making room for one large cover cost several. The entries are keyed
  by `Arc<MediaLocation>` so the order maps share the key rather than cloning a `PathBuf` per row on
  every lookup, and `Arc<T>: Borrow<T>` is what keeps a plain `&MediaLocation` the way they are
  read.

## DSP

- **The chain is remix, lossy restoration, resample, equaliser, gain, the true-peak guard and
  dither, and the equaliser's own rules are `eq.md`'s.** It sits after the resampler because a biquad's shape is warped by the rate it is
  designed at, so designing at the source rate would give one profile a different sound per track;
  it sits before the gain because the volume slider is the listener's last word and should
  attenuate a boost. An equaliser in force makes the plan `Converted`, and a DoP-packed stream
  refuses it outright.
- **The resampler's levels are one filter design at four lengths, and High is the default.**
  `Quality::params` is the whole of the difference between them: `half_taps`, the half-width of the
  windowed sinc in source samples; the phases the kernel is tabulated at; the cutoff as a fraction
  of the lower of the two Nyquist limits; and the Kaiser β. The resampler's own `half_width` is
  `half_taps` divided by the ratio when downsampling, so a level costs more taps going down than up.
  The settings pane prints those four numbers on hover rather than describing a level in adjectives,
  which is why `SincParams` is re-exported through `resonate-engine`. The top level but one is
  `High` rather than the `Transparent` it was called, which named what it claimed rather than what
  it is.
- **`VeryHigh` is built to beat SoX's `rate -v` on paper, and the paper is a test.** SoX states its
  very-high setting as 95 % of Nyquist at the −3 dB point, 175 dB of rejection and no aliasing,
  the stopband starting at Nyquist. `VeryHigh` is 320 half-taps under a Kaiser β of 19.5 with the
  cutoff at 0.978: flat within 4 × 10⁻⁹ dB to 95 % of Nyquist and 4 × 10⁻⁶ dB to 96 %, half power
  at 97.6 %, and 185.7 dB down from Nyquist on, measured on the continuous kernel the tables are
  sampled from. `very_high_beats_sox_very_high_on_paper` holds it to better than 10⁻⁶ dB flat to
  95 %, half power at or past 95 % and every point from one to four Nyquists under −180 dB, and
  `each_quality_meets_its_alias_rejection_floor` plays a 15 kHz tone into 22.05 kHz and hears
  nothing above −175 dB, which only an f64 carrier can show. It costs 0.36 % of a core at 44.1 to
  48 kHz, 0.61 % at 96 to 48 kHz and 1.9 % at 192 to 48 kHz natively, about twice High where the
  taps fit the cache and four times where a 192 kHz row of 2 560 weights no longer does, and a
  table is 2.4 ms to build at 44.1 to 48 kHz. SoX's precision figure — 28 bits for `-v` — is the
  word its arithmetic keeps; here that is the f64 the kernel, the history and now the carrier are
  held in end to end.
- **The resampler's phase is a setting of its own, and a shaped phase is designed once a run.**
  `FilterPhase` is `Linear`, `Intermediate` or `Minimum`, SoX's `-L`, `-I` and `-M`, carried on
  `ResamplerConfig` beside the quality, on `EngineConfig` and `OutputSettings`, as
  `Command::SetFilterPhase`, as the `filter-phase` key and `--filter-phase` flag, and as a second
  row in the settings pane's Resampler group. A shaped phase is designed in `phase.rs` from the
  linear kernel of the same quality: the windowed sinc sampled at `SAMPLED_PER_TAP`, 64 points a
  kernel tap, transformed at sixteen times its length, its log magnitude folded through the real
  cepstrum into the minimum-phase response, and — for `Intermediate` — that phase averaged with the
  linear one's own delay, so the result has the same magnitude and half of each. A minimum-phase
  response is kept whole from its first sample; an intermediate one is longer than the linear
  kernel it came from, because averaging a phase is not a polynomial operation, and is kept from
  1.25 half-widths before its peak to two after, which is where it stops costing stopband: cut to
  the linear kernel's own length it held only −125 dB. Both hold the linear design's −185 dB at
  `VeryHigh`, and `a_shaped_very_high_filter_keeps_the_stopband_and_passband_the_linear_one_promises`
  holds them under −180 dB from one to three Nyquists and flat within 10⁻⁴ dB to 95 %. A design is
  kept in `DESIGNED` for the rest of the run, keyed by the `SincParams` and the phase, because it
  is the same prototype whatever the ratio — the kernel's own time is the lower rate's, so only the
  sampling of it moves — and costs 74 ms for `Minimum` and 64 ms for `Intermediate` at `VeryHigh`
  the first time. A prototype is read at any instant through an eight-point Lagrange stencil
  across its samples, exact for a polynomial of that order, which is what lets both the tables and
  the fallback kernel take it where they take the linear kernel's analytic value.
- **The resampler's reach is two numbers, because a shaped kernel is not symmetric.** `Reach`
  holds `behind`, the source frames of history an instant weighs, and `ahead`, the frames it
  waits for, each the prototype's trail or lead divided by the downsampling scale; linear phase has
  both equal to the old `half_width`. The history is primed with `behind` zeros, compacted to
  `behind` before the instant, flushed with `ahead` zeros, and the latency is `ahead` frames, so a
  minimum-phase `VeryHigh` waits seven frames at 44.1 to 48 kHz where linear waits 348, and a track
  is exactly as long whatever the phase — `a_track_keeps_its_length_whatever_the_phase`. An
  impulse through 44.1 to 48 kHz peaks where it was put under every phase, and what it rings
  before that peak is −17 dB of its energy under linear phase, −45 dB at High and −50 dB at
  `VeryHigh` under intermediate, and −78 and −61 dB under minimum, which
  `a_minimum_phase_filter_rings_only_after_what_it_answers` holds to at least 40 and 10 dB under
  linear. What it costs is taps: minimum phase costs what linear does, 0.37 % of a core at 44.1 to
  48 kHz, and intermediate's longer kernel 0.59 % there and 1.5 % at 96 to 48 kHz.
- **The resampler's kernel is stored polyphase, its history planar, and its read position exactly
  rational.** The position is a `whole` sample index and a `phase` numerator over the phase count,
  stepped by integer arithmetic — the step split once into its whole and its remainder, so a frame
  adds and wraps rather than dividing. The count is `SampleRate::ratio_to(output).numer`, every
  phase the conversion can visit: **one** for every integer ratio, 160 for 44.1 onto 48 kHz. An f64
  accumulator stepped by `1 / ratio` smeared those 160 into 1 441 over twenty thousand frames.
  `Tabulated` is what the exactness buys: every phase's weights are built once in `Resampler::new`,
  each the analytic windowed sinc evaluated in f64 at every distinct distance once — the taps behind
  one phase's instant are the taps ahead of its mirror's — and each phase is then divided by its own
  sum, so every phase passes DC at unity and `scale` folds away. Rounded from an f32 table and left
  unnormalised they put a phase-cycle ripple on DC of 3.6 × 10⁻⁴ at Fast, 7 × 10⁻⁶ at Balanced and
  under an f32 step at High; `every_frame_is_the_exact_windowed_sinc_normalised_at_its_instant`
  holds every frame, block boundaries and flush included, to 10⁻¹² of a direct f64 evaluation, the
  frame leaving in f64 rather than rounded to an f32. A table is 340 KB and 0.84 ms to build at 44.1 to 48 kHz, 4 KB and 0.01 ms where there
  is one phase, paid on every `rebind`. `TABULATED_WEIGHT_BYTES_AT_MOST` is 16 MB, which every pair
  of the ten common rates inside the 32:1 range fits at every level — the widest, 22.05 kHz onto
  384 kHz at `VeryHigh`, visits 2 560 phases and holds 13.4 MB — and
  `every_rate_pair_tabulates_its_phases_at_every_quality` pins it. Only a rate no device offers,
  47.993 kHz beside 44.1, still reaches `Kernel`, which holds the windowed sinc in f64 at `phases`
  points a kernel tap and reads each weight off a four-point Lagrange stencil across them, then
  divides the instant's weights by their sum the way a tabulated row is. It is within 10⁻¹³ of the
  analytic kernel everywhere but the last stencil before the window's edge, where the Kaiser window
  stops at `1 / I₀(β)` rather than at nothing and the stencil straddles that step: 2 × 10⁻⁸ at High,
  under High's own stopband, and 10⁻¹¹ at `VeryHigh`, which
  `an_interpolated_kernel_stays_within_the_step_its_window_ends_on` holds each to.
  The history is one f64 plane per channel, converted once as it arrives, and a frame's channels
  share one pass over the weights: in pairs, each weight loaded once for both, an odd channel alone.
  Each channel keeps `ACCUMULATORS`, eight, independent lanes on every target: a pair already runs
  two add chains, so sixteen and thirty-two measured slower on AVX-512, the longer lane sum costing
  more than the latency it hides, and sixteen spill SSE2's sixteen registers. What the loop waited
  on was the cache — every 64-byte load straddled two lines — so the planes, the lanes and the table
  are aligned to `VECTOR_BYTES`, a frame's blocks start at the aligned sample at or below its first
  tap, and every row carries `LANES_PER_VECTOR` leading zeros so its weights slide under the aligned
  history. Rows end in zeros to a whole block and `compact` zeroes what it vacates, so what is read
  past `filled` is exact zeros and no scalar tail is left. The lanes pass through `lanes` before they
  are summed because, summed in registers, LLVM's SLP vectoriser paired the two channels two-wide off
  their adjacent output stores instead of packing each channel's eight into one vector. High costs
  0.18 % of a core at 44.1 to 48 kHz and 0.27 % at 96 to 48 kHz natively, against 0.20 % and
  0.39 %, and 0.25 % and 0.41 % on the packaged `x86-64` build, against 0.31 % and 0.53 %. The
  alias tests skip `latency_frames` at both ends as well as their margin: a tone switched on against
  a silent history rings that long, and at 24:1 the ringing was the whole of a −105 dB once recorded
  there. The 24:1 test plays 21 kHz, because 20 kHz folds onto the 4 kHz output Nyquist, where every
  output instant is a zero crossing and nothing is measured.
- **The dither stage has a flat path and a shaped one, works in f64, and the plan's curve picks
  between them.** `Dither::prepare` resolves `NoiseShaping::at` the output rate once and keeps what
  survives as the stage's `applied`, and `process` matches on it: `NoiseShaping::None` walks the
  samples and requantises, with no error history to read or write, and neither the coefficient nor
  the history `Vec` is allocated at all. Wherever no curve survives the output rate that is every
  sample of every channel saved a feedback sum and a history write. A sample is worked in steps of
  the target grid in f64 — widened, dithered, rounded and, on the shaped path, its error fed back —
  and handed on in f64, where every grid value up to thirty-two bits is exact. In f32
  the noise added to a 24-bit sample near full scale was itself rounded to two levels a step, which
  left a half-step input biased by a quarter of a step;
  `a_24_bit_word_near_full_scale_is_dithered_without_bias` is the claim. A uniform draw takes 53
  bits and triangular is the sum of two. The rounding adds and subtracts 1.5 × 2⁵², so IEEE rounds
  the sum half to even, the same trick `resonate-core`'s conversions use and for the same reason:
  `round` is a libm call per sample on the packaged `x86-64` baseline. The output is clamped to
  `[-1, 1 - step]`, but the error fed back is taken before the clamp, so a clipped sample cannot run
  the loop away — and an error that is not finite, or lies past what the dither's own peak and half
  a step can make (`DitherKind::largest_error_steps`), is fed back as nothing, because one NaN, one
  infinity or one wild float sample, where the 1.5 × 2⁵² rounding is no longer exact, would
  otherwise ride the feedback for the rest of the track. The check is one comparison on the
  magnitude, false for NaN. Every real sample's error is inside it, which
  `an_error_is_fed_back_whole_wherever_the_input_is_a_real_sample` holds against an unguarded loop
  for every dither kind, and
  `one_wild_sample_is_forgotten_rather_than_fed_back_for_the_rest_of_the_stream` shows NaN, both
  infinities and 10³⁰ each forgotten the same way, every sample after them finite, on the grid and
  in range. The wild sample itself still goes through the clamp, so an infinity lands on full scale,
  and a NaN passed on is silence once the integer conversion reads it. The shaped path is one
  function generic over the tap count, so Lipshitz's five taps and Threshold's twelve are each
  unrolled, and it copies the coefficients out of their `Vec` before the loop, which is what lets
  LLVM hold them in registers rather than reload them past every history write it cannot prove does
  not alias them. Each channel's errors are held twice over in a ring twice the tap count long, the
  newest written at one slot and at that slot plus the tap count, so the most recent errors are
  always one contiguous window read back one element at a time: shifting the whole history instead
  read it back next sample through one wide load spanning the narrow stores just made, which no
  store can forward to. The ring strides by the curve's own tap count, so one channel's error cannot
  reach the channel beside it. The match is on the enum rather than on an empty coefficient slice,
  so a curve added to `NoiseShaping` has to say which path it takes. At 48 kHz stereo the flat path
  costs 0.022 % of a core, Lipshitz 0.033 % — 0.045 % while its history shifted in f32 — and
  Threshold 0.039 % natively, and 0.024 %, 0.038 % and 0.052 % on the packaged `x86-64` build. The
  guard is most of what separates Lipshitz from the 0.028 % it cost without one: under AVX-512 the
  comparison becomes a mask on the loop-carried path. Moving the forgetting out of line into a cold
  call takes it back off that path natively, but costs Threshold, the default, more on the packaged
  build than it saves, so the guard stays a plain select.
- **A 32-bit target is dithered like any other.** Its grid is 2⁻³¹, exact in the f64 the chain now
  carries and far inside what the 1.5 × 2⁵² rounding reaches, so `Dither::new` takes every depth
  and cannot fail, and `plan_for` dithers an S32 stream wherever something narrows or converts: a
  resample, a gain, a float source. An integer source an S32 word holds whole is still `Repacked`.
  While the carrier was f32 the grid was finer than the carrier itself, so the plan refused it and a
  32-bit device was fed samples no dither had covered; `a_32_bit_word_is_dithered_on_its_own_grid_without_bias`
  is the claim, down to every word the conversion writes landing back on the grid value it came from.
- **Lipshitz runs only at the rates it was designed for; Threshold is designed at the rate it runs
  at.** The Lipshitz coefficients are an E-weighted fit to the ear's sensitivity at 44.1 kHz; at
  96 kHz the same taps put the notch in the wrong place, so `NoiseShaping::at` resolves it to flat
  at every rate but 44.1 and 48 kHz. `Threshold` is an error-feedback filter `Dither::prepare`
  designs for the output rate, so it shapes at every rate. The weighting is Terhardt's (1979)
  threshold in quiet, held at its 1 kHz value below 1 kHz so that a low output rate does not push
  noise into the bass, and clamped to `THRESHOLD_RANGE_DB`, 42 dB, above its quietest point, taken
  as a power over 2048 midpoints of the band. Its autocorrelation to lag 12 comes from the
  Chebyshev recurrence on each point's cosine, four points at a time so the recurrence runs across
  points rather than serially down one, and Levinson–Durbin turns it into the order-12
  prediction-error filter `A(z)`. The stage feeds back `c = −a`, so its noise transfer function is
  `A` itself — the Lipshitz constants read the same way, their `A` being `1, −2.033, 2.165, …` — and
  `A` is minimum phase by construction. Weighted by the unclamped threshold over 20 Hz to 20 kHz,
  the noise Threshold leaves is 16.6 dB below flat dither at 44.1 kHz and 18.2 dB below at 48 kHz,
  where Lipshitz leaves 9.4 and 10.5; 25.9 and 26.5 dB below at 88.2 and 96 kHz, 30.7 and 30.8 dB
  at 176.4 and 192 kHz and 31.4 dB at 384 kHz. At 22.05 kHz and below there is little band above the
  ear's reach to move noise into and it gains 1.2 to 2.4 dB, and at 32 kHz 9.7. What it lifts the
  noise to at its peak stays under 30 dB at every rate from 8 to 384 kHz — 27 dB at 32 kHz, 19.5 at
  44.1, 10.2 at 96, 6.9 at 192 — where Lipshitz peaks at 19.4. The tests hold the design to the
  numpy prototype's coefficients at 44.1 and 96 kHz within 10⁻⁹, recover its reflection
  coefficients by the step-down recursion to show every one is under one in magnitude at every rate
  in that span, and quantise a quiet sine at 96 kHz to watch the error fall 26 dB at 1 kHz and
  33 dB at 4 kHz and rise 9 dB at 40 kHz against flat. A design costs about 38 µs at 44.1 kHz and
  55 µs at 192 kHz natively, 42 and 64 µs on the packaged build, paid in `prepare` on every rebind
  and every reshape that builds a dither stage; the transcendentals in the threshold and the
  per-point cosine are most of it. `plan_for` stores what survives in `OutputPlan::shaping`, the
  dither stage applies the plan's curve rather than the configured one, and `resonate explain`
  prints it beside the target depth, so a Lipshitz fallback to flat dither is visible rather than
  silent. `EngineConfig` defaults to `Threshold`, so a 16-bit device at any rate gets shaped dither
  out of the box, and so does a 24-bit device behind a volume below full. `--dither` and
  `--noise-shaping` are the command line's way to say otherwise, `dither` and `noise-shaping` the
  config file's, and the settings pane's Processing category the window's.
- **`OutputMode` names what the plan does to the signal, and a shorter word length is not a
  resample.** Four readings, decided in `plan_for` and drawn as the chip the playback bar and the
  inspector carry: `BitPerfect` where the stream is the source's own triple; `Repacked` where the
  target holds every source value losslessly and only the container is wider; `Dithered` where the
  rate and the channel map are the source's own and only the word length is shorter, with dither
  covering the difference; `Converted` for everything else — a rate or a layout that changed, a
  gain stage, an equaliser, or a narrowing with the dither turned off, which nothing covers. A
  24-bit 48 kHz file on a 16-bit 48 kHz device is the `Dithered` case, and reading it as a
  resample was wrong twice over: nothing resamples, and `no_convert` is `mode != Converted`, so
  the graph was being asked to convert a stream that already matched the device. The order of the
  decision is load-bearing — `dithers` is resolved *before* the mode, because a narrowing the
  listener switched the dither off for is a truncation and must not answer to a name that says it
  was covered.
- **The channel layout is negotiated like the rate and the format, and one matrix is the whole of
  the downmix.** `plan_output` asks `SinkInfo::best_spec_for` for a rate, a format *and* a layout
  together rather than for each in turn, so the plan can never name a triple the sink did not
  advertise as one — `crates/resonate-pipewire/src/sink.rs` asserts that over the cross product, and
  `crates/resonate-engine/tests/transport.rs` plays a 5.1 file into a stereo sink and a 5.1 sink and
  watches what the graph is handed. The fit is lexicographic over rate, then channels, then format:
  keeping every channel beats keeping the depth, because a fold loses a whole signal where a repack
  loses bits nothing recorded. `resonate-dsp`'s `Remix` is the stage the plan asks for, and it runs
  first, so the resampler and the dither cost the channels the sink takes rather than the ones the
  file carries. Its matrix is built from `ChannelPosition`: a channel the target also has is copied
  at unity, one it lacks folds into its nearest neighbours at -3 dB — a centre spreads into both
  fronts, a rear into the side then the front — the LFE is dropped rather than folded, and a target
  channel nothing feeds stays silent, so an upmix invents nothing. Each row is then scaled so the
  gains along it sum to at most one, which is why no downmix can clip a full-scale source and why
  5.1 into stereo lands about 7.7 dB down. That is the same choice as the clip-prevention note
  below: attenuate rather than clip.
- **The chain is carried in f64 from the decoded word to the sink's.** `Processor::process` and
  `flush` take `f64` slices, and the engine widens exactly what the decoder handed it into
  `Output::widened` through `SampleData::widen_into` before the chain sees it, then narrows the
  carrier once, into the sink's word, in `Output::stage`. An f32 carrier held 24 bits of mantissa:
  a 32-bit integer source lost its low byte on the way in, and a 32-bit sink was fed at most about
  twenty-five bits however well every stage worked inside. Every stage already worked in f64 — the
  resampler's history and weights, the equaliser's sections, the dither's grid — so the carrier was
  the one place the precision was thrown away, and moving it cost nothing measurable: the chain from
  5.1 at 44.1 kHz to stereo at 48 kHz through the resampler, ten bands, the gain and the dither is
  0.297 % of a core in f64 against 0.296 % in f32, and narrowing a block to S16, S24 or S32 costs
  0.001 %. `ChainBuilder::input` still names the carrier `SampleFormat::F32`, because the spec is
  a negotiation vocabulary with no wider float, and the stages read only its rate and channels.
- **Lossy restoration is a spectral stage for MP3, AAC and Vorbis alone, off unless asked.**
  `Restoration` is `Off`, `Repair` or `Extend`, the `restore-lossy` key and the settings pane's
  *Lossy sources* group, and `Restore` is pushed after the remix and before the resampler, so it
  works at the source rate where the encoder cut. `Decoded` carries the source's `Tuning` — the
  engine maps `Codec` onto one in `tuning_of`, naming the lossy codecs rather than trusting
  `is_lossless`, which calls `Unknown` lossy too — and the lowpass wall the study found, off
  `TrackHints`; a lossless source never gets the stage whatever the key says, and one it does get
  makes the plan `Converted`. The stage is a weighted overlap-add over sqrt-Hann frames of 1 024 at
  44.1 or 48 kHz, scaled with the rate, a quarter frame apart, planned and allocated in `prepare`
  and run through `process_with_scratch`; it holds back its first `frame − hop` frames rather than
  emitting a silent pre-roll and hands them on in its flush, so a track keeps its length, and with
  nothing to restore it gives back what it took to within 10⁻¹² —
  `with_nothing_to_restore_the_stage_hands_back_what_it_took_exactly_and_whole`. What it does:
  - **The wall** is the study's where there is one and it lies in the codec's plausible range;
    otherwise it is found as the music plays, from a three-second average spectrum read in 100 Hz
    bands once a second and a half has been heard: the highest band whose kilohertz below stands
    `WALL_DB`, 30 dB, over everything from 200 Hz above it to the top, and is within 50 dB of the
    loudest band from 2 to 10 kHz, settled down to the last band still within 10 dB of that
    kilohertz. Once found it is held for the track. A 320 kbps MP3 transcoded from a 16 kHz
    source is read at 15.8 kHz and a 192 kbps LAME encode at 18.6 kHz.
  - **Repair** lifts the droop an encoder's own lowpass leaves under its wall — a quadratic shelf
    of `Tuning::droop`, 2 dB over 1.5 kHz for MP3, 1 dB over 1 kHz for AAC, 1.5 dB over 1.2 kHz
    for Vorbis — and fills a hole: a run of at least four bins from 4 kHz to the wall that falls
    `deeper_than_db` under its own median of the last nine frames, in a frame that as a whole
    still holds within 6 dB of that median, filled with noise at `filled_below_db` under it. The
    median is what keeps a transient's splatter from reading the frames after it as holes, and
    the whole-frame test is what keeps the end of a track from being filled —
    `a_transient_is_not_taken_for_a_hole_in_what_follows_it`. A dropout is filled for its first
    few frames and a band that has really gone is let go.
  - **Extend** also rebuilds the band from the wall to 97 % of Nyquist from the same width below
    it, bin for bin, so a transient keeps its time: each patched bin is scaled to a line starting
    6 dB under the average just below the wall and falling 12 dB an octave faster than the slope a
    fit of the octave under the wall gives, never louder than 3 dB under its own source, faded in
    over the first 500 Hz. On the transcode above that puts 16 to 21 kHz at −18 to −26 dB where the
    file held −62 to −69, 20 to 28 dB under the band at 10 to 13 kHz.
  The codec's curves live in `Tuning` and are applied in the spectrum rather than handed to the
  equaliser, because they follow each track's own wall and must not move the profile a listener
  bound to a device. The stage costs 0.57 % of a core at 44.1 kHz stereo; it adds `frame − hop`
  frames of latency, which `Output::hand_on_what_the_chain_holds` flushes before a swap the way it
  does the guard's.
- **A `Chain` carries a channel width per stage, not one for the chain.** `Remix` changes the frame
  width mid-chain, so the builder records `output_spec(spec).channel_count()` per stage and sizes the
  two scratch buffers by the widest *sample* count rather than the widest frame count. Everything
  downstream of a fold therefore slices the scratch by the width it actually has.
- **A chain drains in one flush, and a flush with less room than that is refused.** The builder
  sums what every stage can still hand on once its input ends into `Chain::max_flush_frames`, and
  `Chain::flush` answers `Error::OutputTooSmall` for a destination shorter than that rather than
  filling what fits. One flush is the whole tail on purpose: a resampler pads half its filter and
  emits up to the last real frame, which keeps a track exactly its own length, and a second flush
  would only hand on the filter's ringing past the end. `Engine::flush` sizes the carrier to
  `max_output_frames`, which covers the bound, so a stage that later advertises less than it
  drains fails loudly — an error record and a track ended without its tail — rather than cutting
  every track short in silence.
- **Clip prevention attenuates the track where it knows the peak; only an over it could not see
  is ridden down.** A ReplayGain boost is capped at the point the track's declared peak would reach
  full scale, so the waveform keeps its shape. `AppliedGain` in `resonate-core` pairs the gain with
  the peak the mode selected and owns that arithmetic, so the DSP stage, `resonate info` and the
  inspector cannot disagree about what was applied. `AppliedGain::heeding` folds a measured true
  peak in beside the declared one and keeps the larger, so a track whose study says it passes full
  scale between its samples — or on them, as a lossy decode often does — is turned down by exactly
  that much even with ReplayGain off, and the plan grows a gain stage for it; `true-peak` off
  leaves the declared peak alone. The per-sample clamp survives only where a *boost* — an amplitude
  over one — has no peak at all, which is the one case with nothing better to go on. At unity or
  below a clamp can only clip overs that were already in the signal, and that is the guard's to
  answer, so `GainConfig::limits_every_sample` reads the amplitude as well as the peak and
  `GainStage` asks it again wherever its target moves: `set_gain` and a ramp handed a start. A
  boost capped at exactly unity — a peak-normalised track declaring 1.0 — changes no sample, so
  `AppliedGain::adjusts` answers false, the plan asks for no gain stage of its own and, with nothing
  else to do, the track plays bit-perfect rather than dithered at unity and labelled `Converted`.
- **What the catalog studied reaches the player through `Hinting`, beside `StandIn` and not gated
  on the vault.** `resonate-core::TrackHints` is the vocabulary — the measured true peak and the
  lowpass wall a study found — because the codec's `Sources`, the library and the engine all need
  it and none may see the others. `Sources::hinted_by` registers a `Hinting`, `Library::hinting`
  answers it from `track_studies` by the row's own path and span, and `playing_from` registers it
  on the player's sources wherever there is a catalog, vault or not. `Track::open` asks once, keeps
  the answer on the track and folds it into the gain in `levelled`, which `re_level` and
  `Command::SetTruePeak` run again; a row with no study answers nothing and plays as it did.
  `a_track_the_catalog_measured_past_full_scale_is_turned_down_under_it` plays a file the hints
  say peaks at 2.0 and hears it at half level, and at full level with the key off.
- **The true-peak guard is a lookahead gain, not a clipper, and it touches nothing under the
  ceiling.** `resonate-dsp`'s `TruePeak` is pushed after the gain stage and before the dither
  wherever `true-peak` is on and the plan converts — any stage, or a float source narrowed to an
  integer word — and never on a bit-perfect or repacked stream. It reads each frame at eight times
  the rate, and wherever a phase passes −0.1 dBTP it asks for the gain that would bring it under;
  the smallest asked across 1.5 ms of lookahead is averaged over the same span, so the gain has
  fallen to what the peak needs by the frame the peak is on, and it recovers with an 80 ms release
  that snaps back to exactly one within 2⁻³² of it. A stream that never passes the ceiling is
  multiplied by exactly one and comes out bit for bit what went in, only later —
  `a_stream_that_never_nears_full_scale_passes_through_exactly_and_whole`. The audio waits
  `lookahead + 16` frames, which it holds back rather than padding with silence and hands on in its
  flush, so a track keeps its length. Because it holds frames, `Engine::swap_chain` first flushes
  the running chain's held tail into the ring — `Output::hand_on_what_the_chain_holds` — and
  defers the swap to the next pump where the ring has no room for it, so a reshape between a
  guarded chain and another never drops what the guard was holding. The detector keeps the eight
  phases' weights transposed so one pass over the window accumulates all of them, and counts the
  ring entries under unity so the common case — nothing over — is constant time: 0.23 % of a core
  at 48 kHz stereo.
- **The pre-amp and the gain of an untagged track are part of what ReplayGain asks for, so clip
  prevention weighs them too.** `Levelling` is a `Trim` for each — quantised millibels, so
  `OutputSettings` keeps its `Eq` — and `resolve_replay_gain` folds them into the `AppliedGain` it
  answers: a tagged gain is raised by the pre-amp and keeps its tagged peak, so a boost the pair
  would push past full scale is capped at the peak like any other; a track that declares no gain
  takes `untagged` with no peak, which is the one case the per-sample limiter is for; and with
  ReplayGain off neither moves anything. `GainConfig` lost the `pre_amp` it carried, which nothing
  had ever set. **A track that declares no gain and that the catalog studied is levelled by what
  was measured instead.** `TrackHints::measured` is a `MeasuredGain`: `Hinted` answers the track's
  gain as ReplayGain 2.0's −18 LUFS reference less the study's integrated loudness, and the
  album's from the energy mean of every track's loudness weighted by its length — only where
  every track of the album, alternatives aside, has been studied, because an album gain from half
  a record is a track gain in disguise. `engine::tagged_or_measured` hands `resolve_replay_gain`
  those two only where the tags carry neither gain, so a tagged file is never second-guessed, and
  the peak is the study's true peak through `heeding` as before. It is what gives a delivered row
  a gain, the vault having stripped the tags it came with; `untagged` is now what a track neither
  tagged nor studied is played at. `resonate explain` reads no hints and still resolves the tags
  alone. `replay-gain-pre-amp` and `replay-gain-untagged` are the config keys, in decibels,
  and the settings pane's ReplayGain group offers them as two chip rows under the mode.
- **The plan is the one judge of a gain stage, a converting plan always carries one, and the engine
  chooses its fill branch from the plan.** `gain_config` answers `None` at full volume under a
  unity gain, and `plan_for` asks for a stage anyway wherever the plan already resamples, remixes or
  equalises — at unity, because such a chain is not bit-perfect whatever its gain, and a stage that
  is already there makes every volume and ReplayGain change a retune rather than a change of shape.
  Only a plan with no other stage leaves it out, which is what keeps a bit-perfect path
  bit-perfect, and `dop_survives` still reads `gain_config`, so DoP is refused only where a gain
  would change samples. `GainStage::is_transparent` answers false, so the builder keeps every gain
  stage the plan pushes — the shape `eq_config` and the equaliser already have. `Engine::fill` and
  `Engine::flush` pick the byte copy or the conversion from `OutputPlan::is_transparent`, the
  reading `delivery()` asks the decoder for its format from, so the two cannot disagree: a chain
  that came out empty in a converting plan costs a format conversion rather than raw f32 bytes in an
  integer ring.
- **Every float reaching an integer word is rounded to nearest, ties to even, and saturated to the
  format's range, and one set of conversions in core is the whole of how.** `SampleData::write_f64`
  is what `Output::stage` narrows the chain's carrier through on its way to the ring, and
  `SampleData::write_f32` is its narrow twin that `convert_into` and `retype` use and the DSD
  decimator hands the decoder its samples through, both over the same rounding, so none of them
  can disagree about a step. A narrowing nothing dithers — the dither switched off, or a float
  source reaching an integer word at its own rate — is `OutputPlan::rounds`, which makes the plan
  non-transparent with an empty chain, so the samples reach `write_f64` rather than symphonia's
  arithmetic shift, which floors, or its truncating cast. Truncation left a dead band a step wide around zero and
  pulled each sample half a step towards it, and on the paths nothing dithers — S16, S24 or S32
  with the dither off — nothing covered it. The scale is `SampleFormat::full_scale` both
  ways, so every S16 and S24 value survives a trip through f32 exactly. The rounding adds a
  constant big enough that IEEE rounds the sum to a whole number, half to even, and reads it back
  out of the bits, rather than calling `round_ties_even`: the packaged `x86-64` baseline has no
  `roundps` and made that a `rintf` call per sample, where the addition vectorises on SSE2, which
  the saturating `as` it replaced never let the loop do.

## The sink

- **The client outlives its daemon.** Everything a connection holds — the core, the registry, their
  listeners and the proxies bound through them — is one `Graph`, and `Reaching` is what makes one:
  the loop keeps its main loop and its context for the life of the process and connects a core
  through them as often as it has to. The core's `error` event with a broken pipe on the core
  itself is the daemon gone; it sends `Request::Lost` through the loop's own channel, because the
  connection cannot be torn down inside one of its own callbacks. `Lost` drops the streams and the
  graph, answers every pending `Sync` by dropping it, empties `Discovered` and announces each sink
  it knew as removed, and a thread of its own sends `Request::Reconnect` a second later, again
  until a core connects. While there is none, a `Sync`, an open and a capture answer
  `Error::Disconnected` at once rather than timing out as `LoopStopped`, and `PipeWire::unanswered`
  tells the two apart by the `connected` flag the loop keeps. What comes back is announced the
  way it was the first time, as the registry hands the globals over again.
  `crates/resonate-pipewire/tests/reconnect.rs` is the claim, and it needs no daemon of the
  session's: it runs its own binary again under `PIPEWIRE_RUNTIME_DIR`, starts a `pipewire` of its
  own with a null sink and nothing else, kills it under an open stream, asserts `Disconnected`,
  starts it again and opens a stream on the sink it finds.
- **The chosen sink is named, not numbered.** `EngineConfig::sink` and `Command::SetSink` carry a
  `NodeName`, which `select_sink` matches on every stream open, because a PipeWire id is assigned
  per object and a device that is unplugged and put back carries a new one. `OutputStatus::sink`
  stays a `SinkId`: it reports what was actually opened. A name nothing in the graph answers to
  warns and falls back to the default until the device turns up, which is what lets a device missing
  at startup bind as soon as it appears — and what makes a typo play on the default device with a
  warning rather than refuse to start.
- **Whether a sink is hardware is a question about the `Device` above it, not about the node.** A
  `Node` global's properties carry `device.id` and not `device.api` — the api is on the `Device` the
  id names — so reading it off the node alone made a real USB DAC read as something the graph had
  made up, and the field had no honest reader for exactly that reason. `Discovered::driven` is the
  set of device ids whose global names a driver, `SinkRecord::device` is the id the node points at,
  and `Discovered::is_hardware` answers for the pair at snapshot time rather than at insert, so the
  two globals may arrive in either order. `resonate sinks` draws it as *DRIVEN BY*, a device or the
  graph, which is what tells a real DAC from a null sink, a loopback or an echo canceller.
- **Which physical port a sink comes out of is the `Device`'s to answer, and the seat is the
  join.** A card's `Route` params name its ports; a sink node's `card.profile.device` names the
  seat on that card it plays through, and `DevicePorts::serving` is the pair — so a sink is drawn
  as *Headphones* or *HDMI / DisplayPort 2* rather than only by the description its node carries.
  The seat is read off the node's `info` event rather than its registry global, because the global
  carries `device.id` and nothing about the profile; the same event is what follows a card
  switched to another profile. Both route params are read and kept apart: `Route` is the port the
  card is *switched to* and outranks the rest, `EnumRoute` is every port it offers — and the
  enumeration is the only place an unplugged port appears at all, because a card publishes no
  current route for one. That is exactly the case worth drawing, so `Plugged` is the reading —
  `Yes`, `No`, or `Unsaid` where the driver does no jack detection, which a USB adapter usually
  does not — and `resonate sinks` says *nothing plugged in* beside the port's name. Each is kept
  under the param index the way a node's formats are, so an enumeration overwrites the last rather
  than piling onto it.
- **The card says which output it is switched to, and the port says whose volume it is.** The
  `Device` subscribes to `Profile` beside its two route params, and `parse_profile` reads the one
  the card currently holds into `Discovered::profiles` — keyed by device id, looked up through
  `SinkRecord::device` exactly as `port_of` looks up the port — so `SinkInfo::profile` is drawn as
  *Analog Stereo Output* rather than being a thing only `pw-dump` could answer. It is a single
  current profile rather than the `EnumProfile` catalogue, because what is worth drawing is what
  the card is doing and not what it could do. `route.hw-volume` needed no new subscription: it is
  a string pair inside `SPA_PARAM_ROUTE_info`, a `Value::Struct` of a count followed by
  alternating keys and values, which `parse_route` was already handed and was throwing away.
  `HardwareVolume` is the reading — `Yes`, `No`, or `Unsaid` where the route says nothing, which
  is a different answer from `No` and must not be drawn as one, because a driver that is silent
  is not a driver claiming the volume is software's. `resonate sinks` gives the profile a column
  of its own and appends the volume to the port cell; the settings pane chains both onto
  `advertised`, so a device line says what it is switched to and where its volume is applied.
- **The latency a stream reports is counted in the stream's own frames, and the conversion happens
  where the graph's tick rate is known.** `pw_time.delay` is the delay to the device expressed in
  `pw_time.rate` — the graph's clock, not the stream's — so a graph running at 48 kHz under a
  44.1 kHz stream reports ticks that are 8.8 % short of frames, in the position readout and in the
  track-boundary fallback alike, which is precisely the converted path this player exists to make
  visible. The `process` callback holds the only consistent snapshot of the delay and its rate
  together, so it builds a `GraphTime` and publishes `downstream(spec.rate)` as one `AtomicU64` of
  stream frames. Storing the pair instead would mean two atomics and a torn read; converting later
  would mean a lock the RT thread may not take. `SinkStream::latency` is therefore `Frames` and
  reads as what it is, rather than a `delay` in a domain its name does not say. What the stream
  still holds is counted beside the delay: `GraphTime::buffered` is `pw_time.buffered`, the
  resampler's own frames, already in the stream's rate and so added rather than converted. What is
  still missing is `pw_time.queued`, the sum of `pw_buffer.size` over the queued buffers, because
  pipewire-rs offers no setter for that field and writing it would take `unsafe`.
- **The callback fills the quantum the graph asked for, not the buffer it was handed.**
  `pw_buffer.requested` is that quantum in frames and `Cycle::asked_for` turns it into bytes,
  clamped to the room the pool gave and falling back to the whole of it where the graph names
  nothing — which is also what the packed DoP path lands on whenever the padded words it fills
  from would not fit in a buffer sized for the three-byte wire form. On this machine the graph
  asks for 1024 frames and the pool hands over 12288, so the ring was being drained twelve quanta
  at a time and an underrun was declared against twelve quanta of want. Both now follow the
  graph.
- **Silence about a capability is not a refusal.** A node that answered `EnumFormat` with no
  channels property, or with no formats at all, is believed about what it did say and left alone
  about the rest: the candidate list falls back to the source's own layout, or to `allowed_rates`
  crossed with the source's format. `SinkInfo::supports` stays strict, because it answers a
  different question — whether the device advertised this exact spec — and that is what the DoP
  path, `resonate explain` and the settings pane read.
- **A sink's advertised formats and the graph's `allowed_rates` are separate fields.** A device that
  advertises 192 kHz is irrelevant if the daemon will not switch the graph to it; conflating them
  would make the bit-perfect claim unfalsifiable.
- **Twenty-four bits are one depth and two words, and which word goes on the wire is the graph
  boundary's alone.** `SampleFormat::S24` is sign-extended in the low 24 bits of an `i32`, so it is
  four bytes everywhere the workspace touches it — the ring, the chain, `bytes_per_frame`, the
  staging buffer. A device may want three. `format::WireWord` is that difference and it lives in
  `resonate-pipewire`: `sample_format` reads `S24LE` and `S24_32LE` as the same depth, `one_of_each`
  keeps the depth once so a device offering both is one row rather than two, and `spa_format` takes
  the word as a second argument. `build_stream` offers both as two `EnumFormat` params, the packed
  one first because a 24-bit DAC's own default is usually the packed word and a conversion avoided
  is the whole point of the bit-perfect path; a peer that will not take it settles on the second.
  What the graph actually chose comes back through `param_changed`, which stores it on an
  `AtomicBool` the callback reads — the same shape the reported latency already uses, because
  `process.rs` may hold no lock.
- **Packing is done in the graph's own buffer, forward, with nothing allocated.** `process::pack`
  reads the low three bytes of the word at `4i` and writes them at `3i`, and `3i + 3 <= 4i + 3` for
  every `i`, so the write never reaches a word that has not been read yet and no scratch buffer is
  needed — which is what keeps it inside the callback's no-allocation rule. The ring still fills
  the buffer at the padded stride and `Silence::write` still lays its DoP markers down as four-byte
  words; packing keeps bytes 0 through 2, and the marker is byte 2, so a DoP carrier survives it.
  `chunk.stride` and `chunk.size` are then the *wire* frame and the packed length, not the ring's.
  Without this a 24-bit master reaching a device that advertises `S24LE` and `S16LE` and nothing
  else was dithered to 16 bits, because the enumeration dropped the only 24-bit word the device
  named.

## Fixtures

- **A test that asserts on which row is playing pauses the transport first.** The fake graph is
  pulled by the test thread and by nothing else, so a queue cannot advance while nobody pulls — but
  that is a property of the harness rather than of the assertion, and a decode that fails under a
  loaded machine reaches `Engine::fail`, which skips. `stand_still` is the pause, and
  `the_queue_the_engine_publishes_is_the_order_it_will_play` and
  `editing_the_queue_leaves_the_playing_track_alone_until_its_own_row_goes` take it before they read
  a row and again after any command that starts the transport — `JumpTo` is one, and `Play` is what
  the second uses to prove a bind, because removing the playing row while paused now defers it.
- **The real-file fixtures are built by ffmpeg at test time and the suite skips without it.**
  `crates/resonate-codec/tests/encoded.rs` reaches 24-bit 96 kHz and 24-bit 5.1 through FLAC and
  ALAC, 48 kHz stereo through AAC, Vorbis and Vorbis-in-Matroska, 22.05 kHz mono through MP3, and
  24-bit 96 kHz through AIFF and CAF. It is also what exercises `probe_stream`'s container walk and
  the ISO-BMFF box order.
- **Two fixtures go through the encoder a ripper actually uses, not ffmpeg.** An ffmpeg tone carries
  none of the furniture a real file has, so `flac` and `metaflac` build a rip with a `SEEKTABLE`, a
  `VORBIS_COMMENT`, a `PICTURE` and a `PADDING` block — the block list is asserted, so the day the
  reference encoder stops writing one the suite says so — and `lame` builds an MP3 with an ID3v2 tag
  to skip and a Xing/LAME header whose priming reaches `encoder_delay`. Each gates on its own tool
  and prints a skip without it, as the `ffmpeg()` gate does.
- **One fixture is written by the suite, because ffmpeg will not write it.** ffmpeg's CAF muxer
  refuses AAC, so `a_caf_rip_drops_the_priming_and_the_remainder_its_packet_table_declares` takes the
  ADTS stream ffmpeg *will* write, reads its frame lengths off the headers and lays the raw packets
  into a `desc`, `pakt` and `data` chunk itself, the way `afconvert` would. The `pakt` chunk is the
  whole point: its `priming_frames` and `remainder_frames` are the declaration the CAF reader turns
  into `Track::delay` and `Track::padding` while putting no trim on a packet at all. No `kuki` chunk
  is written and none is needed — symphonia's AAC decoder falls back to the rate and channel count
  the `desc` chunk already carries.
- **`RESONATE_REAL_FIXTURES` points the suite at a folder of real files.** A CD rip and an iTunes
  purchase cannot be committed, so the only honest way to have them in the loop is to read the ones
  the person running the tests already owns: set it and every file under it must probe, name a
  container and a codec this build knows, report a duration and decode its first block. Unset, it
  prints a skip like every other gate.
  - **The transport and bus tests drive a real `Player` thread and poll for the state they expect**,
  so a state that never arrives costs the full patience before it fails. The failure names the
  transport it was watching and, in the transport tests, what the graph had pulled.
