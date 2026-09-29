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
  symphonia leaves `bits_per_sample` unset for ALAC, so `container.rs` takes the depth from the
  magic cookie in `extra_data`; without it a 24-bit ALAC claims 32 bits and `plan_output` asks the
  sink for a format the file never had — a converted path where the same music in FLAC stays
  bit-perfect. `declared_bits` asks every container that answers, nearest the codec first: the coded
  width symphonia declares, a WAV's `wValidBitsPerSample`, FLAC's `STREAMINFO`, Matroska's
  `BitDepth`, and only then `bits_per_sample` — the *decoded* width for most readers and why the
  chain exists (ALAC's cookie is a further fallback). `wValidBitsPerSample` is how a
  `WAVE_FORMAT_EXTENSIBLE` file says it holds 20 bits in 24-bit words, symphonia reporting the 24;
  the depth a file was made at is what the inspector draws and `resonate info` prints, so reading
  the padding is a lie about the master.
- **A layout is named only where the container places every channel where that layout does.**
  `positioned_layout` weighs symphonia's speaker mask against the named layouts — the side pair and
  the rear pair each counting as a 5.1's or quad's surrounds — and anything else, a 6.0 or an LCRS,
  is `ChannelLayout::Discrete` rather than whichever layout shares its count, since a downmix reads a
  5.1's fourth channel as the LFE and drops it. One and two channels are mono and stereo wherever
  placed (symphonia puts a mono MP3 on the front left); an unplaced set of more is `Discrete`.
- **A tag is read under the naming its writer used, not only its container spec's.** symphonia maps
  a raw key through the container's vocabulary, so ffmpeg's Matroska — writing `ALBUM`,
  `ALBUM_ARTIST`, `DATE` and `PART_NUMBER` at the album target where the spec says `TITLE` and
  `ARTIST` — reached the `TagSet` through nothing and filed an `.mka` under its stem. `tags::Naming`
  decides from the key set which vocabulary is in play: one name the spec never defines means
  Vorbis comments, and then `ARTIST` is the track's rather than the album's and the names symphonia
  declined are read too. `VORBIS_COMMENT_ONLY` is that evidence, holding only names no container
  spec also defines — `ORIGINALDATE`, `TOTALTRACKS`, `COMPILATION`, `MUSICBRAINZ_TRACKID` and
  `_ALBUMID` beside `MUSICBRAINZ_ARTISTID`, `_ALBUMARTISTID`, `_RELEASEGROUPID` and
  `_RELEASETRACKID`, the `REPLAYGAIN_TRACK_`/`_ALBUM_` spellings, twenty-eight in all — since one
  name flips a whole revision. `DATE`, `BPM`, `ISRC`, `LABEL`, `PUBLISHER`, `LYRICS` and
  `DESCRIPTION` are deliberately not among them: Matroska defines each, so a WAV's `INFO` list
  beside a `DATE` would be read in the wrong vocabulary off one ambiguous key. What a reader never
  exposes is taken off the source first — `riff.rs` for a WAV's `INFO` list, `matroska.rs` for the
  segment's `Title` (where ffmpeg puts `-metadata title`) — and `Prescan` is the one bundle carrying
  both, so a growing argument list does not follow every container. A segment names the file, not a
  track, so its `Title` fills one only where the container holds a single audio track: ffmpeg writes
  a `.mka`'s title there and nowhere else, so for one track it is the whole title and for two it
  would claim to be both. symphonia's `wav` feature does turn on `symphonia-metadata/riff-info`, and
  `WavReader::try_new` reads the `INFO` list — into a log it throws away, building itself with the
  caller's external metadata instead; none of those tags reaches a caller, so the prescan rescues
  them. **A WAV's `id3 ` chunk is read the same way, symphonia's reader skipping it.** A leading
  ID3v2 before the `RIFF` header is symphonia's probe's; one inside as an `id3 ` or `ID3 ` chunk —
  where lofty and most taggers write it — `WavReader` never looks at. `riff.rs` holds the chunk
  whole up to `MAX_ID3_CHUNK_BYTES`, `tags::read_id3_chunk` hands it to symphonia's `Id3v2Reader`,
  and its revision is appended *last* to `Revisions`, outranking the `INFO` list and a leading tag.
  Its pictures ride on `Coded::chunk_pictures`, weighed before the reader's, so a cover written into
  a WAV reads back. `standard_info` reads every `INFO` id landing on a `TagSet` field, under each
  name writers use and in any case; `IDIT` and `DTIM` are deliberately not among them — they date the
  digitisation, and would put a rip day where the release year belongs.
- **What the `TagSet` holds is the vocabulary; a name outside it stays a `RawTag`.** Every
  well-known name has a typed field: `credits` for the people a recording names — composer,
  conductor, lyricist, performer, remixer, engineer, producer — beside `grouping`, `collection`,
  `edition`, `label`, `isrc`, `beats_per_minute`, `copyright`, `encoder` and `comment`, the six
  release ids a tagger writes — `musicbrainz_track_id`, `musicbrainz_album_id`,
  `musicbrainz_artist_id`, `musicbrainz_album_artist_id`, `musicbrainz_release_group_id`,
  `musicbrainz_release_track_id` — and `barcode` and `catalog_number` (which a cue sheet's `CATALOG`
  line fills, being what the line is). All three naming paths land on them, so `IENG`, `IMUS`,
  `IWRI`, `IPRO`, `ICOP`, `ISFT` and `ICMT` from a WAV's `INFO`, a Vorbis name a reader declined and
  Matroska's `COLLECTION` and `EDITION` targets above the album reach the same field. The inspector
  reads a field, not a key, and MPRIS fills `xesam:composer`, `xesam:lyricist`, `xesam:comment` and
  `xesam:audioBPM` from it. A name the writer invented has no typed home by definition: it reaches
  `read_raw` and stops there. Three ids are left out on purpose: RIFF's `ISRC` names the *source* a
  recording came from, not the recording code Vorbis and Matroska write under that name, so reading
  it would file a vendor where an identifier belongs; `EncodedBy`
  is who encoded, not what, so `encoder` takes `Encoder` alone — and `ISFT`, the software that wrote
  the file; and RIFF `INFO` defines no album artist, so a WAV tagged through `INFO` alone leaves
  `album_artist` empty — `store::attribution` falling back to the track artist is the whole answer,
  and a compilation tagged that way still splits.
- **A text tag blank once trimmed is no tag, and what is stored is trimmed.** `tags::given` is the
  one door every text `StandardTag` takes into a `TagSet` slot — names, credits, the six MusicBrainz
  ids, barcode, catalogue number, lyrics — trimming the value and leaving the slot as it was where
  nothing is left, so `"   "` names nothing, `"  Echoes \n"` is stored as *Echoes*, a blank frame
  written after a name does not unname it and a later name still wins
  (`a_blank_text_tag_is_no_tag_and_a_padded_one_is_stored_trimmed`). The four ReplayGain values keep
  the rule through `parsed`: a blank or unparsable gain or peak leaves what an earlier frame or
  revision gave, so an empty frame after a real one does not play the track at no gain. A date is
  weighed by kind — recording date over recording year over release date and so on — and one of the
  held kind replaces it, so the newest revision wins a retagged year as it wins a retagged title. A
  tagger writing an empty frame rather than none otherwise files a track under a nameless artist and
  a titleless album, and `stem.rs` never runs for a title that is *there*; read as none, the file is
  named from its stem and grouped as its non-blank tags say.
- **An ID3 recording id arrives as a frame nobody standardised into a tag.** A tagger writes it in
  `UFID`, whose owner names the database, and symphonia reads that frame as a `RawTag` keyed `UFID`
  with an `OWNER` sub-field and a *binary* value, mapped to no `StandardTag` — so the
  `MusicBrainzRecordingId` arm was reached from the Vorbis and MP4 spellings alone and a
  Picard-tagged MP3 carried no recording id. `musicbrainz_identifier` reads it: the key, the owner
  against `MUSICBRAINZ_OWNER`, then the bytes as UTF-8 (what MusicBrainz writes). Tried only where
  `Naming::standard` answered nothing, so a mapped frame is never read twice. The owner is the whole
  check, `UFID` being where CDDB and every other database puts its own id.
- **A Matroska file's length is the segment's, not the track's, counted as well as read.** symphonia
  leaves `num_frames` and `duration` unset for mkv, and `rebind` seeks even to frame zero while a seek
  needs a length to range-check, so an `.mka` could not be loaded at all. `matroska.rs` reads `Duration`
  and `TimestampScale` from the `Info` element it takes the title from, and `rebind` does not seek to
  where the decoder already is. It also walks the clusters, taking each one's `Timestamp` and the
  furthest `SimpleBlock`/`Block` offset inside, so a file declaring no `Duration` — every file a muxer
  streamed rather than seeked back to finish, what `ffmpeg -f matroska -` writes — still has a length
  and a seek bar. The count is deliberately a *lower* bound — where the last block starts, short by that
  block's length, never long. `Segment::duration` keeps the declaration wherever the clusters stay
  within it (the declaration being exact and the count not) and prefers the count only where blocks
  exist past the declared end, the one case the writer is provably wrong. A declaration too *long* is
  undetectable this way and left alone — except for Opus, whose packets are counted exactly (below),
  FLAC, whose every frame header names its block size, and Vorbis: the walk counts the first track whose
  `CodecID` is `A_OPUS`, `A_FLAC` or `A_VORBIS`, and `Segment::counted_frames_of` answers the FLAC or
  Vorbis count as the length — ahead of any declaration, being exact — only where that track is the one
  symphonia decodes (its `Track::id` being the Matroska track number). **A Vorbis packet's length is
  named by the packet before it.** `vorbis::Windows` reads the two block sizes from the identification
  header in the track's `CodecPrivate` (Xiph-laced, three headers) and which modes are long from the
  setup header, walked *backwards* from its framing bit as ffmpeg's `vorbis_parser` does, the modes
  being written last after codebooks nobody wants decoded here: each candidate mode is forty bits
  (`MODE_BITS`) whose window and transform types must be zero and mapping under 64 (`MOST_MODES`), the
  mode count being the last run whose six-bit count agrees. A packet's mode is the bits after its type
  bit; it renders a quarter of the window before it and a quarter of its own, the first rendering half
  its own — what symphonia answers with gapless trimming off, as this build asks — so the count is what
  the player decodes, not what libvorbis would. The previous window rides in the tally block to block
  and lace to lace. `a_vorbis_in_matroska_is_as_long_as_its_packets_whatever_the_segment_declares`
  overstates a file's `Duration` threefold and holds the length to what the packets decode to. The walk
  is what `Prescan::buffered` paid for: over an hour-long `.mka` of ~10^5 blocks it cost 440 ms a probe
  reading four bytes at a time and 15 ms through the window, so the provably-wrong case stays caught for
  about a millisecond on a track-length file.
- **A seek lands on the exact frame only where the track timebase is the sample rate's
  reciprocal.** Matroska ticks are milliseconds, so its landing is converted and approximate, as is
  any other such format's.
- **Opus is decoded by `opus-rs`, registered beside symphonia's decoders.** symphonia 0.6 demuxes
  Opus from Ogg, Matroska and MP4 and decodes none of it, so `opus.rs` wraps `opus-rs` — a pure-Rust
  port of libopus 1.6 — as an `AudioDecoder`, and `registry::codecs` is the `CodecRegistry` holding
  `register_enabled_codecs`, `opus::Opus` and the codec crate's own other decoders, what
  `Decoder::build` makes every decoder from. `Head` reads the identification header from the track's
  extra data: Ogg's and Matroska's `OpusHead` little-endian, MP4's `dOps` big-endian under the magic
  symphonia prepends, told apart by the version byte (0 only in `dOps`). Mapping family 0 is one
  decoder of one or two channels; family 1 is `opus-rs`'s multistream decoder, whose channels come
  out in Vorbis order and are written to the plane each position takes in symphonia's bit order, so a
  5.1's centre is not heard from the right. The header's output gain is applied as samples are
  written, as RFC 7845 asks, and `R128_TRACK_GAIN` and `R128_ALBUM_GAIN` — Q7.8 gains against
  −23 LUFS, relative to that output gain — are read as ReplayGain 5 dB louder, ReplayGain 2.0
  referencing −18. A stereo decoder refuses a mono packet, which libopus writes into a stereo stream
  at low enough bitrates, so a second decoder of the packet's own width answers it and its samples are
  laid across every channel.
  **Where the pre-skip lands is the container's to say, each differently.** Ogg names it as the
  track's delay but counts it *inside* its granules, so `num_frames` holds it and the music starts at
  `start_ts` plus it; Matroska names nothing and shifts its timestamps by `CodecDelay`, so the
  pre-skip is read off the head and the music starts at zero; MP4 names nothing either and its edit
  list is what the box scan reads. `container::Carrying` is that distinction, read by `priming` and
  `music_at`.
  **Matroska's end is its last block's `DiscardPadding`**, which symphonia reads nowhere: the
  prescan's cluster walk reads each `BlockGroup`'s children and keeps the padding of whichever block
  came last, and `Segment::discarded_frames` is the priming's padding. A padding with no end to cut
  it from — `playable` open-ended, all Matroska can say with millisecond timestamps — is what
  `Decoder::build` hands `Coded::trailing`: `fill` reads a packet ahead while it is non-zero and takes
  the padding off the packet that turns out last, so the decode is exactly as long as what went in.
  **The declared length is counted, not read off the timestamps.** Every Opus packet says its length
  in its TOC byte — the config's frame size times the code's frame count, the count byte after it for
  code 3 — and the cluster walk reads that byte from each block of the `A_OPUS` track, so
  `Segment::opus_samples` is the whole stream in 48 kHz samples and `Segment::opus_music` less
  pre-skip and padding is `playable`'s end: the window is closed and the declared length is exactly
  what decodes. A laced block is read through its lace — Xiph's runs of 255, EBML's first size and
  signed differences, or fixed lacing's equal shares — each frame's TOC byte counted as an unlaced
  block's, and only for the Opus track, so another track's lace costs nothing. The count is trusted
  only where nothing escaped it — a lace whose sizes do not add up to the block, a packet naming no
  frames or more than 120 ms, a cluster or segment the walk did not reach the end of, a segment of
  unknown length — any of which leaves it `None`, the window open-ended, and the length the segment's
  millisecond count less the pre-skip it includes (`Carrying::declared_before_the_music`), within
  three half-millisecond roundings. Reaching the segment's end keeps a source that cannot seek honest:
  past `SPOOLED_AT_MOST` its prescan sees only the head, and a count cut short there would close the
  window on the music.
  **A surround stream Matroska leaves unplaced is placed by its head.** Matroska carries a channel
  count and no mask, so a 5.1 Opus track opened as `Discrete(6)`, which a downmix truncated a channel
  for; mapping family 1 *is* the Vorbis order, so `opus::channels_of` answers those positions wherever
  the container named none and the head says family 1, and the layout, speakers and decoder planes
  all read it. Family 255 names no order and stays discrete.
  `a_seek_into_opus_hears_what_decoding_from_the_start_hears_there` and
  `opus_decodes_to_what_libopus_decodes_it_to_in_every_container_that_carries_it` are the claims: in
  CELT, every music bitrate, the decode is within 10⁻⁵ RMS of full scale of ffmpeg's libopus in stereo
  and 10⁻⁴ in 5.1, and exactly as long as what went in. **A seek starts 400 ms early**
  (`PRE_ROLL`, 19 200 frames at 48 kHz), because CELT predicts each band's energy from the frame
  before — at α = 0.5 for a 20 ms frame — so a decoder started cold on the target frame is still
  halving its error 20 ms at a time; RFC 7845's 80 ms leaves a sixteenth of it. `opus::pre_roll` is
  what `seek_reader` subtracts, and the frames it lands early on are decoded and discarded like any
  other skip.
- **WavPack is read by `symphonia-codec-wavpack` and decoded through a wrapper of the codec crate's
  own.** `registry.rs` holds the one `Probe` and one `CodecRegistry` every open goes through:
  symphonia's enabled formats and codecs, the `WavPackReader` and `ApeReader`, and the decoders
  `opus::Opus`, `vorbis::Vorbis`, `wavpack::WavPack` and `ape::Ape`. The crate decodes 8- to 24-bit
  integer blocks, hybrid blocks and every channel layout bit for bit, and gets two things wrong that
  `WavPack` puts right before and after it: it reads the extended bits a 32-bit or floating block
  keeps below its 24 without skipping the four-byte checksum they open with, and on a floating block
  drops the exponent sent for a sample too small for the integers. So `wavpack.rs` rewrites each
  packet first — the checksum taken off an integer block's extended bits, and a floating block's
  `FLOAT_INFO` and extended bits taken out and its `FLOAT_DATA` flag cleared — hands it on, and turns
  the answered integers into floats itself, a port of libwavpack's `float_values` over the stereo
  pair in written order. A Matroska packet, carrying no block headers, is given them first. Both
  restores hold the shift they fill with ones to a word (`SHIFTED_WITHIN_A_WORD`), as the reference's
  32-bit shift does: a sample shifted to nothing under the greatest exponent counts some 254 places,
  and `restore` handing that to `ones` unmasked panicked a debug or fuzz build
  (`a_sample_shifted_to_nothing_under_the_greatest_exponent_is_restored_without_overflowing`).
  `wavpack_decodes_every_depth_and_layout_to_exactly_what_went_in`,
  `a_floating_wavpack_decodes_to_every_bit_that_went_in` (zeros, negative zeros, subnormals and values
  a hundred binades under the peak among them) and
  `a_hybrid_wavpack_decodes_to_what_the_reference_decoder_makes_of_it` are the claims; the first two
  fail against the crate's decoder alone. **A hybrid WavPack is billed as the lossy codec it decodes
  as.** The prescan reads the first block's header — `wavpack::read_coding` for a native stream, and
  in Matroska the flags after the sample count in the first block of the `A_WAVPACK4` track, laced or
  grouped, which `matroska::read_segment` finds — and where they carry `HYBRID` and the track is
  WavPack, `coded_info` answers `wavpack::HYBRID_CODEC_ID`, which `Codec::from_id` reads as
  `Codec::WavPackHybrid`: not `is_lossless`, so the badge, `is:lossy`, the verdict's `Lossy`, the
  vault's `Kept` and every other codec reader take it for what it is, and the catalog stores it as
  code 13 and searches it as `codec:hybrid`. The decoder is still made from the track's parameters,
  so nothing about the decode moves. The test holding the decode to `wvunpack`'s holds the billing,
  and `a_hybrid_wavpack_in_matroska_is_billed_as_hybrid_and_a_lossless_one_is_not` holds it through
  ffmpeg's remux. A catalog written before the flag was read is carried forward by a migration step
  marking every WavPack row `probe_again` (`library.md` has the mechanism). A `.wvc` correction file
  beside it is not read: `symphonia-codec-wavpack` 0.1.1 takes a held zero's correction from the
  range already narrowed to its midpoint — `low = mid` before `read_code(high - low) + low`, where
  libwavpack reads from the narrowed `low` — so a corrected decode drifts off the source from the
  first such word, and splicing the correction blocks into each packet landed within a few steps of
  the source rather than on it. Its APEv2 tags are read by the crate's reader and written by lofty.
- **A Vorbis setup header is walked before symphonia's decoder reads it.** symphonia builds each
  codebook's codewords into a table of 33 lengths indexed by the length a codebook names, and an
  *ordered* codebook counts its lengths up a run at a time with nothing stopping them past 32 — so a
  crafted setup indexed past the table and panicked, an abort of the whole player in a release build.
  `vorbis::Vorbis` is registered over symphonia's `VorbisDecoder`: it finds the setup header in the
  extra data — Xiph-laced or packed behind the identification header, the two shapes the readers hand
  over — walks every codebook forward, lookup tables included, and refuses the decoder where a run of
  entries lands on a length past 32, handing all else to symphonia untouched. A walk that cannot read
  a codebook stops and leaves the refusal to symphonia.
  `a_decoder_is_refused_for_a_setup_whose_codewords_run_past_32_bits_rather_than_panicking` is the
  claim, and panics against the bare decoder.
- **Monkey's Audio is read by a reader of the codec crate's own and decoded frame by frame.**
  `ape.rs`'s `ApeReader` parses the header and seek table through `ape-decoder`, answers one packet
  per frame — read from the seek table's offset less the frame's alignment, that remainder carried in
  the packet's first byte — and seeks by frame, the decoder discarding forward to the frame asked
  for. Its markers are `MAC ` and `MACF`, the second what Monkey's Audio 11 writes for a floating
  stream; missing it, the probe skipped to the stored `RIFF` header and opened that as a WAVE. The
  kept WAVE header is read for its channel mask, so a 5.1 is placed. `Ape` decodes a packet through
  `FrameDecoder`, whose per-frame checksum refuses a frame decoded wrong, and lays the answered bytes
  into a buffer of the stream's own width; a floating stream's words go back into IEEE bits through
  the transform the format stores them by. A stream made from an AIFF decodes little-endian and
  unsigned like any other, the flags saying only what the original was. A 32-bit stereo stream is
  refused, the crate narrowing the side channel to 32 bits before undoing it.
  `monkeys_audio_decodes_every_depth_and_layout_to_exactly_what_went_in` and
  `a_floating_monkeys_audio_decodes_to_every_bit_that_went_in` are the claims, over files `mac`
  writes.
- **This build drops every priming itself and asks nobody else to.** `Decoder::build` makes its
  decoder with `AudioDecoderOptions::gapless(false)`, so symphonia's decoders emit whole blocks and
  the only trimming anywhere is `MediaInfo::playable`. One rule instead of a list: `gapless` defaults
  *on* but only `symphonia-bundle-mp3` and `symphonia-codec-vorbis` implement it, so who trimmed what
  was a fact about the codec crate to keep in step with the symphonia version. It also stops a
  priming coming off twice now that a reader's own declaration fills `playable`.
- **A priming reaches `playable` from whoever named it — the reader or `boxes.rs`.**
  `container::priming` prefers `Track::delay`, `Track::padding` and `Track::num_frames` — what CAF
  reads from its `pakt` chunk, mp3 from the LAME tag and ogg off its first and last pages — and falls
  back to the box scan where the reader named nothing. isomp4's reader names nothing, hence the scan:
  it reads the `iTunSMPB` free-form item under `moov/udta/meta/ilst` where a store wrote one,
  otherwise the first non-empty `elst` edit, its segment rescaled from the movie timescale to the
  media one, weighed against what the `stts` table says the decoder will emit — the sample count
  times the longest delta, not the sum, the last sample's declared duration being short by exactly
  the padding. It answers for the `soun` track alone and only where the media timescale is the sample
  rate, which is what makes a media tick a sample frame.
- **A fragmented MP4's length is in its fragments, not its `moov`.** A DASH-style file — `ftyp`
  `iso8`/`dash`, a `moov` whose `mvhd`, `mdhd` and sample tables are empty, the audio in `moof`/`mdat`
  pairs — declares a length of nothing, which symphonia hands back, so the seek bar and catalog read
  0:00 while it played whole. The box scan finding the priming also reads the length such a file
  carries: `mvex/mehd`'s fragment duration in the movie timescale, or else the summed subsegment
  durations of the first top-level `sidx` in its own timescale, rescaled to the stream's rate.
  `coded_info` reads a declared zero as no length and takes the fragmented one
  (`Movie::fragmented_length`) before the Matroska segment's; it put an 8:37 FLAC-in-MP4 at 8:37
  rather than 0:00. A catalog that scanned such a file before holds its zero until the file is read
  again, an incremental scan passing over a file whose size and mtime have not moved — which the
  Library category's *Read every file again* is for.
- **`playable` is decoded frames, and it may be open-ended.** `Decoder::build` drops
  `playable.start()` decoded frames before the first block and takes `playable.frames()` as its
  limit, so priming and padding both go, and `duration` is the playable count — what the seek bar,
  `mpris:length` and `heard.rs` read. `Priming` holds the count as an `Option`, since a reader can name
  a delay and not a length — an ogg over a pipe reaches no end bound, and a Xing header can carry a
  LAME delay and no frame count — and the priming must still go where the end cannot be said.
- **`Timeline` is told where the music starts, not how long the priming is.** It holds one
  `music_at: Timestamp` and maps a playable frame to a container timestamp by adding to it, the only
  arithmetic a seek needs. A reader that named the delay has already put the music at timestamp zero
  (symphonia's convention is a negative PTS for an encoder-delay frame), so `music_at` is
  `Timestamp::ZERO` for it; one that did not gets `Track::start_ts` plus the scanned priming. Carrying
  `start_ts` and the priming as two fields had ogg wrong both ways, ogg being the one reader that
  *does* set `start_ts` to `-delay`: a seek landed where asked then decoded 896 frames short of the
  rest of the track. A seek can land *ahead* of `music_at` — back to the start, or inside Opus's
  pre-roll — and a frame count cannot say how far, so `seek_reader` answers a `Landing` carrying
  `short_of_the_music` beside the frame and `restart` drops that much before counting; saturating to
  frame zero played the priming as music and heard the rest that many frames late. On a timeline not
  sample-accurate the tick delta is too coarse for the first packet: Matroska counts in milliseconds,
  so symphonia turns an Opus `CodecDelay` of 6.5 ms into 6 ticks — 288 frames of a 312-frame pre-skip
  — and a seek inside the pre-roll heard 24 frames of priming. `Coded` notes the first packet's
  timestamp as it reads it, and a landing on that packet drops `MediaInfo::priming` whole, as the
  cold open does; a sample-accurate timeline (Ogg's) and any later packet keep the delta.
- **Every revision of a file's metadata is read, not only the newest.** symphonia's
  `Metadata::skip_to_latest` discards the revisions it walks past, and containers publish several as
  a matter of course: Matroska one per `Tags` element, isomp4 one for a top-level `meta` atom and one
  for `moov`, AIFF one for its text chunks and one for its ID3. `tags::Revisions` drains the log once
  in `container::open` and hands the same owned set to `tags::read` and `tags::read_raw`, oldest
  first, so the newest still wins a tag two name and the rest are kept. Each revision is absorbed on
  its own, so `Naming` weighs one writer's key set, not every writer's flattened. Only the tags are
  taken: the visuals stay in the reader's log for `probe_cover_art`, keeping a cover out of the clone.
- **A picture is found once and copied once, and who copies is the caller's to say.**
  `probe::picture` is the one walk — the attachments, then the newest revision's visuals, front cover
  first — handing what it found to a `taken` of the caller's choosing, so `probe_cover_art` copies the
  bytes and `Picturing::Whether` answers whether there is one without touching them. The
  `MAX_COVER_BYTES` weighing stays inside the walk, so a picture declined as absurd is declined alike
  whichever question was asked. **`probe_pictured` is the one open answering a file's tags and its
  picture together**, the caller's `Picturing` saying how much of the picture: `Whether` answers
  `Pictured::Carried` or `Bare` and copies nothing, `Copied` answers the bytes as `Pictured::Copied`,
  and a row the sources stand in for answers `StoodIn`, the vault's object carrying no picture and
  the row's own file not being what was opened. The library scan reads every file through
  `probe_scanned`, which asks `Whether` of the picture (`library.md` has why the bytes are left to be
  read later) and then digests the first `PACKETS_DIGESTED` packets of the audio track into a
  `PacketDigest` on the same open — what a moved file is known by once every name it had has been
  retagged away; `resonate tag`'s plan asks `Whether` too, wanting only whether a file carries a
  cover, and its read-back asks `Copied`, so a written picture is weighed off the same open as the
  fields. `TagSource::read` takes the `Picturing` and answers a `Tagged`, so no implementation can
  answer the two through separate opens.
- **The engine's tag reader reads a picture on the open its tags came off wherever it can.** A
  queued row no scan has seen is asked for its name and, where the window draws it, its cover — once
  two opens of one file. `catalog::whole` probes a whole-file row under `Copied`, and
  `Shelf::keep_what_the_tags_saw` files what it learned: the bytes where the picture is already
  waited on, and `Look::Nothing` where the file carries none, so a later look answers at once. A
  picture nobody has asked for yet goes to the `Spares` beside the shelf — at most `SPARE_ART_BYTES`
  (8 MiB), oldest leaving first — staying out of `ART_BYTES_HELD` until something draws it, and an
  art ask arriving after the tags landed takes it from there; only one pushed out of the spares by
  rows read after it is read again
  (`a_picture_asked_for_after_the_tags_landed_comes_off_the_open_they_came_from`). An art ask arriving
  after the tags settled it is skipped rather than read again. A row a sheet cuts from a file is read
  through `probe_span` and says nothing about the picture, the picture being the file's and the
  whole-file row asked on its own.
- **What the engine's catalog holds follows the file.** A read that failed — a share unmounted, a
  file mid-copy — is `Look::Failed` with its time, answered as nothing but claimed again after
  `READ_AGAIN_AFTER` (30 s), where `Nothing` is a read that found nothing and stands. Each entry
  keeps the file's size and modification time (`Stamp`), and `looked_at` weighs them against the
  disc at most every `LOOKED_AT_THE_FILE_EVERY` (5 s): an entry whose file moved — retagged, replaced,
  mounted again — is forgotten and read afresh, so a queued row no longer holds its old name until
  the LRU evicts it (`a_read_that_failed_is_asked_for_again_once_a_while_has_passed`,
  `a_row_whose_file_changed_on_disc_is_read_again`). A row still being read is never weighed.
- **A picture is weighed before it is copied.** symphonia has read a visual into its own buffer by
  the time `probe_cover_art` sees it, so the `to_vec` is a second copy of whatever the file embedded —
  and the engine's `ART_BYTES_HELD` bounds what the catalog *holds*, long after the allocation.
  `MAX_COVER_BYTES` is checked against `data.len()` before the copy, and an oversized visual is
  declined rather than failing the read: `choose` prefers a front cover then any visual, so the next
  candidate is tried and a file whose front cover is absurd still draws its back cover. A file whose
  every picture is oversized answers `None`, as one with no picture does — nothing failed, a picture
  was declined, which is a `tracing` record.
- **A hand-rolled parser never allocates what a header declares.** `riff.rs` and `matroska.rs` read
  `declared.min(MAX_…_BYTES)` — the `id3 ` chunk through a `take` growing only as bytes arrive — then
  seek absolutely past whatever the chunk or element claimed, so a thirty-byte file declaring a
  gigabyte costs one bounded read and a failed seek. A declared size is what the walk advances by,
  never what a `Vec` is sized to.
- **A source that cannot seek is spooled, and only one too long to spool is read from its head.**
  The `INFO` scan looks for its magic where `prescan::opened_first` says symphonia would find it, the
  EBML title scan bails on the first bytes not its magic, and both restore the position they found,
  so a container that is neither pays a rejected read. A source that cannot seek cannot be restored,
  so `container::open` reads it into memory up to `SPOOLED_AT_MOST` — 256 MiB, which a five-minute
  24/192 FLAC fits — for at most `SPOOLED_WITHIN` (750 ms), and where the stream ends inside both it
  is opened as a seekable `Reading` over the bytes with everything a file has: the whole prescan, a
  trailing `moov`, a `LIST INFO` after `data`, the Matroska cluster walk counting an Opus or FLAC track
  exactly, the seek bar and the end bound a priming is trimmed against. The wait keeps a slow remote
  stream from holding the first sample for its whole download: a local provider's copy lands well
  inside it, and a stream still arriving when it runs out — or past the cap — goes on arriving onto
  the disc. `spool::Spool` is an unnamed file under the temporary folder, made and unlinked at once
  so nothing is left whatever ends the run, holding the head, and a `resonate-spool` thread copies the
  rest into it as it arrives, up to `SPOOLED_ON_DISC_AT_MOST` (8 GiB), stopping the moment nothing but
  itself holds the spool. symphonia gets its `Spooling` side, which reads what has arrived and waits
  on a condition for the rest — still `is_seekable() == false` and `byte_len() == None`, since a
  reader thinking it can seek looks for the end at once, which is the whole download — and the walks
  run over a `Cursor` of the head, advancing by absolute seek, which simply stops at its end, so
  neither `riff.rs` nor `matroska.rs` knows the difference. **Once it has all arrived it can seek.**
  `Decoder::settle_the_spool` answers `None` until the copy reaches the source's end, then opens the
  spooled file again as a seekable source — `Unspooled`, reading at its own offset, the whole prescan
  with it — seeks the new decoder to the frame the old had reached, carries delivery, span and tags
  across and takes its place, so a length the stream never declared is known and the seek bar
  appears. The engine asks every pass while the playing track cannot seek
  (`Engine::settle_what_was_spooled`) and republishes the digest, so MPRIS's `CanSeek` and the window
  follow. A spool that could not be made (no writable temporary folder) falls back to the `Replaying`
  head, and one cut short by its ceiling or a failing source plays to where it stopped and never
  claims to be whole. `a_pipe_too_long_to_hold_is_spooled_on_disc_and_seeks_once_it_has_all_arrived`
  holds the stream to every sample across the swap and a seek after;
  `a_source_that_cannot_seek_is_spooled_and_keeps_even_the_tags_after_its_audio`,
  `a_pipe_longer_than_the_spool_is_replayed_from_its_head_and_cannot_seek`,
  `a_stream_that_arrives_slowly_opens_from_what_came_within_the_wait_rather_than_its_whole` and
  `a_vorbis_rip_over_a_pipe_is_spooled_and_drops_the_priming_and_padding_its_pages_declare` are the
  claims.
- **Until it can seek, a track keeps its stream.** A rebuild throws the ring and the carry away and
  seeks the decoder back to where the listener is, which a track that cannot seek cannot do — the
  audio jumped forward by what the ring held and the seek answered `Ok`. So `Engine::seek` on such a
  track refuses with `codec::Error::NotSeekable` (`Cause::CannotSeek`), except a seek to its start —
  a restarting Previous among them — which opens the row again through `start`
  (`seek_where_nothing_seeks`). A setting that rebuilds the stream — the sink, quality, filter phase,
  dither, restoration, true peak, noise shaping, convolution, the forced graph rate, bit-perfect, DoP
  and a retune that packs again — goes through `rebind_where_it_stands`, which instead marks the
  rebuild owed; `settle_what_was_spooled` pays it the pass the spool settles, and the next track's
  `start` forgets it, being built under the new settings anyway. A rebuild the graph forces — a device
  gone, a format renegotiated, a stalled discard — still happens at once
  (`a_stream_that_cannot_seek_refuses_a_seek_and_keeps_its_stream_until_it_can`,
  `a_stream_that_cannot_seek_is_opened_again_to_go_back_to_its_start`).
- **A prescan over a source that *can* seek reads through a window, the five walks being made of
  four-byte reads.** `riff.rs`, `caf.rs`, `matroska.rs`, `boxes.rs` and `flac.rs` each read an id or
  header a few bytes at a time then seek absolutely past what it declared, and `Reading` delegates
  straight to the `File`, so one open cost ~forty syscalls before symphonia got anything.
  `Prescan::buffered` is `Prescan::read` over a `Window`: one `PRESCAN_WINDOW` (4 KiB) buffer refilled
  by an absolute seek and a read, a free `stream_position` (the window counts it), and a seek that
  moves only that count — so a walk reading four bytes, seeking and reading four more pays one read,
  and a seek back to a held byte pays nothing. A seek never invalidates the window, only a read
  outside it replaces it, which makes `boxes.rs`'s one `SeekFrom::End` and the return to the head
  free; a read larger than the window goes straight through. `rewind_the_source` puts the real stream
  back where the prescan found it, each sub-scan restoring the window's count rather than the file's.
  A source that cannot seek keeps its head's `Cursor`, memory already. The window is an array, not a
  `Vec`, so a prescan allocates nothing on a path sixteen workers take at once. Measured over this
  library: a scan's reads fall 58 % and seeks 64 %, for ~3 MiB of peak scan RSS against a 37 MiB
  baseline — even-looking on a warm page cache, plainly worth it where a seek is a round trip.
- **What a fuzz run finds inside symphonia is guarded against where the prescan can see it.** The
  fuzz build carries overflow checks, as the optimised debug build does, and symphonia's WAVE reader
  multiplies a `fmt ` chunk's channel count into a `u16` block alignment and widens a short channel
  mask by a shift past 32 bits; a release build wraps both and only logs, but a debug build panicked
  and stopped a whole scan on one such file. `riff.rs` reads the first `fmt ` chunk's channels and
  extensible mask — the first, being the one symphonia reads — and `container::open` refuses a WAVE
  naming more channels than a WAVE layout holds as `Error::TooManyChannels`, and one whose mask
  `riff::a_mask_the_decoder_cannot_widen` as `Error::ChannelMaskNotRepresentable`, before symphonia
  sees either. symphonia's probe searches for a container's magic byte by byte rather than at the
  start, so a `RIFF` header behind junk, or behind an ID3 tag whose size is not synchsafe, is still
  what it opens: `prescan::opened_first` searches past a tag — or past the start where the tag's size
  cannot be read — as far as symphonia's probe does, `PROBED_WITHIN` (1 MiB), a chunk at a time and
  no further than the first marker, and answers which container that marker is. A `RIFF … WAVE` or a
  `caff` is what the guards weigh; any other container symphonia registers — `fLaC`, `OggS`, EBML,
  `FORM`, `wvpk`, `MAC `, a DSD file, an MP4's `ftyp`, or two frames in a row that symphonia's own
  scoring takes: MPEG-1, 2 or 2.5 at any of the three layers (read through its bitrate tables, its
  reserved rates and its refusal of a layer II rate the channel mode forbids) or ADTS carrying one
  block a frame — means symphonia opens that one first and the guards stand aside rather than refusing
  a WAVE it never reaches. A file whose marker is at its start — every file but a broken one — costs
  the one read it always did. The CAF reader overflows the same way on three declared values, and
  `caf.rs` reads them first: packets whose size in bits a `u32` cannot hold are
  `Error::PacketTooLarge`, a `data` chunk declaring more frames than a `u64` counts is
  `Error::FrameCountNotRepresentable`, and a packet table whose offsets run past one is
  `Error::PacketOffsetNotRepresentable`. A `desc` naming more than `MOST_FRAMES_A_PACKET` frames a
  packet is `Error::PacketTooLong`, since symphonia's PCM decoder allocates a buffer that long before
  reading a byte, and a declared four billion is an out-of-memory abort in a release build.
- **A cue sheet's track is a window on a file, and the window lives in the decoder.**
  `Decoder::open_span` seeks to the span's first frame, stops at its last and rewrites
  `MediaInfo::duration` to the span's length, so everything upstream sees a short file with no notion
  of a cue sheet: the engine's seek clamp, the seek bar, `heard.rs`'s "half the track" and MPRIS's
  `mpris:length` all read `info.duration`. `self.position` stays absolute inside the decoder —
  `fill`, `discard_to` and `seek_reader` are untouched — and only `position()` and `seek()` convert at
  the boundary. `confine` takes the landing `seek_reader` reports rather than assuming an exact seek
  and discards forward from there, landing a FLAC span on a sample the container cannot address
  directly.
- **A window on a file is billed by the sheet that cut it, and the span names the row.** `confine`
  writes the row's own `TagSet` over the file's wherever a cut names the frame the span starts on, so
  a single-file rip plays as twelve tracks, not twelve copies of the album: the title, the number and
  — why it is the decoder's business rather than a front end's — `REPLAYGAIN_TRACK_GAIN`, which the
  engine resolves off `info.tags` when it opens the row (`Unwrapped::open`, then `Track::of`), like
  any file's. `CueFile::cut_at` is the match, on the start frame alone, the one edge both queue row
  and sheet compute from `CueTrack::start`. `CueTrack::titled` is the row's tags with `Track N` where
  the sheet named it nothing, and the scan writes its rows through the same call, so catalog and
  transport cannot bill one row two ways. `cue::cut_for` is where the cut comes from: the
  `MediaInfo::cue` the file embeds, else the sheet beside a *local* file — `<stem>.cue`, then
  `<stem>.CUE` (`SHEET_EXTENSIONS`) — whose `FILE` line names it, or its only cut where it holds one
  (what a rip converted after its sheet was written still reads as). A non-filesystem source has no
  sidecar and keeps the embedded answer or none.
- **A row is probed the way it is played.** `probe_span` is `probe` through the same cut, so the tags
  a queue row draws and plays under come off one reading — `crates/resonate-codec/src/decoder.rs`
  asserts the pair agree. The engine's catalog reads a spanned row through it, which is the whole of
  why a cue row on the bus carries its own title.
- **The 1/75 s unit a cue sheet counts in is a `sector`, never a frame.** `Frames` means a PCM frame
  everywhere here, and a sheet's `mm:ss:ff` does not; calling both "frame" is the mistake the format
  invites. `CueStamp::at` is the one conversion, exact at every supported rate (44100/75 is 588). A
  stamp's minutes are a `u32` and its seconds and sectors a `u8` each, so a minute count no sheet
  could mean is refused as the stamp is read rather than multiplied past `u64` on a scan worker or
  the engine thread.
- **A rip that embedded its sheet is cut by the same reader, and where a track starts is a type.**
  `CueTrack::start` is a `CueStart`: `Written(CueStamp)` for a text sheet's `mm:ss:ff`,
  `Sampled(Frames)` for the sample offset a FLAC `CUESHEET` block counts in, `CueStart::at` the one
  conversion — so routing a block through a stamp cannot round a non-CD-DA rip's boundaries by up to a
  sector's 13 ms. `MediaInfo::cue` is filled in `container::coded_info` from two places, the first
  preferred: a Vorbis `CUESHEET` comment — a whole cue file with titles, read through
  `tags::read_cue_sheet` into `cue::read` — else the binary block, which `flac.rs` walks off the
  metadata headers beside `riff.rs` and `matroska.rs` in the `Prescan`, a non-FLAC costing the
  four-byte magic. A block carries no names, so its tracks come back with an empty `TagSet`. Its last
  track is the lead-out — 170 for CD-DA (`CD_DA_LEAD_OUT`), 255 otherwise (`LEAD_OUT`) — kept in
  `CueFile::tracks` as `CueTrackKind::Data`, so `audio_tracks` skips it while `span_of` reads its
  offset as the last audio track's end. A track's start is its own offset plus its index 1's where it
  declares one, putting a pregap on the track before, as `INDEX 01` does in a text sheet.
- **A track belongs to the file its `INDEX 01` is in, whichever `FILE` line its `TRACK` followed.**
  EAC's default sheet for a one-file-per-track rip — gaps appended to the previous track — writes each
  track's `TRACK` line and `INDEX 00` at the tail of the file before, then the next `FILE` line, then
  `INDEX 01 00:00:00`. Closing the track at the `FILE` line cut every file into its own track and a
  sliver of the next one's gap, a row of 0:00, and left the last file claimed by the sheet with no
  track, never scanned. `Reading::file` carries a track that has not reached its `INDEX 01` across the
  `FILE` line with its start put back to the head of the new file, so the gap stays at the end of the
  track before and each file is one whole row.
- **A cue sheet is read as far as it parses and never fails.** An unknown command is skipped (as
  `lrc.rs` skips a bracket neither moment nor id tag), and rubbish yields a sheet naming nothing. Only
  the source and the `LARGEST_CUE_SHEET` ceiling (1 MiB) raise a `codec::Error`. Text is decoded by
  BOM first, then valid UTF-8, then Windows-1252, EAC really writing UTF-16LE.

## DSD

- **A DSD file's carrier rate is what the rest of the workspace sees, whichever way it is
  delivered.** `SampleRate` is bounded to 768 kHz (`MAX_HZ`), so 2 822 400 Hz is never one; `DsdRate`
  carries the native rate and `carrier()` is `hz / 16`. A DSD64 file is 176.4 kHz `S24` stereo whether
  it reaches the sink DoP-packed or decimated, so `Frames`, `Timeline`, the seek bar, the ring and
  `plan_output` need no special case and the choice between the two is one switch. DSD512's
  1.4112 MHz carrier is out of range and refused at open with `RateNotRepresentable` naming the DSD
  rate.
- **`Packing` is the guard, not a rule to remember.** It rides on `MediaInfo` and `OutputPlan`, and
  the only constructor producing `DopMarked` is `untouched`, which writes `resample`, `equalisation`,
  `gain`, `dither_to` and `shaping` as literals — no path leads from a packed source to a plan with a
  stage. `no_setting_the_pane_offers_can_produce_a_marked_plan_with_a_stage_in_it` walks every sink
  shape crossed with every gain, equaliser, dither, shaping, volume and ReplayGain mode to say so, and
  counts the marked plans reached, since its loop opens with a `continue` and an axis refusing DoP
  throughout would prove nothing while looking thorough. `OutputPlan::delivery` derives what to ask
  the decoder for from the same plan, so the two cannot disagree.
- **DoP is opt-in and off by default.** Nothing in ALSA or SPA advertises DoP support — it is
  invisible to all but the DAC's own detector — and DoP sent to a DAC that does not decode it is
  full-scale white noise. So `EngineConfig::dop` defaults to false, the decimator is what an
  unconfigured run gets, and the gate is `SinkInfo::supports` exactly, never `best_spec_for`, whose
  fallback to a sink's widest format would truncate DoP to S16 noise and whose fold to a sink's layout
  would rewrite the markers channel by channel. `Decoder::set_output_format` means *samples in this
  format* and clears the packing; DoP is reached only through `deliver`. `resonate explain` prints
  which was chosen and why the other was not.
- **An all-zero DSD byte is negative full scale, not silence.** It bites in three places, each with
  its answer: the DSF final block's padding, truncated against the declared sample count; the
  decimator's FIR history, primed with `DSD_SILENCE` (`0x69`, density one half); and the gap a seek
  opens, which the ring fills from `Packing::silence` rather than PCM zeros.
- **The ring is told what silence looks like on the wire; it does not assume zeros.** `ring` takes a
  `Silence` beside the spec and capacity — `Unmarked`, or `Marked` with a four-byte word, its alternate
  and the `SampleFormat` saying how much of each word a sample holds — and `Silence::write` lays one
  word across a frame's channels and swaps it with its alternate for the next, so the marker keeps
  alternating through a gap no audio fills. `Packing::silence` is the only constructor: `Samples`
  answers `Unmarked`, `DopMarked` answers `0x05` and `0xFA` over a `0x69 0x69` pair — DSD silence with
  the marker a DAC locks onto still running — so a DAC holds lock through the prime window a seek
  opens rather than dropping out of DoP and muting. `Unmarked` writes nothing and answers zero, a
  short chunk already being silence to a PCM sink, so every PCM stream gets exactly the bytes it got
  before. `Silence` is `Copy`, allocates and indexes nothing, `RingConsumer::fill` being the RT thread.
- **A gap is a gap however it opened, so a starved read is covered too.** A seek's prime window was
  the only one `Silence` filled; a ring running dry mid-track handed the graph a short chunk, for a
  DoP stream a whole quantum with no carrier — ~6 ms at 176.4 kHz — the one thing a DAC cannot hold
  lock through. `RingConsumer::fill` pads the rest of the quantum after an underrun, allowed by
  `Discard::finished`: the producer sets it when a track's last audio is written, so a short chunk at
  a track's *end* is what it always was and the graph's drain handshake still closes the boundary.
  `RtFault::Underrun` is raised either way, so the padding changes what the sink hears, not what the
  engine is told. An unmarked ring pads nothing (`Silence::write` answering zero).
- **What a starve cost is counted in frames, apart from the faults saying it happened.** The ring adds
  the frames the graph asked for and was not handed to one `AtomicU64` beside the fault queue — an
  add, so the realtime rules hold — whether its silence covered them or the graph's short chunk did,
  never for a track's ending short chunk. `RingMonitor::went_without` swaps the count out, so the
  engine reads what is new since it last asked, and `collect_faults` keeps it on the underrun count's
  terms: while playing and before the track has ended. The ring adds the frames *before* raising the
  fault, with a release the swap acquires, so a fault is never read ahead of its cost; a count arriving
  on a poll that drained no fault is still kept — it is the last fault's, and dropping it left the
  total short. `OutputStatus::went_without` is the total at the sink's rate for the output's life, the
  inspector's *Output* card draws it in milliseconds beside the underruns once there is any, and
  `Event::Underrun` carries it. It is exact where the fault queue is not: a fault the queue had no room
  for arrives as a bare count in `RtFault::Dropped`, which is why `RtFault::Underrun` carries no frames
  — the count is where they live. `what_a_starved_graph_went_without_is_published_in_frames` pulls a
  second from a ring a tenth as deep and weighs the figure against what the pull got.
- **The marker a splice resumes on is the audio's, not the ring's, and both edges of a gap are made
  exact.** A DoP marker alternates by the decoder's *absolute* frame while `Silence` alternates by its
  own count, so silence between two audio frames could put two like markers together at either end —
  worse after a discard, the refill frame being a position nobody has read yet. Both edges are closed
  by reading the marker off the audio, carried by the *sign*: `dop::packed` puts `0x05` in the top
  byte of a 24-bit word and `0xFA` in the same byte sign-extended, so the first reaches the sink as a
  positive sample and the second a negative one, which `marks_negative` reads off the word's top byte
  whatever the format's width. `Silence::follows` is the leading edge — the consumer hands it the last
  real frame of every fill, so the phase always follows what the graph last heard — and
  `RingConsumer::realigned` the trailing one: where padding was written and audio waits, the first
  frame is peeked through an uncommitted `read_chunk`, and one more silence frame written ahead of it
  where `Silence::would_write` says it would otherwise repeat. Such a frame costs 5.7 µs and keeps the
  alternation unbroken across a seek, a starve and the first audio after a stream opens alike.
- **A decimation is at the modulator's level unless the listener asks for PCM's.** DSD's full scale
  is 50 % modulation, so a master decimates ~6 dB quieter than the same music in PCM.
  `EngineConfig::dsd_like_pcm` — the `dsd-like-pcm` key, the Output category's *DSD over PCM* group,
  off by default — raises it: `plan_for` adds `DSD_MODULATION_DB` (6.02 dB) to the gain it was handed
  wherever it decimates, riding the gain stage a decimation always carries, capped at a known peak and
  ridden by the true-peak guard where none is known — a choice rather than the rule, since hot material
  would otherwise clip. A DoP plan is never raised, untouched by construction, and
  `Command::SetDsdLikePcm` retunes the chain in place.
  `a_decimated_stream_is_raised_to_the_pcm_level_only_when_asked_and_dop_is_left_alone` is the claim.
- **A decimation is a conversion, and DoP comes back the moment nothing stands in its way.** A
  decimated stream is the carrier rate at `S24`, exactly what `MediaInfo.spec` says, so `plan_for`
  reads the source's `Packing`, not its spec: a `DopMarked` source delivered as samples is
  `OutputMode::Converted`, and the graph is not asked for `NO_CONVERT` for it. The other way is
  `packs_again`: a DoP stream turned down is decimated through a rebind, and turning it back up would
  otherwise reshape the decimated chain in place until the next track, so `retune` asks it first — the
  source is DSD, the open plan samples on the carrier's own spec, and `dop_survives` against the bound
  sink — and rebinds into the marked plan where it holds.
- **A DST-compressed DSDIFF is unpacked into the stream an uncompressed one would be.** The `DST `
  chunk's `FRTE` says how many frames there are (each 1/75 s of every channel), so the layout is known
  from the header and a scan pays nothing for the compression. Only a decode walks the chunk:
  `dsd::unpacked` collects every `DSTF`, passing `DSTC` checksums by, and hands the DSD path
  `dst::Unpacked`, a `MediaStream` over the frames that decodes whichever one a read lands in and holds
  it, so the reader, DoP, the decimator and a seek see an interleaved, most-significant-first DSDIFF
  sound chunk and nothing else changed. A frame decodes alone — every channel's history starts from
  `0xAA` each frame — so a seek costs one frame. `dst.rs` is ISO/IEC 14496-3 subpart 10 as FFmpeg's
  decoder reads it: the channel-to-filter and channel-to-probability maps, the filter coefficients and
  probability tables plain or predicted and Rice-coded, the 12-bit arithmetic decoder, and a
  prediction summing sixteen 256-entry lookups built per filter from its coefficients, wrapped to 16
  bits as the reference does; a plain frame is copied and padded with DSD silence. Six channels at
  most (`MOST_CHANNELS`), and a frame whose segmentation is not the reference encoder's is refused as
  FFmpeg refuses it. Checked against FFmpeg on its own DST sample: the unpacked bits, written as an
  uncompressed DSDIFF, decode to PCM bit-identical to FFmpeg's decode of the DST file. The tests carry
  their own encoder — the arithmetic coder's inverse with its carry, tables written plain and
  predicted — so `a_coded_frame_unpacks_to_exactly_the_bits_that_were_packed` and
  `a_dst_dsdiff_plays_exactly_what_the_same_bits_uncompressed_play` need no fixture on disc.
- **A packet that will not decode is played as the silence it would have lasted.** symphonia's
  `DecodeError` on one packet — a spoiled frame in an otherwise good file — hands `fill` the
  packet's own length through its `PacketSpan`, and the pending frames are marked `silent`, so
  `coded_block` writes zeros for them rather than skipping the packet; the stream keeps its length,
  the position its clock, and a cut whose limit counts delivered frames ends where its row does. A
  packet whose length the container never said is still passed over, there being nothing to fill.
  `an_undecodable_packet_is_played_as_the_silence_it_would_have_lasted` spoils one MP3 frame's side
  information and weighs the decode against the pristine file's.
- **A DSD read that fails is an error, as a PCM codec's is.** `Planes` holds the location, and a
  seek or read the stream refuses — an `EIO`, a DST frame `dst::Unpacked` cannot decode — reaches
  `next_block` as `Error::Io`, so the engine reports and skips the row rather than taking it as
  finished; an interrupted read is tried again. `fill` breaking on any error ended the track early
  and silently (`a_read_that_fails_mid_track_is_an_error_rather_than_the_end_of_the_track`).
- **A DSDIFF is tagged the two ways its writers tag it.** The `ID3 ` chunk a tagger appends after the
  sound is read by the ID3v2 reader a DSF's metadata block goes through, and the `DIIN` chunk's `DIAR`
  and `DITI` — the edited master's artist and title — fill only what the ID3 tag left empty. A `DIIN`
  text is bounded by `MAX_EDITED_TEXT_BYTES` and by its chunk, so a count claiming more than the chunk
  holds names nothing.
- **DSF bit-reverses and DFF does not.** DSF's `fmt ` chunk declares `1` for LSB-first and `8` for
  MSB-first — checked against ffprobe, which reads a hand-built fixture of each as `dsd_lsbf_planar`
  and `dsd_msbf_planar` — while DFF is always MSB-first. The wrong way round yields recognisable,
  grossly distorted music rather than an obvious failure, so both orders decode the same tone in a
  test.
- **The `0xFA` marker must sign-extend negative.** A DoP word is a sign-extended 24-bit value in the
  low bits of an `i32` — what `S24` means here and `S24_32LE` puts on the wire — so `0xFA1234` is
  `-388_556`. The raw bytes look right either way in a hex dump; the mistake shows only once `retype`
  or `convert` touches the sample.
- **The decimator is the codec's own, the layering forbidding `resonate-dsp`.** A 512-tap
  Kaiser-windowed sinc decimating by 16, evaluated as a byte-indexed table of partial sums: a 1-bit
  input makes an FIR a sum of taps selected by bits, so no multiplies, cheaper than a CIC plus the
  compensator it would need. A box average is the wrong shape — DSD's shaped quantisation noise rises
  steeply above 30 kHz and a sinc¹'s sidelobes would fold it inward — so the cutoff is 45 kHz absolute,
  rebuilt per rate, with ≥90 dB at the output Nyquist proved by a DFT of the designed taps. A DC
  blocker follows, 1-bit modulators routinely leaving a bit density off one half. What it decimates
  reaches an integer word through the chain's output conversion, and the filter overshoots a step into
  all-ones by ~8 %, so full scale saturates at 8 388 607 rather than 8 388 608 — which the old
  arithmetic reached from +1.0 and a 24-bit wire reads as negative full scale.

## Sources

- **`LocalFiles` is the local source's provider**, replaced under `SourceId::local()` by the vault's
  `VaultFiles` where a vault is open (`vault.md`); no non-filesystem provider is registered, so every
  other `MediaLocation` is refused by name. The seam is proved by a provider serving a track from
  memory in `crates/resonate-engine/tests/transport.rs` and by nothing else.
- **A provider hands back a `Read + Seek + Send + Sync` stream and says whether it is seekable**,
  symphonia's own contract. A forward-only source is spooled into memory up to `SPOOLED_AT_MOST` for
  as long as `SPOOLED_WITHIN`, then is a seekable source like any other; past either it is spooled onto
  the disc, plays as it arrives with the prescan of its head alone, and can seek once it has all
  arrived.
- **`Sources` resolves by a linear walk over the registered providers** — right for the handful a
  desktop player registers, wrong for hundreds.
- **A non-filesystem provider is waited on for `Sources::OPENED_WITHIN` and no longer.**
  `Sources::open` asks `SourceId::local()` in line — a scan opens hundreds of thousands of files and a
  thread each would cost more than the opens — and any other provider on a `resonate-open` thread of
  its own, taking `Error::OpenTookTooLong` where nothing answered within five seconds, or what
  `Sources::opening_within` says. The engine reads that as a row that will not open and passes it
  over, the tag reader as a row with nothing to say, so neither is held while a remote open hangs. What
  is given up on is not stopped: the thread stays with the provider until it answers and its answer is
  dropped, so a provider is still expected to put its own deadline on the network.
  `a_provider_that_does_not_answer_in_time_is_given_up_on_and_one_that_does_is_heard` is the claim.
  **Its stream's reads are waited on the same way.** What such a provider opens is a `Deadlined`
  stream: every read and seek goes to a `resonate-read` thread holding the provider's stream and is
  waited on for `Sources::READ_WITHIN` (5 s), or what `Sources::reading_within` says. A read not
  answered in time is an `io::ErrorKind::TimedOut` and leaves the stream stalled — every later call
  fails at once, the answer still owed being bound to land out of order — so the engine fails the
  track and passes on rather than hanging on a network gone quiet.
  `a_read_that_does_not_answer_in_time_fails_and_leaves_the_stream_stalled` is the claim.
- **A non-filesystem track is opened off the engine thread.** `Engine::start` opens a local row in
  line and hands any other to an `Opening`: a `resonate-track-open` thread runs the whole
  `Unwrapped::open` — decoder, hints and box layout — and the engine parks on its answer beside its
  commands, publishing `Loading` meanwhile. Every command is answered while it waits: play or pause
  change only whether the track plays once it lands, a seek moves where it will start, and a stop, a
  load or a skip drop the opening, whose answer nobody reads. A failed landing is passed over while
  playing — `Engine::fail`, as for a local row that will not open — and is an `Event::Failed` with the
  transport stopped otherwise; a thread stopping without answering is `Error::OpenerStopped`.
  `a_source_slow_to_open_leaves_the_engine_answering_while_it_waits` and
  `a_source_that_refuses_after_a_wait_is_passed_over_for_the_next_row` are the claims.

## Transport

- **Bit-perfect output takes precedence at every track boundary. There is no gapless playback** —
  rejected, not deferred. On a track change the engine drains the ring and reopens the stream. Once
  the ring runs dry it asks the graph to drain and waits for `StreamEvent::Drained`, so the boundary
  falls where the graph says it played the tail out, not where a clock guesses; `SinkStream::latency`
  on the wall clock is the fallback for a backend that never answers. A stream takes every
  instruction through one `StreamCommand`, keeping that handshake, `set_active` and `close` on one
  ordered channel to the loop thread.
- **Bluetooth headphones can be kept from cutting the start, off unless asked.** Headphones power
  their radio down when nothing is sent, and the first moment of sound after is spent waking the link,
  so a Play after a pause, or a queue started on a sleeping device, lost its opening. `BluetoothWake`
  is `on`, a `lead` and an `awake_for` — the `bluetooth-wake`, `bluetooth-lead-ms` and
  `bluetooth-awake-s` keys, the Output category's *Bluetooth* group — and touches only a sink
  `SinkInfo::is_bluetooth` names (a `bluez_output.` or `bluez_sink.` node). Two things change there.
  **A pause fades the ring out rather than standing the stream down**: the fade every pause takes (below)
  ends with the consumer feeding the graph real silence — zeros for PCM, the marked word for DoP —
  without consuming a frame, the stream stays active and the link up, and `Output::awake_since` says
  since when; Play fades back in and is heard at once, and once `awake_for` has passed `let_the_link_rest` stands the stream down as a pause always
  did, so the headphones still sleep, `budget` waking the loop for that moment. **A stream starting on
  a link that has slept opens on silence**: `RingProducer::lead_in` hands the consumer a count of
  frames to pad before reading the ring, spent a callback at a time, so the clock stays at the start
  until the lead is out and nothing of the track plays into a waking link. Whether the link slept is
  `Engine::sounded` — the sink and the last moment a stream on it was active or held, noted every pass
  — against `LINK_NAPS_AFTER`, so a track change, reopening within milliseconds, pays no lead and no
  gap. `a_ring_faded_out_ramps_to_silence_then_keeps_every_frame_it_still_holds` and
  `a_lead_in_is_silence_before_the_first_frame_and_then_the_ring_plays` are the ring's claims.
- **Nothing the listener does cuts the waveform where it stands; it fades over `FADED_OVER`.**
  Deactivating a stream, closing it or dropping what the ring holds stops the music mid-cycle and
  resumes it on a sample that is not zero, a click each time. The fade has to be the consumer's,
  the ring already holding up to `buffer-ms` of rendered audio when a pause arrives: `Fader` is
  shared by the ring's two ends, the producer asking for silence or sound (`fade_out`, `fade_in`, the
  frames a whole fade takes) and the consumer walking its `level` toward it a frame at a time,
  scaling the popped samples in their own format (`scale`: `i16`, the `i32` a 24- or 32-bit sample
  sits in, `f32`), popping no further than the frames the fade needs and then feeding silence
  without consuming, and saying so through `is_quiet`. Away from a fade the level is exactly one and
  nothing is touched, so a stream is bit for bit what it was. A DoP ring is never scaled — a marker
  multiplied is noise — and goes quiet at once on its marked silence. **Pause** asks for silence and
  stands the stream down once the consumer is quiet (`finish_fading`), or once `quiet_within` has
  passed — the fade and two sink latencies, between 40 and 400 ms — where the graph never pulled to
  hear it; **Play** asks for sound again and activates. **Stop, a track change and every rebind**
  retire the output rather than closing it: it fades out as `Engine::retiring` and is closed once
  quiet, and `promote` opens no new stream until it is, the client holding one playback stream at a
  time — a hand-picked track starts a fade's length later rather than on a click. **A seek in place**
  fades the old audio out before the consumer drops it (`fade_into_the_discard`) and the new audio
  back in as the refill starts playing. **A stream opened mid-track** — a rebind at a position, a
  seek the ring could not take, a resumption — opens `Entering::FadedIn`; one opened at a track's
  start opens whole, so a gapless run and a bit-perfect first frame are untouched. Only a stream the
  graph is actually pulling fades: the consumer counts its pulls and `Output::is_sounding` wants one
  within `PULLED_WITHIN` (250 ms), so a paused, primed or starved stream closes at once rather than
  waiting on a fade nobody hears. `a_pause_fades_the_level_out_and_a_play_fades_it_back_in`,
  `a_stop_fades_the_level_out_before_the_stream_closes`,
  `a_seek_fades_out_what_the_ring_held_and_fades_in_what_follows` and
  `a_marked_ring_is_never_scaled_and_goes_quiet_on_its_markers` are the claims.
- **A seek does not reopen the stream.** rtrb gives the producer no way to drop what the consumer has
  not read, so the ring carries a discard epoch: the engine bumps it and stops writing, the graph
  thread drains every slot it holds on its next callback and acknowledges, and only then does the
  engine refill from the new position; one not acknowledging after `DISCARD_TIMEOUT` is taken as
  stalled, warns and has the stream rebuilt around the seek. The graph is fed the stream's own silence
  until the ring is half full again — the mark `Output::primed` uses before a stream opens — so a seek
  costs one clean gap, not a run of underruns, and the gap is not reported as a fault. What that
  silence *is* comes off the plan, not a `memset` (DoP zeros are no silence — DSD above). A seek while
  paused is the exception and reopens: a graph not calling `process` can never acknowledge the discard,
  so the in-place path would leave the engine unable to refill until playback resumed; reopening leaves
  the ring primed at the new position, which is what makes a paused scrub resume instantly. A pause
  landing *after* an in-place seek is the same case caught late, read as `Unanswered::NothingPulling`
  rather than a stall: the stall clock is not held at now while nothing pulls, since a clock that can
  never run out leaves the ring discarding, `fill` returning nothing and the next Play starting empty.
  A sink switch and a renegotiation still reopen, the negotiated format changing with them.
- **Leaving or returning to unity gain, and switching the equaliser, reshape the chain in place.**
  `retune` builds the wanted plan against the open stream. Where `OutputPlan::same_shape_as` holds it
  retunes the chain it has; where not but `OutputPlan::becomes_on_the_same_stream` does — same
  `stream`, `packing`, `remix`, `resample` and convolution (the same `Arc<Impulse>`, weighed by
  pointer, so a retune never compares a response tap by tap), and under a resampler or a convolver
  the same `restoration` — it builds
  the new chain with `build_chain` on the engine thread and swaps it in under the ring, stream and
  consumer it holds, so the ring still carries the negotiated format and nothing reaches the realtime
  thread. The new chain's gain stage is handed what the old one applied — `Chain::gain_amplitude`, or
  unity where it had none — through `Chain::ramp_gain_from`, ramping to its own target over the usual
  ramp, so the level never steps. The decoder is pointed at the new plan's `delivery()` and samples
  decoded but not yet converted are retyped with `AudioBuffer::retype`, nothing dropped or read twice. A
  converting plan's delivery is the source's own word — `OutputPlan::decoded_as`, F32 only for a float
  source and decimated DSD — so the retype is exact for every source, 32-bit integer included.
  `Output::take_chain` writes the new plan and mode into what is published and the swap emits
  `Event::OutputChanged` as a rebind does. Going back to a plan with no gain stage — the volume at
  exactly 100 % with nothing else in the chain, or the equaliser off with nothing left — first retunes
  the running chain's gain to unity, holds the wanted plan in `Output::settles_into`, and swaps only once
  `Chain::is_ramping` says the ramp is over, which `finish_reshaping` reads after every `pump`. The
  equaliser switched off while playing waits likewise for its stage to ease out to dry, and one switched
  on eases in on the new chain (`eq.md`). A stage already at unity has nothing to ramp (`GainStage` does
  not ramp to where it stands), so the equaliser off at full volume swaps at once. A newer command drops
  what was waiting and is judged against the chain still running; a rebind replaces the `Output` and
  the wait with it. **A resampler and a convolver are carried across, not rebuilt.** A resampler's
  history and phase cannot be handed to a fresh one without a click — a new resampler starts from
  silence, and a steady level stepped by a twentieth of full scale where the equaliser came and went —
  and a convolver's is worse: flushing it put the response's whole decay, up to `LONGEST_IMPULSE`,
  into the ring ahead of the music, and a fresh one opens on a partition of silence. So where
  `OutputPlan::carries_the_front_into` holds — the plan has a resampler or a convolver and becomes on
  the same stream — `Chain::take_the_front` lifts the stages up to and including the last one
  `Processor::is_carried_across_a_reshape` names off the running chain, the rest is flushed into the
  ring as a whole chain's would be, and `build_chain_after` builds the new stages behind the very
  stages that ran. The room a swap waits for is what the stages *behind* the front hold
  (`Chain::max_flush_frames_behind_the_front`), not the convolver's tail, which the pump never leaves
  free; and while a swap waits for room and no ramp is running, the pump stops filling
  (`Output::waits_for_room_to_reshape`), so the graph's pulls make the room rather than the pump
  taking it back each pass. A swap landing while the ended track's tail is still draining from
  `staged` flushes nothing and stages nothing, the old chain having nothing left to give and `staged`
  being what `drain` still reads.
  `ChainBuilder::build` prepares only what it is handed new, `Restore::prepare` rebuilding its buffers.
  It used to rebind, so switching the equaliser while resampling cost a sink switch's gap.
  `switching_the_equaliser_on_and_off_under_a_resampler_keeps_the_stream_and_its_level` and
  `a_chain_split_at_its_resampler_goes_on_exactly_where_the_resampler_left_off`,
  `a_convolver_is_carried_across_a_reshape_and_picks_up_where_it_left_off`,
  `an_equaliser_switched_on_under_a_convolver_takes_hold_mid_track_without_a_gap` and
  `an_equaliser_switched_on_while_the_tail_drains_lets_the_whole_tail_play` are the claims. A
  restoration switched under a resampler or a convolver still rebinds, standing in front of it; a volume or ReplayGain
  change under a resampler never does, a converting plan always carrying a gain stage (DSP) and being
  retuned rather than reshaped.
- **A setting asking for the stream already open does not reopen it.** The rate policy, DoP and the
  buffer each used to rebind, so switching DoP on under a PCM file, following the graph's rate where
  the file already runs at it, or nudging a buffer the ring's floor absorbs cost a sink switch's gap for
  nothing. `Engine::keeps_its_stream` asks `plan_output` what the bound sink would open at now and weighs
  it against the open plan's stream and packing and against `ring_capacity` for the depth, and
  `reopen_where_the_stream_moves` rebinds only where one of the three moved. The graph rate is not
  weighed so: `clock.force-rate` pins the graph for as long as the stream stands, whatever its rate, so
  switching it still reopens. `a_setting_that_leaves_the_stream_as_it_was_does_not_reopen_it` is the
  claim.
- **A renegotiation converges because the next plan asks for exactly what the graph answered.**
  `StreamEvent::FormatChanged` carries the spec the graph settled on, and `downgrade` hands it to
  `plan_for` as the target, which writes it into `OutputPlan::stream` whole — rate, format and channel
  layout alike. Writing the source's channels back in made the two trade turns forever, each round a
  teardown, an enumeration and a gap. `MAX_RENEGOTIATIONS` (3) is the belt to those braces: a graph
  answering a fourth different spec fails the track with `Error::Renegotiation` rather than looping, the
  count reset by `Engine::start` so it is per track, not per run.
- **The run loop waits on every channel that can change its mind, and the ring sets the timeout.** A
  command, a sink announcement and the stream's events each wake it, so `StreamEvent::Drained` closes a
  boundary the moment the graph reports it. The timeout is half what the ring holds, floored at the
  4 ms `SHORTEST_TICK` so a primed ring cannot spin it and capped at the 16 ms `PUBLISH_TICK`
  `Published` is refreshed at (what a 60 Hz front end draws from) — about a third of the wakeups a
  fixed 4 ms tick cost. Nothing streaming makes it the 100 ms `IDLE_TICK` flat, and a transport *at
  rest* — not playing, no sleep timer counting, the graph not lost, the sink list not stale, and any
  output wanting no filling, discarding nothing, with no reshape waiting and no gain ramping — parks the
  one-second `AT_REST_TICK`, every change that could move it arriving on a channel the `Select` wakes
  for: a paused window woke ten times a second for nothing and now wakes once. A disconnected receiver
  reports ready forever, so `changes` is dropped and `Output::deaf` set the first time one does. What
  paces the loop while playing is publishing, not the ring, since a reader polls `Published` rather than
  being told: a full buffer is still woken over sixty times a second.
- **The set the loop parks on stands as long as it stands, which is why `run` is two loops.** A
  `Select` borrows the receivers it is registered over, and the stream's lives inside the `Output` every
  `&mut self` method replaces, so one built from `&self` cannot outlive the body that rebinds — why it
  was rebuilt from scratch at up to 250 Hz, a `Vec::with_capacity(4)` and three registrations a wakeup,
  for a set that changes only when a stream opens, closes or goes deaf. `Heard` is that set lifted out:
  `Engine::heard` clones the three `Receiver`s (sharing their channels, not copying), the outer loop
  owns it while the inner one parks on it, and `Heard::is_what`, read at the foot of each pass, compares
  by `same_channel`, so the inner loop breaks and the set is rebuilt exactly when it changed — which
  also keeps the outer loop from spinning, a set just built always being what the engine holds.
  `unsafe_code = "forbid"` ruled out the self-referential field this replaces.
- **Enumerating sinks mid-stream happens on a thread of its own.** A `SinkChange` sets a flag, and the
  next tick hands it to `Surveying`: a `resonate-sinks` thread holding the backend's `Surveyor` — for
  PipeWire a `Survey`, the loop sender, the discovered state and the connection flag, all two round
  trips need — answering on a channel `Heard` parks on beside commands, changes and stream. The engine
  never waits on the graph's answer, and nothing depends on how much the ring holds: it once enumerated
  in line only while the ring held twice a 50 ms budget, which a high-rate stream's 8 192-frame ring
  never did, so a new device, a new default and the playing device leaving all waited for the next
  open. `a_stream_whose_ring_holds_under_a_tenth_of_a_second_still_follows_the_default` is the claim.
  One survey is in flight at a time; a change announced meanwhile leaves the flag set and the answer
  landing asks again. Where the thread could not start, the engine enumerates in line only while no
  stream is open, under the 2 s `SINK_TIMEOUT`, and otherwise leaves the list for the next bind.
- **A bind reads the published sink list; only an announcement refreshes it.** `select_sink` is on the
  path of every track change, every seek not done in place, every settings change and every
  renegotiation, so enumerating there put a 2 s `SINK_TIMEOUT` before each against a daemon that stopped
  answering. The change stream's announcements make the list stale, and `note_sink_changes` is read
  before the bind as well as on the tick, so a device that just appeared is bound to and one that did
  not costs nothing. The three standing reasons to ask the graph again are a stale flag, an empty list
  and a change stream gone — and where the ask fails with a list still published, the bind takes it
  rather than failing the track.
- **The engine decides which device the stream is on, and follows the default itself.** A playback
  stream carries `node.dont-move`, so WirePlumber never relinks it behind the engine's back — its
  `linking.follow-default-target`, on by default, wrote `target.node = -1` for a stream on the default
  and moved it whenever the desktop chose another device, so the sound went to the new device while
  `OutputStatus::sink`, written only by `Output::open`, named the old one, and a device chosen by name
  that happened to be the default was taken off it. Instead every sink-list refresh ends in
  `follow_the_sink_it_would_choose`: where `chosen_sink` — the reading `select_sink` binds through: the
  named device, else the default, else the first — answers a device other than the one the output
  opened on, the row is bound again at its position. So a new desktop default, a named device appearing
  mid-track and the playing device leaving each reopen the stream on the right device with a plan made
  for it, and the window names what is heard. The cost: a desktop's per-application device picker
  cannot move the stream (which lasted only to the next track anyway, each track opening its own stream
  under its own `target.object`); the settings pane chooses the device.
  `a_stream_following_the_default_moves_when_the_desktop_chooses_another` and
  `a_device_chosen_by_name_stays_bound_when_the_desktops_default_moves` are the claims.
- **A graph letting go of the ring is waited for once, and fails the track the second time.**
  `RingProducer::is_abandoned` says the consumer was dropped — the graph thread gone with the
  `AudioSource` it was handed, as a daemon restart does, the client dropping every stream of the lost
  connection. `watch_graph` reads it once the stream is open, closes the output, keeps the row with the
  heard position in `unbound` and publishes `Loading`; each pass after marks the sink list stale, so
  the survey thread asks the backend for its sinks, and the answer landing binds the row again at that
  frame (`bind_the_row_the_graph_let_go`), so a restarted daemon costs a gap, not the track, and the
  engine is never held two seconds a pass by an enumeration in line — only where no survey thread
  could start does `wait_for_the_graph_in_line` still ask there. It waits `GRAPH_BACK_WITHIN` (10 s)
  before failing the row with what the last try said (`graph_still_away`), and a
  pause, a stop or another row ends the wait. A graph letting go again within ten seconds of the last
  time is not waited for: it raises `Error::LoopStopped`, so the transport skips and eventually stops
  rather than opening and losing streams for ever (`graph_last_lost` is that memory). Asking the sinks
  first matters: a bind succeeds against the stale list and fails only when the stream opens, so trying
  the bind alone read a daemon still down as back. A disconnected *event* channel is a different
  reading and stays `Output::deaf`, dropped from the select, since a graph can stop reporting and go on
  pulling. A failing `backend.open` takes the whole `Output` with it likewise: the consumer went into
  the call and cannot come back, so an output outliving it would hold a ring with no stream and no way
  to open one.
- **A device going is waited out, not billed to the row.** `Engine::fail` first hands its error to
  `parked_for_a_device`: `NoSink`, `SinkGone` and `StreamFailed` with a track open close the output,
  keep the row in `unbound` at the heard position, publish `Loading` (`Paused` where paused), mark
  the sink list stale and raise `Event::Waiting` rather than `Event::Failed` — the window toasts
  `toast::waits_for_a_device`, `resonate play` prints it. Every sink-list answer then ends in
  `bind_the_row_waiting_for_a_device`, which binds the row again where it was heard: on the fallback
  where the desktop has one, on the next device to appear where it has none, and a `NoSink` meanwhile
  leaves it waiting. Before, a DAC unplugged with nothing else failed every row in turn and finished
  the queue, and with a fallback the stream's failure arrived ahead of the list and skipped the
  track. A stream that fails again within `GRAPH_BACK_WITHIN` of the last loss is the row's after
  all — a format the device will not take — and fails it the usual way (`device_last_lost`). A
  `Load` answering `NoSink` to its caller is unchanged: nothing was playing to wait
  (`a_stream_failing_as_its_device_goes_moves_the_row_to_the_fallback_rather_than_skipping_it`,
  `a_device_going_with_none_left_holds_the_row_until_one_comes_and_plays_on_where_it_was_heard`).
- **A fill is a slice, not a loop to a full ring.** `Engine::fill` decodes and writes for at most
  `FILLED_IN_ONE_GO` (20 ms) and answers `Filled::ForNow` where it stopped with room left; `pump`
  keeps that as `fill_owed`, and `budget` answers no wait while it stands, so the next slice runs
  once the pass has taken the commands waiting. A prime, a seek's refill or a `SetBuffer` deepening
  the ring to seconds used to hold a Pause or a Stop behind the whole of it — 1.7 s behind a slow
  source's twenty-second ring (`a_pause_is_answered_while_a_deep_ring_is_still_priming`).
- **The PCM ring carries `u8`, not `f32`.** An `f32` ring would convert every stream and break
  bit-accuracy for 32-bit sources a 24-bit mantissa cannot hold. Its depth is the buffer setting's to
  ask and `ring_capacity`'s to answer: clamped between `MIN_RING_FRAMES` (8 192) — or
  `BLOCKS_A_RING_HOLDS` of the widest block one pass through the chain writes
  (`Chain::max_process_frames`), where more — and what `LARGEST_RING` bytes come to
  at the negotiated format, so a mistyped `buffer-ms` sizes a ring rather than aborting the process on
  the first open. The block is weighed because the converting `fill` writes only while the ring has room
  for one and `primed` waits for half the ring: an 8 kHz file upsampled onto a 192 kHz sink writes
  24 577 frames a block, which a 100 ms ring of 19 200 could never take, and the track sat in
  `Buffering` with no error. A flush is not a block: a convolver's tail is handed on from `staged` a
  ring's room at a time (`drain`), so it is weighed nowhere in the depth. Weighing it had a
  ten-second response ring twenty seconds whatever `buffer-ms` said — every start and seek rendered
  that much before there was sound, a volume change was heard that late, and the visualiser's tap
  outgrew `LARGEST_TAP`
  (`a_convolver_leaves_the_ring_as_deep_as_the_buffer_asks_whatever_its_tail`). `Frames::from_duration` and `StreamSpec::frames_to_bytes` saturate for the
  same reason. **The ring's depth is the engine's, never the graph's.** Every stream asks
  `LatencyRequest::Auto`, leaving `node.latency` to the daemon: a quantum is a few milliseconds shared by
  every client on the device, and a ring of 100 ms to a second written into it would drag the whole
  graph to PipeWire's largest quantum. The depth decides how long a volume, ReplayGain or equaliser
  change waits to be heard (the gain runs before the ring) and how much an opening stream primes; a seek
  discards the ring and a pause deactivates the stream, so neither waits on it. The settings pane's
  *Buffer* hint says so, rather than letting *latency* in its search words suggest otherwise.
- **A track change does not start the transport; a command does.** `Engine::start` opens the row the
  queue is on at whatever the transport was doing, so `Next`, `Previous` and removing the playing row
  leave a pause in place — what MPRIS says those methods do, and what `transport.rs` asserts by
  watching the fake graph stay idle. `Load { autoplay }`, `Play`, `Command::JumpTo` and an `Insert` asked
  to be heard set `playing`, each a request to hear something now. A track failing mid-play still
  advances into playback, the transport having been playing. **Nothing to hear sets nothing:** a
  `Load` whose rows are empty leaves `playing` clear whatever `autoplay` says, and `Play` over a
  queue with no current row answers `QueueEmpty` before touching it, so the first play/pause after
  rows arrive plays rather than answering `InvalidTransition` for a pause of nothing
  (`a_load_of_nothing_leaves_the_transport_ready_to_play_what_comes_next`).
- **A track change nothing will be heard through opens the file and binds nothing else.**
  `Engine::start` reaching a paused transport records the frame it would bind at in `Engine::unbound`
  and stops, so a run of skips through a paused queue costs one `Unwrapped::open` a row rather than a
  sink selection, a ring, a DSP chain, a decode to half the ring and a `Backend::open` each, all thrown
  away by the next skip. Every `rebind` while `unbound` is set records the frame instead of binding, so
  a setting changed or a seek taken over a paused row does not quietly pay the same; `Engine::play` is
  the one place spending it, and a bind failing anywhere leaves `unbound` set so the next `Play` tries
  again: `rebind` drops the old output before `select_sink`, `Output::open` or the decoder's seek can
  refuse, so `bind` is the part that may fail and `rebind` records the asked frame whenever it does —
  an autoplaying load with no sink answers `NoSink`, and the `Play` after a sink appears binds the row
  rather than calling `set_active` on a stream that is not there. The row still publishes what it is —
  `TrackState` comes off the open decoder, so id, source spec and duration are there — but not
  `PlayerState::output`, which stays `None` until something is bound, there being no plan to report.

## The queue

- **A queue position is an index into the play order, not the load order.**
  `PlayerState::queue_position`, `Command::JumpTo`, `Command::Insert` and `Command::Remove`'s `Span`
  all mean the rows a queue pane draws, so they stay right under shuffle. `Queue::position` is the
  load-order index behind them, published as `PlayerState::loaded_position` for the one reader drawing
  the order the queue was *loaded* in: an opened playlist, marking the row being heard from it. A file
  no scan has seen takes its `TrackId` from `unclaimed_id`, counting down from `u64::MAX` past the ids
  the queue holds, so no front end needs a counter and two cannot mint the same id;
  `Unclaimed::beside` is that walk taken once and counted down for a whole run, what queueing a
  playlist of unscanned files goes through rather than walking the queue per row. **A file the
  catalog holds is queued under the catalog's id wherever there is a catalog to ask.** `Host::held_as`
  answers the library row for a location and span, and `AddTrack` and `OpenUri` ask it before they
  mint; the binary's `mpris::claimed_by` does the same for files `resonate play` and the window are
  handed, so a file queued by path is its library row
  (`a_file_the_catalog_holds_is_queued_under_the_catalogs_own_id`). A resumed row likewise:
  `Resumable::held` is the row `Library::resumption` finds at that path and span, and
  `Queue::restore` claims it before it mints.
- **A queue row's id names that row and no other, and the queue makes it so.** `one_id_each` runs
  over everything `Queue::load` and `Queue::insert` are handed: a row whose id is already claimed —
  by the queue it joins or an earlier row of the batch — gets a minted one, `Unclaimed::claim` marking
  an id taken so a mint cannot hand it out. It must be the queue, not the callers, since two of them
  mint against the queue the engine last *published*: a scanned row carries its library id, so one
  album queued twice arrived as two rows under one id, and two `AddTrack`s in one 200 ms poll minted
  one id from the same stale snapshot. The bus is what the duplicate broke — `Tracks` published one
  object path twice and `row_of` resolved it to the first row, so `RemoveTrack` and `GoTo` could not
  reach the second — and `one_file_queued_twice_is_two_rows_the_track_list_can_tell_apart` is the
  claim, over a real session bus.
- **A queue is stamped by the rows it holds, not the ids they arrived under.** `stamp_of` hashes each
  row's location and span, and `Queue::rows_changed`, `RootView::play_playlist` and the binary's
  `play_queue` all read through it, so a row the queue renamed on the way in still stamps as its
  playlist. Hashing the whole `QueueItem` would make `one_id_each` look like an edit.
- **What was queued is kept apart from what is playing, between the row heard and the rest.**
  `Queue` holds two lists, as Apple Music does: `order`, the loaded album or playlist — what the queue
  is *playing from*, the only list shuffle, a wrap under `RepeatMode::Queue` and unshuffling touch —
  and `next`, the rows somebody asked to hear, in asking order. `after` says how many rows of `order`
  are drawn ahead of `next` and `Seat` whether the row heard is `order[after - 1]`, `next[0]` or
  nothing (`InOrder`, `Next`, `Nowhere`), so the one list every reader draws is
  `order[..after] ++ next ++ order[after..]` and a position still means a row of it. Playing moves
  through `next` before `order` goes on, and a heard or skipped-past row of `next` *leaves the queue*
  rather than joining the playlist — why `PlayerState::queue_stamp` is `Queue::playing_from`, the stamp
  of `order`'s rows alone, and `Library::playing_playlist` still badges the playlist while a queued row
  plays; `Queued::stamp` is every row, what `Keeping` weighs. Going back from a queued row leaves it
  queued, and `next` follows whatever `order` row is heard, so *Previous* or a jump into the playlist
  keeps what was queued waiting after it; choosing a queued row hears it and leaves the rest queued. A
  load replaces `order` and keeps `next`, and a resumption carries `next` as `Resumption::next`, its
  span in the drawn list, held in the `resume` table as `next_first` and `next_last`.
- **Loading replaces what is playing; queueing adds to what waits, and where a row lands is a
  `Placement`.** `Command::Insert` carries `Placement::Next` — the front of `next`, behind a queued row
  being heard — `Placement::Queued` — the end of `next` — or `Placement::At(row)`, a row of the drawn
  list, joining `next` wherever the row before it is the one heard or a queued one and `order`
  otherwise, so *Put back* and MPRIS's `AddTrack` land a row where they name. With nothing heard there
  is nothing to wait behind, so every placement lands in `order` and the queue is left on the first row
  it placed. A drag sorts a row the same way, by the row it lands after, and a row put into `order`
  takes the queue out of `Library::playing_playlist`, no longer being the playlist loaded — restamping
  the queue rather than clearing the cell, so the badge returns if the row is dropped again. Whether it
  starts the transport is the `play` flag beside them (next note). `RootView::queue` takes
  `PlaylistEntry`s — already the vocabulary for a location and its library row where there is one — so
  a scanned track and an unscanned playlist row reach the queue alike (`root::listed` turns the first
  into the second).
- **`Command::Insert` carries whether to hear what it queued, and the row it starts is the one the
  queue answered with.** `Queue::insert` always returned the play-order row the items landed on and the
  engine threw it away; `play: true` spends it through the `Engine::hear` `Command::JumpTo` takes — jump,
  set `playing`, `start(Frames::ZERO)` — so the two cannot disagree about starting a row. An insert of
  no items answers `None` and starts nothing. It makes MPRIS's `SetAsCurrent` one command: `add_track`
  sent an `Insert` then a `JumpTo(at)` against a row computed off the published queue — a reading taken
  before the insert and a second dispatch for the queue to move under — and the binary's `Host::open`
  had the same shape. `RootView::queue` asks for it wherever `PlayerState::current` is `None`: queueing
  into a playing queue queues as always, and queueing when nothing plays plays what was queued — *queue
  it and hear it now* — and `queued` has a third reading for it, a run that started playing never
  having been queued to play next.
- **The queue is put in order in one pass.** `Command::Order` is a permutation of the play order
  applied at once, with the cursor re-seated, so a sort mid-track reopens no stream.
- **Coming back is a command of its own, a resumption being more than a row to start on.**
  `Command::Load` carries the rows and `start_at`; `Command::Resume` carries a whole `Resumption` — the
  rows as they were playing, where each was loaded, the row the queue was on, the frame into it and
  whether it was shuffled — the only caller of `Engine::start(at)` passing anything but `Frames::ZERO`,
  where `rebind`'s seek guard sees the decoder already there and issues nothing. A frame past the end of
  what the row now holds — a file replaced by a shorter one — opens the row at its start rather than
  being handed to a seek that would refuse it on the resume and every `Play` after. A load playing
  nothing and starting nowhere opens no track, as always; a resumption opens its row and stops, since
  `start` reaching a non-playing transport records the frame in `Engine::unbound` and binds no sink — so
  a resumed queue costs one `Unwrapped::open` and one decoder seek, shows position and length at once,
  and spends the bind on the first `Play`. It is not a seek: `Seeks` is not stepped, so a run opening on
  a resumed row announces `Metadata` and no `Seeked`. It replaced a `from` frame on `Load` — a field to
  take out rather than keep — that every caller but one passed `Frames::ZERO`; ids are not kept either,
  so `Queue::restore` mints through the same `Unclaimed::beside` an unscanned file takes. The queue
  opens paused where it was left, playing the order it was playing, and plays only when told. Only the
  bare `resonate` resumes, every other way in naming what to play; turning `resume` off discards what
  was kept rather than merely ceasing to add, in the settings pane at once and on the next run reading
  the key.
- **The unshuffled play order is kept apart from the load order.** `Queue::unshuffled` is what the
  queue plays with shuffle off: a drag, a sort or a play-next while unshuffled edits it along with
  `order`, and turning shuffle on and off comes back to it rather than to the load order. Under shuffle
  it keeps its shape — a removal drops the row, a row queued last lands at its end, a row queued
  elsewhere lands after the row it follows in the shuffle, so a play-next made while shuffled still
  follows the playing row once unshuffled. Not kept across runs: a shuffled resumption comes back with
  the load order beneath it — unshuffling gives back the album, as the next note means.
- **A shuffled pass never opens on the row the last one ended on.** A wrap under
  `RepeatMode::Queue` reshuffles the whole order, and a fair shuffle puts the row just heard first
  one time in the queue's length — every other wrap of a two-row queue. `reshuffle_after_a_pass`
  swaps it with a row picked from the rest, so the same row is never heard twice running across a
  pass (`a_new_shuffled_pass_never_opens_on_the_row_the_last_one_ended_on`). A one-row queue has
  nothing to swap with and repeats it, as it must.
- **A queue comes back in the order it was loaded and plays in the order it was playing.**
  `Resumption::rows` is the load order and `Resumption::order` the play order over it, one entry per
  playing position naming the loaded row there — the pair `Queue` holds as `items` and `order`, so
  `Queue::restore` takes them as they are. `resonate-core::plays_in` makes trusting a stored order safe:
  an order naming each row exactly once is taken as it stands, and one of the wrong length, naming a row
  twice or a row the queue lacks gives back the load order, so a queue is never refused nor left a row
  short over an unreadable number. `Resumption::shuffle` rides beside the rows, the order alone not
  saying what the next wrap or toggle does, and `Queue::restore` writes the flag rather than calling
  `set_shuffle`, which would reshuffle the order just handed. Volume and repeat mode are deliberately not
  kept: they are settings a run makes, not a place it reached.
- **What the transport was doing is sampled as a play is, and the answer says how much moved.**
  `Keeping` sits beside `Listening`, reading the same `PlayerState` and published queue, and its three
  answers are the catalog's three tables. `Queued::stamp` moving — rows arriving or leaving, queued ones
  among them — answers `Keep::Queue` with the whole run. `Queued::revision` or the shuffle moving where
  the stamp did not answers `Keep::Order` with the order alone — a drag, a toggle, or a wrap's
  reshuffle under `RepeatMode::Queue`: the same rows, so rewriting a URI per row said nothing. Anything
  else answers `Keep::Place`, and only where the row changed, the position moved `KEPT_EVERY` (1 s)
  either way, or the transport is at rest somewhere other than the place kept, so a 60 Hz observer
  writes a row about once a second while playing and once more where it stops. A sample with no queue
  position — a queue played to its end — answers no `Keep::Place`, so the last real place stands rather
  than being overwritten as the first row at its start. The two triggers are read together because
  `QueueStamp` is `stamp_of(self.items)`, the rows in load order, where the revision counts every
  republication including a reorder; the shuffle is weighed beside the revision, riding on
  `PlayerState` while the order rides on `Queued`, so a sample reading them a beat apart is corrected by
  the next. Only the first allocates a `Resumption`, keeping a 2 000-row queue from being cloned every
  position tick. The cost: a run killed mid-play loses up to `KEPT_EVERY` of position; one killed paused
  loses nothing, the pause itself being written
  (`a_transport_that_comes_to_rest_keeps_where_it_stopped_at_once`). It was five seconds, lost to any
  kill and to a pause shorter than it. An *unshuffled* wrap under `RepeatMode::Queue` moves neither
  stamp nor revision (only `reshuffle` bumps it; the cursor alone returns to nought), so it has always
  taken the `Keep::Place` path. `Resumable`, `Resumption` and `Reordered` are `resonate-core`'s because
  the engine builds them and the catalog stores them and neither may depend on the other — `Span`'s
  argument.
- **A play is what was heard, not what was started, and the transport is sampled, not hooked.**
  `Listening` is the whole rule: handed a `PlayerState` beside the queue in play order, it accumulates
  the frames the position advanced while playing — a step larger than `A_SEEK` (5 s) is a seek and
  adds nothing — and answers with the row's `MediaLocation` once half the track, or the four minutes
  `COUNTS_AS_HEARD` names, whichever is shorter, has gone by; a track declaring no length counts at the
  four minutes. Once per visit, and a position going backwards starts the count again, so a track
  played from the top counts again. A sampler because the write behind it is SQLite:
  `Library::track_played` would leave the run loop waiting behind a scan holding the writer, so
  `RootView::count_a_play` runs it off the window's observer of `PlayerModel` and `resonate play`'s
  loop off a `HEARD_SAMPLE` (500 ms) tick — why a run with no catalog counts nothing.
- **A visit says how long it was heard, twice.** `Listening::heard` answers a `Counting`:
  `Counts(Played)` at the threshold above, exactly where it always answered, and `Settles(Played)` once
  when a counted visit ends — track changed, transport stopped, or the position gone back far enough to
  begin another. `Played` carries `heard`, what was listened to at that moment and, on the settle, the
  whole of it. A visit that never counted answers `Passes` as it ends, with the time heard where it
  reached `PASSES_AT_LEAST` (1 s), so a flick through the queue is not listening and twenty seconds of
  each of forty songs is; the window hands it to `LibraryModel::track_passed` and `resonate play` to
  `Library::passed`, which keeps it in `passes` apart from the plays — listening time counts it, a skip
  is still not billed as a play. `Listened` keeps the stream's rate from the visit's start, the heard
  frames being counted in it and the transport having moved on by the end. The pair is joined by an id,
  not a location: `Library::track_played` answers the `Listen` it wrote (held or unheld), the caller
  keeps it beside its `Listening`, and `Library::listened` spends it — so a failed write keeps nothing
  and a settle can never land on the wrong row. The ending visit cannot be read off the queue, already
  moved on, so `Listened` holds the `Played` it counted with, which is why neither it nor `Listening` is
  `Copy`. Between the two, a counted visit answers `Hears` every `TOLD_EVERY` (30 s) of listening,
  written through the same `Library::listened` without spending the id, so a window closed mid-track or
  a `resonate play` leaving on `QueueFinished` loses at most that much. `Listening::leaves` settles
  whatever visit is open, and `resonate play` calls it on the way out after one last sample.
- **The sleep timer is the engine's, because a headless run wants one too.**
  `Command::SleepUntil(Option<Until>)` carries `After`, `EndOfTrack` or `EndOfQueue`, `None` cancels,
  and the deadline lives on `Engine`, so `resonate play`, the window and the bus share one timer. It
  **pauses**: `Engine::stop` closes the stream, drops the decoder and clears the track, and somebody
  who falls asleep should wake where they were. `doze` clears the timer before acting, and a timer
  running out against an idle transport simply clears rather than asking `pause` for a transition it
  would refuse and logging the refusal. `budget()` takes the minimum of its usual wait and what is
  left, since an idle transport parks `IDLE_TICK` and would overshoot by up to 100 ms (a sleeping
  engine is never at rest, so the 1 s `AT_REST_TICK` never applies). The end-of variants need no clock,
  only the edges in `skip` — and the wrap is the one that would be missed, `advance` never answering
  `None` under `RepeatMode::Queue`, so `Queue::wraps_next` reads the same two fields `advance` decides
  the wrap from, the question living beside the decision. **It fades the music out over its last
  `SLEEP_FADES_OVER` (10 s)** rather than stopping it on a beat: `sleep_is_due_in` answers what is
  left of a delay, or of the track where it ends the track or ends the queue on its last row
  (`Queue::ends_with_this_row`) with a known length, and once that is inside the ten seconds
  `fade_toward_sleep` asks the ring for silence over exactly what is left, so the pause lands on
  quiet. A timer cancelled or pushed back mid-fade brings the level back; a track change mid-fade
  opens whole and is faded again over what remains
  (`a_sleep_timer_fades_the_music_out_before_it_pauses`). The timer outlives a track change, a seek, a
  pause and a new load: it is a timer on the listener, not the transport. A delay is held to
  `LONGEST_SLEEP`, a day, as it is set, so a `SetSleep` of `u64::MAX` seconds or a minute count
  `resonate sleep` saturated reads back as a day rather than an `Instant` overflowing on the engine
  thread (`PlayerState::sleeping` publishes what is left — below).
- **A skip while one track repeats repeats the queue instead, unless the listener said otherwise.**
  `Command::Next` is a person's skip, and so is `Command::Previous` going back a track — a track's end
  reaches `skip` as `natural`, never through either — so `skipped_by_hand` turns `RepeatMode::Track`
  into `RepeatMode::Queue` before moving, as a streaming player does, and a skip past the last row then
  wraps. `EngineConfig::skip_under_repeat` is the policy, `SkipUnderRepeat::KeepsRepeatingTheTrack` the
  way out, `Command::SetSkipUnderRepeat` sets it and `PlayerState::skip_under_repeat` publishes it; in
  the engine rather than the window, so a media key, an MPRIS `Next` and `resonate play`'s `n` all obey
  it. A jump to a chosen row is not a skip and leaves the repeat alone. The `skip-repeats-queue` key and
  the Library category's *Repeating a track* set it.
- **Previous restarts the song once past its opening, unless the listener said otherwise.**
  `PreviousRestarts::starts_the_track_over` reads the heard position — the scrubber's clock, the decoder
  less what ring, chain and sink still hold — against `PreviousRestarts::OPENING` (3 s). Past it, under
  `RestartsTheTrack`, `Command::Previous` seeks to the start and leaves the queue where it is, so the
  window's button, a media key and an MPRIS `Previous` all restart. Within those three seconds, on a
  track no longer than them, with no track open, or under `AlwaysGoesBack`, it retreats as always,
  `skipped_by_hand` included. A track of unknown length is weighed on the heard position alone. A restart
  is a seek, stepping `Seeks` and clearing the heard floor; a retreat runs `start`. It defaults to
  restarting; `previous-restarts` is the key, `Command::SetPreviousRestarts` sets it and
  `PlayerState::previous_restarts` publishes it; the Library category's *The previous button* writes
  the key and sends the command, so it is live.
- **A relative seek past the end moves on, as the bus's `Seek` does.** `Command::SeekBy` landing at
  or past a known length is `Command::Next` — a skip by hand, stepping no `Seeks` — so a held
  arrow key, `resonate play`'s `f 600` and the window's seek-forward all reach the next row rather
  than a `SeekOutOfRange` toast. An absolute `Command::Seek` past the end is still refused, a
  position asked for by value being a mistake where an offset is a direction
  (`a_relative_seek_past_the_end_moves_on_to_the_next_row`).
- **The published position never steps back within a stretch of listening.** It is the decoder's less
  what ring, chain and sink still hold, and the sink's latency is known only once a stream reports it,
  so every rebind and seek used to publish a position a latency behind the last — which `Listening`
  read as a new visit, keeping a seek forward from counting and letting a rebind count a long track
  twice. `heard_position` holds the published value at or above the last, released only by a seek that
  landed, a track opened and a stop — the only three moves backwards a listener makes.
- **Giving up is counted per run of failures, not per track opened.** `Engine::fail` stops the
  transport once more rows have failed than the queue holds; the count is put back by a track played to
  its end, a stop, or a person choosing a row — `Next`, `Previous`, a jump, a load (`settle` and `stop`
  among the resets) — not by a stream opening: a daemon taking every stream then dropping it would
  otherwise reset the count on each open and loop a repeating queue for ever.
- **A row that will not open is passed over, not stood on.** A deleted file, an unmounted disc or a
  playlist row `--tidy` would drop answers `Error::Decode` out of `Unwrapped::open` (via `start`), and
  `past_what_will_not_open` is what every command meaning *play something* runs the start through —
  `Load` with autoplay, `Play` on a row with no track, `Next`, `Previous`, a jump, an `Insert` to hear
  and a removal of the playing row — so where the transport was meant to play the failure goes to
  `Engine::fail`, which reports and skips, rather than back to the caller with the transport standing on
  nothing. `skip` asks whether the *queue* is on a row rather than whether a track is open, so `Next`
  moves off a row that never opened, and `settle` hands a failed natural skip to `fail` rather than
  `stop`. `Error::track` is how `Event::Failed` names the row a failed open was for, `start` having
  cleared `self.track` first. A paused transport is left on the row: nothing is meant to be heard, so
  the `Play` that follows moves on.

## What the engine publishes

- **An event is owed, not dropped, when nobody has drained the channel.** The 256 slots
  `EVENT_SLOTS` holds fill while a front end is busy, and `emit` used to drop what did not fit, so a
  run of track changes or a sustained underrun could push out the `QueueFinished` headless `play`
  waits on. What does not fit waits in `events_owed`, in order, for `hand_over_what_is_owed` at the
  end of every pass; past `EVENTS_OWED_AT_MOST` (1 024) the oldest goes with a warning. An
  `Underrun` is the one event let go, a count read again next time, and it is told at most every
  `UNDERRUNS_TOLD_EVERY` (1 s), the frames missing in between summed into it rather than one a pass
  (`a_queue_that_runs_out_says_so_however_many_events_went_undrained_before_it`).
- **What the engine publishes is one `Published` bundle, not a growing argument list.** It holds the
  `PlayerState`, `OutputSettings`, `StreamDigest`, the queue, the sink list and the `Tapped` the
  visualiser reads, each behind its own lock so a 60 Hz poll clones a pointer, and the one
  `listening` flag the window writes back. The queue is a `Queued`: the rows in play order, where each
  was loaded and the revision both were drawn at, under *one* lock, so a reader cannot pair rows with
  an order from another moment — `Player::queue` hands back the rows alone (all any pane wants),
  `Player::queued` the three together (what `Keeping` reads). The queue and digest are written
  *before* the `PlayerState` advertising them, so a reader seeing `shuffle` turn on cannot hold the
  order from before. `PlayerState::seeks` is a token beside `queue_stamp`: `Engine::seek` steps it
  where the seek landed and nowhere else, so a reader is told the position moved on purpose rather
  than inferring it — a refused seek steps nothing, and a track change goes through `start`, which does
  not step it, moving to another track not being a seek within one. `resonate-mpris` emits `Seeked`
  from it. `PlayerState::sleeping` rides beside it, carrying what is *left* on the timer rather than
  when it is due, so a front end draws a countdown off the publication it polls and keeps no clock;
  the cost is a state differing every tick while a timer is set. The queue is republished only when
  `Queue::revision` moves — loading and reshuffling bump it, advancing does not — since comparing the
  items would clone every path every 4 ms tick.
- **A command is answered once the state answering for it has been published.** `Engine::dispatch`
  keeps each reply as an `Answer` and `Engine::answer` sends them after `publish` at the end of the
  same loop pass, so `Outcome::wait` means *the published state already says so*, not *the engine has
  seen it*. `Reply` is which of three ways a command came: `Player::send` is waited on by nobody and a
  refusal is announced as `Event::CommandFailed`; `Player::request` hands the refusal back in its
  `Outcome` and announces nothing, the asker saying it; `Player::settle` answers a `Landing` saying
  only that the command was applied, while a refusal is still *announced* — what a command from
  somebody the window cannot see wants, so a Next refused over the bus reaches the window's notice.
  `resonate-mpris` settles every call moving the engine — the transport, `Seek` and `SetPosition`, the
  three writable properties, `SetSleep`, `AddTrack`, `RemoveTrack` and `GoTo`, a notification's
  buttons — through `Shared::settle`, waiting `SETTLE` (500 ms) for the landing, so a client reading
  straight after a call reads what it did. The setters needed it first: zbus emits
  `PropertiesChanged` by calling the getter the moment a setter returns `Ok`, so a setter returning
  first announced the value just changed away from, the right one arriving a poll later. A wait that
  runs out is a debug record and the old behaviour, not a refusal: the command is already on its way.
- **What a setting is *set* to comes from the engine, not the pane that set it.** `OutputSettings` is
  the engine's whole configured output as it holds it — sink name, resampler quality and filter
  phase, true-peak guard, restoration, dither and noise shaping, ReplayGain mode and levelling,
  `prefer_bit_perfect`, DoP, DSD-like PCM, device volume, forced graph rate, Bluetooth wake, buffer
  and equaliser — republished as a new `Arc` only when one moves, so the settings pane marks the
  chosen option from what is in force, not what it last sent. It is apart from `PlayerState` because
  the sink *name* is the choice and `OutputStatus::sink` the `SinkId` actually opened: a name nothing
  answers to yet is still the selection. `EngineConfig::sink` being `None` means *follow the default*,
  a choice a pane must be able to make, so `Setting::Sink` carries an `Option<NodeName>` and
  `config::clear` removes the key rather than writing an empty one.
- **The inspector never reads the playing file a second time.** The engine samples
  `Decoder::last_packet` through a `ProfileBuilder` and publishes a `StreamDigest` beside
  `PlayerState`, as an `Arc` so a 60 Hz poll clones a pointer, not the series; `resonate-ui` renders
  it with no dependency on `resonate-codec`. The box layout beside it is the one thing opening the
  source again, and it is the inspector's to draw, not the transport's to play, so `inspected`
  answers `None` and records why where `probe_boxes` fails: a provider serving one handle, an EMFILE
  or a file replaced underneath refuses a drawing, never a track that would have decoded. The analysis
  pane is the other: `Player::analyse` decodes the playing row whole through the player's `Sources`,
  on the window's background executor, never the engine thread, and only while that pane is in front
  (`analysis.md`).
- **What is heard is tapped where it is written, and read back by where the graph has got to.**
  `Tapping` sits on `Output` beside the ring, and `Engine::fill`, `convert` and `drain` hand it exactly
  the frames `RingProducer` took, from the buffer written: the decoded block on a transparent plan, the
  staged words the chain's carrier was narrowed into on a converting one — so equaliser, gain and
  dither are in it — and the staged tail on a drain. It holds what reaches the sink, at the sink's
  rate, as f32 left and right: a mono stream on both sides, a wider one its first two channels. A
  `Tap` is a ring of `AtomicU32` holding f32 bits, a power of two long, sized to the PCM ring,
  `HEARD_SLACK` of the graph's delay and `WIDEST_LOOK` of analysis window, under a `LARGEST_TAP`
  ceiling — 1 MiB at 48 kHz under the default 500 ms buffer, 2 MiB at 192 kHz. It is made with the
  ring on every bind, but its slots are a `OnceLock` laid down by the first frame recorded while
  somebody listens, so a run never opening the visualiser allocates none, and a read before then is
  silence. A write *claims* its frames behind a release fence before laying them and publishes the
  count with release after; a read takes the count with acquire, reads, fences, rereads the claim and
  silences any frame the writer may have gone round onto — nothing locks and the engine never waits on
  the window, the point of a thread with no slack. Where the graph has got to is an anchor
  `Engine::publish` fixes every pass — frames tapped less what the ring holds less
  `SinkStream::latency`, beside the instant and whether the transport moves — written through a
  three-atomic seqlock, never read torn, the writer never spinning. `Tap::around` runs on from it by the
  clock while the transport moves — never more than `RUNS_AHEAD_AT_MOST`, never past what is written —
  and hands back the frames *centred* on the one heard, an analysis window describing its middle, not
  its end; what `pw_time.queued` leaves out of the latency is the error left. `Player::listen_in` is
  the switch: unlistened, a block costs a load and a store and marks the tap empty, and listening again
  marks `valid_from` at the count, so what the ring held when the pane opened reads as silence until it
  has turned over. A seek in place forgets likewise, so the graph's refill silence reads as silence
  here too. `Player::tap` answers `Tapped`: `Nothing` with no output bound, `Samples` with the tap, and
  `Markers` for a DoP stream (no PCM to read) — republished only when it changes, weighed by pointer.
  On the dev profile a listened tap costs 0.006 % of a core at 48 kHz and 0.02 % at 192 kHz, an
  unlistened one 0.0001 % and 0.0004 %. `transport.rs` pauses the fake graph and holds `Tap::around`
  to the very frames last handed, those either side included; holds a turned-down stream to what the
  chain handed the graph, not what the file holds; and holds an unlistened tap to silence and a DoP
  stream to `Markers`.
- **A profile holds at most `MAX_WINDOWS` points, whatever a packet claims to span.** The series is a
  point per `WINDOW` second, and the windows a packet closes come from its declared duration, so a
  `dur` of `u64::MAX` on a sample-accurate timebase asked for 4×10^14. Once full, the open window is
  abandoned rather than closed again, bounding the `Vec` and the walk — on the engine thread, where
  `Track::sample_packet` feeds the same builder every poll, as much as in `probe_stream`.
- **A queued row nothing plays is read once, off the audio path.** `Player::media` answers from a
  bounded catalog a reader thread fills with `probe`; a miss requests the read and answers nothing
  rather than blocking on a disc, and `Player::media_revision` moves when one lands so a 60 Hz poll
  knows to redraw. It lets `resonate-ui` draw a title, artist, length and cover for an unscanned file
  while not depending on `resonate-codec`, and is what `resonate-mpris` answers `GetTracksMetadata`
  from — under one deadline for the whole call, a client able to name every row at once. `Player::art`
  is the picture from the same entry, asked for and landing the same way, moving the same revision.
  The two are bounded apart, a picture not being a row: `ROWS_HELD` caps the entries and
  `ART_BYTES_HELD` their pictures' weight, so a picture drops back to unasked while its tags stay. They
  are asked on channels of their own and the reader takes a picture only when no name waits, so a
  scroll through a queue cannot push `media_within` past MPRIS's whole-call deadline. What is read is a
  `Row` — a location *and* the span the queue row names — since a dozen rows cut from one file are a
  dozen readings of one file: the span is what `probe_span` bills them apart by, and a row with no span
  is the file. Both live in one `Entry` under the location, the whole file's reading beside a `Cut` per
  span, so the map is still keyed by `Arc<MediaLocation>`, a lookup borrows rather than cloning a path,
  the cover is asked once however many rows a sheet cuts, and eviction takes a file with all its rows.
  `ROWS_HELD` counts readings, not entries (`Held::rows`), a queue — which is unbounded — being what
  bounds how many spans of one file are asked for.
- **Seeing a row unasked and claiming it are one operation under one lock.** `Claim` is what the shelf
  answers — the look, or the read being *ours* to ask — so the 60 Hz poll and the bus thread cannot
  both find a location `Unasked` and both enqueue it. The guard never leaves `Shelf`: a `match`
  scrutinee holds its temporary for the whole match, so a `Catalog` locking the shelf itself would
  deadlock on the arm that asks — why `Shelf::claim_tags` takes and drops the lock inside a call of its
  own rather than handing a guard out.
- **What may be asked at once is bounded, and a row turned away is asked again.** The two request
  channels hold `ROWS_ASKED` names and `PICTURES_ASKED` pictures; an ask that would not fit answers
  `Sent::Backlogged` and the claim is *released* — back to `Unasked`, not settled to `Nothing` — so the
  next poll asks for what is still on screen, not every row a scroll went past. Pictures are bounded
  tighter, costing a read and a copy, and a cover just come into view should not queue behind a
  screenful gone. Bounding the backlog also bounds what `stalest_row` walks past, a pending entry being
  one asked and not yet answered.
- **The catalog is ordered by use, not searched for the stalest.** `Held` keeps a `BTreeMap` from the
  use clock to the row (`order`) and a second holding only rows with a picture (`pictures`), both
  re-keyed whenever a row is looked at, so evicting a row and trimming a picture are a step from the
  front rather than a scan of all `ROWS_HELD` entries — which `evict_until_one_fits` and
  `trim_pictures` each paid per row dropped, so making room for one large cover cost several. Entries
  are keyed by `Arc<MediaLocation>`, the maps sharing the key rather than cloning a `PathBuf` per row
  per lookup, and `Arc<T>: Borrow<T>` keeps a plain `&MediaLocation` the way they are read.

## DSP

- **The chain is remix, lossy restoration, resample, convolver, equaliser, gain, the true-peak guard
  and dither; the equaliser's own rules are `eq.md`'s.** The equaliser sits after the resampler because a
  biquad's shape is warped by its design rate, so designing at the source rate would give one profile a
  different sound per track; before the gain because the volume slider is the listener's last word and
  should attenuate a boost. The convolver comes before it — both are linear, so the order changes
  nothing heard — because that makes it part of the front a reshape carries (below). An equaliser in
  force makes the plan `Converted`; a DoP-packed stream refuses it outright.
- **The resampler's levels are one filter design at four lengths; High is the default.**
  `Quality::params` is the whole difference: `half_taps`, the windowed sinc's half-width in source
  samples; the phases the kernel is tabulated at; the cutoff as a fraction of the lower Nyquist; and
  the Kaiser β. The resampler's own `half_width` is `half_taps` divided by the ratio when downsampling,
  so a level costs more taps going down than up. The settings pane prints those four numbers on hover
  rather than describing a level in adjectives, which is why `SincParams` is re-exported through
  `resonate-engine`. The level below the top is `High`, not the `Transparent` it was called, which
  named what it claimed rather than what it is.
- **`VeryHigh` is built to beat SoX's `rate -v` on paper, and the paper is a test.** SoX states its
  very-high setting as 95 % of Nyquist at −3 dB, 175 dB rejection and no aliasing, the stopband starting
  at Nyquist. `VeryHigh` is 320 half-taps under a Kaiser β of 19.5, cutoff 0.978: flat within
  4 × 10⁻⁹ dB to 95 % of Nyquist and 4 × 10⁻⁶ dB to 96 %, half power at 97.6 %, and 185.7 dB down from
  Nyquist on, measured on the continuous kernel the tables sample.
  `very_high_beats_sox_very_high_on_paper` holds it better than 10⁻⁶ dB flat to 95 %, half power at or
  past 95 % and every point from one to four Nyquists under −180 dB, and
  `each_quality_meets_its_alias_rejection_floor` plays a 15 kHz tone into 22.05 kHz and hears nothing
  above −175 dB, which only an f64 carrier can show. It costs 0.36 % of a core at 44.1 to 48 kHz,
  0.61 % at 96 to 48 and 1.9 % at 192 to 48 natively — about twice High where the taps fit the cache
  and four times where a 192 kHz row of 2 560 weights no longer does — and a table is 2.4 ms to build at
  44.1 to 48 kHz. SoX's precision figure (28 bits for `-v`) is the word its arithmetic keeps; here that
  is the f64 the kernel, the history and the carrier are held in end to end.
- **The resampler's phase is a setting of its own, and a shaped phase is designed once a run.**
  `FilterPhase` is `Linear`, `Intermediate` or `Minimum` (SoX's `-L`, `-I`, `-M`), carried on
  `ResamplerConfig` beside the quality, on `EngineConfig` and `OutputSettings`, as
  `Command::SetFilterPhase`, the `filter-phase` key and `--filter-phase` flag, and a second row in the
  settings pane's Resampler group. A shaped phase is designed in `phase.rs` from the same quality's
  linear kernel: the windowed sinc sampled at `SAMPLED_PER_TAP` (64 points a kernel tap), transformed at
  sixteen times its length (`CEPSTRUM_PADDING`), its log magnitude folded through the real cepstrum into
  the minimum-phase response, and — for `Intermediate` — that phase averaged with the linear one's own
  delay, so the result has the same magnitude and half of each. A minimum-phase response is kept whole
  from its first sample; an intermediate one is longer than the linear kernel, averaging a phase not
  being a polynomial operation, and is kept from 1.25 half-widths before its peak to two after, where it
  stops costing stopband: cut to the linear kernel's length it held only −125 dB. Both hold the linear
  design's −185 dB at `VeryHigh`, and
  `a_shaped_very_high_filter_keeps_the_stopband_and_passband_the_linear_one_promises` holds them under
  −180 dB from one to three Nyquists and flat within 10⁻⁴ dB to 95 %. A design is kept in `DESIGNED` for
  the run, keyed by the `SincParams` and phase, being the same prototype whatever the ratio — the
  kernel's time is the lower rate's, only its sampling moves — and costs 74 ms for `Minimum` and 64 ms
  for `Intermediate` at `VeryHigh` the first time. A prototype is read at any instant through an
  eight-point Lagrange stencil across its samples, exact for a polynomial of that order, which lets the
  tables and the fallback kernel take it where they take the linear kernel's analytic value.
- **The resampler's reach is two numbers, a shaped kernel not being symmetric.** `Reach` holds
  `behind`, the source frames of history an instant weighs, and `ahead`, the frames it waits for, each
  the prototype's trail or lead over the downsampling scale; linear phase has both equal to the old
  `half_width`. The history is primed with `behind` zeros, compacted to `behind` before the instant,
  flushed with `ahead` zeros, and the latency is `ahead` frames, so a minimum-phase `VeryHigh` waits
  seven frames at 44.1 to 48 kHz where linear waits 348, and a track is exactly as long whatever the
  phase (`a_track_keeps_its_length_whatever_the_phase`). An impulse through 44.1 to 48 kHz peaks where
  it was put under every phase, and rings before that peak −17 dB of its energy under linear, −45 dB at
  High and −50 dB at `VeryHigh` under intermediate, and −78 and −61 dB under minimum, which
  `a_minimum_phase_filter_rings_only_after_what_it_answers` holds to at least 40 and 10 dB under linear.
  The cost is taps: minimum phase costs what linear does, 0.37 % of a core at 44.1 to 48 kHz, and
  intermediate's longer kernel 0.59 % there and 1.5 % at 96 to 48.
- **The kernel is stored polyphase, the history planar, the read position exactly rational.** The
  position is a `whole` sample index and a `phase` numerator over the phase count, stepped by integer
  arithmetic — the step split once into whole and remainder, so a frame adds and wraps rather than
  dividing. The count is `SampleRate::ratio_to(output).numer`, every phase the conversion can visit:
  **one** for every integer ratio, 160 for 44.1 onto 48 kHz; an f64 accumulator stepped by `1 / ratio`
  smeared those 160 into 1 441 over twenty thousand frames. `Tabulated` is what exactness buys: every
  phase's weights are built once in `Resampler::new`, the analytic windowed sinc evaluated in f64 at
  every distinct distance once (the taps behind one phase's instant being those ahead of its mirror's),
  each phase then divided by its own sum, so every phase passes DC at unity and `scale` folds away.
  Rounded from an f32 table and unnormalised they put a phase-cycle ripple on DC of 3.6 × 10⁻⁴ at Fast,
  7 × 10⁻⁶ at Balanced and under an f32 step at High;
  `every_frame_is_the_exact_windowed_sinc_normalised_at_its_instant` holds every frame, block boundaries
  and flush included, to 10⁻¹² of a direct f64 evaluation, the frame leaving in f64. A table is 340 KB
  and 0.84 ms at 44.1 to 48 kHz, 4 KB and 0.01 ms with one phase, paid once rather than every `rebind`:
  the last table is kept behind an `Arc` keyed by the rates, `SincParams` and phase, so a track change,
  a paused seek or a settings change at the same rates takes it in 0.3 µs
  (`a_rebind_at_the_same_rates_takes_the_table_already_built`). Only a table of at most
  `KEPT_TABLE_BYTES_AT_MOST` (4 MB) is kept, so a rare wide pair does not stay resident once its chain
  has gone. `TABULATED_WEIGHT_BYTES_AT_MOST` is 16 MB, which every pair of the ten common rates within
  the 32:1 range fits at every level — the widest, 22.05 onto 384 kHz at `VeryHigh`, visits 2 560 phases
  and holds 13.4 MB (`every_rate_pair_tabulates_its_phases_at_every_quality`). Only a rate no device
  offers, 47.993 kHz beside 44.1, still reaches `Kernel`, which holds the windowed sinc in f64 at
  `phases` points a kernel tap, reads each weight off a four-point Lagrange stencil (`LAGRANGE_NODES`)
  across them, then divides the instant's weights by their sum as a tabulated row is. It is within 10⁻¹³
  of the analytic kernel everywhere but the last stencil before the window's edge, where the Kaiser
  window stops at `1 / I₀(β)` rather than nothing and the stencil straddles that step: 2 × 10⁻⁸ at High,
  under High's own stopband, and 10⁻¹¹ at `VeryHigh`
  (`an_interpolated_kernel_stays_within_the_step_its_window_ends_on`). The history is one f64 plane per
  channel, converted once on arrival, and a frame's channels share one pass over the weights — in pairs,
  each weight loaded once for both, an odd channel alone. Each channel keeps `ACCUMULATORS` (eight)
  independent lanes on every target: a pair already runs two add chains, so sixteen and thirty-two
  measured slower on AVX-512 (the longer lane sum costing more than the latency it hides), and sixteen
  spill SSE2's sixteen registers. The loop waited on the cache — every 64-byte load straddled two lines —
  so planes, lanes and table are aligned to `VECTOR_BYTES`, a frame's blocks start at the aligned sample
  at or below its first tap, and every row carries `LANES_PER_VECTOR` leading zeros so its weights slide
  under the aligned history. Rows end in zeros to a whole block and `compact` zeroes what it vacates, so
  what is read past `filled` is exact zeros and no scalar tail is left. The lanes pass through `lanes`
  before being summed because, summed in registers, LLVM's SLP vectoriser paired the two channels
  two-wide off their adjacent output stores instead of packing each channel's eight into one vector.
  High costs 0.18 % of a core at 44.1 to 48 kHz and 0.27 % at 96 to 48 natively (against 0.20 % and
  0.39 % before), and 0.25 % and 0.41 % on an `x86-64` baseline build (against 0.31 % and 0.53 %). The
  alias tests skip `latency_frames` at both ends as well as their margin: a tone switched on against a
  silent history rings that long, and at 24:1 the ringing was the whole of a −105 dB once recorded
  there. The 24:1 test plays 21 kHz, since 20 kHz folds onto the 4 kHz output Nyquist, where every
  output instant is a zero crossing and nothing is measured.
- **The dither stage has a flat path and a shaped one, works in f64, and the plan's curve picks.**
  `Dither::prepare` resolves `NoiseShaping::at` the output rate once and keeps what survives as the
  stage's `applied`, and `process` matches on it: `NoiseShaping::None` walks the samples and
  requantises with no error history, neither coefficient nor history `Vec` allocated — wherever no curve
  survives the output rate, every sample saves a feedback sum and a history write. A sample is worked in
  steps of the target grid in f64 — widened, dithered, rounded and, shaped, its error fed back — and
  handed on in f64, where every grid value up to thirty-two bits is exact. In f32 the noise added to a
  24-bit sample near full scale was itself rounded to two levels a step, biasing a half-step input by a
  quarter step (`a_24_bit_word_near_full_scale_is_dithered_without_bias`). A uniform draw takes 53 bits
  and triangular is the sum of two. The rounding adds and subtracts 1.5 × 2⁵², so IEEE rounds the sum
  half to even — the trick `resonate-core`'s conversions use, `round` being a libm call per sample on the
  `x86-64` baseline. The output is clamped to `[-1, 1 - step]`, but the error fed back is taken before
  the clamp, so a clipped sample cannot run the loop away — and an error that is not finite, or lies
  past what the dither's own peak and half a step can make (`DitherKind::largest_error_steps`), is fed
  back as nothing, since one NaN, infinity or wild float sample, where the 1.5 × 2⁵² rounding is no
  longer exact, would ride the feedback for the rest of the track. The check is one comparison on the
  magnitude, false for NaN. Every real sample's error is inside it
  (`an_error_is_fed_back_whole_wherever_the_input_is_a_real_sample`, against an unguarded loop for every
  dither kind), and `one_wild_sample_is_forgotten_rather_than_fed_back_for_the_rest_of_the_stream` shows
  NaN, both infinities and 10³⁰ each forgotten alike, every later sample finite, on the grid and in
  range. The wild sample still goes through the clamp, so an infinity lands on full scale and a NaN
  passed on is silence once the integer conversion reads it. The shaped path is one function generic
  over the tap count, so Lipshitz's five taps and Threshold's twelve are each unrolled, and it copies the
  coefficients out of their `Vec` before the loop, letting LLVM hold them in registers rather than
  reload them past every history write it cannot prove does not alias them. Each channel's errors are
  held twice over in a ring twice the tap count long, the newest written at one slot and that slot plus
  the tap count, so the most recent errors are always one contiguous window read an element at a time:
  shifting the whole history instead read it back next sample through one wide load spanning the narrow
  stores just made, which no store can forward to. The ring strides by the curve's own tap count, so one
  channel's error cannot reach the next. The match is on the enum rather than an empty coefficient
  slice, so a curve added to `NoiseShaping` must say which path it takes. At 48 kHz stereo the flat path
  costs 0.022 % of a core, Lipshitz 0.033 % (0.045 % while its history shifted in f32) and Threshold
  0.039 % natively, and 0.024 %, 0.038 % and 0.052 % on an `x86-64` baseline build. The guard is most
  of what separates Lipshitz from the 0.028 % it cost without one: under AVX-512 the comparison becomes a
  mask on the loop-carried path. Moving the forgetting into a cold call takes it off that path natively
  but costs Threshold, the default, more on the baseline than it saves, so the guard stays a plain
  select.
- **A 32-bit target is dithered like any other.** Its grid is 2⁻³¹, exact in the f64 carrier and far
  inside what the 1.5 × 2⁵² rounding reaches, so `Dither::new` takes every depth (`Bits32` included) and
  cannot fail, and `plan_for` dithers an S32 stream wherever something narrows or converts: a resample, a
  gain, a float source. An integer source an S32 word holds whole is still `Repacked`. While the carrier
  was f32 the grid was finer than the carrier, so the plan refused it and a 32-bit device got samples no
  dither covered; `a_32_bit_word_is_dithered_on_its_own_grid_without_bias` is the claim, down to every
  written word landing back on the grid value it came from.
- **Lipshitz runs only at the rates it was designed for; Threshold is designed at the rate it runs at.**
  The Lipshitz coefficients are an E-weighted fit to the ear at 44.1 kHz; at 96 kHz the same taps put the
  notch in the wrong place, so `NoiseShaping::at` resolves it to flat at every rate but 44.1 and 48 kHz.
  `Threshold` is an error-feedback filter `Dither::prepare` designs for the output rate, shaping at every
  rate. The weighting is Terhardt's (1979) threshold in quiet, held at its 1 kHz value below 1 kHz so a
  low output rate does not push noise into the bass, and clamped to `THRESHOLD_RANGE_DB` (42 dB) above
  its quietest point, taken as a power over 2048 midpoints of the band. Its autocorrelation to lag 12
  comes from the Chebyshev recurrence on each point's cosine, four points at a time so the recurrence
  runs across points rather than serially down one, and Levinson–Durbin turns it into the order-12
  prediction-error filter `A(z)`. The stage feeds back `c = −a`, so its noise transfer function is `A`
  itself — the Lipshitz constants read the same way, their `A` being `1, −2.033, 2.165, …` — and `A` is
  minimum phase by construction. Weighted by the unclamped threshold over 20 Hz–20 kHz, Threshold leaves
  noise 16.6 dB below flat dither at 44.1 kHz and 18.2 dB at 48 kHz, where Lipshitz leaves 9.4 and 10.5;
  25.9 and 26.5 dB at 88.2 and 96 kHz, 30.7 and 30.8 at 176.4 and 192 kHz and 31.4 at 384 kHz. At
  22.05 kHz and below there is little band above the ear's reach to move noise into and it gains 1.2 to
  2.4 dB, at 32 kHz 9.7. Its peak noise lift stays under 30 dB at every rate from 8 to 384 kHz — 27 dB at
  32 kHz, 19.5 at 44.1, 10.2 at 96, 6.9 at 192 — where Lipshitz peaks at 19.4. The tests hold the design
  to the numpy prototype's coefficients at 44.1 and 96 kHz within 10⁻⁹, recover its reflection
  coefficients by the step-down recursion to show each under one in magnitude at every rate in that span,
  and quantise a quiet sine at 96 kHz to watch the error fall 26 dB at 1 kHz and 33 dB at 4 kHz and rise
  9 dB at 40 kHz against flat. A design costs ~38 µs at 44.1 kHz and 55 µs at 192 kHz natively, 42 and
  64 µs on the baseline, paid in `prepare` on every rebind and every reshape building a dither stage, the
  transcendentals in the threshold and the per-point cosine being most of it. `plan_for` stores what
  survives in `OutputPlan::shaping`, the stage applies the plan's curve rather than the configured one,
  and `resonate explain` prints it beside the target depth, so a Lipshitz fallback to flat is visible,
  not silent. `EngineConfig` defaults to `Threshold`, so a 16-bit device at any rate gets shaped dither
  out of the box, as does a 24-bit device behind a volume below full. `--dither` and `--noise-shaping`
  are the command line's way to say otherwise, `dither` and `noise-shaping` the config file's, and the
  settings pane's Processing category the window's.
- **Digital silence held a moment is handed on as digital silence.** Dither decorrelates the error of
  a signal the grid cannot hold, and a run of exact zeros has none: dithered, a gap between tracks or a
  track's leading silence reached a 16-bit device as shaped hiss a DAC's silence detection could never
  see as silence. Once every channel has read exact zero for `SILENT_FOR_BEFORE_MUTING_SECONDS`
  (50 ms), the stage writes zeros and clears the error it feeds back, and the first non-zero frame is
  dithered again from that clean history — so a fade, however far under the last step, is never taken
  for silence, and the 50 ms before the mute let the shaped error of what came before ring out rather
  than stop on a step. `digital_silence_held_for_a_moment_comes_out_as_digital_silence`,
  `a_fade_below_the_last_step_is_still_dithered` and
  `sound_after_digital_silence_is_dithered_from_a_clean_history` hold the three claims; checking each
  frame costs the stage about a tenth of what it cost.
- **`OutputMode` names what the plan does to the signal, and a shorter word is not a resample.** Four
  readings, decided in `plan_for` and drawn as the chip on the playback bar and inspector: `BitPerfect`
  where the stream is the source's own triple; `Repacked` where the target holds every source value
  losslessly and only the container is wider; `Dithered` where rate and channel map are the source's
  and only the word is shorter, dither covering the difference; `Converted` for all else — a changed
  rate or layout, a gain stage, an equaliser, or a narrowing with dither off, which nothing covers. A
  24-bit 48 kHz file on a 16-bit 48 kHz device is `Dithered`, and reading it as a resample was wrong
  twice: nothing resamples, and `no_convert` is `mode != Converted`, so the graph was asked to convert a
  stream already matching the device. The order is load-bearing — `dithers` is resolved *before* the
  mode, since a narrowing the listener switched dither off for is a truncation and must not answer to
  a name saying it was covered.
- **The channel layout is negotiated like rate and format, and one matrix is the whole downmix.**
  `plan_output` asks `SinkInfo::best_spec_for` for rate, format *and* layout together, so the plan can
  never name a triple the sink did not advertise as one — `crates/resonate-pipewire/src/sink.rs`
  asserts it over the cross product, and `transport.rs` plays a 5.1 file into a stereo sink and a 5.1
  sink and watches what the graph gets. The fit is lexicographic over rate, then channels, then format:
  keeping every channel beats keeping depth, a fold losing a whole signal where a repack loses bits
  nothing recorded. `resonate-dsp`'s `Remix` is the stage asked for, running first, so resampler and
  dither cost the sink's channels, not the file's. Its matrix is built from `ChannelPosition`: a
  channel the target also has is copied at unity (`UNITY`), one it lacks folds into its nearest
  neighbours at −3 dB (`MINUS_3_DB`) — a centre into both fronts, a rear into the side then the front —
  the LFE is dropped, not folded, and a target channel nothing feeds stays silent, so an upmix invents
  nothing. Each row is scaled so its gains sum to at most one, so no downmix can clip a full-scale
  source and 5.1 into stereo lands about 7.7 dB down — the clip-prevention choice below: attenuate
  rather than clip.
- **The chain is carried in f64 from the decoded word to the sink's.** `Processor::process` and
  `flush` take `f64` slices; the engine widens exactly what the decoder handed it into
  `Output::widened` through `SampleData::widen_into` before the chain, and narrows the carrier once,
  into the sink's word, in `Output::stage`. An f32 carrier held 24 bits of mantissa: a 32-bit integer
  source lost its low byte on the way in, and a 32-bit sink got at most ~twenty-five bits however well
  every stage worked inside. Every stage already worked in f64 — the resampler's history and weights,
  the equaliser's sections, the dither's grid — so the carrier was the one place precision was thrown
  away, and moving it cost nothing measurable: 5.1 at 44.1 kHz to stereo at 48 kHz through the
  resampler, ten bands, gain and dither is 0.297 % of a core in f64 against 0.296 % in f32, and
  narrowing a block to S16, S24 or S32 costs 0.001 %. `ChainBuilder::input` still names the carrier
  `SampleFormat::F32`, the spec being a negotiation vocabulary with no wider float, and the stages read
  only its rate and channels.
- **Lossy restoration is a spectral stage for MP3, AAC and Vorbis alone, off unless asked.**
  `Restoration` is `Off`, `Repair` or `Extend` — the `restore-lossy` key and the settings pane's
  *Artificially enhance lossy files* group, marked experimental — and `Restore` is pushed after the
  remix and before the resampler, working at the source rate where the encoder cut. `Decoded` carries
  the source's `Tuning` — the engine maps `Codec` onto one in `tuning_of`, naming the lossy codecs
  rather than trusting `is_lossless`, which calls `Unknown` lossy too — and the lowpass wall the study
  found, off `TrackHints`; a lossless source never gets the stage whatever the key says, and one that
  does makes the plan `Converted`. The stage is a weighted overlap-add over sqrt-Hann frames of 1 024 at
  44.1 or 48 kHz, scaled with the rate, a quarter frame apart, planned and allocated in `prepare` and run
  through `process_with_scratch`; it holds back its first `frame − hop` frames rather than emitting a
  silent pre-roll and hands them on in its flush, so a track keeps its length, and with nothing to
  restore gives back what it took within 10⁻¹²
  (`with_nothing_to_restore_the_stage_hands_back_what_it_took_exactly_and_whole`). What it does:
  - **The wall** is the study's where there is one in the codec's plausible range; otherwise it is found
    as the music plays, from a three-second average spectrum read in 100 Hz bands once a second and a
    half has been heard: the highest band whose kilohertz below stands `WALL_DB` (30 dB) over everything
    from 200 Hz above it to the top, and within 50 dB of the loudest band from 2 to 10 kHz, settled down
    to the last band still within 10 dB of that kilohertz; once found it is held for the track. A 320 kbps
    MP3 transcoded from a 16 kHz source reads at 15.8 kHz, a 192 kbps LAME encode at 18.6 kHz.
  - **Repair** lifts the droop an encoder's own lowpass leaves under its wall — a quadratic shelf of
    `Tuning::droop`: 2 dB over 1.5 kHz for MP3, 1 dB over 1 kHz for AAC, 1.5 dB over 1.2 kHz for Vorbis
    — and fills a hole: a run of at least four bins from 4 kHz to the wall falling `deeper_than_db`
    under its own median of the last nine frames, in a frame that as a whole holds within 6 dB of that
    median, filled with noise at `filled_below_db` under it. The median keeps a transient's splatter from
    reading the frames after it as holes, and the whole-frame test keeps a track's end from being filled
    (`a_transient_is_not_taken_for_a_hole_in_what_follows_it`). A dropout is filled for its first few
    frames, and a band really gone is let go.
  - **Extend** also rebuilds the band from the wall to 97 % of Nyquist from the same width below it,
    bin for bin, so a transient keeps its time: each patched bin is scaled to a line starting 6 dB under
    the average just below the wall and falling 12 dB an octave faster than the slope a fit of the
    octave under the wall gives, never louder than 3 dB under its own source, faded in over the first
    500 Hz. On the transcode above that puts 16–21 kHz at −18 to −26 dB where the file held −62 to −69,
    20–28 dB under the band at 10–13 kHz.
  The codec's curves live in `Tuning` and are applied in the spectrum, not handed to the equaliser,
  following each track's own wall without moving a profile a listener bound to a device. The stage costs
  0.57 % of a core at 44.1 kHz stereo and adds `frame − hop` frames of latency, which
  `Output::hand_on_what_the_chain_holds` flushes before a swap as it does the guard's.
- **A `Chain` carries a channel width per stage, not one for the chain.** `Remix` changes the frame
  width mid-chain, so the builder records `output_spec(spec).channel_count()` per stage and sizes the
  two scratch buffers by the widest *sample* count, not frame count; everything after a fold slices the
  scratch by the width it has.
- **A chain drains in one flush, and a flush with less room is refused.** The builder sums what every
  stage can still hand on once its input ends into `Chain::max_flush_frames`, and `Chain::flush`
  answers `Error::OutputTooSmall` for a shorter destination rather than filling what fits. One flush is
  the whole tail on purpose: a resampler pads half its filter and emits up to the last real frame,
  keeping a track exactly its length, and a second flush would only hand on ringing past the end.
  `Engine::flush` sizes the carrier to `max_output_frames`, which covers the bound, so a stage later
  advertising less than it drains fails loudly — an error record and a track ended without its tail —
  rather than cutting every track short in silence.
- **Clip prevention attenuates where it knows the peak; only an over it could not see is ridden
  down.** A ReplayGain boost is capped where the track's declared peak would reach full scale, so the
  waveform keeps its shape. `AppliedGain` in `resonate-core` pairs the gain with the peak the mode
  selected and owns that arithmetic, so the DSP stage, `resonate info` and the inspector cannot disagree
  about what was applied. `AppliedGain::heeding` folds a measured true peak in beside the declared one
  and keeps the larger, so a track whose study says it passes full scale between its samples — or on
  them, as a lossy decode often does — is turned down by exactly that even with ReplayGain off, and the
  plan grows a gain stage for it; `true-peak` off leaves the declared peak alone. The per-sample clamp
  survives only where a *boost* — an amplitude over one — has no peak at all, the one case with nothing
  better. At unity or below a clamp can only clip overs already in the signal, which is the guard's to
  answer, so `GainConfig::limits_every_sample` reads the amplitude as well as the peak, and `GainStage`
  asks it again wherever its target moves (`set_gain`, a ramp handed a start). A boost capped at exactly
  unity — a peak-normalised track declaring 1.0 — changes no sample, so `AppliedGain::adjusts` answers
  false, the plan asks no gain stage of its own and, with nothing else to do, the track plays bit-perfect
  rather than dithered at unity and labelled `Converted`.
- **What the catalog studied reaches the player through `Hinting`, beside `StandIn` and not gated on
  the vault.** `resonate-core::TrackHints` is the vocabulary — measured true peak and the lowpass wall a
  study found — since the codec's `Sources`, the library and the engine all need it and none may see the
  others. `Sources::hinted_by` registers a `Hinting`, `Library::hinting` answers it from
  `track_studies` by the row's own path and span, and `playing_from` registers it on the player's
  sources wherever there is a catalog, vault or not. The track's open (`Unwrapped::open`) asks once,
  keeps the answer on the track and folds it into the gain in `levelled`, which `re_level` and
  `Command::SetTruePeak` run again; a row with no study answers nothing and plays as it did.
  `a_track_the_catalog_measured_past_full_scale_is_turned_down_under_it` plays a file the hints say
  peaks at 2.0 and hears it at half level, and at full level with the key off.
- **A track nobody studied is measured where its peak would matter.** Where `true-peak` is on, the gain
  in force has no peak — neither tag nor study — and either asks for a boost or the source is float (the
  one word able to carry an over into a bit-perfect stream), `measure::Measuring` decodes the row whole
  on a `resonate-peak` thread through a `TruePeakMeter`, and each run-loop pass asks whether it landed:
  `Track::heed_what_was_measured` puts the peak into the track's hints, `levelled` runs again and
  `retune` reshapes the chain in place, so the boost is capped at the peak rather than ridden by the
  per-sample clamp for the rest of the track, and a float file passing full scale grows a gain stage
  rather than reaching the device as it is. Dropping the track drops the `Measuring`, telling the thread
  to stop at its next block, so a run of skips leaves no decode behind; `re_level` and `SetTruePeak`
  start one where a changed setting now wants it. The measurement is the engine's alone and not kept —
  the lookup's study is what the catalog keeps.
  `a_float_track_over_full_scale_nobody_studied_is_measured_and_turned_down_under_it` opens a
  2.0-peaking float file bit-perfect and hears it no louder than full scale once measured.
- **The true-peak guard is a lookahead gain, not a clipper, and touches nothing under the ceiling.**
  `resonate-dsp`'s `TruePeak` is pushed after the gain stage and before the dither wherever `true-peak`
  is on and the plan converts — any stage, or a float source narrowed to an integer word — never on a
  bit-perfect or repacked stream. It reads each frame at eight times the rate, and wherever a phase
  passes −0.1 dBTP asks for the gain bringing it under; the smallest asked across 1.5 ms of lookahead is
  averaged over the same span, so the gain has fallen to what the peak needs by the peak's frame, and it
  recovers with an 80 ms release snapping back to exactly one within 2⁻³² of it. A stream never passing
  the ceiling is multiplied by exactly one and comes out bit for bit, only later
  (`a_stream_that_never_nears_full_scale_passes_through_exactly_and_whole`). The audio waits
  `lookahead + 48` (`REACH`) frames, held back rather than padded with silence and handed on in its
  flush, so a track keeps its length. Because it holds frames, `Engine::swap_chain` first flushes the
  running chain's held tail into the ring (`Output::hand_on_what_the_chain_holds`) and defers the swap
  to the next pump where the ring has no room, so a reshape between a guarded chain and another never
  drops what the guard held.
- **The guard's interpolator is 96 taps, because 48 could not see the top tenth of the
  band.** Every fractional phase must pass a tone near Nyquist at its own level, or an over carried there
  is read low and passes the ceiling. 24 half-taps at cutoff 0.985 under Kaiser β 8 — which had replaced
  sixteen at 0.95 for the same reason — were flat to 85 % of Nyquist but read a tone at 90 % 0.06 dB low,
  at 92 % 0.33 dB and at 95 % 2 dB. 48 half-taps at 0.998 under β 10 are flat within 0.001 dB to 92 %
  and 0.09 dB at 95 %, inside the 0.1 dB between ceiling and full scale, and read nothing over —
  `every_phase_passes_a_tone_at_nineteen_twentieths_of_nyquist_within_a_tenth_of_a_decibel` weighs each
  phase's own response there. What is left is the top 5 %, above 22.8 kHz at 48 kHz, which 64 half-taps
  would take to 97 % for another third of the cost. The interpolating path costs 0.42 % of a core at
  48 kHz stereo where it cost 0.23, and 1.7 % at 192 kHz where it cost 1.0; the skip below is untouched.
  The study's `TruePeakMeter` reads through the same interpolator, so the two agree.
- **The guard skips the interpolation wherever it can prove nothing is over, and is constant time when
  something is.** No phase can answer more than the input's loudest sample times the largest sum of
  absolute weights any phase holds — 3.0 — so while no sample in the window has been louder than the
  ceiling over that, −9.7 dBFS, the frame is written into the history and not interpolated
  (`quiet_below`, `loudest_that_could_pass`);
  `the_detector_skips_only_frames_no_phase_could_carry_over_the_ceiling` holds the skip to frames the full
  reading also passes. A volume under 100 % or a ReplayGain turning a loud master down is exactly that
  case: 0.05 % of a core at 48 kHz stereo where interpolation costs 0.42 %. The interpolation keeps the
  eight phases' weights transposed, fused multiply-adds them, and reads the channels a pair at a time so
  one weight-row load serves both. Limiting is kept constant too: the smallest gain across the lookahead
  is a monotonic queue, not a scan, and its average a running sum re-summed once a lookahead so error
  cannot build, so a hot 192 kHz master costs 1.8 % of a core where a scan cost 4.7 % under the 48-tap
  interpolator.
- **The pre-amp and an untagged track's gain are part of what ReplayGain asks for, so clip prevention
  weighs them too.** `Levelling` is a `Trim` for each — quantised millibels, so `OutputSettings` keeps
  its `Eq` — and `resolve_replay_gain` folds them into its `AppliedGain`: a tagged gain is raised by the
  pre-amp and keeps its tagged peak, so a boost the pair would push past full scale is capped at the peak
  like any other; a track declaring no gain takes `untagged` with no peak, the one case the per-sample
  limiter is for; with ReplayGain off neither moves anything. `GainConfig` lost the `pre_amp` nothing
  ever set. **A track declaring no gain that the catalog studied is levelled by what was measured.**
  `TrackHints::measured` is a `MeasuredGain`: `Hinted` answers the track's gain as ReplayGain 2.0's
  −18 LUFS reference less the study's integrated loudness, and the album's from the energy mean of every
  track's loudness weighted by length — only where every track of the album, alternatives aside, has
  been studied, an album gain from half a record being a track gain in disguise.
  `engine::tagged_or_measured` hands `resolve_replay_gain` those only where the tags carry neither gain,
  so a tagged file is never second-guessed, the peak being the study's true peak through `heeding`. It is
  what gives a delivered row a gain, the vault having stripped its tags; `untagged` is now what a track
  neither tagged nor studied plays at. `resonate explain` reads no hints and resolves the tags alone.
  `replay-gain-pre-amp` and `replay-gain-untagged` are the keys, in decibels, offered by the settings
  pane's ReplayGain group as two chip rows under the mode.
- **The plan is the one judge of a gain stage, a converting plan always carries one, and the engine
  picks its fill branch from the plan.** `gain_config` answers `None` at full volume under unity gain,
  and `plan_for` asks for a stage anyway wherever the plan resamples, remixes or equalises — at unity,
  such a chain not being bit-perfect whatever its gain, and a stage already there making every volume and
  ReplayGain change a retune rather than a change of shape. Only a plan with no other stage leaves it
  out, keeping a bit-perfect path bit-perfect, and `dop_survives` still reads `gain_config`, so DoP is
  refused only where a gain would change samples. `GainStage::is_transparent` answers false, so the
  builder keeps every gain stage the plan pushes (as `eq_config` and the equaliser do). `Engine::fill`
  and `Engine::flush` pick the byte copy or the conversion from `OutputPlan::is_transparent`, the reading
  `delivery()` asks the decoder's format from, so the two cannot disagree: a chain empty in a converting
  plan costs a format conversion rather than raw f32 bytes in an integer ring.
- **Every float reaching an integer word is rounded to nearest, ties to even, and saturated to the
  format's range, and one set of conversions in core is the whole of how.** `SampleData::write_f64` is
  what `Output::stage` narrows the carrier through on its way to the ring, and `SampleData::write_f32`
  its narrow twin that `convert_into` and `retype` use and the DSD decimator hands the decoder its
  samples through, both over the same rounding, so none can disagree about a step. A narrowing nothing
  dithers — dither off, or a float source reaching an integer word at its own rate — is
  `OutputPlan::rounds`, making the plan non-transparent with an empty chain, so the samples reach
  `write_f64` rather than symphonia's arithmetic shift (which floors) or truncating cast. Truncation
  left a dead band a step wide around zero and pulled each sample half a step towards it, and on paths
  nothing dithers — S16, S24 or S32 with dither off — nothing covered it. The scale is
  `SampleFormat::full_scale` both ways, so every S16 and S24 value survives a trip through f32 exactly.
  The rounding adds a constant big enough that IEEE rounds the sum to a whole number, half to even, and
  reads it back from the bits, rather than calling `round_ties_even`: the `x86-64` baseline has no
  `roundps` and made that a `rintf` call per sample, where the addition vectorises on SSE2, which the
  saturating `as` it replaced never let the loop do.

## The sink

- **A sink's formats are read again whenever the node says they moved.** A port switched or an EDID
  read again can change what a node advertises while the node stays, so its `info` event is watched
  for `PARAMS`: the `EnumFormat` entry's `SERIAL` flag flips on every change, `SinkRecord::formats_moved`
  compares it with the last seen (the first sighting only records it, the bind having enumerated
  already), drops every index held and asks for `EnumFormat` again, and `SinkChange::Reformatted`
  has the engine survey the graph afresh. The survey's round trip is issued after the enumeration, so
  it reads the new list rather than an empty one. The seq a `param` event carries is not the one
  `Node::enum_params` was handed but the proxy's asynchronous reply number, which pipewire-rs 0.10
  does not return, so replies cannot be told apart by enumeration; results of an older enumeration
  still in flight when a second change lands would be kept beside the new ones until the next.
- **A global leaving the registry takes its proxy with it, whatever it was told first.** A device's
  proxy is dropped on `global_remove` whether or not a `Route` param ever arrived for it, so a card
  unplugged before its routes were enumerated — a USB DAC pulled in its first moment, a Bluetooth
  profile that never settled — does not keep a proxy and its listener until the client reconnects.
- **The client outlives its daemon.** Everything a connection holds — the core, the registry, their
  listeners and the proxies bound through them — is one `Graph`, and `Reaching` makes one: the loop
  keeps its main loop and context for the process's life and connects a core through them as often as
  it must. The core's `error` event with a broken pipe on the core itself is the daemon gone; it sends
  `Request::Lost` through the loop's own channel, a connection being impossible to tear down inside its
  own callback. `Lost` drops the streams and the graph, answers every pending `Sync` by dropping it,
  empties `Discovered` and announces each known sink removed, and a thread sends `Request::Reconnect` a
  second later, again until a core connects. **The first connect is one more of these tries**: a
  daemon not there when `PipeWire::start` runs leaves the graph empty, warns and asks again a second
  later, so a player started before its session's daemon, or in a session whose daemon is late,
  finds the graph once it comes rather than the binary exiting
  (`a_client_started_before_its_daemon_finds_the_graph_once_it_comes`, beside the restart in the
  same `reconnect.rs`, each rerunning the test binary under a daemon of its own). While there is
  none, a `Sync`, an open and a capture
  answer `Error::Disconnected` at once rather than timing out as `LoopStopped`, `PipeWire::unanswered`
  telling the two apart by the loop's `connected` flag. What comes back is announced as the first time,
  the registry handing the globals over again. `crates/resonate-pipewire/tests/reconnect.rs` is the
  claim and needs no session daemon: it reruns its binary under `PIPEWIRE_RUNTIME_DIR`, starts a
  `pipewire` of its own with a null sink and nothing else, kills it under an open stream, asserts
  `Disconnected`, starts it again and opens a stream on the sink it finds. **A recording carries on
  across the restart, the listener reopening what the client dropped.** `Lost` takes a capture with
  it, and the capture's events channel disconnects when it goes (its only sender being in the stream's
  listener), so `resonate-listen`'s `Capture` reads that as lost and asks the same client for the same
  capture every `LOOKS_EVERY`, recording into the same `Recording` from where it had filled, and adds
  the time the graph was away to its deadline once one opens. `crates/resonate-listen/tests/reconnect.rs`
  is the claim: the same hosted daemon, killed under a recording and restarted, holds the capture's node
  a second time — read through `pw-cli`, since with no session manager nothing links a capture and no
  sound reaches it to weigh.
- **A metadata key cleared is read as unset.** The `settings` and `default` metadata are read by
  `Discovered::heard`, keyed by which object spoke (`HeldIn`): a key with no value clears what it
  held — `clock.force-rate` no longer forced, `default.audio.sink` naming nothing — and a key of
  nothing clears every key that object carries and no other's, as `pw-metadata -d` does. The default
  sink announces `SinkChange::DefaultChanged` only where the name it reads moved. Matching only a
  key carrying a value left `forced_rate`, `clock_rate` and the default sink stale and announced
  nothing (`a_rate_key_cleared_or_zeroed_leaves_the_rate_unforced`,
  `a_default_sink_cleared_is_forgotten_and_announced_once`,
  `every_key_cleared_at_once_clears_only_what_that_metadata_holds`).
- **The chosen sink is named, not numbered.** `EngineConfig::sink` and `Command::SetSink` carry a
  `NodeName`, which `select_sink` matches on every stream open, a PipeWire id being assigned per object
  and a device unplugged and put back carrying a new one. `OutputStatus::sink` stays a `SinkId`,
  reporting what was opened. A name nothing answers to warns and falls back to the default until the
  device turns up — so a device missing at startup binds when it appears, and a typo plays on the
  default with a warning rather than refusing to start.
- **Whether a sink is hardware is a question about the `Device` above it, not the node.** A `Node`
  global carries `device.id` and not `device.api` (the api is on the `Device` the id names), so reading
  it off the node alone made a real USB DAC read as something the graph made up, and the field had no
  honest reader for that reason. `Discovered::driven` is the set of device ids whose global names a
  driver, `SinkRecord::device` the id the node points at, and `Discovered::is_hardware` answers for the
  pair at snapshot time, not insert, so the two globals may arrive in either order. `resonate sinks`
  draws it as *DRIVEN BY*, a device or the graph — what tells a real DAC from a null sink, a loopback or
  an echo canceller.
- **Which physical port a sink comes out of is the `Device`'s to answer, and the seat is the join.** A
  card's `Route` params name its ports; a sink node's `card.profile.device` names the seat on that card
  it plays through, and `DevicePorts::serving` is the pair — so a sink is drawn as *Headphones* or
  *HDMI / DisplayPort 2*, not only its node's description. The seat is read off the node's `info`
  event, not its registry global (which carries `device.id` and nothing of the profile); the same event
  follows a card switched to another profile. Only an event whose change mask says `PROPS` is read: a
  node's volume moving raises an `info` with no props, and reading that as *no seat* lost the sink its
  port and route the first time anything turned it. Both route params are read and kept apart: `Route`
  is the port the card is *switched to* and outranks the rest (`Held::Current`), `EnumRoute` every port
  it offers (`Held::Offered`) — the enumeration being the only place an unplugged port appears, a card
  publishing no current route for one. That is the case worth drawing, so `Plugged` is the reading —
  `Yes`, `No`, or `Unsaid` where the driver does no jack detection (a USB adapter usually does not) —
  and `resonate sinks` says *nothing plugged in* beside the port. Each is kept under the param index as
  a node's formats are, so an enumeration overwrites the last rather than piling onto it.
- **The card says which output it is switched to, and the port whose volume it is.** The `Device`
  subscribes to `Profile` beside its two route params, and `parse_profile` reads the current one into
  `Discovered::profiles` — keyed by device id, looked up through `SinkRecord::device` as `port_of`
  looks up the port — so `SinkInfo::profile` is drawn as *Analog Stereo Output* rather than being
  something only `pw-dump` answers. **What the card could switch to is read beside it.** The `Device`
  subscribes to `EnumProfile` too, and `parse_profile` reads each into a `CardProfile` — its
  `ProfileIndex`, name, description, priority, whether what it plays through is plugged in, and how many
  sinks its `classes` open — kept per device under `Discovered::offered_profiles`. `SinkInfo::profiles`
  is the card's sink-opening profiles, most preferred first, so *Off* and an input-only profile are never
  offered from a device list they would take the device out of. `Command::SwitchProfile` names the sink
  and index, `Backend::set_card_profile` is the seam, and `PipeWire` sets `Profile` on the `Device` with
  `save`, as a desktop's sound settings do; the card then tears its nodes down and makes new ones, which
  the engine follows like any device leaving and arriving. A current profile that moved announces
  `SinkChange::Switched` for the card's sinks. The settings pane draws the profiles of the device in use,
  or the one chosen, as chips under the device list.
  `a_card_offers_the_profile_it_plays_through_among_those_it_could_switch_to` holds a real daemon's card
  to it. `route.hw-volume` needed no new subscription: it is a string pair inside
  `SPA_PARAM_ROUTE_info`, a `Value::Struct` of a count followed by alternating keys and values, which
  `parse_route` was handed and threw away. `HardwareVolume` is the reading — `Yes`, `No`, or `Unsaid`
  where the route says nothing, a different answer from `No` never drawn as one, a silent driver not
  claiming the volume is software's. `resonate sinks` gives the profile a column (*PROFILE*) and appends
  the volume to the port cell, with the route's level where it says; the settings pane chains both onto
  `advertised`, so a device line says what it is switched to and where its volume is applied.
- **A device turning its own volume can be handed the slider, and the stream then stays bit-perfect at
  any volume.** `device-volume`, off by default, is `EngineConfig::device_volume`,
  `Command::SetDeviceVolume`, `OutputSettings::device_volume` and the Output category's *Hardware
  volume* group. `Attenuator::of` weighs it against `SinkInfo::turns_its_own_volume` — the route saying
  `route.hw-volume` is `Yes`, never `Unsaid` — and `Attenuator::leaves` is the volume the stream still
  applies: `Volume::MAX` where the device takes it, so `gain_config` and `gain_of` see full volume and
  the plan keeps no gain stage for it, while a ReplayGain adjustment stays the stream's own.
  `Output::attenuator` is decided at open and weighed again in `retune`, so the switch reshapes the chain
  in place like any gain change. What the device is turned to is the route: `parse_route` reads its
  `index`, each channel's volume and `props` — `SinkPort::volume` is the loudest channel whether or not
  the route is muted, `SinkPort::muted` the mute apart from it — and `PipeWire::set_device_volume`
  (`route_change` with `RouteSetting::Volume`) sets `Route` on the `Device` with its loudest channel at
  `Volume::to_gain` and every other scaled by the same factor, `save`d and saying nothing of the mute,
  what pipewire-pulse writes for a desktop slider; setting the node's `Props` would be the adapter's
  software volume. **A turn keeps the device's balance and its mute.** Writing one gain into every
  channel flattened a balance set in the desktop's mixer, and writing `mute = false` with it unmuted a
  device muted there the moment the slider moved; a route whose channels all read nothing is turned
  level, there being no balance left to keep. The mute is set apart: `Command::SetDeviceMute` reaches
  `PipeWire::set_device_mute` (`RouteSetting::Mute`), which says the mute and nothing of the volume,
  and does nothing where the stream turns the volume. `OutputStatus::device_turned` says the device
  has the slider, and there the window's mute mark mutes and unmutes the device rather than sending
  the slider to nothing and back, which would have flattened the balance through a zero. **The slider starts where the device already is** — a bind, or the switch turned on, takes
  `Volume::heard_at` of the route's reading into `EngineConfig::volume` rather than pushing the stored
  volume at it, so handing the slider over cannot jump headphones to full — and it follows a device
  turned from the desktop: a subscribed `Route` is not re-sent when its volume moves, so the device's
  `info` event saying `PARAMS` changed enumerates `Route` again, `DevicePorts::keep` answers the seats of
  a current route whose volume moved, `SinkChange::Turned` names their sinks, and the following survey
  ends in `follow_the_devices_volume` (`take_the_devices_volume`). What the engine sent comes back the
  same way, so `Engine::turned` remembers the last `TURNS_REMEMBERED` gains and a reading within
  `ONE_LEVEL_WITHIN` of any is an echo, which keeps a drag from being pulled back to a moment ago. **A
  device muted from the desktop is kept apart from its level.** A mute was read as a volume of nothing,
  so the slider dropped to 0 % and the window's mute mark, remembering its level only when it did the
  muting, had nothing to return to. `OutputStatus::device_muted` is `Attenuator::hears_the_mute_of` the
  bound sink, weighed at open, whenever the attenuator is weighed again and on every survey;
  `DevicePorts::keep` counts a mute that moved as a turn, so muting at an unchanged level is still read.
  The slider stays at the level, the window lights its mute mark and says *muted*, and pressing it sends
  `SetDeviceMute(false)` (`a_device_muted_from_elsewhere_keeps_the_slider_where_it_was`,
  `a_stream_turning_its_own_volume_leaves_the_devices_mute_alone`,
  `a_turned_route_keeps_its_balance_leaves_its_mute_and_asks_to_be_remembered`). Turning the switch off leaves the
  device where it was and puts the gain stage back on top, quieter rather than louder.
  `a_device_that_turns_its_own_volume_is_turned_and_the_stream_stays_bit_perfect`,
  `a_device_turned_from_elsewhere_moves_the_slider_and_its_own_echo_does_not` and
  `handing_the_volume_back_to_the_stream_puts_the_gain_stage_back` are the claims; everything else
  playing through the device is turned with it, as the group's hint says.
- **A stream's reported latency is counted in its own frames, converted where the graph's tick rate is
  known.** `pw_time.delay` is the delay to the device in `pw_time.rate` — the graph's clock, not the
  stream's — so a graph at 48 kHz under a 44.1 kHz stream reports ticks 8.8 % short of frames, in the
  position readout and the track-boundary fallback alike, precisely the converted path this player
  exists to make visible. The `process` callback holds the only consistent snapshot of delay and rate
  together, so it builds a `GraphTime` and publishes `downstream(spec.rate)` as one `AtomicU64` of stream
  frames; storing the pair would mean two atomics and a torn read, converting later a lock the RT thread
  may not take. `SinkStream::latency` is therefore `Frames`, not a `delay` in an unnamed domain. What the
  stream still holds is counted beside it: `GraphTime::buffered` is `pw_time.buffered`, the resampler's
  own frames, already at the stream's rate and so added, not converted. Still missing is
  `pw_time.queued`, the sum of `pw_buffer.size` over queued buffers: pipewire-rs offers no setter for
  that field and writing it would take `unsafe`.
- **The callback fills the quantum the graph asked for, not the buffer it was handed.**
  `pw_buffer.requested` is that quantum in frames and `Cycle::asked_for` turns it into bytes, clamped to
  the room the pool gave and falling back to all of it where the graph names nothing (a quantum of 0) —
  also where the packed DoP path lands whenever the padded words it fills from would not fit a buffer
  sized for the three-byte wire form. On this machine the graph asks for 1024 frames and the pool hands
  over 12288, so the ring was drained twelve quanta at a time and an underrun declared against twelve
  quanta of want. Both now follow the graph.
- **Silence about a capability is not a refusal.** A node answering `EnumFormat` with no channels
  property, or no formats at all, is believed about what it said and left alone about the rest: the
  candidate list falls back to the source's own layout, or `allowed_rates` crossed with the source's
  format. `SinkInfo::supports` stays strict about the rest, answering another question — whether the
  device advertised this exact format at this exact rate — which the DoP path, `resonate explain` and
  the settings pane read; a format entry naming no channels takes any layout there too
  (`SinkFormats::takes`), as the candidate list reads it, or a sink saying nothing of its channels was
  never offered DoP (`a_format_naming_no_channels_supports_every_layout_the_chooser_would_offer`).
- **A sink's advertised formats and the graph's `allowed_rates` are separate fields.** A device
  advertising 192 kHz is irrelevant if the daemon will not switch the graph to it; conflating them
  would make the bit-perfect claim unfalsifiable.
- **Twenty-four bits are one depth and two words, and which word goes on the wire is the graph
  boundary's alone.** `SampleFormat::S24` is sign-extended in the low 24 bits of an `i32`, so four bytes
  everywhere the workspace touches it — ring, chain, `bytes_per_frame`, staging buffer — while a device
  may want three. `format::WireWord` is that difference, in `resonate-pipewire`: `sample_format` reads
  `S24LE` and `S24_32LE` as one depth, `one_of_each` keeps the depth once so a device offering both is
  one row, and `spa_format` takes the word as a second argument. The row still says which words were
  named: `SinkFormats::words` is a `Words` — `Whole` for every depth filling its word, and `Packed`,
  `Padded` or `PackedAndPadded` for twenty-four bits — so `resonate sinks` spells the row `S24LE`,
  `S24_32LE` or both, and `resonate explain` prints, under a 24-bit output, the words the stream offers
  beside the device's. `build_stream` offers both as two `EnumFormat` params, the packed one first, a
  24-bit DAC's own default usually being the packed word and a conversion avoided being the point of the
  bit-perfect path; a peer refusing it settles on the second. What the graph chose comes back through
  `param_changed`, stored on an `AtomicBool` the callback reads (the reported latency's shape,
  `process.rs` holding no lock). The same callback sends `StreamEvent::FormatChanged` with the spec and
  the `Words` settled on, and the engine keeps the latter on `OutputStatus::words` — `None` until the
  graph says — which the inspector draws as *on the wire* wherever the depth has two words
  (`the_word_the_graph_settled_on_is_published_once_it_says`).
- **Packing is done in the graph's own buffer, forward, allocating nothing.** `process::pack` reads the
  low three bytes of the word at `4i` and writes them at `3i`, and `3i + 3 <= 4i + 3` for every `i`, so
  the write never reaches an unread word and no scratch is needed — keeping it inside the callback's
  no-allocation rule. The ring still fills the buffer at the padded stride and `Silence::write` still
  lays DoP markers as four-byte words; packing keeps bytes 0–2 and the marker is byte 2, so a DoP carrier
  survives. `chunk.stride` and `chunk.size` are then the *wire* frame and packed length, not the ring's.
  Without it a 24-bit master reaching a device advertising `S24LE` and `S16LE` alone was dithered to 16
  bits, the enumeration having dropped the only 24-bit word the device named.

## Fixtures

- **A test asserting which row plays pauses the transport first.** The fake graph is pulled by the test
  thread alone, so a queue cannot advance while nobody pulls — but that is the harness's property, not
  the assertion's, and a decode failing on a loaded machine reaches `Engine::fail`, which skips.
  `stand_still` is the pause, and `the_queue_the_engine_publishes_is_the_order_it_will_play` and
  `editing_the_queue_leaves_the_playing_track_alone_until_its_own_row_goes` take it before reading a row
  and again after any command starting the transport — `JumpTo` is one, and `Play` is what the second
  uses to prove a bind, removing the playing row while paused now deferring it.
- **The real-file fixtures are built by ffmpeg at test time, and the suite skips without it.**
  `crates/resonate-codec/tests/encoded.rs` reaches 24-bit 96 kHz through FLAC and ALAC, 24-bit 5.1
  through FLAC, 48 kHz stereo through AAC, Vorbis and Vorbis-in-Matroska, 22.05 kHz mono through MP3,
  and 24-bit 96 kHz through AIFF and CAF. It also exercises `probe_stream`'s container walk and the
  ISO-BMFF box order.
- **Two fixtures go through the encoder a ripper uses, not ffmpeg.** An ffmpeg tone carries none of a
  real file's furniture, so `flac` and `metaflac` build a rip with `SEEKTABLE`, `VORBIS_COMMENT`,
  `PICTURE` and `PADDING` blocks — the block list asserted, so the day the reference encoder stops
  writing one the suite says so — and `lame` builds an MP3 with an ID3v2 tag to skip and a Xing/LAME
  header whose priming reaches `encoder_delay`. Each gates on its own tool and prints a skip without it,
  as the `ffmpeg()` gate does.
- **One fixture is written by the suite, ffmpeg refusing to write it.** ffmpeg's CAF muxer refuses AAC,
  so `a_caf_rip_drops_the_priming_and_the_remainder_its_packet_table_declares` takes the ADTS stream
  ffmpeg *will* write, reads its frame lengths off the headers and lays the raw packets into `desc`,
  `pakt` and `data` chunks itself, as `afconvert` would. The `pakt` chunk is the point: its
  `priming_frames` and `remainder_frames` are what the CAF reader turns into `Track::delay` and
  `Track::padding` while trimming no packet. No `kuki` chunk is written or needed — symphonia's AAC
  decoder falls back to the rate and channel count the `desc` chunk carries.
- **`RESONATE_REAL_FIXTURES` points the suite at a folder of real files.** A CD rip and an iTunes
  purchase cannot be committed, so the honest way to have them in the loop is to read the ones the
  tester owns: set it and every file under it must probe, name a container and codec this build knows,
  report a duration and decode its first block. Unset, it prints a skip like every other gate.
- **The transport and bus tests drive a real `Player` thread and poll for the state they expect**, so a
  state that never arrives costs the full patience before failing. The failure names the transport it
  watched (`transport(player)`) and, in the transport tests, what the graph had pulled.
